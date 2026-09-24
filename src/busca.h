// The search. This part is ours.
//
// Every value here was SWEPT in this engine's own tree, not inherited. A
// constant borrowed from another program is calibrated for that program's
// evaluation scale and history tables; ours are about fifty times smaller, so
// the form transfers and the number does not.
//
// What is not implemented is marked FALTA rather than left silent: a missing
// piece has to be visible in the code.
#ifndef KS_BUSCA_H
#define KS_BUSCA_H

#include <iostream>
#include <memory>
#include <string>
#include <thread>
#include <atomic>
#include <chrono>
#include <mutex>
#include <cstdint>
#include <vector>

#include "aval.h"
#include "position.h"
#include "tt.h"
#include "types.h"

namespace Kestrel {

/// A tranca da saida do motor. Partilhada pela thread dos comandos e pela da
/// busca. Definida em busca.cpp.
std::mutex& trinco_saida();

/// Uma linha de saida, escrita sob a tranca. Vive ate' ao fim da expressao que
/// a usa -- `saida() << a << b << std::endl;` --, e e' ai' que larga a tranca.
class Linha {
   public:
    Linha(std::ostream& o, std::mutex* m) : os(o) {
        if (m)
            tr = std::unique_lock<std::mutex>(*m);
    }
    template<class T>
    Linha& operator<<(const T& v) {
        os << v;
        return *this;
    }
    Linha& operator<<(std::ostream& (*f)(std::ostream&)) {
        f(os);
        return *this;
    }

   private:
    std::ostream&                os;
    std::unique_lock<std::mutex> tr;
};

// `MAX_PLY`, `VALUE_MATE` and the mate bound come from the substrate. They are
// not redefined here, so that there is only one truth about each number.

// Everything the engine compares against a number lives here; nothing is
// hardcoded in the middle of the search. That rule is what made it possible to
// sweep ninety-eight parameters without recompiling once.
// JA' MEDIDO E REJEITADO EM 8 DE SETEMBRO, e redescoberto a 21 a custo de um
// dia inteiro. Do `PLANO.md` que se perdeu com a maquina:
//
//   "alinhar a arvore pela forma        -65 Elo. A forma nao e' qualidade"
//   "transplantar a reducao inteira     arvore x5. As constantes nao transferem"
//   "geracao por etapas                 -4,5% de instrucoes, -3,9 pontos de ordem"
//   "bonus de killer no historico       -16,4% de arvore, +0,8 de acerto,
//                                       -2,48 +/- 9,62 em partidas"
//
// As duas primeiras sao as duas maiores experiencias de 21-09. A forma da arvore
// foi usada o dia inteiro como instrumento -- ja' estava medida a -65 Elo. O
// bloco de constantes da referencia foi montado e testado -- ja' estava escrito
// que nao transferem.
//
// A licao nao e' sobre busca, e' sobre onde se guarda o que se mede: aquele
// documento vivia num ficheiro solto e morreu com a maquina. Os numeros que
// sobreviveram foram os que estavam ao lado do codigo. E' por isso que este
// ficheiro esta' cheio de paragrafos como este.
//
// E FICA A PEDRA POR VIRAR que esse plano deixou: "killers fora da banda --
// corte ao primeiro lance 76,6% -> 79,7%". Medido hoje, o nosso corte ao
// primeiro lance e' **76,71%** -- exactamente o valor de partida. Os tres
// pontos nunca chegaram ao motor. E `usa_killers = 1`, que e' o que temos,
// PIORA para 75,85%: o que ca' esta' nao e' a versao "fora da banda". A
// referencia esta' em 85,74%.

// NAO SAO AS CONSTANTES. Medido dos dois lados.
//
// A pergunta: a referencia corre com os parametros DELA e, com a NOSSA rede,
// fica dezenas de Elo acima de nos. Serao as nossas oitenta constantes
// simplesmente piores?
//
// Testou-se a serio -- todas de uma vez, nao uma a uma, porque uma a uma da'
// sempre negativo (ver o paragrafo do defeito absorvido). `KS_PADRAO_SF` troca
// o bloco inteiro: futilidade inversa, razoring, lance nulo, LMP, poda pelo
// historico, futilidade dos tranquilos, tabela de reducao, IIR.
//
//   em partidas, 10+0,1, 200 jogos:
//     os NOSSOS valores contra o preset:  **+45,42 +/- 19,38**
//     33 vitorias contra 7
//
//   e a profundidade a 3 milhoes de nos, nesta arvore:
//     nosso       ply 20    prof1-3 55,4%   prof>=14 0,23%
//     padrao_sf   ply 20    prof1-3 71,3%   prof>=14 0,31%
//     referencia  ply 23    prof1-3 40,2%   prof>=14 1,93%
//
// O preset nao ganha um ply aqui, e perde 45 Elo. Num motor menos reposto
// ganhava dois plies -- e perdia na mesma.
//
// O QUE ISTO ELIMINA: a diferenca para a referencia nao esta' nos NUMEROS com
// que fazemos as coisas. Esta' no que a busca dela FAZ e a nossa nao faz.
// Estrutura, nao calibracao.
//
// RESSALVA, porque o teste tem um limite real: o que se mede e' uma TRANSCRICAO
// dos valores deles, e algumas formas nao tem correspondencia -- o razoring
// deles e' quadratico na profundidade e o nosso linear, a reducao deles tem um
// termo `delta*577/rootDelta` que nao temos, e a tabela `2872/128*log(i)` foi
// convertida para a nossa base/divisor por aproximacao. Conclui-se "esta
// traducao, neste motor, perde"; nao se conclui "os valores deles sao maus".

// NENHUM INDICADOR QUE TEMOS PREVE ELO. Nem um.
//
// Contados num dia de trabalho, com partidas a fechar cada pergunta:
//
//   contagem de nos          apontou ao contrario do resultado DEZ vezes
//   tempo ate' a profundidade aprovou a margem do RFP, que perdeu 20 Elo
//   forma da arvore          o par da poda pelo historico moveu-a para junto
//                            da referencia -- nos fundos a duplicar, rasos de
//                            57,3% para 52,8% -- e valeu ZERO (+-7 em 1400
//                            partidas por ponta)
//
// Os tres servem para escolher ONDE procurar. Nenhum diz O QUE vale. Quem
// decide sao as partidas, e nao ha' atalho -- procurar um custou a este projecto
// mais tempo do que qualquer outra coisa.
//
// E O PADRAO DE ONDE VEM O ELO, medido no mesmo dia:
//
//   termo da profundidade no lance nulo   NOSSO, declarado e morto      +18
//   avaliacao completa                    NOSSA, amputada por escala    +18
//   low_ply                               importado                     -48
//   ProbCut pequeno                       importado                      -9
//   par da poda pelo historico            importado e adaptado            0
//   margem do RFP                         derivada dos nossos dados     -20
//   IIR com os gates deles                importado             ultimo de 4
//
// Cinco importacoes, cinco fracassos. Duas restauracoes de codigo NOSSO que
// estava morto, os dois unicos ganhos. E o placar recuperado do motor perdido
// diz o mesmo com outros numeros: seis importacoes, seis fracassos; duas
// restauracoes, +18 cada.
//
// A razao e' a mesma do paragrafo seguinte: um mecanismo importado aterra numa
// paisagem calibrada SEM ele, e perde mesmo estando certo. Um mecanismo nosso
// que nunca correu nao tem vizinhos afinados a` sua volta -- por isso e' que
// liga-lo paga.
//
// Procurar codigo morto, nao ideias novas.

// UM DEFEITO QUE VIVE TEMPO SUFICIENTE E' ABSORVIDO PELA AFINACAO FEITA POR
// CIMA DELE, E CORRIGI-LO SOZINHO PODE PIORAR O MOTOR.
//
// Isto nao e' teoria. Aconteceu aqui, com medida, argumento e forma derivada dos
// nossos proprios dados:
//
// A margem do RFP e' `110 * prof`, fixa e sem tecto, contra `min(45+4p, 85)*p`
// da referencia. Mediu-se o que a margem devia cobrir -- `estatica - nota
// final` -- e da' um p90 plano em ~150 a partir do quarto ply. A` profundidade
// 12 a nossa margem e' 1.320 contra 147: nove vezes o que a busca alguma vez
// recupera. Construiram-se duas formas nossas sobre esses dados e cortavam 45%
// dos nos E do tempo.
//
// Em 2.602 partidas por ponta, a margem "errada" ganhou e as duas corrigidas
// foram as piores das quatro.
//
// A razao: as outras margens, as reducoes e as podas foram todas varridas com
// esta margem no sitio. Ela nao e' um erro isolado -- e' o eixo a` volta do qual
// o resto foi calibrado. Corrigi-la sozinha nao devolve o motor a um estado bom;
// devolve-o a um estado que ninguem afinou.
//
// COMO ISTO MUDA O METODO:
//
//   1. Um defeito antigo so' se corrige COM os vizinhos, ou nao se corrige.
//      Medir "com e sem" mede a mudanca agrilhoada a constantes que ja' nao lhe
//      servem, e o resultado e' quase sempre negativo mesmo quando a mudanca
//      esta' certa.
//   2. Os defeitos que PAGARAM hoje foram os que ninguem tinha afinado a` volta:
//      um parametro declarado e nunca lido, e tres camadas de avaliacao
//      amputadas. Nao havia calibracao por cima porque o mecanismo nunca correu.
//   3. Por isso vale mais procurar mecanismos MORTOS do que mecanismos mal
//      afinados. Um morto nao tem vizinhos desafinados a defende-lo.
//
// E O TESTE PARA OS ENCONTRAR: contagens de nos IDENTICAS ao ultimo digito com
// valores diferentes da manete. Isso nao e' "nao faz diferenca" -- e' codigo
// morto. Apanhou aqui um `low_ply` posto na funcao errada (arvore igual com peso
// 8 e com peso 100.000) e um `KS_AMEACA_F` que se chamava `KS_AMEACA`.

// VARRER MANETES PELA CONTAGEM DE NOS NAO FUNCIONA. Nao e' que engane as
// vezes: nao produz gradiente nenhum.
//
// Medido a 21-09 em tres manetes diferentes, na mesma noite, sempre sobre
// posicoes fixas a` profundidade 12:
//
//   margem do ProbCut, 4 posicoes somadas
//     600 -2,5%   700 -0,9%   800 +5,5%    -- e a 700 poupa 9% numa posicao e
//                                             DUPLICA outra
//   `nmp_prof_div`, 6 posicoes somadas
//     2: 1.123.052   3: 1.194.733   4: 1.395.874
//     5: 1.310.421   7: 1.051.079  99: 1.338.237
//     -- o 7 da' a menor, o 4 a maior, e o "desligado" fica no meio
//   margens do SEE, uma posicao
//     capturas=20 tranquilos=1 -> 45.625
//     capturas=20 tranquilos=0 -> 77.714
//     -- uma unidade de diferenca, setenta por cento de arvore
//
// A razao e' simples quando se ve: mudar uma margem muda a ORDEM pela qual a
// busca encontra as coisas, e a arvore a uma profundidade fixa depende de
// acertar ou nao na variante cedo. Isso e' quase binario por posicao. Somar
// meia duzia de posicoes nao faz media nenhuma -- faz uma soma de meia duzia de
// moedas ao ar.
//
// O que a contagem de nos SERVE para: dizer se uma manete esta' viva. Arvore
// identica ao no' com valores diferentes significa que ela nao esta' a ser lida
// -- e isso ja' apanhou aqui um `KS_AMEACA_F` que se chamava `KS_AMEACA`.
//
// Para saber quanto vale, so' partidas. Nao ha' atalho, e procurar um custou a
// este projecto mais tempo do que qualquer outra coisa.

struct Parametros {
    // --- reverse futility: the form this engine uses by default ---
    // A margin linear in depth, with a fixed discount when improving. With
    // `rfp_mult = 0` the per-ply multiplier is constant and the cap never
    // bites; a default pinned to the edge of its own range (the cap at 100000)
    // is the signature of a term that is deliberately inert.
    /// A MARGEM DA FUTILIDADE INVERSA. 110 por ply, fixa, sem tecto.
    ///
    /// **MEDIDA E DEFENDIDA.** Parece errada e nao e'. A historia vale mais do
    /// que o numero:
    ///
    /// A forma da referencia e' `min(45 + 4*prof, 85) * prof` -- cresce e
    /// satura. A nossa e' `110 * prof` -- cresce sem travao. Em unidades de
    /// peao, 0,55 por ply contra 0,41: exigimos 34% mais vantagem antes de
    /// podar. Trocar pela forma deles corta **45% dos nos E do tempo**.
    ///
    /// Depois mediu-se o que a margem devia cobrir -- `estatica - nota final`,
    /// por ply, no nosso proprio motor:
    ///
    ///     prof  1   p90  94       prof  6   p90 158
    ///     prof  2   p90  61       prof  8   p90 228
    ///     prof  4   p90 147       prof 12   p90 147
    ///
    /// Plana a partir do quarto ply. A` profundidade 12 a nossa margem e' 1.320
    /// contra um p90 de 147 -- NOVE vezes o que a busca alguma vez recupera.
    /// Com isso construiram-se duas formas nossas, varridas nos nossos dados,
    /// que batiam a deles em 10% de nos.
    ///
    /// Em partidas, quatro pontas, 2.602 partidas cada:
    ///
    ///     hoje (110 fixo)   **+11,75 +/- 5,33**   <- a melhor
    ///     forma deles        +4,15 +/- 5,46
    ///     nossa 45/4/70      -6,95 +/- 5,41
    ///     nossa 35/5/85      -8,96 +/- 5,63
    ///
    /// Os -45% de nos e de tempo compraram Elo NEGATIVO. A margem larga corta
    /// menos, e os cortes que ela evita valem mais do que os nos que gasta.
    ///
    /// A LICAO, que e' o que fica: medir "quanto a busca desmente a estatica"
    /// NAO mede "quanto custa cortar mal" -- e e' a segunda que manda. A
    /// primeira tem instrumento e a segunda nao. Um argumento elegante
    /// construido sobre a medida errada e' so' um erro bem apresentado.
    ///
    /// Nao mexer sem 2.500 partidas a dizer o contrario.
    int rfp_margem     = 110;
    int rfp_mult       = 0;
    int rfp_tecto      = 100000;
    int rfp_melhorando = 150;
    /// A cap on the TOTAL, not on the per-ply term. It comes from our own
    /// measurement: how much the search contradicts the static evaluation
    /// (`static - score`) does not grow with depth in this tree. The p90
    /// settles at about 150 from the fourth ply on and stays there. With
    /// `110*depth` the margin reaches 1,320 at depth 12 -- nine times anything
    /// the search ever recovers -- and reverse futility stops firing at all.
    /// 0 = no cap.
    int rfp_tecto_tot  = 0;
    int rfp_prof       = 9;

