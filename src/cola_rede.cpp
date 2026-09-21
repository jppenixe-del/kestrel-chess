// A COLA. Este ficheiro e' NOSSO, nao e' codigo derivado.
//
// Conduz a posicao do leitor vendorizado a par da nossa com make/unmake, que
// e' o que faz o acumulador seguir a busca em vez de ser reconstruido a cada
// no'. Escrito para este motor.

// A leitura e a avaliacao da rede de outro motor, expostas em C.
//
// A ideia nao e' minha: a rede deles nao e' uma funcao que recebe uma posicao e
// devolve um numero, e' um subsistema que vive agarrado ao tabuleiro -- a pilha
// de acumuladores e' alimentada pelo `do_move`, as tabelas de refresco sao
// indexadas pela casa do rei, e as features de ameaca precisam dos ataques
// calculados pelos bitboards deles. Reimplementar isso a` mao foi o que passei
// um dia a fazer, com ganhos de tres e seis por cento e tres regressoes.
//
// Entao o nosso `make_move` passa a conduzir tambem uma `Position` deles, e a
// avaliacao vem de la'. Ficamos com a geracao de lances, a ordenacao, a poda e o
// tempo -- que e' onde a medicao diz que esta' o nosso buraco: eles chegam a`
// profundidade 11 em 181 mil nos onde nos precisamos de 966 mil.
//
// O que devolve e' a soma crua de material e posicional, sem a mistura de
// optimismo nem o amortecimento deles: essa parte e' da busca, e a busca e'
// nossa. Tambem evita o `assert(!pos.checkers())` do `Eval::evaluate`, que a
// nossa busca nao respeita.
//
// Licencas: os dois lados sao GPL-3.

#include <cstdio>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "attacks.h"
#include "evaluate.h"
#include "nnue/network.h"
#include "nnue/nnue_accumulator.h"
#include "nnue/nnue_misc.h"
#include "position.h"
#include "types.h"

using namespace Kestrel;

namespace {

std::unique_ptr<Eval::NNUE::Network> g_rede;
bool                                 g_iniciado = false;

// Um contexto por thread de busca: o tabuleiro deles, a pilha de estados que o
// `do_move` encadeia, a pilha de acumuladores e as tabelas de refresco.
struct Ctx {
    Position                                      pos;
    std::vector<StateInfo>                        st;
    // Os lances feitos, na codificacao deles, para se desfazerem exactamente
    // como foram feitos em vez de adivinhados a partir do tabuleiro seguinte.
    std::vector<Move>                             feitos;
    // Lances que o `sfn_lance` RECUSOU por falta de espaco na pilha.
    //
    // Sem isto, um lance recusado nao empilha nada mas o `sfn_desfaz` que vem
    // a seguir desempilha na mesma: tira do `feitos` o lance ANTERIOR, desfaz
    // um lance que nao devia, e faz `accs.pop()` sem o push correspondente. O
    // `pop` do acumulador decrementa `size` sem guarda em release, e o ciclo
    // do `evaluate` -- `for (usize curr_idx = size - 1; ...)` -- arranca em
    // 18 446 744 073 709 551 615 e anda para tras. Nao rebenta nem devolve:
    // fica la' dentro dezenas de segundos.
    //
    // Medido antes da correccao: 48, 36 e 33 segundos de SILENCIO entre a
    // ultima linha `info` e o `bestmove`, em 3 lances de 195, a profundidades
    // diferentes e sem reproduzir a frio -- porque depende do estado da pilha
    // e nao da posicao.
    std::size_t                                   recusados = 0;
    // Profundidade maxima que a pilha atingiu na busca actual.
    std::size_t                                   pico = 0;
    std::size_t                                   n = 0;
    Eval::NNUE::AccumulatorStack                  accs;
    std::unique_ptr<Eval::NNUE::AccumulatorCaches> caches;
};

// O lance, no formato deles.
//
// Tres casos que nao sao `NORMAL` e que, se ficarem por tratar, dao um
// acumulador que descreve outra posicao sem nada a assinalar: a promocao, o
// en passant, e o roque -- que eles codificam como o rei a TOMAR a propria
// torre, e nao como o rei a andar duas casas.
Move faz_lance(const Position& pos, int de, int para, int tipo) {
    (void) pos;
    const Square from = Square(de);
    const Square to   = Square(para);

    if (tipo >= 1 && tipo <= 4)
        return Move::make<PROMOTION>(from, to, PieceType(tipo + 1));  // 1=N .. 4=Q

    if (tipo == 5)
        return Move::make<EN_PASSANT>(from, to);

    if (tipo == 6)
    {
        // Eles codificam o roque como o rei a TOMAR a propria torre.
        const int    d   = int(to) - int(from);
        const Square rsq = d > 0 ? Square(int(from) + 3) : Square(int(from) - 4);
        return Move::make<CASTLING>(from, rsq);
    }

    return Move(from, to);
}

}  // namespace

