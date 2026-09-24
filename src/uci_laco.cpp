// The UCI loop.
//
// Deliberately thin: it parses, keeps the position, and calls the search. It
// prints neither `bestmove` nor `info depth` -- `Busca::arranca` does that,
// because only the search knows when an iteration is worth reporting.
#include <deque>
#include <iostream>
#include <algorithm>
#include <sstream>
#include <thread>
#include <string>

#include "aval.h"
#include "bitboard.h"
#include "busca.h"
#include "misc.h"
#include "movegen.h"
#include "position.h"
#include "syzygy/tbprobe.h"
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
int                    g_hash_mb = 64;
// O tecto dos fios sai da maquina e nao de um numero cravado: um `Threads` que
// aceita mais fios do que ha' nucleos so' serve para os por a disputar cache.
const int              FIOS_MAX = std::max(1, int(std::thread::hardware_concurrency()));
int                    g_fios   = 1;

// A BUSCA CORRE NUMA THREAD PROPRIA.
//
// Corria nesta, a que le' os comandos: enquanto buscava ninguem lia o `stdin`,
// o `stop` so' era lido quando ela ja' tinha acabado, e um `go infinite` nunca
// acabava. Nas partidas com relogio nao se notava -- o motor para sozinho pelo
// tempo --, mas o `stop` e' obrigatorio no protocolo, e sem ele nao ha' analise
// em interface nenhuma nem `ponder`.
std::thread            g_fio_busca;
bool                   g_busca_infinita = false;

// Espera que a busca em curso acabe, SEM a mandar parar.
void espera_busca() {
    if (g_fio_busca.joinable())
        g_fio_busca.join();
}

// Manda parar a busca em curso e espera por ela. Ela proprio escreve o
// `bestmove` antes de acabar.
void para_busca() {
    g_busca.parar.store(true, std::memory_order_relaxed);
    espera_busca();
}

// Uma linha escrita pela thread dos comandos, sob a mesma tranca da busca: o
// `readyok` pode chegar a meio de uma busca e nao pode cortar uma linha `info`.
void diz(const char* texto) {
    std::lock_guard<std::mutex> g(trinco_saida());
    std::cout << texto << std::endl;
}

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
        else if (tok == "movestogo") is >> lim.movestogo;
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
    // Os ajudantes nascem AQUI e nao no `setoption`: assim a rede ja' esta'
    // carregada, e e' dela que cada um tira as suas caches. Prepara-los antes
    // da rede dava ajudantes a avaliar tudo a zero.
    g_busca.prepara_fios(g_fios, g_aval);
    // O `parar` repoe-se AQUI, antes de a thread nascer. Se fosse a propria
    // busca a repo-lo, um `stop` que chegasse entre o nascimento dela e essa
    // linha perdia-se.
    g_busca.parar.store(false, std::memory_order_relaxed);
    g_busca_infinita = lim.infinito;
    g_fio_busca      = std::thread([lim] { g_busca.arranca(g_pos, lim, g_aval); });
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
        else {
            std::cout << "info string rede carregada: " << valor << std::endl;
            // Os ajudantes guardam um ponteiro para a rede do dono. Trocada a
            // rede, esse ponteiro fica a apontar para a antiga -- deitam-se
            // fora, e o proximo `go` reconstroi-os sobre a nova.
            g_busca.prepara_fios(1, g_aval);
        }
    } else if (nome == "hash") {
        g_hash_mb = std::max(1, std::atoi(valor.c_str()));
        g_busca.minha_tabela()->redimensiona(std::size_t(g_hash_mb));
    } else if (nome == "syzygypath") {
        // AS TABLEBASES ESTAVAM COMPILADAS E INALCANCAVEIS.
        //
        // A busca ja' as sonda -- `probe_wdl`, `MaxCardinality`, o contador de
        // acertos, tudo la'. So' que ninguem chamava `Tablebases::init`, e sem
        // essa chamada o `MaxCardinality` fica a zero e o codigo nunca dispara.
        // O motor anunciava tablebases que nao tinha maneira de receber.
        //
        // A configuracao de producao do bot passava
        // `SyzygyPath=/home/kestrel/syzygy` por UCI; esta opcao era o caminho
        // por onde isso entrava.
        Tablebases::init(valor);
        if (Tablebases::MaxCardinality > 0)
            std::cout << "info string tablebases ate' " << Tablebases::MaxCardinality
                      << " pecas" << std::endl;
        else
            std::cout << "info string sem tablebases em " << valor << std::endl;
    } else if (nome == "move overhead") {
        // Quanto se guarda de cada orcamento para o que nao e' pensar: o
        // arbitro, a rede, o processo. Cravado a 30 e nao lido; a producao
        // corria a 200, e enganar-se aqui e' assimetrico -- a mais custa um
        // pouco de forca, a menos custa partidas inteiras pela bandeira.
        g_busca.sobrecarga_ms = std::clamp(std::atoi(valor.c_str()), 0, 5000);
    } else if (nome == "threads") {
        g_fios = std::clamp(std::atoi(valor.c_str()), 1, FIOS_MAX);
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
        // Os tres comandos que o protocolo deixa chegar A MEIO de uma busca.
        if (tok == "isready") {
            diz("readyok");
            continue;
        }
        if (tok == "stop") {
            para_busca();
            continue;
        }
        if (tok == "quit") {
            para_busca();
            break;
        }
        // Qualquer outro mexe no que a busca esta' a usar -- a posicao, a
        // tabela, a rede, os parametros. Espera-se que ela acabe primeiro.
        espera_busca();

        if (tok == "uci") {
            std::cout << "id name KestrelStrike " KS_VERSAO "\n"
                      << "id author Joao\n"
                      << "option name EvalFile type string default "
                      << Avaliador::nome_por_omissao() << "\n"
                      << "option name Hash type spin default 64 min 1 max 65536\n"
                      << "option name SyzygyPath type string default <empty>\n"
                      << "option name Move Overhead type spin default 30 min 0 max 5000\n"
                      << "option name Threads type spin default 1 min 1 max "
                      << FIOS_MAX << "\n"
                      << "uciok" << std::endl;
        } else if (tok == "ucinewgame") {
            g_busca.nova_partida();
            g_aval.repoe();
        } else if (tok == "setoption") {
            faz_setoption(is);
        } else if (tok == "position") {
            faz_position(is);
        } else if (tok == "go") {
            faz_go(is);
        }
    }
    // FIM DA ENTRADA. Nao e' um `quit`: quem fecha o cano depois de um
    // `go depth N` quer a busca inteira -- e' assim que se mede a profundidade
    // fixa. Uma busca finita acaba sozinha; so' a infinita se manda parar, senao
    // o processo ficava vivo para sempre sem ninguem a ouvir.
    if (g_busca_infinita)
        para_busca();
    else
        espera_busca();
    return 0;
}
