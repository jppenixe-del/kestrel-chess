// Three printing helpers that the substrate's `uci.cpp` also defines.
//
// Written here rather than taken, because taking them would mean compiling the
// substrate's whole engine layer -- which carries its own transposition table
// and its own search -- to get three small functions.
#include "uci.h"

#include <string>

#include "position.h"
#include "types.h"

namespace Kestrel {

std::string UCIEngine::square(Square s) {
    return std::string{char('a' + file_of(s)), char('1' + rank_of(s))};
}

std::string UCIEngine::move(Move m, bool chess960) {
    if (m == Move::none())
        return "(none)";
    if (m == Move::null())
        return "0000";

    Square de  = m.from_sq();
    Square para = m.to_sq();

    // No roque, o UCI normal diz rei-para-casa-de-destino; o Chess960 diz
    // rei-come-torre, que e' como o lance esta' guardado.
    if (m.type_of() == CASTLING && !chess960)
        para = make_square(para > de ? FILE_G : FILE_C, rank_of(de));

    std::string s = square(de) + square(para);
    if (m.type_of() == PROMOTION)
        s += " pnbrqk"[m.promotion_type()];
    return s;
}

int UCIEngine::to_cp(Value v, const Position&) {
    // A normalizacao do substrato depende da rede dele. Nos imprimimos a nota
    // nas NOSSAS unidades, que e' o que a busca ja' faz; isto so' existe para
    // o `eval` do substrato ligar.
    return int(v);
}

}  // namespace Kestrel
