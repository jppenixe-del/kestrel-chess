#pragma once

// Minimal stand-in for Nap2Siriux's real board.h (459 lines, itself pulling
// in attacks.h/cuckoo.h/eval_state.h/movegen.h -- their whole engine core,
// none of which nnue_net.cpp actually calls). nnue_net.cpp only ever touches
// five methods on `board` (grep-confirmed against the vendored copy):
// sideToMove(), kingSq(Color), pieces(Color,PieceType), pieces(PieceType),
// pieces(Color), allPieces(), gamePly(). This provides exactly those, backed
// by plain arrays the Rust-side FFI shim fills in directly -- no movegen, no
// Zobrist, no FEN parsing pulled in for a static eval call.

#include "defs.h"
#include "bitboard.h"
#include "attacks.h"

class Board
{
public:
    Bitboard m_Pieces[2][6] = {};
    Color m_Stm = Color::WHITE;
    i32 m_GamePly = 0;

    Color sideToMove() const { return m_Stm; }

    Square kingSq(Color color) const
    {
        return m_Pieces[static_cast<i32>(color)][static_cast<i32>(PieceType::KING)].lsb();
    }

    Bitboard pieces(Color color, PieceType type) const
    {
        return m_Pieces[static_cast<i32>(color)][static_cast<i32>(type)];
    }

    Bitboard pieces(PieceType type) const
    {
        return m_Pieces[0][static_cast<i32>(type)] | m_Pieces[1][static_cast<i32>(type)];
    }

    Bitboard pieces(Color color) const
    {
        Bitboard bb = EMPTY_BB;
        for (i32 t = 0; t < 6; ++t)
            bb |= m_Pieces[static_cast<i32>(color)][t];
        return bb;
    }

    Bitboard allPieces() const { return pieces(Color::WHITE) | pieces(Color::BLACK); }

    i32 gamePly() const { return m_GamePly; }
};