    // --- poda do no' interior, a` profundidade NOMINAL ---
    int lmp_base        = 3;
    int lmp_prof        = 6;

    /// Nao estando a melhorar, corta-se o limite do LMP a meio. Do binario,
    /// `negamax` em `43818c`:
    ///
    ///     43818c:  mov 0x30(%rbp),%r9d    ; lmp_melhora
    ///     438190:  imul %edx,%eax         ; prof*prof
    ///     438195:  add 0x28(%rbp),%eax    ; + lmp_base
    ///     43819b:  je 4381b0              ; lmp_melhora == 0 -> salta
    ///     43819d:  cmpb $0x0,0xc4(%rsp)   ; melhorando
    ///     4381a5:  jne 4381b0             ; melhorando -> salta
    ///     4381a7:  shr $0x1f / add / sar $1  ; conta /= 2
    ///
    /// A ZERO no molde do binario.
    int lmp_melhora   = 0;
    int hist_poda       = 600;
    /// The depth cap on history pruning, and the shape of its margin. They are
    /// a pair: capping the depth only makes sense with the quadratic margin.
    int hpoda_prof      = 4;
    int hpoda_lin       = 0;
    int fut_base        = 100;
    int fut_decl        = 150;
    int fut_prof        = 12;
    int fut_hist_div    = 75;
    int see_poda        = 70;
    int see_poda_tranq  = 5;

    // --- extensao singular ---
    /// Depth discount after alpha rises. 0 = off.
    ///
    /// Once a move raises alpha, this node already has a real best. The moves
    /// that follow do not have to prove they are good in absolute terms -- they
    /// only have to beat THAT one. Searching them at full depth is paying for a
    /// proof that is no longer needed.
    ///
    /// The value comes from our own sweep. The same idea in another engine took
    /// 1 and later 3, and those numbers are calibrated for a tree that is not
    /// this one.
    /// Killers. 0 = none. The value IS the band they are given.
    ///
    /// The note that used to stand here said: "quiet moves that were killers
    /// hit first 60.1% of the time against 11.9% for generic quiets, and that
    /// difference comes out of the history tables -- which already know them --
    /// without needing a band of their own". The first half is MEASURED; the
    /// second is an INFERENCE, and the whole decision not to have killers rests
    /// on it.
    ///
    /// Against that inference stands the ordering number: this engine cuts on
    /// the first move in 69.2% of nodes. If the history really knew them that
    /// well, one would expect better.
    ///
    /// So: a knob, and let the games decide instead of the reasoning.
    /// A singular so' com limite INFERIOR, como a referencia. 0 = como estava.
    ///
    /// A nossa porta aceita `Inferior` OU `Exacto`; a deles so' testa
    /// `ttData.bound & BOUND_LOWER`. Aceitar exactos abre um conjunto muito
    /// maior de sondagens, e e' a diferenca que explica as cinco vezes mais
    /// chamadas que o documento de fidelidade notou sem lhe achar a causa.
    ///
    /// MEDIDO: na tactica, 749 sondagens a` profundidade 14, das quais 74%
    /// estendem e 0,1% reduzem. A referencia gasta 91.311 nos onde nos gastamos
    /// 295.150, e a nossa profundidade selectiva vai a 34 onde a dela vai a 22.
    int sing_so_inferior = 0;
    /// Quanto a reducao cresce quando o ply SEGUINTE ja' cortou muitas vezes.
    /// Em milesimos de ply, como o resto da reducao. 0 = como estava.
    ///
    /// Um no' cujos filhos nao param de cortar e' um no' facil: a referencia
    /// reduz mais la'. Nos nao tinhamos o contador nem o termo.
    ///
    /// Os valores deles sao 264 e 1095 em 1024 avos, que em milesimos dao 258 e
    /// 1069. A terceira parcela deles depende de `allNode`, nocao que nao temos.
    /// A NOTA DO LANCE DA TABELA QUANDO O LIMITE E' SUPERIOR.
    ///
    /// Um lance guardado com limite superior e' um lance que NAO provou nada:
    /// a busca que o guardou falhou em baixo. Po-lo no topo da lista na mesma
    /// que um lance que cortou e' dar-lhe credito que ele nao tem.
    ///
    /// 1000000 = como estava (topo, sempre). Omissao do binario que jogava.
    int tt_sup_nota   = 1000000;
    int cut_cnt_base  = 0;
    int cut_cnt_mais  = 0;
    /// A recaptura escapa ao filtro do SEE na quiescencia. 0 = como estava.
    ///
    /// A referencia nao poda o lance que vai para a casa que o adversario
    /// acabou de ocupar: e' a outra metade de uma troca ja' comecada, e
    /// julga-la pelo SEE isolado e' julgar meia troca. O comentario que explica
    /// isto ja' estava escrito na nossa quiescencia -- o codigo e' que nunca
    /// existiu.
    int qs_recaptura  = 0;
    int usa_killers = 0;
    /// BAIXAR A PROFUNDIDADE QUANDO UM LANCE SOBE O ALPHA.
    ///
    /// A ideia: se este lance ja' provou ser melhor do que tudo o que veio
    /// antes, os que vem a seguir tem mais a provar e podem ser vistos mais
    /// raso. Soa bem, e o mecanismo funciona -- mede-se a mexer: a arvore vai
    /// de 123.557 nos para 195.386 e depois 235.019 conforme se aumenta.
    ///
    /// MEDIDO EM PARTIDAS E REJEITADO, tres vezes e das tres negativo:
    ///
    ///     -21,37 +/- 9,41    contra o binario que estava no bot
    ///     -44,06 +/- 11,92   sozinho, 888 partidas
    ///     -74                registado na lista dos rejeitados
    ///
    /// Fica a ZERO e fica esta nota, que e' o que falta a uma manete rejeitada
    /// para nao voltar a ser tentada. Estava aqui sem comentario nenhum -- um
    /// zero sem explicacao le-se como "ainda nao experimentado", que e' o
    /// convite exacto para alguem o experimentar outra vez.
    ///
    /// `ad_min` e `ad_max` sao a janela de profundidade onde agiria. Nao servem
    /// de nada com isto a zero, e ficam porque um dia se pode querer medir a
    /// ideia noutra forma -- nao esta.
    int alpha_desc  = 0;
    /// A JANELA DE PROFUNDIDADE do desconto do alpha. Estava CRAVADA em
    /// `prof > 2 && prof < 16`; o binario que jogava traz 3 e 12.
    ///
    /// Em cima a nota ainda salta e descontar profundidade cega a raiz; em
    /// baixo ja' nao ha' profundidade que valha a pena descontar. A janela
    /// existe para o desconto so' agir onde ele paga.
    int ad_min      = 3;
    int ad_max      = 12;
    /// A EXTENSAO SINGULAR -- a peca mais cara e a mais valiosa deste motor.
    ///
    /// Medida com o interruptor `KS_OFF_SING`, o mesmo binario dos dois lados,
    /// 1 fio, 5+0,05:
    ///
    ///     com singulares contra sem:  **+83,82 +/- 19,83** em 338 partidas
    ///
    /// E o que ela custa, a` profundidade 12:
    ///
    ///     Kiwipete   87.098 -> 26.119 nos sem ela   -- 70% da arvore
    ///     inicial    88.599 -> 67.803 nos sem ela   -- 23% da arvore
    ///
    /// Setenta por cento da arvore numa posicao rica. A busca de verificacao em
    /// si e' so' 16% dos nos; o resto e' a profundidade que ela distribui, a
    /// compor-se por cada ramo que toca.
    ///
    /// Porque isto fica escrito: olhando so' para a contagem de nos, este e' o
    /// maior bloco de arvore que ha' para recuperar, e a tentacao de o apertar
    /// e' enorme. As partidas dizem que seria o pior negocio do motor. A arvore
    /// e' grande porque as extensoes a COMPRAM, e compram-na bem.
    int sing_prof   = 5;
    int sing_margem = 2;
    int ext_dupla   = 40;
    // Extensao DUPLA quando a entrada da tabela e' de limite exacto.
    //
    // 1 = permite (o que se fazia). 0 = so' extensao simples nesse caso.
    //
    // A porta da singular aceita `Inferior` ou `Exacto`. Com `Inferior` a
    // tabela diz "pelo menos isto" e a sondagem a ficar muito aquem e' mesmo
    // sinal de que o lance esta' sozinho. Com `Exacto` a tabela ja' sabe o
    // valor: a distancia a que os outros ficam diz menos, porque nao ha'
    // incerteza para a sondagem resolver -- e mesmo assim davamos dois plies.
    //
    // MEDIDO AQUI (2026-09-11, posicao inicial, profundidade 13): a singular
    // vale 65% da arvore deste motor nesta posicao -- 150 961 nos com ela,
    // 52 653 sem. As buscas de verificacao sao so' 7,4%; o caro sao as
    // extensoes que disparam, 623 simples e 154 duplas. Sem a singular a nossa
    // arvore fica MENOR que a do outro motor nosso (0,88x).
    int ext_dupla_exacto = 1;
    // Quanto e' que a singular TIRA quando a sondagem diz que o lance nao esta'
    // sozinho. 1 = o que se fazia.
    //
    // A sondagem singular e' UMA busca que paga QUATRO decisoes: estender,
    // cortar o no', reduzir, ou nada. Mexer so' no lado da extensao e deixar o
    // lado da reducao como esta' e' pagar a busca inteira e usar metade da
    // resposta -- foi o que aconteceu quando medi a guarda do `ext_dupla_exacto`
    // sozinha: menos extensoes (154 -> 135) e MAIS arvore (+15,9%), porque as
    // extensoes resolvem linhas e tira-las gasta largura a nao resolver nada.
    //
    // Os dois andam juntos: se se confia menos na sondagem para estender,
    // confia-se mais nela para reduzir.
    int ext_neg = 1;

    // --- o que fica DESLIGADO, como no half2k ---
    // Nao se apagam: ficam aqui a dizer que existem e que nao estao em uso, para
    // poderem ser medidos um a um. Ver doc/FIDELIDADE.md.
    // Sem tecto, a margem dava treze peoes a` profundidade 12. Uma margem dessas
    // nao estabelece coisa nenhuma: corta nos que nao estavam decididos, e o que
    // se poupa paga-se em re-buscas mais abaixo.
    int rfpsf_base   = 40;    // outro motor usa 45
    int rfpsf_decl   = 5;     // 4
    int rfpsf_tecto  = 85;
    int rfpsf_sem_tt = 34;    // 20 -- sem entrada na tabela pesa mais na nossa escala
    int rfpsf_impr   = 5600;  // 2789 -- as nossas tabelas sao ~50x menores
    int rfpsf_adv    = 290;   // 335
    int rfpsf_prof   = 19;
    int rfpsf_depth  = 9;

    // --- razoring ---
    int razor_margem = 348;
    int razor_prof   = 5;

    // --- lance nulo ---
    int nmp_base     = 4;
    int nmp_div      = 6;    // tecto do termo da estatica (eles nao tem)
    int nmp_div_est  = 200;  // divisor do termo da estatica (deles: 256)
    int nmp_prof_div = 5;   // a reducao TEM de crescer com a profundidade

    int probcut_margem = 294;

