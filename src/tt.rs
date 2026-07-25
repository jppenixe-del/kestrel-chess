use crate::moves::{Move, MoveFlag};
use crate::types::{PieceType, Square};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

/// Sentinel for "no cached static eval in this entry" -- e.g. entries
/// stored while in check, where static eval is meaningless and never
/// computed. i16::MIN is never a real eval (evals are always clamped
/// to fit comfortably inside i16 well above this), so it's safe as a
/// dedicated "absent" marker.
pub const TT_EVAL_NONE: i16 = i16::MIN;

#[derive(Copy, Clone)]
pub struct TtEntry {
    pub key: u64,
    pub depth: i32,
    pub score: i32,
    pub bound: Bound,
    pub best: Option<Move>,
    /// TTPV: was this entry written by a search at a PV node (full
    /// alpha-beta window), as opposed to a null/scout window? A later
    /// probe that only reaches this position via a scout search still
    /// knows it was "important" once -- used to reduce LESS via LMR at
    /// that position, since a genuinely quiet position wouldn't have
    /// earned full-window treatment before.
    pub pv: bool,
    /// Cached static eval (the full `evaluate(board)`, uncorrected) at
    /// the position this entry was written for. TT_EVAL_NONE if none
    /// was available (in check). Reused by the search on a later probe
    /// of the same position to skip recomputing the full eval.
    pub static_eval: i16,
}

/// Lock-free slot: `data` packs the whole entry into 64 bits (see
/// encode_data/decode_data), `key_xor_data` stores `key ^ data`. A reader
/// recomputes `key_xor_data ^ data` and compares against the key it's
/// looking for -- on a torn read (another thread wrote concurrently, so
/// the two loads came from different writes), the XOR essentially never
/// matches the real key, so the probe is safely treated as a miss instead
/// of returning garbage. This is what lets the table be shared and
/// lock-free under Lazy SMP -- no locks, so no contention between search
/// threads.
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

// Packing of `data` into 64 bits (see TtSlot doc comment above for the
// lock-free XOR-trick this feeds into):
//   bits  0..18  mv_bits       (18 bits -- encode_move)
//   bits 18..34  score16       (16 bits, signed)
//   bits 34..42  depth8        ( 8 bits, signed -- search depth never
//                                gets remotely close to the i8 range;
//                                see MAX_PLY/negamax's ply>=MAX_PLY-1
//                                guard, which hard-stops recursion at
//                                127. A pathological `go depth 200`+
//                                request just clamps harmlessly here,
//                                same as score16 already did/does.)
//   bits 42..44  bound_bits    ( 2 bits)
//   bit  44      pv_bit        ( 1 bit)
//   bits 45..61  static_eval16 (16 bits, signed -- cached full eval)
//   bits 61..64  unused        ( 3 bits free)
//
// TT structure note. This is a single-slot, always-replace table. A 4-way
// set-associative cluster with generation aging was implemented and measured
// against it across 6 positions, and regressed node-count-to-fixed-depth by
// 35-45%, which is why the simple design stayed.
//
// That comparison is NOT settled, for two reasons found on 2026-07-25, and
// both cut in favour of re-running it rather than trusting it:
//   1. It was scored on node-count-to-fixed-depth. This project has since
//      learned not to trust that metric for tuning decisions (it proved
//      chaotic and non-monotonic when used on the SEE margins). What decides
//      a replacement policy is strength at fixed TIME, over real games.
//   2. It ran at what were believed to be 16MB and 64MB tables -- but the
//      sizing bug fixed below meant those were really 8MB and 32MB. Table
//      size is exactly the variable that governs how much replacement policy
//      matters: the smaller the table, the more eviction hurts. The buckets
//      were judged in the conditions least favourable to them.
// So: re-measure at true sizes, over games, before concluding either way.
// Nothing here is settled by authority -- only by measurement on our tree.
fn encode_data(depth: i32, score: i32, bound: Bound, best: Option<Move>, pv: bool, static_eval: i16) -> u64 {
    let mv_bits = encode_move(best);
    let score16 = (score.clamp(i16::MIN as i32, i16::MAX as i32) as i16 as u16) as u64;
    let depth8 = (depth.clamp(i8::MIN as i32, i8::MAX as i32) as i8 as u8) as u64;
    let bound_bits: u64 = match bound {
        Bound::Exact => 0,
        Bound::Lower => 1,
        Bound::Upper => 2,
    };
    let pv_bit: u64 = if pv { 1 } else { 0 };
    let eval16 = (static_eval as u16) as u64;
    mv_bits | (score16 << 18) | (depth8 << 34) | (bound_bits << 42) | (pv_bit << 44) | (eval16 << 45)
}

fn decode_data(data: u64) -> (i32, i32, Bound, Option<Move>, bool, i16) {
    let mv_bits = data & 0x3FFFF;
    let score = ((data >> 18) & 0xFFFF) as u16 as i16 as i32;
    let depth = ((data >> 34) & 0xFF) as u8 as i8 as i32;
    let bound = match (data >> 42) & 0x3 {
        0 => Bound::Exact,
        1 => Bound::Lower,
        _ => Bound::Upper,
    };
    let pv = (data >> 44) & 1 == 1;
    let static_eval = ((data >> 45) & 0xFFFF) as u16 as i16;
    (depth, score, bound, decode_move(mv_bits), pv, static_eval)
}

impl TranspositionTable {
    pub fn new(mb: usize) -> Self {
        let bytes = mb * 1024 * 1024;
        let slot_size = std::mem::size_of::<TtSlot>();
        // The index is masked, so the slot count must be a power of two, and
        // it must not exceed what was asked for. Round DOWN to the previous
        // power of two -- which, when the count already IS one, keeps it.
        //
        // 2026-07-25: this used to be `next_power_of_two() / 2`, meaning that
        // whenever the requested size divided into an exact power of two --
        // which is every realistic setting, since hash is always asked for in
        // powers of two -- the table was silently halved. `Hash=32` built a
        // 16MB table. Every measurement ever taken on this engine at a given
        // Hash value was really taken at half of it.
        let requested = (bytes / slot_size).max(1024);
        let count = 1usize << requested.ilog2();
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
        let (depth, score, bound, best, pv, static_eval) = decode_data(data);
        Some(TtEntry { key, depth, score, bound, best, pv, static_eval })
    }

    /// Takes `&self`, not `&mut self` -- entries are updated via atomics,
    /// so many search threads can call this concurrently on the SAME
    /// shared table (the whole point of Lazy SMP: independent threads,
    /// one shared TT, no locks).
    #[inline]
    pub fn store(&self, key: u64, depth: i32, score: i32, bound: Bound, best: Option<Move>, pv: bool, static_eval: i16) {
        let data = encode_data(depth, score, bound, best, pv, static_eval);
        let idx = (key as usize) & self.mask;
        let slot = &self.slots[idx];
        // Always-replace: keeps the most recent (and, under iterative
        // deepening, usually the deepest-so-far) information in every slot.
        // It measured better than a 4-way bucket here, but see the "TT
        // structure note" above encode_data for why that comparison is worth
        // re-running rather than treating as settled.
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