extern "C" {

// Le a rede. Uma vez, para todos os contextos.
int sfn_init(const char* caminho) {
    if (!g_iniciado)
    {
        Attacks::init();
        Position::init();
        g_iniciado = true;
    }
    g_rede = std::make_unique<Eval::NNUE::Network>();
    Eval::NNUE::EvalFile ef;
    g_rede->load(std::filesystem::path("."), std::filesystem::path(caminho), ef);
    return ef.current.has_value() ? 0 : -1;
}

void* sfn_novo(void) {
    if (!g_rede)
        return nullptr;
    Ctx* c = new Ctx();
    c->st.resize(1024);
    c->caches = std::make_unique<Eval::NNUE::AccumulatorCaches>(*g_rede);
    return c;
}

void sfn_fim(void* p) { delete static_cast<Ctx*>(p); }

// Poe o tabuleiro numa posicao e recomeca a pilha do zero.
int sfn_fen(void* p, const char* fen) {
    Ctx* c = static_cast<Ctx*>(p);
    // O pico da busca que acabou. Se for pequeno, a pilha nao cresce e o
    // bloqueio nao vem daqui; se for grande, vem.
    if (c->pico > 0) {
        std::fprintf(stderr, "PICO DA PILHA: %zu (de %zu)\n", c->pico, c->st.size());
        c->pico = 0;
    }
    c->n   = 0;
    c->feitos.clear();
    auto e = c->pos.set(std::string(fen), false, &c->st[0]);
    if (e.has_value())
        return -1;
    c->accs.reset();
    return 0;
}

int sfn_lance(void* p, int de, int para, int promo) {
    Ctx*       c = static_cast<Ctx*>(p);
    const Move m = faz_lance(c->pos, de, para, promo);
    if (c->n + 2 >= c->st.size()) {
        c->recusados++;
        static int avisos = 0;
        if (avisos < 3) { std::fprintf(stderr, "PILHA ESGOTADA: n=%zu\n", c->n); avisos++; }
        return -1;
    }
    Dirties& d = c->accs.push();
    c->n++;
    if (c->n > c->pico) c->pico = c->n;
    c->feitos.push_back(m);
    c->pos.do_move(m, c->st[c->n], c->pos.gives_check(m), d, nullptr, nullptr);
    return 0;
}

void sfn_desfaz(void* p, int de, int para, int promo) {
    Ctx* c = static_cast<Ctx*>(p);
    (void) de;
    (void) para;
    (void) promo;
    // O lance sai da pilha, exactamente como foi feito. A versao anterior
    // reconstruia-o do tabuleiro DEPOIS dele e comparava a casa de destino com
    // `pos.ep_square()`, que nessa altura ja' e' a da posicao nova: a captura
    // ao passo era desfeita como lance normal e vice-versa, e os dois
    // tabuleiros separavam-se sem nada a assinalar.
    // Um lance recusado nao empilhou nada: o desfazer dele tem de nao
    // desempilhar nada tambem. Sem este ramo, cada recusa deixava a pilha do
    // acumulador um nivel abaixo do que devia, e bastavam poucas para o
    // `size` chegar a zero.
    if (c->recusados > 0) {
        c->recusados--;
        return;
    }
    if (c->feitos.empty())
        return;
    const Move m = c->feitos.back();
    c->feitos.pop_back();
    c->pos.undo_move(m);
    c->accs.pop();
    c->n--;
}

// O lance nulo: passa a vez sem mexer peca nenhuma. Nao mexe no acumulador
// -- nao ha' feature nenhuma a mudar -- mas o estado tem de ser empilhado na
// mesma, senao o `undo_null_move` desfaz o lance errado.
void sfn_nulo(void* p) {
    Ctx* c = static_cast<Ctx*>(p);
    if (c->n + 2 >= c->st.size())
        return;
    c->n++;
    c->pos.do_null_move(c->st[c->n]);
}

void sfn_desfaz_nulo(void* p) {
    Ctx* c = static_cast<Ctx*>(p);
    c->pos.undo_null_move();
    c->n--;
}

// Os contadores do acumulador, no fim da busca. Fica deste lado -- e nao no
// Declarada deste lado desde 20-09-2026: vivia no `nnue_accumulator.h` deles e
// a actualizacao do vendor levou-a. O que e' nosso declara-se em ficheiro nosso.
namespace Kestrel { namespace Eval { namespace NNUE { void imprime_conta_h2k(); } } }

// `main` deles -- para nada no motor deles ter de saber que isto existe.
void sfn_conta(void) { ::Kestrel::Eval::NNUE::imprime_conta_h2k(); }

// A posicao actual em FEN, para o portao poder comparar os dois tabuleiros.
const char* sfn_fen_actual(void* p) {
    static thread_local std::string s;
    s = static_cast<Ctx*>(p)->pos.fen();
    return s.c_str();
}

// Material mais posicional, do ponto de vista de quem joga.
int sfn_avalia(void* p) {
    Ctx* c              = static_cast<Ctx*>(p);
    auto [psqt, posic]  = g_rede->evaluate(c->pos, c->accs, *c->caches);
    return int(psqt) + int(posic);
}

// Para o portao: o mesmo numero com a pilha recomecada do zero, que obriga a
// uma reconstrucao completa em vez do caminho incremental.
int sfn_avalia_do_zero(void* p) {
    Ctx* c = static_cast<Ctx*>(p);
    c->accs.reset();
    auto [psqt, posic] = g_rede->evaluate(c->pos, c->accs, *c->caches);
    return int(psqt) + int(posic);
}

}  // extern "C"
