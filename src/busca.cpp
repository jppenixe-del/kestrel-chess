// A busca, transcrita da nossa `search.rs`.
//
// Nada aqui e' invencao nova: cada bloco corresponde a um bloco de la', com o
// mesmo comentario a dizer porque' existe e o que foi medido nele. Onde uma peca
// ainda nao veio, esta' escrito FALTA -- uma peca que falta tem de se ver.
#include <atomic>
#include <vector>
#include <algorithm>

// Contadores do acumulador, definidos no vendor/nnue/nnue_accumulator.cpp em
// ESCOPO GLOBAL. Declarados aqui fora do `namespace Kestrel` de proposito: la'
// dentro o `extern` procurava `Kestrel::g_conta_apply` e nao ligava.
extern std::atomic<unsigned long long> g_conta_apply;
extern std::atomic<unsigned long long> g_conta_idx;
extern std::atomic<unsigned long long> g_conta_refresh;

#include "busca.h"

#include <algorithm>
#include <cstdio>
#include <cmath>
#include <cstdlib>
#include <cstring>

#include <iostream>

#include "movegen.h"
#include "syzygy/tbprobe.h"
#include "uci.h"

namespace Kestrel {

// A tabela de Zobrist do substrato. Esta' definida em `vendor/position.cpp` e
// nao aparece em cabecalho nenhum -- mas tem ligacao externa, portanto declara-se
// aqui em vez de se mexer no `vendor/`. Serve para tirar o rei do `nonPawnKey`,
// que o traz e o nosso canal de forca nao quer. Ver `indices_rapido`.
namespace Zobrist {
extern Key psq[PIECE_NB][SQUARE_NB];
}

std::mutex& trinco_saida() {
    static std::mutex m;
    return m;
}

// QUANTO E' QUE A BUSCA DESMENTE A ESTATICA, por profundidade.
//
// E' isto que a margem da futilidade inversa tem de cobrir: ela diz "estou tao
// a` frente que, mesmo dando `margem`, ainda bato o beta". Se a busca nunca
// recupera mais do que X em `prof` plies, uma margem acima de X e' segura.
//
// A da referencia satura em 85 por ply porque a arvore DELES tem retornos
// decrescentes. A nossa e' outra arvore. Este coletor mede a nossa em vez de
// lhes copiar o numero.
namespace {
constexpr int EST_PROF = 20;
std::vector<int> g_desmente[EST_PROF];
// A FORMA da arvore: quantos nos em cada profundidade restante.
//
// Melhor instrumento do que o total. O total diz "somos 4x"; isto diz ONDE.
// O Triumviratus mediu assim e desmentiu uma crença de tres meses do proprio
// projecto: "73.5% of our nodes sit at depth 1-3 against 58.4%, and 0.3% get
// past depth 14 against 2.8%".
std::uint64_t g_forma[64];
}
std::uint64_t g_corr_n = 0, g_corr_zero = 0, g_corr_soma = 0;
namespace {
// A QUALIDADE DA ORDENACAO, num numero: de todos os nos que cortaram, quantos
// cortaram no PRIMEIRO lance.
//
// E' a medida que julga a geracao da lista inteira. Ela nao existe para ser
// rapida -- a por etapas e' mais rapida, porque para' assim que corta. Existe
// para ordenar com a lista toda na mao. Se isso vale, tem de aparecer aqui.
std::uint64_t g_cortes, g_cortes_1;
std::uint64_t g_forma_qs;
}

namespace {
// Os interruptores de diagnostico lem-se UMA VEZ.
//
// Estavam a ser lidos com `getenv` dentro do ciclo dos lances -- uma consulta ao
// ambiente por tranquilo pontuado e duas por verificacao singular. E' trabalho
// posto no sitio mais quente da busca por causa de instrumentacao, que e'
// exactamente o erro que a instrumentacao devia evitar.
struct Diag {
    bool sem_pre, bonus_antigo, sem_capt_pen, ordem_escalada, sing_sem_corte, sing_conta;
    // Desligam o lance nulo e a IIR. Sao os dois que faltavam para se poder
    // fazer a mesma radiografia que se faz a` referencia: sem eles so' sabiamos
    // medir os mecanismos pequenos.
    bool sem_null, sem_iir;
    // O termo da profundidade na reducao do lance nulo, e o lance nulo fora dos
    // nos de corte -- que e' o que a referencia faz.
    bool sem_nmp_prof, nmp_todos;
    bool forma;
    // QUEM PESA NA REDUCAO: conta, por termo, quantas vezes dispara e quanto
    // vale em MODULO. E' a radiografia que mostra se a reducao escolhe o lance
    // ou se reduz tudo por igual.
    bool quem;
    // OS QUATRO GRANDES, sem interruptor ate' agora. Nao e' para os ligar ou
    // desligar em producao -- e' para se poder perguntar o que cada um vale,
    // que e' a unica forma de saber o que a busca esta' mesmo a fazer.
    bool sem_lmr, sem_corr, sem_asp, sem_ext;
    // O estudo da margem: quanto e' que a busca desmente a estatica, por ply.
    bool margem_estudo;
    // A re-busca proporcional (`RebuscaFina` no half2k).
    bool reb_fina;
    // A avaliacao completa do substrato, com escala nossa por cima.
    bool eval_crua;
    int  eval_escala;
    Diag()
        : sem_pre(std::getenv("KS_SEM_PRE") != nullptr),
          bonus_antigo(std::getenv("KS_BONUS_ANTIGO") != nullptr),
          sem_capt_pen(std::getenv("KS_SEM_CAPT_PEN") != nullptr),
          ordem_escalada(std::getenv("KS_ORDEM_ESCALADA") != nullptr),
          sing_sem_corte(std::getenv("KS_SING_SEM_CORTE") != nullptr),
          sem_null(std::getenv("KS_OFF_NULL") != nullptr),
          sem_iir(std::getenv("KS_OFF_IIR") != nullptr),
          sem_nmp_prof(std::getenv("KS_SEM_NMP_PROF") != nullptr),
          forma(std::getenv("KS_FORMA") != nullptr),
          quem(std::getenv("KS_QUEM") != nullptr),
          sem_lmr(std::getenv("KS_OFF_LMR") != nullptr),
          sem_corr(std::getenv("KS_OFF_CORR") != nullptr),
          sem_asp(std::getenv("KS_OFF_ASP") != nullptr),
          sem_ext(std::getenv("KS_OFF_EXT") != nullptr),
          nmp_todos(std::getenv("KS_NMP_TODOS") != nullptr),
          margem_estudo(std::getenv("KS_MARGEM_ESTUDO") != nullptr),
          reb_fina(std::getenv("KS_REB") != nullptr),
          eval_crua(std::getenv("KS_EVAL_CRUA") != nullptr),
          eval_escala(std::getenv("KS_EVAL_ESC") ? std::atoi(std::getenv("KS_EVAL_ESC")) : 100),
          sing_conta(std::getenv("KS_SING_CONTA") != nullptr) {}
};
const Diag DIAG;

// Os termos da reducao, por ordem de aplicacao. Conta-se o MODULO: o que
// interessa nao e' se um termo empurra para cima ou para baixo, e' quanto do
// numero final e' dele.
enum { Q_TABELA, Q_CUTCNT, Q_PIORA, Q_CAPT, Q_TTPV, Q_CUT, Q_NAOPV, Q_HIST, Q_AMORT, Q_N };
const char* const Q_NOMES[Q_N] = {"tabela", "cut_cnt", "piora", "captura",
                                  "tt-pv", "cut node", "nao-PV", "historico", "amortec."};
std::uint64_t g_q_vezes[Q_N];
std::uint64_t g_q_modulo[Q_N];
inline void conta_q(int i, int d) {
    if (d != 0) { ++g_q_vezes[i]; g_q_modulo[i] += std::uint64_t(d < 0 ? -d : d); }
}


// A banda dos tranquilos por pontuar. Ver o `pontua_tarde` la' em baixo.
constexpr int TRANQUILO_POR_PONTUAR = -500000;
constexpr int INFINITO              = VALUE_INFINITE;

// O tecto das tabelas. As nossas topam MUITO abaixo das de outros motores --
// 15000 na principal, 30000 na continuacao -- e ja' nos custou um teste morto
// importar um divisor feito para uma escala cinquenta vezes maior.
constexpr int TECTO_HIST      = 15000;
constexpr int TECTO_CONT      = 30000;
constexpr int TECTO_CAPT      = 16384;
constexpr int TECTO_PEAO      = 8192;

// As formulas do half2k, e nao aproximacoes delas. O tecto do bonus tranquilo
// e' 4000 e nao 1700 -- errar aqui por 2,35x desloca TODAS as leituras que
// dependem do historico: o limiar da poda pelo historico (600*prof^2), o divisor
// da futilidade (75) e o da reducao (22000) foram varridos contra esta escala.
int bonus_hist(int prof) {
    if (DIAG.bonus_antigo)
        return std::min(160 * prof - 100, 1700);
    return std::max(std::min(200 * prof, 4000), 1);
}
int bonus_capt(int prof) {
    if (DIAG.bonus_antigo)
        return std::min(160 * prof - 100, 1700);
    return std::clamp(prof * 680 - 250, 0, 2400);
}

// Aproxima-se do tecto e nunca o passa: o incremento encolhe a` medida que o
// valor sobe, portanto uma entrada que ja' esta' no topo quase nao se mexe e
// uma que esta' a zero move-se toda.
void soma_hist(int& e, int bonus, int tecto) {
    bonus = std::clamp(bonus, -tecto, tecto);
    e += bonus - e * std::abs(bonus) / tecto;
}

// A mesma formula numa entrada PARTILHADA entre fios.
//
// Ler, calcular e escrever, em ordem relaxada, sem travar nada. Dois fios a
// creditar o mesmo lance ao mesmo tempo podem perder um dos creditos -- e isso
// nao tem importancia: a tabela e' um conselho de ordenacao construido ao longo
// de milhoes de nos, nao uma conta que tenha de fechar. O que importa e' nao
// haver leitura e escrita simultaneas num `int` cru, que e' comportamento
// indefinido e pode dar qualquer coisa, e nao apenas um numero desactualizado.
void soma_hist(std::atomic<int>& e, int bonus, int tecto) {
    bonus  = std::clamp(bonus, -tecto, tecto);
    int v  = e.load(std::memory_order_relaxed);
    e.store(v + bonus - v * std::abs(bonus) / tecto, std::memory_order_relaxed);
}

// Redimensiona uma tabela partilhada e poe-na a zero. `assign` nao serve: um
// `std::atomic<int>` nao e' copiavel, portanto nao ha' como o atribuir em
// bloco.
void zera_partilhada(std::vector<std::atomic<int>>& v, std::size_t n) {
    if (v.size() != n)
        v = std::vector<std::atomic<int>>(n);   // em C++20 nasce a zero
    else
        for (auto& e : v)
            e.store(0, std::memory_order_relaxed);
}

int idx_pc(PieceType pt) { return int(pt) - 1; }

// O LANCE MAIS VOTADO, e nao a busca com a nota mais alta.
//
// Quatro fios chegam ao fim com quatro respostas. Tomar a do principal deita
// fora tres buscas; tomar a de nota mais alta entrega a decisao a um unico fio
// que pode ter tido sorte numa linha. A votacao usa-as todas e pesa-as:
//
//     peso = nota - menor_nota + VOTO_PESO
//
// Uma busca muito abaixo das outras vota pouco; uma a` frente vota muito. Dois
// fios que cheguem ao MESMO lance somam os seus pesos, e e' aqui que esta' a
// forca do metodo -- o lance que varios caminhos independentes encontraram vale
// mais do que o que so' um encontrou, mesmo que esse um lhe tenha dado mais
// nota.
//
// O peso 24 e' o desta escala. Nao e' um numero redondo escolhido a` mao: e' o
// piso que impede que uma busca com a nota mais baixa de todas fique com voto
// zero e deixe de contar.
Move vota(const std::vector<std::pair<Move, int>>& cand) {
    if (cand.empty())
        return Move::none();
    const int VOTO_PESO = 24;
    int menor = cand[0].second;
    for (auto& [m, sc] : cand)
        menor = std::min(menor, sc);
    std::vector<std::pair<Move, std::int64_t>> votos;
    for (auto& [m, sc] : cand) {
        std::int64_t peso = std::int64_t(sc) - menor + VOTO_PESO;
        bool achou = false;
        for (auto& v : votos)
            if (v.first == m) { v.second += peso; achou = true; break; }
        if (!achou)
            votos.emplace_back(m, peso);
    }
    Move melhor = votos[0].first;
    std::int64_t mais = votos[0].second;
    for (auto& [m, v] : votos)
        if (v > mais) { mais = v; melhor = m; }
    return melhor;
}



// --- a historia de correccao ---
// SEIS familias de correccao, nao cinco.
//
// A sexta tem a chave no lance de SEIS plies atras. A ideia veio da fila de
// testes deles, onde `tweak_corrhist3b` -- "add (ss-6) continuationCorrection
// History" -- ia em LLR +1,87 quando a li.
//
// Entra com peso ZERO: com ele a zero a arvore e' identica ao no', e a familia
// existe sem mentir sobre o que se esta' a medir. `KS_CORR6` poe-lhe peso.
constexpr int CORR_FAM     = 6;
constexpr int CORR_TAM     = 16384;
constexpr int CORR_PESO[CORR_FAM] = {203, 109, 109, 121, 72, 0};
constexpr int CORR_DIV     = 2048;
constexpr int CORR_TECTO   = 1024;
constexpr int CORR_PASSO   = CORR_TECTO / 4;

std::uint64_t mistura(std::uint64_t x) {
    std::uint64_t z = x + 0x9e3779b97f4a7c15ULL;
    z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9ULL;
    z = (z ^ (z >> 27)) * 0x94d049bb133111ebULL;
    return z ^ (z >> 31);
}

// A nossa propria tabela de aleatorios por peca e casa. A do substrato e' local
// ao ficheiro dele, e nao faz falta que seja a mesma: o que se exige e' que duas
// posicoes partilhem chave exactamente quando as pecas escolhidas estao nas
// mesmas casas com as mesmas cores.
//
// A versao anterior disto no half2k indexava pela OCUPACAO, que nao distingue
// uma dama branca em d1 de um cavalo preto em d1, e juntava as cores. Uma tabela
// indexada assim arquiva uma correccao aprendida numa posicao onde outra sem
// relacao a vai ler, e as duas nao ensinam nada uma a` outra -- medido, custou
// catorze Elo.
struct Aleatorios {
    std::uint64_t v[16][64];
    Aleatorios() {
        std::uint64_t x = 0x243f6a8885a308d3ULL;
        for (int p = 0; p < 16; ++p)
            for (int s = 0; s < 64; ++s) {
                x = mistura(x);
                v[p][s] = x;
            }
    }
};
const Aleatorios ALEA;

// Uma chave sobre um subconjunto das pecas.
std::uint64_t chave_de(const Position& pos, Color c, const PieceType* tipos, int n) {
    std::uint64_t k = 0;
    for (int i = 0; i < n; ++i) {
        Bitboard bb = pos.pieces(c, tipos[i]);
        while (bb) {
            Square s = pop_lsb(bb);
            k ^= ALEA.v[int(c) * 8 + int(tipos[i])][int(s)];
        }
    }
    return k;
}

// O valor de uma peca, NAS UNIDADES DO HALF2K.
//
// Sao centipeoes classicos e nao os valores internos do substrato
// (208/781/825/1276/2538). A diferenca nao e' cosmetica: o `mvv = vitima*16 -
// atacante` ordena TODAS as capturas, e com os valores do substrato ele sai
// duas a tres vezes maior, o que desloca a barra do SEE proporcional
// (`-(mvv+passado)/60`) e a posicao das capturas em relacao a`s bandas.
int valor_de(PieceType pt) {
    switch (pt) {
    case PAWN: return 100;
    case KNIGHT: return 320;
    case BISHOP: return 330;
    case ROOK: return 500;
    case QUEEN: return 900;
    case KING: return 20000;
    default: return 0;
    }
}

// A mesma escala, nas unidades da AVALIACAO -- o dobro. So' a futilidade da
// quiescencia a usa, e essa esta' DESLIGADA no half2k (`QsFutility=false`);
// fica escrita para quando o interruptor for medido.
[[maybe_unused]] int valor_aval(PieceType pt) { return valor_de(pt) * 2; }

// Um limiar de SEE escrito nas unidades do half2k (peao = 100), convertido para
// as unidades em que o `see_ge` do substrato trabalha (peao = 208).
//
// Sem esta conversao, `-70*prof` -- que quer dizer "nao percas mais de 0,7 peoes
// por ply" -- chegava a uma funcao que le' 70 como um terco de peao. Todas as
// margens de troca da busca foram varridas na escala do half2k e sao passadas a
// uma troca que nao fala a mesma lingua.
int lim_see(int cp) { return cp * PawnValue / 100; }

int nota_para_tt(int nota, int ply) {
    if (nota >= VALUE_MATE_IN_MAX_PLY) return nota + ply;
    if (nota <= VALUE_MATED_IN_MAX_PLY) return nota - ply;
    return nota;
}
int nota_da_tt(int nota, int ply) {
    if (nota >= VALUE_MATE_IN_MAX_PLY) return nota - ply;
    if (nota <= VALUE_MATED_IN_MAX_PLY) return nota + ply;
    return nota;
}
bool e_mate(int v) { return std::abs(v) >= VALUE_MATE_IN_MAX_PLY; }

// A escolha do proximo: uma passagem pela lista em vez de a ordenar toda.
// Nove em cada dez nos cortam nos primeiros lances, portanto ordenar a lista
// inteira e' trabalho feito e deitado fora em massa.
void escolhe(Move* lances, int* notas, int* hist, int n, int em) {
    // Nada a escolher quando so' resta um.
    if (em + 1 >= n)
        return;
    // A nota do melhor fica em registo em vez de ser relida do vector a cada
    // comparacao. Esta funcao e' 6% do tempo todo -- e' o preco de olhar para a
    // lista inteira a cada lance, e faz-se em quase todos os nos.
    int melhor = em, mn = notas[em];
    for (int j = em + 1; j < n; ++j)
        if (notas[j] > mn) {
            mn     = notas[j];
            melhor = j;
        }
    if (melhor != em) {
        std::swap(lances[em], lances[melhor]);
        std::swap(notas[em], notas[melhor]);
        std::swap(hist[em], hist[melhor]);
    }
}

}  // namespace

void Busca::limpa() {
    std::memset(killers, 0, sizeof(killers));
    std::memset(principal, 0, sizeof(principal));
    std::memset(hist_pc, 0, sizeof(hist_pc));
    capt_hist.assign(6 * 64 * 6, 0);
    // Cinco planos sempre reservados: os dois ultimos ficam a zero e por tocar
    // quando `cont_n` e' 3, e assim a manete nao mexe na memoria.
    // So' o DONO limpa as tabelas partilhadas. Um ajudante a limpar apagava, a
    // cada busca, tudo o que a busca principal tinha aprendido -- e o sintoma
    // seria uma busca paralela mais fraca do que a de um fio, sem nada a
    // apontar a causa.
    if (hist_proprio) {
        // Cinco planos sempre reservados: os dois ultimos ficam a zero e por
        // tocar quando `cont_n` e' 3, e assim a manete nao mexe na memoria.
        zera_partilhada(cont_hist_meu, 5 * (6 * 64) * (6 * 64));
        zera_partilhada(hist_peao_meu, std::size_t(p.peao_chaves) * 12 * 64);
        zera_partilhada(corr_meu, CORR_FAM * 2 * CORR_TAM);
    }
    low_ply.assign(LOW_PLY * 64 * 64, 102);
    p_tab->limpa();
}

void Busca::nova_partida() {
    limpa();
    chaves_jogo.clear();
}

// --- as tabelas, lidas e escritas -------------------------------------------

int Busca::capt_nota(const Position& pos, Move m) const {
    if (capt_hist.empty())
        return 0;
    PieceType movida = type_of(pos.moved_piece(m));
    PieceType comida = m.type_of() == EN_PASSANT ? PAWN : type_of(pos.piece_on(m.to_sq()));
    if (comida == NO_PIECE_TYPE)
        return 0;
    return capt_hist[(idx_pc(movida) * 64 + int(m.to_sq())) * 6 + idx_pc(comida)];
}

void Busca::credita_captura(const Position& pos, Move m, int bonus) {
    if (capt_hist.empty())
        return;
    PieceType movida = type_of(pos.moved_piece(m));
    PieceType comida = m.type_of() == EN_PASSANT ? PAWN : type_of(pos.piece_on(m.to_sq()));
    if (comida == NO_PIECE_TYPE)
        return;
    soma_hist(capt_hist[(idx_pc(movida) * 64 + int(m.to_sq())) * 6 + idx_pc(comida)], bonus,
              TECTO_CAPT);
}

// --- as duas leituras do historico, que NAO sao a mesma ---
//
// O half2k le' as tabelas de duas maneiras e isso nao e' acidente:
//
//   `hist_de`  serve a PODA. Principal CRUA mais as continuacoes com os pesos
//              [2,1,1] nos recuos [1,2,4]. Devolve ZERO para toda a captura,
//              por construcao -- e e' por isso que os termos de historia dos
//              ramos de captura sao estruturalmente nulos.
//   `ordena`   serve a ORDEM. Principal ESCALADA por 138/32, as mesmas
//              continuacoes, mais a tabela peca-casa.
//
// Usei uma so' para as duas coisas e isso muda a poda e a ordem ao mesmo tempo.
//
// Tres posicoes e nao cinco: os recuos 3 e 6 existem no half2k e estao
// desligados (`ContLongo=false`).
namespace {
// AS FAMILIAS DAS CONTINUACOES.
//
// Os tres primeiros sao os de sempre e ficam NA MESMA ORDEM de proposito: o
// indice `k` e' tambem o plano da tabela, portanto trocar a ordem trocava o
// sitio onde tudo o que ja' foi aprendido esta' guardado. Os dois novos entram
// no fim.
//
// A referencia soma cinco: ss-1, ss-2, ss-3, ss-4 e ss-6. Faltavam-nos o de
// TRES plies atras e o de SEIS.
constexpr int CONT_RECUO[5]  = {1, 2, 4, 3, 6};
constexpr int CONT_PESO[5]   = {2, 1, 1, 1, 1};
}  // namespace

// Ver `hist_prefetch` nos parametros. So' pede: nao le nem escreve nada que
// mude um valor. As linhas das continuacoes dependem do caminho e nao do lance,
// por isso acham-se uma vez; do lance so' vem a coluna (peca e casa).
void Busca::pede_historias(const Position& pos, const Move* ls, const int* ns,
                           int de, int n, int ply) const {
    garante_ameacas(pos, ply);
    const int lado = int(pos.side_to_move());
    const std::atomic<int>* linha[5];
    int nl = 0;
    for (int k = 0; k < p.cont_n && k < 5; ++k) {
        int atras = ply - CONT_RECUO[k];
        int apc, apara;
        if (atras >= 0) {
            if (jogado_pc[atras] < 0)
                continue;
            apc = jogado_pc[atras]; apara = jogado_para[atras];
        } else {
            int i = -atras - 1;
            if (i >= pre_n)
                continue;
            apc = pre_pc[i]; apara = pre_para[i];
        }
        linha[nl++] = &(*cont_hist)[(k * (6 * 64) + apc * 64 + apara) * (6 * 64)];
    }
    const bool peao = p.peao_f > 0 && !hist_peao->empty();
    const std::size_t base_peao =
      peao ? (std::size_t(pos.pawn_key() & (std::uint64_t(p.peao_chaves) - 1)) * 12
              + 6 * std::size_t(lado)) * 64
           : 0;
    for (int j = de; j < n; ++j) {
        if (ns[j] != TRANQUILO_POR_PONTUAR)
            continue;
        const Move m  = ls[j];
        const int  pa = int(m.to_sq());
        const int  pc = idx_pc(type_of(pos.moved_piece(m)));
        __builtin_prefetch(&principal[lado][int(m.from_sq())][pa][balde(m, ply)]);
        for (int k = 0; k < nl; ++k)
            __builtin_prefetch(linha[k] + pc * 64 + pa);
        if (peao)
            __builtin_prefetch(&(*hist_peao)[base_peao + std::size_t(pc) * 64 + pa]);
    }
}

int Busca::conts(const Position& pos, Move m, int ply, int pc) const {
    int h = 0;
    for (int k = 0; k < p.cont_n; ++k) {
        int atras = ply - CONT_RECUO[k];
        int apc, apara;
        if (atras >= 0) {
            if (jogado_pc[atras] < 0)
                continue;
            apc = jogado_pc[atras]; apara = jogado_para[atras];
        } else {
            // Acima da raiz: o lance anterior esta' na partida.
            int i = -atras - 1;
            if (i >= pre_n)
                continue;
            apc = pre_pc[i]; apara = pre_para[i];
        }
        int ant = apc * 64 + apara;
        h += CONT_PESO[k]
           * (*cont_hist)[(k * (6 * 64) + ant) * (6 * 64) + pc * 64 + int(m.to_sq())]
                 .load(std::memory_order_relaxed);
    }
    (void) pos;
    return h;
}

// O MAPA DAS AMEACAS, CALCULADO SO' QUANDO FOR PRECISO.
//
// Estava a ser calculado a` entrada de TODOS os nos -- seis varreduras de
// ataques, uma por tipo de peca. Mas so' e' lido dentro do `balde()`, que so'
// corre quando se pontuam os TRANQUILOS. E os tranquilos pontuam-se tarde de
// proposito: um em cada quatro nos chega ali, e desses 92% cortam sem precisar
// de um unico tranquilo.
//
// Ou seja: calculava-se quatro vezes mais vezes do que se usava. E num motor em
// que 41% dos nos estao a` profundidade restante 1 -- medido -- isso nao e' um
// detalhe.
//
// MEASURED, best of three at depth 15: 1,892 ms computing it on demand against
// 1.912 ms ansioso, e a arvore identica ao no (88.386 dos dois lados). Os dois
// sao indistinguiveis com este ruido -- a primeira medicao que fiz, uma so,
// dizia que o tempo subia 10% e estava errada. Fica a forma recuperada.
//
// A guarda pela chave ja estava la e faz o resto: dentro do mesmo no so se
// calcula uma vez, por muitas vezes que se peca.
void Busca::garante_ameacas(const Position& pos, int ply) const {
    if (ameacas_chave[ply] == pos.key() && ameacas_chave[ply] != 0)
        return;
    Color adv = ~pos.side_to_move();
    // Os ataques por tipo, para se poder perguntar "batido por peca MAIS
    // than this one". They are kept separated -- pawns, knights, bishops --
    // e e' o unico arranjo em que a pergunta do `ameaca()` faz sentido.
    Bitboard a_p = pos.attacks_by<PAWN>(adv);
    Bitboard a_c = pos.attacks_by<KNIGHT>(adv);
    Bitboard a_b = pos.attacks_by<BISHOP>(adv);
    Bitboard a_t = pos.attacks_by<ROOK>(adv);
    // Peao e rei a zero: nao ha' peca mais barata do que um peao, e o rei nao
    // se troca.
    ameacas_barato[ply][idx_pc(PAWN)]   = 0;
    ameacas_barato[ply][idx_pc(KNIGHT)] = a_p;
    ameacas_barato[ply][idx_pc(BISHOP)] = a_p;
    ameacas_barato[ply][idx_pc(ROOK)]   = a_p | a_c | a_b;
    ameacas_barato[ply][idx_pc(QUEEN)]  = a_p | a_c | a_b | a_t;
    ameacas_barato[ply][idx_pc(KING)]   = 0;
    ameacas_mapa[ply] = pos.attacks_by<PAWN>(adv) | pos.attacks_by<KNIGHT>(adv)
                      | pos.attacks_by<BISHOP>(adv) | pos.attacks_by<ROOK>(adv)
                      | pos.attacks_by<QUEEN>(adv) | pos.attacks_by<KING>(adv);
    ameacas_chave[ply] = pos.key();
}

// A parcela das ameacas. Ver `ameaca_f` no `busca.h` -- e ver la' tambem que
// the threat term is only read when quiet moves are scored.
//
// Premeia SAIR de uma casa batida por peca mais barata e castiga ENTRAR nela,
// com o valor da peca como peso.
int Busca::ameaca(const Position& pos, Move m, int ply) const {
    PieceType pt = type_of(pos.moved_piece(m));
    if (pt == PAWN || pt == KING)
        return 0;
    Bitboard batido = ameacas_barato[ply][idx_pc(pt)];
    if (!batido)
        return 0;
    // A ESCALA DESTE TERMO ESTAVA ERRADA POR 244 VEZES.
    //
    // Estava `p.ameaca_f * valor_de(pt) / 100`, que para um cavalo da'
    // 20*320/100 = 64. O `ks_1.20260919` faz (`negamax`, `43899e`):
    //
    //     438922:  mov 0x178(%r15),%esi     ; ameaca_f
    //     438984:  lea ...,%rcx             ; # 5fe200 <PieceValue>
    //     43899e:  imul (%rcx,%r8,4),%esi   ; ameaca_f * PieceValue[pt]
    //     4389a3:  imul %esi,%r10d          ; * (saiu - entrou)
    //     4389a7:  add %r10d,%r11d          ; nota += ...
    //
    // sem divisao nenhuma, e com a tabela do SUBSTRATO, que esta' em
    // `0x5fe200` e vale `0, 208, 781, 825, 1276, 2538`. Para o mesmo cavalo:
    // 20*781 = 15.620. Duzentas e quarenta e quatro vezes mais.
    //
    // A referencia concorda, e e' dela que o 20 vem
    // (`vendor/movepick.cpp:246`):
    //
    //     int v = 20 * (bool(threatByLesser[pt] & from) - bool(... & to));
    //     value += PieceValue[pt] * v;
    //
    // E a nota do `ameaca_f` no `busca.h` ja' escrevia a formula CERTA --
    // `nota += valor_da_peca * ameaca_f * (saiu - entrou)`, sem /100. O codigo
    // e' que nunca bateu a` sua propria documentacao: a linha do `/100` vem do
    // commit inicial, do port, e quando `3eb5461` pos a manete a 20 ninguem
    // olhou para a formula por baixo. O numero estava la'; o efeito nao.
    //
    // Na escala em que a `ordena` trabalha -- historia ate' +-15.000 -- 64 e'
    // ruido e 15.620 e' um termo a serio. E' a diferenca entre ter a manete e
    // so' ter o seu nome.
    int peso = p.ameaca_f * int(PieceValue[pt]);
    int r = 0;
    if (batido & m.from_sq()) r += peso;   // sair de uma casa batida
    if (batido & m.to_sq())   r -= peso;   // entrar numa casa batida
    return r;
}

int Busca::balde(Move m, int ply) const {
    if (p.sem_balde)
        return 0;
    Bitboard mapa = ameacas_mapa[ply];
    int de   = (mapa >> int(m.from_sq())) & 1;
    int para = (mapa >> int(m.to_sq())) & 1;
    return de * 2 + para;
}

int Busca::hist_de(const Position& pos, Move m, int ply) const {
    if (pos.capture_stage(m) || m.type_of() == PROMOTION)
        return 0;
    // O `balde` le' o mapa das ameacas. Como o mapa passou a ser calculado a`
    // hora, garante-se aqui -- a guarda pela chave torna isto de graca quando
    // ja' esta' feito.
    garante_ameacas(pos, ply);
    int lado = int(pos.side_to_move());
    int h = principal[lado][int(m.from_sq())][int(m.to_sq())][balde(m, ply)];
    // O peso das continuacoes, em 32 avos. 32 = como estava.
    //
    // Aqui, e nao no `ordena()`: e' este o caminho que o motor percorre. O aviso
    // ja' estava escrito tres linhas abaixo e fui pô-lo no sitio errado na
    // mesma; a medida a zero -- que nao mexeu um no' -- e' que denunciou.
    //
    // E aqui o numero serve DUAS coisas: esta nota ordena os tranquilos E e' a
    // que a poda pelo historico le. Nao ha' como mexer so' numa; o par nao se
    // separa neste caminho, e quem medir isto esta' a medir as duas.
    h += conts(pos, m, ply, idx_pc(type_of(pos.moved_piece(m)))) * p.ordem_cont_f / 32;
    // O historico dos PRIMEIROS PLIES. Divide-se pelo ply porque quanto mais
    // fundo se vai, menos ele sabe e mais a tabela grande ja' aprendeu.
    //
    // ATENCAO ao sitio: a primeira versao disto foi parar ao `ordena()`, que so'
    // corre com `KS_ORDEM_ESCALADA` ligado. A arvore nao mexeu um no' com o peso
    // a 8 nem com o peso a 100.000 -- e foi essa medida a zero que denunciou o
    // engano. E' aqui que o motor ordena os tranquilos.
    if (p.low_f > 0 && ply < LOW_PLY && !low_ply.empty())
        h += p.low_f * low_ply[(ply * 64 + int(m.from_sq())) * 64 + int(m.to_sq())] / (1 + ply);
    // O historico de peoes. Mesma casa de peso que as continuacoes: 32 = um,
    // 64 = o peso dobrado com que a referencia o soma.
    if (p.peao_f > 0 && !hist_peao->empty()) {
        std::size_t ipeao =
          (std::size_t(pos.pawn_key() & (std::uint64_t(p.peao_chaves) - 1)) * 12
           + std::size_t(idx_pc(type_of(pos.moved_piece(m))))
           + 6 * std::size_t(int(pos.side_to_move()))) * 64
          + std::size_t(int(m.to_sq()));
        h += p.peao_f * (*hist_peao)[ipeao].load(std::memory_order_relaxed) / 32;
    }
    return h;
}

// NAO USADA no caminho por omissao. Fica escrita porque e' o que o half2k faz
// quando `PontuaTarde` esta' desligado, e para nao ser reinventada de memoria
// quando esse interruptor for medido.
int Busca::ordena(const Position& pos, Move m, int ply) const {
    int lado = int(pos.side_to_move());
    int pc   = idx_pc(type_of(pos.moved_piece(m)));
    // A principal DOMINA: medido, o modulo medio dela e' 1374 e o das
    // continuacoes 301/234/297. A composicao inversa -- continuacoes primeiro e
    // a principal escalada por cima -- e' que e' a boa, e o peso fica em
    // parametro para se varrer na NOSSA arvore.
    int h = principal[lado][int(m.from_sq())][int(m.to_sq())][balde(m, ply)]
          * p.ordem_princ_f / 32;
    h += conts(pos, m, ply, pc) * p.ordem_cont_f / 32;
    // A de-para nunca pode dizer isto: aprende que g3-f5 resulta, nao que um
    // cavalo em f5 e' bom, porque cada casa de partida guarda a sua conta.
    h += p.hist_pc_f * hist_pc[pc][int(m.to_sq())] / 100;
    return h;
}

// As cinco chaves desta posicao.
void Busca::indices(const Position& pos, int ply, int fora[6]) const {
    // Guardam-se as TRES que dependem so' da posicao; as outras duas
    // recalculam-se sempre.
    //
    // As chaves dos peoes e da forca de cada lado saem de uma passagem por todas
    // as pecas -- umas trinta iteracoes, e este bloco e' 4,3% do tempo todo. Mas
    // sao funcao da POSICAO, portanto valem para qualquer caminho que la' chegue.
    //
    // As outras duas nao: o `cont` sai do lance anterior e o `trans` da mudanca
    // da chave. Guardar essas foi o que tentei primeiro, e a arvore saltou de
    // 171.041 para 303.788 nos -- a mesma casa alcancada por outro lance passou
    // a ler os indices do caminho errado.
    static const PieceType PEAO[1]  = {PAWN};
    static const PieceType FORCA[4] = {KNIGHT, BISHOP, ROOK, QUEEN};

    const std::uint64_t m = CORR_TAM - 1;
    if (!DIAG.sem_pre && corr_pos_chave[ply] == pos.key() && corr_pos_chave[ply] != 0) {
        fora[0] = corr_pos[ply][0];
        fora[1] = corr_pos[ply][1];
        fora[2] = corr_pos[ply][2];
    } else {
        // Ver `indices_rapido` nos parametros: as tres chaves ou saem das que a
        // `Position` ja' mantem a` custa do `do_move` -- O(1) --, ou refazem-se
        // varrendo os bitboards, que e' 43% desta funcao.
        //
        // O `nonPawnKey` traz o rei e o nosso `FORCA[]` nao o tem; tira-se com
        // um XOR, para as classes de equivalencia ficarem as mesmas.
        std::uint64_t peoes, fb, fp;
        if (p.indices_rapido) {
            peoes = pos.pawn_key();
            fb    = pos.non_pawn_key(WHITE)
                  ^ Zobrist::psq[make_piece(WHITE, KING)][pos.square<KING>(WHITE)];
            fp    = pos.non_pawn_key(BLACK)
                  ^ Zobrist::psq[make_piece(BLACK, KING)][pos.square<KING>(BLACK)];
        } else {
            peoes = chave_de(pos, WHITE, PEAO, 1) ^ chave_de(pos, BLACK, PEAO, 1);
            fb    = chave_de(pos, WHITE, FORCA, 4);
            fp    = chave_de(pos, BLACK, FORCA, 4);
        }
        fora[0] = int(mistura(peoes) & m);
        fora[1] = int(mistura(fb) & m);
        fora[2] = int(mistura(fp) & m);
        corr_pos_chave[ply] = pos.key();
        corr_pos[ply][0] = fora[0];
        corr_pos[ply][1] = fora[1];
        corr_pos[ply][2] = fora[2];
    }

    std::uint64_t cont = 0;
    if (ply > 0 && jogado_pc[ply - 1] >= 0)
        cont = mistura((std::uint64_t(jogado_pc[ply - 1]) << 8)
                       | std::uint64_t(jogado_para[ply - 1]) | 0x5eed00000000ULL);

    // O ultimo lance visto como MUDANCA da chave da posicao: duas posicoes
    // diferentes a que se chegou pelo mesmo lance partilham este indice.
    std::uint64_t delta = 0;
    if (chaves.size() >= 2)
        delta = chaves[chaves.size() - 1] ^ chaves[chaves.size() - 2];
    std::uint64_t trans = delta ? mistura(delta) : 0;

    // A SEXTA: o lance de seis plies atras. Acima da raiz vai buscar-se a`
    // partida, como o `conts()` faz -- sem isso a familia nascia vazia
    // justamente no topo da arvore, onde vive a maior parte dos nos.
    std::uint64_t seis = 0;
    {
        int atras = ply - 6;
        int apc = -1, apara = 0;
        if (atras >= 0) {
            apc = jogado_pc[atras]; apara = jogado_para[atras];
        } else {
            int i = -atras - 1;
            if (i < pre_n) { apc = pre_pc[i]; apara = pre_para[i]; }
        }
        if (apc >= 0)
            seis = mistura((std::uint64_t(apc) << 8) | std::uint64_t(apara)
                           | 0x6e15000000000ULL);
    }

    fora[3] = int(cont & m);
    fora[4] = int(trans & m);
    // SEM lance seis plies atras, a familia NAO participa.
    //
    // Antes isto punha a chave a zero, e zero e' um indice valido: todas as
    // posicoes sem historia suficiente caiam no mesmo balde e a familia passava
    // a ser um vies global em vez de um indice. Media-se logo -- a arvore
    // inchava 28 a 45% com qualquer peso, o que e' de mais para uma familia que
    // acrescenta 11% a` magnitude da correccao.
    fora[5] = seis ? int(seis & m) : -1;

}

int Busca::corrigida(const Position& pos, int cru, int ply) const {
    if (DIAG.sem_corr)
        return cru;
    if (corr->empty())
        return cru;
    int idx[6];
    indices(pos, ply, idx);
    int lado = int(pos.side_to_move());
    int soma = 0;
    for (int k = 0; k < CORR_FAM; ++k) {
        if (idx[k] < 0)
            continue;
        soma += (*corr)[(k * 2 + lado) * CORR_TAM + idx[k]].load(std::memory_order_relaxed)
              * (k == 5 ? p.corr6_peso : CORR_PESO[k]);
    }
    int v = cru + soma / CORR_DIV;
    return std::clamp(v, -VALUE_MATE_IN_MAX_PLY + 1, VALUE_MATE_IN_MAX_PLY - 1);
}

void Busca::aprende(const Position& pos, int dif, int prof, int ply) {
    if (DIAG.sem_corr)
        return;
    if (corr->empty())
        return;
    int idx[6];
    indices(pos, ply, idx);
    int lado  = int(pos.side_to_move());
    int bonus = std::clamp(dif * prof / 8, -CORR_PASSO, CORR_PASSO);
    for (int k = 0; k < CORR_FAM; ++k) {
        if (idx[k] < 0)
            continue;
        std::atomic<int>& e = (*corr)[(k * 2 + lado) * CORR_TAM + idx[k]];
        int v = e.load(std::memory_order_relaxed);
        v += bonus - v * std::abs(bonus) / CORR_TECTO;
        e.store(std::clamp(v, -CORR_TECTO, CORR_TECTO), std::memory_order_relaxed);
    }
}

void Busca::credita(const Position& pos, Move m, int ply, int bonus) {
    // A escrita usa o MESMO `balde` que a leitura. Se o mapa aqui fosse outro,
    // creditava-se numa gaveta e lia-se de outra.
    garante_ameacas(pos, ply);
    int lado = int(pos.side_to_move());
    if (p.low_f > 0 && ply < LOW_PLY && !low_ply.empty())
        soma_hist(low_ply[(ply * 64 + int(m.from_sq())) * 64 + int(m.to_sq())],
                  bonus * p.low_bonus / 1024, TECTO_HIST);
    soma_hist(principal[lado][int(m.from_sq())][int(m.to_sq())][balde(m, ply)], bonus,
              TECTO_HIST);
    PieceType pt = type_of(pos.moved_piece(m));
    soma_hist(hist_pc[idx_pc(pt)][int(m.to_sq())], bonus, TECTO_HIST);
    // A escrita do historico de peoes. A LEITURA veio em fonte verdadeira de
    // The write mirrors the read exactly, which is what determines it: reading
    // e' o unico indice que a leitura aceita. Ler uma tabela que nunca se
    // escreve e' pior do que nao a ler: a ordenacao passa a depender de uma
    // entrada que fica sempre a zero.
    if (p.peao_f > 0 && !hist_peao->empty()) {
        std::size_t ipeao =
          (std::size_t(pos.pawn_key() & (std::uint64_t(p.peao_chaves) - 1)) * 12
           + std::size_t(idx_pc(pt)) + 6 * std::size_t(lado)) * 64
          + std::size_t(int(m.to_sq()));
        // O BONUS QUE ENTRA NA HISTORIA DOS PEOES NAO E' O BONUS CRU.
        //
        // O `ks_1.20260919` escala-o antes de somar, e com pesos diferentes
        // para cima e para baixo. `credita` em `41242c`:
        //
        //     41242c:  cmp $0xfffffffd,%r12d    ; bonus vs -3
        //     412430:  jl 412460                ; abaixo -> o outro peso
        //     412432:  imul $0x450,%r12d,%edx   ; bonus * 1104
        //     412444:  sar $0xa,%eax            ; / 1024
        //     412460:  imul $0x1cb,%r12d,%r15d  ; bonus * 459
        //
        // `%r12d` e' o bonus: `412290: mov %r8d,%r12d` guarda o quinto
        // argumento inteiro a` entrada e nada lhe toca ate' aqui. O bloco e'
        // mesmo o dos peoes -- `4123cc` le' o `peao_f`, `4123f7` le' o
        // `peao_chaves`, e `41249d` le' o `cont_n` para o ciclo seguinte.
        //
        // Sobe o premio 7,8% e corta o castigo para 45%. A tabela dos peoes e'
        // lida pela `hist_de`, que serve a PODA, e pela `ordena`, que serve a
        // ORDEM: isto mexe na busca em todos os nos, ao contrario de tudo o
        // resto que esta reconstrucao repos esta noite.
        soma_hist((*hist_peao)[ipeao], bonus * (bonus >= -3 ? 1104 : 459) / 1024,
                  TECTO_PEAO);
    }
    // Escreve nas MESMAS posicoes que o `conts` le', incluindo as que ficam
    // acima da raiz. Ler uma posicao que nunca se escreve e' pior do que nao a
    // ler: a ordenacao no topo da arvore passa a depender de uma entrada que
    // esta busca nunca actualiza. Medido: com a leitura sem a escrita, a mesma
    // posicao dada por lista de lances custava 48.460 nos contra 21.501 dada
    // pela FEN -- a mesma posicao, mais do dobro da arvore.
    for (int k = 0; k < p.cont_n; ++k) {
        int atras = ply - CONT_RECUO[k];
        int apc, apara;
        if (atras >= 0) {
            if (jogado_pc[atras] < 0)
                continue;
            apc = jogado_pc[atras]; apara = jogado_para[atras];
        } else {
            int i = -atras - 1;
            if (i >= pre_n)
                continue;
            apc = pre_pc[i]; apara = pre_para[i];
        }
        int ant = apc * 64 + apara;
        soma_hist((*cont_hist)[(k * (6 * 64) + ant) * (6 * 64) + idx_pc(pt) * 64 + int(m.to_sq())],
                  bonus, TECTO_CONT);
    }
}

// --- a ordem ----------------------------------------------------------------

void Busca::pontua(const Position& pos, Lista& l, int de, Move tt_lance,
                   Limite tt_limite, int ply, int prof) const {
    Move* lances = l.lances;
    int*  notas  = l.notas;
    int*  hist   = l.hist;
    for (int n = de; n < l.n; ++n) {
        Move m = lances[n];
        hist[n] = 0;

        if (m == tt_lance) {
            // Sempre no topo. Descer o lance guardado quando ele vem de um
            // limite superior existe no half2k e esta' desligado
            // (`TtFraco=false`).
            (void) tt_limite;
            // Ver `tt_sup_nota`. Com a manete no valor de origem isto e'
            // exactamente o que era: o lance da tabela sempre no topo.
            notas[n] = (tt_limite == Limite::Superior) ? p.tt_sup_nota : 1000000;
            continue;
        }

        // `capture` e nao `capture_stage`: o segundo conta tambem a promocao a
        // dama, que no half2k tem banda propria (500.000) e nao entra pelo ramo
        // das capturas. Com `capture_stage` uma promocao tranquila era pontuada
        // como captura de casa vazia -- vitima 0, atacante 100, mvv negativo --
        // e ia parar ao fundo da lista em vez de ir a` frente.
        bool captura = pos.capture(m);
        if (captura) {
            int vitima  = m.type_of() == EN_PASSANT ? valor_de(PAWN)
                                                    : valor_de(type_of(pos.piece_on(m.to_sq())));
            int atacante = valor_de(type_of(pos.moved_piece(m)));
            int mvv      = vitima * 16 - atacante;
            int passado  = capt_nota(pos, m) * 16 / std::max(p.capt_hist_div, 1);
            // O corte proporcional ao MERITO do proprio lance, e nao a`
            // profundidade. Com a barra vinda da profundidade, a historia mexia
            // na ORDEM e mais nada: uma captura que a tabela adora e outra que
            // ela despreza eram julgadas boas ou mas pelo mesmo criterio.
            int barra = lim_see(-(mvv + passado) / std::max(p.capt_bar_div, 1));
            // Uma captura que perde material nao e' um bom lance que por acaso
            // e' violento. Fica abaixo de tudo -- mas ainda a` frente de nada.
            notas[n] = (pos.see_ge(m, barra) ? 600000 : -600000) + mvv + passado;
            continue;
        }

        if (m.type_of() == PROMOTION && m.promotion_type() == QUEEN) {
            notas[n] = 500000;
            continue;
        }

        // Killers NAO ha'. Estao fora de proposito: herdar a lista do ply de
        // baixo, que veio de outra sub-arvore, e' promover lances a` frente por
        // razao nenhuma. Os tranquilos que eram killers acertam 60,1% a`
        // primeira contra 11,9% dos genericos, e essa diferenca sai igual do
        // historico -- que ja' os conhece -- sem precisar de banda propria.
        //
        // Pontuar tarde: um em cada quatro nos chega aqui e desses 92% cortam
        // sem precisar de um unico tranquilo. Perguntar a`s tabelas por cada um
        // deles e' o trabalho mais caro da ordenacao, e quase todo deitado fora.
        // A banda dos killers fica ENTRE a promocao a dama (500.000) e os
        // tranquilos por pontuar. Dois valores para o primeiro ficar a` frente
        // do segundo.
        if (p.usa_killers > 0 && ply < MAX_PLY) {
            if (m == killers[ply][0]) { notas[n] = p.usa_killers + 1; hist[n] = 0; continue; }
            if (m == killers[ply][1]) { notas[n] = p.usa_killers;     hist[n] = 0; continue; }
        }
        notas[n] = TRANQUILO_POR_PONTUAR;
        hist[n]  = TRANQUILO_POR_PONTUAR;
        (void) ply;
        (void) prof;
    }
}

// --- a reducao --------------------------------------------------------------
//
// Acumulada em MILESIMOS e dividida no fim, para um termo poder valer um terco
// de ply em vez de tudo ou nada. A primeira versao somava plies inteiros, e so'
// o termo do cut node valia dois onde um terco e' o tamanho certo.
int Busca::reducao(int prof, int i, int alpha, int beta, bool melhorando, bool tranquilo,
                   bool cut, bool pv, bool tt_pv, Move tt_lance, int hist_i,
                   bool ttpv_bate_alpha, bool ttpv_fundo, int cortes_filho) const {
    // Sem reducao nenhuma: a arvore inteira a` profundidade nominal. E' a
    // medida de quanto a reducao esta' a poupar, e de quanto custa em qualidade.
    if (DIAG.sem_lmr)
        return 0;
    int r = lmr[std::min(prof, 63)][std::min(i, 63)];
    if (DIAG.quem) conta_q(Q_TABELA, r);

    // O ply SEGUINTE ja' cortou muitas vezes: no' facil, reduz-se mais.
    if (cortes_filho > 1) {
        int d = p.cut_cnt_base + p.cut_cnt_mais * int(cortes_filho > 2);
        r += d;
        if (DIAG.quem) conta_q(Q_CUTCNT, d);
    }

    if (!melhorando) {
        int d = r * p.lmr_piora_f / 512;
        r += d;
        if (DIAG.quem) conta_q(Q_PIORA, d);
    }
    // A janela deste no' contra a da raiz. Vem do `ks_1.20260919` (`40ae6f`), e
    // entra AQUI: depois do termo do `melhorando` e antes de partir ao meio nas
    // capturas -- a ordem importa, porque o `!tranquilo` corta metade do que
    // estiver acumulado ate' esse ponto.
    if (p.lmr_delta > 0)
        r -= (beta - alpha) * p.lmr_delta / std::max(delta_raiz, 1);

    if (!tranquilo) {
        int d = -(r / 2);
        r += d;
        if (DIAG.quem) conta_q(Q_CAPT, d);
    }

    // O PAR. Ver a nota nos parametros: entram os dois ou nenhum.
    //
    // E' INDEPENDENTE da extensao negativa. Eu tinha-os ligados ao mesmo
    // interruptor, o que impedia medir o par sozinho -- e o terceiro motor que
    // usa isto tem o ttPv a 1 ply com a extensao negativa DESLIGADA por omissao.
    // Sao duas ideias diferentes e cada uma tem de ter o seu numero.
    if (p.lmr_ttpv > 0 || p.lmr_cut_f > 0) {
        // Abre: um no' que ja' foi importante merece mais profundidade.
        if (tt_pv) {
            int d = -(p.lmr_ttpv + p.lmr_ttpv_pv * int(pv)
                      + p.lmr_ttpv_alpha * int(ttpv_bate_alpha)
                      + p.lmr_ttpv_fundo * int(ttpv_fundo));
            r += d;
            if (DIAG.quem) conta_q(Q_TTPV, d);
        }
        // Fecha: num no' onde se espera cortar, e ainda mais sem lance guardado
        // por onde comecar.
        if (cut) {
            int d = p.lmr_cut_f + p.lmr_cut_sem_tt * int(tt_lance == Move::none());
            r += d;
            if (DIAG.quem) conta_q(Q_CUT, d);
        }
    } else {
        (void) cut;
    }

    if (!pv) {
        r += p.lmr_nonpv_f;
        if (DIAG.quem) conta_q(Q_NAOPV, p.lmr_nonpv_f);
    }
    (void) tt_pv;
    (void) tt_lance;
    (void) alpha;
    (void) beta;
    // O historico DEVOLVE reducao, com o desvio grampeado nos dois sentidos.
    int dh = -std::clamp(hist_i * 1024 / std::max(p.lmr_hist_div, 1), -2048, 2048);
    r += dh;
    if (DIAG.quem) conta_q(Q_HIST, dh);
    // Amortece o lado negativo: aproxima a extensao do zero sem a proibir.
    if (r < 0) {
        int antes = r;
        r = r * p.lmr_ext_amort / 100;
        if (DIAG.quem) conta_q(Q_AMORT, r - antes);
    }
    return r;
}

// --- empate -----------------------------------------------------------------

bool Busca::repeticao(const Position& pos) const {
    Key  k = pos.key();
    int  n = 0;
    // So' ate' onde o contador dos cinquenta lances deixa: antes disso houve
    // captura ou lance de peao e a posicao nao pode voltar.
    int limite = std::min<int>(pos.rule50_count(), int(chaves.size()));
    for (int i = int(chaves.size()) - 2; i >= int(chaves.size()) - limite && i >= 0; i -= 2)
        if (chaves[i] == k && ++n >= 1)
            return true;
    return false;
}

bool Busca::sem_tempo() {
    if (parado)
        return true;
    if (parar.load(std::memory_order_relaxed)) {
        parado = true;
        return true;
    }
    if (nos_limite > 0 && nos >= nos_limite) {
        parado = true;
        return true;
    }
    // Uma vez em cada 1024 nos: `steady_clock::now()` custa uma chamada e a
    // busca passa por aqui milhoes de vezes.
    if ((nos & 1023) == 0 && duro.count() > 0) {
        auto passou = std::chrono::duration_cast<std::chrono::milliseconds>(
          std::chrono::steady_clock::now() - inicio);
        if (passou >= duro)
            parado = true;
    }
    return parado;
}

// --- a quiescencia ----------------------------------------------------------

int Busca::quiescencia(Position& pos, int alpha, int beta, int ply) {
    ++nos;
    if (DIAG.forma) ++g_forma_qs;
    if (ply > sel_prof)
        sel_prof = ply;
    if (sem_tempo())
        return 0;
    if (ply >= MAX_PLY - 1)
        return pos.checkers() ? VALUE_ZERO : (++n_aval, DIAG.eval_crua ? av->avalia(pos) : av->avalia_cheia(pos, DIAG.eval_escala, otimismo_de(pos)));
    if (pos.rule50_count() >= 100 || repeticao(pos))
        return valor_empate(pos);

    bool em_xeque   = bool(pos.checkers());
    int  alpha_orig = alpha;

    // A tabela vale a pena aqui tambem. A quiescencia e' quase toda a arvore, as
    // mesmas sequencias de capturas transpoem sem parar, e cada acerto poupa
    // nao um no' mas a cauda toda atras dele.
    Entrada e;
    bool    tem = p_tab->sonda(pos.key(), e);
    Move    tt_lance = tem ? e.melhor : Move::none();
    if (tem && e.tem_limite()) {
        int sc = nota_da_tt(e.nota, ply);
        bool serve = e.limite == Limite::Exacto
                  || (e.limite == Limite::Inferior && sc >= beta)
                  || (e.limite == Limite::Superior && sc <= alpha);
        if (serve)
            return sc;
    }

    // Ficar quieto: quem joga nao e' obrigado a capturar, portanto a estatica
    // e' um chao. Nao em xeque, onde tudo e' forcado e nao ha' chao nenhum.
    int estatica = TT_SEM_AVAL;
    int chao     = TT_SEM_AVAL;
    if (!em_xeque) {
        if (tem && e.aval != TT_SEM_AVAL) {
            ++n_hit_qs;
            estatica = int(e.aval);
        } else {
            estatica = (++n_aval_qs, ++n_aval, int(DIAG.eval_crua ? av->avalia(pos) : av->avalia_cheia(pos, DIAG.eval_escala, otimismo_de(pos))));
            // Ver `qs_guarda_aval` nos parametros: sem isto o `chao >= beta`
            // logo abaixo leva a rede consigo pela porta fora.
            if (p.qs_guarda_aval)
                p_tab->guarda_so_aval(pos.key(), std::int16_t(estatica));
        }
        // O chao e' o melhor entre a estatica e o que a tabela ja' estabeleceu:
        // um limite guardado do lado certo da estatica veio de uma busca que foi
        // la' ver. Ficar no numero pior e' procurar capturas para chegar a um
        // valor que ja' se tinha na mao.
        chao = estatica;
        if (tem && e.tem_limite()) {
            int ts = nota_da_tt(e.nota, ply);
            bool melhor = e.limite == Limite::Exacto
                       || (e.limite == Limite::Inferior && ts > estatica)
                       || (e.limite == Limite::Superior && ts < estatica);
            if (melhor)
                chao = ts;
        }
        if (chao >= beta)
            return chao;
        if (chao > alpha)
            alpha = chao;
    }

    Lista L;
    if (em_xeque)
        for (const auto& m : MoveList<LEGAL>(pos))
            L.junta(m);
    else
        for (const auto& m : MoveList<CAPTURES>(pos))
            if (pos.legal(m))
                L.junta(m);
    if (em_xeque && L.n == 0)
        return mated_in(ply);

    pontua(pos, L, 0, tt_lance, Limite::Nenhum, ply, 1);
    Move* lances = L.lances;
    int*  notas  = L.notas;
    int*  hist   = L.hist;

    int  melhor       = em_xeque ? -INFINITO : chao;
    Move melhor_lance = Move::none();
    // A casa que o lance anterior acabou de ocupar. Uma captura ai' e' a outra
    // metade de uma troca ja' comecada e nao entra na conta do travao.
    int casa_ant = (ply > 0 && jogado_pc[ply - 1] >= 0) ? jogado_para[ply - 1] : -1;

    for (int i = 0; i < L.n; ++i) {
        escolhe(lances, notas, hist, L.n, i);
        Move m = lances[i];

        // Travao da quiescencia e futilidade da quiescencia NAO: os dois
        // existem no half2k e estao DESLIGADOS (`TravaoQs=false`,
        // `QsFutility=false`).

        // Uma captura que perde material nao pode levantar o chao em que ja'
        // estamos, e ir atras dela e' como a quiescencia persegue todas as
        // recapturas ate' ao horizonte em vez de assentar.
        if (!em_xeque
            && !(p.qs_recaptura && int(m.to_sq()) == casa_ant)
            && !pos.see_ge(m, 0))
            continue;

        StateInfo st;
        // Ver `prefetch_antes`: o balde do filho pede-se ja', para a ida a`
        // memoria correr junto com o `do_move`.
        if (p.prefetch_antes)
            __builtin_prefetch(p_tab->first_entry(pos.prefetch_key(m)));
        chaves.push_back(pos.key());
        auto& sujas = av->pilha().push();
        pos.do_move(m, st, pos.gives_check(m), sujas, nullptr, nullptr);
        int nota = -quiescencia(pos, -beta, -alpha, ply + 1);
        pos.undo_move(m);
        av->pilha().pop();
        chaves.pop_back();

        if (parado)
            return 0;
        if (nota > melhor) {
            melhor = nota;
            if (nota > alpha) {
                alpha        = nota;
                melhor_lance = m;
                if (alpha >= beta)
                    break;
            }
        }
    }

    Limite lim = melhor >= beta        ? Limite::Inferior
               : melhor > alpha_orig   ? Limite::Exacto
                                       : Limite::Superior;
    p_tab->guarda(pos.key(), 0, nota_para_tt(melhor, ply), lim, melhor_lance, false,
                  em_xeque ? TT_SEM_AVAL : std::int16_t(estatica));
    return melhor;
}

// --- o no' ------------------------------------------------------------------

int Busca::negamax(Position& pos, int prof, int alpha, int beta, int ply, bool pv, bool cut) {
    pv_n[ply] = 0;

    if (prof <= 0)
        return quiescencia(pos, alpha, beta, ply);

    ++nos;
    if (DIAG.forma && prof < 64)
        ++g_forma[prof];
    if (ply > sel_prof)
        sel_prof = ply;
    if (sem_tempo())
        return 0;

    bool raiz     = ply == 0;
    bool em_xeque = bool(pos.checkers());
    // Repoe-se DOIS plies a` frente: o contador do ply seguinte tem de
    // sobreviver a esta visita para a reducao o poder ler.
    if (ply + 2 < MAX_PLY + 8)
        cut_cnt[ply + 2] = 0;

    if (!raiz) {
        if (pos.rule50_count() >= 100 || repeticao(pos))
            return valor_empate(pos);

        // Ha' um lance a` distancia que empata por repeticao?
        if (p.usa_cuckoo && !(p.cuckoo_sem_inc && !tem_incremento)
            && alpha < valor_empate(pos) && pos.upcoming_repetition(ply)) {
            alpha = valor_empate(pos);
            if (alpha >= beta)
                return alpha;
        }
        if (ply >= MAX_PLY - 1)
            return em_xeque ? VALUE_ZERO : (++n_aval, DIAG.eval_crua ? av->avalia(pos) : av->avalia_cheia(pos, DIAG.eval_escala, otimismo_de(pos)));

        // TABELAS DE FINAIS.
        //
        // Abaixo do tamanho que as tabelas cobrem, o resultado NAO e' uma
        // estimativa: e' a verdade. Devolve-la acaba a sub-arvore de uma vez, e
        // acaba-a com a resposta certa -- o que vale mais do que os nos
        // poupados, porque os finais que as tabelas cobrem sao justamente os que
        // uma rede le' pior.
        //
        // A distancia a` raiz entra na nota para uma vitoria curta continuar a
        // ser preferida a uma longa. Sem isso, todos os lances que mantem a
        // vitoria valem o mesmo, a busca nao tem por onde escolher, e uma torre
        // a mais acaba em empate pela regra dos cinquenta lances -- foi medido
        // no half2k: rei e torre contra rei passou de `a1a6`, que aperta o rei,
        // para `e1e2`, que nao faz nada.
        if (Tablebases::MaxCardinality > 0
            && pos.count<ALL_PIECES>() <= Tablebases::MaxCardinality
            && pos.rule50_count() == 0 && !pos.can_castle(ANY_CASTLING)) {
            Tablebases::ProbeState estado;
            Tablebases::WDLScore   w = Tablebases::probe_wdl(pos, &estado);
            if (estado != Tablebases::FAIL) {
                ++tb_acertos;
                int v = w >= Tablebases::WDLCursedWin  ? VALUE_TB_WIN_IN_MAX_PLY - ply
                      : w <= Tablebases::WDLBlessedLoss ? VALUE_TB_LOSS_IN_MAX_PLY + ply
                                                        : VALUE_DRAW;
                Limite lim = w >= Tablebases::WDLCursedWin    ? Limite::Inferior
                           : w <= Tablebases::WDLBlessedLoss  ? Limite::Superior
                                                              : Limite::Exacto;
                p_tab->guarda(pos.key(), MAX_PLY - 1, nota_para_tt(v, ply), lim, Move::none(),
                              pv, TT_SEM_AVAL);
                return v;
            }
        }

        // Distancia ao mate: nenhuma linha daqui pode bater um mate ja'
        // encontrado mais perto da raiz.
        int a = std::max(alpha, mated_in(ply));
        int b = std::min(beta, mate_in(ply + 1));
        if (a >= b)
            return a;
        alpha = a;
    }

    Entrada e;
    bool    tem      = p_tab->sonda(pos.key(), e);
    Move    tt_lance = tem ? e.melhor : Move::none();
    Limite  tt_lim   = tem ? e.limite : Limite::Nenhum;
    bool    tt_pv    = pv || (tem && e.pv);

    // `excluido[ply] == none` E' OBRIGATORIO. Um no' que procura com um lance
    // excluido esta' a fazer uma pergunta diferente da que a tabela respondeu, e
    // a resposta guardada INCLUI o lance que estamos a fingir que nao existe.
    //
    // Sem esta guarda a busca singular devolvia imediatamente a nota da tabela,
    // portanto dizia sempre "nao e' singular". MEDIDO: a extensao singular assim
    // encolhia a arvore 46% (62.743 -> 29.646 nos na inicial a` profundidade 12)
    // e desliga-la valia +83,20 +/- 21,79 Elo em 400 partidas. Uma extensao que
    // ENCOLHE a arvore esta' avariada, e era este o motivo.
    if (tem && !pv && excluido[ply] == Move::none() && e.prof >= prof && e.tem_limite()) {
        int  s     = nota_da_tt(e.nota, ply);
        bool serve = e.limite == Limite::Exacto
                  || (e.limite == Limite::Inferior && s >= beta)
                  || (e.limite == Limite::Superior && s <= alpha);
        // Nao perto da parede dos cinquenta lances: ali a mesma posicao vale
        // coisas diferentes conforme o contador, e a tabela nao sabe qual delas
        // guardou.
        if (serve && pos.rule50_count() < 90)
            return s;
    }

    // Ver `corr_prefetch` nos parametros: a correccao pede-se ja', e chega
    // enquanto a rede avalia.
    if (p.corr_prefetch && !em_xeque && !DIAG.sem_corr && !corr->empty()) {
        int idx[6];
        indices(pos, ply, idx);
        const int lado = int(pos.side_to_move());
        for (int k = 0; k < CORR_FAM; ++k)
            if (idx[k] >= 0)
                __builtin_prefetch(&(*corr)[(k * 2 + lado) * CORR_TAM + idx[k]]);
    }

    // O numero CRU fica a` parte, porque e' o que vai para a tabela la' em
    // baixo. Guardar o corrigido e' um desastre silencioso: a visita seguinte
    // le'-o, aplica a correccao outra vez, guarda isso, e o erro compoe-se a cada
    // passagem sem nada parecer errado de fora.
    int estatica = TT_SEM_AVAL;
    if (!em_xeque) {
        if (tem && e.aval != TT_SEM_AVAL) {
            ++n_hit_bs;
            estatica = int(e.aval);
        } else {
            estatica = (++n_aval_bs, ++n_aval, int(DIAG.eval_crua ? av->avalia(pos) : av->avalia_cheia(pos, DIAG.eval_escala, otimismo_de(pos))));
            // Escrever a avaliacao no momento em que ela e' calculada faz o
            // trabalho sobreviver a`s catorze maneiras de sair deste no' antes
            // de se chegar ao guardar la' em baixo. A perda era invisivel:
            // aparecia como entrada AUSENTE, e por isso contava como primeira
            // visita.
            p_tab->guarda_so_aval(pos.key(), std::int16_t(estatica));
        }
    }
    // O numero CRU fica a` parte: e' o que vai para a tabela la' em baixo. A
    // tabela e' partilhada e quem a le' depois aplica a SUA correccao; guardar
    // o corrigido faria a visita seguinte corrigir duas vezes, guardar isso, e o
    // erro compunha-se a cada passagem sem nada parecer errado de fora.
    int estatica_crua = estatica;
    if (!em_xeque && estatica != TT_SEM_AVAL)
        estatica = corrigida(pos, estatica, ply);

    aval_ply[ply] = estatica;

    auto usavel = [](int v) { return v != TT_SEM_AVAL; };
    // Um ply passado em xeque nao tem estatica e a casa dele tem um sentinela,
    // nao uma pontuacao. Comparar contra o sentinela fazia parecer que TUDO
    // estava a melhorar durante dois plies depois de qualquer xeque, porque
    // qualquer coisa bate menos trinta e dois mil.
    bool melhorando = false;
    if (!em_xeque) {
        if (ply >= 2 && usavel(aval_ply[ply - 2]))
            melhorando = estatica > aval_ply[ply - 2];
        else if (ply >= 4 && usavel(aval_ply[ply - 4]))
            melhorando = estatica > aval_ply[ply - 4];
    }
    // O adversario piorou: a nossa estatica esta' acima do SIMETRICO da estatica
    // do ply anterior. Nao e' o mesmo que `melhorando`, que compara connosco
    // proprios dois plies atras -- este compara com o lado de la'.
    // So' a forma DESLIGADA da futilidade inversa (`RfpSf`) le' isto. Fica
    // calculado e por usar, de proposito: quando esse interruptor entrar, entra
    // com o termo ja' escrito e nao inventado nessa altura.
    [[maybe_unused]] bool adv_piorou =
      ply >= 1 && usavel(aval_ply[ply - 1]) && estatica > -aval_ply[ply - 1];

    // Duas avaliacoes daqui para a frente, e nao sao o mesmo numero. A
    // `estatica` e' o que a rede diz. A `aval_poda` e' essa melhorada pelo que a
    // tabela ja' sabe: um limite inferior guardado acima da estatica, ou um
    // superior abaixo dela, e' por definicao uma estimativa melhor -- uma busca
    // foi la' ver. A poda de no' inteiro devia usar a melhor, e usava a pior.
    int aval_poda = estatica;
    if (!em_xeque && tem && e.tem_limite()) {
        int  ts = nota_da_tt(e.nota, ply);
        bool melhor = e.limite == Limite::Exacto
                   || (e.limite == Limite::Inferior && ts > estatica)
                   || (e.limite == Limite::Superior && ts < estatica);
        if (melhor)
            aval_poda = ts;
    }

    if (!pv && !em_xeque && usavel(estatica)) {
        // Futilidade inversa: tao a` frente que dar a margem toda ainda bate
        // o beta.
        //
        // Esta e' a forma que o half2k USA por omissao: margem linear na
        // profundidade, com um desconto fixo quando as coisas estao a melhorar.
        // A outra forma -- multiplicador que satura, desconto proporcional a`
        // margem, guarda pelo lance da tabela -- esta' escrita no half2k mas
        // DESLIGADA (`RfpSf=false`). Portei-a por engano: portei o codigo em vez
        // da configuracao. Fica para interruptor, nao para omissao.
        // O ADVERSARIO PIOROU: a nossa estatica esta' acima do simetrico da dele.
    // Sinal deles que nao tinhamos -- entra no desconto da margem, porque um no'
    // onde o adversario acabou de piorar merece podar mais.
    bool pior_adv = ply >= 1 && usavel(aval_ply[ply - 1])
                 && estatica > -aval_ply[ply - 1];
    // O LANCE DA TABELA E' UMA CAPTURA. Eles so' fazem futilidade inversa quando
    // NAO ha' lance na tabela ou o que la' esta' e' uma captura: com um tranquilo
    // guardado, a entrada ja' diz que ha' um plano tranquilo bom e cortar pela
    // estatica descarta-o.
    bool tt_capt = tem && tt_lance != Move::none() && pos.capture_stage(tt_lance);
    int margem = std::min(p.rfp_margem + prof * p.rfp_mult, p.rfp_tecto);
        margem = margem * prof - p.rfp_melhorando * int(melhorando);
        if (p.usa_pior_adv)
            margem -= p.rfp_adv_f * margem / 1024 * int(pior_adv);
        if (p.corr_marg_div > 0) {
            int _c = corrigida(pos, estatica, ply);
            int _d = std::abs(_c - estatica);
            extern std::uint64_t g_corr_n, g_corr_zero, g_corr_soma;
            ++g_corr_n;
            if (_d == 0) ++g_corr_zero;
            g_corr_soma += _d;
            margem += _d / p.corr_marg_div;
        }
        if (p.rfp_tecto_tot > 0)
            margem = std::min(margem, p.rfp_tecto_tot);
        if (prof < p.rfp_prof && aval_poda - margem >= beta
            && (!p.usa_tt_capt || tt_lance == Move::none() || tt_capt)
            && std::abs(aval_poda) < VALUE_MATE_IN_MAX_PLY) {
            // Parte do caminho ate' a` estimativa e nao o caminho todo: a margem
            // estabelece que o no' esta' acima do beta, nao por QUANTO, e
            // devolver a distancia inteira passa para cima uma confianca que
            // nunca foi ganha.
            return beta + (aval_poda - beta) / 3;
        }

        // Razoring: tao atras que nem a quiescencia deve encontrar o suficiente,
        // portanto pergunta-se-lhe directamente em vez de gastar largura na
        // resposta. Com a estatica CRUA: um limite vindo da tabela ja' pagou as
        // capturas, e perguntar com ele e' fazer uma pergunta ja' respondida.
        if (prof <= p.razor_prof && std::abs(alpha) < 2000
            && estatica + p.razor_margem * prof <= alpha) {
            int q = quiescencia(pos, alpha, alpha + 1, ply);
            if (q < alpha)
                return q;
        }

        // Lance nulo: da'-se um lance de borla ao adversario e ve'-se se a
        // posicao aguenta. Nao com so' peoes, onde passar e' muitas vezes o
        // melhor lance que ha' e a conclusao sairia errada.
        if (!DIAG.sem_null && (cut || DIAG.nmp_todos) && prof >= 3 && aval_poda >= beta && aval_poda >= estatica
            && estatica >= beta - 20 * prof - 40 * int(melhorando) + 100
            && pos.non_pawn_material(pos.side_to_move()) > 0
            && !(ply > 0 && nulo_em[ply - 1])) {
            // A reducao TEM de crescer com a profundidade. Sem o termo -- que e'
            // como estava -- reduz-se quatro plies a` profundidade 3 e os MESMOS
            // quatro a` profundidade 14. A pergunta e' sempre a mesma, mas o
            // custo de a fazer cresce com a profundidade, e a confianca na
            // resposta tambem.
            // SEM o termo da profundidade: existe no half2k e esta' desligado
            // (`NmpProfundidade=false`).
            // O divisor e o tecto do termo da estatica ficam em parametro
            // porque a forma deles e' outra: `/256 sem tecto` contra o nosso
            // `/200 com tecto 6`. A` profundidade 12 com a estatica no beta eles
            // reduzem 11 plies e nos 6 -- quase o dobro.
            int r = p.nmp_base
                  + std::min((aval_poda - beta) / std::max(p.nmp_div_est, 1), p.nmp_div);
            // O termo que faltava. O parametro `nmp_prof_div` existia e NAO era
            // usado -- a nota acima defendia-o e a linha nao o aplicava. Medido
            // a` profundidade 12: desligar o nosso lance nulo ENCOLHE a arvore
            // 4%, enquanto a` referencia desliga-lo incha-lha 80%. Uma reducao
            // que nao cresce faz a busca de verificacao correr quase tao fundo
            // como a real: custa o que poupa.
            // A DOSE, nao um sim/nao. `nmp_prof_div` divide a profundidade:
            // 2 reduz o dobro de 4. Fica em ambiente porque a constante certa
            // NAO se herda -- `nmp_base`, `nmp_div` e as margens todas foram
            // varridas contra a arvore ANTIGA, e mudar-lhe a forma torna-as
            // desafinadas. Cada mudanca grande obriga a recalibrar o que a
            // rodeia; medir so' ligado/desligado mede a mudanca agrilhoada a
            // constantes que ja' nao lhe servem.
            // MEDIDO E ADOPTADO. Sem este termo a reducao nao cresce: quatro
            // plies a` profundidade 3 e os MESMOS quatro a` 14, portanto a busca
            // de verificacao corre quase tao fundo como a real e custa o que
            // poupa. Desligar o lance nulo chegava a ENCOLHER a arvore 4%,
            // quando a` referencia desliga-lo incha-lha 80%.
            //
            // Com o termo: -27% de nos e -28% de tempo ate' a` profundidade 12,
            // seis posicoes. Em partidas, +18,2 Elo em 1604 (duelo) e ultimo
            // lugar para o "sem termo" numa escada de cinco com 400 por ponta.
            // A dose 5 saiu de la'; o 3 que estava escrito no ficheiro era pior,
            // e a contagem de nos tambem escolhia o 3.
            if (p.nmp_prof_div > 0 && !DIAG.sem_nmp_prof)
                r += prof / p.nmp_prof_div;
            StateInfo st;
            chaves.push_back(pos.key());
            nulo_em[ply] = true;
            jogado_pc[ply] = -1;
            pos.do_null_move(st);
            int nota = -negamax(pos, prof - r, -beta, -beta + 1, ply + 1, false, !cut);
            pos.undo_null_move();
            nulo_em[ply] = false;
            chaves.pop_back();
            if (nota >= beta)
                // Um mate vindo de uma busca com lance nulo e' um artefacto do
                // lance de borla; devolve-se o limite.
                return e_mate(nota) ? beta : nota;
        }
    }

    // Nada na tabela para um no' desta profundidade quer dizer que nao ha' lance
    // que valha a pena tentar primeiro, e procurar a` profundidade toda para
    // descobrir um custa mais do que o encontrar um ply mais raso e voltar.
    // A IIR. Tres gates deles que nos nao tinhamos, e o do meio e' o que o
    // Triumviratus identificou como diferenca estrutural para o SF19 ("no IIR at
    // ALL nodes", num pacote de quatro que lhe valeu +5,31 +/- 3,44 em 11.182
    // partidas):
    //
    //   deles   !followPV && !allNode && depth >= 6 && !ttMove
    //   nos     prof >= 4 && !ttLance
    //
    // Um no' ALL espera falhar em baixo: vai ter de procurar tudo de qualquer
    // maneira, e reduzir-lhe a profundidade so' lhe tira qualidade sem poupar o
    // corte que nunca vai haver.
    bool no_all = !pv && !cut;
    if (!DIAG.sem_iir && prof >= p.iir_prof && tt_lance == Move::none()
        && !(p.iir_sem_all && no_all))
        --prof;

    // ProbCut COM BUSCA.
    //
    // Se uma captura -- verificada primeiro pela quiescencia, que e' barata, e so'
    // depois por uma busca rasa -- voltar muito acima do beta, entao a posicao e'
    // boa de mais para precisar da largura toda e corta-se ja'.
    //
    // A verificacao em DUAS FASES e' o que torna isto seguro e barato: a
    // quiescencia deita fora quase todos os candidatos antes de se gastar uma
    // busca em algum deles. Sem ela, tentar isto em cada captura custava mais do
    // que poupava.
    //
    // So' capturas: sao os lances que podem mudar o material o suficiente para
    // justificar uma margem de um peao, e ha' poucas por no'.
    //
    // O que se devolve nao e' o valor cru: e' descontado da margem, porque o que
    // ficou provado foi "acima do beta mais a margem" e nao o valor exacto.
    // `pc_prof > 0` explicito. Pus `pc_prof = 0` a pensar que desligava e a
    // condicao era `prof >= pc_prof`, que com zero LIGA SEMPRE -- a arvore foi a
    // 878.281 nos em vez dos 425.973 que eu esperava. E' a mesma armadilha dos
    // valores por omissao encostados a um limite, ao contrario.
    if (p.pc_prof > 0 && !pv && !em_xeque && excluido[ply] == Move::none() && prof >= p.pc_prof
        && std::abs(beta) < VALUE_MATE_IN_MAX_PLY && usavel(estatica)
        && !(tem && e.tem_limite() && nota_da_tt(e.nota, ply) < beta + p.pc_margem)
        // E so' quando ha' mesmo alguma coisa a provar: com a estatica ja' acima
        // do alvo, quem trata do no' e' a futilidade inversa e nao isto.
        && estatica < beta + p.pc_margem) {
        ++pc_nos;
        int pc_beta = beta + p.pc_margem - p.pc_impr * int(melhorando);
        int pc_prof = prof - (melhorando ? p.pc_red_impr : p.pc_reducao);

        Lista caps;
        for (const auto& mm : MoveList<CAPTURES>(pos))
            if (pos.legal(mm) && mm != excluido[ply]
                // So' as que, pela troca estatica, ja' chegam sozinhas ao que
                // falta -- `pc_beta - estatica` e nao metade disso.
                //
                // MEDIDO: com metade da margem a arvore subiu de 425.973 para
                // 633.461 nos a` profundidade 16. O filtro deixava passar
                // capturas que nunca poderiam provar nada, e cada uma custava
                // uma quiescencia. Um ProbCut que nao filtra e' so' trabalho.
                // O limiar nunca desce abaixo de zero.
                //
                // MEDIDO: sem este chao, quando a estatica ja' esta' acima do
                // `pc_beta` o filtro fica VAZIO e toda a captura e' tentada --
                // 9.898 tentativas com 65% de sucesso, que nao e' um corte
                // selectivo, e' fazer uma busca rasa em toda a parte. A arvore
                // subiu de 425.973 para 684.805 nos.
                && pos.see_ge(mm, std::max(lim_see(pc_beta - estatica), 0)))
                caps.junta(mm);

        for (int ci = 0; ci < caps.n; ++ci) {
            Move mm = caps.lances[ci];
            StateInfo st2;
            if (p.prefetch_antes)
                __builtin_prefetch(p_tab->first_entry(pos.prefetch_key(mm)));
            chaves.push_back(pos.key());
            jogado_pc[ply]   = idx_pc(type_of(pos.moved_piece(mm)));
            jogado_para[ply] = int(mm.to_sq());
            auto& sj = av->pilha().push();
            pos.do_move(mm, st2, pos.gives_check(mm), sj, nullptr, nullptr);
            ++pc_tentativas;
            int v = -quiescencia(pos, -pc_beta, -pc_beta + 1, ply + 1);
            if (v >= pc_beta) ++pc_passou_qs;
            if (v >= pc_beta && pc_prof > 0)
                v = -negamax(pos, pc_prof, -pc_beta, -pc_beta + 1, ply + 1, false, !cut);
            pos.undo_move(mm);
            av->pilha().pop();
            chaves.pop_back();
            if (parado)
                return 0;
            if (v >= pc_beta) {
                ++pc_cortes;
                p_tab->guarda(pos.key(), pc_prof + 1, nota_para_tt(v, ply), Limite::Inferior, mm,
                              tt_pv, std::int16_t(estatica_crua));
                if (!e_mate(v))
                    return v - (pc_beta - beta);
            }
        }
    }

    // O PROBCUT PEQUENO. Cinco linhas e ZERO busca.
    //
    // Nao e' o ProbCut de cima -- esse gasta uma busca de verificacao por
    // candidato e, medido em 866 partidas, nao vale nada nesta arvore. Este nao
    // gasta nada: le' a tabela e pergunta se ja' la' esta' a resposta.
    //
    // Se a entrada tem um limite INFERIOR bem acima do beta, vindo de uma busca
    // quase tao funda como esta, entao este no' ja' foi provado acima do beta
    // por outro caminho. Devolve-se o limiar e nao o valor: o que ficou provado
    // foi "acima disto", nao quanto.
    //
    // NAO e' nosso, e' um mecanismo que eles usam e que nos simplesmente nao
    // tinhamos. Os dois ganhos de hoje vieram os dois de restaurar coisas assim.
    // A margem fica em parametro para ser varrida aqui: a deles e' 428 numa
    // escala com o peao a ~208, o que da' ~2,06 peoes; na nossa, com o peao a
    // 200 depois da avaliacao completa, o equivalente ronda 412.
    if (p.pcp_margem > 0 && !pv && tem && e.limite == Limite::Inferior
        && e.prof >= prof - p.pcp_prof && std::abs(beta) < VALUE_MATE_IN_MAX_PLY) {
        int alvo = beta + p.pcp_margem;
        int sc   = nota_da_tt(e.nota, ply);
        if (sc >= alvo && std::abs(sc) < VALUE_MATE_IN_MAX_PLY)
            return alvo;
    }

    // A lista INTEIRA de uma vez. A geracao por etapas existe no half2k e
    // esta' desligada (`GeraEtapas=false`).
    Lista L;
    for (const auto& m : MoveList<LEGAL>(pos))
        L.junta(m);
    if (L.n == 0)
        return em_xeque ? mated_in(ply) : VALUE_DRAW;

    pontua(pos, L, 0, tt_lance, tt_lim, ply, prof);
    Move* lances = L.lances;
    int*  notas  = L.notas;
    int*  hist   = L.hist;

    int   melhor_nota  = -INFINITO;
    Move  melhor_lance = Move::none();
    int   alpha_orig   = alpha;
    bool  algum        = false;
    bool  tranquilos_pontuados = false;
    Move tranquilos_vistos[MAX_LANCES]; int n_tranq = 0;
    Move capturas_vistas[MAX_LANCES];   int n_capt  = 0;
    // Uma vez esgotados os tranquilos por contagem ou por margem, os que
    // faltam saltam-se -- mas as CAPTURAS atras deles nao, que foi o erro de
    // uma versao anterior no half2k.
    bool saltar_tranquilos = false;

    for (int i = 0; i < L.n; ++i) {
        const std::uint64_t nos_antes_deste = nos;
        algum = true;
        escolhe(lances, notas, hist, L.n, i);

        // A pergunta a`s tabelas e' feita DEPOIS do `escolhe` e sobre o lance
        // escolhido: o primeiro tranquilo por pontuar a ser escolhido e'
        // exactamente o momento em que a tabela, as capturas boas e as promocoes
        // se esgotaram. Perguntar antes obrigava a uma varredura por cada lance.
        if (!tranquilos_pontuados && notas[i] == TRANQUILO_POR_PONTUAR) {
            if (p.hist_prefetch)
                pede_historias(pos, lances, notas, i, L.n, ply);
            for (int j = i; j < L.n; ++j)
                if (notas[j] == TRANQUILO_POR_PONTUAR) {
                    // A ordem e a poda leem o MESMO numero: `hist_de`, e nada
                    // mais.
                    //
                    // Eu tinha aqui tres coisas a mais -- a principal escalada
                    // por 138/32, a tabela peca-casa e um bonus de 16384 para
                    // xeques. As tres existem no half2k e as tres vivem no ramo
                    // que so' corre com `PontuaTarde=false`, ou entao em
                    // interruptores desligados (`HistPc=false`,
                    // `OrdemPosicao=false`). No caminho que o motor percorre
                    // mesmo, o tranquilo vale exactamente o que as tabelas
                    // dizem dele e mais nada.
                    hist[j]  = hist_de(pos, lances[j], ply);
                    // O XEQUE, no sitio em que ela o poe: so' na ORDEM.
                    //
                    // A poda pelo historico le' `hist[]`, e um lance que da'
                    // xeque nao e' um lance com historia -- e' um lance que
                    // forca. Somar-lho ali mudava a poda a fingir que era
                    // ordenacao, que e' o erro que este ficheiro ja' regista
                    // duas vezes.
                    //
                    // O `see_ge` so' se chama quando a casa de chegada ja'
                    // esta' no mapa de xeques desta peca -- raro. A ordem dos
                    // dois testes e' a dela e poupa quase todas as trocas.
                    notas[j] = DIAG.ordem_escalada
                                 ? ordena(pos, lances[j], ply)
                                 : hist[j];
                    // A parcela das ameacas entra AQUI e nao no `hist_de`: a
                    // ordem leva-a, a poda pelo historico nao.
                    if (p.ameaca_f != 0)
                        notas[j] += ameaca(pos, lances[j], ply);
                    if (p.ordem_xeque_f != 0) {
                        Move mm = lances[j];
                        if ((pos.check_squares(type_of(pos.moved_piece(mm))) & mm.to_sq())
                            && (p.ordem_xeque_see == 0 || pos.see_ge(mm, p.ordem_xeque_see)))
                            notas[j] += p.ordem_xeque_f;
                    }
                }
            tranquilos_pontuados = true;
            escolhe(lances, notas, hist, L.n, i);
        }

        Move m = lances[i];
        // Um no' que procura com um lance excluido esta' a fazer uma pergunta
        // diferente da que a tabela respondeu.
        if (m == excluido[ply])
            continue;
        bool tranquilo = !pos.capture_stage(m) && m.type_of() != PROMOTION;

        int extensao = 0;
        // Sem extensoes: nenhum ramo ganha profundidade. Mede o que as
        // extensoes -- singular, dupla, negativa -- estao a comprar.
        const bool ext_ligadas = !DIAG.sem_ext;

        // Uma vez esgotados os tranquilos, os que faltam saltam-se. As capturas
        // atras deles NAO: elas nao falham o mesmo teste, e uma regra sobre
        // lances tranquilos a apagar capturas foi um defeito real no half2k.
        if (saltar_tranquilos && tranquilo)
            continue;

        // A ordem abaixo nao e' nossa para escolher: contagem de lances,
        // historico, margem estatica, troca -- a pergunta mais barata primeiro,
        // para as caras nunca serem feitas sobre um lance que ja' se foi.
        //
        // Este e' o ramo POR OMISSAO do half2k. O outro -- o que julga o lance a`
        // profundidade REDUZIDA, com margem quadratica no SEE e o historico a
        // devolver profundidade -- esta' escrito la' e DESLIGADO (`PodaSF=false`,
        // `PodaReduzida=false`), e foi esse que eu portei por engano. Aqui a
        // profundidade que julga e' a NOMINAL.
        if (!raiz && !pv && !em_xeque && melhor_nota > -VALUE_MATE_IN_MAX_PLY
            && pos.non_pawn_material(pos.side_to_move()) > 0 && usavel(estatica)) {
            // Ver `poda_red` no `busca.h`. A zero por omissao, e entao isto e'
            // exactamente `prof`.
            int prof_poda = prof;
            if (p.poda_red) {
                int r = lmr[std::min(prof, 63)][std::min(int(i), 63)] / 1024;
                prof_poda = std::max(prof - r, 0);
            }
            if (tranquilo) {
                // Poda por contagem: passado um certo numero de lances a` pouca
                // profundidade, a ordenacao ja' errou vezes suficientes para os
                // que faltam nao valerem os nos. Para os TRANQUILOS, nao o ciclo:
                // as capturas que perdem material pontuam abaixo de todo o
                // tranquilo e ficam no fim da lista.
                int conta = p.lmp_base + prof * prof;
                // Quem nao esta' a melhorar ve' o limite a meio. Do
                // `ks_1.20260919` (`43818c`); a zero por omissao.
                if (p.lmp_melhora && !melhorando)
                    conta /= 2;
                if (prof <= p.lmp_prof && int(i) >= conta) {
                    saltar_tranquilos = true;
                    continue;
                }

                // Poda pelo historico. O limiar cresce com o QUADRADO da
                // profundidade, para so' morder onde errar e' barato. A constante
                // esta' nas NOSSAS unidades e teve de estar: vinda de um motor
                // cujas tabelas vao a ~105000 contra as nossas que topam perto de
                // 24500, nunca disparava -- duas corridas com contagens de nos
                // identicas ao byte, que e' o aspecto de um ramo morto.
                // O PAR: o tecto de profundidade e a forma da margem.
                //
                //   eles   history < -4136 * depth        sem tecto, linear
                //   nos    prof <= 4 && hist < -600*prof^2  tecto em 4, quadratica
                //
                // Deixamos de podar pelo historico acima do ply 4 e eles nunca
                // deixam -- e a forma da arvore diz o mesmo: 57,3% dos nossos
                // nos vivem nos primeiros tres plies contra 40,2% deles, e
                // chegamos acima do ply 14 catorze vezes menos.
                //
                // Sao UM par, nao duas mudancas: com o tecto em 4 a margem so'
                // manda em quatro plies, e com a margem quadratica alargar o
                // tecto poe limiares que o historico nunca alcanca. O
                // Triumviratus mediu isto exactamente assim (`ContHistPruneDepth
                // 2 -> 6 com HistPruneMargin 2097 -> 1200`, +4,48 +/- 2,65 em
                // 19.228 partidas) e registou que separadas sao inertes.
                {
                    int lim_h = p.hpoda_lin ? p.hist_poda * prof
                                            : p.hist_poda * prof * prof;
                    if (prof <= p.hpoda_prof && hist[i] < -lim_h)
                        continue;
                }

                // Futilidade: mesmo dada a margem este lance nao chega ao alpha,
                // e um tranquilo nao muda o material para compensar. O termo do
                // historico pertence aqui: um lance de que as tabelas gostam vale
                // a tentativa mesmo quando a margem diz que nao.
                if (prof_poda <= p.fut_prof
                    && estatica + p.fut_base + p.fut_decl * prof_poda
                         + hist[i] / std::max(p.fut_hist_div, 1)
                       <= alpha) {
                    saltar_tranquilos = true;
                    continue;
                }

                // Um tranquilo tambem pode perder material. A troca estatica
                // diz-o antes de a busca ter de o descobrir, e pergunta-se por
                // ultimo porque e' a mais cara.
                if (prof_poda <= 8
                    && !pos.see_ge(m, lim_see(-p.see_poda_tranq
                                              * (prof_poda + prof_poda * prof_poda))))
                    continue;
            } else {
                // Uma captura que perde mais do que a profundidade poderia
                // plausivelmente reaver.
                if (prof <= 8 && !pos.see_ge(m, lim_see(-p.see_poda * prof)))
                    continue;
            }
        }

        // Extensao singular. Se a tabela diz que este lance corta, procura-se
        // cada um dos OUTROS contra uma janela logo abaixo disso. Se todos
        // ficarem aquem, este e' o unico que segura a posicao, e uma linha que
        // pende de um lance merece mais um ply.
        if (!raiz && excluido[ply] == Move::none() && m == tt_lance
            && prof >= p.sing_prof && ply < MAX_PLY - 8 && tem && e.tem_limite()
            && e.prof >= prof - 3
            && (e.limite == Limite::Inferior
                || (!p.sing_so_inferior && e.limite == Limite::Exacto))) {
            int ts = nota_da_tt(e.nota, ply);
            if (!e_mate(ts)) {
                int alvo = ts - p.sing_margem * prof;
                excluido[ply] = m;
                std::uint64_t antes_sing = nos;
                int sc = negamax(pos, (prof - 1) / 2, alvo - 1, alvo, ply, false, cut);
                excluido[ply] = Move::none();
                ++conta_sing; nos_sing += nos - antes_sing;
                if (parado)
                    return 0;
                if (sc < alvo) {
                    ++conta_ext1;
                    extensao = ext_ligadas ? 1 : 0;
                    // Nao apenas singular mas singular por uma distancia. So' fora
                    // da variante principal, onde errar custa uma sub-arvore e nao
                    // o lance que se joga.
                    bool dupla_ok = p.ext_dupla_exacto || e.limite != Limite::Exacto;
                    if (!pv && dupla_ok && sc < alvo - p.ext_dupla) { ++conta_ext2; extensao = ext_ligadas ? 2 : 0; }
                } else if (alvo >= beta && !DIAG.sing_sem_corte) {
                    // Todos os outros tambem batem o beta: a posicao esta' ganha
                    // por razoes que nao dependem deste lance.
                    return alvo;
                } else if (!pv && !e_mate(sc) && sc >= beta
                           && !DIAG.sing_sem_corte) {
                    return sc;
                } else if (ts >= beta) {
                    // (o quanto fica no `p.ext_neg`)
                    // A tabela diz que este lance corta e a busca acabou de dizer
                    // que nao e' o unico. Um no' com varias boas respostas e' o
                    // oposto do caso que merece extensao.
                    ++conta_ext_neg;
                    extensao = ext_ligadas ? -p.ext_neg : 0;
                }
            }
        }

        StateInfo st;
        // Ver `prefetch_antes`. Com a manete ligada o pedido sai AQUI, antes do
        // xeque e do `do_move`, que e' o trabalho que esconde a latencia; com ela
        // desligada sai la' em baixo, depois do `do_move`, como estava.
        if (p.prefetch_antes)
            __builtin_prefetch(p_tab->first_entry(pos.prefetch_key(m)));
        chaves.push_back(pos.key());
        jogado_pc[ply]   = idx_pc(type_of(pos.moved_piece(m)));
        jogado_para[ply] = int(m.to_sq());
        // Pede-se a entrada do filho agora. A sondagem acontece uma chamada de
        // funcao e uma deteccao de xeque depois, o que chega para cobrir parte
        // da ida a` memoria -- e essa ida era um quinto da busca toda.
        bool dava_xeque = pos.gives_check(m);
        // O `do_move` curto do substrato usa um rascunho para as pecas sujas e
        // deita-o fora: com ele, o acumulador da rede NUNCA e' actualizado e a
        // avaliacao devolve o mesmo numero em toda a arvore. Foi exactamente
        // isso que aconteceu na primeira busca que correu aqui -- oito
        // profundidades, `score cp 0` em todas, e a variante a empurrar peoes
        // de torre. Uma avaliacao constante nao da' erro nenhum: da' um motor
        // que joga e nao ve'.
        auto& sujas = av->pilha().push();
        pos.do_move(m, st, dava_xeque, sujas, nullptr, nullptr);
        if (!p.prefetch_antes)
            __builtin_prefetch(p_tab->first_entry(pos.key()));

        // Extensao de xeque NAO: existe no half2k e esta' desligada
        // (`CheckExt=false`).

        int nova_prof = prof - 1 + extensao;
        int nota;
        bool reduzido = false, repetido = false;

        if (i == 0) {
            // O primeiro lance de um no' de PV leva a outro; em qualquer outro
            // sitio o filho espera o contrario de nos.
            nota = -negamax(pos, nova_prof, -beta, -alpha, ply + 1, pv, pv ? false : !cut);
        } else {
            reduzido = true;
            bool poucas = pos.count<ALL_PIECES>() <= p.lmr_pecas_fim;
            // So' os TRANQUILOS. Reduzir capturas existe no half2k e esta'
            // desligado (`LmrCaptures=false`).
            bool reduzivel = !poucas && tranquilo;
            int  r = 0;
            if (prof >= 3 && reduzivel && !em_xeque) {
                // A nota guardada bate o alpha? A entrada e' funda? Sao os dois
                // sinais que dizem quanto e' que este no' ja' provou de si.
                bool bate_alpha = tem && e.tem_limite() && nota_da_tt(e.nota, ply) > alpha;
                bool ent_funda  = tem && e.prof >= prof;
                r = reducao(prof, int(i), alpha, beta, melhorando, tranquilo, cut, pv, tt_pv,
                            tt_lance, hist[i], bate_alpha, ent_funda, cut_cnt[ply + 1])
                  / 1024;
                // O chao deixa de ser zero: com `lmr_ext_max` a reducao pode
                // ficar NEGATIVA, e ai' o lance e' ESTENDIDO em vez de reduzido.
                r = std::clamp(r, -p.lmr_ext_max, std::max(nova_prof - 1, 0));
            }
            // Uma busca reduzida com janela nula anda a` procura de uma razao
            // para parar, portanto trata-se o filho como esperando cortar.
            nota = -negamax(pos, nova_prof - r, -alpha - 1, -alpha, ply + 1, false, true);
            if (nota > alpha && r > 0) {
                repetido = true;
                // A RE-BUSCA PROPORCIONAL. Ate' aqui refazia-se SEMPRE a`
                // profundidade inteira, e o comentario que estava neste sitio
                // chamava a isso "o mais caro dos tres casos possiveis" e
                // deixava o mecanismo por ligar (`reb_fundo` e `reb_raso`
                // declarados, nunca lidos).
                //
                // A ideia: a busca reduzida ja' disse alguma coisa sobre o
                // lance. Se ele passou o alpha por MUITO, vale a pena olhar um
                // ply mais fundo do que o nominal; se passou por pouco, um ply
                // menos chega. Refazer sempre ao mesmo sitio deita fora essa
                // informacao, e as re-buscas sao metade da razao de o nosso nps
                // ser 1,45x pior do que o da referencia.
                int prof_re = nova_prof;
                // So' com um `melhor_nota` que exista. No primeiro lance do no'
                // ele ainda e' -INFINITO, e ai' a comparacao "passou por muito"
                // e' verdadeira sempre: estendia-se TODA a primeira re-busca de
                // todos os nos. Medido com o defeito: +13% de nos e +20% de
                // tempo, o que deitava fora o ganho inteiro do lance nulo.
                if (DIAG.reb_fina && melhor_nota > -VALUE_MATE_IN_MAX_PLY) {
                    bool fundo = nota > melhor_nota + p.reb_fundo + 2 * nova_prof;
                    bool raso  = nota < melhor_nota + p.reb_raso;
                    prof_re += int(fundo) - int(raso);
                    // Nunca abaixo da profundidade que ja' se procurou: seria
                    // repetir a mesma busca e chamar-lhe verificacao.
                    prof_re = std::max(prof_re, nova_prof - r + 1);
                }
                nota = -negamax(pos, prof_re, -alpha - 1, -alpha, ply + 1, false, !cut);
            }
            if (nota > alpha && nota < beta) {
                // A janela nula nao chegou: e' preciso o valor exacto, e a busca
                // anterior so' deu um limite.
                nota = -negamax(pos, nova_prof, -beta, -alpha, ply + 1, true, false);
            }
        }

        pos.undo_move(m);
        av->pilha().pop();
        chaves.pop_back();

        // Um tranquilo que foi reduzido e depois teve de ser procurado outra vez
        // disse alguma coisa das duas maneiras: ou valeu a segunda olhada, ou
        // nao valeu. As duas ficam registadas, e nenhuma delas aparece no
        // credito do corte, que so' ve' o lance que fechou o no'.
        if (reduzido && repetido && tranquilo)
            credita(pos, m, ply, nota > melhor_nota ? bonus_hist(prof) : -bonus_hist(prof));

        if (parado)
            return 0;

        // So' na raiz: desiste-se de uma iteracao que ja' passou muito do que
        // estava previsto para o lance INTEIRO. O limite mole so' e' consultado
        // ENTRE iteracoes, portanto uma iteracao longa passa-lhe ao lado e so' a
        // parede a apanha -- muito mais tarde e muito mais caro.
        //
        // Seguro aqui porque um lance da raiz que acabou tem nota a serio: o
        // melhor ate' agora e' mesmo o melhor ate' agora, e nao o primeiro que
        // calhou.
        if (raiz && i > 0 && duro.count() > 0
            && std::chrono::duration_cast<std::chrono::milliseconds>(
                 std::chrono::steady_clock::now() - inicio)
                 >= mole * 2)
            parado = true;

        if (raiz) {
            notas_raiz.push_back({m, nota});
            if (i < MAX_LANCES)
                nos_por_lance[i] = nos - nos_antes_deste;
        }
        if (tranquilo) {
            if (n_tranq < MAX_LANCES) tranquilos_vistos[n_tranq++] = m;
        } else {
            if (n_capt < MAX_LANCES) capturas_vistas[n_capt++] = m;
        }

        if (nota > melhor_nota) {
            melhor_nota  = nota;
            // Ver `lance_so_alpha` nos parametros: num no' ALL nao ha' melhor
            // lance, ha' o menos mau -- e guarda-lo apaga da tabela o lance bom
            // que la' estava.
            if (p.lance_so_alpha == 0 || nota > alpha)
                melhor_lance = m;
            if (nota > alpha) {
                alpha = nota;
                // Ver a nota do `alpha_desc` nos parametros: com um melhor a
                // serio na mao, os lances seguintes procuram-se mais raso.
                //
                // Mexe-se no `prof` e nao no `nova_prof` porque o desconto tem
                // de valer para todas as iteracoes que faltam -- e tambem para
                // as margens e para a reducao, que leem o `prof`. O `prof` e'
                // copia local, portanto isto nao sai deste no'.
                //
                // As guardas: na raiz nunca (ha' que dar a todos os lances a
                // mesma profundidade para as notas se poderem comparar), so' com
                // profundidade para dar, e nunca em cima de um mate, onde a nota
                // nao e' uma medida de quanto.
                // SO' ABAIXO DO BETA. O `ks_1.20260919` tem esta guarda e esta
                // reconstrucao nao tinha (`negamax`, `43857a`):
                //
                //     43857a:  mov 0x74(%rbp),%r9d     ; alpha_desc > 0
                //     438583:  cmp %r12d,0x30(%rsp)    ; nota < beta   <-- faltava
                //     438588:  jle -> salta
                //     43858f:  cmp %r11d,0x78(%rbp)    ; prof > ad_min
                //     438595:  cmp %r11d,0x7c(%rbp)    ; prof < ad_max
                //     4385a9:  cmp $0xf813 / cmovae    ; nunca sobre um mate
                //     4385b3:  mov %edi,0x4(%rsp)      ; prof -= alpha_desc
                //
                // Sem ela o desconto dispara tambem no lance que CORTA -- e o
                // `prof` ainda e' lido depois do ciclo: a TT guarda uma
                // profundidade mais rasa do que a que foi provada, e a `credita`
                // da' um bonus menor ao lance que cortou. Sondas piores e ordem
                // pior: e' a explicacao mais simples para a arvore CRESCER quando
                // se liga um mecanismo que devia encolhe-la (a nota nos
                // parametros regista 123.557 -> 195.386 -> 235.019 nos).
                if (!raiz && p.alpha_desc > 0 && nota < beta
                    && prof > p.ad_min && prof < p.ad_max
                    && std::abs(nota) < VALUE_MATE_IN_MAX_PLY)
                    prof -= p.alpha_desc;
                // A variante principal deste ply e' este lance seguido da do
                // filho.
                pv_tab[ply][0] = m;
                // O comprimento tem de caber na linha. Sem o tecto, uma variante
                // que chegue ao fundo faz `pv_n[ply] = MAX_PLY` e o ply de cima
                // escreve em `pv_tab[ply][MAX_PLY]` -- uma casa a mais, fora do
                // vector. Com as listas em vectores isto passava despercebido;
                // com elas em campos fixos passou a estragar o que estava ao
                // lado, e a busca comecou a rebentar a` profundidade 8.
                int n_filho = pv_n[ply + 1];
                if (n_filho > MAX_PLY - 2)
                    n_filho = MAX_PLY - 2;
                for (int j = 0; j < n_filho; ++j)
                    pv_tab[ply][j + 1] = pv_tab[ply + 1][j];
                pv_n[ply] = n_filho + 1;
                if (raiz) {
                    melhor_raiz = m;
                    nota_raiz   = nota;
                }
                if (alpha >= beta) {
                    // Actualizacoes pequenas e pouco frequentes escalam bem:
                    // so' conta quando NAO houve extensao dupla, ou em no' de PV.
                    cut_cnt[ply] += int(extensao < 2 || pv);
            if (DIAG.forma) { ++g_cortes; if (i == 0) ++g_cortes_1; }
                    // O corte. Credita-se o lance que o fez e castiga-se o que
                    // foi tentado antes dele e nao o fez -- as duas metades da
                    // mesma prova.
                    //
                    // As capturas ja' tentadas sao castigadas SEMPRE, mesmo
                    // quando quem cortou foi um tranquilo: elas foram tentadas
                    // primeiro, por serem capturas, e nao cortaram.
                    if (tranquilo) {
                        if (p.usa_killers > 0 && killers[ply][0] != m) {
                            killers[ply][1] = killers[ply][0];
                            killers[ply][0] = m;
                        }
                        int bonus = bonus_hist(prof);
                        credita(pos, m, ply, bonus);
                        for (int q = 0; q < n_tranq; ++q)
                            if (tranquilos_vistos[q] != m)
                                credita(pos, tranquilos_vistos[q], ply, -bonus);
                    } else {
                        credita_captura(pos, m, bonus_capt(prof));
                    }
                    // `c < n_capt`, e nao um `for` de intervalo sobre o vector.
                    //
                    // Quando estas listas passaram de `std::vector` para vectores
                    // FIXOS de 256, este ciclo ficou a percorrer as 256 casas --
                    // as 250 por preencher incluidas. O lixo delas era lido como
                    // lances e ia direito a` tabela de capturas, com indices
                    // negativos. A busca rebentava a` profundidade 8.
                    if (!DIAG.sem_capt_pen)
                        for (int c = 0; c < n_capt; ++c)
                            if (capturas_vistas[c] != m)
                                credita_captura(pos, capturas_vistas[c], -bonus_capt(prof));
                    break;
                }
            }
        }
    }

    // Com geracao por etapas, uma posicao AFOGADA deixa de ser apanhada pela
    // verificacao de antes do ciclo: a lista das capturas nasce vazia, os
    // tranquilos sao gerados la' dentro e tambem nao ha' nenhum.
    if (!algum)
        return em_xeque ? mated_in(ply) : VALUE_DRAW;

    // Havia lances mas TODOS foram podados: nao ha' nota nenhuma para este no'.
    // Devolve-se o alpha e nao se guarda nada.
    //
    if (DIAG.margem_estudo && !em_xeque && estatica != TT_SEM_AVAL
        && melhor_nota > -VALUE_MATE_IN_MAX_PLY && prof >= 1 && prof < EST_PROF)
        g_desmente[prof].push_back(estatica - melhor_nota);
    // Sem isto a `melhor_nota` fica em -INFINITO e vai para a tabela: uma
    // entrada a dizer que esta posicao vale menos trinta e dois mil, lida por
    // toda a busca a seguir. E' o pior tipo de defeito porque nada da' erro e a
    // entrada envenenada sobrevive na tabela muito depois do no' que a escreveu.
    if (melhor_nota == -INFINITO)
        return alpha;

    // A correccao aprende-se aqui, com o no' fechado.
    //
    // So' onde a nota FINAL e' informacao sobre a posicao: abaixo da estatica e
    // abaixo do beta, portanto ha' um limite superior a provar que a estatica
    // era optimista; ou acima dela com um lance que mostre porque'. Em qualquer
    // outro sitio o numero e' produto de um corte e nao uma leitura da posicao.
    // As capturas ficam de fora: ali o salto vem do material e nao de uma ma'
    // leitura, e ensinaria a tabela ao contrario.
    if (!em_xeque && usavel(estatica) && std::abs(melhor_nota) < VALUE_MATE_IN_MAX_PLY
        && !(melhor_lance != Move::none() && pos.capture_stage(melhor_lance))
        && ((melhor_nota < estatica && melhor_nota < beta)
            || (melhor_nota > estatica && melhor_lance != Move::none())))
        aprende(pos, melhor_nota - estatica, prof, ply);

    Limite lim = melhor_nota >= beta      ? Limite::Inferior
               : melhor_nota > alpha_orig ? Limite::Exacto
                                          : Limite::Superior;
    // NAO se guarda quando ha' um lance excluido: o no' respondeu a uma pergunta
    // diferente da que a tabela guarda, e deixar la' a resposta dele engana quem
    // vier fazer a pergunta normal.
    if (excluido[ply] == Move::none())
        p_tab->guarda(pos.key(), prof, nota_para_tt(melhor_nota, ply), lim, melhor_lance, tt_pv,
                      em_xeque ? TT_SEM_AVAL : std::int16_t(estatica_crua));
    return melhor_nota;
}

// --- a raiz -----------------------------------------------------------------

// A janela estreita a` volta do que a iteracao anterior disse. Quase toda a
// gente falha dentro dela, e quando falha alarga-se em vez de se comecar de
// novo com a janela toda.
// O que vale um empate, do ponto de vista de quem joga NESTE no'.
//
// Negativo para o lado que manda na raiz -- e' a nos que um empate custa -- e
// positivo para o outro. Entre repetir e continuar a jogar, a busca passa a
// preferir jogar, e so' aceita o empate quando a alternativa e' mesmo pior.
int Busca::valor_empate(const Position& pos) const {
    if (p.contempt == 0)
        return VALUE_DRAW;
    // SE SABEMOS CONTRA QUEM JOGAMOS, e' isso que manda.
    if (p.elo_margin > 0 && opponent_elo > 0) {
        if (p.our_elo - opponent_elo >= p.elo_margin)
            return pos.side_to_move() == lado_raiz ? -p.contempt : p.contempt;
        return VALUE_DRAW;
    }
    // Com incremento nao ha' final a` bandeira: a repeticao e' resultado
    // legitimo e fugir-lhe so' piora a posicao.
    if (p.contempt_sem_inc && tem_incremento)
        return VALUE_DRAW;
    // Se ja' estamos a perder, um empate e' bom: nao se lhe foge.
    if (p.contempt_um_lado && nota_raiz_ant < 0)
        return VALUE_DRAW;
    return pos.side_to_move() == lado_raiz ? -p.contempt : p.contempt;
}

// O optimism, a partir da nota da raiz. Ver `otimismo_f` no `busca.h`.
//
// Positivo para o lado que joga na raiz, negativo para o outro -- quem acredita
// estar melhor e' um lado so'.
int Busca::otimismo_de(const Position& pos) const {
    if (p.otimismo_f == 0)
        return 0;
    int a = nota_raiz_ant;
    int o = p.otimismo_f * a / (std::abs(a) + 85);
    return pos.side_to_move() == lado_raiz ? o : -o;
}

int Busca::aspiracao(Position& pos, int prof, int anterior) {
    nota_raiz_ant = anterior;
    // Sem janela: cada iteracao corre de -INFINITO a +INFINITO. E' a medida do
    // que a aspiracao poupa -- e do que custa, porque cada falha da janela e'
    // uma re-busca inteira.
    if (DIAG.sem_asp)
        return negamax(pos, prof, -INFINITO, INFINITO, 0, true, false);
    // A largura arranca MAIOR nas profundidades rasas e aperta a` medida que
    // desce: `5 + 25*8/prof`. Uma largura fixa e' larga de mais em cima, onde a
    // nota ainda salta, e estreita de mais em baixo, onde ja' nao devia saltar.
    int delta = 5 + p.asp_delta * 8 / std::max(prof, 1);
    int alpha = -INFINITO, beta = INFINITO;
    // So' serve com o `asp_sf`. Ver a nota nos parametros.
    int falhas_altas = 0;
    if (prof > p.asp_prof && !e_mate(anterior)) {
        alpha = anterior - delta;
        beta  = anterior + delta;
    }
    for (;;) {
        delta_raiz = std::max(beta - alpha, 1);
        // Passada certa distancia deitava-se a janela fora. O tecto era 1000,
        // que na nossa escala sao CINCO PEOES -- e uma busca com beta infinito
        // nao pode falhar alto, portanto nunca colapsa.
        //
        // Medido: na posicao `2r3k1/pp2bppp/2n1pn2/3q4/...` (ganha por dez
        // peoes) a busca deles gasta 3.706 nos a` profundidade 12 e a nossa
        // 118.448 -- trinta e duas vezes. A arvore deles cresce x1,2 por
        // iteracao; a nossa x2,7 entre a profundidade 6 e a 8. Eles mantem a
        // janela apertada a` volta da nota anterior seja ela qual for.
        if (alpha < -p.asp_tecto)
            alpha = -INFINITO;
        if (beta > p.asp_tecto)
            beta = INFINITO;

        // Com o `asp_sf`, uma iteracao que ja' falhou alto varias vezes nao
        // merece a profundidade inteira: o lance ja' se mostrou, falta so'
        // confirma-lo.
        int prof_ajust = p.asp_sf ? std::max(prof - falhas_altas, 1) : prof;

        int nota = negamax(pos, prof_ajust, alpha, beta, 0, true, false);
        if (parado)
            return nota;

        if (nota <= alpha) {
            // Falhou em baixo: o beta desce com ele, senao a re-busca fica com
            // a janela deslocada em vez de alargada.
            beta  = p.asp_sf ? alpha : (alpha + beta) / 2;
            alpha = std::max(nota - delta, -INFINITO);
            falhas_altas = 0;
        } else if (nota >= beta) {
            beta = std::min(nota + delta, INFINITO);
            ++falhas_altas;
        } else {
            return nota;
        }
        delta += p.asp_sf ? delta / 3 : delta / 2;
    }
}

// Este lance repete a posicao ja' a seguir?
bool Busca::repete_ja(Position& pos, Move m) {
    StateInfo st;
    chaves.push_back(pos.key());
    pos.do_move(m, st, pos.gives_check(m), av->pilha().push(), nullptr, nullptr);
    bool r = repeticao(pos);
    pos.undo_move(m);
    av->pilha().pop();
    chaves.pop_back();
    return r;
}

// Cria ou destroi os ajudantes do Lazy SMP.
//
// Faz-se quando o `Threads` muda e nao a cada lance: cada ajudante carrega uma
// cache de acumuladores, e construi-la e' caro o bastante para se notar num
// controlo de tempo rapido.
//
// O que cada ajudante recebe do dono: a tabela de transposicao, as tres
// familias de historico, e a rede -- a MESMA rede, por ponteiro, porque sao 96
// MB que nao mudam durante a busca. O que ele tem de seu: a pilha de
// acumuladores, as caches, o historico de capturas, os killers e a tabela
// peca-casa.
void Busca::prepara_fios(int n, Avaliador& av_dono) {
    n = std::max(1, n);
    if (n == n_fios && int(ajudantes.size()) == n - 1)
        return;
    ajudantes.clear();
    av_ajudantes.clear();
    for (int i = 0; i < n - 1; ++i) {
        auto a = std::make_unique<Avaliador>();
        std::string erro;
        if (!a->partilha_rede(av_dono, erro)) {
            // Sem rede o ajudante nao avalia nada. Melhor menos fios do que
            // fios a procurar com uma avaliacao a zeros.
            saida() << "info string fio " << (i + 1) << " sem rede: " << erro << std::endl;
            break;
        }
        auto b = std::make_unique<Busca>();
        b->ajudante = true;
        b->p        = p;
        b->partilha_tabela(p_tab);
        b->partilha_historico(*this);
        av_ajudantes.push_back(std::move(a));
        ajudantes.push_back(std::move(b));
    }
    n_fios = int(ajudantes.size()) + 1;
}

void Busca::arranca(Position& pos, const Limites& lim, Avaliador& avaliador) {
    // Quem joga na raiz: o optimism e' assimetrico e precisa de saber o lado.
    lado_raiz      = pos.side_to_move();
    nota_raiz_ant  = 0;
    tem_incremento = lim.inc[0] > 0 || lim.inc[1] > 0;
    // Interruptores de diagnostico por ambiente. Nao sao opcoes UCI de
    // proposito: servem para partir a busca ao meio e ver que metade e' que
    // custa Elo, nao para alguem os ligar numa partida.
    if (const char* v = std::getenv("KS_SING"))
        p.sing_prof = std::atoi(v);
    // O ProbCut com busca. Rejeitei-o pela contagem de nos -- e a contagem de nos
    // ja' apontou ao contrario do resultado cinco vezes so' hoje. Ele incha a
    // arvore, sim; mas a referencia ESCOLHE gasta-los, e as buscas de
    // verificacao escrevem na tabela, o que melhora a ordenacao de tudo o que
    // vem depois. Isso pode comprar qualidade de escolha em vez de a gastar.
    // Decide-se em partidas.
    if (const char* v = std::getenv("KS_PC"))
        p.pc_prof = std::atoi(v);
    // A MARGEM, que nunca foi varrida. A nota do parametro diz "volta quando a
    // arvore estiver menor OU COM A MARGEM VARRIDA A SERIO", e so' a primeira
    // metade foi tentada. 205 da' 61% de acerto, que e' baixo: um ProbCut que
    // acerta pouco esta' a fazer uma busca rasa em toda a parte em vez de
    // distinguir posicoes.
    if (const char* v = std::getenv("KS_PC_M"))
        p.pc_margem = std::atoi(v);
    if (const char* v = std::getenv("KS_NMP_DIV"))
        p.nmp_prof_div = std::atoi(v);
    if (const char* v = std::getenv("KS_NMP_BASE"))
        p.nmp_base = std::atoi(v);
    if (const char* v = std::getenv("KS_NMP_PROF_DIV")) p.nmp_prof_div = std::atoi(v);
    if (const char* v = std::getenv("KS_NMP_DIVEST")) p.nmp_div_est = std::atoi(v);
    if (const char* v = std::getenv("KS_NMP_TECTO"))  p.nmp_div = std::atoi(v);
    if (const char* v = std::getenv("KS_REB_FUNDO"))
        p.reb_fundo = std::atoi(v);
    if (const char* v = std::getenv("KS_REB_RASO"))
        p.reb_raso = std::atoi(v);
    // A IIR: quando nao ha' lance na tabela, procura-se um ply mais raso para
    // arranjar um. Na radiografia, desliga-la ENCOLHE a nossa arvore 13%,
    // enquanto a` referencia desliga-la incha-lha 17%. Ou o limiar esta' errado
    // ou o mecanismo nao serve nesta arvore -- e isso decide-se em partidas.
    if (const char* v = std::getenv("KS_IIR_PROF")) p.iir_prof = std::atoi(v);
    if (const char* v = std::getenv("KS_IIR_SEM_ALL")) p.iir_sem_all = std::atoi(v);
    if (const char* v = std::getenv("KS_HP_PROF")) p.hpoda_prof = std::atoi(v);
    if (const char* v = std::getenv("KS_HP_M"))    p.hist_poda = std::atoi(v);
    if (const char* v = std::getenv("KS_HP_LIN"))  p.hpoda_lin = std::atoi(v);
    if (const char* v = std::getenv("KS_PIOR_ADV")) p.usa_pior_adv = std::atoi(v);
    // O PESO do termo, que nao tinha interruptor. Sem ele, varrer `rfp_adv_f`
    // exigia recompilar -- e a assinatura do ramo morto denunciou-o: 290 e 335
    // davam arvores IDENTICAS ao no', porque as duas corridas usavam 335.
    //
    // O valor importa: o nosso `CONSTANTES.md` regista `RfpAdv | 290 | varrido;
    // outro usa 335`, e o que esta' aqui por omissao e' o 335 DELES. Uma peca
    // nossa, ja' varrida na nossa arvore, a correr com a constante da
    // referencia.
    if (const char* v = std::getenv("KS_ADV_F")) p.rfp_adv_f = std::atoi(v);
    if (const char* v = std::getenv("KS_TT_CAPT"))  p.usa_tt_capt = std::atoi(v);
    if (const char* v = std::getenv("KS_CORR_MARG")) p.corr_marg_div = std::atoi(v);

    // O PADRAO SF, EM BLOCO.
    //
    // Nao e' para adoptar -- e' para MEDIR. Testar uma constante de cada vez da'
    // sempre negativo, porque as outras oitenta continuam a ser nossas e o
    // motor esta' afinado a` volta delas. Se as trocarmos todas de uma vez,
    // ficamos a saber se o problema e' o conjunto ou pecas soltas.
    //
    // A pergunta que isto responde: a referencia corre com os parametros DELA,
    // afinados para a rede DELA, e com a NOSSA rede fica em +73,95 no
    // campeonato -- 52 Elo acima do Triumviratus. Nos, com parametros
    // supostamente adaptados a` nossa rede, estamos em -11,03. Se os deles
    // funcionam com a nossa rede sem adaptacao nenhuma, entao o problema nao e'
    // transferencia: e' que os nossos sao piores.
    //
    // Os NOSSOS valores ficam na declaracao de `Parametros`, intactos. Isto e'
    // um interruptor, nao uma substituicao.
    if (std::getenv("KS_PADRAO_SF")) {
        // Futilidade inversa: `min(45 + 4*prof, 85) * prof`, tecto de
        // profundidade em 19, desconto por melhoria de 2789/1024 do
        // multiplicador (contra os nossos 150 fixos).
        p.rfp_margem = 45; p.rfp_mult = 4; p.rfp_tecto = 85; p.rfp_prof = 19;
        p.rfp_melhorando = 231;   // 2789/1024 * 85, no ponto de saturacao
        p.rfp_tecto_tot = 0;
        // Razoring: `eval < alpha - 482*prof*prof`. O nosso e' linear em prof,
        // portanto o valor equivalente ao segundo ply e' 482*2 = 964.
        p.razor_margem = 964; p.razor_prof = 5;
        // Lance nulo: `7 + prof/3 + max((eval-beta)/256, 0)`, sem tecto.
        p.nmp_base = 7; p.nmp_prof_div = 3; p.nmp_div_est = 256; p.nmp_div = 999;
        // LMP: `(3 + prof*prof) / (2 - melhorando)`.
        p.lmp_base = 3; p.lmp_prof = 12;
        // Poda pelo historico: `-4136 * prof`, linear e sem tecto.
        p.hist_poda = 4136; p.hpoda_lin = 1; p.hpoda_prof = 64;
        // Futilidade dos tranquilos: `estatica + 234 + 247*lmrDepth`, ate' 12.
        p.fut_base = 234; p.fut_decl = 247; p.fut_prof = 12;
        // A tabela da reducao: `2872/128 * log(i)` de cada lado, mais 982.
        // Em milesimos: base 982/1024 -> 96, divisor 1/(22,4375^2/1024) -> 204.
        p.lmr_base = 96; p.lmr_div = 204;
        p.lmr_piora_f = 197 * 1024 / 512;   // `!improving * escala * 197/512`
        p.iir_prof = 6; p.iir_sem_all = 1;
        p.low_f = 0;
    }
    if (const char* v = std::getenv("KS_LOW_F")) p.low_f = std::atoi(v);
    if (const char* v = std::getenv("KS_PCP_M")) p.pcp_margem = std::atoi(v);
    if (const char* v = std::getenv("KS_PCP_P")) p.pcp_prof = std::atoi(v);
    if (const char* v = std::getenv("KS_ASP_SF")) p.asp_sf = std::atoi(v);
    if (const char* v = std::getenv("KS_ASP_TECTO"))
        p.asp_tecto = std::atoi(v);
    // A LARGURA INICIAL da janela, que nunca teve interruptor.
    //
    // `delta = 5 + asp_delta*8/prof`: com 25, sao +-30 a` profundidade oito e
    // +-15 a` vinte. MEDIDO com o `KS_RAIZ`: 1,89 passagens por profundidade em
    // media nas quatro posicoes, e 2,56 na kiwipete -- cada passagem a mais
    // refaz a raiz inteira.
    if (const char* v = std::getenv("KS_ASP_DELTA"))
        p.asp_delta = std::atoi(v);
    if (const char* v = std::getenv("KS_ASP_PROF"))
        p.asp_prof = std::atoi(v);
    // A LARGURA INICIAL da janela, que nunca teve interruptor.
    //
    // `delta = 5 + asp_delta*8/prof`: com 25, sao +-30 a` profundidade oito e
    // +-15 a` vinte. MEDIDO com o `KS_RAIZ`: 1,89 passagens por profundidade em
    // media nas quatro posicoes, e 2,56 na kiwipete -- cada passagem a mais
    // refaz a raiz inteira.
    if (const char* v = std::getenv("KS_ASP_DELTA"))
        p.asp_delta = std::atoi(v);
    if (const char* v = std::getenv("KS_ASP_PROF"))
        p.asp_prof = std::atoi(v);
    // Quanto e' que a reducao pode ESTENDER, em milesimos de ply. 0 = so' reduz.
    if (const char* v = std::getenv("KS_EXT"))
        p.lmr_ext_max = std::atoi(v);
    if (const char* v = std::getenv("KS_TTPV"))
        p.lmr_ttpv = std::atoi(v);
    if (const char* v = std::getenv("KS_CUT"))
        p.lmr_cut_f = std::atoi(v);
    if (const char* v = std::getenv("KS_AMORT"))
        p.lmr_ext_amort = std::atoi(v);
    // Verificacao da MORDIDA: um mecanismo que nao muda a arvore nao esta'
    // ligado. Desliga-se cada um a` vez pondo o limite de profundidade dele
    // abaixo de tudo, e ve-se quanto a arvore muda.
    // Os tres que faltavam. Sem eles nao ha' comparacao possivel com a
    // referencia: sao justamente os que mais lhe poupam -- lance nulo 80% da
    // arvore, ProbCut 65%, futilidade 50% -- e nos so' sabiamos medir os
    // pequenos.
    if (std::getenv("KS_OFF_SING"))  p.sing_prof = 999;
    if (const char* v = std::getenv("KS_EXT2_EXACTO")) p.ext_dupla_exacto = std::atoi(v);
    if (const char* v = std::getenv("KS_EXT_NEG"))    p.ext_neg = std::atoi(v);
    if (const char* v = std::getenv("KS_ORDEM_CONT")) p.ordem_cont_f = std::atoi(v);
    if (const char* v = std::getenv("KS_ORDEM_CONT")) p.ordem_cont_f = std::atoi(v);
    if (const char* v = std::getenv("KS_CORR6"))      p.corr6_peso = std::atoi(v);
    // O LIMITE DE PROFUNDIDADE DA FUTILIDADE INVERSA. O nosso e' 9; o da
    // referencia e' 19, e ao lado dele esta' escrito "the depth condition is
    // important for mate finding, it should NOT be tuned". E o RFP e' o nosso
    // MAIOR redutor -- desliga-lo incha a arvore 105% -- portanto ele deixa de
    // actuar exactamente onde a nossa arvore comeca a explodir (profundidade 8).
    if (const char* v = std::getenv("KS_RFP_PROF")) p.rfp_prof = std::atoi(v);
    // A MARGEM. 110 por ply fixo, contra `min(45+4*prof, 85)` deles -- em
    // unidades de peao 0,55 por ply contra 0,41, portanto exigimos 34% mais
    // vantagem antes de podar. E a deles SATURA; a nossa cresce sem travao.
    if (const char* v = std::getenv("KS_RFP_M")) p.rfp_margem = std::atoi(v);
    if (const char* v = std::getenv("KS_RFP_MULT")) p.rfp_mult = std::atoi(v);
    if (const char* v = std::getenv("KS_RFP_TECTO")) p.rfp_tecto = std::atoi(v);
    if (const char* v = std::getenv("KS_RFP_TT")) p.rfp_tecto_tot = std::atoi(v);
    if (std::getenv("KS_OFF_RFP"))   p.rfp_prof = 0;
    if (std::getenv("KS_OFF_RAZOR")) p.razor_prof = 0;
    if (std::getenv("KS_OFF_LMP"))   p.lmp_prof = 0;
    if (std::getenv("KS_OFF_HPODA")) p.hist_poda = 1000000000;
    if (std::getenv("KS_OFF_FUT"))   p.fut_prof = 0;
    // ESTES DOIS ESTAVAM MAL NOMEADOS. `see_poda` e `see_poda_tranq` sao
    // MARGENS, nao interruptores: poda-se a captura se `!see_ge(m, -70*prof)`.
    // Po-las a zero nao desliga a poda -- torna-a MAXIMA, porque passa a podar
    // toda a captura com SEE negativo. Chamar-lhes KS_OFF dizia o contrario do
    // que faziam, e a radiografia saiu com dois sinais trocados.
    //
    // Ficam como valores, que e' o que sao. A zero a arvore encolhe 24 a 27% na
    // posicao de referencia, o que diz que as margens de hoje -- 70 e 5 -- estao
    // permissivas de mais.
    if (const char* v = std::getenv("KS_SEEQ")) p.see_poda_tranq = std::atoi(v);
    if (const char* v = std::getenv("KS_SEEC")) p.see_poda = std::atoi(v);
    if (const char* v = std::getenv("KS_CUCKOO")) p.usa_cuckoo = std::atoi(v);
    if (const char* v = std::getenv("KS_OTIMISMO")) p.otimismo_f = std::atoi(v);
    if (const char* v = std::getenv("KS_CONT_N")) p.cont_n = std::clamp(std::atoi(v), 1, 5);
    if (const char* v = std::getenv("KS_PEAO_F")) p.peao_f = std::atoi(v);
    if (const char* v = std::getenv("KS_PEAO_CH")) p.peao_chaves = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_ESF_BASE")) p.tm_esf_base = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_ESF_F")) p.tm_esf_f = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_QUEDA")) p.tm_queda_max = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_INSTAB")) p.tm_instab_div = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_INSTAB_MAX")) p.tm_instab_max = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_ESCALA_MAX")) p.tm_escala_max = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_CHAO")) p.tm_chao_ms = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_ESCALA_MIN")) p.tm_escala_min = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_TECTO")) p.tm_tecto_x10 = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_CURVA")) p.tm_curva = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_CURVA_MIN")) p.tm_curva_min = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_ADV_F")) p.tm_adv_f = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_ADV_MIN")) p.tm_adv_min = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_ADV_MAX")) p.tm_adv_max = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_CURVA_F")) p.tm_curva_f = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_CURVA_PCT")) p.tm_curva_pct = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_CRESCE")) p.tm_cresce = std::atoi(v);
    if (const char* v = std::getenv("KS_TM_SEM_INC_TECTO")) p.tm_sem_inc_tecto = std::atoi(v);
    if (const char* v = std::getenv("KS_ORDEM_XEQUE")) p.ordem_xeque_f = std::atoi(v);
    if (const char* v = std::getenv("KS_ORDEM_XEQUE_SEE")) p.ordem_xeque_see = std::atoi(v);
    if (const char* v = std::getenv("KS_SEM_BALDE")) p.sem_balde = std::atoi(v);
    if (const char* v = std::getenv("KS_AMEACA")) p.ameaca_f = std::atoi(v);
    if (const char* v = std::getenv("KS_KILLERS")) p.usa_killers = std::atoi(v);
    if (const char* v = std::getenv("KS_SING_LOWER")) p.sing_so_inferior = std::atoi(v);
    if (const char* v = std::getenv("KS_QS_RECAPT")) p.qs_recaptura = std::atoi(v);
    if (const char* v = std::getenv("KS_TT_SUP")) p.tt_sup_nota = std::atoi(v);
    if (const char* v = std::getenv("KS_CUTCNT")) p.cut_cnt_base = std::atoi(v);
    if (const char* v = std::getenv("KS_CUTCNT_MAIS")) p.cut_cnt_mais = std::atoi(v);
    if (const char* v = std::getenv("KS_CONTEMPT")) p.contempt = std::atoi(v);
    if (const char* v = std::getenv("KS_CONT_SEMINC")) p.contempt_sem_inc = std::atoi(v);
    if (const char* v = std::getenv("KS_CUCKOO_SEMINC")) p.cuckoo_sem_inc = std::atoi(v);
    if (const char* v = std::getenv("KS_RECUSA_FRESCA")) p.recusa_fresca = std::atoi(v);
    if (const char* v = std::getenv("KS_CONT_1LADO")) p.contempt_um_lado = std::atoi(v);
    if (const char* v = std::getenv("KS_OUR_ELO")) p.our_elo = std::atoi(v);
    if (const char* v = std::getenv("KS_ELO_MARGIN")) p.elo_margin = std::atoi(v);
    // AS MANETES QUE FALTAVAM. Os campos ja' existiam -- o que faltava era
    // poder mexer-lhes de fora para os medir um a um. Os valores por omissao
    // sao os do binario que jogava, lidos do molde em .rodata.
    if (const char* v = std::getenv("KS_ALPHA_DESC")) p.alpha_desc = std::atoi(v);
    if (const char* v = std::getenv("KS_AD_MIN")) p.ad_min = std::atoi(v);
    if (const char* v = std::getenv("KS_AD_MAX")) p.ad_max = std::atoi(v);
    if (const char* v = std::getenv("KS_CUT_SEM_TT")) p.lmr_cut_sem_tt = std::atoi(v);
    if (const char* v = std::getenv("KS_EXT_DUPLA")) p.ext_dupla = std::atoi(v);
    if (const char* v = std::getenv("KS_LMP_PROF")) p.lmp_prof = std::atoi(v);
    if (const char* v = std::getenv("KS_LMP_MELHORA")) p.lmp_melhora = std::atoi(v);
    if (const char* v = std::getenv("KS_LMR_BASE")) p.lmr_base = std::atoi(v);
    if (const char* v = std::getenv("KS_LMR_DIV")) p.lmr_div = std::atoi(v);
    if (const char* v = std::getenv("KS_LMR_DELTA")) p.lmr_delta = std::atoi(v);
    if (const char* v = std::getenv("KS_LANCE_ALPHA")) p.lance_so_alpha = std::atoi(v);
    if (const char* v = std::getenv("KS_QS_GUARDA_AVAL")) p.qs_guarda_aval = std::atoi(v);
    if (const char* v = std::getenv("KS_IND_RAPIDO")) p.indices_rapido = std::atoi(v);
    if (const char* v = std::getenv("KS_PREFETCH")) p.prefetch_antes = std::atoi(v);
    if (const char* v = std::getenv("KS_HIST_PREFETCH")) p.hist_prefetch = std::atoi(v);
    if (const char* v = std::getenv("KS_CORR_PREFETCH")) p.corr_prefetch = std::atoi(v);
    if (const char* v = std::getenv("KS_PODA_RED")) p.poda_red = std::atoi(v);
    if (const char* v = std::getenv("KS_LMR_EXT_MAX")) p.lmr_ext_max = std::atoi(v);
    if (const char* v = std::getenv("KS_LMR_PECAS_FIM")) p.lmr_pecas_fim = std::atoi(v);
    if (const char* v = std::getenv("KS_PCP_MARGEM")) p.pcp_margem = std::atoi(v);
    if (const char* v = std::getenv("KS_SING_MARGEM")) p.sing_margem = std::atoi(v);
    if (const char* v = std::getenv("KS_TTPV_ALPHA")) p.lmr_ttpv_alpha = std::atoi(v);
    if (const char* v = std::getenv("KS_TTPV_FUNDO")) p.lmr_ttpv_fundo = std::atoi(v);
    if (const char* v = std::getenv("KS_TTPV_PV")) p.lmr_ttpv_pv = std::atoi(v);
    av     = &avaliador;
    nos    = 0;
    tb_acertos = 0;
    sel_prof = 0;
    conta_sing = nos_sing = conta_ext1 = conta_ext2 = conta_ext_neg = 0;
    pc_nos = pc_tentativas = pc_passou_qs = pc_cortes = n_aval = 0;
    n_aval_bs = n_hit_bs = n_aval_qs = n_hit_qs = 0;
    parado = false;
    // O `parar` NAO se repoe aqui. Quem arranca a busca repoe-no ANTES de ela
    // nascer: a thread dos comandos para a principal, o proprio `arranca` para
    // cada ajudante (mais abaixo). Reposto aqui, um `stop` que chegasse entre o
    // nascimento da thread e esta linha perdia-se -- e com `go infinite` a busca
    // ficava a correr para sempre.
    inicio = std::chrono::steady_clock::now();
    melhor_raiz = Move::none();
    nota_raiz   = 0;
    ultima_prof = 0;
    notas_raiz.clear();
    std::memset(pv_n, 0, sizeof(pv_n));
    std::memset(nulo_em, 0, sizeof(nulo_em));
    for (int i = 0; i < MAX_PLY; ++i) {
        aval_ply[i]  = TT_SEM_AVAL;
        jogado_pc[i] = -1;
    }
    chaves = chaves_jogo;
    // A pilha dos acumuladores comeca do zero a cada busca: o que la' estava era
    // de outra arvore.
    av->pilha().reset();
    if (capt_hist.empty())
        limpa();

    // A tabela de reducao, em milesimos de ply.
    {
        double base = p.lmr_base / 100.0;
        double div  = std::max(p.lmr_div / 100.0, 0.01);
        for (int d = 1; d < 64; ++d)
            for (int m = 1; m < 64; ++m)
                lmr[d][m] = int((base + std::log(double(d)) * std::log(double(m)) / div) * 1024.0);
    }

    // --- o orcamento ---
    //
    // O incremento entra no BOLO -- `relogio + inc*(n-1)` -- em vez de ser
    // somado uma vez a` parte. Somar um incremento so' conta UM: a 120+1 orca
    // 5,4 s e recebe 1 s, e a diferenca sai do capital ate' a bandeira cair.
    //
    // O tecto e' `min(80% do relogio, 2x o optimo)`. Era `optimo * 2` cravado, e
    // era ELE que travava tudo. O travao dos 80% fica, e e' esse que impede a
    // bandeira.
    const std::int64_t sobrecarga = std::max<std::int64_t>(sobrecarga_ms, 0);
    nos_limite = lim.nos > 0 ? std::uint64_t(lim.nos) : 0;
    if (lim.infinito || lim.profundidade > 0 || lim.nos > 0) {
        mole = duro = std::chrono::milliseconds(86400000);
    } else if (lim.movetime > 0) {
        auto usavel = std::max<std::int64_t>(lim.movetime - sobrecarga, 1);
        mole = duro = std::chrono::milliseconds(usavel);
    } else {
        int          lado = int(pos.side_to_move());
        std::int64_t t    = lim.tempo[lado];
        std::int64_t inc  = lim.inc[lado];
        // OS LANCES ATE' AO CONTROLO.
        //
        // O `movestogo` nao era lido. Na 40/15 da CCRL o relogio volta a encher
        // ao lance 40, e o motor orcava cada periodo como se o relogio tivesse
        // de chegar ao fim da partida: ao lance 30, com 4 minutos e 10 lances
        // ate' ao controlo, pensava ~6,5 s por lance em vez de ~20 s, e o que
        // sobrava ia fora no controlo. Nenhum orcamento aqui passa do controlo.
        //
        // Dois lances de folga: o ultimo antes do controlo nao leva o relogio
        // inteiro. Com `movestogo 1` orca-se um terco do que resta, e o tecto
        // dos 80% guarda o resto.
        const std::int64_t ate_controlo = lim.movestogo > 0 ? std::int64_t(lim.movestogo) + 2 : 0;
        std::int64_t n    = std::max(p.tm_bolo_n, 1);
        if (ate_controlo > 0) n = std::min(n, ate_controlo);
        std::int64_t bolo = t + inc * (n - 1);
        std::int64_t optimo = std::max(bolo / n - sobrecarga, std::int64_t(1));
        // QUANTOS LANCES FALTAM MESMO. Ver `tm_curva` no `busca.h`.
        //
        // O `bolo_n` plano diz "faltam vinte", sempre. Esta tabela diz o que as
        // NOSSAS 2224 partidas dizem, e nao e' vinte quase nunca. Interpolada
        // entre pontos.
        if (p.tm_curva) {
            static const int PLY[]   = {0, 20, 40, 60, 80, 100, 120, 160, 220};
            // Cinco percentis da MESMA medida. Ver `tm_curva_pct` no `busca.h`:
            // orcar pela mediana e' orcar para METADE das partidas rebentarem o
            // orcamento, e sao essas que acabam a zero.
            // [BINARIO] A PRIMEIRA LINHA e' a do `ks_1.20260919`, lida do
            // simbolo `Busca::arranca(...)::FALTA` em 0x5ec520.  O simbolo tem
            // tamanho declarado 0x24 = 36 bytes = NOVE inteiros: uma linha so',
            // nao cinco.  Os valores:
            //
            //     [131, 111, 91, 74, 59, 47, 38, 24, 16]
            //
            // Os sete primeiros batiam ja'.  Os dois ultimos nao: estavam 31 e
            // 28, contra os 24 e 16 do original.
            //
            // Ao ply 220 isso da', com 10 s no relogio, 714 ms por lance em vez
            // dos 1250 ms que o original gastava -- 57% do tempo, na fase da
            // partida onde o calculo profundo mais decide.  E repare-se que com
            // `faltam = 16` o `faltam/2 = 8` toca EXACTAMENTE o `tm_curva_min`:
            // o original foi afinado para assentar no chao ao ply 220, e com 28
            // nunca la' chegava.
            //
            // A escada dos plies esta' no binario como comparacoes e nao como
            // tabela (446355: cmp $0x4f / $0x63 / $0x77 / $0x9f / $0xdb, que sao
            // 80-1, 100-1, 120-1, 160-1, 220-1) -- por isso uma procura da
            // tabela por bytes nao a encontra.
            //
            // As outras quatro linhas sao NOSSAS, posteriores ao binario, e
            // ficam como estao.
            static const int FALTA_P[5][9] = {
              {131, 111,  91,  74,  59,  47,  38,  24,  16},   // p50 — do binario
              {143, 123, 103,  86,  70,  57,  47,  41,  41},   // p60
              {156, 136, 117,  99,  83,  70,  61,  55,  54},   // p70
              {164, 144, 125, 107,  91,  79,  69,  63,  61},   // p75
              {174, 154, 134, 118, 102,  88,  79,  73,  75},   // p80
            };
            const int* FALTA = FALTA_P[std::clamp(p.tm_curva_pct, 0, 4)];
            const int N = int(sizeof(PLY) / sizeof(PLY[0]));
            int jogados = int(chaves_jogo.size());
            int faltam  = FALTA[N - 1];
            // DEPOIS DO FIM DA TABELA, A ESTIMATIVA JA' FOI DESMENTIDA.
            //
            // A tabela acaba no ply 220. Dai' para a frente dizia sempre o
            // mesmo numero, fizesse a partida 230 plies ou 400 -- deixava de
            // ser estimativa e voltava a ser constante, que e' o defeito que
            // ela veio corrigir.
            //
            // Uma partida que passou o ultimo degrau PROVOU que e' das longas.
            // A clikKov5 (600+0) durou 254 plies contra uma mediana de 131 e
            // acabou com 1,0s. 100 = um por um.
            if (jogados >= PLY[N - 1] && p.tm_cresce > 0)
                faltam += (jogados - PLY[N - 1]) * p.tm_cresce / 100;
            for (int k = 0; k + 1 < N; ++k)
                if (jogados < PLY[k + 1]) {
                    int d2 = PLY[k + 1] - PLY[k];
                    faltam = FALTA[k] + (FALTA[k + 1] - FALTA[k]) * (jogados - PLY[k]) / d2;
                    break;
                }
            // De plies para lances NOSSOS, e nunca menos do que um punhado:
            // perto do fim a mediana desce mas o relogio tambem, e dividir por
            // dois lances e' pedir para cair.
            std::int64_t lc = std::max<std::int64_t>(faltam / 2, p.tm_curva_min);
            lc = std::max<std::int64_t>(lc * p.tm_curva_f / 100, 1);
            if (ate_controlo > 0) lc = std::min(lc, ate_controlo);
            std::int64_t bolo2 = t + inc * (lc - 1);
            optimo = std::max(bolo2 / lc - sobrecarga, std::int64_t(1));
        }

        // Quem tem mais relogio do que o adversario pode gastar mais, e quem
        // tem menos tem de poupar. Ver `tm_adv_f` no `busca.h`; a zero por
        // omissao. Entra DEPOIS da curva, porque no binario os dois ramos do
        // `tm_curva` juntam-se antes disto.
        if (p.tm_adv_f != 0) {
            std::int64_t tadv = lim.tempo[1 - lado];
            if (tadv > 0) {
                double razao = double(t) / double(tadv);
                double esc   = 1.0 + (razao - 1.0) * (double(p.tm_adv_f) / 100.0);
                esc = std::min(std::max(esc, double(p.tm_adv_min) / 100.0),
                               double(p.tm_adv_max) / 100.0);
                optimo = std::int64_t(double(optimo) * esc);
            }
        }

        std::int64_t tecto  = std::max(
          std::min(8 * t / 10, optimo * p.tm_tecto_x10 / 10) - sobrecarga, std::int64_t(1));

        // O TRAVAO DE VERDADE.
        //
        // Perdeu-se um jogo a` bandeira num final de torre por torre, ao lance
        // 127, com o adversario ainda com 7,9 s. A conta de cima explica-o
        // sozinha: o `optimo` orca por um BOLO que inclui incrementos que ainda
        // nao foram recebidos, e o `duro` era `max(tecto, optimo)` -- o maximo,
        // nao o minimo. O `tecto` traz os 80% do relogio; o `max` deitava-os
        // fora sempre que o `optimo` fosse maior.
        //
        // E e' maior sempre que `19*inc > 15*t`, ou seja assim que o relogio
        // desce abaixo de cerca de 1,27 vezes o incremento -- o sitio onde toda
        // a partida longa acaba. A 120+1, com 800 ms no relogio e 1 s de
        // incremento: bolo 19.800, optimo 960 ms. Pensava 960 ms com 800 ms.
        //
        // O incremento so' entra no relogio DEPOIS de o lance ser jogado, por
        // isso o que ha' mesmo para gastar e' `t`, e nem tudo: fica de fora a
        // sobrecarga e mais um vigesimo para a viagem ate' ao servidor.
        // O TECTO SEM INCREMENTO. Ver `tm_sem_inc_tecto` no `busca.h`.
        //
        // So' quando nao ha' incremento: com incremento o gasto e' reposto.
        // E com controlo tambem -- o relogio volta a encher.
        // Corta o TECTO, nao o orcamento.
        if (p.tm_sem_inc_tecto > 0 && inc == 0 && ate_controlo == 0)
            tecto = std::max(std::min(tecto, t * p.tm_sem_inc_tecto / 100), std::int64_t(1));

        std::int64_t seguro = std::max(t - sobrecarga - t / 20, std::int64_t(1));
        // O chao, limitado pelo que resta -- ver `tm_chao_ms`.
        std::int64_t chao = std::min<std::int64_t>(std::max(p.tm_chao_ms, 0), seguro);
        mole = std::chrono::milliseconds(std::max(std::min(optimo, seguro), chao));
        duro = std::chrono::milliseconds(
          std::max(std::min(std::max(tecto, optimo), seguro), chao));
    }

    p_tab->avanca_geracao();

    // Um so' lance legal nao precisa de busca nenhuma: procura-lo e' gastar
    // relogio para chegar a` unica resposta possivel. Medido: sem isto, uma
    // posicao com lance forcado consumiu 639.881 nos para dizer o que ja' se
    // sabia.
    if (!lim.infinito && lim.profundidade == 0) {
        MoveList<LEGAL> ml(pos);
        if (ml.size() == 1) {
            melhor_raiz = *ml.begin();
            saida() << "info depth 1 seldepth 1 score cp 0 nodes 1 nps 0 time 0 pv "
                      << UCIEngine::move(melhor_raiz, false) << std::endl;
            saida() << "bestmove " << UCIEngine::move(melhor_raiz, false) << std::endl;
            return;
        }
    }

    melhor_anterior = Move::none();
    iters_sem_mudar = 0;
    mudancas        = 0;
    nota_media      = 0;
    mole_base       = mole;

    int  anterior = 0;
    auto t0       = std::chrono::steady_clock::now();
    std::int64_t custo_anterior = 0;
    int prof_max = lim.profundidade > 0 ? lim.profundidade : MAX_PLY - 2;


    // --- LAZY SMP: os ajudantes ---
    //
    // Nao procuram noutro sitio: procuram a MESMA posicao, e chegam a`s coisas
    // por outra ordem porque a tabela de transposicao que partilham lhes vai
    // respondendo coisas diferentes. Quem enche a tabela mais depressa faz a
    // principal encontrar la' trabalho ja' feito, e ela desce mais.
    //
    // O que mudou aqui em relacao a` primeira versao e' a partilha do
    // HISTORICO. So' com a tabela, a quatro fios fazia-se 3,17x os nos de um
    // fio e chegava-se ao ply 21 onde uma referencia chegava ao 25 -- os nos
    // estavam la', o que nao circulava era a aprendizagem.
    //
    // A posicao vai por FEN. Um `Position` nao se copia: ele aponta para uma
    // cadeia de `StateInfo` que e' da arvore de quem o construiu. O historico
    // de repeticoes vai a` parte, em `chaves_jogo`, que e' o que a FEN nao leva.
    if (!ajudante && !ajudantes.empty()) {
        const std::string fen = pos.fen();
        Limites lim_aj  = lim;
        lim_aj.infinito = true;   // quem manda parar e' a principal
        lim_aj.nos      = 0;
        lim_aj.movetime = 0;
        lim_aj.profundidade = 0;
        fios.clear();
        for (std::size_t i = 0; i < ajudantes.size(); ++i) {
            Busca&     b = *ajudantes[i];
            Avaliador& a = *av_ajudantes[i];
            b.p           = p;          // os mesmos parametros
            b.chaves_jogo = chaves_jogo;
            b.pre_n       = pre_n;
            std::memcpy(b.pre_pc, pre_pc, sizeof(pre_pc));
            std::memcpy(b.pre_para, pre_para, sizeof(pre_para));
            b.parar.store(false, std::memory_order_relaxed);
            fios.emplace_back([&b, &a, fen, lim_aj]() {
                StateInfo st;
                Position  pa;
                pa.set(fen, false, &st);
                b.arranca(pa, lim_aj, a);
            });
        }
    }

    // A LINHA QUE O ARBITRO LE'. Extraida para lambda porque ha' DOIS
    // sitios que precisam dela: o fim de uma iteracao completa, e o corte a
    // meio de uma iteracao que ja' tinha provado um lance melhor.
    auto anuncia = [&](int prof_a, int nota_a, std::int64_t passou_a) {
            // A nota vai para fora a DIVIDIR POR DOIS, como o half2k.
            //
            // A escala interna dos dois motores e' a mesma -- as estaticas batem ao
            // ponto -- mas o half2k faz `score/2` ao imprimir e eu mostrava o valor
            // cru. Nao mudava a busca nada, mas mudava o TESTE: o arbitro decide o
            // abandono a 600 e o empate a 10 pelo que o motor ANUNCIA, e os dois
            // estavam a ser julgados por reguas diferentes.
            //
            // E os mates passam a ser anunciados como mates. Sem isto um mate ia
            // como um `cp` enorme, e nem a interface nem o arbitro o viam.
            saida() << "info depth " << prof_a << " seldepth " << sel_prof << " score ";
            if (std::abs(nota_a) >= VALUE_MATE_IN_MAX_PLY) {
                int plies = VALUE_MATE - std::abs(nota_a);
                int mv    = (plies + 1) / 2;
                saida() << "mate " << (nota_a > 0 ? mv : -mv);
            } else {
                saida() << "cp " << nota_a / 2;
            }
            // WDL: vitoria, empate e derrota em milesimos, do ponto de vista de quem
            // joga.
            //
            // As duas constantes NAO sao uma escolha nem um valor por omissao: sao o
            // `in-offset` e o `in-scaling` com que a REDE foi treinada. Por isso e'
            // que se aplicam directamente a` nossa nota, sem conversao nenhuma -- a
            // quantizacao guarda cada peso ja' multiplicado pela escala do treino.
            //
            // E por isso tambem e' que o modelo da referencia ter mudado nao nos
            // afecta: este nao vem de la', vem do treino desta rede. Mudar um destes
            // numeros sem mudar o treino faz o motor anunciar probabilidades que a
            // rede nunca aprendeu a produzir.
            {
                // POR FAZER, e e' trabalho nosso e nao transcricao: estas duas
            // constantes sao FIXAS, e a referencia faz o mesmo modelo com um
            // `a` e um `b` que dependem do MATERIAL, por um polinomio em
            // (peoes + 3*menores + 5*torres + 9*damas).
            //
            // A razao e' boa: a mesma avaliacao nao vale a mesma probabilidade
            // com trinta e duas pecas e com seis. Uma vantagem de meio peao num
            // final e' quase decisiva; na abertura nao e' nada.
            //
            // Isso NAO vem do treino -- e' um ajuste feito sobre partidas
            // reais. Podemos fazer o nosso: temos milhares de PGN com resultado
            // e avaliacao lance a lance. Ajustar sobre os NOSSOS dados, nao
            // copiar o polinomio deles, que e' da avaliacao deles.
            //
            // Nao muda a busca; muda o que o motor ANUNCIA -- e o arbitro decide
            // o abandono e o empate pelo que se anuncia.
            auto sig = [](double x) { return 1.0 / (1.0 + std::exp(-x)); };
                double cp = double(nota_a);
                int w = int(std::lround(1000.0 * sig((cp - 285.2706341467852) / 295.6539508488627)));
                int l = int(std::lround(1000.0 * sig((-cp - 285.2706341467852) / 295.6539508488627)));
                saida() << " wdl " << w << " " << (1000 - w - l) << " " << l;
            }
            saida() << (tb_acertos ? " tbhits " + std::to_string(tb_acertos) : std::string())
                      << " hashfull " << p_tab->cheia() << " nodes " << nos_totais()
                      << " time " << passou_a << " nps "
                      << (passou_a > 0 ? nos_totais() * 1000 / std::uint64_t(passou_a) : 0) << " pv";
            for (int j = 0; j < pv_n[0]; ++j)
                saida() << " " << UCIEngine::move(pv_tab[0][j], false);
            saida() << std::endl;
    };

    for (int prof = 1; prof <= prof_max; ++prof) {
        auto antes_iter = std::chrono::steady_clock::now();
        std::uint64_t nos_antes = nos;
            // O esforco compara-se DENTRO da iteracao: marca-se onde ela comeca
        // a escrever em `notas_raiz` e quantos nos ja' levava.
        raiz_marca   = notas_raiz.size();
        nos_iter_ini = nos;
        std::memset(nos_por_lance, 0, sizeof(nos_por_lance));
        int nota = aspiracao(pos, prof, anterior);
        // Uma iteracao cortada a meio nao tem lance em que se confie: o que ela
        // tem e' o primeiro da lista, que ainda nao foi comparado com nada.
        if (parado && prof > 1) {
            // MAS pode ter provado um lance melhor antes de ser cortada. So' se
            // escreve em `melhor_raiz` com o alpha subido, ou seja com o lance
            // ja' pesquisado ate' ao fim e ja' melhor do que tudo o que veio
            // antes -- e esse joga-se. O que faltava era CONTA-LO: a iteracao
            // saia por aqui sem imprimir, o arbitro ficava com a variante da
            // iteracao anterior, e o `bestmove` nao batia certo com ela.
            //
            // Nao e' cosmetica: a interface mostra uma linha que ja' nao e' a
            // nossa, e um arbitro que verifique a coerencia avisa -- ou, pior,
            // acredita na linha velha.
            if (melhor_raiz != Move::none() && melhor_raiz != melhor_anterior) {
                // A VARIANTE PODE ESTAR VAZIA e o lance continuar bom.
                //
                // A aspiracao re-pesquisa quando falha a janela, e cada
                // re-busca limpa `pv_n[0]` ao entrar. Cortada A MEIO de uma
                // re-busca, fica-se com `melhor_raiz` de uma passagem anterior
                // -- que subiu o alpha e portanto vale -- e sem linha nenhuma
                // para o acompanhar. Anunciar assim dava `... pv` e mais nada,
                // que e' pior do que nao anunciar: o arbitro le uma variante
                // vazia em vez de uma desactualizada.
                if (pv_n[0] == 0) {
                    pv_tab[0][0] = melhor_raiz;
                    pv_n[0]      = 1;
                }
                auto ate_agora = std::chrono::duration_cast<std::chrono::milliseconds>(
                                   std::chrono::steady_clock::now() - t0).count();
                anuncia(prof, nota_raiz, ate_agora);
            }
            break;
        }
        ultima_prof = prof;
        anterior = nota;

        auto agora = std::chrono::steady_clock::now();
        auto passou = std::chrono::duration_cast<std::chrono::milliseconds>(agora - t0).count();
        custo_anterior =
          std::chrono::duration_cast<std::chrono::milliseconds>(agora - antes_iter).count();
        (void) nos_antes;

        anuncia(prof, nota, passou);

        // --- o elastico ---
        //
        // Tres sinais, todos da propria busca:
        //   `queda`    a nota caiu em relacao a` media -> gastar mais
        //   `estab`    ha' iteracoes que o melhor lance nao muda -> gastar menos
        //   `instab`   o melhor lance ja' mudou n vezes -> gastar mais
        //
        // A media e' movel com peso 9/10, portanto uma queda subita conta e uma
        // oscilacao pequena nao.
        if (prof == 1) {
            nota_media = nota;
        } else {
            nota_media = (nota + 9 * nota_media) / 10;
        }
        if (melhor_raiz != melhor_anterior) {
            melhor_anterior = melhor_raiz;
            iters_sem_mudar = 0;
            ++mudancas;
        } else {
            ++iters_sem_mudar;
        }
        {
            double d      = std::max(prof, 1);
            double queda  = std::clamp(1.0 + std::max(nota_media - nota, 0) / 100.0,
                                       1.0, p.tm_queda_max / 100.0);
            double estab  = std::clamp(1.0 - iters_sem_mudar / (2.0 * d), 0.75, 1.0);
            double instab = std::clamp(0.9 + mudancas / (p.tm_instab_div / 10.0 * d),
                                       1.0, p.tm_instab_max / 100.0);
            // O QUARTO FACTOR: o ESFORCO, que fraccao desta iteracao foi para
            // o melhor lance. Posicao facil -- quase tudo num lance so' -- joga
            // mais depressa; posicao dificil -- nos espalhados -- pensa mais.
            //
            // Confere com o `ks_1.20260919` (`arranca`, `440c92`-`440cf4`):
            //
            //     440cb6:  vdivsd %xmm10,%xmm13,%xmm11  ; nm / nos_iter
            //     440c96:  vcvtsi2sdl 0x200(%r12)       ; tm_esf_base
            //     440cd5:  vsubsd %xmm11,%xmm5,%xmm12   ; base/100 - frac
            //     440ccd:  vdivsd 100.0                 ; tm_esf_f/100
            //     440cda:  vmulsd %xmm3,%xmm12,%xmm0
            //     440ce2 / 440cf4:  grampo em [0.5, 2.0]  (608f98 / 608fa0)
            double esf = 1.0;
            if (p.tm_esf_f > 0) {
                std::uint64_t nos_iter = nos > nos_iter_ini ? nos - nos_iter_ini : 0;
                if (nos_iter > 0) {
                    // O indice do melhor lance NESTA iteracao: o `notas_raiz`
                    // guarda-os por ordem a partir do `raiz_marca`, e o
                    // `nos_por_lance` usa a mesma ordem.
                    std::uint64_t nm = 0;
                    for (std::size_t k = raiz_marca; k < notas_raiz.size(); ++k) {
                        if (notas_raiz[k].first == melhor_raiz) {
                            std::size_t idx = k - raiz_marca;
                            if (idx < MAX_LANCES) nm = nos_por_lance[idx];
                            break;
                        }
                    }
                    double frac = double(nm) / double(nos_iter);
                    esf = std::clamp((p.tm_esf_base / 100.0 - frac) * (p.tm_esf_f / 100.0),
                                     0.5, 2.0);
                }
            }
            double escala = std::clamp(queda * estab * instab * esf,
                                       p.tm_escala_min / 1000.0, p.tm_escala_max / 1000.0);
            auto novo = std::chrono::milliseconds(
              std::int64_t(mole_base.count() * escala));
            mole = std::min(novo, duro);
        }

        if (lim.infinito)
            continue;
        // A iteracao seguinte nem sempre custa o dobro, e mesmo quando custa nao
        // ha' mal em comeca-la: o `duro` esta' la' para a cortar. Parar sem
        // sequer tentar e' devolver relogio de graca.
        //
        // EM PROVA a 120+1. O valor 7 (0,7x) foi medido a 10+0,1 e ganhou
        // +17,5 +/- 15,9; a 120+1 o sinal que temos e' NEGATIVO com poucas
        // partidas, e e' esse o relogio a que o bot joga. Ate' esse teste
        // fechar, isto e' um numero em prova e nao uma decisao.
        std::int64_t estim = custo_anterior * p.tm_estim / 10;
        if (passou + estim >= mole.count())
            break;
    }

    // `go infinite`: o `bestmove` so' sai depois do `stop` (ou do `quit`), mesmo
    // que a busca tenha acabado sozinha -- chegou a` profundidade maxima, ou a
    // posicao e' trivial. O protocolo e' explicito: em modo infinito o motor
    // nao manda `bestmove` antes de lho pedirem. Os ajudantes correm SEMPRE em
    // modo infinito, de proposito, e quem os para e' a principal; nao esperam.
    if (lim.infinito && !ajudante)
        while (!parar.load(std::memory_order_relaxed))
            std::this_thread::sleep_for(std::chrono::milliseconds(1));

    // Os ajudantes param quando a principal parou. Cada um le' a SUA bandeira,
    // e e' esta linha que as levanta todas -- sem ela ficavam a procurar para
    // sempre, porque correm com limites infinitos de proposito.
    if (!fios.empty()) {
        for (auto& b : ajudantes)
            b->parar.store(true, std::memory_order_relaxed);
        for (auto& f : fios)
            if (f.joinable())
                f.join();
        fios.clear();

        // A VOTACAO.
        //
        // Sem ela os ajudantes so' valiam pelo que deixaram na tabela e no
        // historico, e as respostas deles -- tres buscas inteiras -- eram
        // deitadas fora no fim.
        //
        // [BINARIO] Quem vota: o principal, e cada ajudante que tenha lance e
        // que tenha COMPLETADO PELO MENOS UMA ITERACAO.  Nada mais.
        //
        // O `ks_1.20260919` testa, em 4466cf:
        //
        //     mov  0x2fc(%rax),%esi    ; Busca+0x274 = ultima profundidade
        //     test %esi,%esi
        //     jg   449b6c              ; > 0  ->  vota
        //
        // e NAO compara com a profundidade do principal em lado nenhum --
        // procurei `cmp` sobre 0x274 e sobre 0x2fc em todo o binario e nao
        // existe.
        //
        // O que estava aqui antes era `b->ultima_prof + 2 >= ultima_prof`, com
        // o argumento de que um fio atrasado decide com menos informacao.  O
        // argumento soa bem e e' uma DEDUCAO nossa: o motor perdido nao a
        // fazia.
        //
        // E ha' razao para nao a fazer.  O piso de 24 da `vota()` existe
        // precisamente para que um fio atrasado CONTINUE A CONTAR: se a nota
        // dele for muito inferior, o peso `nota - menor + 24` da-lhe quase so'
        // o piso, e ele entra como um voto de CONTAGEM e nao de forca.  E' a
        // contagem que da' forca ao metodo -- o lance que varios caminhos
        // independentes encontraram vale mais -- e o filtro dos dois plies
        // deitava fora essa amostra por inteiro.  Com tres ajudantes, um
        // atrasado fazia a votacao passar de quatro vozes a tres.
        std::vector<std::pair<Move, int>> cand;
        if (melhor_raiz != Move::none() && nota_raiz > -VALUE_MATE)
            cand.emplace_back(melhor_raiz, nota_raiz);
        for (auto& b : ajudantes)
            if (b->melhor_raiz != Move::none() && b->ultima_prof > 0)
                cand.emplace_back(b->melhor_raiz, b->nota_raiz);
        if (cand.size() > 1) {
            Move v = vota(cand);
            if (v != Move::none() && v != melhor_raiz) {
                // Anunciar: o lance vai mudar depois da ultima linha impressa,
                // e sem isto o arbitro fica outra vez com uma variante que ja'
                // nao e' a nossa -- o mesmo defeito, por um terceiro caminho.
                saida() << "info string votacao: " << UCIEngine::move(v, false)
                        << " com " << cand.size() << " fios" << std::endl;
                melhor_raiz = v;
                pv_tab[0][0] = v;
                pv_n[0]      = 1;
                saida() << "info depth " << ultima_prof << " score cp " << nota_raiz / 2
                        << " pv " << UCIEngine::move(v, false) << std::endl;
            }
        }
    }


    // Recusar uma repeticao quando se esta' a ganhar.
    //
    // Guardar so' o melhor lance nao deixa nada em que cair quando ele repete:
    // ha' um lance e nenhuma razao para preferir outro. Com a lista toda
    // pontuada, a repeticao pode ser recusada a favor de algo quase tao bom -- e
    // QUANTO pior se aceita e' um numero e nao um acidente.
    if (melhor_raiz != Move::none() && nota_raiz >= p.recusa_limiar
        && repete_ja(pos, melhor_raiz)) {
        Move alt = Move::none();
        int  melhor_alt = -INFINITO;
        for (auto& [m, sc] : notas_raiz)
            if (m != melhor_raiz && sc > melhor_alt && !repete_ja(pos, m)) {
                melhor_alt = sc;
                alt        = m;
            }
        if (alt != Move::none() && melhor_alt >= nota_raiz - p.recusa_margem) {
            melhor_raiz = alt;
            // Trocado DEPOIS da ultima linha anunciada. Sem isto o arbitro fica
            // com a variante do lance que se acabou de recusar, e o `bestmove`
            // nao bate certo com ela -- o mesmo defeito do corte de iteracao,
            // por outro caminho. A variante aqui e' de um lance so': nao ha'
            // linha pesquisada para este, ha' a pontuacao que a raiz lhe deu.
            saida() << "info string recusada a repeticao: "
                    << UCIEngine::move(melhor_raiz, false) << " em vez do melhor"
                    << std::endl;
            saida() << "info depth " << sel_prof << " score cp " << melhor_alt / 2
                    << " pv " << UCIEngine::move(melhor_raiz, false) << std::endl;
        }
    }

    // Nunca devolver nada: se ate' a profundidade um foi cortada, joga-se o
    // primeiro lance legal em vez de perder a partida.
    if (melhor_raiz == Move::none()) {
        MoveList<LEGAL> ml(pos);
        if (ml.size() > 0) {
            melhor_raiz = *ml.begin();
            // Tambem este tem de ser anunciado: e' um lance que nenhuma linha
            // anterior menciona.
            saida() << "info depth 1 score cp 0 pv "
                    << UCIEngine::move(melhor_raiz, false) << std::endl;
        }
    }
    if (DIAG.forma) {
        {
            extern std::uint64_t g_corr_n, g_corr_zero, g_corr_soma;
            if (g_corr_n)
                saida() << "info string CORR chamadas=" << g_corr_n
                          << " a_zero=" << (100.0 * g_corr_zero / g_corr_n) << "%"
                          << " modulo_medio=" << (double(g_corr_soma) / g_corr_n)
                          << std::endl;
        }
        std::uint64_t tot = g_forma_qs;
        for (int d = 1; d < 64; ++d) tot += g_forma[d];
        std::uint64_t r13 = g_forma[1] + g_forma[2] + g_forma[3];
        std::uint64_t f14 = 0;
        for (int d = 14; d < 64; ++d) f14 += g_forma[d];
        saida() << "info string ORDEM cortes=" << g_cortes
                  << " no_primeiro=" << g_cortes_1
                  << " (" << (g_cortes ? 100.0 * g_cortes_1 / g_cortes : 0) << "%)"
                  << std::endl;
        saida() << "info string FORMA total=" << tot
                  << " qs=" << (tot ? 100.0 * g_forma_qs / tot : 0) << "%"
                  << " prof1-3=" << (tot ? 100.0 * r13 / tot : 0) << "%"
                  << " prof>=14=" << (tot ? 100.0 * f14 / tot : 0) << "%" << std::endl;
        saida() << "info string FORMA por profundidade:";
        for (int d = 1; d <= 16; ++d)
            saida() << " " << d << ":" << (tot ? 100.0 * g_forma[d] / tot : 0);
        saida() << std::endl;
    }
    if (DIAG.margem_estudo) {
        saida() << "info string MARGEM  prof     n      p50      p90      p95      p99"
                  << std::endl;
        for (int d = 1; d < EST_PROF; ++d) {
            auto& v = g_desmente[d];
            if (v.size() < 50)
                continue;
            std::sort(v.begin(), v.end());
            auto q = [&](double f) { return v[std::min(v.size() - 1, size_t(v.size() * f))]; };
            saida() << "info string MARGEM  " << d << "  " << v.size()
                      << "  " << q(0.50) << "  " << q(0.90) << "  " << q(0.95)
                      << "  " << q(0.99) << std::endl;
        }
    }
    if (DIAG.sing_conta)
        saida() << "info string AVAL chamadas=" << n_aval << " nos=" << nos
                  << " por_no=" << (nos ? double(n_aval) / double(nos) : 0.0)
                  << " | busca: calc=" << n_aval_bs << " tabela=" << n_hit_bs
                  << " (" << (n_aval_bs + n_hit_bs ? 100.0 * n_hit_bs / (n_aval_bs + n_hit_bs) : 0) << "% poupados)"
                  << " | qs: calc=" << n_aval_qs << " tabela=" << n_hit_qs
                  << " (" << (n_aval_qs + n_hit_qs ? 100.0 * n_hit_qs / (n_aval_qs + n_hit_qs) : 0) << "% poupados)"
                  << std::endl;
    if (DIAG.quem) {
        std::uint64_t tot = 0;
        for (int k = 0; k < Q_N; ++k) tot += g_q_modulo[k];
        saida() << "info string QUEM PESA NA REDUCAO -- modulo, em milesimos de ply" << std::endl;
        for (int k = 0; k < Q_N; ++k)
            saida() << "info string   " << Q_NOMES[k] << "\t" << g_q_vezes[k]
                    << " vezes\tmodulo " << g_q_modulo[k]
                    << "\tmedio |" << (g_q_vezes[k] ? g_q_modulo[k] / g_q_vezes[k] : 0) << "|"
                    << "\t" << (tot ? 100.0 * double(g_q_modulo[k]) / double(tot) : 0.0) << "%"
                    << std::endl;
    }
    if (DIAG.sing_conta)
        saida() << "info string PC nos_entrados=" << pc_nos << " tentativas=" << pc_tentativas
                  << " passaram_qs=" << pc_passou_qs << " cortes=" << pc_cortes << std::endl;
    if (DIAG.sing_conta)
        saida() << "info string SING chamadas=" << conta_sing << " nos=" << nos_sing
                  << " (" << (nos ? 100.0 * nos_sing / nos : 0) << "% da arvore)"
                  << " ext1=" << conta_ext1 << " ext2=" << conta_ext2
                  << " ext-1=" << conta_ext_neg << std::endl;
    saida() << "bestmove " << UCIEngine::move(melhor_raiz, false) << std::endl;
}

}  // namespace Kestrel