    // --- ProbCut com busca ---
    //
    // Uma captura que, verificada, volta muito acima do beta torna o no' inteiro
    // dispensavel. A margem desce quando as coisas ja' estao a melhorar, porque
    // ai' e' preciso menos prova.
    //
    // Os numeros sao NOSSOS e estao para varrer. A escala interna do half2k tem
    // o peao perto de 200, portanto uma margem de 205 e' cerca de um peao -- que
    // e' a ordem de grandeza que faz sentido para "muito acima".
    int pc_margem   = 205;   // margem sobre o beta
    int pc_impr     = 54;    // desconto quando esta' a melhorar
    int pc_reducao  = 3;     // plies a menos na busca de verificacao
    int pc_red_impr = 5;     // e mais dois quando esta' a melhorar
    // DESLIGADO (`pc_prof = 0`). MEDIDO na mesma posicao a` profundidade 16:
    //
    //     sem ProbCut     425.973 nos
    //     com ProbCut     703.499 nos
    //     referencia      265.532 nos
    //
    // O mecanismo FUNCIONA -- 11.149 tentativas, 6.849 cortes, 61% de sucesso --
    // mas a verificacao custa mais do que o corte poupa. A razao e' a nossa
    // arvore: cada busca de verificacao corre dentro dela e por isso sai cara,
    // enquanto quem tem a arvore pequena a paga barata. Uma taxa de 61% tambem
    // diz que a margem esta' baixa de mais para a NOSSA escala -- isto nao esta'
    // a distinguir posicoes, esta' a fazer uma busca rasa em toda a parte.
    //
    // A MARGEM FOI VARRIDA, e nao salva o mecanismo. Quatro posicoes, a
    // profundidade 12, `pc_prof = 5`, contra o mesmo motor com ele desligado:
    //
    //     margem        600       700       800
    //     vs desligado  -2,5%     -0,9%     +5,5%
    //
    // No conjunto e' ruido. E por posicao e' pior do que ruido -- e' caotico: a
    // margem 700 poupa 9% numa posicao (83.704 -> 75.994) e quase DUPLICA outra
    // (228.095 -> 422.266). Uma manete que faz as duas coisas com o mesmo
    // numero nao esta' a medir o que dissemos que media.
    //
    // A taxa de sucesso responde como esperado -- 81% a 205, 88% a 700 com
    // metade das tentativas, 100% a 1000 com sete -- portanto a margem FUNCIONA
    // como filtro. O que ela nao faz e' tornar o mecanismo rentavel aqui.
    //
    // Fica desligado, e agora com a condicao daquela frase cumprida: ja' nao ha'
    // "falta varrer a margem" a dever-se a ninguem. O que resta e' a outra
    // metade -- "quando a arvore estiver menor" -- e essa e' consequencia, nao
    // causa: se a arvore encolher por outra via, este mecanismo volta a valer a
    // pena sozinho.
    int pc_prof     = 0;     // profundidade minima para tentar; 0 = desligado

    // --- a poda do no' interior, com a profundidade REDUZIDA ---
    // A profundidade a que o lance VAI mesmo ser procurado, e nao a nominal.
    // Sete tentativas anteriores falharam por levarem pecas soltas do conjunto.
    int poda_ttpv       = 929;
    int poda_capt_base  = 234;
    int poda_capt_decl  = 247;
    int poda_capt_hist  = 134;
    int poda_capt_see   = 177;
    int poda_cont_prune = 965;
    int poda_fut_decl   = 119;
    int poda_fut_base   = 164;
    int poda_see        = 23;
    // O divisor do historico tem FORMA, nao e' um escalar: menor no meio da
    // arvore, onde o historico ja' aprendeu alguma coisa, maior nas pontas.
    int pdiv_base   = 700;
    /// ORFAOS os quatro (`pdiv_base`, `pdiv_curva`, `pdiv_centro`, `pdiv_tecto`).
    /// Nenhum e' lido. A ideia esta' escrita acima -- o divisor do historico com
    /// FORMA em vez de escalar -- e nunca foi implementada: o que corre hoje e'
    /// o `lmr_hist_div = 22000`, plano em toda a arvore.
    ///
    /// Ficam como ideia por fazer e NAO como manete desligada. A diferenca
    /// importa: uma manete desligada poe-se a um e mede-se; isto precisa de
    /// codigo que nao existe.
    int pdiv_curva  = 0;
    int pdiv_centro = 8;
    int pdiv_tecto  = 16;

    // --- reducao ---
    int lmr_base      = 77;    // centesimos
    int lmr_div       = 236;   // centesimos
    int lmr_piora_f   = 197;
    int lmr_nonpv_f   = 1024;
    /// O TERMO DO NO' DE CORTE. Reduz mais onde se espera cortar.
    ///
    /// **MEDIDO E MUDADO de 2048 para 1024.**
    ///
    /// O nosso motor contra si proprio, so' este termo a mover, `lmr_ttpv`
    /// fixo em 1024, 1 fio, 5+0,05:
    ///
    ///     1024 contra 2048:  **+19,97 +/- 10,68** em 1080 partidas
    ///
    /// Estavel desde as 500: +9,8 (320), +20,9 (502), +22,6 (800), +20,5
    /// (1000), +19,97 (1080). Um efeito real assenta assim; ruido nao.
    ///
    /// O historico abaixo fica porque explica porque nao se mudou antes. Tres doses do par (este termo e o
    /// `lmr_ttpv` movidos juntos), o mesmo binario nas tres pontas, 10+0,1:
    ///
    ///      960 partidas   1024 +15,57 +/- 11,22    2048 -14,88 +/- 10,93
    ///     1014 partidas   1024 +14,74 +/- 10,96    2048 -13,37 +/- 10,64
    ///     1402 partidas   1024  +7,68 +/-  9,52    2048  -6,70 +/-  9,36
    ///
    /// A vantagem caiu de 16 Elo para 8, o intervalo passou a apanhar o zero, e
    /// os DOIS extremos vieram para o meio ao mesmo tempo. Essa e' a assinatura
    /// de ruido com forma de sinal a desfazer-se -- um efeito real ganha
    /// definicao com as partidas, nao a perde.
    ///
    /// A primeira versao desta nota dizia "vinte e oito Elo entre a nossa dose
    /// e a melhor". Estava escrita sobre as 1014 e nao se aguentou. Fica aqui
    /// porque o erro e' instrutivo: ler uma diferenca a meio de uma corrida e
    /// trata-la como fechada e' o mesmo engano que a contagem de nos, por outro
    /// caminho.
    ///
    /// Porque e' que o 2048 ficou: **a corrida morreu a meio.** Estava em 1014
    /// de 60.000 partidas planeadas quando as maquinas se foram abaixo a 19-09,
    /// e o resultado nunca chegou a ser aplicado. Nao ha' registo de decisao
    /// nenhuma sobre esta manete -- so' o inventario a anotar o valor que o
    /// binario trazia. O mesmo que aconteceu ao `TmPawnN`, que morreu na
    /// partida 13.126 de 18.000.
    ///
    /// NAO se muda aqui ainda por uma razao: aquele teste moveu os DOIS termos
    /// juntos, portanto o `dose_2048` era `ttPv 2048 + cut 2048`. O nosso e'
    /// `ttPv 1024 + cut 2048` -- uma mistura que nenhuma das tres pontas cobriu.
    /// Ha' um teste a correr que mede exactamente isso, com o `ttPv` fixo.
    ///
    /// No outro motor a mesma coisa aconteceu e com a mesma licao. Aos 1548
    /// jogos o desligado parecia ganhar por +10,25; aos 2200 esta' a +7,58
    /// contra +5,69 do 1024 -- 1,9 Elo com margem de 7,5, ou seja EMPATADOS. So'
    /// o 4096 e' claramente mau (-13,27). A conclusao pratica nao muda (fica
    /// desligado la'), mas a razao e' outra: o termo e' neutro naquele motor,
    /// nao prejudicial.
    ///
    /// Duas leituras precipitadas na mesma manha, uma em cada motor.
    int lmr_cut_f     = 1024;
    int lmr_hist_div  = 22000;
    int lmr_pecas_fim = 0;

    /// A PROFUNDIDADE REDUZIDA PARA PODAR. O comentario do ciclo de lances ja'
    /// dizia que "o outro ramo -- o que julga o lance a` profundidade REDUZIDA
    /// -- esta' escrito la' e DESLIGADO". Estava escrito no `ks_1.20260919`
    /// tambem, e nunca chegou aqui. Do binario, `negamax` em `438125`:
    ///
    ///     438125:  mov 0x128(%rbp),%r11d      ; poda_red
    ///     438131:  je 43817f                  ; == 0 -> prof_poda = prof
    ///     438153:  lea 0xe8(%r9,%rsi,1),%rax  ; a MESMA tabela da reducao
    ///     43815b:  mov 0x10(%rbp,%rax,4),%r8d ; lmr[min(prof,63)][min(i,63)]
    ///     43816e:  sar $0xa,%r11d             ; / 1024
    ///     438172:  sub %r11d,%edi             ; prof - r
    ///     43817c:  cmovs %edi,%ecx            ; max(..., 0)
    ///
    /// SO' a futilidade dos tranquilos e o SEE dos tranquilos a usam. O LMP, a
    /// poda pelo historico e o SEE das CAPTURAS ficam com o `prof` cru -- o
    /// binario le' `0x4(%rsp)` nesses tres (`438188`, `4381e3`, `4394b8`).
    /// A ZERO no molde.
    int poda_red      = 0;

    // --- reduzir os maus, ESTENDER os bons ---
    //
    // A reducao nao tem de ser so' para baixo. Um no' que ja' foi importante uma
    // vez -- procurado com janela inteira, marcado ttPv na tabela -- merece MAIS
    // profundidade e nao menos, mesmo aparecendo tarde na lista.
    //
    // Medido no motor de referencia, que faz isto: com so' a base a arvore dele
    // seria 74.038 nos; com o termo do ttPv sozinho vai a 786.304, dez vezes
    // mais. O que a traz de volta e' o termo do CUT NODE, que sozinho a encolhe
    // para 51.616. Juntos dao 224.861. Sao duas forcas opostas e calibradas: uma
    // abre a arvore onde ela merece, a outra fecha-a onde nao merece.
    //
    // Por isso entram AS DUAS ou nenhuma. Levar so' a que abre da' um motor
    // inchado; levar so' a que fecha da' um motor cego. O half2k ja' aprendeu
    // isto na poda, ao fim de sete tentativas com pecas soltas.
    //
    // Os numeros sao NOSSOS e estao para varrer -- a unidade e' milesimos de
    // ply, portanto 1024 e' um ply.
    // A DOSE, corrigida. Eu tinha 2048+1024+768+768 = 4,6 plies de ttPv e 3
    // plies de cut node, escolhidos a olho. Medi -50,79 +/- 48,44 em partidas.
    //
    // O terceiro motor que sabemos usar isto poe o ttPv em **1 ply** e o cut
    // node em **1 ply**, e regista que o ttPv ISOLADO lhes mede -2,6 Elo. Tem-no
    // como inteiro auto-desligavel (0/1/2) de proposito, para o afinador o poder
    // levar de volta a zero sozinho se continuar a prejudicar.
    //
    // Ou seja: peguei numa peca que eles medem ligeiramente negativa, multiplique
    // -a por quatro e meia, e liguei-lhe por cima uma extensao que eles deixam
    // desligada. O -50 era previsivel.
    int lmr_ttpv       = 1024;  // 1 ply, como eles
    int lmr_ttpv_pv    = 0;     // os refinamentos ficam a zero ate' haver varrimento
    int lmr_ttpv_alpha = 0;
    int lmr_ttpv_fundo = 0;
    int lmr_cut_sem_tt = 0;
    // Ate' onde a reducao pode ficar NEGATIVA, ou seja quanto pode ESTENDER.
    // Zero = como estava, so' pode reduzir.
    // Tecto da extensao, em milesimos de ply. 2560 = 2,5 plies, que e' a ordem
    // de grandeza que a referencia usa. 0 = so' reduz, como estava.
    //
    // MEDIDO: o tecto NAO e' o que manda. Com o termo do ttPv ligado a arvore
    // sobe de 60.934 para 250.000-600.000 nos a` profundidade 12, e da' o mesmo
    // com tecto de 1, 2 ou 3 plies -- porque o que a faz crescer e' o `r` ficar
    // negativo em muitos sitios, nao a profundidade extra em cada um.
    int lmr_ext_max    = 0;
    // AMORTECEDOR do lado negativo, em percentagem.
    //
    // 100 = a extensao vale o que a formula diz; 50 = metade; 0 = nao estende.
    //
    // Existe porque o tecto NAO e' o que manda: com tecto de 1, 2 ou 3 plies a
    // arvore da' o mesmo, porque o que a faz crescer e' o `r` ficar negativo em
    // MUITOS SITIOS e nao a profundidade extra em cada um. O tecto limita quanto
    // se estende; isto limita QUANTAS VEZES se chega a estender, que e' a
    // grandeza que realmente controla a arvore.
    int lmr_ext_amort  = 100;

    /// O termo `delta/rootDelta` da reducao, que o `ks_1.20260919` tem e esta
    /// reconstrucao nao tinha. Do binario, `Busca::reducao` em `40ae6f`:
    ///
    ///     40ae6f:  mov 0x12c(%rdi),%esi      ; lmr_delta
    ///     40ae75:  test %esi,%esi / jle      ; <= 0 -> nao faz nada
    ///     40ae84:  vmovd 0x47f20(%rdi),%xmm0 ; delta_raiz
    ///     40ae8c:  sub %ebx,%eax             ; beta - alpha
    ///     40ae8e:  imul %esi,%eax            ; * lmr_delta
    ///     40ae91:  vpmaxsd %xmm1,%xmm0,%xmm2 ; max(delta_raiz, 1)
    ///     40ae9c:  idiv %r8d
    ///     40ae9f:  sub %eax,%ecx             ; r -= ...
    ///
    /// Uma janela larga a` volta deste no' em relacao a` da raiz quer dizer que
    /// a nota ainda anda a saltar por aqui: reduz-se menos. A ZERO no molde do
    /// binario, portanto nao muda nada enquanto nao for ligado -- entra para o
    /// codigo existir e poder ser medido.
    int lmr_delta      = 0;

    // --- re-busca proporcional ---
    // Refazer sempre a` profundidade inteira e' o mais caro dos tres casos
    // possiveis, e faziamo-lo sempre.
    int reb_fundo = 53;
    int reb_raso  = 8;

