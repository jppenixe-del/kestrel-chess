// The UCI loop.
//
// Deliberately thin: it parses, keeps the position, and calls the search. It
// prints neither `bestmove` nor `info depth` -- `Busca::arranca` does that,
// because only the search knows when an iteration is worth reporting.
#include <deque>
#include <iostream>
#include <sstream>
#include <string>

#include "aval.h"
#include "bitboard.h"
#include "busca.h"
#include "misc.h"
#include "movegen.h"
#include "position.h"
#include "tt.h"
#include "uci.h"

#ifndef KS_VERSAO
    #define KS_VERSAO "1.0"
#endif

using namespace Kestrel;

namespace {

Position               g_pos;
std::deque<StateInfo>  g_pilha;
Busca                  g_busca;
Avaliador              g_aval;
int                    g_hash_mb = 16;

const std::string INICIAL = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

// Um lance em notacao UCI so' se traduz contra a posicao: `e1g1` pode ser rei
// para g1 ou roque, e so' a posicao sabe qual.
Move le_lance(const Position& pos, const std::string& s) {
    for (const auto& m : MoveList<LEGAL>(pos))
        if (s == UCIEngine::move(Move(m), false))
            return Move(m);
    return Move::none();
}

void faz_position(std::istringstream& is) {
    std::string tok, fen;
    if (!(is >> tok))
        return;
    if (tok == "startpos") {
        fen = INICIAL;
        is >> tok;  // consome o "moves", se vier
    } else if (tok == "fen") {
        while (is >> tok && tok != "moves")
            fen += tok + " ";
    } else {
        return;
    }

    g_pilha.clear();
    g_pilha.emplace_back();
    g_pos.set(fen, false, &g_pilha.back());
    g_busca.chaves_jogo.clear();
    g_busca.pre_n = 0;

    if (tok == "moves" || tok == "startpos") {
        while (is >> tok) {
            Move m = le_lance(g_pos, tok);
            if (m == Move::none())
                break;
            // A chave ANTES do lance: e' a posicao que pode voltar a aparecer.
            g_busca.chaves_jogo.push_back(g_pos.key());
            // E o lance, para a historia de continuacao poder olhar para tras
            // ALEM da raiz. Guardado do mais recente para o mais antigo.
            for (int k = 5; k > 0; --k) {
                g_busca.pre_pc[k]   = g_busca.pre_pc[k - 1];
                g_busca.pre_para[k] = g_busca.pre_para[k - 1];
            }
            g_busca.pre_pc[0]   = int(type_of(g_pos.moved_piece(m))) - 1;
            g_busca.pre_para[0] = int(m.to_sq());
            if (g_busca.pre_n < 6)
                ++g_busca.pre_n;
            g_pilha.emplace_back();
            g_pos.do_move(m, g_pilha.back());
        }
    }
}

void faz_go(std::istringstream& is) {
    Limites lim;
    std::string tok;
    while (is >> tok) {
        if      (tok == "wtime")     is >> lim.tempo[WHITE];
        else if (tok == "btime")     is >> lim.tempo[BLACK];
        else if (tok == "winc")      is >> lim.inc[WHITE];
        else if (tok == "binc")      is >> lim.inc[BLACK];
        else if (tok == "movetime")  is >> lim.movetime;
        else if (tok == "depth")     is >> lim.profundidade;
        else if (tok == "nodes")     is >> lim.nos;
        else if (tok == "infinite")  lim.infinito = true;
    }
    if (!g_aval.tem_rede) {
        std::cout << "info string ERRO: sem rede -- este executavel foi "
                     "construido sem rede embebida; use `setoption name EvalFile "
                     "value <ficheiro>`. Sem rede a avaliacao seria zero em toda "
                     "a parte." << std::endl;
        std::cout << "bestmove 0000" << std::endl;
        return;
    }
    g_busca.arranca(g_pos, lim, g_aval);
}

void faz_setoption(std::istringstream& is) {
    std::string tok, nome, valor;
    is >> tok;  // "name"
    while (is >> tok && tok != "value")
        nome += (nome.empty() ? "" : " ") + tok;
    while (is >> tok)
        valor += (valor.empty() ? "" : " ") + tok;
    for (auto& c : nome) c = char(std::tolower(c));

    if (nome == "evalfile") {
        std::string erro;
        // A rede e' verificada AQUI: um motor que morre a arrancar nao
        // consegue dizer ao arbitro o que lhe falta.
        if (!g_aval.carrega(valor, erro))
            std::cout << "info string ERRO: " << erro << std::endl;
        else
            std::cout << "info string rede carregada: " << valor << std::endl;
    } else if (nome == "hash") {
        g_hash_mb = std::max(1, std::atoi(valor.c_str()));
        g_busca.minha_tabela()->redimensiona(std::size_t(g_hash_mb));
    } else if (nome == "threads") {
        // Uma so', por agora: o Lazy SMP do KestrelStrike ainda nao foi
        // recuperado. Anunciar mais do que se faz e' o defeito que ja' nos
        // custou uma noite -- o `Threads` ficou anunciado e nunca lido, e o
        // paralelismo estava morto sem ninguem dar por isso.
        std::cout << "info string Threads=" << valor
                  << " pedidas; esta versao corre a UMA." << std::endl;
    }
}

}  // namespace

int main() {
    Attacks::init();
    Position::init();
    g_busca.minha_tabela()->redimensiona(std::size_t(g_hash_mb));
    g_busca.nova_partida();

    g_pilha.emplace_back();
    g_pos.set(INICIAL, false, &g_pilha.back());

    // A REDE EMBEBIDA, ao arrancar. Assim o motor joga sem configuracao
    // nenhuma: e' o que a CCRL espera de um executavel e o que evita que
    // alguem o corra com a rede de outro motor por engano. O `EvalFile`
    // continua a funcionar e substitui esta.
    {
        std::string erro;
        if (g_aval.carrega_embebida(erro))
            std::cout << "info string rede embebida: " << Avaliador::nome_por_omissao()
                      << std::endl;
    }

    std::string linha;
    while (std::getline(std::cin, linha)) {
        std::istringstream is(linha);
        std::string tok;
        if (!(is >> tok))
            continue;
        if (tok == "uci") {
            std::cout << "id name KestrelStrike " KS_VERSAO "\n"
                      << "id author Joao\n"
                      << "option name EvalFile type string default "
                      << Avaliador::nome_por_omissao() << "\n"
                      << "option name Hash type spin default 16 min 1 max 65536\n"
                      << "option name Threads type spin default 1 min 1 max 1\n"
                      << "uciok" << std::endl;
        } else if (tok == "isready") {
            std::cout << "readyok" << std::endl;
        } else if (tok == "ucinewgame") {
            g_busca.nova_partida();
            g_aval.repoe();
        } else if (tok == "setoption") {
            faz_setoption(is);
        } else if (tok == "position") {
            faz_position(is);
        } else if (tok == "go") {
            faz_go(is);
        } else if (tok == "quit" || tok == "stop") {
            if (tok == "quit") break;
        }
    }
    return 0;
}
