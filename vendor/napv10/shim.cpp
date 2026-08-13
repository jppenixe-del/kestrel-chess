// Thin C ABI boundary between the vendored Nap2Siriux NNUE code (unmodified,
// same evaluate() the reference engine plays with) and our Rust engine.
// Board construction is the only new code here -- everything past that is
// their real forward pass, their real SIMD.

#include "board.h"
#include "napoleon/nnue_net.h"
#include <cstring>

// Stubs for the embedded-net path (embedded_net.cpp, not vendored -- we
// always load from a file via napv10_load, never the binary-baked-in
// default net their standalone executable ships with). Never called, only
// linked: loadEmbedded()/loadSmallEmbedded() check hasEmbeddedNet() first
// and return false immediately, so these bodies never run.
namespace napoleon::nnue
{
bool hasEmbeddedNet() { return false; }
bool hasEmbeddedSmallNet() { return false; }
const uint8_t* embeddedNetData() { return nullptr; }
size_t embeddedNetSize() { return 0; }
const uint8_t* embeddedSmallNetData() { return nullptr; }
size_t embeddedSmallNetSize() { return 0; }
}

extern "C" {

int napv10_load(const char* path)
{
    // Magic-bitboard tables (attacks::bishopAttacks/rookAttacks etc, used by
    // the threat-feature gathering) are precomputed here, not at static-init
    // time -- skipping this is a straight segfault the first time evaluate()
    // touches a slider attack.
    static bool attacks_ready = false;
    if (!attacks_ready) {
        attacks::init();
        attacks_ready = true;
    }
    return napoleon::nnue::load(std::string(path)) ? 1 : 0;
}

// pieces: [color][piece_type] bitboards, color 0=white 1=black,
// piece_type 0=pawn 1=knight 2=bishop 3=rook 4=queen 5=king -- same order
// Kestrel's own Board::pieces array already uses.
int napv10_evaluate(const unsigned long long pieces[2][6], int side_to_move, int game_ply)
{
    Board b;
    for (int c = 0; c < 2; ++c)
        for (int t = 0; t < 6; ++t)
            b.m_Pieces[c][t] = Bitboard(pieces[c][t]);
    b.m_Stm = static_cast<Color>(side_to_move);
    b.m_GamePly = game_ply;
    return napoleon::nnue::evaluate(b);
}

}
