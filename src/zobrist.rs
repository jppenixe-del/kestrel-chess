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
