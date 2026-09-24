// The transposition table.
//
// Three ways to a bucket, each slot 24 bytes: the XOR-folded key, the packed
// data, and the generation. Storing `key ^ data` rather than the key means a
// torn read fails the comparison instead of being taken for a hit.
#include "tt.h"

#include <atomic>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <new>
#if defined(__linux__)
#include <sys/mman.h>
#endif
#if defined(_WIN32)
#include <malloc.h>
#endif

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
};
static constexpr std::size_t VIAS = 3;
// O BALDE CABE NUMA LINHA DE CACHE.
//
// A geracao vivia dentro de cada `Ranhura`, ao lado dos dois `uint64`: 17 bytes
// que o alinhamento arredondava para 24, e tres vias davam um balde de 72. Isso
// custava duas coisas, e o binario de producao tinha as duas -- nao e' trabalho
// perdido a recuperar, e' o desenho original:
//
//  1. **So' se usava 56% da memoria pedida.** O `redimensiona` arredonda o numero
//     de baldes a uma potencia de 2, e com 72 bytes a maior que cabe fica sempre
//     em 56,25% do pedido: `Hash=64` usava 36 MB, `Hash=16` usava 9 MB. Em
//     qualquer tamanho.
//  2. **Cada sondagem tocava duas linhas de cache**, as vezes tres -- 72 bytes
//     atravessam sempre uma fronteira de 64. O `prefetch` so' traz uma.
//
// Com a geracao num vector a parte no fim do balde: 3 x 16 + 3 = 51 bytes, que
// o `alignas(64)` fecha em 64 certos. As mesmas tres vias, os mesmos campos, a
// mesma logica de substituicao -- muda so' ONDE esta' cada byte. Com o mesmo
// numero de baldes a arvore sai identica ao no'; com o mesmo `Hash` passa a
// haver o dobro dos baldes, que e' o que o `Hash` sempre prometeu.
//
// MEDIDO EM PARTIDAS, as tres pecas de velocidade juntas -- este balde, as
// paginas grandes e o prefetch antes do lance -- contra `7c25e1c` construido limpo:
// **+19,14 +/- 7,66 Elo** em 2108 partidas a 5+0,05, `Hash=16`, um fio, LLR 2,95,
// H1 aceite. Contagem propria do PGN, trinomial: IC95% [+8,9; +29,4].
// A parte deste balde, pela velocidade com arvores identicas: -5,6% de tempo.
// E a parte da capacidade, que essa medida nao ve: a `Hash=16` o motor antigo
// usava 9 MB e este usa 16.
struct alignas(64) Balde {
    Ranhura                   via[VIAS];
    std::atomic<std::uint8_t> ger[VIAS];
};
static_assert(sizeof(Balde) == 64, "o balde tem de ocupar exactamente uma linha de cache");

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
        b.ger[i].store(ger, std::memory_order_relaxed);
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
        int velhice = std::uint8_t(ger - b.ger[i].load(std::memory_order_relaxed));
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
// Paginas grandes para a tabela. Ver `aloca_baldes`. 0 = paginas normais.
const int G_TT_GRANDE = [] { const char* v = std::getenv("KS_TT_GRANDE"); return v ? std::atoi(v) : 1; }();
}  // namespace

