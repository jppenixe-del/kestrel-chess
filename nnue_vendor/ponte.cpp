// A PONTE. Este ficheiro e' NOSSO, nao e' codigo derivado.
//
// Vive dentro do `nnue_vendor/` porque e' aqui que ele fala com o leitor
// vendorizado, mas nao veio de la': foi escrito para este motor. Tem
// cabecalho proprio a dize-lo para quem olhar para a pasta conseguir separar
// o que e' derivado do que e' original -- que e' a unica maneira de o
// NOTICES.md ser verdade e nao uma declaracao de boa vontade.
//
// O `conta_h2k.cpp` e o `src/cola_rede.cpp` estao na mesma situacao.

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

#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "attacks.h"
#include <cstdlib>
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
    if (c->n + 2 >= c->st.size())
        return -1;
    Dirties& d = c->accs.push();
    c->n++;
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
// `main` deles -- para nada no motor deles ter de saber que isto existe.
//
// A DECLARACAO tambem fica deste lado, desde 20-09-2026. Vivia no
// `nnue/nnue_accumulator.h` deles e a actualizacao do vendor copiou o ficheiro
// de origem por cima e levou-a. Tudo o que e' nosso declara-se em ficheiro
// nosso, senao a proxima actualizacao parte outra vez o mesmo sitio.
namespace Kestrel { namespace Eval { namespace NNUE { void imprime_conta_h2k(); } } }

void sfn_conta(void) { ::Kestrel::Eval::NNUE::imprime_conta_h2k(); }

// A posicao actual em FEN, para o portao poder comparar os dois tabuleiros.
const char* sfn_fen_actual(void* p) {
    static thread_local std::string s;
    s = static_cast<Ctx*>(p)->pos.fen();
    return s.c_str();
}

// As tres camadas que o `Eval::evaluate()` deles poe por cima da rede crua.
//
// 20-09: media-se que o half2k desviava 57 cp do KestrelStrike com a MESMA
// rede, e a razao entre os dois era constante em ~1,47 no meio-jogo e caia
// para 1,04 num final de torres. Isso nao e' leitor desactualizado -- o vendor
// ja' estava ao nivel da origem, verificado linha a linha. E' esta funcao:
//
//   o KestrelStrike chama o `Eval::evaluate()` INTEIRO
//   a ponte chamava so' o `Network::evaluate()`, a rede crua
//
// A ponte saltou-o para fugir ao `assert(!pos.checkers())` deles, que a nossa
// busca viola de proposito. As tres camadas vem entao para aqui, e o assert
// fica de fora -- que e' o unico motivo por que se saltava a funcao.
//
// O `optimism` vai a ZERO: nasce do estado da raiz da busca deles e nos nao
// temos equivalente. Inventar um seria acrescentar uma quarta diferenca a uma
// correccao que existe para tirar tres.
//
// Os valores de peca tem de ser os DELES -- e' por isso que se usa o
// `non_pawn_material()` da posicao e o 534 por peao. Com os nossos o divisor
// 91000 engolia o termo inteiro.
static bool cheia_ligada() {
    static const bool v = std::getenv("H2K_AVAL_CHEIA") != nullptr;
    return v;
}

// O FACTOR DE ESCALA, em centesimos. 100 = nao mexe.
//
// Com as tres camadas ligadas e mais nada, o desvio face ao ks_1.20260919 caiu
// de 57,1 para 51,2 cp -- mas a razao entre os dois ficou CONSTANTE em 1,18 nas
// doze posicoes, do meio-jogo ao final de torres. Uma razao constante nao e'
// camada em falta, e' escala.
//
// Medido a 20-09 com as doze posicoes do `estatica_ponte.py`:
//
//     rede crua                      57,1 cp de desvio medio
//     + as tres camadas              51,2
//     + as camadas e o factor 118     1,8   <- sete posicoes batem ao certo
//
// 100 por omissao, para nao mudar nada sem se mandar. O valor que faz os dois
// motores coincidirem e' 118.
static int factor_escala() {
    static const int v = [] {
        const char* e = std::getenv("H2K_AVAL_FACTOR");
        const int   n = e ? std::atoi(e) : 100;
        return (n >= 10 && n <= 400) ? n : 100;
    }();
    return v;
}

// O OPTIMISM, posto pela busca antes de avaliar.
//
// Nasce na busca deles assim (search.cpp):
//
//     optimism[us]  = 114 * avg / (abs(avg) + 85);
//     optimism[~us] = -optimism[us];
//
// onde `avg` e' a nota media do lance de raiz. O 114 e' o que o KestrelStrike
// expoe como `KS_OTIMISMO` e traz ligado por omissao na versao que o bot joga.
//
// Por thread, porque cada ajudante tem a sua busca e a sua nota de raiz.
// Zero por omissao: sem a busca o pedir, a avaliacao e' a que era.
static thread_local int g_otimismo = 0;

void sfn_otimismo(int v) { g_otimismo = v; }

// Material mais posicional, do ponto de vista de quem joga.
int sfn_avalia(void* p) {
    Ctx* c              = static_cast<Ctx*>(p);
    auto [psqt, posic]  = g_rede->evaluate(c->pos, c->accs, *c->caches);
    if (!cheia_ligada())
        return int(psqt) + int(posic);
    long long nnue = (long long) psqt + (long long) posic;
    const long long complexidade = std::llabs((long long) psqt - (long long) posic);
    long long otim = g_otimismo;
    otim += otim * complexidade / 476;
    nnue -= nnue * complexidade / 18236;
    const long long material =
      534LL * c->pos.count<PAWN>() + (long long) c->pos.non_pawn_material();
    long long v = nnue + (nnue * material + otim * 7675) / 91000;
    v -= v * (long long) c->pos.rule50_count() / 199;
    v = v * (long long) factor_escala() / 100;
    return int(v);
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