    // --- ordenacao ---
    int ordem_princ_f = 138;  // peso da historia principal, em 32 avos
    // Peso das continuacoes NA ORDENACAO, em 32 avos. 32 = como estava.
    //
    // MEDIDO: o modulo medio da principal e' 1374 e o das continuacoes
    // 301/234/297. Com os pesos {2,1,1} as continuacoes somam 1133; a principal,
    // escalada pelo `ordem_princ_f` de 138, entra a 5926. A proporcao efectiva
    // e' 1:0,19.
    //
    // A referencia compoe os tranquilos com 2252 na principal e 1126+1093 nas
    // duas continuacoes -- 1:0,985, ou seja as continuacoes juntas valem tanto
    // como a principal. As nossas estao cinco vezes abaixo dessa proporcao.
    //
    // A FORMA transfere-se, o numero nao: o que se copia e' a proporcao, nao os
    // valores deles, porque as nossas tabelas sao outra escala. Para igualar a
    // proporcao com a nossa principal onde esta', este factor daria 167.
    //
    // So' na ordenacao. A poda le o historico por outro caminho (`hist_de`) e
    // fica intacta -- sao duas leituras separadas e mexer nas duas ao mesmo
    // tempo nao diria qual delas mexeu.
    int ordem_cont_f  = 32;
    // Peso das continuacoes NA ORDENACAO, em 32 avos. 32 = como estava.
    //
    // MEDIDO: o modulo medio da principal e' 1374 e o das continuacoes
    // 301/234/297. Com os pesos {2,1,1} as continuacoes somam 1133; a principal,
    // escalada pelo `ordem_princ_f` de 138, entra a 5926. A proporcao efectiva
    // e' 1:0,19.
    //
    // A referencia compoe os tranquilos com 2252 na principal e 1126+1093 nas
    // duas continuacoes -- 1:0,985, ou seja as continuacoes juntas valem tanto
    // como a principal. As nossas estao cinco vezes abaixo dessa proporcao.
    //
    // A FORMA transfere-se, o numero nao: o que se copia e' a proporcao, nao os
    // valores deles, porque as nossas tabelas sao outra escala. Para igualar a
    // proporcao com a nossa principal onde esta', este factor daria 167.
    //
    // So' na ordenacao. A poda le o historico por outro caminho (`hist_de`) e
    // fica intacta -- sao duas leituras separadas e mexer nas duas ao mesmo
    // tempo nao diria qual delas mexeu.
    // (declaracao duplicada removida: o cont.py acrescentou um campo
    //  que esta arvore ja' trazia)
    /// Peso da SEXTA familia de correccao, a do lance de seis plies atras.
    ///
    /// Zero = inerte, e a arvore fica identica ao no'. As outras cinco valem
    /// 203, 109, 109, 121 e 72 sobre um divisor de 2048; um valor na casa dos
    /// 70 poe esta no mesmo peso da mais fraca das que ja' la' estao.
    int corr6_peso    = 0;
    int hist_pc_f     = 2400; // peso da tabela peca-casa, em centesimos
    /// O DESPREZO PELO EMPATE. 0 = desligado (empate vale zero).
    ///
    /// Negativo para o lado que manda na RAIZ -- e' a nos que um empate custa --
    /// e positivo para o outro. Entre repetir e continuar a jogar, a busca passa
    /// a preferir jogar, e so' aceita o empate quando a alternativa e' pior.
    int contempt      = 0;
    /// Com incremento nao ha' final a` bandeira: a repeticao e' resultado
    /// legitimo e fugir-lhe so' piora a posicao.
    /// Desligar o CUCKOO quando nao ha' incremento. 0 = como estava.
    ///
    /// O cuckoo diz "ha' aqui um lance que empata por repeticao" e poe o alpha
    /// nesse valor -- um piso garantido. Sem incremento esse piso e' meio ponto
    /// oferecido: a posicao arrastada acaba a` bandeira e ganha quem tem mais
    /// relogio, portanto o que estava em jogo era o ponto inteiro.
    ///
    /// Com o desprezo activo o piso ja' fica NEGATIVO, o que ja' desencoraja.
    /// Isto e' a versao forte: nem sequer procurar a repeticao.
    int cuckoo_sem_inc  = 0;
    /// Recusar a repeticao so' quando a entrada da tabela e' FRESCA.
    /// 0 = como estava.
    int recusa_fresca   = 0;
    int contempt_sem_inc = 0;
    /// Se ja' estamos a perder, um empate e' bom: nao se lhe foge.
    int contempt_um_lado = 0;
    /// SE SABEMOS CONTRA QUEM JOGAMOS, e' isso que manda. O `UCI_Opponent` e'
    /// padrao e os interfaces mandam-no. Com o Elo do adversario na mao, a
    /// regra do relogio deixa de ser precisa: um empate contra quem e' muito
    /// mais fraco custa meio ponto haja ou nao incremento.
    ///
    /// A ideia vem do lc0, que usa o Elo do adversario para recalibrar a curva
    /// de vitoria/empate/derrota. Aqui a peca e' mais simples: so' decide se o
    /// desprezo liga.
    int our_elo       = 3000;
    int elo_margin    = 150;
    int ordem_xeque_f = 0;
    /// O crivo do SEE para o bonus do xeque, nas unidades do SUBSTRATO
    /// (peao = 208). -75 e' o valor da referencia. 0 = sem crivo.
    int ordem_xeque_see = -75;
    /// A partir de que nota se deita a janela de aspiracao fora. 1000 sao cinco
    /// peoes na nossa escala, e acima disso procurava-se com beta infinito --
    /// uma busca que nao pode falhar alto e portanto nunca colapsa.
    /// Profundidade minima para a IIR. 4 e' o que la' estava cravado.
    int iir_prof    = 4;
    /// Nao aplicar a IIR em nos ALL, como eles fazem. 0 = como estava.
    int iir_sem_all = 0;
    /// O ProbCut PEQUENO: corte pela tabela, sem busca nenhuma. `pcp_margem`
    /// e' o quanto o limite guardado tem de estar acima do beta; `pcp_prof` e'
    /// quantos plies mais rasa a entrada pode ser. 0 = desligado.
    /// O histórico dos PRIMEIROS PLIES, que eles tem e nos nao tinhamos.
    ///
    /// Uma tabela `[ply][lance]` que so' vive perto da raiz e entra na ordem com
    /// peso a dividir pelo ply: `low_f * valor / (1 + ply)`. Existe porque perto
    /// da raiz ha' poucas visitas e o historico geral ainda nao aprendeu nada
    /// sobre ELA -- a tabela grande e' dominada por lances de profundidade, que
    /// sao a esmagadora maioria. E' o sitio onde uma escolha errada custa a
    /// arvore toda por baixo.
    ///
    /// `low_f = 0` desliga.
    /// MEDIDO E REJEITADO. Com peso 16 perde 48 Elo contra nao o ter (700
    /// partidas por ponta, 10+0,1): base +23,79 +/- 15,03 contra low16 -23,86
    /// +/- 15,63. Fica a zero.
    ///
    /// Estava a 8 por omissao entre a implementacao e esta medida, o que e' o
    /// erro que este ficheiro inteiro existe para evitar: um mecanismo entrou no
    /// motor com um valor herdado deles antes de alguem o medir aqui.
    /// AS PECAS EM FALTA, todas por omissao DESLIGADAS.
    ///
    /// A licao que as fez aparecer: pusemos as constantes deles em bloco no
    /// nosso motor e perdemos 45 Elo em 200 partidas. Nao porque as constantes
    /// sejam mas -- porque estao calibradas para uma busca que TEM estas pecas.
    /// O `-4136*prof` da poda pelo historico e' afinado para um historico somado
    /// de cinco fontes; nos temos tres, os modulos sao outros e o limiar aterra
    /// no sitio errado. Estrutura primeiro, constantes depois.
    ///
    /// `pior_adv`: a estatica do adversario piorou (`est > -est_anterior`).
    /// `tt_capt`:  o lance da tabela e' uma captura -- eles usam-no para PERMITIR
    ///             a futilidade inversa (`!ttMove || ttCapture`) e nas margens da
    ///             extensao singular.
    /// `corr_marg`: a magnitude da correccao entra na margem da futilidade.
    int usa_pior_adv  = 0;
    /// A REPETICAO QUE O LADO QUE JOGA AINDA PODE ALCANCAR. 0 = como estava.
    ///
    /// So' viamos repeticoes ja' acontecidas -- anda-se para tras na historia
    /// das chaves ate' bater numa igual. Uma que esteja a UM lance de distancia
    /// nao se ve', e e' essa que poe um chao de empate debaixo do no': quem
    /// esta' a perder pode reclama-la, e quem esta' a ganhar tem de saber que o
    /// adversario tem essa fuga em vez de julgar a linha ganha.
    ///
    /// O `upcoming_repetition` ja' vem compilado no substrato -- as tabelas
    /// cuckoo do algoritmo publicado do van Kervinck -- e a nossa busca nunca o
    /// chamava.
    ///
    /// Aplica-se como CHAO e nao como nota: so' quando o alpha esta' abaixo do
    /// empate. Aplica-lo estando melhor do que empate seria deitar fora uma
    /// posicao ganha, tratando a fuga do adversario como nossa.
    ///
    /// OMISSAO 1, e nao 0 como o remendo original a criou. O remendo poe a peca
    /// desligada para a medir; a medicao ESTA' FEITA -- `+10,34 +/- 5,59 em
    /// 4234 partidas, APROVADO` -- e o binario que jogava traz KS_CUCKOO=1.
    /// A afinacao transporta-se; nao se volta a decidir. `KS_CUCKOO=0` desliga.
    /// O OPTIMISM, a partir da nota da raiz. 0 = desligado.
    ///
    ///     o = otimismo_f * nota_raiz_anterior / (|nota| + 85)
    ///
    /// Positivo para o lado que joga na RAIZ e negativo para o outro, e faz
    /// sentido assim: quem acredita estar melhor e' um lado so'.
    ///
    /// O substrato ja' recebia este numero no `Eval::evaluate` e nos passavamos
    /// zero -- a peca estava la', so' nao lhe davamos nada.
    ///
    /// 114 e' o valor afinado que o binario que jogava trazia (KS_OTIMISMO),
    /// lido do molde das omissoes em .rodata. Transporta-se.
    /// QUANTAS FAMILIAS DE CONTINUACOES se somam. 3 = como estava.
    ///
    /// A referencia soma cinco -- ss-1, ss-2, ss-3, ss-4 e ss-6. Faltavam-nos o
    /// de TRES plies atras e o de SEIS, e eles entram no FIM do CONT_RECUO para
    /// nao mexer no sitio onde o ja' aprendido esta' guardado.
    ///
    /// A tabela reserva sempre cinco planos; com 3 os dois ultimos ficam a zero
    /// e por tocar, portanto a manete nao mexe na memoria.
    /// O HISTORICO DA ESTRUTURA DE PEOES. 0 = desligado.
    ///
    /// Indexado pela chave dos peoes, pela peca e pela casa de chegada. Um
    /// lance bom com esta estrutura de peoes continua bom quando as pecas se
    /// mexem a` volta dela, e nenhuma das outras tabelas sabe isso: a principal
    /// so' conhece de-para, as continuacoes so' conhecem o lance anterior.
    ///
    /// Mesma casa de peso que as continuacoes: 32 = um, 64 = o peso dobrado com
    /// que a referencia o soma. 32 e 8192 sao os valores do binario que jogava
    /// (KS_PEAO_F, KS_PEAO_CH).
    int peao_f        = 32;
    /// Quantas chaves de peoes distintas. TEM de ser potencia de dois: o indice
    /// faz `pawn_key & (peao_chaves - 1)`.
    int peao_chaves   = 8192;
    int cont_n        = 3;
    int otimismo_f    = 114;
    /// Bonus for a quiet move that gives check, in the ordering. 0 = off.
    ///
    /// Why this is the target. Measured with matching counters on both sides,
    /// same position, same network, `go depth 18`, one thread, 256MB, against
    /// a strong reference engine:
    ///
    ///                                        ours   reference
    ///     table probe hit                   47.4%     73.0%
    ///     of those, carrying a move         67.7%     55.8%   <- we win here
    ///     no table move, cut on the 1st     67.2%     75.5%   <- the gap
    ///     cut on the first move (overall)   69.2%     84.1%
    ///
    /// The probe hit rate is NOT a capacity problem: from 64MB to 4GB it goes
    /// from 45.7% to 48.1% and saturates. More than half of our nodes are
    /// positions the search has never seen, which is the wide tree viewed from
    /// the table's side.
    ///
    /// What can be attacked on its own is the 8.3 points of ordering WITHOUT a
    /// table move -- and a check is a signal that needs no learning: it is
    /// worth something on the first visit to any node, which is exactly where
    /// we are poor, since 68% of our nodes have no table move.
    ///
    /// The -75 is in the SUBSTRATE's units (pawn = 208) and therefore does NOT
    /// go through `lim_see`. The scales line up: our main history has a ceiling
    /// of 15000 and enters with weight 1; theirs is about 7200 at weight 2.
    ///
    /// The tree is 35% smaller at the same depth: 1,126,865 -> 736,436 nodes at
    /// depth 18. But measuring the tree has pointed the wrong way five times
    /// out of five in this engine, so it stays at ZERO until games decide.
    /// Drop the threat bucket -- TRIED AND REJECTED. 0 = keep it.
    ///
    /// The idea: a single drawer instead of four, so that samples stop being
    /// split, with the threat information living in `ameaca_f` added to the
    /// score rather than in the drawer. It was measured in games and did not
    /// pass.
    ///
    /// The knob stays, and so does this note -- so that it is not tried again
    /// for looking like a good idea.
    /// The threat term in the ordering. 0 = off.
    ///
    /// Rewards LEAVING a square attacked by a cheaper piece and penalises
    /// ENTERING one, weighted by the value of the piece. Pawn and king are
    /// worth zero, deliberately: nothing is cheaper than a pawn, and a king is
    /// never traded.
    ///
    /// Why this matters more than it looks: 62.2% of our nodes live at plies 1
    /// to 3, where the history tables have few samples and order badly. This
    /// term needs to learn nothing -- it is worth something on the first visit
    /// to any node. It is signal where we currently have none.
    // AVISO DE LEITURA, porque isto ja' me custou uma tarde.
    //
    // Os blocos `///` acima documentam manetes DIFERENTES, encostados uns aos
    // outros, e as quatro declaracoes seguem todas em fila no fim. O texto
    // "TRIED AND REJECTED" que aparece la' em cima e' do `sem_balde` -- de
    // deitar fora o BALDE das ameacas -- e nao do `ameaca_f`. Quem ler de
    // passagem conclui que as ameacas foram rejeitadas, quando a propria nota
    // diz o contrario: a informacao de ameaca sobrevive porque vive aqui.

