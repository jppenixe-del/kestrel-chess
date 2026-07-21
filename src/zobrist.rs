use crate::board::Board;
use crate::types::*;

pub struct Zobrist {
    pub piece_sq: [[[u64; 64]; 6]; 2], // [color][piece][square]
    pub side: u64,
    pub castling: [u64; 16],
    pub ep_file: [u64; 8],
}

// PRNG simples (splitmix64) so' para gerar as chaves uma vez no arranque --
// determinismo entre execucoes nao importa aqui, so' que sejam bem
// distribuidas.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

impl Zobrist {
    pub fn new() -> Self {
        let mut state = 0x9E3779B97F4A7C15u64;
        let mut piece_sq = [[[0u64; 64]; 6]; 2];
        for c in 0..2 {
            for p in 0..6 {
                for s in 0..64 {
                    piece_sq[c][p][s] = splitmix64(&mut state);
                }
            }
        }
        let side = splitmix64(&mut state);
        let mut castling = [0u64; 16];
        for c in castling.iter_mut() {
            *c = splitmix64(&mut state);
        }
        let mut ep_file = [0u64; 8];
        for e in ep_file.iter_mut() {
            *e = splitmix64(&mut state);
        }
        Zobrist { piece_sq, side, castling, ep_file }
    }

    /// Recomputed from scratch each call (once per search node). This
    /// is deliberately NOT maintained incrementally, despite that being
    /// the textbook approach: tried it (2026-07-21, incremental hash in
    /// add_piece/remove_piece/make_move, verified bit-exact over 124M
    /// perft nodes) and it was a NET LOSS here -- perft(6) went ~4.8s ->
    /// ~5.9s. Reason specific to this engine: generate_legal() does a
    /// make/unmake per candidate move for legality (~35 per node), so
    /// make/unmake are called far more often than this hash() is; paying
    /// a few XORs in every make/unmake to save one recompute per node is
    /// negative when the make:hash call ratio is ~35:1. Measured and
    /// reverted, documented here so it isn't re-attempted as an "obvious
    /// speedup" without also changing how legality checking works.
    pub fn hash(&self, board: &Board) -> u64 {
        let mut h = 0u64;
        for c in [Color::White, Color::Black] {
            for pt in ALL_PIECES {
                let mut bbp = board.pieces[c.idx()][pt.idx()];
                while bbp != 0 {
                    let s = bbp.trailing_zeros() as usize;
                    bbp &= bbp - 1;
                    h ^= self.piece_sq[c.idx()][pt.idx()][s];
                }
            }
        }
        if board.side == Color::Black {
            h ^= self.side;
        }
        h ^= self.castling[(board.castling & 0xF) as usize];
        if board.ep_square != NO_SQUARE {
            h ^= self.ep_file[file_of(board.ep_square) as usize];
        }
        h
    }
}
