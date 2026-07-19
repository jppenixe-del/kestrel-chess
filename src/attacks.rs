use crate::bitboard::*;
use crate::types::*;

pub struct Attacks {
    pub knight: [Bitboard; 64],
    pub king: [Bitboard; 64],
    pub pawn: [[Bitboard; 64]; 2], // [color][square]
}

fn knight_attacks_from(s: Square) -> Bitboard {
    let f = file_of(s) as i32;
    let r = rank_of(s) as i32;
    let deltas: [(i32, i32); 8] = [
        (1, 2), (2, 1), (2, -1), (1, -2),
        (-1, -2), (-2, -1), (-2, 1), (-1, 2),
    ];
    let mut out = 0u64;
    for (df, dr) in deltas {
        let nf = f + df;
        let nr = r + dr;
        if (0..8).contains(&nf) && (0..8).contains(&nr) {
            out |= bb(sq(nf as u8, nr as u8));
        }
    }
    out
}

fn king_attacks_from(s: Square) -> Bitboard {
    let f = file_of(s) as i32;
    let r = rank_of(s) as i32;
    let mut out = 0u64;
    for df in -1..=1 {
        for dr in -1..=1 {
            if df == 0 && dr == 0 {
                continue;
            }
            let nf = f + df;
            let nr = r + dr;
            if (0..8).contains(&nf) && (0..8).contains(&nr) {
                out |= bb(sq(nf as u8, nr as u8));
            }
        }
    }
    out
}

fn pawn_attacks_from(s: Square, color: Color) -> Bitboard {
    let f = file_of(s) as i32;
    let r = rank_of(s) as i32;
    let dr: i32 = if color == Color::White { 1 } else { -1 };
    let mut out = 0u64;
    for df in [-1, 1] {
        let nf = f + df;
        let nr = r + dr;
        if (0..8).contains(&nf) && (0..8).contains(&nr) {
            out |= bb(sq(nf as u8, nr as u8));
        }
    }
    out
}

impl Attacks {
    pub fn new() -> Self {
        let mut knight = [0u64; 64];
        let mut king = [0u64; 64];
        let mut pawn = [[0u64; 64]; 2];
        for s in 0..64u8 {
            knight[s as usize] = knight_attacks_from(s);
            king[s as usize] = king_attacks_from(s);
            pawn[Color::White.idx()][s as usize] = pawn_attacks_from(s, Color::White);
            pawn[Color::Black.idx()][s as usize] = pawn_attacks_from(s, Color::Black);
        }
        Attacks { knight, king, pawn }
    }
}

/// Sliding attacks via ray-cast + blocker scan (simple, correct; not the
/// fastest approach but avoids magic-bitboard bugs for the first version).
#[inline]
pub fn bishop_attacks(s: Square, occ: Bitboard) -> Bitboard {
    ray_attacks(s, occ, &[(1, 1), (1, -1), (-1, 1), (-1, -1)])
}

#[inline]
pub fn rook_attacks(s: Square, occ: Bitboard) -> Bitboard {
    ray_attacks(s, occ, &[(1, 0), (-1, 0), (0, 1), (0, -1)])
}

#[inline]
pub fn queen_attacks(s: Square, occ: Bitboard) -> Bitboard {
    bishop_attacks(s, occ) | rook_attacks(s, occ)
}

fn ray_attacks(s: Square, occ: Bitboard, dirs: &[(i32, i32)]) -> Bitboard {
    let f0 = file_of(s) as i32;
    let r0 = rank_of(s) as i32;
    let mut out = 0u64;
    for &(df, dr) in dirs {
        let mut f = f0 + df;
        let mut r = r0 + dr;
        while (0..8).contains(&f) && (0..8).contains(&r) {
            let t = sq(f as u8, r as u8);
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