    /// O termo das ameacas na ordenacao. 20 = o valor de producao.
    ///
    ///     nota += valor_da_peca * ameaca_f * (saiu_de_ameacada - foi_para_ameacada)
    ///
    /// Premeia SAIR de uma casa batida por peca mais barata e castiga ENTRAR
    /// numa. Peao e rei valem zero de proposito: nada e' mais barato do que um
    /// peao, e um rei nunca se troca.
    ///
    /// E' a manete que mais Elo deu a este motor: **+4,70**. A omissao foi
    /// mudada de 0 para 20 a 18-09 e o binario que o bot corria em producao
    /// traz 20. Esta reconstrucao tinha-a a ZERO -- nao por decisao, por
    /// omissao: a manete mudou de nome (`ameaca` no binario, `ameaca_f` aqui)
    /// e a comparacao por nome exacto nao a apanhava.
    ///
    /// O 40 ficou por decidir. Corria a +10,62 +/- 19,17 quando a maquina que
    /// o media morreu, e e' isso -- e nao o mecanismo -- o que os registos
    /// chamam "a retractacao do ameaca_f=40".
    ///
    /// VARRIDO NA ESCALA CORRIGIDA, 24-09-2026: o **20 fica**. O 10 mediu
    /// **+2,95 +/- 5,65** em 4002 partidas a 5+0,05, `Hash=16`, um fio, IC95%
    /// [-2,7; +8,6] -- neutro. Mesmo binario dos dois lados, o 10 por involucro.
    ///
    /// A pergunta era legitima e ficou respondida: o `4a21c2f` mediu a formula
    /// NOVA contra a VELHA, nao mediu o 20 contra outros valores da formula
    /// nova. Com `peso = ameaca_f * PieceValue[pt]` o 20 poe um cavalo a 15.620
    /// e uma dama a 50.760, numa ordenacao onde a historia inteira vai ate'
    /// +-15.000, e a suspeita era que 244 vezes mais forte tivesse passado do
    /// ponto. Nao passou. A nota de 18-09 -- "o mecanismo esta' provado, falta
    /// so' saber se 20 era o sitio certo" -- pode fechar-se.
    ///
    /// O varrimento por nos a profundidade 12 nao teria dado esta resposta: 0 da'
    /// 269.203, 5 da' 313.450, 20 da' 295.733 e 30 da' 417.293, sem gradiente.
    int ameaca_f      = 20;
    /// Deitar fora o BALDE das ameacas na historia de continuacao: uma gaveta
    /// em vez de quatro, para as amostras deixarem de ser divididas. MEDIDO EM
    /// PARTIDAS E REJEITADO. 0 = fica o balde.
    int sem_balde     = 0;
    /// So' se guarda como MELHOR um lance que tenha batido o alpha. 1 = liga.
    ///
    /// O `negamax` escrevia `melhor_lance = m` sempre que a nota subia acima da
    /// melhor ate' ali, mesmo num no' ALL -- onde nada bate o alpha e portanto
    /// nao ha' melhor lance nenhum, so' o menos mau. A `quiescencia` desta mesma
    /// busca ja' fazia o contrario (a atribuicao la' esta' DENTRO do `nota >
    /// alpha`), e e' isso que diz que aqui foi deslize e nao decisao.
    ///
    /// Custa duas vezes, e as duas na tabela:
    ///
    ///  1. guarda-se como dica de ordenacao um lance que ninguem mostrou ser
    ///     bom. Quem o ler a seguir tenta-o primeiro e ele nao corta.
    ///  2. pior: por nao ser `Move::none()`, a guarda de preservacao em
    ///     `tt.cpp` -- que so' repoe o lance antigo quando o novo e' nulo --
    ///     nunca dispara nos nos ALL. O lance bom de uma busca anterior, talvez
    ///     mais funda, e' apagado e substituido por este.
    ///
    /// Bate certo com o que a auditoria de 22-09 mediu contra o SF19 com a
    /// NOSSA rede: temos lance na tabela MAIS vezes do que eles (34-70% contra
    /// 22-59%) e ele rende METADE (30-43% contra 52-63%). Sao os dois lados do
    /// mesmo defeito -- guardamos mais lances porque guardamos tambem os que
    /// nao valem nada, e cada um desses ocupa o lugar de um que valia.
    ///
    /// A `melhor_nota` continua a subir na mesma: o valor de retorno e o limite
    /// que vai para a tabela nao mudam. O que muda e' so' QUE lance a acompanha.
    /// A raiz nao depende disto -- usa `melhor_raiz`, atribuido dentro do
    /// `nota > alpha`.
    ///
    /// MEDIDO EM PARTIDAS E REJEITADO, 24-09-2026: **-26,02 +/- 14,59** em 602
    /// partidas a 5+0,05, `Hash=16`, um fio, IC95% [-40,6; -11,4]. Mesmo binario
    /// dos dois lados, a manete por involucro. FICA A ZERO.
    ///
    /// O que isto ensina e' mais do que "nao da' Elo". O raciocinio acima estava
    /// certo na mecanica -- a arvore encolheu 23,6% a profundidade 12 (295.733
    /// -> 226.040) -- e errado na conclusao. Guardar o lance de melhor tentativa
    /// num no' ALL VALE, neste motor, apesar de nenhum deles bater o alpha: e'
    /// o lance mais promissor que aquela posicao tem, e quando ela voltar com
    /// uma janela mais alta e' por ele que se deve comecar.
    ///
    /// O Stockfish e o Triumviratus nao o guardam -- e podem nao o guardar
    /// porque a preservacao do lance antigo lhes tapa o buraco. Aqui a
    /// preservacao so' actua quando a via escolhida ja' tem ESTA chave; quando
    /// a via e' de outra posicao, guardar `Move::none()` deixa a entrada nova
    /// sem lance nenhum, e a visita seguinte fica sem dica.
    ///
    /// Terceira vez que a contagem de nos aponta ao contrario do Elo. Ver
    /// [[metricas-que-mentem]]: -23,6% de arvore e -26 Elo na mesma mudanca.
    int lance_so_alpha = 0;
    /// A quiescencia grava a avaliacao no momento em que a calcula. 1 = liga.
    ///
    /// Ela ja' a gravava -- mas so' no fim, junto com o resto da entrada. E a
    /// saida mais frequente da quiescencia nao passa la': e' o `chao >= beta`,
    /// o ficar quieto, que devolve mal a estatica e' conhecida. Em todos esses
    /// nos corre-se a rede e deita-se o numero fora; a visita seguinte a` mesma
    /// posicao paga-a outra vez.
    ///
    /// A busca principal ja' tem este remendo e o comentario dela explica-o:
    /// gravar no momento do calculo faz o trabalho sobreviver a`s maneiras de
    /// sair do no' antes do guardar la' de baixo. A quiescencia ficou de fora, e
    /// e' nela que a perda e' maior -- MEDIDO a profundidade 14: a busca poupa
    /// 37-47% das avaliacoes pela tabela e a quiescencia so' 8-10%, com 32.867
    /// calculos em 145.040 nos.
    ///
    /// Isto pesa onde dói: a rede e' 53% dos ciclos deste motor (`apply_combined`
    /// sozinho e' 31,9%). Uma avaliacao poupada vale mais aqui do que em
    /// qualquer outro sitio da arvore.
    int qs_guarda_aval = 0;
    /// Os tres primeiros canais da correccao saem das chaves que a `Position`
    /// ja' mantem, em vez de uma passagem por todas as pecas. 1 = liga.
    ///
    /// O `indices()` guarda em cache os canais 0-2, que so' dependem da
    /// posicao. Quando a cache falha, recalcula-os varrendo os bitboards peca a
    /// peca -- perto de trinta iteracoes, cada uma com um XOR contra a `ALEA`.
    /// MEDIDO no perfil: esses lacos sao **43% do `indices()`**, que por sua vez
    /// e' 2,52% dos ciclos do motor -- **1,08% do total**, so' a recalcular uma
    /// coisa que o substrato ja' tem pronta.
    ///
    /// A `Position` actualiza `pawnKey` e `nonPawnKey[cor]` dentro do `do_move`,
    /// incrementalmente. Le-las e' O(1). O Stockfish faz exactamente isto e
    /// nunca varre pecas para indexar a correccao.
    ///
    /// **As classes de equivalencia ficam as MESMAS.** O `nonPawnKey` inclui o
    /// rei e o nosso `FORCA[]` nao, por isso tira-se o rei com um XOR -- tambem
    /// O(1). O que muda e' so' a funcao de dispersao: a chave passa a vir da
    /// tabela `Zobrist::psq` em vez da nossa `ALEA`, portanto as MESMAS posicoes
    /// continuam a partilhar indice, mas o indice concreto e' outro.
    ///
    /// Isso tem uma consequencia a ter em conta ao medir: **a arvore muda**, nao
    /// por a busca decidir diferente mas por os baldes da correccao serem
    /// reatribuidos. Nao se pode usar "nos identicos" como controlo aqui. O
    /// controlo certo NAO e' "nos identicos" -- e a medida por no' tambem nao
    /// serve, porque a composicao da arvore muda com os baldes: na kiwipete a
    /// profundidade 14 deu -5,4% de instrucoes por no', e no conjunto de quatro
    /// posicoes a profundidade 13 deu +9,6%. Mede-se a FUNCAO, nao o motor.
    ///
    /// MEDIDO assim, `perf record` na kiwipete a profundidade 14: a `indices()`
    /// passa de **2,15% para 1,19%** dos ciclos. O mecanismo esta' certo e o
    /// custo dela fica a metade.
    ///
    /// **Mas o premio e' 0,96% dos ciclos**, ou seja ~1% de nos por segundo, que
    /// pela relacao habitual vale **~+0,2 Elo** -- abaixo do que um SPRT nosso
    /// distingue. Fica a ZERO ate' haver com que o juntar: varias pecas de
    /// velocidade medidas assim, ligadas de uma vez, ja' dao um efeito mensuravel.
    /// Liga-la sozinha seria mudar o motor sem medida que o sustente.
    int indices_rapido = 0;
    /// Pede-se o balde da tabela do FILHO antes de jogar o lance, e nao depois.
    /// 1 = como o binario de producao fazia.
    ///
    /// O `ks_1.20260919` fazia, na `quiescencia` (`4213d4`) e no `negamax`
    /// (`43839c`), ANTES do `do_move`:
    ///
    ///     call prefetch_key(m)       ; a chave que o lance vai dar, sem o jogar
    ///     and  0x8(%r12),%rax        ; & mascara
    ///     lea  (%rax,%rax,8),%r11    ; * 9
    ///     prefetcht0 (%r9,%r11,8)    ; * 8 -> o balde de 72 bytes
    ///
    /// Assim a ida a` memoria corre EM PARALELO com o `do_move` inteiro -- as
    /// ameacas sujas, a pilha do acumulador, as chaves. A reconstrucao perdeu
    /// isto de duas maneiras:
    ///
    ///  - na `quiescencia` nao havia prefetch nenhum;
    ///  - no `negamax` havia, mas DEPOIS do `do_move`, com `pos.key()`: o balde
    ///    certo, pedido quando ja' nao ha' trabalho nenhum para esconder a
    ///    latencia -- a sondagem do filho vem logo a seguir.
    ///
    /// O comentario que la' estava ja' dizia o que isto vale: "essa ida era um
    /// quinto da busca toda". Estava certo e o codigo nao lhe obedecia.
    ///
    /// As chaves batem: `Position::key()` e' `adjust_key50(st->key)`, e o
    /// `prefetch_key(m)` devolve `adjust_key50<true>` calculado antes do lance,
    /// que e' o mesmo numero. Nos lances especiais (roque, en passant, promocao,
    /// e o lance a seguir a um avanco duplo) a chave do `prefetch_key` sai
    /// aproximada e o pedido vai ao balde errado -- sem mal nenhum, porque um
    /// prefetch e' so' um pedido.
    ///
    /// NAO muda a busca: um prefetch nao altera um unico valor. A arvore tem de
    /// sair IDENTICA ao no' -- e' esse o controlo, e passou: 295.733 nos a
    /// profundidade 12, e 1.785.977 a 15, com a manete ligada e desligada.
    ///
    /// E ficou mais eficaz com o balde de 64 bytes: agora o balde cabe numa
    /// linha, e o `prefetcht0` traz o balde INTEIRO. Com 72 bytes trazia so'
    /// parte dele.
    ///
    /// MEDIDO, oito voltas com a ordem rodada, arvores identicas, sobre o balde
    /// de 64 bytes e as paginas grandes: mediana **3642 -> 3544 ms, -2,7%**,
    /// mais rapido em 6 voltas de 8. E' o incremento mais pequeno dos tres e o
    /// mais ruidoso; duas medicoes anteriores, so' com esta peca, deram -1,4%
    /// de media e -3,0% de mediana.
    ///
    /// MEDIDO EM PARTIDAS, junto com o balde de 64 bytes e as paginas grandes,
    /// contra `7c25e1c`: **+19,14 +/- 7,66 Elo** em 2108 partidas a 5+0,05,
    /// `Hash=16`, um fio, H1 aceite. A parte desta peca, pela velocidade: -2,7%.
    int prefetch_antes = 1;
    int usa_cuckoo    = 1;
    /// Futilidade inversa SO' quando nao ha' lance na tabela, ou quando o que
    /// la' esta' e' uma captura. 1 = o valor de producao.
    ///
    /// Sem isto a futilidade inversa corta tambem onde a tabela ja' guardou um
    /// tranquilo que resultou -- justamente onde ha' informacao a dizer que o
    /// no' merece ser visto.
    int usa_tt_capt   = 1;
    /// MEDIDO MORTO em 2026-09-13: nao muda um unico no'.
    ///
    /// Testado com divisor 198435 (o deles), 50000, 20000, 8000 e 1000, em tres
    /// posicoes -- abertura, meio-jogo e final. As contagens sao identicas ao
    /// byte em todas. Nao e' o divisor estar mal escolhido: o `usa_pior_adv`,
    /// que esta' na linha logo acima e no MESMO bloco, muda a arvore em 27%,
    /// portanto o bloco corre. O termo e' que da' sempre zero.
    ///
    /// Nao se afina um numero que nao move nada. Antes de lhe mexer outra vez,
    /// perceber porque e' que `corrigida(pos, estatica, ply) - estatica` e' nulo
    /// AQUI quando a tabela da correccao esta' alocada e e' escrita.
    /// MEDIDO MORTO em 2026-09-13: nao muda um unico no'.
    ///
    /// Testado com divisor 198435 (o deles), 50000, 20000, 8000 e 1000, em tres
    /// posicoes -- abertura, meio-jogo e final. As contagens sao identicas ao
    /// byte em todas. Nao e' o divisor estar mal escolhido: o `usa_pior_adv`,
    /// que esta' na linha logo acima e no MESMO bloco, muda a arvore em 27%,
    /// portanto o bloco corre. O termo e' que da' sempre zero.
    ///
    /// Nao se afina um numero que nao move nada. Antes de lhe mexer outra vez,
    /// perceber porque e' que `corrigida(pos, estatica, ply) - estatica` e' nulo
    /// AQUI quando a tabela da correccao esta' alocada e e' escrita.
    int corr_marg_div = 0;      // 0 = nao usar; deles: 198435
    /// O termo do `opponentWorsening`: desconta da margem da futilidade inversa
    /// quando a estatica do adversario acabou de piorar.
    ///
    /// **ESTE VALOR E' DELES, E TEMOS O NOSSO VARRIDO.** O `doc/CONSTANTES.md`
    /// regista `RfpAdv | 290 | varrido; outro usa 335`. Ficou aqui o 335 porque
    /// a peca foi escrita a partir do codigo deles e ninguem foi ver a nossa
    /// propria folha.
    ///
    /// E nao tinha interruptor -- `KS_ADV_F` nao existia -- portanto nao se
    /// podia varrer sem recompilar. Isso foi apanhado pela assinatura do ramo
    /// morto: 290 e 335 davam arvores IDENTICAS ao no', porque as duas corridas
    /// usavam 335. Com o interruptor, 979.434 desligado, 1.216.521 a 290 e
    /// 1.143.797 a 335.
    ///
    /// So' age com `usa_pior_adv = 1`, que esta' a zero.
    int rfp_adv_f     = 335;    // termo do `opponentWorsening`, /1024
    int low_f       = 0;
    int low_bonus   = 712;
    int pcp_margem  = 0;
    int pcp_prof    = 4;
    int asp_tecto   = 1000;
    int asp_delta     = 25;
    int asp_prof      = 4;

