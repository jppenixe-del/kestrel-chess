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

/// Como perft(), mas em cada no' compara os acumuladores incrementais
/// (mg_score/eg_score/phase, mantidos por add_piece/remove_piece) contra
/// um recalculo do zero -- apanha qualquer divergencia (roque, en passant,
/// promocao, promocao+captura) em vez de confiar so' na inspecao do
/// codigo. Devolve o numero de nos visitados e o numero de discrepancias
/// encontradas (deve ser sempre 0).
pub fn verify_incremental_eval(board: &mut Board, depth: u32, atk: &Attacks) -> (u64, u64) {
    let mut fresh = board.clone();
    fresh.recompute_eval_accumulators();
    let mut mismatches = 0u64;
    if fresh.mg_score != board.mg_score || fresh.eg_score != board.eg_score || fresh.phase != board.phase {
        mismatches += 1;
        eprintln!(
            "MISMATCH fen={} incremental=({},{},{}) fresh=({},{},{})",
            board.to_fen(), board.mg_score, board.eg_score, board.phase,
            fresh.mg_score, fresh.eg_score, fresh.phase
        );
    }
    if depth == 0 {
        return (1, mismatches);
    }
    let moves = generate_legal(board, atk);
    let mut nodes = 1u64;
    for mv in moves {
        let undo = board.make_move(&mv);
        let (n, m) = verify_incremental_eval(board, depth - 1, atk);
        nodes += n;
        mismatches += m;
        board.unmake_move(&mv, &undo);
    }
    (nodes, mismatches)
}
