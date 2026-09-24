# A fila de medidas, depois do `ameaca_f`

## A regra

Uma medida de Elo vale **contra a base em que foi feita**. Quando alguma coisa
muda o jogo a serio, as outras medidas ficam desactualizadas -- incluindo as
que mandaram desligar ou reverter qualquer coisa.

A correccao do `ameaca_f` (`4a21c2f`) muda a ordenacao em toda a arvore: o
termo passou de 64 para 15.620 num cavalo, numa escala onde a historia vai ate
+-15.000. **Tudo o que foi medido antes disto esta por revalidar.**

O commit `47cf36a` ja tinha dito o mesmo por outras palavras: o teste antigo do
`lmr_cut_f` media 2048 com o `lmr_ttpv` tambem a 2048, uma mistura que a
producao nunca correu, e por isso nao era prova.

## Como

- **um SPRT de cada vez, um termo so a mexer**, sempre contra a base ACTUAL
- nao reverter por analogia nem por um torneio de fundo: medir
- uma REVERSAO feita com base numa medida antiga fica tambem por revalidar
- "esta medido" tem sempre de vir com *contra que base*

## A fila

A base e o HEAD com o `ameaca_f` corrigido, depois de o seu proprio SPRT
fechar.

| # | o que medir | nota |
|---|---|---|
| 1 | `lmr_cut_f` 2048 contra 1024 | **reaberto**. Foi revertido em `681bd12` com o teste de `47cf36a`, que e' anterior ao `ameaca_f` funcionar. O 2048 e' o valor do binario de producao |
| 2 | `63c8f44` -- politica de substituicao da TT | os valores antigos vinham do commit inicial e nunca foram medidos; os meus tambem nao |
| 3 | `8441245` -- a cauda da curva de tempo (24/16 contra 31/28) | **NAO se ve a profundidade fixa nem a 10+0.1**. Precisa de um tempo real, 60+0.6 ou mais, e de partidas longas |
| 4 | `95a93ac` -- o filtro de profundidade dos votantes | **so actua com Threads > 1**. Medir a 4 fios. O filtro foi desenhado de proposito em `c7105ac`, que admite nao o ter medido |
| 5 | `27c038c` -- a escala do bonus da historia de peoes | foi medido contra a base ERRADA (134 partidas, inconclusivo, e com o `lmr_cut_f` a 2048). Refazer |
| 6 | `KS_LMR_DELTA` | inerte a zero. Agora e' uma experiencia nova sobre uma base nova |
| 7 | `KS_ASP_SF` | idem. Troca tres coisas ao mesmo tempo -- ver a nota nos parametros |
| 8 | `KS_LMP_MELHORA` | idem |
| 9 | `KS_PODA_RED` | idem |
| 10 | `KS_TM_ADV_F` | idem, e como o 3 precisa de tempo real |

## O que NAO precisa de medida

Os commits `503326e`, `12fde85` e `6fa7441` repoem codigo que esta a zero por
omissao. Verificado com contagens de nos identicas ao no' antes e depois. Nao
mudam o jogo enquanto as manetes nao forem ligadas -- e liga-las e' o que a
fila acima faz.

## O erro que originou esta fila

Tratei "diferente do `ks_1.20260919`" como "errado". O binario e uma fotografia
de 19-09 e cairam 41 commits depois, alguns deles melhorias MEDIDAS que se
afastam dele de proposito. A desmontagem diz o que o binario fazia; nao diz se
aquilo foi uma decisao ou um acidente, nem ve o que se aprendeu depois.

O `lmr_cut_f` foi o caso: repus o 2048 do binario por cima de um 1024 que
valia +19,97 +/- 10,68 em 1080 partidas, e o meu proprio SPRT deu -4,5 a andar
sempre para baixo sem eu o ouvir.

## Estado

| # | termo | resultado |
|---|---|---|
| 0 | `ameaca_f` com a escala certa (`4a21c2f`) | **+10,14 Elo** [+3,88, +16,41], 5688 partidas, `ob1` + PC somados. FICA |
| 1 | `lmr_cut_f` 2048 contra 1024 | **fica o 1024.** -5,34 +/- 5,40 em 2081 partidas. A interaccao existe -- a vantagem do 1024 encolheu de ~20 para ~5 com a ameaca a funcionar --, mas nao chega a inverter: o limite superior do intervalo e' +5 |
| 2 | politica da TT (`63c8f44`) | a correr no PC. **Com `Hash=16`**: a substituicao so' actua com a tabela cheia, e a profundidade 12 a partir de uma TT vazia da' 93.464 nos com e sem ela -- nao mede nada. Em jogo real vimos `hashfull 1000` |
| 3 | escala dos peoes (`27c038c`) | a correr no `ob1` |
| - | `alpha_desc` | **novo na fila.** A guarda do beta que faltava foi reposta (`89ddfac`); as tres rejeicoes antigas foram medidas SEM ela |

### Como construir as variantes

Com worktrees, sem tocar na arvore principal:

    git worktree add --detach /tmp/wt_x HEAD
    ln -s .../ks-cf796d1f923d.nnue /tmp/wt_x/      # a rede nao esta' no git
    cd /tmp/wt_x && git revert --no-commit <commit>
    make ARCH=avx2 NOME=ks_x

