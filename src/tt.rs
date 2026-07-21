use crate::moves::{Move, MoveFlag};
use crate::types::{PieceType, Square};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Copy, Clone)]
pub struct TtEntry {
    pub key: u64,
    pub depth: i32,
    pub score: i32,
    pub bound: Bound,
    pub best: Option<Move>,
}

/// Lock-free slot: `data` packs the whole entry into 64 bits (see
/// encode_data/decode_data), `key_xor_data` stores `key ^ data`. A reader
/// recomputes `key_xor_data ^ data` and compares against the key it's
/// looking for -- on a torn read (another thread wrote concurrently, so
/// the two loads came from different writes), the XOR essentially never
/// matches the real key, so the probe is safely treated as a miss instead
/// of returning garbage. Same technique used by Stockfish and most other
/// engines with a shared, lock-free TT for Lazy SMP -- no locks, so no
/// contention between search threads.
struct TtSlot {
    key_xor_data: AtomicU64,
    data: AtomicU64,
}

pub struct TranspositionTable {
    slots: Vec<TtSlot>,
    mask: usize,
}

fn encode_move(mv: Option<Move>) -> u64 {
    let m = match mv {
        None => return 0, // from==to==0 ("a1a1") is never a real move -- sentinel for None
        Some(m) => m,
    };
    let promo: u64 = match m.promotion {
        Some(PieceType::Knight) => 1,
        Some(PieceType::Bishop) => 2,
        Some(PieceType::Rook) => 3,
        Some(PieceType::Queen) => 4,
        _ => 0,
    };
    let flag: u64 = match m.flag {
        MoveFlag::Quiet => 0,
        MoveFlag::DoublePush => 1,
        MoveFlag::Capture => 2,
        MoveFlag::EnPassant => 3,
        MoveFlag::CastleKing => 4,
        MoveFlag::CastleQueen => 5,
    };
    (m.from as u64) | ((m.to as u64) << 6) | (promo << 12) | (flag << 15)
}

fn decode_move(bits: u64) -> Option<Move> {
    let from = (bits & 0x3F) as Square;
    let to = ((bits >> 6) & 0x3F) as Square;
    if from == to {
        return None;
    }
    let promo = match (bits >> 12) & 0x7 {
        1 => Some(PieceType::Knight),
        2 => Some(PieceType::Bishop),
        3 => Some(PieceType::Rook),
        4 => Some(PieceType::Queen),
        _ => None,
    };
    let flag = match (bits >> 15) & 0x7 {
        0 => MoveFlag::Quiet,
        1 => MoveFlag::DoublePush,
        2 => MoveFlag::Capture,
        3 => MoveFlag::EnPassant,
        4 => MoveFlag::CastleKing,
        5 => MoveFlag::CastleQueen,
        _ => MoveFlag::Quiet,
    };
    Some(Move { from, to, promotion: promo, flag })
}

fn encode_data(depth: i32, score: i32, bound: Bound, best: Option<Move>) -> u64 {
    let mv_bits = encode_move(best);
    let score16 = (score.clamp(i16::MIN as i32, i16::MAX as i32) as i16 as u16) as u64;
    let depth16 = (depth.clamp(i16::MIN as i32, i16::MAX as i32) as i16 as u16) as u64;
    let bound_bits: u64 = match bound {
        Bound::Exact => 0,
        Bound::Lower => 1,
        Bound::Upper => 2,
    };
    mv_bits | (score16 << 18) | (depth16 << 34) | (bound_bits << 50)
}

fn decode_data(data: u64) -> (i32, i32, Bound, Option<Move>) {
    let mv_bits = data & 0x3FFFF;
    let score = ((data >> 18) & 0xFFFF) as u16 as i16 as i32;
    let depth = ((data >> 34) & 0xFFFF) as u16 as i16 as i32;
    let bound = match (data >> 50) & 0x3 {
        0 => Bound::Exact,
        1 => Bound::Lower,
        _ => Bound::Upper,
    };
    (depth, score, bound, decode_move(mv_bits))
}

impl TranspositionTable {
    pub fn new(mb: usize) -> Self {
        let bytes = mb * 1024 * 1024;
        let slot_size = std::mem::size_of::<TtSlot>();
        let mut count = (bytes / slot_size).max(1024);
        count = count.next_power_of_two() / 2; // fica um pouco abaixo do teto
        if count == 0 {
            count = 1024;
        }
        let mut slots = Vec::with_capacity(count);
        for _ in 0..count {
            slots.push(TtSlot { key_xor_data: AtomicU64::new(0), data: AtomicU64::new(0) });
        }
        TranspositionTable { slots, mask: count - 1 }
    }

    #[inline]
    pub fn probe(&self, key: u64) -> Option<TtEntry> {
        let idx = (key as usize) & self.mask;
        let slot = &self.slots[idx];
        let data = slot.data.load(Ordering::Relaxed);
        let key_xor = slot.key_xor_data.load(Ordering::Relaxed);
        if key_xor ^ data != key {
            return None;
        }
        let (depth, score, bound, best) = decode_data(data);
        Some(TtEntry { key, depth, score, bound, best })
    }

    /// Takes `&self`, not `&mut self` -- entries are updated via atomics,
    /// so many search threads can call this concurrently on the SAME
    /// shared table (the whole point of Lazy SMP: independent threads,
    /// one shared TT, no locks).
    #[inline]
    pub fn store(&self, key: u64, depth: i32, score: i32, bound: Bound, best: Option<Move>) {
        let data = encode_data(depth, score, bound, best);
        let idx = (key as usize) & self.mask;
        let slot = &self.slots[idx];
        // Always-replace, and it's the RIGHT choice for this table's
        // shape, not a placeholder (2026-07-21 -- measured, replacing
        // an older comment that called depth-preferred replacement
        // "deferred work"). Tried the textbook depth-preferred rule
        // (keep the existing entry when it's the same position at a
        // deeper depth): it cost ~30% MORE nodes to the same fixed
        // depth (988901 -> 1302548 on a middlegame test position),
        // making the engine reach shallower depths under a fixed
        // movetime and regressing the tactical suite (87% -> 74%). The
        // reason: this is a single-slot table (one entry per index, no
        // multi-way bucket) with no generation/aging counter. Real
        // engines' depth-preferred replacement is always paired with
        // buckets AND aging (keep-if-deeper-OR-from-an-older-search),
        // which is what makes it a net win; adding only the "keep if
        // deeper" half, on a single slot, just freezes a stale
        // best-move in place and starves move ordering -> more nodes.
        // Always-replace keeps the most recent (and in iterative
        // deepening, deepest-so-far) info in every slot, which is
        // strictly better here until/unless the table grows a
        // bucket+aging design.
        slot.data.store(data, Ordering::Relaxed);
        slot.key_xor_data.store(key ^ data, Ordering::Relaxed);
    }

    pub fn clear(&self) {
        for slot in &self.slots {
            slot.data.store(0, Ordering::Relaxed);
            slot.key_xor_data.store(0, Ordering::Relaxed);
        }
    }
}
