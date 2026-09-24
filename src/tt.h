// The transposition table.
//
// Packed into 64 bits per slot plus an XOR-folded key, three ways to a bucket.
// The layout is documented next to `empacota()` in tt.cpp.
#ifndef KESTREL_TT_H
#define KESTREL_TT_H

#include <atomic>
#include <cstdint>
#include <cstdlib>

#include "types.h"

namespace Kestrel {

enum class Limite : std::uint8_t { Nenhum = 0, Inferior = 1, Superior = 2, Exacto = 3 };

// "Nao ha' avaliacao guardada nesta entrada." Usado nas entradas escritas em
// xeque, onde nao se avalia.
constexpr int TT_SEM_AVAL = 32002;

struct Entrada {
    std::uint64_t chave  = 0;
    int           prof   = 0;
    int           nota   = 0;
    Limite        limite = Limite::Nenhum;
    Move          melhor = Move::none();
    // Esta entrada foi escrita por uma busca com janela inteira? Uma sondagem
    // posterior que so' la' chegue por janela nula continua a saber que aquela
    // posicao ja' foi importante uma vez -- e reduz-se menos por causa disso.
    bool          pv     = false;
    // A estatica CRUA da posicao, sem correccao. A correccao e' de quem le':
    // a tabela e' partilhada e cada um aplica a sua.
    std::int16_t  aval   = TT_SEM_AVAL;

    // [FONTE] Usado em toda a busca: uma entrada sem limite traz avaliacao mas
    // nao traz veredicto, e nao serve para cortar.
    bool tem_limite() const { return limite != Limite::Nenhum; }
};

struct Balde;

class TranspositionTable {
   public:
    bool sonda(std::uint64_t chave, Entrada& e) const;
    void guarda(std::uint64_t chave, int prof, int nota, Limite limite, Move melhor, bool pv,
                std::int16_t aval);
    // A `guarda` normal escolhe a pior via do balde e escreve la'. Esta so'
    // poe a avaliacao, preservando o lance que a via ja' tivesse desta chave.
    void guarda_so_aval(std::uint64_t chave, std::int16_t aval);
    void limpa();
    int  cheia() const;
    void avanca_geracao();
    void redimensiona(std::size_t mb);
    void* first_entry(std::uint64_t chave) const;

   private:
    // Tres vias num balde de 64 bytes -- uma linha de cache. Eram 72 (tres
    // ranhuras de 24) ate' a geracao sair de dentro de cada ranhura; ver a nota
    // no `Balde`, em tt.cpp.
    std::size_t                via_a_substituir(Balde& b, std::uint64_t chave,
                                                std::uint8_t ger) const;
    Balde*                     baldes  = nullptr;
    std::size_t                mascara = 0;
    std::atomic<std::uint8_t>  geracao{0};
};

// The global table. The search reaches it through `p_tab`, a pointer, so that
// ponteiro `p_tab->`, que e' a mudanca que o TT partilhado do Lazy SMP obriga.
// Ficam as duas: o ponteiro aponta para esta.
extern TranspositionTable tabela;

}  // namespace Kestrel

#endif
