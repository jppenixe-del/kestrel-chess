//! Real magic bitboards for sliding-piece attacks, replacing the naive
//! ray-cast scan that used to live in attacks.rs (profiled at ~17% of
//! total search time -- see the commit that adds this file). Standard,
//! well-known technique (Chess Programming Wiki "Magic Bitboards");
//! magic numbers are found here via random search at startup rather than
//! hardcoded, so there's nothing to transcribe/verify against an
//! external source -- the search is self-verifying (every candidate is
//! checked against the reference ray-cast implementation for every
//! occupancy subset before being accepted).
use crate::bitboard::*;
use crate::types::Square;

fn file_of_i(s: Square) -> i32 {
    (s % 8) as i32
}
fn rank_of_i(s: Square) -> i32 {
    (s / 8) as i32
}
fn sq_i(f: i32, r: i32) -> Square {
    (r * 8 + f) as Square
}

/// Ground-truth slow attack generator (same algorithm the old
/// attacks::ray_attacks used) -- only ever called during table
/// construction at startup, never from the search.
fn ray_attacks_slow(s: Square, occ: Bitboard, dirs: &[(i32, i32)]) -> Bitboard {
    let f0 = file_of_i(s);
    let r0 = rank_of_i(s);
    let mut out = 0u64;
    for &(df, dr) in dirs {
        let mut f = f0 + df;
        let mut r = r0 + dr;
        while (0..8).contains(&f) && (0..8).contains(&r) {
            let t = sq_i(f, r);
            out |= bb(t);
            if occ & bb(t) != 0 {
                break;
            }
            f += df;
            r += dr;
        }
    }
    out
}

const ROOK_DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const BISHOP_DIRS: [(i32, i32); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];

/// Relevant-occupancy mask: the ray in each direction, EXCLUDING the
/// final edge square (a blocker there can't hide anything further, so
/// it never changes the attack set and is dropped to shrink the table).
fn relevant_mask(s: Square, dirs: &[(i32, i32)]) -> Bitboard {
    let f0 = file_of_i(s);
    let r0 = rank_of_i(s);
    let mut out = 0u64;
    for &(df, dr) in dirs {
        let mut f = f0 + df;
        let mut r = r0 + dr;
        while (0..8).contains(&(f + df)) && (0..8).contains(&(r + dr)) {
            out |= bb(sq_i(f, r));
            f += df;
            r += dr;
        }
    }
    out
}

/// n-th subset of `mask` via the standard binary-counter-over-set-bits
/// trick (enumerates all 2^popcount(mask) occupancy subsets).
fn subset(index: usize, mask: Bitboard) -> Bitboard {
    let mut result = 0u64;
    let mut m = mask;
    let mut i = index;
    while m != 0 {
        let bit = m & m.wrapping_neg(); // lowest set bit
        m &= m - 1;
        if i & 1 != 0 {
            result |= bit;
        }
        i >>= 1;
    }
    result
}

struct Xorshift64(u64);
impl Xorshift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Sparse random candidate -- magics with few set bits are far more
    /// likely to hash well, standard trick.
    fn sparse(&mut self) -> u64 {
        self.next() & self.next() & self.next()
    }
}

struct SquareMagic {
    mask: Bitboard,
    magic: u64,
    shift: u32,
    table: Vec<Bitboard>,
}

impl SquareMagic {
    #[inline(always)]
    fn index(&self, occ: Bitboard) -> usize {
        (((occ & self.mask).wrapping_mul(self.magic)) >> self.shift) as usize
    }
    #[inline(always)]
    fn attacks(&self, occ: Bitboard) -> Bitboard {
        self.table[self.index(occ)]
    }
}

fn find_magic_for_square(s: Square, dirs: &[(i32, i32)], rng: &mut Xorshift64) -> SquareMagic {
    let mask = relevant_mask(s, dirs);
    let bits = mask.count_ones();
    let size = 1usize << bits;
    let shift = 64 - bits;

    // Precompute every (occupancy subset, real attack) pair once -- reused
    // across every magic candidate tried below.
    let mut occs = Vec::with_capacity(size);
    let mut atts = Vec::with_capacity(size);
    for i in 0..size {
        let occ = subset(i, mask);
        occs.push(occ);
        atts.push(ray_attacks_slow(s, occ, dirs));
    }

    loop {
        let magic = rng.sparse();
        let mut table: Vec<Option<Bitboard>> = vec![None; size];
        let mut ok = true;
        for i in 0..size {
            let idx = ((occs[i].wrapping_mul(magic)) >> shift) as usize;
            match table[idx] {
                None => table[idx] = Some(atts[i]),
                Some(existing) if existing == atts[i] => {} // constructive collision, fine
                Some(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        let final_table: Vec<Bitboard> = table.into_iter().map(|o| o.unwrap_or(0)).collect();
        return SquareMagic { mask, magic, shift, table: final_table };
    }
}

pub struct Magics {
    rook: Vec<SquareMagic>,
    bishop: Vec<SquareMagic>,
}

impl Magics {
    pub fn new() -> Self {
        // Fixed seed: deterministic startup (same magics every run, no
        // need to persist them), and irrelevant to search quality (only
        // affects which valid magic is found, not correctness -- every
        // candidate is verified against the slow reference above).
        let mut rng = Xorshift64(0x9E3779B97F4A7C15);
        let mut rook = Vec::with_capacity(64);
        let mut bishop = Vec::with_capacity(64);
        for s in 0..64u8 {
            rook.push(find_magic_for_square(s, &ROOK_DIRS, &mut rng));
            bishop.push(find_magic_for_square(s, &BISHOP_DIRS, &mut rng));
        }
        Magics { rook, bishop }
    }

    #[inline(always)]
    pub fn rook_attacks(&self, s: Square, occ: Bitboard) -> Bitboard {
        self.rook[s as usize].attacks(occ)
    }
    #[inline(always)]
    pub fn bishop_attacks(&self, s: Square, occ: Bitboard) -> Bitboard {
        self.bishop[s as usize].attacks(occ)
    }
}
