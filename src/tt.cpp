// The transposition table.
//
// Three ways to a bucket, each slot 24 bytes: the XOR-folded key, the packed
// data, and the generation. Storing `key ^ data` rather than the key means a
// torn read fails the comparison instead of being taken for a hit.
#include "tt.h"

#include <atomic>
#include <cstring>
#include <memory>

namespace Kestrel {

namespace {

// [BINARIO] O empacotamento, lido da `sonda`:
//
//     bits  0-15  lance          bits 40-41  limite
//     bits 16-31  nota  (int16)  bit  42     pv
//     bits 32-39  prof  (int8)   bits 43-58  aval  (int16)
constexpr int B_LANCE = 0, B_NOTA = 16, B_PROF = 32, B_LIM = 40, B_PV = 42, B_AVAL = 43;

std::uint64_t empacota(int prof, int nota, Limite limite, Move melhor, bool pv,
                       std::int16_t aval) {
    return (std::uint64_t(std::uint16_t(melhor.raw()))                  << B_LANCE)
         | (std::uint64_t(std::uint16_t(std::int16_t(nota)))            << B_NOTA)
         | (std::uint64_t(std::uint8_t(std::int8_t(prof)))              << B_PROF)
         | (std::uint64_t(static_cast<unsigned>(limite) & 0x3)          << B_LIM)
         | (std::uint64_t(pv ? 1u : 0u)                                 << B_PV)
         | (std::uint64_t(std::uint16_t(aval))                          << B_AVAL);
}

int prof_de(std::uint64_t d) { return int(std::int8_t((d >> B_PROF) & 0xFF)); }

// [BINARIO] Profundidade com que se marca uma entrada que so' traz avaliacao,
// e peso da idade na escolha da via a deitar fora.
//
// Os tres numeros saem do `ks_1.20260919` e sao um DESENHO, nao valores
// soltos:
//
//   PROF_SO_AVAL = -8   e' o que a `guarda_so_aval` grava (4205be: a mascara
//                       0x3f800000000 tem 0xf8 = -8 nos bits 32-39)
//   PROF_DESCART = -7   e' o limiar abaixo do qual uma via se sacrifica
//                       (42056d: `cmp $0xf9,%r8b` seguido de `jl`)
//   PENA_VELHA   = 3    (420378: `lea (,%r14,4)` seguido de `sub` -- x - 4x,
//                       que e' -3x.  Escrito assim para evitar um imul, e
//                       facil de ler como 4.)
//
// O -8 esta' UM PLY ABAIXO do -7 de proposito: uma entrada de so'-avaliacao
// nasce ja' descartavel pela via seguinte.  Com os dois a -128, como estava,
// essa relacao quebra-se e as entradas de so'-avaliacao tornam-se imortais.
constexpr int PROF_SO_AVAL = -8;
constexpr int PROF_DESCART = -7;
constexpr int PENA_VELHA   = 3;

}  // namespace

// [BINARIO] Ranhura de 24 bytes, balde de 72 -- tres vias.
struct Ranhura {
    std::atomic<std::uint64_t> chave_xor{0};   // chave ^ dados
    std::atomic<std::uint64_t> dados{0};
    std::atomic<std::uint8_t>  ger{0};
};
static constexpr std::size_t VIAS = 3;
struct Balde {
    Ranhura via[VIAS];
};

// [BINARIO] A sonda, tal como o binario a faz: percorre as tres vias, e a via
// e' desta chave quando `chave_xor ^ dados == chave`. Ao acertar, REFRESCA a
// geracao da ranhura -- o binario escreve la' a geracao corrente antes de
// preencher a saida.
bool TranspositionTable::sonda(std::uint64_t chave, Entrada& e) const {
    if (!baldes)
        return false;
    Balde&       b   = baldes[chave & mascara];
    std::uint8_t ger = geracao.load(std::memory_order_relaxed);
    for (std::size_t i = 0; i < VIAS; ++i) {
        std::uint64_t dados = b.via[i].dados.load(std::memory_order_relaxed);
        std::uint64_t xr    = b.via[i].chave_xor.load(std::memory_order_relaxed);
        if ((xr ^ dados) != chave)
            continue;
        b.via[i].ger.store(ger, std::memory_order_relaxed);
        e.chave  = chave;
        e.prof   = prof_de(dados);
        e.nota   = int(std::int16_t((dados >> B_NOTA) & 0xFFFF));
        e.limite = Limite((dados >> B_LIM) & 0x3);
        e.melhor = Move(std::uint16_t((dados >> B_LANCE) & 0xFFFF));
        e.pv     = ((dados >> B_PV) & 1) != 0;
        e.aval   = std::int16_t((dados >> B_AVAL) & 0xFFFF);
        return true;
    }
    return false;
}

// [ESCRITO] So' se escreve numa via que ja' seja desta chave, ou numa que nao
// valha a pena guardar (vazia, ou ja' so'-avaliacao, ou de uma geracao antiga).
// Se as tres estiverem ocupadas com trabalho a serio, deita-se fora a mais rasa.
std::size_t TranspositionTable::via_a_substituir(Balde& b, std::uint64_t chave,
                                                 std::uint8_t ger) const {
    std::size_t pior = 0;
    int         nota_pior = 1 << 30;
    for (std::size_t i = 0; i < VIAS; ++i) {
        std::uint64_t dados = b.via[i].dados.load(std::memory_order_relaxed);
        std::uint64_t xr    = b.via[i].chave_xor.load(std::memory_order_relaxed);
        if ((xr ^ dados) == chave)
            return i;
        int velhice = std::uint8_t(ger - b.via[i].ger.load(std::memory_order_relaxed));
        // [BINARIO] NAO ha' curto-circuito na via vazia.  O ciclo de 420348 a
        // 42039d percorre sempre as tres e escolhe a de menor nota; uma via
        // vazia da' `0 - PENA_VELHA*velhice` e compete como qualquer outra.
        // Devolver a primeira vazia diverge quando ha' outra ainda pior.
        //
        // Quanto MENOS valor, mais cedo se deita fora: uma entrada rasa ou
        // velha vale menos do que uma funda e fresca.
        int nota = prof_de(dados) - PENA_VELHA * velhice;
        if (nota < nota_pior) { nota_pior = nota; pior = i; }
    }
    return pior;
}

// [FONTE] A POLITICA DE ESCRITA. 0 = como estava (escreve sempre, por cima de
// tudo).
//
// MEDIDO a 15-09-2026, seis posicoes, `go depth 15`, mesma rede dos dois lados:
// quando temos lance na tabela ele corta em 30-43% dos nos; na referencia, nos
// mesmos tabuleiros, 52-63%. Nao e' por ela ter lance mais vezes -- tem menos
// (22-59% contra os nossos 34-70%). E' o lance guardado que e' melhor.
//
//   KS_TT_LANCE  Quando a busca falha em baixo nao ha' lance que preste e o
//                `melhor_lance` vem `Move::none()`. Nos escreviamos esse nada
//                por cima do que la' estava -- muitas vezes um lance vindo de
//                uma busca mais funda.
//
//   KS_TT_PROF   Nao tinhamos guarda de profundidade nenhuma: uma revisita rasa
//                da mesma posicao sobrescrevia a entrada funda.
//
// VALIDADO EM PARTIDAS a 17-09-2026 e por isso OMISSAO:
//   tt_par vs base, 10+0,1, 1t, 64MB, UHO_4060_v2
//   Elo +5,49 +/- 4,12   nElo +10,12   8040 partidas   LLR 2,53-2,72
//
// AS DUAS JUNTAS, e e' obrigatorio. A KS_TT_LANCE sozinha mediu -5,99 +/- 6,79
// em 2960 partidas: preservar o lance velho e escrever por cima a nota e a
// profundidade novas deixa uma entrada em que o lance diz uma coisa e a nota diz
// outra. Separa-las para medir uma de cada vez foi um erro de metodo.
namespace {
const int G_TT_LANCE = [] { const char* v = std::getenv("KS_TT_LANCE"); return v ? std::atoi(v) : 1; }();
const int G_TT_PROF  = [] { const char* v = std::getenv("KS_TT_PROF");  return v ? std::atoi(v) : 1; }();
}  // namespace

void TranspositionTable::guarda(std::uint64_t chave, int prof, int nota, Limite limite,
                                Move melhor, bool pv, std::int16_t aval) {
    if (!baldes)
        return;
    Balde&        b   = baldes[chave & mascara];
    std::uint8_t  ger = geracao.load(std::memory_order_relaxed);
    Ranhura&      r   = b.via[via_a_substituir(b, chave, ger)];

    if (G_TT_LANCE || G_TT_PROF) {
        std::uint64_t antigos = r.dados.load(std::memory_order_relaxed);
        std::uint64_t xr      = r.chave_xor.load(std::memory_order_relaxed);
        bool          mesma   = antigos != 0 && (xr ^ antigos) == chave;

        if (G_TT_LANCE && mesma && melhor == Move::none())
            melhor = Move(std::uint16_t(antigos & 0xFFFF));

        if (G_TT_PROF && mesma) {
            int velhice = std::uint8_t(ger - r.ger.load(std::memory_order_relaxed));
            if (!(limite == Limite::Exacto || prof + 2 * int(pv) > prof_de(antigos) - 4
                  || velhice != 0))
                return;
        }
    }

    std::uint64_t dados = empacota(prof, nota, limite, melhor, pv, aval);
    r.dados.store(dados, std::memory_order_relaxed);
    r.chave_xor.store(chave ^ dados, std::memory_order_relaxed);
    r.ger.store(ger, std::memory_order_relaxed);
}

// [FONTE] Se as tres vias estiverem ocupadas com trabalho a serio, deita-se fora
// a avaliacao em vez do lance -- recalcular a avaliacao custa uma passagem pela
// rede, recalcular a ordenacao custa uma sub-arvore.
void TranspositionTable::guarda_so_aval(std::uint64_t chave, std::int16_t aval) {
    if (!baldes)
        return;
    Balde&       b   = baldes[chave & mascara];
    std::uint8_t ger = geracao.load(std::memory_order_relaxed);

    int alvo = -1;
    for (std::size_t i = 0; i < VIAS; ++i) {
        std::uint64_t dados = b.via[i].dados.load(std::memory_order_relaxed);
        std::uint64_t xr    = b.via[i].chave_xor.load(std::memory_order_relaxed);
        if ((xr ^ dados) == chave) {
            alvo = int(i);
            break;
        }
        if (alvo >= 0)
            continue;
        int velhice = std::uint8_t(ger - b.via[i].ger.load(std::memory_order_relaxed));
        // [BINARIO] 42056d: `cmp $0xf9,%r8b` / `jl` -> prof < -7, estrito.
        // 420579: `cmp $0x2,%r8b` / `jbe` para RECUSAR -> serve com velhice > 2.
        if (dados == 0 || prof_de(dados) < PROF_DESCART || velhice > 2)
            alvo = int(i);
    }
    if (alvo < 0)
        return;

    Ranhura& r = b.via[alvo];
    // Preserva o que a via ja' tivesse desta mesma chave: se e' desta posicao,
    // o lance dela continua a valer.
    std::uint64_t dados_ant = r.dados.load(std::memory_order_relaxed);
    std::uint64_t xr_ant    = r.chave_xor.load(std::memory_order_relaxed);
    Move          antigo    = ((xr_ant ^ dados_ant) == chave)
                              ? Move(std::uint16_t(dados_ant & 0xFFFF))
                              : Move::none();

    std::uint64_t dados = empacota(PROF_SO_AVAL, 0, Limite::Nenhum, antigo, false, aval);
    r.dados.store(dados, std::memory_order_relaxed);
    r.chave_xor.store(chave ^ dados, std::memory_order_relaxed);
    r.ger.store(ger, std::memory_order_relaxed);
}

// [FONTE] Mil baldes chegam para uma estimativa; varrer a tabela inteira a cada
// `info` custava mais do que a informacao vale.
int TranspositionTable::cheia() const {
    if (!baldes)
        return 0;
    int usadas = 0, vistas = 0;
    for (std::size_t i = 0; i < 1000 && i <= mascara; ++i)
        for (const auto& r : baldes[i].via) {
            ++vistas;
            if (r.dados.load(std::memory_order_relaxed) != 0)
                ++usadas;
        }
    return vistas ? usadas * 1000 / vistas : 0;
}

// [ESCRITO] daqui para baixo.
void TranspositionTable::limpa() {
    if (baldes)
        std::memset(static_cast<void*>(baldes), 0, (mascara + 1) * sizeof(Balde));
    geracao.store(0, std::memory_order_relaxed);
}

void TranspositionTable::avanca_geracao() {
    geracao.fetch_add(1, std::memory_order_relaxed);
}

void* TranspositionTable::first_entry(std::uint64_t chave) const {
    return baldes ? static_cast<void*>(&baldes[chave & mascara]) : nullptr;
}

void TranspositionTable::redimensiona(std::size_t mb) {
    // Sem esta guarda, cada chamada libertava a tabela, realocava e chamava
    // `limpa` -- e o interface manda `setoption name Hash` mais do que uma vez.
    std::size_t quer = std::size_t(mb) * 1024 * 1024 / sizeof(Balde);
    std::size_t n = 1;
    while (n * 2 <= quer) n *= 2;
    if (baldes && n == mascara + 1) {
        limpa();
        return;
    }
    delete[] baldes;
    baldes  = new Balde[n];
    mascara = n - 1;
    limpa();
}

TranspositionTable tabela;

}  // namespace Kestrel
