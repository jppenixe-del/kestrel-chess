use crate::attacks::*;
use crate::bitboard::*;
use crate::board::*;
use crate::moves::*;
use crate::types::*;

pub fn generate_pseudo_legal(board: &Board, atk: &Attacks, out: &mut Vec<Move>) {
    let us = board.side;
    let them = us.opp();
    let own = board.occ_color[us.idx()];
    let occ = board.occ_all;

    // Pawns
    let pawns = board.pieces[us.idx()][PieceType::Pawn.idx()];
    let (push_dir, start_rank, from_promo_rank): (i32, Bitboard, u8) = if us == Color::White {
        (8, RANK_2, 6) // pawn on rank 7 (index 6) promotes on push/capture
    } else {
        (-8, RANK_7, 1) // pawn on rank 2 (index 1) promotes
    };
    let mut p = pawns;
    while p != 0 {
        let from = pop_lsb(&mut p);
        let will_promote = rank_of(from) == from_promo_rank;
        let one = ((from as i32) + push_dir) as u8;
        if one < 64 && bb(one) & occ == 0 {
            if will_promote {
                add_promotions(out, from, one, false);
            } else {
                out.push(Move::quiet(from, one));
            }
            if bb(from) & start_rank != 0 {
                let two = ((from as i32) + push_dir * 2) as u8;
                if bb(two) & occ == 0 {
                    out.push(Move { from, to: two, promotion: None, flag: MoveFlag::DoublePush });
                }
            }
        }
        let attacks = atk.pawn[us.idx()][from as usize];
        let mut caps = attacks & board.occ_color[them.idx()];
        while caps != 0 {
            let to = pop_lsb(&mut caps);
            if will_promote {
                add_promotions(out, from, to, true);
            } else {
                out.push(Move::capture(from, to));
            }
        }
        if board.ep_square != NO_SQUARE && attacks & bb(board.ep_square) != 0 {
            out.push(Move { from, to: board.ep_square, promotion: None, flag: MoveFlag::EnPassant });
        }
    }

    // Knights
    let mut n = board.pieces[us.idx()][PieceType::Knight.idx()];
    while n != 0 {
        let from = pop_lsb(&mut n);
        gen_from_attacks(out, from, atk.knight[from as usize] & !own, board, them);
    }
    // Bishops
    let mut b = board.pieces[us.idx()][PieceType::Bishop.idx()];
    while b != 0 {
        let from = pop_lsb(&mut b);
        gen_from_attacks(out, from, bishop_attacks(from, occ) & !own, board, them);
    }
    // Rooks
    let mut r = board.pieces[us.idx()][PieceType::Rook.idx()];
    while r != 0 {
        let from = pop_lsb(&mut r);
        gen_from_attacks(out, from, rook_attacks(from, occ) & !own, board, them);
    }
    // Queens
    let mut q = board.pieces[us.idx()][PieceType::Queen.idx()];
    while q != 0 {
        let from = pop_lsb(&mut q);
        gen_from_attacks(out, from, queen_attacks(from, occ) & !own, board, them);
    }
    // King
    let king_sq = board.king_sq(us);
    gen_from_attacks(out, king_sq, atk.king[king_sq as usize] & !own, board, them);

    // Castling
    if !board.in_check(us, atk) {
        if us == Color::White {
            if board.castling & CASTLE_WK != 0
                && occ & (bb(5) | bb(6)) == 0
                && !board.is_square_attacked(5, them, atk)
                && !board.is_square_attacked(6, them, atk)
            {
                out.push(Move { from: 4, to: 6, promotion: None, flag: MoveFlag::CastleKing });
            }
            if board.castling & CASTLE_WQ != 0
                && occ & (bb(1) | bb(2) | bb(3)) == 0
                && !board.is_square_attacked(3, them, atk)
                && !board.is_square_attacked(2, them, atk)
            {
                out.push(Move { from: 4, to: 2, promotion: None, flag: MoveFlag::CastleQueen });
            }
        } else {
            if board.castling & CASTLE_BK != 0
                && occ & (bb(61) | bb(62)) == 0
                && !board.is_square_attacked(61, them, atk)
                && !board.is_square_attacked(62, them, atk)
            {
                out.push(Move { from: 60, to: 62, promotion: None, flag: MoveFlag::CastleKing });
            }
            if board.castling & CASTLE_BQ != 0
                && occ & (bb(57) | bb(58) | bb(59)) == 0
                && !board.is_square_attacked(59, them, atk)
                && !board.is_square_attacked(58, them, atk)
            {
                out.push(Move { from: 60, to: 58, promotion: None, flag: MoveFlag::CastleQueen });
            }
        }
    }
}

fn add_promotions(out: &mut Vec<Move>, from: Square, to: Square, _capture: bool) {
    for p in [PieceType::Queen, PieceType::Rook, PieceType::Bishop, PieceType::Knight] {
        out.push(Move { from, to, promotion: Some(p), flag: if _capture { MoveFlag::Capture } else { MoveFlag::Quiet } });
    }
}

fn gen_from_attacks(out: &mut Vec<Move>, from: Square, targets: Bitboard, board: &Board, them: Color) {
    let mut t = targets;
    while t != 0 {
        let to = pop_lsb(&mut t);
        if board.occ_color[them.idx()] & bb(to) != 0 {
            out.push(Move::capture(from, to));
        } else {
            out.push(Move::quiet(from, to));
        }
    }
}

/// Legality check via make/unmake on the SAME board (found in review,
/// 2026-07-21) instead of a full `Board::clone()` + `make_move()` per
/// pseudo-legal candidate -- every other legality check in the search
/// (negamax/quiescence's own move loops) already uses make/unmake on
/// one board; this was the one remaining spot still paying a full
/// struct clone (mailbox, accumulators, castling/ep state, everything)
/// per candidate move, at every single node. Requires `&mut Board`
/// instead of `&Board` -- every call site already holds a `&mut Board`
/// or an owned `Board`, so this is a mechanical signature change.
pub fn generate_legal(board: &mut Board, atk: &Attacks) -> Vec<Move> {
    let mut pseudo = Vec::with_capacity(64);
    generate_pseudo_legal(board, atk, &mut pseudo);
    let mut legal = Vec::with_capacity(pseudo.len());
    for mv in pseudo {
        let us = board.side;
        let undo = board.make_move(&mv);
        let illegal = board.in_check(us, atk);
        board.unmake_move(&mv, &undo);
        if !illegal {
            legal.push(mv);
        }
    }
    legal
}
