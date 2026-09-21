// Os contadores do acumulador. NOSSOS, num ficheiro NOSSO.
//
// Isto vivia enxertado dentro do `nnue/nnue_accumulator.cpp` deles, e por isso
// desapareceu quando o vendor foi posto ao nivel da origem a 20-09-2026: a
// actualizacao copia o ficheiro deles por cima e leva o que la' tivessemos
// metido. Passa a viver aqui para a proxima actualizacao nao lhe tocar.
//
// O que mediam: uma actualizacao incremental corre UMA VEZ POR PLY da distancia
// entre o ultimo acumulador calculado e a posicao a avaliar. Uma busca que
// avalia todos os nos mantem essa distancia em um; uma que salta avaliacoes --
// porque a nota ja' estava em cache, ou porque estava em xeque -- paga a
// distancia inteira na avaliacao seguinte. A distancia media e' o que decide se
// o custo do acumulador e' inerente ou auto-infligido, e isso nao se le' num
// perfil: o perfil mostra o total, nunca o comprimento.
//
// ATENCAO, e' preciso dize-lo com clareza: os pontos que INCREMENTAVAM estes
// contadores estavam dentro do codigo deles e NAO foram repostos. A funcao
// existe e imprime, mas imprime zeros ate' alguem voltar a tecer as chamadas ao
// `marca()` nos sitios certos. Ficou assim de proposito -- e' um diagnostico
// que so' liga com `H2K_CONTA` no ambiente, e repo-lo custava mexer outra vez
// no ficheiro deles, que e' exactamente o que causou este problema.
#include <array>
#include <atomic>
#include <cstdint>
#include <cstdio>
#include <cstdlib>

namespace Kestrel {
namespace Eval {
namespace NNUE {

void imprime_conta_h2k();

namespace h2k {

std::array<std::atomic<uint64_t>, 5> conta{};

// Estatico de funcao, e nao de espaco de nomes: este objecto vem de um arquivo
// e o inicializador estatico dele nao chegou a correr quando o ligador o
// puxou. Assim a leitura do ambiente acontece na primeira utilizacao, que
// acontece sempre.
bool ligado() {
    static const bool v = [] {
        const bool on = std::getenv("H2K_CONTA") != nullptr;
        if (on)
            std::atexit(&::Kestrel::Eval::NNUE::imprime_conta_h2k);
        return on;
    }();
    return v;
}

void marca(int i, uint64_t n) {
    if (ligado())
        conta[i].fetch_add(n, std::memory_order_relaxed);
}

}  // namespace h2k

void imprime_conta_h2k() {
    if (!h2k::ligado())
        return;
    static const char* nm[5] = {"avaliacoes", "saltos brancas", "saltos pretas", "reconstrucoes",
                                "hibridos"};
    const uint64_t     av    = h2k::conta[0].load(std::memory_order_relaxed);
    std::fprintf(stderr, "\n  --- acumulador ---\n");
    for (int i = 0; i < 5; i++)
    {
        const uint64_t v = h2k::conta[i].load(std::memory_order_relaxed);
        std::fprintf(stderr, "  %-16s %12llu", nm[i], (unsigned long long) v);
        if (i > 0 && av)
            std::fprintf(stderr, "   %6.3f por avaliacao", double(v) / double(av));
        std::fprintf(stderr, "\n");
    }
}

}  // namespace NNUE
}  // namespace Eval
}  // namespace Kestrel