    /// O ciclo de aspiracao ALTERNATIVO que o `ks_1.20260919` traz. Nao e' uma
    /// manete de um valor: liga tres coisas ao mesmo tempo, e o binario le-a
    /// em tres sitios do `arranca` (`4400ba`, `4400d0`, `4401b3`).
    ///
    ///  1. a profundidade dada a` `negamax` passa a `max(prof - falhas, 1)`
    ///     (`440153: sub %r13d,%edx` / `440170: cmove %r12d,%edx`);
    ///  2. ao falhar em baixo, `beta = alpha` em vez de `(alpha+beta)/2`
    ///     (`4400e3: mov %ebx,%r14d`);
    ///  3. o delta cresce um TERCO em vez de metade
    ///     (`4400fc: imul $0x55555556` = 2^32/3).
    ///
    /// O contador de falhas altas zera a cada falha baixa (`4400f3`) e sobe a
    /// cada falha alta (`4401c0`). A ZERO no molde do binario.
    ///
    /// MEDIDO, 24-09-2026: **neutro**, e fica a zero. -1,74 +/- 6,21 em 3202
    /// partidas a 5+0,05, `Hash=16`, um fio, IC95% [-8,0; +4,5].
    /// Foi medido como UM desenho e nao como tres manetes -- as tres pecas
    /// ligam juntas, e separa-las seria repetir o erro que a nota do `tt.cpp`
    /// regista sobre o par `KS_TT_LANCE`/`KS_TT_PROF`.
    ///
    /// Vale a pena guardar o contraste: a profundidade 12 e em quatro posicoes
    /// isto encolhe a arvore **21,9%** (295.733 -> 230.837), e em partidas nao
    /// vale nada. Terceira vez no mesmo dia que a arvore e o Elo discordam.
    int asp_sf        = 0;
    // Recusar a repeticao so' quando se esta' mesmo a ganhar, e aceitar um lance
    // ate' esta margem pior para a evitar.
    int recusa_limiar = 300;
    int recusa_margem = 20;
    int capt_bar_div  = 60;   // corte captura boa/ma, pelo MERITO do lance
    int capt_hist_div = 256;
    // Um limite superior quer dizer que naquele no' todos os lances falharam em
    // baixo: o guardado e' o menos mau. Vai primeiro 21.538 vezes e corta 13,8%,
    // contra 66,8% de um limite inferior.
    /// ORFAO, e e' um RESTO DE NOME. Esta ideia vive hoje em `tt_sup_nota`, que
    /// e' lida e tem interruptor (`KS_TT_SUP`). Este campo nao e' lido em lado
    /// nenhum -- ficou do baptismo anterior.
    ///
    /// Mas nao se apaga ainda, porque guarda uma coisa que o outro nao guarda:
    /// **o valor proposto**. O `tt_sup_nota` esta' em 1.000.000, que a nota dele
    /// diz ser "como estava (topo, sempre)", e este tem 550.000 -- a dose que a
    /// medicao acima justifica e que ninguem chegou a experimentar.
    int tt_fraco_pont = 550000;

    // --- quiescencia ---
    /// ORFAO. Nao e' lido. Era a margem para saltar uma captura que nem
    /// ganhando tudo chega perto do alpha -- o `QsFutility` do outro motor, que
    /// la' foi MEDIDO CINCO VEZES e saiu negativo das cinco. Aqui nunca chegou a
    /// ser ligado, e a nota do outro motor explica porque nao vale a pena: em
    /// cima de um filtro por troca que ja' corta o que perde material, esta
    /// margem so' remove as capturas sas que a quiescencia existe para ver.
    int qs_margem   = 100;
    /// ORFAO. Nao e' lido. Limitaria quantas capturas se veem por no' na
    /// quiescencia. Sem medicao nenhuma associada e sem codigo.
    int travao_qs_n = 3;

    /// ORFAO. Nao e' lido. Estender um xeque conforme a avaliacao estatica, em
    /// vez de sempre ou nunca. Sem medicao nenhuma associada e sem codigo.
    int ext_xeque_aval = 75;

