//! Endgame tablebase probing (Syzygy WDL).
//!
//! With five men or fewer these files hold the exact result of every legal
//! position: win, draw or loss under perfect play. No search reaches that and
//! no evaluation approximates it -- a position our tables score at +0.4 may be
//! a dead draw, and in an endgame that difference is the whole game.
//!
//! Scope, honestly stated. Of the 62 positions in our blunder suite -- real
//! mistakes from real games -- exactly ZERO have five men or fewer. Seven have
//! six to ten. So this is not the fix for our endgame weakness, which lives at
//! 11 to 16 pieces and is a question of what the pieces are worth, not of
//! perfect knowledge. It is worth having anyway: a six-man position becomes a
//! five-man position after one capture, and a search that knows the truth one
//! ply ahead evaluates the capture correctly.
//!
//! The format is not an array of results. Positions are indexed canonically
//! using the symmetries of the board, and values are stored compressed with
//! recursive pairing over a Huffman code -- each symbol expands to two other
//! symbols, recursively, down to literals. Probing means: map the position to
//! its canonical index, find the block holding it, walk the Huffman code to a
//! symbol, then walk the pairing tree down to the literal.
//!
//! Written from the public format description. The compression and indexing
//! scheme is the file format itself, not any engine's code.

use crate::board::Board;
use crate::types::{Color, PieceType};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const WDL_MAGIC: u32 = 0x5d23_e871;

/// Result of a probe, from the side to move's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wdl {
    Loss,
    BlessedLoss,
    Draw,
    CursedWin,
    Win,
}

impl Wdl {
    /// Centipawn score. Far above any evaluation term so a known result
    /// outranks every heuristic judgement, and far below a mate score so it is
    /// never mistaken for a mate the search actually proved.
    pub fn to_score(self) -> i32 {
        match self {
            Wdl::Loss => -20_000,
            Wdl::Win => 20_000,
            // Cursed and blessed results are wins and losses that the
            // fifty-move rule turns into draws. Under normal rules they are
            // draws, and calling them anything else would have the search
            // chase a win that cannot be collected.
            _ => 0,
        }
    }

    fn from_raw(v: u8) -> Wdl {
        match v {
            0 => Wdl::Loss,
            1 => Wdl::BlessedLoss,
            2 => Wdl::Draw,
            3 => Wdl::CursedWin,
            _ => Wdl::Win,
        }
    }
}

/// The compressed-block structure for one table.
///
/// `offset`, `base` and `sym_len` together are the Huffman decoder: `base[l]`
/// is the smallest code of length `l`, `offset[l]` the first symbol id of that
/// length, and `sym_len[s]` how many literals symbol `s` expands to. That last
/// one is what makes seeking possible without decompressing everything: the
/// walk can skip a whole symbol by subtracting its length.
struct PairsData {
    idx_bits: u8,
    block_size: u8,
    min_len: u8,
    /// Constant-value table: some tables hold a single value everywhere.
    const_value: Option<u8>,
    base: Vec<u64>,
    offset: Vec<u16>,
    sym_len: Vec<u8>,
    sym_pat: Vec<u8>,
    index_table: Vec<u8>,
    size_table: Vec<u16>,
    data: Vec<u8>,
}

impl PairsData {
    /// Expand symbol `s` down the pairing tree to the literal at `rem`.
    fn expand(&self, mut s: u32, mut rem: u32) -> u8 {
        loop {
            let w = &self.sym_pat[3 * s as usize..3 * s as usize + 3];
            let s2 = ((w[2] as u32) << 4) | ((w[1] as u32) >> 4);
            if s2 == 0x0fff {
                // A literal: the symbol expands to itself.
                return w[0];
            }
            let s1 = (((w[1] & 0x0f) as u32) << 8) | w[0] as u32;
            let left = self.sym_len[s1 as usize] as u32 + 1;
            if rem < left {
                s = s1;
            } else {
                rem -= left;
                s = s2;
            }
        }
    }

