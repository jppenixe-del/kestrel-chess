#include "aval.h"

#include <filesystem>
#include <system_error>

#include "evaluate.h"
#include "nnue/nnue_misc.h"

namespace Kestrel {

Avaliador::Avaliador() = default;

bool Avaliador::carrega(const std::string& caminho, std::string& erro) {
    tem_rede = false;
    // O ficheiro e' verificado AQUI e nao la' dentro: o substrato desiste do
    // processo quando nao encontra a rede, e um motor que morre a arrancar nao
    // consegue dizer ao arbitro o que lhe falta. Perguntar primeiro deixa-nos
    // recusar com uma frase em vez de desaparecer.
    std::error_code ec;
    if (!std::filesystem::exists(std::filesystem::path(caminho), ec)) {
        erro = "nao encontrei a rede em " + caminho;
        return false;
    }
    // NAO se preenche o `current` antes de chamar. Ele e' a SAIDA -- diz o que
    // ficou carregado -- e o `load` do substrato so' vai ao disco se
    // `current != caminho pedido`. Preenche-lo antes faz o carregamento ser
    // saltado por inteiro, calado, e a rede fica a zeros: procura-se com uma
    // avaliacao constante e nada levanta erro. Custou-me uma tarde a perceber
    // que a rede nunca chegava a ser lida.
    ficheiro.current.reset();
    rede.load(std::filesystem::path("."), std::filesystem::path(caminho), ficheiro);
    // A `load` do substrato falha CALADA: devolve sem descricao e deixa os
    // pesos a zero, e uma rede a zeros avalia tudo a zero sem levantar erro
    // nenhum. E' preciso perguntar-lhe se ela ficou mesmo la'.
    if (ficheiro.netDescription.empty()) {
        erro = "a rede " + caminho + " nao foi aceite (arquitectura ou ficheiro truncado)";
        return false;
    }
    caches   = std::make_unique<Eval::NNUE::AccumulatorCaches>(rede);
    tem_rede = true;
    return true;
}

bool Avaliador::carrega_embebida(std::string& erro) {
#ifdef KESTREL_REDE_EMBEBIDA
    tem_rede = false;
    // Mesma armadilha da `carrega`: o `current` e' a SAIDA. Se ficar preenchido
    // de uma tentativa anterior, o substrato salta o carregamento calado e a
    // rede fica a zeros.
    ficheiro.current.reset();
    rede.load_internal(ficheiro);
    // E a mesma verificacao: a `load` falha sem dizer nada e deixa os pesos a
    // zero. Uma rede a zeros avalia tudo a zero e nada levanta erro.
    if (ficheiro.netDescription.empty()) {
        erro = "a rede embebida nao foi aceite";
        return false;
    }
    caches   = std::make_unique<Eval::NNUE::AccumulatorCaches>(rede);
    tem_rede = true;
    return true;
#else
    erro = "este executavel foi construido sem rede embebida";
    return false;
#endif
}

const char* Avaliador::nome_por_omissao() {
#ifdef KESTREL_REDE_EMBEBIDA
    return EvalFileDefaultName;
#else
    return "<vazio>";
#endif
}

void Avaliador::repoe() { acumuladores.reset(); }

void Avaliador::partes(const Position& pos, int& psqt, int& posicional) {
    psqt = posicional = 0;
    if (!tem_rede || pos.checkers())
        return;
    auto [a, b] = rede.evaluate(pos, acumuladores, *caches);
    psqt = int(a); posicional = int(b);
}

// A avaliacao COMPLETA deles, com a nossa escala por cima.
//
// A `Eval::evaluate` nao devolve a rede: devolve a rede depois de quatro
// camadas que NAO estao dentro dela --
//
//   nnueComplexity = |psqt - positional|          discordancia das duas cabecas
//   nnue -= nnue * complexity / 18236             desconfia quando discordam
//   v = nnue + (nnue*material + optimism*7675)/91000   escala pelo material
//   v -= v * rule50 / 199                         amortece a arrastar
//
// Tiramo-las porque mudavam a ESCALA -- a estatica saia 1,24x a 1,50x maior --
// e as margens da busca foram varridas na escala do half2k. Mas a escala e a
// FORMA sao coisas diferentes: o que interessa daqui e' a forma (o valor encolhe
// no final, encolhe quando as cabecas discordam, encolhe a arrastar), e a escala
// devolve-se com uma constante. E' isso que este caminho faz.
//
// `optimism` vai a zero: o valor deles vem do estado da raiz da busca e nos nao
// temos equivalente. Inventar um seria acrescentar uma quinta diferenca a uma
// medida que existe para isolar quatro.
Value Avaliador::avalia_cheia(const Position& pos, int escala_x100, int otimismo) {
    if (pos.checkers() || !tem_rede)
        return VALUE_ZERO;
    Value v = Eval::evaluate(rede, pos, acumuladores, *caches, otimismo);
    return Value(int(v) * escala_x100 / 100);
}

Value Avaliador::avalia(const Position& pos) {
    // Em xeque a estatica nao quer dizer nada e o substrato nem a calcula.
    // Quem chama tem de perguntar primeiro; se chegou aqui em xeque, e' um
    // erro nosso e vale mais devolver um zero visivel do que ler lixo.
    if (pos.checkers())
        return VALUE_ZERO;
    if (!tem_rede)
        return VALUE_ZERO;
    // O numero CRU da rede: `psqt + posicional`, e mais nada.
    //
    // O `Eval::evaluate` do substrato poe por cima disto uma escala por material
    // (`v = nnue + nnue*material/91000`), um ajuste pela complexidade e um
    // amortecimento pela regra dos cinquenta. O half2k NAO faz nada disso -- o
    // leitor dele devolve `saida*escala + psqt` e pronto.
    //
    // MEDIDO, e e' a razao de isto estar aqui escrito: com o `Eval::evaluate` a
    // estatica saia 1,24x a 1,50x maior que a do half2k (inicial 25 contra 17,
    // kiwipete -467 contra -311). Todas as margens da busca -- 110 por ply na
    // futilidade inversa, 348 no razoring, 100+150 na futilidade dos tranquilos
    // -- foram varridas nas unidades DELE. Dar-lhes uma avaliacao 40% maior
    // aperta cada margem em 40% sem ninguem dar por isso, e a busca poda noutro
    // sitio. Era esta a diferenca de fundo entre os dois motores.
    // E vem noutra ESCALA DE SAIDA. O leitor do half2k e este nao usam as mesmas
    // constantes de quantizacao, e a diferenca foi RESOLVIDA e nao adivinhada.
    //
    // Com os dois termos impressos em separado, quatro posicoes chegam para a
    // determinar (P = psqt, Q = posicional, H = o valor do half2k):
    //
    //     P=   0  Q=  20  ->  H=  17
    //     P=-161  Q=-205  ->  H= -311
    //     P= 131  Q=-334  ->  H= -172
    //     P=  15  Q= 159  ->  H=  147
    //
    // A primeira da' 17/20 = 0,85; as outras tres batem certo com 0,85*(P+Q) ao
    // ponto -- -311,1, -172,6, 147,9. O mesmo factor nos DOIS termos, portanto
    // e' escala de saida e nao uma mistura dos dois.
    //
    // Isto importa porque TODAS as margens da busca -- 110 por ply na futilidade
    // inversa, 348 no razoring, 100+150 na dos tranquilos -- foram varridas nas
    // unidades do half2k. Uma avaliacao 18% maior aperta cada uma delas em 18%
    // sem ninguem dar por isso.
    auto [psqt, posicional] = rede.evaluate(pos, acumuladores, *caches);
    return Value((int(psqt) + int(posicional)) * 85 / 100);
}

}  // namespace Kestrel
