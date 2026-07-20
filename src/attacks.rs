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

/// Sliding attacks via real magic bitboards (see magic.rs) -- was a naive
/// ray-cast + blocker scan until profiling showed it at ~17% of total
/// search time. Magic numbers are found by self-verifying random search
/// at startup (see magic.rs), not hardcoded/transcribed from anywhere.
static MAGICS: std::sync::OnceLock<crate::magic::Magics> = std::sync::OnceLock::new();
fn magics() -> &'static crate::magic::Magics {
    MAGICS.get_or_init(crate::magic::Magics::new)
}

#[inline]
pub fn bishop_attacks(s: Square, occ: Bitboard) -> Bitboard {
    magics().bishop_attacks(s, occ)
}

#[inline]
pub fn rook_attacks(s: Square, occ: Bitboard) -> Bitboard {
    magics().rook_attacks(s, occ)
}

#[inline]
pub fn queen_attacks(s: Square, occ: Bitboard) -> Bitboard {
    bishop_attacks(s, occ) | rook_attacks(s, occ)
}
