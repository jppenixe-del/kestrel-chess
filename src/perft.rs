use crate::attacks::Attacks;
use crate::board::Board;
use crate::movegen::generate_legal;

pub fn perft(board: &mut Board, depth: u32, atk: &Attacks) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = generate_legal(board, atk);
    if depth == 1 {
        return moves.len() as u64;
    }
    let mut nodes = 0u64;
    for mv in moves {
        let undo = board.make_move(&mv);
        nodes += perft(board, depth - 1, atk);
        board.unmake_move(&mv, &undo);
    }
    nodes
}