    // --- gestao de tempo ---
    // 7 e nao 20: exigir que a iteracao seguinte caiba INTEIRA fazia parar a
    // 62% do orcamento e gastar 3,7% do relogio. MAS isto esta' medido a 10+0,1
    // (+17,5 +/- 15,9) e NAO a 120+1, onde o sinal que temos e' negativo com
    // poucas partidas. Ate' esse fechar, e' um numero em prova.
    /// OS GRAMPOS DO ELASTICO, agora em parametro. Estavam CRAVADOS.
    ///
    /// Duas coisas descobertas a fazer a conta, e as duas estao na fonte de
    /// the 1.5 clamp on `instab` was UNREACHABLE by construction -- it
    /// vale 0,9 + mudancas/(div/10 * d) e o `mudancas` sobe no maximo uma vez
    /// por iteracao, logo com divisor 2,0 o tecto real e' 1,4. Quem prende e' o
    /// DIVISOR. E o elastico e' cortado pelo `duro`, portanto levantar um sem o
    /// outro nao da' nada -- foi o erro do primeiro teste do tecto, que deu zero
    /// exacto em 6018 partidas.
    ///
    /// O VARRIMENTO DO tm_escala_max FECHOU A 15-09, tudo a 20+0,2:
    ///     2,45x -> 5,000x   +17,76 +/-  7,12   2076 partidas  LLR 2,95 ACEITE
    ///     5,00x vs 4,000x    +0,08 +/-  4,51   4618 partidas  igual
    ///     5,00x vs 6,674x    -3,55 +/-  3,69   6850 partidas  REJEITADO
    /// Ha' um planalto de 4,0 a 5,0. Os +17,76 vieram de SAIR do 2,45.
    /// O ESFORCO: que fraccao da arvore foi para o melhor lance.
    ///
    /// E' o QUARTO factor do elastico e o KestrelStrike nao o tinha; o half2k
    /// tem-no com a mesma forma, `(base - fraccao) * f`.
    ///
    /// Se o melhor lance levou quase toda a arvore, a posicao esta' decidida e
    /// gasta-se menos; se levou pouco, ha' concorrencia entre lances e vale a
    /// pena gastar mais. E' o sinal mais directo de "esta posicao ainda esta'
    /// em aberto" que existe, e nos decidiamos sem ele.
    ///
    /// LIGADO POR OMISSAO desde 15-09: nasceu a zero de proposito, para o
    /// `base` continuar a ser o motor que jogava enquanto se media. Ja' se
    /// mediu -- e' este factor que faltava, e sem ele o resto dos grampos nao
    /// paga.
    int tm_esf_base   = 140;   // x100
    int tm_esf_f      = 155;   // x100
    int tm_queda_max  = 250;   // x100
    int tm_instab_div = 9;     // x10
    int tm_instab_max = 200;   // x100
    int tm_escala_max = 5000;  // x1000
    /// O chao do elastico. 650 = 0,65, que e' o numero que estava CRAVADO no
    /// clamp -- it is not new tuning, it is the original value promoted
    /// a manete.
    int tm_escala_min = 650;   // x1000
    /// A CURVA DO HORIZONTE. 1 = ligada, como no binario que jogava.
    ///
    /// O `tm_bolo_n = 20` diz "faltam vinte lances" SEMPRE. Medido nas NOSSAS
    /// 2224 partidas, no primeiro lance faltam SESSENTA E CINCO. E' dai' que
    /// vinha gastar-se quase o dobro do que se devia na abertura.
    /// QUAL PERCENTIL DA DURACAO. 0=p50 1=p60 2=p70 3=p75 4=p80.
    ///
    /// Orcar pela mediana e' orcar para METADE das partidas durarem mais do que
    /// o orcamento previu -- e sao essas que acabam a zero. Medido nas nossas
    /// 2224 partidas, no ply 40 com 480s no relogio:
    ///
    ///     p50  45 lances  10,7s   rebentam 50% das partidas
    ///     p70  58 lances   8,3s   rebentam 30%
    ///     p75  62 lances   7,7s   rebentam 25%
    ///     p80  67 lances   7,2s   rebentam 20%
    ///
    /// Nao gasta menos no total: distribui por um horizonte certo para tres
    /// partidas em quatro em vez de uma em duas. 0 e' o que foi validado.
    int tm_curva_pct  = 0;
    /// O HORIZONTE CRESCE depois de a estimativa ser desmentida, em centesimos
    /// de ply por ply de excesso. 0 = desligado. 100 = um por um.
    int tm_cresce     = 0;
    /// O TECTO DURO SEM INCREMENTO, em percentagem do relogio. 0 = desligado.
    ///
    /// O nosso tecto e' um MULTIPLO do optimo, e um multiplo do optimo nao e'
    /// tecto nenhum: se o optimo sobe, o tecto sobe com ele. Com incremento
    /// tudo bem; sem incremento cada milissegundo gasto foi-se para sempre.
    ///
    /// MEDIDO na PDaYgQnB (600+0, perdida a` bandeira ao lance 163): no lance
    /// 12, com 501s no relogio, o optimo era 24,9s e o tecto 137s. O motor
    /// gastou 73s e passou por baixo a` vontade. Com 10% do relogio o tecto era
    /// 50s e aquele lance nao tinha existido.
    ///
    /// A regra vem do CODA, que e' GPL-3 -- verificado antes de a usar.
    int tm_sem_inc_tecto = 0;   // % do relogio; 10 = a regra do Coda
    int tm_curva      = 1;
    int tm_curva_min  = 8;
    int tm_curva_f    = 100;   // x100
    // O ELASTICO CONTRA O RELOGIO DO ADVERSARIO NAO EXISTE AQUI, E E' DE
    // PROPOSITO. Foi implementado e MEDIDO: -14,29 +/- 12,22, LLR -1,08 em 905
    // partidas. Perde Elo. Fica a nota para nao voltar a ser tentado por
    // parecer boa ideia -- e parece.
    /// O CHAO DO ORCAMENTO, em milissegundos.
    ///
    /// Um relogio muito baixo ainda deve comprar um lance que foi OLHADO, e nao
    /// um que foi adivinhado. Sem isto, se a sobrecarga for maior do que o
    /// orcamento por lance, o `optimo` fica em 1 ms e o motor responde na
    /// profundidade tres -- que e' pior do que o pequeno risco de passar alguns
    /// milissegundos.
    ///
    /// Acontece por conta certa e nao por acidente: a 5+0,05 a curva do
    /// horizonte da' ~47 lances, portanto ~127 ms por lance. Uma sobrecarga de
    /// 200 ms -- que e' o valor de um bot na internet -- nao cabe la' dentro.
    ///
    /// O chao e' ELE PROPRIO limitado pelo que resta no relogio, por isso nunca
    /// pode fazer o motor gastar o que nao tem. Mesma forma do `TmFloorMs` do
    /// half2k, que tambem tem 10.
    int tm_chao_ms   = 10;
    int tm_estim     = 7;
    /// QUANTOS LANCES SE ASSUME QUE FALTAM, para repartir o relogio.
    ///
    /// **A medicao mais forte deste projecto, e quase se perdeu com a maquina
    /// que a estava a fazer.** Tres valores em paralelo, 10+0, 8.746 partidas
    /// por motor:
    ///
    ///     n=20    +18,49 +/- 3,97   nElo +33,95
    ///     n=30     -2,30 +/- 3,96   nElo  -4,24
    ///     n=14    -16,18 +/- 4,03   nElo -29,27
    ///
    /// O 20 subiu de +16,6 +/- 6,4 para +18,49 +/- 3,97 ao dobrar as partidas
    /// e a barra fechou para menos de metade -- ou seja, nao foi sorte a
    /// desfazer-se, foi um efeito real a ganhar definicao. O optimo e'
    /// INTERIOR: 30 e 14 sao ambos piores, portanto nao ha' para onde empurrar.
    ///
    /// A corrida morreu a 19-09 as 07:47, na partida 13.126 de 18.000, quando a
    /// maquina se foi abaixo. As 4.874 que faltavam nao mudariam nada -- com
    /// esta barra o veredicto ja' estava fechado.
    ///
    /// POR CONFIRMAR A 60+0. Fechado esta' a 10+0, e so' la'. A sobrecarga
    /// fixa de 30 ms por lance pesa de outra maneira quando o relogio e' seis
    /// vezes maior: a 10+0 ela e' uma fatia visivel de cada orcamento, a 60+0
    /// quase desaparece -- e e' o orcamento que o `n` reparte. Nao ha' razao
    /// para o optimo ficar no mesmo sitio nas duas escalas, e nao foi medido.
    ///
    /// Nao confundir com o `tm_curva_pct` la' em cima: aquilo discute substituir
    /// este numero plano por uma curva, e a nota de la' -- "no primeiro lance
    /// faltam sessenta e cinco" -- e' o argumento PARA a curva, nao contra o 20.
    /// Enquanto o horizonte for plano, 20 e' o sitio.
    int tm_bolo_n    = 20;
    /// O tecto do tempo por lance, em decimos do optimo.
    ///
    ///     duro  = min(max(tecto, optimo), seguro)
    ///     tecto = min(80% do relogio, optimo * tm_tecto_x10/10)
    ///
    /// ARRUMADO EM 2026-09-14: mexer aqui NAO paga, e sabe-se porque'.
    ///
    ///     10+0,1   +1,32 +/- 2,46   17.880 partidas
    ///     20+0,2   -0,00 +/- 4,06    6.240 partidas (1544 V contra 1544 D)
    ///
    /// Duas cadencias, quase 24 mil partidas, zero. E a razao nao e'
    /// estatistica, e' aritmetica: a busca NUNCA CHEGA AO TECTO. O que a
    /// estica alem do optimo e' o elastico
    ///
    ///     mole = min(mole_base * queda * estab * instab, duro)
    ///
    /// e os tres factores estao grampeados em 1,75 / 1,00 / 1,50. O produto
    /// maximo seria 2,625 -- mas o `instab` vale `0.9 + mudancas/(2d)` e o
    /// `mudancas` sobe no maximo UMA vez por iteracao, logo `mudancas <= d` e o
    /// tecto real dele e' 1,4. O maximo mesmo e' 1,75 x 1,00 x 1,40 = 2,45, e
    /// so' se a nota cair 37,5 cp E o melhor lance mudar em TODAS as iteracoes.
    ///
    /// Comparar 2,0x com 6,9x foi comparar uma porta que as vezes morde com
    /// duas que nao existem. Quem prende e' o elastico, nao a porta -- e e' nos
    /// grampos dele que se mexe (`tm_queda_max`, `tm_instab_div`,
    /// `tm_instab_max`).
    ///
    /// A referencia da' 6,873 aqui, mas o `optScale` dela e' menor que o nosso
    /// (~3,3% do tempo util contra 5%): eles pensam pouco por omissao e muito
    /// quando e' preciso, nos pensamos medio sempre. A forma que falta esta' no
    /// optimo e nos grampos, nao no tecto.
    /// O tecto do tempo por lance, em decimos do optimo.
    ///
    ///     duro  = min(max(tecto, optimo), seguro)
    ///     tecto = min(80% do relogio, optimo * tm_tecto_x10/10)
    ///
    /// ARRUMADO EM 2026-09-14: mexer aqui NAO paga, e sabe-se porque'.
    ///
    ///     10+0,1   +1,32 +/- 2,46   17.880 partidas
    ///     20+0,2   -0,00 +/- 4,06    6.240 partidas (1544 V contra 1544 D)
    ///
    /// Duas cadencias, quase 24 mil partidas, zero. E a razao nao e'
    /// estatistica, e' aritmetica: a busca NUNCA CHEGA AO TECTO. O que a
    /// estica alem do optimo e' o elastico
    ///
    ///     mole = min(mole_base * queda * estab * instab, duro)
    ///
    /// e os tres factores estao grampeados em 1,75 / 1,00 / 1,50. O produto
    /// maximo seria 2,625 -- mas o `instab` vale `0.9 + mudancas/(2d)` e o
    /// `mudancas` sobe no maximo UMA vez por iteracao, logo `mudancas <= d` e o
    /// tecto real dele e' 1,4. O maximo mesmo e' 1,75 x 1,00 x 1,40 = 2,45, e
    /// so' se a nota cair 37,5 cp E o melhor lance mudar em TODAS as iteracoes.
    ///
    /// Comparar 2,0x com 6,9x foi comparar uma porta que as vezes morde com
    /// duas que nao existem. Quem prende e' o elastico, nao a porta -- e e' nos
    /// grampos dele que se mexe (`tm_queda_max`, `tm_instab_div`,
    /// `tm_instab_max`).
    ///
    /// A referencia da' 6,873 aqui, mas o `optScale` dela e' menor que o nosso
    /// (~3,3% do tempo util contra 5%): eles pensam pouco por omissao e muito
    /// quando e' preciso, nos pensamos medio sempre. A forma que falta esta' no
    /// optimo e nos grampos, nao no tecto.
    int tm_tecto_x10 = 55;

    /// O RELOGIO DO ADVERSARIO. Nao existia nesta reconstrucao. Do binario,
    /// `arranca` em `4461bc`, logo a seguir ao ponto onde os dois ramos do
    /// `tm_curva` se juntam (`4463fd: jmp 4461b5`), portanto vale nos dois:
    ///
    ///     4461bc:  mov 0x22c(%r8),%r12d   ; tm_adv_f; se 0 -> nada
    ///     4461dd:  mov (%r15,%rax,8),%rsi ; tempo[1 - nos]
    ///     4461e4:  jle 446262             ; <= 0 -> nada
    ///     446232:  vdivsd                 ; razao = tempo[nos]/tempo[eles]
    ///     44623c:  vsubsd 1.0             ; razao - 1
    ///     446240:  vmulsd (tm_adv_f/100)
    ///     446244:  vaddsd 1.0
    ///     446248:  vmaxsd (tm_adv_min/100)
    ///     44624c:  vminsd (tm_adv_max/100)
    ///     446251:  vmulsd orcamento
    ///
    /// As constantes 1.0 e 100.0 estao em `0x608ef0` e `0x608fd0`.
    ///
    /// O `tm_adv_f` vale ZERO no molde, portanto esta desligado -- mas o minimo
    /// e o maximo estao em 70 e 140, que nao sao valores de quem nunca mediu
    /// isto. Vem da cauda do molde em `0x609eb0` (e NAO de `0x609c80`, que e' a
    /// armadilha em que este trabalho ja' caiu uma vez: e' dali que sai o
    /// `tm_tecto = 55` acima, que confirma a leitura).
    int tm_adv_f     = 0;
    int tm_adv_min   = 70;
    int tm_adv_max   = 140;
};

struct Limites {
    std::int64_t tempo[2]{}, inc[2]{}, movetime = 0;
    int          profundidade = 0;
    std::int64_t nos          = 0;
    bool         infinito     = false;
};

class Busca {
   public:
    /// Ms retidos por lance para o lance chegar ao arbitro. 30 e' o que basta
    /// num duelo local por canos; um bot na internet paga a viagem e quer mais.
    std::int64_t sobrecarga_ms = 30;
    /// O limite de nos do `go nodes N`. Estava a ser LIDO do comando e nunca
    /// imposto: `lim.nos > 0` desligava o relogio e mais nada, portanto
    /// `go nodes 3000000` procurava ate' ao fim do mundo -- medido, 126 milhoes
    /// de nos e 526 segundos antes de eu o interromper. Sem isto nao ha' busca a
    /// nos fixos, que e' a maneira de comparar dois motores sem o relogio pelo
    /// meio.
    std::uint64_t nos_limite = 0;
    void arranca(Position& pos, const Limites& lim, Avaliador& av);

    /// SOU UM AJUDANTE DO LAZY SMP?
    ///
    /// Um ajudante corre a mesma busca, na mesma posicao, partilhando a tabela
    /// de transposicao e as tres familias de historico. Nao e' que ele procure
    /// noutro sitio -- e' que chega a`s coisas por outra ordem, enche a tabela
    /// mais depressa, e a principal encontra la' o trabalho ja' feito.
    ///
    /// O que ele NAO faz: escrever para a saida (havia um `bestmove` por fio),
    /// e gerir o relogio (quem decide quando parar e' a principal, e ela
    /// escreve em `parar`, que todos leem).
    bool ajudante = false;
    /// Quantos fios esta busca usa ao todo, a principal incluida.
    int  n_fios = 1;
    /// Cria (ou destroi) os ajudantes e liga-lhes a tabela, o historico e a
    /// rede desta busca. Chamar quando o `Threads` muda, nao a cada lance.
    void prepara_fios(int n, Avaliador& av_dono);
    /// A ULTIMA PROFUNDIDADE QUE ESTA BUSCA COMPLETOU.
    ///
    /// Serve para pesar a votacao. Um ajudante que o relogio apanhou na 12 nao
    /// pode valer o mesmo que o principal na 17 -- e, pior, pode derruba-lo.
    int ultima_prof = 0;

    /// Os nos de TODOS os fios.
    ///
    /// A principal, a quatro fios, anda MENOS nos do que andaria sozinha --
    /// encontra na tabela o que os outros ja' fizeram. Anunciar so' os dela
    /// dava um motor que parece abrandar quando se lhe dao fios, e um `nps`
    /// que nao e' o trabalho que a maquina esta' mesmo a fazer.
    std::uint64_t nos_totais() const {
        std::uint64_t t = nos;
        for (const auto& b : ajudantes)
            t += b->nos;
        return t;
    }
    /// A saida desta busca. No ajudante e' um sorvedouro: um `ostream` sem
    /// buffer deita fora tudo o que la' se escreve. Proprio de cada busca e nao
    /// partilhado, senao os fios disputavam os bits de estado do mesmo stream.
    std::ostream  nulo{nullptr};
    /// Com a busca numa thread propria, a thread dos comandos tambem escreve --
    /// o `readyok` pode chegar a meio de uma busca. Sem tranca, sai a meio de
    /// uma linha `info ... pv ...` e parte o protocolo. A `Linha` segura a
    /// tranca durante a EXPRESSAO inteira `saida() << ... << std::endl`, e por
    /// isso nenhum dos sitios que escrevem teve de mudar. O ajudante escreve
    /// para o sorvedouro e nao tranca nada.
    Linha saida() {
        if (ajudante)
            return Linha(nulo, nullptr);
        return Linha(std::cout, &trinco_saida());
    }
    void limpa();
    void nova_partida();

    Parametros         p;

