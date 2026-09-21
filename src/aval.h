// The evaluator.
//
// A thin wrapper over the substrate's network: it loads the file, keeps the
// accumulator stack that the search pushes and pops, and applies this
// program's own scaling on top of the raw network output.
#ifndef KESTREL_AVAL_H
#define KESTREL_AVAL_H

#include <memory>
#include <string>

#include "nnue/network.h"
#include "nnue/nnue_accumulator.h"
#include "nnue/nnue_misc.h"
#include "position.h"
#include "types.h"

namespace Kestrel {

class Avaliador {
   public:
    Avaliador();

    bool  carrega(const std::string& caminho, std::string& erro);
    // A rede que veio dentro do executavel. Devolve falso quando se construiu
    // sem ela -- ai' o motor exige `EvalFile`, como antes.
    bool  carrega_embebida(std::string& erro);
    // O nome da rede que esta' a ser usada, para o `uci` o poder anunciar.
    static const char* nome_por_omissao();
    void  repoe();
    void  partes(const Position& pos, int& psqt, int& posicional);
    Value avalia_cheia(const Position& pos, int escala_x100, int otimismo = 0);
    Value avalia(const Position& pos);

    bool tem_rede = false;

    // A busca empurra e tira acumuladores a cada lance: `av->pilha().push()`.
    Eval::NNUE::AccumulatorStack& pilha() { return acumuladores; }

   private:
    Eval::NNUE::Network                              rede;
    Eval::NNUE::EvalFile                             ficheiro;
    Eval::NNUE::AccumulatorStack                     acumuladores;
    std::unique_ptr<Eval::NNUE::AccumulatorCaches>   caches;
};

}  // namespace Kestrel

#endif