`ARCH=avx2` e nao `native`: o `ob1` e' EPYC e o PC e' Ryzen 3600, e um binario
`native` de um pode usar instrucoes que o outro nao tem. As contagens de nos
sao iguais entre os dois -- confirmado, 93.464 nos na kiwipete em ambos.

### Como somar duas maquinas

Os W/E/D somam-se; o LLR NAO -- cada instancia calcula o seu so' com as suas
partidas. A decisao le-se pelo intervalo de 95% do total, com a variancia REAL
dos resultados (com empates), nao a de Bernoulli, que a inflaciona muito
quando metade das partidas sao empates.
| 2 | politica da TT (`63c8f44`) | **neutra.** +0,51 +/- 5,29 em 2025 partidas, `Hash=16`, IC95% [-9,9, +10,9]. O SPRT simetrico nunca fecha com um efeito perto de zero -- fechado pelo intervalo. Fica a do binario, por fidelidade e nao por Elo |
| 4 | `alpha_desc = 1` | a correr no PC, limites [0, 5]. Mesmo binario dos dois lados, a manete ligada por um invólucro (`KS_ALPHA_DESC=1 exec ./ks_base`) |
| 3 | escala dos peoes (`27c038c`) | **neutra.** +2,03 +/- 5,18 em 2220 partidas, IC95% [-8,1, +12,2]. Fica, por fidelidade |

### Pendente: o teste a 4 fios (marcado para 24-09)

Todas as medidas acima foram a UM fio. O filtro dos votantes (`95a93ac`) nem
existe a um fio, e a CCRL corre a 4. Falta: **`ks_modifs` (HEAD) contra
`ks_github` (`origin/kestrelstrike`, `6d0555c`), os dois a `Threads=4`.**

- 4+4 = 8 fios por partida. O `ob1` e o `ob6` tem 6 nucleos: nao cabe sem
  sobrecarga, e sobrecarregar distorce precisamente a escala do SMP. O unico
  sitio onde cabe e' o PC (12 fios logicos), a uma partida de cada vez.
- Binarios ja construidos, `ARCH=avx2`: `/root/sprt_bin/ks_modifs`
  (`9306c468`) e `/root/sprt_bin/ks_github` (`6cc3d431`) no `ob1`.

## 24-09-2026 — o que se mediu e o que se fechou

| o que | resultado |
|---|---|
| `alpha_desc = 1` (item 4) | **fechado por interrupcao, sem veredicto.** 1100 partidas a 10+0,1: +9,16 +/- 10,76, LLR 0,66. Parei-o para dar a maquina a outra coisa; o parcial fica em `/root/sprt_ad/ad_parcial_1010.*`. Continua na fila |
| `lance_so_alpha` (novo) | **REJEITADO.** -26,02 +/- 14,59 em 602 partidas a 5+0,05, `Hash=16`, IC95% [-40,6; -11,4]. Ver `0d5f360` |
| `ameaca_f` 10 contra 20 (novo) | **neutro, fica o 20.** +2,95 +/- 5,65 em 4002 partidas, IC95% [-2,7; +8,6]. Ver `d35bd60`. Fecha a pergunta de 18-09 |
| `asp_sf` (item 7) | **neutro, fica a zero.** -1,74 +/- 6,21 em 3202 partidas, IC95% [-8,0; +4,5]. Ver `d4a5183` |

### O que a auditoria da ordenacao diz hoje

A taxa de corte ao primeiro lance **nao fechou** o buraco contra o SF19 com a
NOSSA rede. Medida no HEAD, uma posicao por processo, `go depth 15`:

| posicao | auditoria 22-09 | SF19 | HEAD 24-09 |
|---|---|---|---|
| inicial | 74,07 | 84,58 | 74,72 |
| kiwipete | 75,77 | 85,62 | 76,17 |
| siciliana | 71,87 | 82,34 | **74,98** |
| aberta d4 | 72,17 | 83,35 | 72,33 |
| final T+P | 78,31 | 90,79 | 78,18 |
| final peoes | 67,22 | 83,57 | **72,86** |

A `ameaca` corrigida deu +3,1 e +5,6 nas duas ultimas. Ficam 8 a 12 pontos.

### Onde estao os ciclos

Do `perf.data` que estava por analisar em `/root/perf_ks/` (seis posicoes,
`go depth 15`): **53% em acumuladores NNUE**, com `apply_combined` sozinho a
31,9%; a busca inteira e' 30%. O CPU do `ob1` e' AVX2 **sem AVX-512** (1.773
`ymm`, zero `zmm` no binario), portanto `ARCH=avx2` ja' e' o tecto e nao ha'
ganho por essa via.

### Hipoteses eliminadas, para nao se repetirem