    // Os ajudantes e os fios que os correm. Vivem aqui entre buscas para nao
    // se pagar a construcao -- e sobretudo as caches de acumuladores de cada
    // um -- a cada lance da partida.
    std::vector<std::unique_ptr<Busca>>     ajudantes;
    std::vector<std::unique_ptr<Avaliador>> av_ajudantes;
    std::vector<std::thread>                fios;
    // A TABELA E' PARTILHADA, e e' por ela que as threads conversam.
    //
    // Era membro por valor: com N buscas davam N tabelas separadas, e uma busca
    // paralela em que cada thread tem a sua tabela nao e' busca paralela -- e'
    // N motores a repetir o mesmo trabalho sem se dizerem nada.
    //
    // A dona continua a ser esta instancia enquanto houver so' uma. Com varias,
    // todas apontam para a da primeira. A tabela ja' estava preparada para isto:
    // as entradas sao `std::atomic` com a chave em XOR com os dados, que e' o
    // que torna uma leitura rasgada detectavel em vez de silenciosa.
    TranspositionTable  tabela_propria;
    TranspositionTable* p_tab = &tabela_propria;

    /// Aponta esta busca para a tabela de outra. Chamar ANTES de arrancar.
    void partilha_tabela(TranspositionTable* t) { p_tab = t; }
    /// Aponta o historico desta busca para o de outra. Chamar ANTES de arrancar.
    void partilha_historico(Busca& dono) {
        cont_hist    = &dono.cont_hist_meu;
        hist_peao    = &dono.hist_peao_meu;
        corr         = &dono.corr_meu;
        hist_proprio = false;
    }
    TranspositionTable* minha_tabela() { return &tabela_propria; }
    std::atomic<bool>  parar{false};
    std::uint64_t      nos = 0;
    // Quantas vezes as tabelas de finais responderam em vez da busca.
    std::uint64_t      tb_acertos = 0;
    // A profundidade MAXIMA que a busca chegou a tocar, incluindo a quiescencia.
    int                sel_prof = 0;
    // Contadores de diagnostico da extensao singular.
    std::uint64_t conta_sing = 0, nos_sing = 0, conta_ext1 = 0, conta_ext2 = 0, conta_ext_neg = 0;
    std::uint64_t pc_nos = 0, pc_tentativas = 0, pc_passou_qs = 0, pc_cortes = 0;
    /// Quantas vezes a REDE foi mesmo chamada. Nao conta as leituras da tabela.
    ///
    /// O acumulador e a avaliacao valem-nos 43% do tempo e a` referencia 22,7%,
    /// com o mesmo codigo de rede. Uma de duas: ou cada avaliacao nos custa o
    /// dobro -- impossivel, e' o mesmo codigo -- ou fazemos o dobro delas. Este
    /// contador diz qual.
    /// `[ply][from*64+to]` para os primeiros LOW_PLY plies.
    static constexpr int LOW_PLY = 5;
    std::vector<int> low_ply;
    std::uint64_t n_aval = 0;
    /// A mesma conta repartida: busca contra quiescencia, calculadas contra
    /// lidas da tabela. E' o que diz se o problema e' a tabela falhar ou a
    /// quiescencia ser grande de mais.
    std::uint64_t n_aval_bs = 0, n_hit_bs = 0, n_aval_qs = 0, n_hit_qs = 0;

    // O historico da partida ate' a` raiz, para a repeticao.
    std::vector<Key> chaves_jogo;
    // Os ultimos lances da PARTIDA, como (peca, destino).
    //
    // A historia de continuacao pergunta "o que resulta em resposta ao que
    // acabou de acontecer", e no topo da arvore o que acabou de acontecer esta'
    // na PARTIDA e nao na busca. Sem isto as tabelas nascem vazias nos primeiros
    // plies de cada busca -- que e' onde vive a maior parte dos nos -- e a
    // ordenacao ali corre so' com a tabela de-para.
    int pre_pc[6]{}, pre_para[6]{};
    int pre_n = 0;

   private:
    int negamax(Position& pos, int prof, int alpha, int beta, int ply, bool pv, bool cut);
    int quiescencia(Position& pos, int alpha, int beta, int ply);
    int aspiracao(Position& pos, int prof, int anterior);

    // Listas de tamanho fixo em vez de `std::vector`.
    //
    // Cada no' alocava tres vectores no monte -- lances, notas e historico -- e
    // libertava-os a` saida. Sao milhoes de alocacoes por busca, num sitio onde
    // o numero de lances tem tecto conhecido: nao ha' posicao legal com mais de
    // 218, e 256 e' folga de sobra.
    static constexpr int MAX_LANCES = 256;
    struct Lista {
        Move lances[MAX_LANCES];
        int  notas[MAX_LANCES];
        int  hist[MAX_LANCES];
        int  n = 0;
        void junta(Move m) { if (n < MAX_LANCES) lances[n++] = m; }
    };

    void pontua(const Position& pos, Lista& l, int de, Move tt_lance,
                Limite tt_limite, int ply, int prof) const;
    int  hist_de(const Position& pos, Move m, int ply) const;   // para a PODA
    int  ordena(const Position& pos, Move m, int ply) const;    // para a ORDEM
    int  conts(const Position& pos, Move m, int ply, int pc) const;
    int  otimismo_de(const Position& pos) const;
    int  valor_empate(const Position& pos) const;
    void garante_ameacas(const Position& pos, int ply) const;
    int  ameaca(const Position& pos, Move m, int ply) const;
    // O balde de ameacas: 0 nenhuma, 1 chega a casa ameacada, 2 sai de casa
    // ameacada, 3 as duas. E' a quarta dimensao da tabela principal -- o mesmo
    // lance de-para quer dizer coisas diferentes conforme a peca esteja ou nao
    // em perigo antes e depois.
    int  balde(Move m, int ply) const;
    int  capt_nota(const Position& pos, Move m) const;
    void credita(const Position& pos, Move m, int ply, int bonus);
    void credita_captura(const Position& pos, Move m, int bonus);
    int  reducao(int prof, int i, int alpha, int beta, bool melhorando, bool tranquilo,
                 bool cut, bool pv, bool tt_pv, Move tt_lance, int hist_i,
                 bool ttpv_bate_alpha, bool ttpv_fundo, int cortes_filho) const;

    // --- historia de correccao ---
    //
    // Quanto e' que a estatica costuma estar ERRADA aqui.
    //
    // As margens da poda sao numeros fixos comparados contra a estatica. Quando
    // essa estatica esta' SISTEMATICAMENTE errada para uma familia de posicoes
    // -- e esta', porque a rede nao ve' o que so' a busca encontra -- essas
    // margens mordem no sitio errado, sempre da mesma maneira. Isto guarda a
    // media do que a busca acabou por dizer menos o que a estatica dizia, por
    // familia, e devolve-a da proxima vez que a familia aparecer.
    void indices(const Position& pos, int ply, int fora[6]) const;
    int  corrigida(const Position& pos, int cru, int ply) const;
    void aprende(const Position& pos, int dif, int prof, int ply);

    bool sem_tempo();
    bool repeticao(const Position& pos) const;

    Avaliador* av = nullptr;
    bool       parado = false;

    std::chrono::steady_clock::time_point inicio;
    std::chrono::milliseconds             mole{0}, duro{0};

    // A tabela de reducao, em MILESIMOS de ply. Truncar cedo deitava fora tudo
    // o que a formula diz entre um ply e dois, antes de os ajustes -- captura,
    // cut node, historico -- poderem usar essa precisao.
    int lmr[64][64]{};

    // --- as tabelas ---
    int principal[2][64][64][4]{};
    // O mapa de ameacas guardado POR PLY. Calcula-lo por lance seria dezasseis
    // ciclos sobre as pecas vezes dez lances por no'; dentro de um no' a posicao
    // e' a mesma, portanto calcula-se uma vez.
    //
    // Um lugar por ply e nao um so': com um lugar unico o filho calculava o seu
    // mapa e despejava o do pai, e quando o pai voltava para creditar o
    // historico tinha de o calcular outra vez -- dois mapas por no' em vez de um.
    mutable std::uint64_t ameacas_chave[MAX_PLY]{};
    // As tres chaves da correccao que dependem SO' da posicao.
    mutable std::uint64_t corr_pos_chave[MAX_PLY]{};
    mutable int           corr_pos[MAX_PLY][3]{};

    mutable Bitboard      ameacas_mapa[MAX_PLY]{};
    // As casas batidas por peca MAIS BARATA do que cada tipo. Indice 0..5 por
    // `idx_pc`; peao e rei ficam a zero de proposito.
    mutable Bitboard      ameacas_barato[MAX_PLY][6]{};
    /// Quantas vezes cada ply ja' cortou por beta nesta busca.
    int cut_cnt[MAX_PLY + 8]{};
    // O que costuma resultar com esta peca nesta casa. A de-para nunca pode
    // dizer isto: aprende que g3-f5 resulta, nao que um cavalo em f5 e' bom.
    int hist_pc[6][64]{};
    // [peca que come][casa][peca comida]. O que uma captura leva sabe-se antes
    // de a jogar; se ela resulta, nao.
    std::vector<int> capt_hist;  // 6*64*6
    // [distancia][peca anterior * 64 + casa anterior][peca][casa]
    // Dois killers por ply. Guardam-se no corte de um TRANQUILO; nao se
    // guardam capturas, que ja' tem banda propria pelo MVV e pelo SEE.
    Move killers[MAX_PLY][2]{};
    // AS TRES FAMILIAS QUE OS FIOS PARTILHAM.
    //
    // A quatro fios fazemos 3,17x os nos de um fio e mesmo assim chegamos ao
    // ply 21 onde um motor de referencia chega ao 25. Os nos estao la'; o que
    // nao circulava era a APRENDIZAGEM. Cada fio tinha o seu historico, e o que
    // um descobria morria com ele.
    //
    // Sao tres e nao quatro: as continuacoes, a correccao e o historico de
    // peoes. A de capturas fica por fio -- o que uma captura leva sabe-se antes
    // de a jogar, portanto ha' menos a ganhar em partilha-la, e era assim que
    // estava.
    //
    // Atomicas com ordem RELAXADA. Nao ha' ordem a garantir entre elas: sao
    // conselhos de ordenacao, e um conselho lido a meio de uma actualizacao
    // ainda e' um conselho. O que nao se pode e' ter leitura e escrita
    // simultaneas em `int` cru, que e' comportamento indefinido e nao apenas um
    // numero errado.
    //
    // O padrao e' o mesmo da tabela de transposicao logo acima: um vector
    // proprio e um ponteiro que os ajudantes reapontam para o do dono.
    std::vector<std::atomic<int>>  cont_hist_meu;   // 5 * (6*64) * (6*64)
    std::vector<std::atomic<int>>* cont_hist = &cont_hist_meu;
    // O historico indexado pela ESTRUTURA DE PEOES: peao_chaves * 12 * 64.
    std::vector<std::atomic<int>>  hist_peao_meu;
    std::vector<std::atomic<int>>* hist_peao = &hist_peao_meu;

    // [familia][lado][chave]. Cinco familias: peoes, a forca restante de cada
    // lado, o ultimo lance por peca e destino, e o ultimo lance como mudanca da
    // chave da posicao.
    std::vector<std::atomic<int>>  corr_meu;   // 6 * 2 * 16384
    std::vector<std::atomic<int>>* corr = &corr_meu;
    /// Sou dono destas tabelas, ou sao emprestadas de quem me lancou?
    ///
    /// So' o dono as limpa. Um ajudante a limpar apagaria, a cada busca, tudo
    /// o que a busca principal tinha aprendido.
    bool hist_proprio = true;

    // O que se jogou em cada ply, como (peca, destino).
    int jogado_pc[MAX_PLY]{};
    int jogado_para[MAX_PLY]{};
    // A estatica em cada ply, para se saber se as coisas tem estado a melhorar.
    int aval_ply[MAX_PLY]{};
    // Que plies la' chegaram por passar. Dois passes seguidos nao provam nada.
    bool nulo_em[MAX_PLY]{};
    // O lance que este ply esta' a fingir que nao existe, enquanto descobre se
    // era o unico a segurar a posicao.
    Move excluido[MAX_PLY]{};
    std::vector<Key> chaves;

    Move pv_tab[MAX_PLY][MAX_PLY]{};
    int  pv_n[MAX_PLY]{};
    Move melhor_raiz = Move::none();
    int  nota_raiz   = 0;
    // Para o optimism: a nota da iteracao ANTERIOR e de quem joga na raiz.
    int   nota_raiz_ant = 0;
    Color lado_raiz     = WHITE;
    // Para o desprezo: o Elo do adversario (0 = desconhecido) e se ha'
    // incremento nesta partida.
    int   opponent_elo   = 0;
    bool  tem_incremento = false;
    // --- o elastico da gestao de tempo ---
    // O orcamento de um lance nao e' fixo: aperta ate' 0,75 quando a posicao e'
    // facil e estica ate' 1,75x1,5 quando ela nao esta' resolvida. E' daqui que
    // vem a poupanca. Sem isto o motor estica quase todos os lances ao maximo --
    // mediana do factor 2,7 no half2k antes de o ter -- e perde por bandeira.
    Move melhor_anterior = Move::none();
    int  iters_sem_mudar = 0;
    int  mudancas        = 0;
    int  nota_media      = 0;
    std::chrono::milliseconds mole_base{0};
    // Cada lance da raiz com o que esta iteracao achou dele. Guardar so' o
    // melhor nao deixa nada em que cair quando o melhor repete: ha' um lance e
    // nenhuma razao para preferir outro.
    std::vector<std::pair<Move, int>> notas_raiz;
    // Para o factor do esforco: onde esta iteracao comecou a escrever em
    // `notas_raiz`, quantos nos ela ja' levou, e quantos foram para cada lance
    // da raiz.
    std::size_t   raiz_marca   = 0;
    std::uint64_t nos_iter_ini = 0;
    std::uint64_t nos_por_lance[MAX_LANCES]{};
    bool repete_ja(Position& pos, Move m);
    int  delta_raiz  = 1;
};

}  // namespace Kestrel

#endif