    /// Decompress the value at canonical index `idx`.
    fn value_at(&self, idx: u64) -> Option<u8> {
        if let Some(v) = self.const_value {
            return Some(v);
        }
        let main_idx = (idx >> self.idx_bits) as usize;
        let mut lit_idx = (idx & ((1u64 << self.idx_bits) - 1)) as i64
            - (1i64 << (self.idx_bits - 1));

        let e = 6 * main_idx;
        if e + 6 > self.index_table.len() {
            return None;
        }
        let mut block = u32::from_le_bytes([
            self.index_table[e],
            self.index_table[e + 1],
            self.index_table[e + 2],
            self.index_table[e + 3],
        ]) as usize;
        lit_idx += u16::from_le_bytes([self.index_table[e + 4], self.index_table[e + 5]]) as i64;

        // The index points near the right block, not exactly at it: walk
        // forwards or backwards over whole blocks until the literal falls
        // inside one.
        while lit_idx < 0 {
            block = block.checked_sub(1)?;
            lit_idx += *self.size_table.get(block)? as i64 + 1;
        }
        while lit_idx > *self.size_table.get(block)? as i64 {
            lit_idx -= *self.size_table.get(block)? as i64 + 1;
            block += 1;
        }

        let start = block << self.block_size;
        if start + 8 > self.data.len() {
            return None;
        }
        let mut ptr = start;
        let mut code = u64::from_be_bytes(self.data[ptr..ptr + 8].try_into().ok()?);
        ptr += 8;
        let mut bit_cnt: u32 = 0;
        let m = self.min_len as usize;

        loop {
            let mut l = m;
            while l < self.base.len() && code < self.base[l] {
                l += 1;
            }
            if l >= self.base.len() || l >= self.offset.len() {
                return None;
            }
            let sym = self.offset[l] as u32 + ((code - self.base[l]) >> (64 - l)) as u32;
            let len = *self.sym_len.get(sym as usize)? as i64 + 1;
            if lit_idx < len {
                return Some(self.expand(sym, lit_idx as u32));
            }
            lit_idx -= len;
            code <<= l;
            bit_cnt += l as u32;
            if bit_cnt >= 32 {
                bit_cnt -= 32;
                if ptr + 4 > self.data.len() {
                    return None;
                }
                let tmp = u32::from_be_bytes(self.data[ptr..ptr + 4].try_into().ok()?);
                ptr += 4;
                code |= (tmp as u64) << bit_cnt;
            }
        }
    }
}

/// Recursively compute how many literals each symbol expands to.
fn calc_sym_len(sym_pat: &[u8], s: u32, len: &mut Vec<u8>, done: &mut Vec<bool>) {
    if done[s as usize] {
        return;
    }
    let w = &sym_pat[3 * s as usize..3 * s as usize + 3];
    let s2 = ((w[2] as u32) << 4) | ((w[1] as u32) >> 4);
    if s2 == 0x0fff {
        len[s as usize] = 0;
    } else {
        let s1 = (((w[1] & 0x0f) as u32) << 8) | w[0] as u32;
        calc_sym_len(sym_pat, s1, len, done);
        calc_sym_len(sym_pat, s2, len, done);
        len[s as usize] = len[s1 as usize]
            .saturating_add(len[s2 as usize])
            .saturating_add(1);
    }
    done[s as usize] = true;
}

/// Parse one PairsData header out of `data` at `pos`, returning it and the
/// three block-section sizes that follow the headers.
fn setup_pairs(data: &[u8], pos: &mut usize, tb_size: u64) -> Option<(PairsData, [usize; 3])> {
    let flags = *data.get(*pos)?;
    if flags & 0x80 != 0 {
        let v = *data.get(*pos + 1)?;
        *pos += 2;
        return Some((
            PairsData {
                idx_bits: 0,
                block_size: 0,
                min_len: 0,
                const_value: Some(v),
                base: Vec::new(),
                offset: Vec::new(),
                sym_len: Vec::new(),
                sym_pat: Vec::new(),
                index_table: Vec::new(),
                size_table: Vec::new(),
                data: Vec::new(),
            },
            [0, 0, 0],
        ));
    }

    let block_size = *data.get(*pos + 1)?;
    let idx_bits = *data.get(*pos + 2)?;
    let real_num_blocks = u32::from_le_bytes(data.get(*pos + 4..*pos + 8)?.try_into().ok()?);
    let num_blocks = real_num_blocks as usize + *data.get(*pos + 3)? as usize;
    let max_len = *data.get(*pos + 8)? as usize;
    let min_len = *data.get(*pos + 9)? as usize;
    let h = max_len - min_len + 1;

    let mut offset = Vec::with_capacity(h);
    for i in 0..h {
        let o = *pos + 10 + 2 * i;
        offset.push(u16::from_le_bytes(data.get(o..o + 2)?.try_into().ok()?));
    }
    let num_syms =
        u16::from_le_bytes(data.get(*pos + 10 + 2 * h..*pos + 12 + 2 * h)?.try_into().ok()?) as usize;
    let pat_start = *pos + 12 + 2 * h;
    let sym_pat = data.get(pat_start..pat_start + 3 * num_syms)?.to_vec();

    let mut sym_len = vec![0u8; num_syms];
    let mut done = vec![false; num_syms];
    for s in 0..num_syms {
        calc_sym_len(&sym_pat, s as u32, &mut sym_len, &mut done);
    }

    // base[l] is the smallest code of length l, derived backwards from the
    // longest. Shifted left so a code can be compared against it directly at
    // full width.
    let mut base = vec![0u64; h];
    for i in (0..h.saturating_sub(1)).rev() {
        base[i] = (base[i + 1] + offset[i] as u64 - offset[i + 1] as u64) / 2;
    }
    for (i, b) in base.iter_mut().enumerate() {
        *b <<= 64 - (min_len + i) as u32;
    }
    // Indexed by code length, so pad the front: lengths below min_len are
    // never used but the lookup indexes by l directly.
    let mut base_padded = vec![u64::MAX; min_len];
    base_padded.extend_from_slice(&base);
    let mut offset_padded = vec![0u16; min_len];
    offset_padded.extend_from_slice(&offset);

    *pos = pat_start + 3 * num_syms + (num_syms & 1);

    let num_indices = ((tb_size + (1u64 << idx_bits) - 1) >> idx_bits) as usize;
    let sizes = [6 * num_indices, 2 * num_blocks, (real_num_blocks as usize) << block_size];

    Some((
        PairsData {
            idx_bits,
            block_size,
            min_len: min_len as u8,
            const_value: None,
            base: base_padded,
            offset: offset_padded,
            sym_len,
            sym_pat,
            index_table: Vec::new(),
            size_table: Vec::new(),
            data: Vec::new(),
        },
        sizes,
    ))
}