- **As manetes desligadas nao sao um segundo `ameaca_f`.** Teste de inercia a
  profundidade 12 (base 295.733): `KS_LMR_DELTA=100` 406.809, `KS_ASP_SF=1`
  230.837, `KS_LMP_MELHORA=100` 256.900, `KS_PODA_RED=100` 326.764,
  `KS_ALPHA_DESC=1` 315.349. Todas vivas. O `KS_TM_ADV_F` aparece inerte porque
  e' do relogio e o teste e' a profundidade fixa
- **`guarda_so_aval` nao expulsa entradas fundas desta busca**: a geracao avanca
  uma vez por `go`, nao por iteracao, logo `velhice > 2` sao jogadas passadas
- **A busca principal nao desperdica avaliacoes**: ja' le' a da tabela, 37-47%
  poupadas. A quiescencia so' poupa 8-10%, mas gravar a dela faz a arvore
  crescer 15,6% -- ver `qs_guarda_aval`, fica a zero
- **O RFP ja' devolve `beta + (aval - beta)/3`**, as continuacoes ja' sao
  `{1, 2, 4}`, a assimetria `hist_de`/`ordena` ja' esta' comentada

### O que esta noite ELIMINOU como caminho

Duas experiencias independentes atacaram o mesmo defeito -- o lance da tabela
render 30-43% contra 52-63% da referencia -- e falharam do mesmo modo:

- **`lance_so_alpha`** apaga o lance nos nos ALL: **-26 Elo**
- **`tt_sup_nota`** desce-lhe a prioridade quando vem de limite superior: faz a
  arvore crescer 33-55% e **nao tem meio-termo** -- a transicao esta entre
  600.000 e 700.000, e abaixo dela o lance cai por baixo de TODAS as capturas
  de uma vez (999k/950k/900k/800k/700k dao 295.733; 600k da' 459.853; 550k/300k/
  60k dao ~394.600)

A licao comum: **o lance guardado vale muito mais do que a sua taxa de corte
sugere.** Mesmo vindo de um no' onde nada provou nada, e' a melhor pista que
aquela posicao tem. O Stockfish e o Triumviratus podem dispensa-lo porque a
preservacao do lance antigo lhes tapa o buraco; aqui a preservacao so' actua
quando a via escolhida ja' tem ESTA chave.

Portanto o buraco dos 8-12 pontos **nao e'** guardarmos lances a mais. E' outra
coisa, e estes dois candidatos estao fechados.

### O placar da noite

Quatro medidas, ~11.000 partidas, **zero Elo ganho**: uma rejeicao clara
(`lance_so_alpha`, -26) e tres neutros (`ameaca_f` 10, politica da TT ja' de
ontem, `asp_sf`). O valor esta nos becos fechados e no contraste arvore/Elo,
que hoje discordaram tres vezes em tres.

## Fora da fila de Elo, mas antes da CCRL: o `stop` nao funciona

Encontrado a 24-09-2026 ao medir as paginas grandes. **Com uma busca a correr,
o motor nao responde ao `stop` nem ao `quit`.** O antigo e o novo, igual:

    uci / position startpos / go depth 30 / (3 s) stop / (1 s) quit
    -> nenhum `bestmove`, o processo so' morre pelo `timeout` (rc=124)

**A causa esta' no `uci_laco.cpp`:** o `faz_go` chama `g_busca.arranca(...)`
na MESMA thread que le' o `stdin`. Enquanto a busca corre ninguem le' os
comandos; o `stop` so' e' lido quando ela ja' acabou, e ai' nao faz nada (o
ramo e' `if (tok == "quit") break;` e mais nada).

**Nas partidas com relogio nao aparece** -- o motor para sozinho pelo tempo --,
e por isso todos os SPRTs correram bem. Mas:

- um `go infinite` fica pendurado PARA SEMPRE: a analise em qualquer interface
  (Arena, ChessBase, Banksia) nao funciona
- `ponder` e' impossivel
- o `stop` e' obrigatorio no protocolo UCI

**A infraestrutura ja' existe.** O `sem_tempo()` le' um `std::atomic<bool>
parar`, e a busca principal ja' o usa para parar os ajudantes (`busca.cpp`,
`b->parar.store(true)`). Falta so' a busca correr numa thread separada da que
le' os comandos. Tres cuidados:

1. **uma corrida no arranque**: o `arranca` faz `parar.store(false)` logo ao
   comecar, e um `stop` que chegue antes disso perde-se. O `parar` tem de ser
   reposto na thread dos comandos, ANTES de a busca nascer.
2. **o `go infinite` nao pode imprimir `bestmove` antes do `stop`**, mesmo que
   a busca acabe sozinha (profundidade maxima, mate encontrado).
3. **a pilha**: no Windows uma `std::thread` nasce com 1 MB, e a recursao funda
   do `negamax` -- `Lista`, `StateInfo`, os vectores de lances vistos -- pode
   nao caber. O Stockfish cria as threads com 8 MB de proposito. Os ajudantes
   do SMP ja' correm em `std::thread` e tem o mesmo risco no Windows.

**O teste:** o `stop` tem de dar `bestmove` em milissegundos, o `quit` tem de
terminar o processo, o `isready` durante a busca tem de responder, e a
contagem de nos a profundidade fixa tem de ficar IDENTICA (a busca em si nao
muda).