void TranspositionTable::guarda(std::uint64_t chave, int prof, int nota, Limite limite,
                                Move melhor, bool pv, std::int16_t aval) {
    if (!baldes)
        return;
    Balde&        b   = baldes[chave & mascara];
    std::uint8_t  ger = geracao.load(std::memory_order_relaxed);
    std::size_t   k   = via_a_substituir(b, chave, ger);
    Ranhura&      r   = b.via[k];

    if (G_TT_LANCE || G_TT_PROF) {
        std::uint64_t antigos = r.dados.load(std::memory_order_relaxed);
        std::uint64_t xr      = r.chave_xor.load(std::memory_order_relaxed);
        bool          mesma   = antigos != 0 && (xr ^ antigos) == chave;

        if (G_TT_LANCE && mesma && melhor == Move::none())
            melhor = Move(std::uint16_t(antigos & 0xFFFF));

        if (G_TT_PROF && mesma) {
            int velhice = std::uint8_t(ger - b.ger[k].load(std::memory_order_relaxed));
            if (!(limite == Limite::Exacto || prof + 2 * int(pv) > prof_de(antigos) - 4
                  || velhice != 0))
                return;
        }
    }

    std::uint64_t dados = empacota(prof, nota, limite, melhor, pv, aval);
    r.dados.store(dados, std::memory_order_relaxed);
    r.chave_xor.store(chave ^ dados, std::memory_order_relaxed);
    b.ger[k].store(ger, std::memory_order_relaxed);
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
        int velhice = std::uint8_t(ger - b.ger[i].load(std::memory_order_relaxed));
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
    b.ger[alvo].store(ger, std::memory_order_relaxed);
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

// PAGINAS GRANDES.
//
// Uma tabela de 64 MB em paginas de 4 KB sao 16.384 paginas -- muito mais do
// que a TLB guarda. Cada sondagem a um balde qualquer paga entao duas idas a`
// memoria em vez de uma: a do balde e a da traducao do endereco. Em paginas de
// 2 MB os mesmos 64 MB sao 32 paginas, e a traducao fica sempre na TLB.
//
// Este sistema tem o THP em `madvise`: as paginas grandes so' se dao a` memoria
// marcada com `madvise(MADV_HUGEPAGE)`. A tabela alocava-se com um `new` simples
// e portanto NUNCA as recebia. O Stockfish aloca a dele assim de proposito.
//
// E' codigo nosso e nao o alocador do substrato, porque a tabela e' nossa. No
// Windows fica o alinhamento a` linha de cache: la' as paginas grandes pedem um
// privilegio que quase ninguem tem ligado, e o ganho perde-se de qualquer modo.
//
// MEDIDO EM PARTIDAS, as tres pecas de velocidade juntas -- este balde, as
// paginas grandes e o prefetch antes do lance -- contra `7c25e1c` construido limpo:
// **+19,14 +/- 7,66 Elo** em 2108 partidas a 5+0,05, `Hash=16`, um fio, LLR 2,95,
// H1 aceite. Contagem propria do PGN, trinomial: IC95% [+8,9; +29,4].
// A parte das paginas grandes, pela velocidade: -5,8% de tempo. So' no Linux.
namespace {
Balde* aloca_baldes(std::size_t n, std::size_t& bytes_out) {
    std::size_t alinha = alignof(Balde);
#if defined(__linux__)
    constexpr std::size_t DOIS_MB = std::size_t(2) * 1024 * 1024;
    if (G_TT_GRANDE)
        alinha = DOIS_MB;
#endif
    // O `aligned_alloc` exige um tamanho multiplo do alinhamento.
    std::size_t bytes = (n * sizeof(Balde) + alinha - 1) / alinha * alinha;
#if defined(_WIN32)
    void* mem = _aligned_malloc(bytes, alinha);
#else
    void* mem = std::aligned_alloc(alinha, bytes);
#endif
    if (!mem)
        return nullptr;
#if defined(__linux__) && defined(MADV_HUGEPAGE)
    if (G_TT_GRANDE)
        madvise(mem, bytes, MADV_HUGEPAGE);
#endif
    Balde* b = static_cast<Balde*>(mem);
    for (std::size_t i = 0; i < n; ++i)
        new (&b[i]) Balde();
    bytes_out = bytes;
    return b;
}

void liberta_baldes(Balde* b) {
    if (!b)
        return;
#if defined(_WIN32)
    _aligned_free(b);
#else
    std::free(b);
#endif
}
}  // namespace

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
    liberta_baldes(baldes);
    std::size_t bytes = 0;
    baldes = aloca_baldes(n, bytes);
    // Sem memoria, a tabela fica vazia em vez de o processo morrer: todas as
    // funcoes daqui ja' sabem o que fazer com `baldes == nullptr`.
    mascara = baldes ? n - 1 : 0;
    limpa();
}

TranspositionTable tabela;

}  // namespace Kestrel