struct Tables {
    dir: PathBuf,
    available: HashMap<String, ()>,
    max_men: usize,
    cache: Mutex<HashMap<String, bool>>,
}

static TABLES: OnceLock<Option<Tables>> = OnceLock::new();

/// Point the prober at a directory of `.rtbw` files. Returns whether any were
/// found. Safe to call repeatedly; only the first call does anything.
pub fn init(dir: &str) -> bool {
    TABLES
        .get_or_init(|| {
            let entries = fs::read_dir(Path::new(dir)).ok()?;
            let mut available = HashMap::new();
            let mut max_men = 0usize;
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if let Some(stem) = name.strip_suffix(".rtbw") {
                    max_men = max_men.max(stem.chars().filter(|c| c.is_ascii_uppercase()).count());
                    available.insert(stem.to_string(), ());
                }
            }
            if available.is_empty() {
                return None;
            }
            Some(Tables {
                dir: Path::new(dir).to_path_buf(),
                available,
                max_men,
                cache: Mutex::new(HashMap::new()),
            })
        })
        .is_some()
}

/// Largest man-count covered by the loaded tables, 0 if none.
pub fn max_men() -> usize {
    TABLES.get().and_then(|t| t.as_ref()).map_or(0, |t| t.max_men)
}

/// Material signature in Syzygy's spelling: stronger side first, pieces in
/// descending value, e.g. "KQPvKR".
fn material_key(board: &Board) -> (String, bool) {
    let order = [
        (PieceType::Queen, 'Q'),
        (PieceType::Rook, 'R'),
        (PieceType::Bishop, 'B'),
        (PieceType::Knight, 'N'),
        (PieceType::Pawn, 'P'),
    ];
    let side = |c: Color| {
        let mut s = String::from("K");
        for (pt, ch) in order {
            for _ in 0..board.pieces[c.idx()][pt.idx()].count_ones() {
                s.push(ch);
            }
        }
        s
    };
    let w = side(Color::White);
    let b = side(Color::Black);
    if (w.len(), &w) >= (b.len(), &b) {
        (format!("{}v{}", w, b), false)
    } else {
        (format!("{}v{}", b, w), true)
    }
}

/// Probe the WDL tables.
///
/// Returns `None` whenever the answer is not certain -- tables absent, too many
/// men, castling still available (the tables assume none), or the canonical
/// index not yet implemented for this table's shape. That last one is the
/// current limit and it is deliberate: a tablebase that is occasionally wrong
/// is worse than no tablebase at all, because the search trusts it absolutely
/// and stops looking.
pub fn probe_wdl(board: &Board) -> Option<Wdl> {
    let tables = TABLES.get()?.as_ref()?;
    if board.occ_all.count_ones() as usize > tables.max_men {
        return None;
    }
    if board.castling != 0 {
        return None;
    }
    let (key, _flipped) = material_key(board);
    if !tables.available.contains_key(&key) {
        return None;
    }
    // Reading and decompressing a table is implemented above (PairsData /
    // setup_pairs / value_at). What is not yet implemented is the canonical
    // index: mapping a position to its slot requires the board symmetries
    // (horizontal, vertical and diagonal folding) plus binomial ranking of
    // like pieces, and for pawn tables a separate file/rank encoding.
    //
    // Until that exists this returns "unknown" rather than a value it cannot
    // justify. See probe_wdl's contract above for why that is the only
    // acceptable half-built state.
    let _ = &tables.cache;
    let _ = Wdl::from_raw;
    let _ = |d: &PairsData, i: u64| d.value_at(i);
    let _ = |data: &[u8], p: &mut usize, s: u64| setup_pairs(data, p, s);
    let _ = WDL_MAGIC;
    None
}
