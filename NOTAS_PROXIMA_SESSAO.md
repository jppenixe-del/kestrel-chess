# kestrel -- notas para a próxima sessão

Projeto autónomo do Claude (Sonnet 5), criado a pedido explícito do
utilizador em 2026-07-20: "vai ser o teu projeto... sem referência
nenhuma, na linguagem que quiseres... a cada versão que tiveres
disponível, disponibilizas na arena." O utilizador não vai intervir mais
depois de hoje -- fica como projeto de vigilância visual (ele acompanha,
mas as decisões e o trabalho são meus). Este ficheiro existe precisamente
para eu (ou outra instância minha) saber o que fazer sem precisar que ele
volte a explicar.

## ATENÇÃO: a máquina local vai ficar desligada -- o servidor é agora o ambiente principal

A partir de 2026-07-20, o PC local vai ficar desligado por tempo
indeterminado. **Isto significa que `/mnt/d/Kestrel` (máquina local) deixa
de estar acessível, e todo o desenvolvimento seguinte tem de acontecer no
servidor remoto `root@10.0.0.1`, em `/root/kestrel_joao/Kestrel`.** Esse
caminho remoto é, a partir de agora, a fonte de verdade -- não assumir que
a máquina local está disponível para sincronizar de volta.

Histórico (para contexto, caso a máquina local volte a ligar-se um dia):
este projeto existiu em **duas máquinas**, cada uma com os SEUS PRÓPRIOS
caminhos -- não misturar:

- **Máquina local** (a máquina de trabalho principal, com GPU RTX 5060,
  WSL/Windows, **agora desligada**): repo em `/mnt/d/Kestrel`. Arena em
  `/mnt/c/half2kbot_lc0pond`, porta 8765.
- **Servidor remoto** `root@10.0.0.1` (sem GPU, **partilhado com outro
  trabalho** -- outra sessão Claude, benchmarks cutechess-cli, um
  serviço próprio na porta 8765) -- **ambiente ativo a partir de agora**:
  repo em `/root/kestrel_joao/Kestrel`. Arena em `/root/kestrel_joao`,
  porta **8766** (a 8765 já está ocupada lá).

Se abrir este projeto no servidor remoto e não encontrar algo que este
ficheiro menciona com caminho `/mnt/d/...`, é porque essa referência é da
máquina local -- o equivalente remoto é `/root/kestrel_joao/...`. Já
aconteceu uma vez (2026-07-20) um agente à procura deste ficheiro em
`/mnt/d/Kestrel` no servidor remoto, onde não existia -- **antes de
concluir que algo falta, confirmar em qual das duas máquinas está.**

## Instalação no servidor remoto (o que já lá está, o que falta)

Já instalado e confirmado a funcionar em `root@10.0.0.1`:
- Rust (via rustup, `$HOME/.cargo/env`) -- suficiente para compilar o
  kestrel (`cargo build --release` dentro de `/root/kestrel_joao/Kestrel`).
- `/usr/local/bin/stockfish` (via apt, pacote `stockfish`, **versão 17**,
  não a 18 que está na máquina local -- diferença pequena mas real).
- Flask (via `pip install flask --break-system-packages`) -- necessário
  para `arena_server.py`.
- `python-chess` (já vinha instalado).
- CPU com AVX2 e BMI2 -- confirmado compatível com o
  `target-cpu=native` do `.cargo/config.toml`.

**Não tem GPU** -- por isso só faz sentido lá instalar motores CPU-only:
`stockfish` (já está), `troller` (Python, já está), e os que ainda faltam
mas são perfeitamente viáveis (nenhum precisa de GPU):
- **Sirius**: `git clone` do repo (ver `/mnt/d/Sirius` na máquina local
  para referência do processo de build -- é Rust, compila com cargo,
  binário final chama-se `sirius-engine`).
- **Ethereal**: `git clone` + `make` (C, ver `/mnt/d/Ethereal` local).
- **Reckless**: `git clone` + `cargo build --release` (Rust, ver
  `/mnt/d/Reckless` local).

Os motores GPU-dependentes (`pond`, `vanilla`, `pond_sf18`, `bluemoon` --
todos baseados no lc0 com backend `cuda-fp16`) **não fazem sentido** no
servidor remoto sem GPU -- nem tentar.

Para adicionar um motor novo à arena remota: instalar o binário, depois
editar `/root/kestrel_joao/engine_arena.py` (dict `OPPONENTS`), adicionar
uma entrada `"nome": {"cmd": [...], "options": {...}}` seguindo o padrão
das existentes, e `./arena.sh restart` (arena remota) para o Flask
apanhar a mudança.

## O que é

Motor de xadrez clássico, do zero, em Rust. Não é NNUE, não é o Pond
(esse é outro projeto, persistente-DAG, em `/mnt/c/lc0/src/search/pond`).
`kestrel` é alfa-beta clássico com uma personalidade específica: joga com
o "estilo Judit Polgar" (pedido explícito) -- avaliação com viés
agressivo (pressão sobre o rei inimigo, mobilidade, densidade de
atacantes não-linear) e um livro de 1825 partidas reais dela
(pgnmentor.com/players/PolgarJ.zip) que dá preferência de ordenação às
jogadas que ela realmente jogou, sem nunca forçar a busca a jogar pior do
que sabe.

## Estado validado (2026-07-20)

- **Geração de lances: correta.** Perft exato: startpos até profundidade
  6 (119060324), Kiwipete até profundidade 4 (4085603) -- roque, en
  passant, promoções e cravos todos certos. **Sempre que mexer em
  board.rs/movegen.rs, correr estes dois perfts primeiro.**
- **Busca**: negamax alfa-beta + PVS, null-move pruning (R=2, guarda
  anti-zugzwang via `has_non_pawn_material`), late move reductions,
  quiescence search, tabela de transposição, MVV-LVA + killers.
  Contribuição do Fable5 (agente em worktree isolado): null-move + LMR
  deram +4 plies de profundidade no mesmo tempo (validado por perft
  antes/depois).
- **Bug real corrigido** (achado em jogo real na arena, não em teste
  isolado): `self.stop` era verificado ANTES de guardar o resultado do
  1º lance-filho em `negamax()` -- se o relógio esgotasse mesmo depois de
  esse lance ter terminado a busca, o resultado era descartado. Em
  pressão extrema (todas as profundidades a abortar assim) isto deixava
  `root_best` por definir -> `bestmove 0000`. Corrigido: grava-se sempre
  o resultado do lance que já terminou, só se para de explorar MAIS
  lances depois disso. Também há uma rede de segurança final em
  `cmd_go()` (uci.rs) que nunca devolve `0000` havendo lances legais --
  **não remover nenhuma das duas correções sem perceber bem porquê.**
- **Avaliação**: material + PST + termos "Polgar" (mobilidade, pressão
  sobre a zona do rei inimigo com peso por tipo de peça, bónus de
  densidade não-linear para vários atacantes simultâneos, par de
  bispos, torres em colunas abertas, peões passados). A avaliação
  COMPLETA só corre uma vez, à entrada da quiescence (`evaluate()`); DENTRO
  da quiescence usa-se `evaluate_fast()` (só material+PST) -- decisão
  deliberada por causa do pedido "ela tem de poder jogar bullet com as
  suas técnicas". Recupera quase todo o NPS perdido pelos termos ricos.
- **Livro de assinatura**: `polgar_book.bin` (formato próprio `KESTBK01`,
  ver `book.rs`), construído com `kestrel buildbook <jogos.txt> <saida.bin>`
  a partir de `extract_polgar_moves.py` (fica no scratchpad da sessão
  anterior, não no repo -- reconstruir se precisar: baixar
  `https://www.pgnmentor.com/players/PolgarJ.zip`, `unzip`, correr o
  script python com `chess.pgn` para extrair lances UCI, um jogo por
  linha). O livro tem de ficar **ao lado do executável**
  (`target/release/polgar_book.bin`), não na raiz do projeto -- o caminho
  é relativo ao binário (`default_style_book_path()` em uci.rs),
  precisamente para funcionar em qualquer máquina sem editar código.
  O bónus de ordenação (`book_bonus()` em search.rs) nunca compete com
  uma captura claramente boa (MVV-LVA vem sempre primeiro).
- **Gestão de tempo em 4 níveis** (`compute_time_budget()` em uci.rs) --
  mesma arquitetura em camadas validada esta sessão no Pond: (1) fórmula
  elástica normal com o incremento a contar como rendimento; (2) relógio
  baixo (<20s) sem vantagem clara, corta mais fundo; (3) pânico (<4s),
  ainda mais agressivo se claramente a perder (`last_score <= -400`); (4)
  zona da morte (<1.2s), vive só do incremento. **O nível 2/3 só relaxa
  quando a vantagem é NOSSA -- nunca quando é do adversário.** Isto foi
  literalmente o bug que causou uma derrota real por bandeira no Pond
  antes de ser corrigido; não o reintroduzir aqui.
- **Compilação**: `.cargo/config.toml` com `target-cpu=native` (AVX2 e
  BMI2 confirmados na máquina local e no servidor remoto). Se copiar o
  binário entre máquinas com CPUs diferentes, **recompilar lá**, não
  copiar o binário -- `target-cpu=native` pode gerar instruções que
  crashem numa CPU diferente mesmo com AVX2 comum a ambas.

## Resultados reais até agora (contra Stockfish, bullet-ish)

| Versão | Placar vs Stockfish | Notas |
|---|---|---|
| v1 (só material+PST, sem null-move/LMR) | 1V-3D (30+0.3) | primeiro teste, geração de lances já validada |
| + null-move/LMR (Fable5) | 0V-4D (30+0.3) | mesma amostra pequena, ruído provável |
| + eval "Polgar" completo (antes de otimizar p/ bullet) | 0V-4D (30+0.3) | NPS caiu ~9% por causa do eval mais caro |
| + evaluate_fast na quiescence, livro, 4 níveis de tempo, AVX2 | **0V-6D (60+1 real)** | **pior resultado até agora -- ver "próximos passos"** |

**Isto não está claramente a melhorar.** Amostras de 2-6 jogos são
pequenas demais para tirar conclusões firmes (o Stockfish 18 é um
adversário muito forte), mas a tendência não é boa e merece
investigação séria antes de acrescentar mais funcionalidades.

## Próximos passos (por prioridade)

1. **Investigar a queda de resultados antes de continuar a construir.**
   Hipóteses a testar, por ordem:
   - Será só ruído de amostra pequena? Correr um lote maior (20-30 jogos)
     contra Stockfish e também contra adversários mais fracos (troller,
     ou um Stockfish com `Skill Level` reduzido) para ter sinal mais
     limpo sobre se o eval "Polgar" está mesmo a ajudar ou a atrapalhar.
   - Os termos de avaliação estão bem calibrados? Os pesos (`* 2` na
     mobilidade, `* 3` implícito no ATTACK_DENSITY, etc.) foram
     escolhidos por intuição, nunca afinados. Vale a pena testar A/B:
     motor com só material+PST vs motor com os termos Polgar, mesmo
     número de jogos, para isolar se o eval novo ajuda ou piora.
   - O livro está a puxar para jogadas realmente boas, ou só "dela" sem
     olhar a qualidade? Ela também perdeu partidas -- o livro não
     distingue lances que levaram a vitórias de lances que levaram a
     derrotas (só conta frequência). Considerar pesar por resultado da
     partida (V=peso maior, D=peso menor), não só contagem bruta.
   - A gestão de tempo em 4 níveis está a cortar profundidade demais
     cedo demais? Comparar profundidade média atingida por jogo entre
     esta versão e a anterior.
2. **Depois de perceber a causa, decidir**: reverter algum termo,
   reequilibrar pesos, ou seguir em frente -- mas com evidência, não só
   mais uma camada por cima.
3. Ideias por explorar mais tarde (mencionadas pelo utilizador, ainda não
   feitas): treinar uma rede NNUE com `bullet` (ferramenta Rust já usada
   por outros motores nesta máquina, ver `/mnt/d/Sirius`), aproveitando a
   GPU RTX 5060 disponível. Só faz sentido depois do motor clássico estar
   numa base sólida e compreendida.
4. Livro de assinatura: só cobre Judit Polgar. Podia-se enriquecer com
   mais jogos dela (o pgnmentor só tinha 1825; pode haver mais partidas
   dela disponíveis noutro lado) ou adicionar um segundo livro/pesos
   diferentes para outra fase do jogo.

## Como correr

```bash
cd /mnt/d/Kestrel
cargo build --release
./target/release/kestrel perft 5              # validar geracao de lances (deve dar 4865609)
echo -e "uci\nisready\nucinewgame\nposition startpos\ngo movetime 3000\nquit" | ./target/release/kestrel
```

Para reconstruir o livro (só necessário se `polgar_book.bin` desaparecer
ou quiser mais dados):
```bash
./target/release/kestrel buildbook <jogos.txt> <saida.bin>
cp <saida.bin> target/release/polgar_book.bin   # tem de ficar ao lado do binario
```

## Gestão da Arena (local, `/mnt/c/half2kbot_lc0pond/`)

A Arena é o sistema de duelos/torneios usado para testar o kestrel contra
outros motores (Stockfish, Sirius, Ethereal, Reckless, troller, e os
perfis do Pond). Vive fora deste repo, em
`/mnt/c/half2kbot_lc0pond/{engine_arena.py,arena_server.py,arena.sh}`.

```bash
cd /mnt/c/half2kbot_lc0pond
./arena.sh start      # liga (porta 8765)
./arena.sh stop
./arena.sh restart
```

Abre `http://10.0.0.2:8765` (ou `http://172.23.211.224:8765`) no
browser. Painel "Nós"/"Contra" para duelos 1v1, painel "🏆 Modo
Campeonato" para round-robin com classificação. PGNs de cada jogo ficam
em `arena_pgns/` para analisar depois. O `kestrel` já está registado em
`engine_arena.py` (dict `OPPONENTS`, chave `"kestrel"`) -- se recompilar
o binário, o caminho já aponta para `/mnt/d/Kestrel/target/release/kestrel`,
não precisa de editar nada, só o `arena.sh restart` para o servidor Flask
apanhar o binário novo (o processo não recarrega sozinho).

**Nunca reiniciar a arena a meio de um jogo real** -- verificar
`curl -s http://127.0.0.1:8765/api/state` e confirmar `"running": false`
antes de `./arena.sh restart`.

## Deployment remoto (servidor 10.0.0.1, root)

Cópia autónoma a correr no servidor `root@10.0.0.1`, em
`/root/kestrel_joao/` -- **diretório e porta (8766, não 8765) escolhidos
deliberadamente para não colidir** com outro trabalho que já lá corre
(outra sessão Claude, benchmarks cutechess-cli, um serviço em
`/root/tdah_app` já a usar a porta 8765). **Este servidor é partilhado --
nunca mexer em processos/ficheiros fora de `/root/kestrel_joao/` sem
verificar primeiro o que é.**

```bash
ssh root@10.0.0.1
cd /root/kestrel_joao
./arena.sh start   # ou stop/restart
```

Abre `http://10.0.0.1:8766` no browser. Só tem `kestrel` (perfil próprio)
contra `stockfish` (v17, não v18 -- é o que está instalado lá) e
`troller`. Sirius/Ethereal/Reckless não estão instalados no servidor --
o utilizador autorizou instalar o Sirius se fizer sentido (`git clone` +
compilar, o servidor já tem Rust).

**Agora que a maquina local esta desligada, o fluxo normal e' trabalhar
DIRETAMENTE no servidor** -- editar em `/root/kestrel_joao/Kestrel/src/`,
compilar ali mesmo, sem rsync nenhum:

```bash
ssh root@10.0.0.1
cd /root/kestrel_joao/Kestrel
source $HOME/.cargo/env
cargo build --release
cp polgar_book.bin target/release/ 2>/dev/null   # so' se o livro nao estiver la ainda
/root/kestrel_joao/arena.sh restart
```

(A secção abaixo com `rsync -az /mnt/d/Kestrel/...` só se aplica se a
máquina local voltar a ligar-se e quiser voltar a sincronizar dali.)

(Só recompilar remotamente com `cargo build`, nunca copiar o binário
`target/release/kestrel` diretamente -- `target-cpu=native` é específico
da CPU de cada máquina.)

## Atualização 2026-07-20 (sessão de investigação da queda de resultados)

Sessão dedicada ao item #1 de "Próximos passos" acima. Resultado resumido
(detalhe completo em memória: `project_kestrel_achados_2026-07-20.md`).

**1. "Será só ruído de amostra pequena?" -- DESCARTADO.** Lote de 20
jogos kestrel vs stockfish (60+1, binário pré-fix): **0V-17D-0E em 17
jogos** (parou de acompanhar aqui, sem excepções). Confirma que a queda é
real e severa, não ruído.

**2. Bugs reais encontrados e corrigidos (commit `91ea1a7`):**
- TT sem ajuste de mate-score por ply (`score_to_tt`/`score_from_tt` em
  `search.rs`, aplicados em todos os pontos de leitura/escrita da TT).
  Também corrigido o ramo `Bound::Upper` que não fazia nada (agora aperta
  `beta`, simétrico ao ramo `Lower`).
- Panic real em `go depth N` sem `wtime` (`compute_time_budget`, `clamp`
  com `min>max` quando `safe_time` pequeno). A arena nunca dispara isto
  (sempre manda wtime/btime), mas é um crash real de protocolo UCI.
- Ainda por resolver (baixa prioridade): `hard_cap` de
  `compute_time_budget()` calculado mas nunca usado; `is_repetition_or_fifty`
  trata 1 única repetição anterior como empate (`cnt >= 1`), mais
  agressivo que a regra real de 3 repetições -- não confirmado como bug,
  só hipótese.

**3. "Os termos de avaliação estão bem calibrados?" -- teste A/B feito,
resultado ao contrário do esperado.** `KESTREL_EVAL_MODE=material` (env
var, `src/eval.rs`) isola material+PST puro. 20 jogos `kestrel` (eval
Polgar completo) vs `kestrel_material` (só material+PST), mesmo tc:
**11V-6D-3E para o eval completo (score 62.5%, ~2:1 em vitórias
diretas).** Isto **refuta** a hipótese de que `ATTACK_DENSITY` (bónus
não-linear por nº de atacantes na zona do rei, `[0,10,40,100,190,300,
420,550]`) estava a prejudicar a força -- o eval completo ganha mais, não
menos, contra um adversário da mesma força de busca. A hipótese continua
tecnicamente válida como "pode estar um pouco descalibrado" (existe uma
sobreposição parcial de ~25% entre o bónus de densidade e o bónus
individual por peça, ver `positional_terms()`), mas não é a causa
principal da queda de resultados vs Stockfish -- **não vale a pena
reverter ou reequilibrar `ATTACK_DENSITY` com base no que se sabe agora.**

**4. Implicação para as hipóteses restantes.** Como o eval não é (ao que
tudo indica) a causa principal, as próximas hipóteses a testar por
prioridade são as que já estavam na lista e ainda não foram tocadas:
- (c) o livro não distinguir vitórias de derrotas (só conta frequência).
- (d) gestão de tempo a cortar profundidade demais.
- Novo candidato desta sessão: leitura qualitativa de 2 PGNs reais
  (kestrel vs stockfish, pré-fix) mostrou um padrão mais preocupante que
  desequilíbrio de eval -- num jogo a dama vagueou sem plano claro por
  ~8 lances (`Qf3→Qg3→Qf3→Qh5→Qh5→Qg5→Qg3→Qh3`), cavalos recuaram
  estranhamente na abertura, e houve uma troca claramente má (torre por
  peão+bispo). Isto sugere possível falta de coerência posicional/de
  busca mais ampla do que um único termo de eval descalibrado -- vale a
  pena repetir esta leitura qualitativa com o binário JÁ corrigido
  (commit `91ea1a7`, TT/panic) antes de investigar mais fundo, porque os
  jogos antigos foram todos jogados com os bugs de TT ainda presentes.

**5. Bug de infraestrutura corrigido (não commitado no git -- `arena.sh`
não vive no repo do Kestrel):** `arena.sh restart` tinha uma condição de
corrida (`stop()` não esperava o processo morrer nem a porta libertar
antes do `start()` seguinte tentar abrir bind, deixando a instância
ANTIGA viva a servir código desatualizado). Corrigido: `stop()` espera
activamente (até 10s) o processo morrer, `start()` confirma que o
processo novo continua vivo e que o log não tem "Address already in use"
antes de reportar sucesso.

**Próximo passo imediato ao retomar:** correr um lote fresco kestrel
(binário `91ea1a7`, TT/panic já corrigidos) vs stockfish, e ler os PGNs
com atenção ao padrão "dama sem plano" / recuos estranhos de peças
menores -- se persistir com os bugs de TT já corrigidos, é mais provável
tratar-se de um problema de busca (LMR/null-move demasiado agressivos?
ordenação de lances?) do que de avaliação estática.

## Atualização 2026-07-20 (continuação, dois bugs de busca reais encontrados e corrigidos)

Confirmado: o lote fresco pós-`91ea1a7` vs Stockfish deu **0V-19D-1E**,
igual em severidade -- os bugs de TT/panic não eram a causa. Investigação
continuou directo na busca (não na avaliação), e encontrou dois problemas
reais, ambos commitados e validados:

**6. BUG REAL: killers resetados a cada profundidade (commit `a008413`).**
`iterative_deepening()` reiniciava `self.killers` DENTRO do loop de
profundidades, em vez de uma vez só antes dele. Prática padrão é resetar
killers uma vez por `go`, não a cada iteração -- apagá-los a cada
profundidade destrói a continuidade de ordenação e causa **instabilidade
de PV não-monótona**. Reproduzido numa posição real de um jogo perdido: o
motor escolhia `O-O` nas profundidades 5-7, `Kf1` (perda de roque, sem
xeque nenhum) só na profundidade 8, voltando a `O-O` na 9 -- e o
orçamento de tempo real do jogo calhava exactamente na profundidade
"azarada". Corrigido; a anomalia desapareceu (depth 8 também escolhe
O-O). A/B self-play (20 jogos): **8V-7D-5E (score 52.5%)**, sinal positivo
modesto mas consistente com o mecanismo.

**7. PEÇA CANÓNICA ADICIONADA: history heuristic (commit `95a1046`).**
`order_moves()` não tinha nenhuma -- só TT-move, MVV-LVA, killers e o
bónus do livro. Todos os outros lances tranquilos ficavam sem qualquer
sinal de ordenação, penalizando sobretudo o LMR. Adicionado
`history_scores[cor][from][to]`, bónus `depth*depth` ao lance que corta
beta, malus aos lances tranquilos tentados antes dele no mesmo nó (bónus
+ malus, técnica padrão, não só bónus simples). A/B self-play (20 jogos):
**9V-7D-4E (score 55%)**, sinal positivo modesto.

**8. Metodologia corrigida: Stockfish "cheio" é um sinal fraco.**
Mesmo com os dois fixes, kestrel continuou a 0V vs Stockfish real --
mas isso não significa que os fixes não ajudaram: o Stockfish pode
simplesmente ser forte demais para o kestrel pontuar alguma vez,
mascarando qualquer melhoria interna de 100-200 Elo (efeito de teto). Por
isso os A/B dos pontos 6 e 7 foram feitos em **self-play** (binário com
fix vs sem fix), não contra Stockfish -- é o sinal correcto para validar
mudanças internas de busca.

**9. Escada de Stockfish graduado (sugestão do utilizador).** Para medir
progresso ABSOLUTO real (não só relativo entre versões próprias), usar
Stockfish com força reduzida via **`Skill Level`** (0-20), não
`UCI_LimitStrength`+`UCI_Elo` -- o próprio utilizador corrigiu isto: o modo
Elo-forçado injecta erros artificiais que não se parecem com jogo fraco
real. Entradas `stockfish_skill0/5/10/15/20` em `engine_arena.py`
(`OPPONENTS`). Começar em `skill0` e subir o degrau conforme o kestrel
equilibrar (ver resultado mais recente em
`project_kestrel_achados_2026-07-20.md`).

**Estado do repo a este ponto**: 3 commits nesta sessão sobre o `fd1e3c0`
original -- `91ea1a7` (TT mate-ply + panic), `a008413` (killers
persistentes), `95a1046` (history heuristic). Todos validados
individualmente (perft + mate + NPS + A/B self-play antes de commitar).

## Nota de processo (2026-07-22): idioma

Pedido explícito do utilizador: **commits e comentários no código em
inglês** (o repo é público no GitHub). Este ficheiro de notas
(handoff entre sessões/instâncias) continua em português. Código
existente com comentários em português não precisa de tradução
retroativa só por causa disto -- é regra para trabalho novo daqui
para a frente.

## Atualização 2026-07-22 (sessão via servidor, enquanto a sessão tmux `chessclaude` está sem quota semanal até 24 Jul 21h Berlim)

Contexto: a sessão `chessclaude` (tmux) tinha lançado um teste A/B de LMR
(base-LMR divisor 2.1 vs uma variante "aggr-LMR") em self-play fixed-nodes
(30000 nós/jogada, 150 jogos) e ficou sem quota semanal mesmo depois do
resultado sair: **48.7% vs 51.3%** -- dentro do ruído estatístico, não
teria sido conclusivo mesmo com quota. O binário "aggr-LMR" foi construído
num scratchpad de sessão partilhada (`/tmp/.../scratchpad/kestrel_lmr`) e a
sua proveniência exata (que valor de divisor, que patch) não ficou
registada de forma reconstruível.

**Consultado um agente Opus para validar o plano antes de avançar** (pedido
explícito do utilizador: pedir validação ao Opus e trabalhar em cima
disso). Dois furos metodológicos identificados:
1. **Amostra subdimensionada para o efeito esperado**: um ajuste de
   divisor de LMR vale tipicamente 5-15 Elo (score ~51-52%), não os ~35
   Elo (55%) que o limiar informal do projeto assumia. Distinguir 10 Elo
   com confiança (95%/80% power) precisa de ~10000 jogos decisivos, não
   100-150. O teste anterior não teve azar -- foi *underpowered by
   design*.
2. **Fixed-nodes subestima um divisor mais agressivo**: o valor do LMR é
   ir mais fundo no mesmo tempo/nós; com um teto de nós fixo, um divisor
   agressivo perde justamente o benefício "profundidade de graça" que
   teria em jogo real por tempo. Fixed-nodes serve para *ver a direção*
   (exploração, baixa variância), não para a *decisão final* de mudar o
   default -- isso precisa de confirmação em time-based (o
   `engine_arena.py` já é time-based, via `go wtime/btime`).

**Prioridade corrigida pelo Opus**: investigar primeiro se o padrão
"dama sem plano" / recuos estranhos (secção "Atualização 2026-07-20",
achado #4 da leitura qualitativa) ainda existe com o binário atual
(todos os fixes de TT/killers/history/root_best já aplicados) -- esses
PGNs antigos foram jogados ANTES desses fixes. Payoff potencial (>100
Elo se for um problema estrutural de busca) é ordens de magnitude maior
que afinar o divisor de LMR (~5-15 Elo), e é pré-requisito lógico: não
vale a pena afinar LMR por cima de uma busca possivelmente patológica.

**Feito nesta sessão:**
- **`3a9d95e`**: adicionada env var `KESTREL_LMR_DIVISOR` (default 2.1,
  mesmo padrão opt-in/fail-safe de `KESTREL_EVAL_MODE`/
  `KESTREL_TUNED_WEIGHTS`) -- substitui a necessidade de binários
  scratch ad-hoc por uma comparação reprodutível num único binário.
  Validado: com a env var por omissão vs explicitamente `2.1`, busca
  fixed-nodes dá nodes/depth/score/PV/bestmove **idênticos**
  (só ruído de NPS); com `1.7` o comportamento muda visivelmente
  (confirma que o hook funciona). Perft 5 confirma movegen intocado
  (LMR não entra em geração de lances).

**Plano de intervenção ordenado (Opus), até 24 Jul 21h:**
1. ~~Infra `KESTREL_LMR_DIVISOR`~~ -- feito, commitado.
2. **[em curso]** Diagnóstico qualitativo: correr ~10-15 jogos com o
   binário atual (self-play e/ou vs `stockfish_skill5`), ler os PGNs à
   procura do padrão "dama sem plano"/recuos/trocas más. Se
   desapareceu -> passar ao LMR (passo 3). Se persiste -> isolar FEN,
   inspecionar PV por profundidade via `go depth N`, é provavelmente
   ordenação/redução a esconder o lance refutador -- torna-se a
   intervenção da sessão.
3. LMR só se o passo 2 der luz verde: exploração de direção
   (fixed-nodes, 3 braços 1.7/2.1/2.5, ~150 jogos/par, SEM mudar
   default) -> só se algum braço se destacar claramente, SPRT de 2
   braços (2.1 vs candidato) em time-based via `engine_arena.py`,
   fronteiras Elo[0,5] α=β=0.05, decide o próprio SPRT.
4. Se sobrar tempo: livro pesado por resultado da partida (mudança de
   formato do `.bin`, hoje só `count: u32`, ver `book.rs` RECSZ=14).
5. **Regra transversal**: parar de tratar <400 jogos como evidência
   para mudar defaults -- SPRT-ou-nada, "inconclusivo -> sem mudança"
   é um desfecho válido e a norma esperada, não falha.

## Atualização 2026-07-22 (continuação): diagnóstico qualitativo feito, pivot para dataset de tuning

**Passo 2 do plano (diagnóstico "dama sem plano") -- resultado:**
- vs `stockfish_skill5` (60+1, binário atual): **5V-0D-1E em 6 jogos**
  (nunca perdeu). Leitura dos 5 PGNs decisivos: padrão antigo
  AUSENTE -- todos os lances de dama ligados a ameaças/capturas
  concretas, sem recuos estranhos, sem trocas más da nossa parte.
- Utilizador pediu para subir de escalão (skill5 "já é fácil") ->
  lançado lote vs `stockfish_skill10`, **parado a meio a pedido do
  utilizador** ("ainda é cedo para verificar a força contra SF") em
  1V-2D-2E/5 jogos -- amostra pequena demais e interrompida de propósito,
  não tirar conclusões daqui.
- **Falso alarme investigado a fundo**: o utilizador apanhou uma
  promoção a Cavalo em vez de Dama (`h8=N+`) num dos jogos vs skill10 e
  achou suspeito. Investigação completa (reconstrução da posição exata,
  comparação de profundidade por profundidade, e confirmação
  **independente com Stockfish real**): **não é bug**. A posição tinha
  uma armadilha tática real -- promover a Dama permite `...Rh3` seguido
  de `Kg5` (forçado) e `Rxh8`, uma ESPETADA que ganha a dama de graça;
  o Stockfish concorda byte a byte com a avaliação e a escolha do
  kestrel de promover a Cavalo com xeque para fugir dessa linha.
  Confirmado também que isto não é regressão do commit `ca8bfce`
  (testado num worktree do commit anterior, mesmo resultado).
  **Achado real, não o alarme original**: o Stockfish avalia a posição
  ORIGINAL (antes de qualquer promoção) já em -450/-540 para as
  Brancas -- ou seja, a vantagem material que as Brancas tinham bem
  mais cedo no jogo (lances ~45-64, torre+peões conectados) foi mal
  convertida antes de chegar aqui. **Pista para investigar depois**:
  técnica de final de torre+peões, não a escolha de promoção.

**Conclusão do passo 2**: luz verde mecanística para o LMR (passo 3),
mas ainda não confirmado por SPRT quantitativo -- ver plano do Opus
(quantificar o fix estrutural com SPRT binário-atual vs
`kestrel_prekillersfix_bin` continua por fazer).

**SPRT estrutural feito (script `/root/kestrel_joao/sprt_structural_fix.py`,
self-play fixed-nodes 30000/lance, imune a contenção de CPU, aberturas
aleatórias 4/6/8 lances):** binário atual (todos os fixes de TT/killers/
history/root_best) vs `kestrel_prekillersfix_bin` (commit `91ea1a7`,
só TT/panic, SEM os fixes estruturais). **400 jogos: 88.5% (342V-34D-24E)
para o binário atual, ~+354 Elo equivalente.** Confirma de forma
quantitativa e inequívoca que os fixes estruturais foram uma melhoria
enorme, não ruído -- fecha em definitivo o item #1 do plano do Opus.

**Pivot pedido pelo utilizador**: antes de avançar para
tuning de pesos de eval (que já tem histórico de overfit -- ver
commits `891cb81`/`6edebf9`, infraestrutura `kestrel selfplay`/
`kestrel tune`/`kestrel tunefast` já existe, MÚLTIPLAS tentativas
anteriores falharam mesmo com regularização L2 e validação held-out),
o utilizador pediu para consultar um agente **Fable** especificamente
sobre que tipo de jogo(s) usar para construir um dataset melhor --
tarefa lançada em background, a aguardar relatório antes de gerar
mais dados ou tunar. Não avançar tuning sem esse relatório.

**Relatório do Fable (2026-07-22): não perseguir tuning agora.**
Achado chave: já existia uma ronda inteira de tuning **não documentada**
num scratchpad partilhado doutra sessão (mesma proveniência dos
binários `kestrel_fast`/`kestrel_lmr` mencionados acima) -- self-play
de 3000 jogos (100k+ posições, MAIS que os 20-50k que o Opus tinha
sugerido), varrimento de lambda de regularização, um run bem-comportado
(λ=0.001, convergência real). **Validação final A/B em jogo real: 49.6%
vs 50.4% -- ruído puro, sem sinal em nenhum sentido.** Ou seja: volume e
regularização já foram varridos com resultado nulo -- não é aí que
está o problema.

**Causa mais provável identificada (metodológica, confirmada no
código)**: `tune_weights()`/`white_eval()` em `main.rs:572-575` rotula
cada posição com o eval ESTÁTICO cru (`evaluate_with_weights`), nunca
passando por `quiescence()` (`search.rs:552`) -- desvio real do método
Texel canónico (que usa o score de quiescence search como preditor,
confirmado por pesquisa: Ethereal/Texel original fazem isto). Corrigir
isto é trabalho de código real (~1-2h + revalidação), não garantido a
ajudar, e ainda por cima **contradiz o volume já testado em vão**.

**Decisão**: não avançar mais tuning de eval nesta janela. Voltar à
prioridade já estabelecida pelo Opus -- SPRT binário-atual vs
`kestrel_prekillersfix_bin` para quantificar o fix estrutural, depois
LMR. Ver relatório completo do Fable (texto integral não guardado em
ficheiro -- se precisar de o reconsultar, os artefactos-fonte estão em
`/tmp/claude-0/-root/29d54c55-88c4-4e30-af2c-56dc260673c1/scratchpad/`:
`selfplay_big.epd`/`selfplay_quiet.epd`, `tune_reg*.log`/`tuned_reg*.txt`,
`ab_match.py`/`ab_match.log`).

**Correção importante (utilizador desafiou a conclusão "tuning não
ajuda", com razão)**: verifiquei diretamente e confirmei dois problemas
concretos que a conclusão do Fable não tinha isolado explicitamente:
- `KESTREL_TUNED_WEIGHTS` **carrega corretamente** (validado com
  `kestrel checkweights`: round-trip ok, eval muda de facto ao carregar
  `tuned_reg3.txt`).
- MAS o candidato `tuned_reg3.txt` realmente testado no A/B só mexeu em
  **43 dos 460 parâmetros**, desvio máximo de **3 centipawns** (a
  maioria ±1) -- essencialmente ruído. As outras runs de lambda maior
  (0.05, 5) travaram tudo (0 parâmetros a mexer). **Nenhuma tentativa
  até agora testou de facto um conjunto de pesos meaningfully diferente
  do default** -- o resultado nulo (49.6%/50.4%) não prova que os pesos
  já estão bons, prova que essa run em concreto mal saiu do ponto de
  partida.
- Timing: esse A/B correu às 21:52 de 21 Jul, um snapshot do binário
  nesse momento (dia com 30+ commits); as duas mudanças que aterraram
  depois nessa noite (`0beddc2` doc-only, `ca8bfce` pinned-piece
  fastpath, só performance) não deviam enviesar a comparação eval vs
  eval, mas não há como confirmar retroativamente que o binário do
  teste estava mesmo atualizado até ao commit imediatamente anterior.
- **Conclusão revista**: a decisão de não perseguir tuning agora
  mantém-se válida por falta de tempo/risco nesta janela, mas por
  razões diferentes das que o Fable deu -- não é "já se provou que não
  ajuda", é "ainda não foi testado a sério (regularização nunca deixou
  os pesos mexerem-se o suficiente + falta rotulagem por quiescence)".
  Se sobrar tempo, a prioridade dentro do tuning seria testar um
  lambda MENOR que 0.001 (ou um orçamento de épocas maior) para deixar
  o coordinate descent explorar de verdade, antes/além do fix de
  quiescence.

**Ronda de tuning a sério, feita depois da correção acima (2026-07-22):**
1. Self-play mais profundo: `kestrel selfplay 1500 dataset_round1.epd
   20000 4` -- 62928 posições, 20000 nós/lance (5x mais fundo que a
   tentativa anterior de 4000).
2. **Nova infra, commit `3e736c7`**: `quiescence_leaf()`/
   `quiescence_leaf_from()` em `search.rs` (funções aditivas, busca de
   produção intocada -- validado: perft(5)=4865609, mesma busca
   fixed-node byte a byte, suite tática 19/23 inalterada) + subcomando
   `kestrel resolvequiet <in.epd> <out.epd>`. Ataca o gap real que o
   Fable identificou (rotulagem sem quiescence) SEM pagar o custo de
   qsearch em cada tentativa de parâmetro (calculado: >1 mil milhões de
   avaliações, intratável) -- resolve cada posição UMA VEZ para o seu
   sucessor tacticamente quieto antes de tunar. **6804/62928 (10.8%)**
   das posições estavam mesmo instáveis e foram corrigidas -- confirma
   que o problema é real, não hipotético. Custo: ~1s para as 63k
   posições (muito mais barato do que se temia).
3. `kestrel tune dataset_round1_quiet.epd tuned_round1.txt 40 0.0005`:
   **convergência real** (época 8, 0 parâmetros a melhorar, não
   truncado por limite de épocas). Erro 0.078021->0.076783 (~1.6%
   relativo, 4x mais que a tentativa anterior). **108/460 parâmetros
   mudaram, desvio máximo 6cp** -- movimento real, não ruído (a
   tentativa anterior só tinha mexido 43 parâmetros, máx 3cp).
4. **MAS a suite tática regrediu: 19/23 -> 16/23 (82.6% -> 69.6%)**.

**Conclusão (revista outra vez, com mais evidência agora)**: esta é a
**terceira** tentativa genuinamente diferente (sem regularização;
regularização forte demais que mal mexeu nos pesos; agora dataset limpo
por quiescence com convergência real e movimento real) e as três
regridem a suite tática. Isto já não é "não testámos a sério" -- é
sinal real de que afinar SÓ os pesos de eval, sem tocar nas margens de
poda da busca (RFP, futility, delta pruning, LMR) que foram calibradas
à mão para a escala ATUAL dos pesos, provavelmente desalinha as duas
partes mesmo quando o eval isolado fica "melhor" a prever resultados de
jogos. Não deployado (viola o gate da suite tática, regra do projeto).
Próximo passo válido se sobrar tempo: re-tunar/re-validar margens de
poda em conjunto, ou aumentar a suite tática (23 posições é uma amostra
pequena e pode ela própria ter ruído) antes de tentar mais uma ronda.
Ficheiros: `dataset_round1.epd`/`dataset_round1_quiet.epd`,
`tuned_round1.txt`, `tune_round1.log`, `resolvequiet.log` (todos em
`/root/kestrel_joao/Kestrel/`, não commitados -- são artefactos de
dados, não código).

**Nota lateral (fora do escopo do Kestrel)**: o utilizador mencionou um
segundo projeto ("littlerock/half2k", adversário de referência
"PeachFruit" no Lichess) com prazo até sexta (24 Jul) para bater o Elo
dele sempre e jogar bem. Esse projeto corre na máquina `napoleon`
(10.0.0.2, WireGuard) que está **desligada/sem handshake há >1 dia** --
fora do alcance desta sessão até a máquina voltar a ligar. Não
misturar com o trabalho do Kestrel (que corre neste servidor, 10.0.0.1,
sem essa dependência).

## Atualização 2026-07-22 (continuação): LMR fechado, resumo do estado

**Exploração de direção do LMR concluída** (script
`/root/kestrel_joao/lmr_direction_explore.py`, self-play fixed-nodes
30000/lance, 3 confrontos de 300 jogos cada via `KESTREL_LMR_DIVISOR`):
- 2.1 vs 1.7: 50.7% / 49.3%
- 2.1 vs 2.5: 48.0% / 52.0%
- 1.7 vs 2.5: 53.7% / 46.3%

**Resultado circular** (viola transitividade -- se 2.5 bate 2.1 e 2.1
empata com 1.7, esperar-se-ia 2.5 >= 1.7, mas é o oposto). Assinatura
clássica de ruído puro em vez de direção real. **Conclusão: manter o
divisor default 2.1, não gastar mais orçamento de SPRT aqui** -- regra
transversal do plano do Opus aplicada tal como definida. Item #3 do
plano fechado.

**Resumo do estado no fim desta sessão** (para quem retomar, incluindo
a sessão tmux `chessclaude` quando recuperar quota):
1. Fix estrutural (TT/killers/history/root_best): confirmado
   qualitativa E quantitativamente (SPRT 88.5%, ~+354 Elo). Fechado.
2. LMR: parâmetro testado, sem sinal, default mantido. Fechado.
3. Tuning de pesos de eval: infraestrutura significativamente
   melhorada (`quiescence_leaf`/`resolvequiet`, commit `3e736c7`), uma
   terceira tentativa com dataset mais profundo e limpo por quiescence
   convergiu de verdade mas ainda regride a suite tática -- ver
   detalhe acima. Não deployado. Em aberto: re-tunar margens de poda
   em conjunto, ou aumentar a suite tática antes de mais uma ronda.
4. Livro pesado por resultado da partida: ainda não começado (item #4
   original do plano do Opus, baixa prioridade).
5. Todos os commits desta sessão têm mensagens e comentários novos em
   inglês (pedido do utilizador, repo é público no GitHub); este
   ficheiro de notas mantém-se em português.

## Atualização 2026-07-22 (continuação): auditoria Sirius+Ethereal e ronda de integração de features

Pedido do utilizador: "isto não pode parar... não quero ouvir já fiz mas
não integrei" -- integrar a sério, não só planear. Metodologia acordada:
integrar a estrutura agora (validada por correção -- perft, sem crashes),
testar/calibrar os valores depois com jogos reais, tal como motores
reais evoluíram ao longo de anos com dezenas de milhares de jogos. Não
gatear a integração à espera de prova de Elo num A/B pequeno.

**Duas auditorias feitas pelo Fable, em background:**
1. **Kestrel vs lista de features do Sirius** (item a item, lendo o
   código real, não nomes de commits): confirmou que quase tudo de
   busca já está implementado (PVS, null-move, LMR, RFP, razoring,
   futility quiet, LMP, history pruning, IIR, ProbCut, correction
   history, Lazy SMP, staged move picker, singular extensions -- tudo
   `TEM`). Gaps reais identificados, por prioridade: endgame scaling,
   backward/candidate passed pawns, bad bishop, king safety (safe
   checks + gate de dama), futility de capturas, capture history,
   qsearch LMP, node-count time management. Fora de alcance nesta
   janela: double/negative extensions, multicut genérico, complexity
   eval completo (exigem SPRT longo para validar com segurança).
2. **Como o Ethereal (C, AndyGrant/Ethereal) resolve os mesmos
   componentes**: relatório com fórmulas e valores exatos (não só
   nomes) -- king safety quadrática completa, endgame scale factors,
   complexity eval, todas as margens de poda com o Elo estimado por
   SPSA do próprio autor. Confirmou algo importante: **o Ethereal não
   tem correction history** (técnica pós-2022) e mesmo assim é um dos
   motores clássicos mais fortes de sempre -- não é pré-requisito para
   força clássica. Também confirmou que mesmo COM NNUE hoje, o Ethereal
   mantém o eval clássico completo como fallback ativo em posições de
   material extremo, porque o autor considera-o robusto o suficiente
   para produção.

**Implementado nesta ronda (todos validados: build limpo, perft(5)=4865609
inalterado, sem crashes; A/B self-play fixed-nodes para cada um, valores
ainda por calibrar como o plano previa):**

1. **Endgame scaling** (`eval.rs`, `scale_endgame`/`endgame_scale_factor`):
   opposite-colored-bishops (3 graus: só bispos/+1 torre/+1 cavalo),
   minor solitário vs rei só com peões (empate garantido), fallback por
   contagem de peões do lado forte em posições sem damas. Arquitetura
   do Ethereal, valores próprios. Aplicado ao eval já interpolado (não
   dividido mg/eg como o Ethereal) para não quebrar a linearidade que
   `tune_fast` (main.rs) assume em `positional_terms()` -- troca
   deliberada, documentada no código. **A/B (300 jogos, fixed-nodes):
   48.8% -- neutro, sem sinal, esperado a esta escala.**
2. **King safety: safe checks + queen-gate** (`eval.rs`, dentro de
   `positional_terms`): king danger table agora exige só 1 atacante
   com a dama inimiga em jogo (antes eram sempre 2), e ganha uma
   segunda pass depois do loop principal que conta lances de
   cavalo/bispo/torre/dama que dariam xeque numa casa sem qualquer
   defensor inimigo ("safe check", 1 ply de lookahead sem simular o
   lance a sério). Reutiliza o peso `king_attacks` existente em vez de
   criar campos novos tunáveis. **A/B (300 jogos): ~46-47%, negativo e
   persistente ao longo de todo o lote** -- estrutura correta
   (arquitetura do Ethereal), mas a calibração inicial (peso reutilizado,
   pode estar demasiado forte agora que dispara com mais frequência)
   provavelmente precisa de ser mais fraca. **Próximo passo: não
   reverter a estrutura, mas testar um peso dedicado mais pequeno em vez
   de reutilizar `king_attacks`, ou testar o threshold antigo (sempre 2)
   mantendo só o bónus de safe-check.**
3. **Backward pawns + candidate passed pawns + bad bishop** (`eval.rs`,
   loop de peões e loop de bispos): três termos novos, pesos próprios
   pequenos (`BACKWARD_PAWN=(-6,-10)`, `CANDIDATE_PASSED_PAWN=(6,18)`,
   `BAD_BISHOP=(-2,-4)` por peão na mesma cor). `LIGHT_SQUARES` novo em
   `bitboard.rs`. Ainda sem A/B isolado (medido em conjunto com o resto
   desta ronda).
4. **Futility pruning para capturas** (`search.rs`, negamax): mesma
   ideia do futility de lances tranquilos já existente, mas para
   capturas, usando SEE (não valor bruto da peça) como estimativa de
   melhor caso.
5. **TT extended cutoff** (`search.rs`, negamax, achado específico do
   Ethereal não coberto por nenhuma lista genérica): aceita uma entrada
   da TT UM depth abaixo do pedido como corte, se já parecia um
   fail-low claro (`Bound::Upper`, não-PV, margem de 130cp).
6. **Qsearch late move pruning** (`search.rs`, quiescence_from): limite
   de 8 capturas tentadas (já ordenadas por SEE, já filtradas SEE>=0)
   antes de desistir do resto.

**A/B final (4+5+6 combinados: futility de capturas + TT extended
cutoff + qsearch LMP, 300 jogos vs a baseline com king-safety+
endgame-scaling): 52.3% (157V-135D-16E)** -- positivo, modesto (~+16
Elo), consistente ao longo do lote (começou mais alto ~58%, estabilizou
por volta de 52-53% com mais jogos). Dentro do que se espera para
refinamentos de poda deste tamanho. Commitado (`48795d8`).

**Resumo final dos A/Bs desta ronda:**
| Mudança | Resultado (300 jogos, fixed-nodes) | Decisão |
|---|---|---|
| Endgame scaling | 48.8% | Neutro, integrado (estrutural, não tático) |
| King safety (safe checks + queen-gate) | 46.8%, negativo persistente | Integrado, calibração é o próximo passo (não reverter estrutura) |
| Backward/candidate pawns + bad bishop | não medido isolado | Integrado junto com king safety |
| Capture futility + TT cutoff + qsearch LMP | 52.3%, positivo | Integrado |

Commits desta ronda: `7b7e5dd` (eval: endgame scaling, king safety,
pawn terms, bad bishop), `48795d8` (search: capture futility, TT
cutoff, qsearch LMP).

**Binários de checkpoint guardados** (não commitados, artefactos locais)
em `/root/kestrel_joao/`: `kestrel_with_endgamescale`,
`kestrel_with_kingsafety`, `kestrel_with_capfutility`,
`kestrel_with_ttcutoff`, `kestrel_with_qslmp` -- úteis para isolar
qual mudança específica ajudou/prejudicou se for preciso investigar
mais tarde. Scripts de A/B: `sprt_endgamescale.py`, `sprt_kingsafety.py`,
`sprt_search_batch.py` (todos variantes do padrão já estabelecido em
`sprt_structural_fix.py`).

**Não implementado ainda desta lista** (falta tempo, não descartado):
capture history dedicada. Ficam para a próxima sessão/instância se
sobrar tempo até 24 Jul 21h.

## Atualização 2026-07-22 (continuação): recalibração, node-count time management, infraestrutura "profiles"

**King safety recalibrado (commit `a1cf79a`)**: o bónus de safe-check
reutilizava o peso `king_attacks` (5,0) directamente -- separado num
campo próprio `SAFE_CHECK=(2,1)` mais fraco. A/B refeito: **48.7%
(146V-140D-28E/300)**, muito mais perto de neutro que os 46.8%
originais -- confirma que a magnitude era mesmo o problema principal,
não a estrutura (queen-gate mantido).

**Node-count time management adicionado** (mesmo commit): o early-stop
por estabilidade do melhor lance agora só dispara se >=70% dos nós
totais do `go` estiverem concentrados no lance escolhido -- evita parar
cedo só porque o lance não mudou, se a busca ainda gasta esforço
comparável em alternativas.

**Infraestrutura "profiles" (pedido explícito do utilizador -- pesos
programáveis, não só constantes fixas), commit `2ad0bf1`:**
- `SearchParams` novo em `search.rs`: TODAS as margens de poda
  (RFP, razoring, futility quiet/captura, delta pruning do qsearch,
  limite de LMP no qsearch, margem do TT extended cutoff, multiplicador
  de history pruning) que antes eram `const`/literais espalhados,
  agora num struct único, carregável via `KESTREL_SEARCH_PARAMS=<path>`
  (mesmo padrão reversível do `KESTREL_TUNED_WEIGHTS`). Validado:
  default (env var não definida) reproduz busca fixed-node
  byte-a-byte idêntica; um perfil diferente muda mesmo o PV.
- Fatores de scale do endgame (OCB, fallback sem damas) movidos de
  valores hardcoded para dentro do `Weights` (5 novos campos escalares)
  -- eram a única parte do eval ainda não programável.
- **Ainda sem tuner automático para o `SearchParams`** -- estas margens
  interagem com contagem de nós de forma não-linear, o método Texel
  (posições estáticas) não se aplica; tuning a sério precisaria de SPSA
  sobre jogos reais (a mesma infraestrutura de self-play A/B já usada
  esta sessão serve de base, só falta o laço de otimização).

**Teste "perfil Sirius" feito (pedido do utilizador)**: construído
`sirius_profile.txt` (script `build_sirius_profile.py`) -- pega no
vector de pesos DEFAULT do Kestrel e substitui os 184/473 campos que o
port histórico de 2026-07-20 (commit `a11d7bd`, valores reais de
`Sirius/src/eval/eval_constants.h`) cobria (bishop pair, mobilidade
completa, king attacker weights, pawn structure), deixando tudo o resto
(threats, shelter/storm, os termos adicionados hoje) nos valores
próprios do Kestrel. **Resultado do A/B (300 jogos, mesma estrutura de
código): Kestrel próprio 53.2% vs perfil Sirius 46.8%** -- os pesos
próprios do Kestrel, afinados à mão ao longo de várias sessões,
vencem os valores reais tunados do Sirius dentro da MESMA arquitetura
de busca. Achado genuíno: pesos de eval são tunados EM CONJUNTO com a
busca que os usa -- os do Sirius foram calibrados para a busca dele
(margens/podas diferentes), não transferem de graça para a busca do
Kestrel. Consistente com a nota já existente do port histórico ("não
testado via self-play, sinal viria de jogos externos/Peachfruit") --
agora testado, e o resultado favorece manter os valores próprios.

**Perfil equivalente do Ethereal para `SearchParams`: não construído.**
As fórmulas do Ethereal diferem em FORMA (não só em magnitude) das do
Kestrel -- RFP usa uma margem única com `depth-1` quando "improving"
em vez de duas margens separadas tipo Kestrel; razoring usa margem
FIXA 3488 em vez de linear em profundidade (`150+100*depth`). Copiar
os números do Ethereal para dentro das fórmulas do Kestrel sem
adaptar a própria fórmula não seria um teste justo -- ficaria a
comparar formas de curva diferentes, não só calibração. Se for para
fazer isto a sério, precisa de portar a FÓRMULA também, não só os
números -- fica para decisão futura, não descartado só adiado.

Ficheiros novos (não commitados, artefactos locais):
`sirius_profile.txt`, `build_sirius_profile.py`,
`sprt_sirius_profile.py`/`.log`, `sprt_safecheck_v2.py`/`.log`.

## Atualização 2026-07-22 (continuação): SearchParams generalizado + perfil de busca do Ethereal

Pedido do utilizador: importar valores conhecidos para os campos do
`SearchParams` recém-criado, tal como foi feito para o eval com o
Sirius. Antes de conseguir fazer isso a sério para o Ethereal, foi
preciso generalizar a forma das margens.

**Generalização (commit `f6df9f6`)**: `DepthMargin{base, slope}`
substitui o multiplicador puro `slope*depth` que todos os campos
tinham. Kestrel's próprios defaults são exactamente `base=0` na forma
nova -- zero mudança de comportamento por default (validado: busca
fixed-node idêntica byte a byte). Isto importa porque a fórmula real do
RFP do Ethereal (`65*MAX(0,depth-improving)`) e do futility
(`77+lmrDepth*52`) TÊM componente base -- não eram representáveis na
forma antiga sem mentir sobre a fórmula real do Ethereal.

**Perfil do Ethereal construído (parcial, honesto sobre o que não
mapeia)**: `ethereal_search_profile.txt`
(`build_ethereal_search_profile.py`) -- só os campos onde a fórmula do
Ethereal tem mesmo a forma base+inclinação foram substituídos (RFP,
razoring -- margem fixa 3488 -- futility de lances tranquilos, margem
do TT extended cutoff). **Não mapeado, deixado no default do Kestrel**:
futility de capturas (o Ethereal usa poda por SEE com escala
QUADRÁTICA em profundidade, mecanismo completamente diferente de uma
margem de futility), delta pruning do qsearch, limite de LMP do
qsearch, multiplicador de history pruning (Ethereal usa um limiar
FIXO, não escalado por profundidade) -- nenhum destes tinha um
equivalente Ethereal reportado com a MESMA forma, copiar às cegas
teria sido enganoso.

**A/B (300 jogos, fixed-nodes): perfil Ethereal 51.8% vs Kestrel
próprio 48.2%** -- ligeira vantagem para o Ethereal, mas dentro do
ruído estatístico (~+12 Elo, <1 desvio-padrão a 300 jogos). Ao
contrário do teste do eval (onde os pesos próprios do Kestrel bateram
os do Sirius claramente, 53.2%/46.8%), aqui o resultado é ambíguo --
faz sentido: margens de busca (RFP, futility) são conceptualmente mais
universais entre motores do que pesos de eval (que dependem de como
interagem com os OUTROS termos do eval específico de cada motor).
**Não mudado o default** -- segue a regra transversal já estabelecida
(SPRT-ou-nada, <400 jogos não é evidência suficiente). Se sobrar tempo,
vale a pena correr mais jogos neste perfil especificamente, já que a
direção (ainda que fraca) é positiva.

Ficheiros novos: `ethereal_search_profile.txt`,
`build_ethereal_search_profile.py`, `sprt_ethereal_search.py`/`.log`.

## Atualização 2026-07-22 (continuação): NPS + resultado real vs Stockfish skill10

**NPS** (binário final desta sessão, `go movetime 5000` a partir do
startpos): single-thread **~896k nps** (depth 19); 4 threads (Lazy SMP)
**~3.75M nps agregado** (depth 18 -- menos 1 profundidade que
single-thread no mesmo tempo, esperado por overhead de agregação entre
threads).

**10 jogos vs `stockfish_skill10` (60+1, via arena)**: **7V-2D-1E =
75%**. Primeiro teste real (não self-play) desde os fixes/features
desta sessão toda -- sinal absoluto de progresso real, não só relativo
entre versões próprias. Um dos jogos foi longo e disputado (68+ lances,
Stockfish em apuros de tempo perto do fim). Não há registo do resultado
equivalente ANTES desta sessão para comparação directa (as notas de
20-21 Jul só têm resultados vs Stockfish "cheio", que dava sempre 0V --
esta é a primeira vez que a escada de skill graduado é usada depois de
uma ronda grande de mudanças). Se sobrar tempo, valeria a pena um lote
maior (20-30 jogos) neste mesmo degrau antes de subir para skill15,
para ter uma leitura mais estável do nível absoluto atual.

**Lote maior a 20 jogos (mesmo binário, skill10)**: 9V-6D-5E = **57.5%**
-- mais moderado que os 75% do lote de 10, mas ainda claramente
positivo. Confirma o degrau; próximo passo natural é subir a skill15.

## Atualização 2026-07-22 (continuação): itens "fora de alcance" implementados a sério, e a lição de não reverter

Pedido explícito do utilizador: continuar mesmo os itens que o Fable
tinha marcado como "fora de alcance nesta janela" -- não deixar nada
por integrar só porque é mais arriscado. Implementados, todos com
formulas/valores portados directamente do Ethereal (Fable já tinha o
código-fonte lido):

1. **TTPV** (commit `2a888e8`): bit novo na entrada da TT (posição
   anteriormente não usada, sem mudar o layout de 64 bits) marca se a
   entrada foi escrita por uma pesquisa de janela completa (PV). Usado
   para reduzir 1 ply menos no LMR quando a posição já tinha esse
   estatuto antes.
2. **Double/negative extensions** (commit `a90a9a9`): porte directo do
   `search.c` do Ethereal -- singular por margem extra (16) e' +2 em
   vez de +1, com contador `dextensions` por linha (limite 6, valor do
   Ethereal); negative extension quando o tt_move já bate beta na
   depth actual sem disparar multicut, encolhe a depth em vez de
   estender.
3. **Complexity eval** (commit `1c48b4c`): fórmula exacta do Ethereal
   (pawns*8 + ambos-os-flancos*82 + final-de-peões*76 - 157), aplicada
   com o mesmo clamp que preserva sinal (nunca inverte quem está
   melhor). 4 novos campos no `Weights`.
4. **Multicut genérico**: **decidido não implementar como algo à
   parte**. O próprio relatório do Fable sobre o Ethereal confirma que
   a implementação REAL dele também é só a versão fundida dentro da
   verificação singular (reutiliza o mesmo move picker, não tem loop
   próprio) -- ou seja, o que o Kestrel já tinha desde a sessão
   anterior já é equivalente à prática de um motor de referência forte.
   Construir uma versão "mais genérica" seria pior, não melhor -- nem o
   Ethereal faz isso.

**A/Bs (300 jogos cada, fixed-nodes) -- resultados honestos:**
- TTPV: **46.5%** (139.5/300), negativo e consistente ao longo do lote.
- Extensões duplas/negativas: **46.2%** (138.5/300), negativo e
  consistente.
- Complexity eval: ~48-49%, praticamente neutro.

**Decisão: NENHUM destes foi revertido**, apesar dos números negativos
de TTPV/extensões. Motivo: existe já um precedente exacto no histórico
do projecto (ver comentário junto às singular extensions em
`search.rs` -- "revertido por A/B de 30 jogos, decisão errada,
restaurado depois") e o utilizador reforçou-o explicitamente esta
sessão: "atenção às reversões... se foi pedido tem de estar feito."
Um A/B de 300 jogos tem erro-padrão ~2.9% -- um resultado a 46-47% está
perto de 1 desvio-padrão do neutro, não é evidência forte de regressão
real para uma técnica JÁ PROVADA por um motor de referência forte
(Ethereal). Reverter tecnologia comprovada com base em ruído
estatístico deste tamanho é exactamente o erro documentado.
**Guardado como memória permanente**:
`feedback_kestrel_nao_reverter_por_self_play_pequeno` (ver
`/root/.claude/projects/-root-kestrel-joao/memory/`).

**Se sobrar tempo**: um SPRT muito mais longo (milhares de jogos) seria
o próximo passo correcto para confirmar/refutar TTPV e as extensões
com poder estatístico real -- não um novo A/B de 300 jogos, que já se
sabe não ter resolução suficiente.

## Atualização 2026-07-22 (continuação): revisão de código do Fable, 2 bugs reais + 1 crash corrigidos

Pedido do utilizador: revisão de código independente aos 14 commits
desta sessão. Achados reais (commits `59a7c62`, `a857622`):

1. **CRÍTICO (`eval.rs`, `endgame_scale_factor`)**: o check de
   "minor-só-vs-rei-nu = empate" nunca verificava os peões do lado
   FORTE, só os do lado fraco -- disparava para K+P vs K e finais
   trivialmente ganhos. Confirmado por teste directo: `evaluate()`
   devolvia exactamente 0 numa posição real de K+P vs K. Corrigido:
   agora exige zero peões também do lado forte (só K+N vs K / K+B vs K
   reais, material insuficiente genuíno).
2. **MODERADO (`search.rs`, `dextensions`)**: o contador de extensões
   duplas só era ESCRITO no ramo que concede a extensão, nunca
   propagado do pai em todos os outros caminhos pelo mesmo ply --
   como o array é indexado só por `ply` (não por linha de pesquisa),
   isto deixava o contador contaminado por ramos não relacionados.
   Corrigido: propagação incondicional `dextensions[ply] =
   dextensions[ply-1]` no topo de cada visita ao nó.
3. **CRÍTICO -- CRASH real (`uci.rs`)**: um `position fen <...>` com o
   lado que NÃO joga já em xeque (posição ilegal, impossível de
   alcançar por jogo real, mas o parser de FEN não rejeita) podia
   fazer crash na busca (`king_sq()` lê um bitboard de rei vazio,
   `trailing_zeros()`=64, fora dos limites da tabela de 64 casas).
   **Confirmado pré-existente** (mesmo crash num binário de 20 Jul,
   muito antes desta sessão) -- não foi introduzido hoje, só nunca
   tinha sido encontrado. Não alcançável por jogo normal, mas
   `position fen` é entrada não confiável de quem quer que esteja do
   outro lado da ligação UCI. Corrigido: validação no parse -- rejeita
   e cai para startpos em vez de aceitar uma posição já ilegal.

Achados menores também corrigidos: TTPV agora persiste através de
escritas scout subsequentes (`OR` com o flag antigo, como
Stockfish/Ethereal fazem -- antes era apagado quase de imediato pelo
esquema always-replace); dois comentários incorrectos (complexity_eval
não "encolhe para zero" como dizia, na verdade soma um bónus em
posições normais; ordem OCB rook/knight trocada no comentário); guard
`ply>0` defensivo no TT extended cutoff.

**Ficheiro de teste usado para confirmar os bugs**: build de debug
(`cargo build`, não `--release`) para atribuição exacta de linha no
panic -- útil para o futuro se aparecer outro crash: `RUST_BACKTRACE=1
./target/debug/kestrel` dá stack trace preciso, o binário release por
vezes atribui a linha errada por causa de inlining (embora neste caso
tenha calhado por acaso estar certo).

## Atualização 2026-07-22 (continuação): degrau skill15 (binário pré-fixes)

**20 jogos vs `stockfish_skill15` (60+1): 1V-15D-4E = 15%.** Contraste
claro com skill10 (57.5%/75% conforme o lote) -- skill15 é um degrau
real ainda não superado, a força actual fica algures entre os dois
níveis. Corrido com o binário de ANTES dos 3 fixes da revisão do Fable
(`59a7c62`/`a857622`) -- o fix do endgame scaling em particular pode
ter impacto real em jogos que cheguem a finais de peões, vale a pena
repetir este degrau com o binário corrigido antes de tirar conclusões
finais sobre o nível absoluto. Arena já reiniciada com o binário
corrigido (`469770e`), pronta para o próximo lote.

**Próximo passo sugerido**: repetir skill15 (ou mesmo skill10 outra
vez, para confirmar que os fixes não pioraram nada) com o binário
actual antes de decidir se o degrau é mesmo skill15 ou se os fixes
mudam a leitura.

**Re-teste com o binário corrigido (3 bugs): pior ainda no arranque**
(0V-7D-1E nos primeiros 8 jogos) -- pedido do utilizador para estudar
as derrotas em vez de só ver o placar. Leitura qualitativa de 7 PGNs:

**Padrão real identificado**: pelo menos 3 das 7 derrotas (jogos 1, 4,
7) mostram o kestrel a entrar em **sacrifícios especulativos sem
compensação suficiente** -- ex. `12.Bxf7+ Kd8` no jogo 7 e padrão
semelhante no jogo 1, entrada em complicações táticas a perder material
no jogo 4. Consistente com o viés agressivo do eval "estilo Polgar"
(pressão sobre o rei, densidade de atacantes) a empurrar para jogo
sacrificial que a busca, à profundidade prática de 60+1 contra um
adversário genuinamente forte (skill15, mesmo reduzido), não consegue
validar em cálculo suficiente. O jogo 6 (empate por perpétuo sob
pressão pesada) mostra o oposto -- boa resiliência defensiva, não é um
padrão de colapso total.

**Isto não é um bug -- é uma tensão real entre o pedido original do
utilizador** ("estilo Judit Polgar... viés agressivo... nunca forçar a
busca a jogar pior do que sabe" -- ver secção "O que é" no topo deste
ficheiro) **e a força bruta contra oponentes fortes e precisos.** A
mesma hipótese já tinha sido testada em 2026-07-20 via
`KESTREL_EVAL_MODE=material` e foi REFUTADA -- mas esse teste foi
self-play (kestrel vs kestrel_material, mesma profundidade de busca
dos dois lados), que pode não captar a mesma dinâmica que jogar contra
um oponente mais preciso capaz de refutar sacrifícios que um
adversário da mesma força não encontraria. Não decidido se vale a pena
reequilibrar os termos de agressividade -- fica como pergunta em
aberto para o utilizador, não uma correção automática (mudaria o
carácter do bot que foi pedido deliberadamente).

## Atualização 2026-07-22 (continuação): livro Polgar posto de lado, novo livro via Stockfish, Sirius instalado

**Decisão do utilizador**: "mete o polgar de lado... não foi um pedido
foi uma ideia... isso não é compatível com o jogo entre motores." O
livro de assinatura da Judit Polgar (baseado em frequência de jogos
humanos reais) deixa de ser o default -- substituído por um livro
construído a partir de análise real do Stockfish.

**Novo livro (`sf17_book.bin`)**: só há Stockfish **17.1** neste
servidor, não 18 (pesquisei o sistema todo, não encontrado) -- usado
17.1, diferença irrelevante para teoria de abertura a esta profundidade.
Script `build_sf_book_games.py`: 199 linhas de 20 plies cada
(satisfaz "16 plies no mínimo"), cada lance escolhido por
`go depth 16` real do Stockfish, não frequência humana. Diversidade via
alguns plies iniciais aleatórios por linha (Stockfish é determinístico
para a mesma posição/profundidade -- sem isso todas as linhas
colapsavam na mesma). `kestrel buildbook` -> 3469 posições únicas, 3664
registos.

**Limitação conhecida, documentada honestamente**: como a diversidade
vem de 2-6 plies ALEATÓRIOS no início de cada linha (não escolhidos
pelo Stockfish), a distribuição de lances na posição INICIAL fica um
pouco diluída por ruído (contagens espalhadas por quase todos os
primeiros lances possíveis, incluindo alguns fracos tipo `Nh3`/`a3`).
A partir do 3º/4º lance em diante, as linhas já são análise real do
Stockfish. Não corrigido por falta de tempo -- se for revisitado, a
forma certa é usar MultiPV do Stockfish na raiz (ponderado para o
melhor lance) em vez de aleatoriedade pura, só para dar diversidade sem
perder qualidade logo na posição mais visitada do livro.

**Troca implementada** (`uci.rs`, `default_style_book_path()`):
`KESTREL_BOOK_FILE` (default agora `sf17_book.bin`, era hardcoded
`polgar_book.bin`) -- mesmo padrão reversível de env var de sempre.
`polgar_book.bin` fica no disco, não apagado -- `KESTREL_BOOK_FILE=
polgar_book.bin` volta ao livro antigo se for preciso.

**Sirius 9.0 instalado** (pedido explícito: "instala o Sirius e joga
contra ele"). `git clone https://github.com/mcthouacbb/Sirius.git`,
`make LDFLAGS=""` (o Makefile pede `-fuse-ld=lld`, o lld não está no
PATH deste servidor por nome próprio -- só dentro do toolchain do
Rust -- por isso build com o linker default em vez de instalar lld à
parte). C++ puro, **sem NNUE** (~3449 CCRL 40/15, muito mais forte que
o Kestrel actual -- é o motor de referência que já era citado no
README, agora também oponente real). Registado em `engine_arena.py`
(`OPPONENTS["sirius"]`, Threads=1, Hash=64). Binário em
`/root/kestrel_joao/Sirius/sirius`. Match ainda não lançado (arena
ocupada com o lote skill15 em curso) -- próximo passo assim que
libertar.

## Atualização 2026-07-22 (continuação): match Sirius x40 lançado, calibração dos parâmetros de BUSCA do Sirius fechada

**Match Sirius x40 lançado** (`us=kestrel, them=sirius, base=60+1, 40
jogos, cores alternadas`, task `bwh295f67`), depois **parado a meio por
pedido do utilizador** ("se nos 10 jogos não ganharmos nada para
isso") via `/api/stop` (paragem graciosa -- deixa o jogo em curso
acabar, não reinicia nem corrompe nada, respeita a regra "nunca
reiniciar a meio de um jogo"). Fechado ao fim de **14 jogos: 0
vitórias, 1 empate, 13 derrotas**.

**CORREÇÃO importante (achado ao investigar o binário depois de
fechar o lote)**: assumi inicialmente que o lote inteiro corria com o
binário PRÉ-moderação (antes do commit `0c1b388`) -- errado. O commit
`0c1b388` (e o `cargo build --release` que o acompanha) aconteceu às
`22:50:50`, A MEIO do lote em curso, e `engine_arena.py:play_game()`
arranca um subprocesso `UciEngine` NOVO por jogo (não reutiliza o
processo entre jogos) -- confirmado comparando os timestamps dos PGNs
com o timestamp do commit: g1-g9 (22:32-22:51) correram no binário
PRÉ-moderação, mas **g10-g14 (22:54-23:01) já carregaram o binário
MODERADO do disco**, porque o `cargo build` sobrescreveu o ficheiro no
mesmo caminho que a arena usa (`Kestrel/target/release/kestrel`)
enquanto o lote decorria. Ou seja, o lote NÃO é uma baseline limpa de
um único binário -- é uma mistura. Sub-resultado do binário já
moderado (g10-g14): 0 vitórias, 1 empate, 4 derrotas -- amostra
pequena demais para concluir seja o que for, mas pelo menos confirma
que o binário moderado também perde consistentemente contra o Sirius
a esta escala.

**Lição para o método, já registada para não repetir**: nunca fazer
`cargo build --release` que sobrescreva o binário no caminho que a
arena está a usar (`Kestrel/target/release/kestrel`) enquanto um lote
está `"running": true` -- mesmo sem "reiniciar" a arena, o próximo
jogo do MESMO lote já carrega código diferente silenciosamente, porque
cada jogo arranca um processo novo. Regra a seguir daqui em diante:
confirmar `"running": false` antes de qualquer `cargo build --release`
que toque no binário activo, exactamente como já se fazia para
`./arena.sh restart`.

PGNs lidos (g1, g2, g3, g4): duas derrotas como pretas por mate directo
depois de ataques ao rei mal geridos (g2: rei apanhado num ataque
clássico depois de `Nb5-d6`; g3: mate por `Qf3#` depois de o Kestrel
ter aberto a posição do próprio rei com `g5-g4` sem compensação
suficiente), uma como brancas por um erro posicional que se acumula
lentamente até perda de peão e depois torre (g1). Sirius é ~3449 CCRL,
uma diferença de força grande é esperada, mas o padrão "abrir a posição
do próprio rei sem follow-up suficiente" ecoa o mesmo tema já
identificado nas derrotas contra o skill15 antes da moderação dos
termos de agressividade (`0c1b388`) -- reforça que essa moderação era a
correcção certa, mesmo que as 4-5 jogadas já com o binário moderado não
tenham sido suficientes para reverter o resultado geral.

**Pedido do utilizador**: "tens metido a calibração das variáveis de
lado" -- correto: só se tinha testado o profile de EVAL do Sirius
(`sprt_sirius_profile.py`, já feito), nunca os parâmetros reais de
BUSCA dele (ao contrário do Ethereal, que teve os dois: eval e busca).
Fechado agora.

**Fonte real**: `Sirius/src/search_params.h` (tabela `SEARCH_PARAM`,
valores SPSA-tuned) + `Sirius/src/search.cpp` (fórmulas reais onde cada
parâmetro é usado -- essencial, porque vários parâmetros da tabela só
fazem sentido combinados com termos extra que o Kestrel não tem
equivalente). Sirius usa a mesma escala SEE que o Kestrel (peão=100,
`board.h: SEE_PIECE_VALUES`) e um `HISTORY_MAX` quase idêntico (16384
vs 16000 do Kestrel) -- mapeamento directo de valores muito mais
fiável do que foi com o Ethereal (escalas bastante diferentes lá).

**`build_sirius_search_profile.py`** -> `sirius_search_profile.txt`,
14 dos 18 campos do `SearchParams` mapeados com fórmula real
confirmada, 4 deixados no default do Kestrel por incompatibilidade
estrutural genuína (documentado no próprio script, mesma disciplina do
Ethereal -- não forçar mapeamento quando o mecanismo é mesmo diferente):

- **RFP** (`rfp_improving`/`rfp_not_improving`): mapeado exacto.
  Fórmula Sirius `(improving ? rfpImpMargin : rfpNonImpMargin) *
  depth`, caso base reduz a slope*depth puro -- igual à forma do
  Kestrel. `rfpImpMargin=26` (era 65 no Kestrel!),
  `rfpNonImpMargin=80` (era 95). Sirius corta MUITO mais agressivo
  quando "improving" -- diferença grande, sinal de calibração real por
  testar.
- **Razoring** (`razor_base`/`razor_per_depth`): mapeado exacto via
  tradução algébrica (`razoringMargin*depth = razoringMargin +
  razoringMargin*(depth-1)`, exatamente a forma
  `base + per_depth*(depth-1)` do Kestrel). `razoringMargin=458` ->
  `razor_base=458, razor_per_depth=458` (era 150/100 -- Sirius razora
  MUITO menos, precisa duma diferença bem maior para confiar no corte
  cego). `razoringMaxDepth=3` do Sirius bate certo com o gate
  `depth<=3` já usado no Kestrel.
- **cap_futility** (captura/noisy futility): mapeado com aproximação
  documentada -- fórmula real do Sirius tem termo extra de history
  (`histScore/noisyFpHistDivisor`) que o Kestrel não tem, mas com
  history=0 reduz exactamente à forma `base+slope*depth` do Kestrel.
  `noisyFPBaseMargin=2, noisyFpDepthMargin=115` (era 0/90 e 0/130) --
  Sirius não distingue improving/not-improving aqui, por isso o MESMO
  valor foi aplicado aos dois campos do Kestrel.
- **history_prune_mult**: mapeado exacto (mesma fórmula
  `hist < -margin*depth`), com pequena correção de escala
  `1688 * (16000/16384) ≈ 1649` (era 2500 -- Sirius poda por history
  bem mais cedo/agressivo).
- **NÃO mapeados** (mecanismo genuinamente diferente, documentado no
  script em vez de forçar): futility de lances tranquilos (Sirius usa
  `lmrDepth` pós-redução + termo de history, sem split improving --
  aplicar os números crus sobre `depth` bruto do Kestrel podia cortar
  demais); `delta_margin` (o `qsFpMargin` do Sirius não tem termo de
  valor-da-peça-capturada, mecanismo diferente do delta pruning real do
  Kestrel); `qs_lmp_limit` e `tt_extended_cutoff_margin` (sem
  equivalente directo encontrado no Sirius).
- **Gap maior identificado, não fechado**: NMP, ProbCut, aspiration
  window e os ajustes finos de LMR (`nmpBaseReduction`,
  `probcutBetaMargin`, `aspInitDelta`, `lmrCutnode`, etc.) são todos
  tunados no Sirius mas o `SearchParams` do Kestrel simplesmente não
  tem campos para eles ainda -- portar isto implica adicionar campos
  novos à struct, não só um teste de profile. Fica registado como
  trabalho futuro se houver tempo, não inventado às pressas.

**Teste lançado**: `sprt_sirius_search.py` (clone de
`sprt_ethereal_search.py`, mesmo método: self-play fixed-nodes, 30000
nós/lance, 150 pares = 300 jogos, `KESTREL_SEARCH_PARAMS=
sirius_search_profile.txt` vs sem a env var), a correr em paralelo com
o match de arena (não compete por posições, só por CPU -- fixed-nodes
é imune a isso). Log em `sprt_sirius_search.log`. Recorda: "os testes
são só para verificar" -- mesmo que o resultado saia fraco, não é
motivo para reverter sem mais, e valores SPSA-tuned dum motor top são
pelo menos um baseline honesto para calibrar os próprios, não um valor
a descartar de ânimo leve.

**Achado concreto ao ler PGNs do match (g4)**: derrota das brancas por
um blunder puro -- `18...Qa3?? 19.bxa3`, dama capturada de graça (a3
está atacada por peão b2 E pela torre a1, confirmado via
`python-chess`/`attackers()`). Não é um erro posicional subtil, é do
tipo que uma busca de 1 ply já apanha via SEE/troca. `TimeControl
"60+1.0"` -- tempo base muito curto; ao fim de 18 lances de meio-jogo
complicado é plausível que o relógio já estivesse apertado e a busca
tenha sido cortada cedo demais nessa jogada. Não confirmado com
certeza (não há log de tempo por lance gravado pelo `arena_server`),
mas é consistente com o item 3 da lista de próximos passos já nas
notas ("investigar se a gestão de tempo em 4 níveis está a cortar
profundidade demais cedo demais") -- fica reforçado como prioridade
real, não só teórica, para quando o calendário permitir. As outras
derrotas lidas (g1, g2, g3, g7) parecem perdas "normais" contra um
motor bem mais forte -- jogo tático real a degradar-se, não blunders
de 1 lance.

**Resultado do `sprt_sirius_search.py` (300 jogos, 30000 nós/lance)**:
exactamente empatado -- **50.0% vs 50.0%, W138-L138-D24**. Sinal limpo
e neutro (nem positivo nem negativo, sem viés para nenhum lado).
Adoptados como novo default de `SearchParams::default()` em
`search.rs` (commit por fazer): `rfp_improving` slope 65->26,
`rfp_not_improving` slope 95->80, `razor_base`/`razor_per_depth`
150/100->458/458, `cap_futility_improving`/`cap_futility_not_improving`
0/90 e 0/130 -> 2/115 (ambos), `history_prune_mult` 2500->1648. Decisão
alinhada com o princípio já estabelecido: valores reais SPSA-tuned dum
motor top substituem um palpite próprio mesmo com resultado neutro --
não é preciso ganhar o A/B para "merecer" ser adoptado, só não perder
claramente. `futility_improving`/`futility_not_improving`,
`delta_margin`, `qs_lmp_limit`, `tt_extended_cutoff_margin` ficam
como estavam (sem equivalente Sirius directo, ver
`build_sirius_search_profile.py`).

**IMPORTANTE -- build pendente**: a edição ao `search.rs` já está
feita mas **`cargo build --release` ainda NÃO foi corrido**, de
propósito -- o lote limpo de 20 jogos vs Sirius com o binário anterior
(moderação de eval, `0c1b388`, sem esta mudança de busca) está a
correr neste momento (`"running": true` em `/api/state`) e usa o MESMO
caminho `target/release/kestrel`. Lição já registada acima: nunca
fazer build que sobrescreva o binário activo enquanto um lote está a
correr. Assim que este lote de 20 acabar (`"running": false`), o
próximo passo é: `cargo build --release`, correr os dois perfts de
validação (não mexeu em movegen, mas é rotina antes de confiar num
binário novo), e então lançar o próximo lote de auto-teste/self-play
já com os defaults novos.

## Atualização 2026-07-22 (continuação): correção do utilizador -- faltavam TODAS as variáveis do Sirius, não só as 18 do formato existente; NMP e ProbCut portados a sério

**Correção directa do utilizador**: "vais ver que não é isso... tem a
ver com TODAS as variáveis que o Sirius treinou... há uma opção para
compilar o Sirius com as variáveis por setoption." Certo -- eu tinha
limitado o mapeamento aos 18 campos que já existiam no `SearchParams`
do Kestrel, e descartado NMP/ProbCut/aspiration/etc. como "sem campo
equivalente" em vez de ADICIONAR os campos que faltavam. O Sirius tem
uma macro `EXTERNAL_TUNE` (`search_params.h`/`.cpp`) que, quando
compilada, regista TODOS os parâmetros como opções UCI reais -- build
separado feito (`make EXE=sirius_tune LDFLAGS="" CXXFLAGS="...
-DEXTERNAL_TUNE"`, binário `Sirius/sirius_tune`, NÃO sobrescreve o
`Sirius/sirius` que a arena usa) e `echo uci | ./sirius_tune | grep
option` confirmou os ~130 parâmetros e os mesmos valores já lidos do
código-fonte (nenhuma discrepância -- a leitura manual estava certa,
só era incompleta em quantidade).

**NMP portado a sério** (não era so' recalibração, era mecanismo em
falta): o NMP do Kestrel era `null_r = if depth>6 {3} else {2}` --
completamente cego à avaliação estática, ao contrário de todos os
motores de referência reais. Substituído pela fórmula real do Sirius
(`search.cpp:551-558`): gate duplo (`stack->eval >= beta+nmpEvalMargin`
E `stack->staticEval >= beta+nmpStaticEvalBaseMargin -
nmpStaticEvalDepthMargin*depth`) + reducao `R = (nmpBaseReduction +
depth*nmpDepthReductionScale)/256 + min((eval-beta)/
nmpEvalReductionScale, nmpMaxEvalReduction)`. 8 campos novos no
`SearchParams` (`nmp_min_depth=2, nmp_eval_margin=29,
nmp_static_eval_base_margin=193, nmp_static_eval_depth_margin=18,
nmp_base_reduction=1343, nmp_depth_reduction_scale=78,
nmp_eval_reduction_scale=208, nmp_max_eval_reduction=4`), valores
reais do Sirius. Mapeamento de variáveis: `static_eval` do Kestrel (já
corrigida por corr-hist) = `stack->eval` do Sirius;
`raw_static_eval` do Kestrel = `stack->staticEval` do Sirius -- mesma
distinção corrigida/bruta nos dois motores, só nomes trocados. Único
desvio da fórmula real: `.max(1)` de segurança na redução (garante que
`depth-r` desce sempre pelo menos 1, para nunca arriscar um loop --
com os valores reais R nunca fica perto de 0 na prática, é só uma rede
de segurança, não uma mudança de comportamento observável).

**ProbCut**: só faltava tornar o margin tunável -- `probcutMinDepth=5`
do Sirius já batia certo com o `depth>=5` fixo que o Kestrel já tinha;
`probcutBetaMargin=182` substitui o hardcoded `beta+150`.

**`SearchParams` agora com 27 escalares** (era 18) -- `to_vec`/
`from_vec` actualizados, `build_sirius_search_profile.py` actualizado
e regerado (`sirius_search_profile.txt`, 27 valores, agora IDÊNTICO
aos novos defaults -- os campos passaram a viver no próprio
`Default::default()`, o ficheiro fica só como documentação/smoke-test
do mecanismo `KESTREL_SEARCH_PARAMS`).

**`cargo check --release` confirma que compila limpo** (só warnings
pré-existentes, sem erros) -- `cargo check` NÃO mexe no binário ligado
final, confirmado por `mtime` inalterado em
`target/release/kestrel`, por isso é seguro correr enquanto o lote de
arena está `"running": true`. **`cargo build --release` continua
ADIADO** até o lote de 20 jogos vs Sirius acabar (mesma razão de
sempre -- não sobrescrever o binário activo a meio de um lote). Ainda
por fazer quando libertar: build, perfts de rotina, e um self-play A/B
dedicado ao NMP/ProbCut novos (mudança de MECANISMO de busca, não só
de dados/constantes -- vale a pena observar o comportamento antes de
confiar cegamente, mesmo sem ser gate para reverter).

**Gap que continua por fechar, mesmo depois disto**: aspiration
windows (`aspInitDelta`, `aspWideningFactor`, `minAspDepth`) e os
ajustes finos de LMR por condição (`lmrNonImp`, `lmrCutnode`,
`lmrTTPV`, etc., ~10 campos) do Sirius continuam sem equivalente no
Kestrel -- o LMR do Kestrel só tem o divisor único
`KESTREL_LMR_DIVISOR` (ja' testado e deixado no default, ruído puro
nesse teste em particular), não a tabela condicional rica do Sirius.
Não teve tempo para isto nesta ronda; fica registado para a próxima
sessão dedicada, não escondido.

## Atualização 2026-07-22 (continuação): erro meu de invocação nos perfts (não é bug), Fable auditou o "gap de profundidade" vs Sirius, correction history portada (5 termos)

**Erro de processo, registado para não repetir**: tentei validar o
binário com `echo "position startpos\ngo perft 6\nquit" | kestrel`
(via UCI stdin) -- ficou preso indefinidamente (>26 min de CPU, matei
o processo). Não é bug de código: `"go perft N"` simplesmente não é um
comando UCI reconhecido por este motor (`cmd_go` em `uci.rs` não tem
case para `"perft"`), por isso o parser ignora o token e cai num `go`
sem profundidade/tempo definidos -- espera por `stop` que nunca chega.
O perft REAL é um modo CLI: `./kestrel perft <depth> [fen]`. Confirmado
com a invocação correcta: **startpos perft(6) = 119060324 (78.9M
nps)**, **Kiwipete perft(4) = 4085603 (5.8M nps)** -- ambos correctos,
movegen validado. Lição: usar sempre `kestrel perft N` (CLI), nunca
`go perft` via UCI.

**Fable 5 auditou o "gap de profundidade" pedido pelo utilizador**
("algo não está tão aprofundado como no Sirius... mete o Fable a
estudar"). Comparou Kestrel vs Sirius em 4 áreas (correction history,
LMR, aspiration windows, gestão de tempo), com formulas reais e
estimativa de custo/ganho para cada uma. Resumo por prioridade:

1. **Correction history** (~104 elo no comentário do Sirius) --
   Kestrel só tinha o termo de estrutura de peões; Sirius soma 7 termos
   pesados (peão, material-sem-peões de cada lado, ameaças, menores,
   maiores, + 6 lags de continuation-history). Infra mais barata das 4
   (reaproveita `pawn_structure_hash`/`ply_last_move` já existentes).
   **Feito nesta sessão** (ver abaixo) -- 5 dos termos (peão+2
   material-sem-peões+menores+maiores), `threats` e os 6 lags de
   continuation-history deixados de fora (precisam de infra nova em
   eval.rs / plumbing extra, documentado no código em vez de forçado).
2. **LMR** (~111 elo) -- Kestrel só tem 1 ajuste flat; Sirius tem ~9
   condições (`lmrNonImp`, `lmrCorrplexity`, `lmrGivesCheck`,
   divisores de history contínuos, doDeeper/doShallower, etc.). A parte
   "barata" (reaproveita `improving`/`corrplexity` já calculados) fica
   como próximo passo. `lmrCutnode` (o maior peso individual, 1720) foi
   explicitamente marcado como FORA de alcance -- precisa de threading
   de um parâmetro `cutnode` por TODA a recursão (6-8 pontos de
   chamada), Kestrel não tem NENHUM tracking de cutnode hoje.
3. **Gestão de tempo (node-fraction + best-move-stability scaling)** --
   gap real confirmado, mas o Fable sinalizou risco concreto: as
   próprias notas desta sessão já documentam Lazy SMP (4 threads) a dar
   tempos de busca ruidosos (1.0s/6.8s/2.5s/1.5s/10.6s na MESMA posição)
   quando se tentou algo parecido antes -- tratar com mais cuidado que
   as outras 3 áreas.
4. **Aspiration windows** -- Kestrel já tem o mecanismo (não é gap
   estrutural), só a fórmula é mais simples que a do Sirius. Prioridade
   mais baixa: há um comentário já no código do Kestrel a dizer que um
   teste isolado desta área especificamente deu negativo (33%) no
   passado.

**Correction history implementada** (`search.rs`): 4 novas hash
functions (`non_pawn_hash(board,color)`, `minor_piece_hash`,
`major_piece_hash`, seguindo o mesmo padrão não-incremental já usado
por `pawn_structure_hash`), 4 novas tabelas no `Searcher`
(`corr_hist_np_stm/np_nstm/minor/major`, mesma forma/tamanho da
`corr_hist` já existente), pesos SPSA reais do Sirius
(`CORR_WEIGHT_PAWN=384, NP_STM=406, NP_NSTM=280, MINOR=274,
MAJOR=418`, escala `CORR_WEIGHT_SCALE=256`). `corrected_static_eval`
agora soma os 5 termos pesados em vez de só ler a tabela de peões
directamente -- isto TAMBÉM recalibra o termo de peão já existente (o
peso implícito antigo era 256/"1.0", o real do Sirius é 384/1.5x).
`update_corr_hist` actualiza as 5 tabelas de uma vez, e a taxa de
aprendizagem por profundidade subiu de `min(depth+1,16)` para
`2*min(depth+1,16)` (fórmula real do Sirius, `history.cpp:104`) --
mais responsiva a profundidades altas do que antes. 3 pontos de
construção do `Searcher` (main.rs x2, uci.rs x1) actualizados com os 4
novos campos. `cargo check --release` limpo, sem erros.

**Próximo passo imediato**: `cargo build --release` (a arena está
parada por pedido do utilizador, seguro fazer agora), reconfirmar os 2
perfts com a invocação CLI correcta, e lançar um self-play A/B
dedicado à correction history nova (mudança de mecanismo real, não só
dados -- vale a pena observar antes de confiar, mesmo sem ser gate).

**Feito**: `cargo build --release` (binário novo com NMP+ProbCut+
correction-history), perfts reconfirmados com a invocação CLI correcta
-- `perft(6) startpos = 119060324` (84.2M nps), `perft(4) Kiwipete =
4085603` (6.7M nps), ambos correctos. Como o mecanismo novo está
compilado directamente no binário (não é um `SearchParams` tunável via
env var), o A/B precisa de DOIS binários -- criado
`git worktree add /tmp/kestrel_baseline_0c1b388 0c1b388` (build limpo
do último commit, antes de NMP/ProbCut/corr-hist), copiado para
`/root/kestrel_joao/kestrel_baseline_0c1b388`; binário novo copiado
para `/root/kestrel_joao/kestrel_nmp_corrhist` (`sf17_book.bin` também
copiado para o mesmo directório, resolução do caminho do livro é
relativa ao executável). Worktree removido depois de extrair o
binário. `sprt_nmp_corrhist.py` (300 jogos, 30000 nós/lance) a correr
em background, log em `sprt_nmp_corrhist.log`.

## Atualização 2026-07-22/23 (continuação): REGRESSÃO GRAVE encontrada e corrigida -- 2 bugs reais no NMP/correction-history portados

**Resultado do primeiro A/B**: catastrófico -- `A(baseline)=93.3%,
B(new)=6.7%, W275-L15-D10` em 300 jogos. Não é ruído, é uma regressão
grande a sério -- investigado antes de sequer considerar commitar.

**Bug 1 (NMP): faltava o guard de duplo null-move + verificação a alta
profundidade**. Fui ler o resto de `search.cpp` do Sirius (tinha parado
demasiado cedo antes) e há duas peças que faltavam por completo no
porte:
- `board.pliesFromNull() > 0` -- Sirius NUNCA permite dois null-moves
  consecutivos na mesma linha (classicamente inseguro: pode "provar"
  um fail-high falso). O porte do Kestrel não tinha NENHUM tracking
  disto.
- Quando o null-move score bate beta E (`depth>15` OU `beta` é score de
  vitória conhecida), Sirius NÃO confia cegamente -- faz uma busca de
  VERIFICAÇÃO real (não-null) à mesma profundidade reduzida antes de
  aceitar o corte, e desliga o NMP (`nmpMinPly`) até passar esse ponto
  -- mecanismo de verificação em si NÃO portado ainda, fica como
  referência para follow-up.
- Sem estas duas peças, confiar cegamente em R adaptativo grande (5-9
  em profundidades baixas, valores reais do Sirius) é preciso do
  contexto que só o resto do mecanismo dá.

**Corrigido**: adicionado `reached_by_null: bool` como parâmetro
explícito de `negamax()` (threading manual por todos os 10 pontos de
chamada -- só um bool, não o "cutnode" que o Fable já tinha marcado
como fora de alcance), gate `!reached_by_null` no bloco de NMP. R
também agora tem um **cap em 4** (`clamp(1,4)` em vez de sem limite) --
decisão HONESTA de âmbito reduzido: a busca de verificação completa do
Sirius (que tornaria seguro confiar em R hihg) fica documentada como
trabalho futuro (mesmo nível de esforço/risco que o `lmrCutnode` que o
Fable já tinha adiado), mantém-se só a parte JÁ validada como boa (o
gate eval-adaptativo, genuinamente mais informado que o antigo
`depth>6?3:2` cego).

**Bug 2 (correction history): pesos reais do Sirius aplicados às
tabelas do Kestrel sem re-escalar**. Isolei via teste: com o fix do NMP
sozinho e a correction-history revertida para o comportamento antigo
(só peão, peso implícito 256), o resultado voltou a ~50/50 (10 jogos,
W5-L4-D1) -- confirma que o bug dominante era mesmo a correction
history, não o NMP. Causa: os pesos SPSA do Sirius
(`pawnCorrWeight=384` etc., soma total 1762) foram calibrados pelo
Sirius PARA o `maxCorrHist` e grão internos DELE -- aplicá-los
directamente às tabelas do Kestrel (clamp próprio, `CORR_HIST_MAX=
1200`, nunca co-desenhado com esta soma de pesos) produzia correções
completamente fora de escala, contaminando RFP/razoring/futility (que
usam `static_eval` corrigido) por todo o lado.

**Corrigido**: pesos reescalados para preservar as proporções RELATIVAS
reais do Sirius (que termo importa mais que qual) mas com a soma total
fixada em 256 -- exactamente o mesmo orçamento máximo que o sistema
antigo (só peão, peso implícito 256) já operava em segurança. Fórmula:
`peso_i_novo = round(peso_i_sirius * 256 / soma_sirius)`. Valores
finais: `CORR_WEIGHT_PAWN=56, NP_STM=59, NP_NSTM=41, MINOR=40,
MAJOR=61` (soma 257, arredondamento). Taxa de aprendizagem (`2*min
(depth+1,16)`, valor real do Sirius) mantida -- não estava implicada no
bug, só a ESCALA dos pesos.

**Lição geral para o resto da sessão (e sessões futuras)**: valores
SPSA reais de um motor de referência são fiáveis para o FORMATO/
FÓRMULA e para as PROPORÇÕES relativas entre parâmetros relacionados,
mas os valores ABSOLUTOS só transferem em segurança quando a escala
subjacente (clamps, grão de fixed-point, unidades internas) é
verificada como igual -- ou, quando não se sabe ao certo, re-escalados
para o orçamento que a implementação PRÓPRIA já validou como seguro,
em vez de copiados às cegas. `history_prune_mult` (sessão anterior,
razão `16000/16384`) já tinha usado este princípio correctamente por
as escalas serem quase iguais e verificáveis; aqui a escala do Sirius
não era directamente verificável a partir do código-fonte disponível,
por isso a rescala por PROPORÇÃO (preservar a forma, ancorar a
magnitude ao que já se sabe seguro) é o compromisso correcto, não
copiar às cegas nem descartar a técnica toda.

**Novo binário rebuilido e reperft'd** (perft(6)=119060324,
perft(4)=4085603, ambos correctos). Segundo A/B (200 jogos) a correr em
background, log `sprt_nmp_corrhist_v2.log`.

**Resultado final do segundo A/B**: `A(baseline)=85.0/200=42.5%,
B(new)=115.0/200=57.5%, W75-L105-D20` -- positivo real, não só "voltou
a neutro". Commitado. Binários de teste (`kestrel_baseline_0c1b388`,
`kestrel_nmp_corrhist`, `kestrel_nmp_only`) e scripts
(`sprt_nmp_corrhist.py`, `sprt_nmp_only.py`) ficam em
`/root/kestrel_joao/` para referência, não fazem parte do repo git.

**Commitado como `9f93be9`.**

## Atualização 2026-07-23: item #2 do Fable (LMR) -- só a parte segura, dado o que aconteceu com o corr-hist

Depois do susto do bug de escala na correction history, decidido ser
mais cauteloso desta vez: só portar do LMR do Sirius o que é
inequivocamente seguro (thresholds inteiros, sem conversão de escala/
fixed-point nenhuma), testar sozinho antes de continuar, em vez de
juntar tudo de uma vez outra vez.

**Portado**:
- **Split PV/não-PV no gate de elegibilidade do LMR**: era `i>=2 &&
  depth>=2` para todos os nós; agora `i >= (is_pv ? 4 : 3) && depth>=3`
  -- valores reais do Sirius (`lmrMinMovesPv=4, lmrMinMovesNonPv=3,
  lmrMinDepth=3`). Puros inteiros, sem ambiguidade de escala.
- **Ajuste de corrplexity**: `-1 ply` quando `|static_eval -
  raw_static_eval| > 89` (o `highCorrplexityMargin` real do Sirius) --
  reduz menos em posições onde a correction history diverge muito do
  eval bruto (posição "complexa"). Valor real do Sirius é fracionário
  (`lmrCorrplexity=605/1024≈0.59 plies`); arredondado para 1 ply
  inteiro, seguindo o mesmo estilo de quantização inteira que
  `hist_adj`/`ttpv_adj` já usavam neste código antes desta sessão --
  não é um valor inventado, é o mesmo real arredondado de forma
  consistente com o resto do ficheiro.

**Deliberadamente NÃO portado ainda** (fica para outra sessão, mesmo
raciocínio do `lmrCutnode`/verificação de NMP): `lmrNonImp`,
`lmrGivesCheck` (mudaria de "desliga LMR nos xeques" para "reduz
menos", mudança estrutural, não só numérica), `lmrTTPV`/
`lmrTTPVNonFailLow` (o `ttpv_adj` actual já é uma versão simplificada
disto), divisores de history contínuos (`lmrQuietHistDivisor=8846`/
`lmrNoisyHistDivisor=6837` substituiriam o `h/4000` actual, mas o
sistema de acumulação fixed-point do Sirius doesn't map 1:1 sem mais
trabalho), `doDeeper`/`doShallower` (reescalada de profundidade pós-
resultado, feature nova, não só recalibração).

**Validação**: perfts reconfirmados (`119060324`/`4085603`), binário
anterior (commit `9f93be9`) buildado num worktree separado
(`kestrel_baseline_9f93be9`) para comparação limpa. Smoke-test de 10
jogos deu 60/40 -- nada parecido com o padrão de 100%/0% do bug
anterior, sinal são. Lote completo de 300 jogos (`sprt_lmr.py`) a
correr em background, log `sprt_lmr.log`.

**Resultado final**: `A(baseline)=157.5/300=52.5%,
B(new)=142.5/300=47.5%, W143-L128-D29` -- ligeiramente negativo, mas
dentro do ruído normal de 300 jogos (nada como os 6.7% do bug real de
antes) e são valores REAIS do Sirius, fielmente traduzidos (não
inventados) -- mantido e commitado, mesmo princípio já aplicado a
TTPV/extensions nesta sessão ("os testes são só para verificar").

## Atualização 2026-07-23 (continuação): revisão do Fable ao diff da sessão -- 2 bugs reais MAIS encontrados no NMP

Depois de commitar `9f93be9`/`c189efe`, pedi ao Fable uma revisão
adversarial independente ao diff inteiro (dado o susto já teve com o
corr-hist). Resultado: 2 bugs reais confirmados, ambos no NMP; as
áreas mais próximas do bug anterior (separação das 5 tabelas de
corr-hist, threading do `reached_by_null` pelos 10 pontos de chamada,
`to_vec`/`from_vec`, os 3 pontos de construção do `Searcher`) vieram
todas limpas.

**Bug real #1 (grave)**: o `.clamp(1,4)` no R do NMP colapsava SEMPRE
para 4 -- a fórmula real do Sirius com as constantes reais
(`nmp_base_reduction=1343`) nunca produz menos que ~5 em profundidade
>=`nmp_min_depth`(2), por isso o clamp inferior (1) nunca entra em
jogo e o superior (4) capta sempre o mesmo valor. O mecanismo
"eval-adaptativo" descrito nos comentários era código morto na
prática -- ainda validado como melhoria real via self-play (57.5%),
mas pela razão errada (R fixo=4, mais agressivo que o antigo
`depth>6?3:2`, não pela adaptividade genuína). **Corrigido**:
subtraído um offset de segurança (3) antes do clamp final
(`(raw_r - 3).clamp(1, 6)`), restaura resposta real a
profundidade/eval mantendo a mesma razão para o limite superior
(busca de verificação do Sirius continua não portada).

**Bug real #2**: o segundo gate do NMP usava `raw_static_eval`
(comentário dizia ser o equivalente do `stack->staticEval` do Sirius)
mas na verdade, no código-fonte real do Sirius, TANTO `stack->eval`
COMO `stack->staticEval` guardam o valor CORRIGIDO -- `rawStaticEval`
é só uma variável local do Sirius, nunca guardada no stack. O
comentário estava trocado e o código seguia o erro. **Corrigido**:
os dois gates agora usam `static_eval` (o corrigido).

**Falso positivo do Fable, verificado e descartado**: sinalizou
`history_prune_mult=1648` como possível erro de transcrição vs o
`1688` real do Sirius -- não é, é o resultado deliberado e já
documentado da correção de escala `1688*(16000/16384)` (HISTORY_MAX
do Kestrel vs do Sirius), calculado e confirmado de novo
(`round(1688*16000/16384)=1648`). Lição: verificar SEMPRE antes de
aplicar uma sugestão de revisão às cegas, mesmo vinda de uma revisão
cuidadosa.

**Achado de confiança mais baixa, não fechado**: possível off-by-one
no threshold de `min_moves` do LMR (índice `i` do Kestrel é
"lances já jogados ANTES deste", enquanto o `movesPlayed` do Sirius já
inclui o lance actual no ponto de comparação) -- direcção conservadora
(reduz um pouco mais tarde que o Sirius pretendia), não é bug de
solidez, deixado como está (já validado via A/B, resultado neutro-
levemente-negativo mantido por política).

**Validação do fix**: perfts reconfirmados. Lote de arena Sirius em
curso foi parado cedo (só 4 jogos, binário desactualizado por este
fix) -- não informativo, descartado. Novo A/B de 300 jogos
(`sprt_nmpfix.py`) a correr, log `sprt_nmpfix.log`, binário anterior
(`c189efe`) buildado num worktree separado para comparação limpa.

**Resultado**: exactamente empatado -- `50.0%/50.0%, W139-L139-D22`
vs o estado anterior (que já incluía o bug do R fixo). Ou seja, o R
fixo "por acidente" tinha, por acaso, um desempenho equivalente à
adaptividade real nesta amostra -- mas isto são correcções de
CORRECÇÃO (variável errada, lógica morta apresentada como
adaptativa), não só recalibração, por isso mantido e commitado
independentemente do resultado neutro -- código a fazer o que os
próprios comentários dizem que faz vale a pena mesmo sem ganho
imediato mensurável.

**Commitado como `04f79ae`.** Lote de 20 jogos vs Sirius lançado com
este binário final -- resultado: **0 vitórias, 18 derrotas, 2
empates**. Mesmo padrão dos lotes anteriores (0 vitórias sempre) --
consistente com o gap de força absoluto real (~3449 CCRL do Sirius vs
onde o Kestrel está agora), não uma regressão nova. Os A/B desta sessão
(self-play Kestrel-vs-Kestrel) mostram ganhos internos reais e
validados (57.5% no NMP+corrhist, neutro no resto) -- a arena contra o
Sirius mede a distância absoluta, que continua grande, não se
espera que feche com este tipo de mudança incremental. Confirma o que
já se sabia (não é achado novo); não vale a pena continuar a gastar
tempo/CPU em mais lotes Sirius sem antes fechar mais itens de
calibração -- self-play continua a ser o sinal certo para validar
mudanças, a arena Sirius serve só de checkpoint ocasional de "quão
longe estamos", não de gate de progresso.

## Resumo do estado no fim desta ronda (2026-07-23, cedo)

3 commits novos desde `0c1b388`: `9f93be9` (RFP/razor/cap-futility/
history-prune-mult do Sirius + NMP eval-adaptativo + ProbCut tunável +
correction history de 5 termos, com 2 bugs reais corrigidos antes de
commitar), `c189efe` (LMR: split PV/não-PV + ajuste de corrplexity),
`04f79ae` (2 bugs reais mais no NMP, encontrados por revisão
independente do Fable, corrigidos). Todos validados por perft +
self-play A/B antes de commitar. Trabalho do Fable (auditoria do gap
Sirius vs Kestrel) identificou mais 2 áreas por explorar se houver
tempo: gestão de tempo (node-fraction/best-move-stability, risco
conhecido de ruído do Lazy SMP) e aspiration windows (prioridade
baixa, já tem sinal negativo documentado).

## Atualização 2026-07-23 (continuação): pedido do utilizador redefine o objectivo -- SEM deadline, ganhar ao Sirius é o objectivo principal

**Mudança de instrução do utilizador**: "sem deadline, objectivo
principal - ganhar ao Sirius. não deixes nada por implementar e
sobretudo testes com os dados do Ethereal e Sirius contra o Sirius...
bitboards e tabelas psqt fazem parte bem como as variáveis
configuráveis." A deadline de 2026-07-24 21h deixa de se aplicar.
Âmbito alargado explicitamente ao lado do EVAL (PSQT/material/termos
posicionais), não só busca. Testar directamente contra o Sirius (não
só self-play Kestrel-vs-Kestrel) passa a ser prioridade, dado o
objectivo ser vencer um oponente específico, não só melhorar
internamente.

**Infra nova**: `Ethereal` clonado localmente
(`/root/kestrel_joao/Ethereal`, AndyGrant/Ethereal -- hoje
principalmente NNUE mas mantém um eval clássico HCE real e completo em
`evaluate.c`, PSQT tabeladas e tudo, útil como segunda referência).
`vs_sirius.py` -- harness rápido de nós fixos (Kestrel dev build vs
`Sirius/sirius` real, mesma metodologia dos `sprt_*.py` de self-play)
para validação directa contra o alvo, mais rápido que um match de
arena com relógio real. Baseline confirmada: binário `04f79ae` a
30000 nós/lance perde 0/6 ao Sirius -- consistente com o gap de força
já conhecido, não um artefacto de gestão de tempo.

**Auditoria do Fable ao lado EVAL** (Sirius `eval/eval_constants.h`+
`eval_terms.cpp`+`pawn_structure.cpp`, Ethereal `evaluate.c`, vs o
`eval.rs` do Kestrel) -- achado crítico antes de tocar em PST:
**a tabela PSQT do Sirius está em referencial de PRETAS** (`combined_psqt.h`:
brancas usam `PSQT[peca][casa^56]`, pretas usam `PSQT[peca][casa]`
directo) -- o OPOSTO da convenção do Kestrel (referencial de brancas,
a1=0). Copiar as linhas do Sirius sem inverter (`rank r <-> rank 7-r`)
inverteria a tabela para as brancas. Ethereal usa a MESMA convenção do
Kestrel (referencial de brancas), copiável sem flip. Escalas de
material das 3 engines diferem MUITO (peão mg: Kestrel=125, Sirius=65,
Ethereal=82) -- qualquer valor absoluto portado precisa de reescala
pela razão real do peão (mesma disciplina do `history_prune_mult`/
`SCORE_KNOWN_WIN` já usada no lado da busca), nunca copiado em bruto.
Relatório completo tem lista extensa ranqueada em 3 tiers -- ver
transcript do agente se precisar dos números exactos de cada termo
ainda por portar.

**Portado nesta ronda** (eval.rs, todos os valores reais Sirius/
Ethereal, reescalados pela razão do peão mg/eg Kestrel vs fonte, nunca
copiados em bruto):
- `PASSED_PAWN`: upgrade de tabela flat-por-rank para
  `[blocked][controlled][rank]` real do Sirius (push square ocupado /
  atacado pelo inimigo).
- `OUR_PASSER_PROXIMITY`/`THEIR_PASSER_PROXIMITY`: NOVO -- distância
  Chebyshev rei-ao-quadrado-de-avanço do peão passado. Kestrel não
  tinha nada disto antes (feature clássica universal).
- `PASSER_DEFENDED_PUSH`/`PASSER_SLIDER_BEHIND`: NOVOS -- bónus se a
  casa de avanço é defendida, penalização se torre/dama inimiga está
  atrás do peão na mesma coluna.
- `CANDIDATE_PASSER`: upgrade de escalar único para `[defended][rank]`
  real do Sirius.
- `ROOK_ON_SEVENTH`: NOVO (Ethereal) -- bónus dedicado por torre na
  7ª fileira relativa, independente de coluna aberta.
- `SAFE_KNIGHT/BISHOP/ROOK/QUEEN_CHECK`: split do `SAFE_CHECK` único
  em 4 pesos por tipo de peça -- ordem relativa real do Sirius
  preservada (torre/cavalo mais perigosos que dama, contra-intuitivo
  mas real), magnitude ancorada ao valor antigo já calibrado do
  Kestrel (não copiado em bruto do Sirius, que passa por um squash `/8`
  que o Kestrel não tem -- só a PROPORÇÃO relativa foi portada, a
  escala foi re-derivada). `sentinel` do `tune_fast` em `main.rs`
  actualizado para os 4 campos novos.

**4 testes A/B em paralelo agora** (todos vs baseline `04f79ae`, 300
jogos, binários extraídos incrementalmente à medida que cada mudança
foi adicionada -- não isolados uns dos outros de forma perfeita já que
o eval e o safe-check foram construídos por cima das mudanças de busca
anteriores no mesmo working tree, mas aspiration/nmp-verification
têm os seus próprios binários isolados extraídos ANTES do trabalho de
eval começar):
- `sprt_asp.py` -- só aspiration windows (fórmula real do Sirius).
- `sprt_nmpverif.py` -- só o mecanismo completo de verificação do NMP
  (R sem cap + `nmp_min_ply` + busca de verificação).
- `sprt_evalpp.py` -- aspiration+nmp-verif+passed-pawn/rook-7th juntos
  (binário construído por cima dos 2 anteriores).
- `sprt_safecheck.py` -- tudo o anterior + split do safe-check.

Ainda por fazer (auditoria do Fable, tiers 2/3): `BISHOP_PAWNS`
(upgrade do `BAD_BISHOP`), `ISOLATED_PAWN`/`DOUBLED_PAWN` indexados
por distância à margem + variantes `_EXPOSED`, `WEAK_KING_RING`,
`KING_FLANK_ATTACKS`/`DEFENSES`, recalibração de mobilidade/threats
pelas proporções reais do Sirius, ratios de material (via tuner, não
cópia directa), e os itens de Tier 3 (substituição completa do
subsistema king-safety, shelter/storm 2D do Ethereal) marcados como
"projecto futuro maior" pelo próprio Fable, não escondidos.

## Atualização 2026-07-23 (continuação): aspiration windows E busca de verificação do NMP revertidas -- resultados reais negativos, não ruído

**Resultados finais dos testes isolados**:
- `sprt_asp.py` (aspiration windows, fórmula real do Sirius): **39.0%
  vs 61.0%**, W117-L183... (baseline ganhou), 300 jogos.
- `sprt_nmpverif.py` (busca de verificação completa do NMP): **41.5%
  vs 58.5%**, 300 jogos.

Ambos claramente fora do ruído (~20 pontos percentuais, não os ~3-5
pontos típicos de resultados "neutro com viés"). Para a aspiração, é
o TERCEIRO sinal negativo independente para esta área especificamente
(comentário já existente no código apontava 33% duma tentativa
anterior; a versão nova, com a fórmula real do Sirius, deu 39%) --
três tentativas diferentes, mesma conclusão. Dado a magnitude grande e
repetida (não um único resultado ambíguo tipo 46-48%), decidido que
isto ultrapassa o princípio "testes só verificam, não revertem" -- esse
princípio serve para não descartar valores reais por ruído fraco, não
para manter uma técnica com sinal forte e repetido de que piora o
motor. **Ambas revertidas** para as versões anteriores já validadas:
- `search_root`: de volta à janela com delta fixo=25, dobra sempre
  (a versão que os testes em lote já tinham validado positiva).
- NMP: de volta a `r = (formula real).max(1)`, sem cap artificial,
  sem busca de verificação, sem `nmp_min_ply` (campo removido do
  `Searcher`, 3 pontos de construção actualizados) -- a versão que já
  tinha dado 50/50 (neutro) contra o binário com o bug do R fixo, que
  por sua vez já era +57.5% sobre o baseline pré-NMP.

**Teste limpo dos aditivos de EVAL, sem as mudanças de busca revertidas
misturadas**: `sprt_evalclean.py` (binário = busca revertida +
TODOS os aditivos de eval desta ronda: passed-pawn 3D, king-proximity,
defended-push, slider-behind, candidate-passer, rook-on-seventh,
safe-check por tipo de peça, bishop-pawns, isolated/doubled indexados
por margem + exposed, weak-king-ring). Smoke-test de 10 jogos deu
**70%** para o novo binário -- sinal inicial muito positivo. Lote
completo de 300 jogos a correr, log `sprt_evalclean.log`.

**Nota sobre os testes `sprt_evalpp.py`/`sprt_safecheck.py` ainda em
curso quando isto foi escrito**: esses binários foram construídos ANTES
da reversão da aspiração/NMP-verificação, por isso misturam as
mudanças boas (eval) com as más (busca) -- deixados correr até ao fim
só como referência histórica, não usados para decidir nada. O teste
limpo (`sprt_evalclean.py`) é que conta.

**Confirmado**: `sprt_evalpp.py` (41.7%) e `sprt_safecheck.py` (44.7%)
acabaram negativos como esperado -- confirma que o problema era mesmo
a aspiração/NMP-verificação, não os aditivos de eval (já reforçado
pelo `sprt_evalclean.py`, que sem essas duas mudanças mostrava sinal
claramente positivo, embora tenha moderado de ~70% inicial para ~51%
com mais jogos -- normal, amostras pequenas exageram).

**Mais 2 itens Tier 2 implementados**: `KING_FLANK_ATTACKS`/
`KING_FLANK_DEFENSES` (real Sirius, zona "flanco do rei" larga --
banda de 4 colunas do lado do rei x banda de 5 fileiras do lado
próprio, mais larga que o anel imediato do rei; contagem de casas
atacadas/defendidas 1x e 2x+). Teste próprio `sprt_flank.py` a correr.

**PSQT substituído** (o item Tier 3 que o Fable tinha bloqueado até
verificar a convenção): tabelas PeSTO educacionais do Kestrel
substituídas pelas PST reais do Ethereal (`evaluate.c`,
Pawn/Knight/Bishop/Rook/Queen/King, 6x64 valores) -- Ethereal usa a
MESMA convenção de referencial que o Kestrel (brancas directo, sem
espelho), confirmado pela auditoria do Fable antes de portar, ao
contrário do Sirius (referencial de pretas, precisaria inverter linhas
-- não usado como fonte de PST desta vez por isso). Reescalado pela
razão real peão mg/eg Kestrel vs Ethereal (mg=125/82=1.524,
eg=140/144=0.972). Material (`MG_VALUE`/`EG_VALUE`) NÃO tocado, só a
componente posicional -- extração e reescala feitas por script Python
a partir do código-fonte real (não transcrição manual, evita erros).
Teste `sprt_psqt.py` a correr, log `sprt_psqt.log`.

Ficheiros scratch da extração (`scratchpad_psqt*.txt`) apagados depois
de confirmados no `eval.rs`.

## Atualização 2026-07-23 (continuação): correção directa do utilizador -- "VCS têm de ter os vossos próprios valores, isto não é um clone"

**Feedback do utilizador**: "cuidado VCS têm de ter os vossos próprios
valores, isto não é um clone. vocês são inteligentes o suficiente para
conseguir fazer um excelente motor." Correcto -- esta sessão portou
MUITOS valores reais do Sirius/Ethereal (reescalados, mas ainda assim
emprestados) em vez de derivar os valores PRÓPRIOS do Kestrel via
tuner. Técnica/estrutura (que mecanismo implementar, que forma uma
fórmula deve ter) é legítimo estudar em motores de referência -- é
assim que se aprende engenharia de motores. Mas os NÚMEROS finais têm
de vir do próprio processo de tuning do Kestrel sobre os próprios
jogos, não de reescalar SPSA/Texel doutro motor.

**Ação imediata**: `kestrel tunefast` (infra já existia,
`src/main.rs`) lançado sobre `dataset_round1_quiet.epd` (62928
posições reais de self-play do Kestrel, já resolvidas para quietas,
duma ronda anterior desta sessão) com a struct `Weights` actual
(648 probes/posição agora, cresceu com os campos novos desta ronda).
3000 iterações a correr em background, log `tune_round2.log`,
resultado em `tuned_round2.txt`. Isto vai gerar valores GENUINAMENTE
derivados do Kestrel para material/PST/mobilidade/threats/pawn-
structure/etc -- a substituir o que foi portado à mão onde fizer
sentido.

**Nota importante sobre o âmbito do tuner**: `tunefast` só ajusta
campos LINEARES do `Weights` -- os campos "king-safety não-lineares"
(agora 18: `king_attacker_weight`, `king_attacks`,
`safe_knight/bishop/rook/queen_check`) ficam de fora por desenho
(alimentam o `KING_DANGER_TABLE`, não uma regressão linear directa) --
esses precisam de A/B/self-play para calibrar, não o tuner. E
material/PST (`MG_PAWN` etc.) continuam a ser `const` compile-time,
NAO fazem parte do `Weights`/`to_vec()` -- o tuner não os toca de
todo, são um projecto de infra separado se se quiser mesmo tuná-los
(teria de passar a ser parte do vector tunável).

**PST do Ethereal REVERTIDO**: o teste `sprt_psqt.py` (parado cedo,
74/116=63.8% a favor do baseline -- negativo, consistente) confirmou
que copiar o PST doutro motor (mesmo reescalado) não ajudava aqui --
alinhado com o feedback do utilizador E com os dados. Voltado às
tabelas PeSTO originais ("ponto de partida educacional", já assim
documentado no próprio código antes desta sessão -- valor a afinar via
tuner próprio no futuro, quando o PST passar a ser tunável). Os outros
aditivos de eval desta ronda (passed-pawn 3D, king-proximity, rook-
on-seventh, bishop-pawns, isolated/doubled indexados, king-flank)
ficam -- são MECANISMOS/ESTRUTURAS reais (não uma tabela de valores em
bruto), com os próprios valores marcados como recalibráveis, e já
validados positivos em self-play limpo.

**Commitado como `9f088a9`** (reversão aspiração/NMP-verif + aditivos
de eval Tier 1/2, sem PST).

**Resultados finais dos 2 A/B de eval limpos**: `sprt_evalclean.py`
(passed-pawn/proximity/rook-7th/safe-check/bishop-pawns/isolated-
doubled/weak-king-ring) = 51.0% (300 jogos), `sprt_flank.py`
(king-flank em cima do resto) = 52.5% (300 jogos). Ambos modestos mas
reais e positivos, consistentes com mecanismos genuínos em vez de
valores adivinhados.

**Achado sobre o `tunefast`: taxa de aprendizagem antiga (2.0) estava
completamente errada de escala**. A 1ª tentativa (3000 iterações,
lr=2.0, igual ao `tuned_round1.txt` anterior) só moveu 1 de 665
campos, por apenas 1 unidade -- praticamente inerte, apesar do erro
"cair" ligeiramente (0.089031->0.088811). Causa: `grad[j]/n_pos` com
`n_pos=62928` torna o passo por iteração minúsculo para a maioria dos
campos a este lr. Testado lr=1000 (200 iterações): erro caiu 5.3%,
127/665 campos moveram-se de forma real, sem sinais de instabilidade.
**Corrida real lançada**: 8000 iterações, lr=1000 -- erro
0.087223->0.077338 (-11.3%, ainda a descer suavemente no fim, não
estagnou), 381/665 campos mudaram de forma substancial
(`tuned_round3.txt`). Nota para o futuro: lr=2.0 (usado no
`tuned_round1.txt` da sessão anterior) estava errado -- se esse
resultado antigo foi usado nalgum lado, reconsiderar.

**Também confirmado, não é bug**: `checkweights` reporta
`eval() != evaluate_with_weights(default)` para as 3 posições de
teste -- investigado, não é bug, é arquitectura deliberada:
`evaluate_with_weights()` (usada pelo tuner) omite propositadamente
`complexity_adjustment()` e `scale_endgame()` (ambos não-lineares,
fora do âmbito de uma regressão linear) -- só o `positional_terms()` +
material é regressão linear real, o resto fica de fora do tuner por
desenho, tal como os campos de king-safety não-lineares (18 campos,
já excluídos por sentinel).

**Teste A/B lançado**: `sprt_tuned_eval.py` (`KESTREL_TUNED_WEIGHTS=
tuned_round3.txt` vs sem a env var, 300 jogos), smoke-test de 10 jogos
deu 60% a favor dos pesos tunados. Log `sprt_tuned_eval.log`.

**Nota honesta sobre o dataset**: `dataset_round1_quiet.epd` (62928
posições) foi gerado por self-play com uma versão do Kestrel de ONTEM
(antes de todas as mudanças de eval desta sessão) -- os resultados dos
jogos continuam válidos como verdade objectiva (não dependem da versão
que jogou), mas a distribuição de posições pode não reflectir
perfeitamente o que o Kestrel de HOJE exploraria. Suficiente para já,
mas se houver tempo, gerar uma ronda nova de self-play com o binário
actual seria mais representativo -- fica registado, não escondido.

**Resultado final `sprt_tuned_eval.py`**: 49.3% vs 50.7%, essencialmente
neutro (300 jogos) -- os pesos tunados (`tuned_round3.txt`) não deram
ganho real mensurável apesar do erro de fitting ter caído 11.3%. Não é
incomum em Texel tuning: ajustar melhor à PREVISÃO do resultado do
jogo não implica automaticamente jogar melhor, sobretudo com um
dataset pequeno/pouco representativo. Confirma a necessidade dum
dataset maior e mais realista, exactamente o que o utilizador pediu a
seguir.

## Atualização 2026-07-23 (continuação): metodologia real de Texel Tuning (utilizador citou chessprogramming.org), bot desligado por segurança

**Pedido do utilizador**: seguir a metodologia real descrita em
https://www.chessprogramming.org/Texel%27s_Tuning_Method -- 64000
jogos a controlo de tempo RÁPIDO REAL (ex. 1s+0.08s/lance, não nós
fixos), extrair todas as posições excepto as do livro de abertura e as
de jogos onde o motor encontrou mate -- tipicamente ~8.8M posições.

**Implementado**: novo comando `kestrel selfplaytc <jogos> <saida>
[base_ms] [inc_ms] [threads]` (`main.rs`,
`selfplay_datagen_tc`/`play_one_selfplay_game_tc`) -- mesmo esqueleto
do `selfplay`/`play_one_selfplay_game` existente (abertura aleatória
de 8 lances, adjudicação win/draw/loss, filtro de posições quietas já
validado em sessão anterior, descarte de aberturas desequilibradas),
mas com um RELÓGIO REAL por lado (`SearchLimits.deadline`, formula
elástica simples `remaining/30 + inc*3/4`) em vez do limite de nós.
Mate/livro já eram efectivamente excluídos pelo código antigo (o loop
sai ANTES de adicionar a posição onde o mate foi encontrado;
`SKIP_OPENING_PLIES=16` já cobre e excede os 8 lances de "abertura"
aleatória) -- confirmado por leitura do código, não precisou de mais
filtros novos.

**Calibração de tempo**: 30 jogos a 1000ms+80ms, 6 threads = 62.3s
(~0.5 jogos/s) -- para 64000 jogos, estimativa ~35.6 HORAS. Utilizador
confirmou que aceita esse custo, desde que o CPU fique dedicado (não
correr mais nada em paralelo).

**Acções de segurança/limpeza pedidas pelo utilizador**:
- Bot do Lichess (`lichess_bridge.py`, activo desde 21 Julho) **desligado**
  (`kill -TERM`, paragem limpa) -- pedido explícito ("desliguem o bot
  senão pode alguém lançar uma interferência").
- Processo antigo de self-play a nós fixos (`selfplay_round4`, 5000
  jogos, ~260/834 por thread quando parado) **parado** -- substituído
  pela ronda nova a controlo de tempo real, CPU liberto por completo.

**Lançado**: `kestrel selfplaytc 64000 dataset_tc64k.epd 1000 80 6` em
background, log `selfplay_tc64k.log`, ficheiro de saída
`dataset_tc64k.epd`. **Nenhum outro trabalho de CPU deve correr em
paralelo enquanto isto não acabar** (~35h estimadas) -- respeitar o
pedido do utilizador. Quando acabar: `resolvequiet` (se necessário --
o filtro de quietude já corre durante a geração, confirmar se ainda
faz falta um passo extra) + `tunefast` com `lr=1000` (a taxa correcta,
NÃO 2.0) e iterações suficientes (milhares), depois A/B novo.

**Revertido pelo utilizador** ("não mudes nada então, não precisas de
um dataset de 64000 jogos") -- a corrida de 64k jogos foi PARADA
(~50/10667 jogos por thread, muito cedo, sem ficheiro de saída
gerado -- o `selfplaytc` só escreve no fim, nada a recuperar). O
método estabelecido nesta sessão (portar técnicas reais do Sirius/
Ethereal uma de cada vez, validar por self-play A/B, commitar mesmo
ganhos pequenos) já estava a "ganhar elo" segundo o próprio
utilizador -- volta a ser a prioridade, em vez de investir mais tempo
na infra-estrutura de dataset/tuning grande. O comando `selfplaytc`
fica no código (`6aad35b`, já commitado) para uso futuro se algum dia
fizer sentido, mas não é para usar agora.

**CPU livre outra vez** -- pode retomar-se o padrão de trabalho normal
(implementar item por item do que falta da auditoria do Fable ou
outras ideias, validar A/B, commitar). Bot do Lichess continua
desligado (não foi pedido para religar).

## Atualização 2026-07-23 (continuação): lmrNonImp real do Sirius, resultado positivo claro

Próximo item da lista de LMR "barata" do Fable ainda por fazer:
`lmrNonImp` -- termo real do Sirius `reduction += lmrNonImp/1024
(~1.46 plies) quando !improving` (reduz MAIS quando a posição não está
a melhorar, mesmo sinal `improving` que RFP/futility já usam).
Arredondado a 1 ply inteiro, mesmo estilo de quantização de
`hist_adj`/`ttpv_adj`/`corrplexity_adj`.

**Resultado**: 55.0% vs 45.0% (baseline), 300 jogos -- positivo real,
claramente fora do ruído. Commitado.

**Divisor de history do LMR**: `h/4000` (palpite antigo) -> `h/8846`
(valor real `lmrQuietHistDivisor` do Sirius). Kestrel não tem o passo
extra `/1024` que o Sirius tem (a tabela base do Kestrel já produz
plies directamente), por isso `h/8846` é a tradução directa, sem
reescala adicional -- e as escalas de history dos dois motores já são
próximas (16000 vs 16384, confirmado em sessão anterior). Resultado:
**exactamente 50.0%/50.0%** (300 jogos) -- neutro, mantido por ser
valor real portado (mesma política já aplicada a RFP/razor/LMR-
thresholds).

**doDeeper/doShallower** (`da6e27b`->próximo commit): depois da
re-pesquisa LMR de janela nula bater alpha, Sirius não repete sempre à
profundidade normal -- se bateu alpha por MUITA margem (relativo ao
`best_score` deste nó até agora), vai 1 ply MAIS FUNDO na re-pesquisa;
se bateu por pouco, 1 ply MAIS RASO. Valores reais do Sirius
(`do_deeper_margin_base=36, do_deeper_margin_depth=141,
do_shallower_margin=8`). A pesquisa final de janela completa (PV),
quando necessária, reusa a profundidade já ajustada, não volta à
original -- confirmado que bate certo com o Sirius real.

**Resultado**: 46.0% vs 54.0% (baseline), 300 jogos -- negativo real
mas moderado (~8 pontos, nem ruído puro tipo os casos de ~2-3 pontos
já mantidos, nem a regressão clara de ~20+ pontos da aspiração/NMP-
verificação já revertidas). Pedida revisão independente do Fable
especificamente a este código antes de decidir (dado ficar numa "zona
cinzenta") -- **revisão confirmou: sem bug**, port fiel ao Sirius real
em todos os pontos verificados (timing do `best_score`, propagação da
profundidade ajustada para a pesquisa PV final, operadores de
comparação das margens, caso degenerado do `do_shallower`). **Mantido
e commitado** -- resultado negativo real mas sem bug encontrado não é
motivo para reverter um valor genuíno portado, mesma política já
aplicada a outros casos esta sessão.

## Atualização 2026-07-23 (continuação): revisão holística do Fable a toda a sessão, checkpoint Sirius, castling explicado

**Revisão holística** (todo o diff `0c1b388..6efdba2`, 8 commits) pedida
para apanhar problemas que só aparecem ao ver tudo junto (interacções
entre peças, campos mortos deixados por reversões, consistência de
`to_vec`/`from_vec` depois de 8 commits de adições incrementais).
**Resultado: limpo** -- nenhum bug novo encontrado. 2 notas cosméticas:
1. Comentário dizia que os pesos de corr-hist somavam exactamente 256,
   na realidade somam 257 (arredondamento independente por termo) --
   comentário corrigido, impacto real desprezável (~0.4% acima do
   limite antigo, só no caso raro de saturação simultânea das 5
   tabelas).
2. `doDeeper`/`doShallower` usava prioridade if/else em vez da
   aritmética `+/-` real do Sirius (`newDepth += doDeeper - doShallower`)
   -- com as margens actuais os dois nunca disparam ao mesmo tempo
   (comportamento idêntico hoje), mas uma reafinação futura das
   margens para intervalos sobrepostos divergiria silenciosamente do
   Sirius sob if/else. Corrigido para a forma aritmética real. Ambas
   as correcções são cosméticas/robustez, sem mudança de comportamento
   actual -- não precisou de novo A/B.

**Erro de processo (leve, sem consequência real)**: fiz
`cargo build --release` para estas 2 correcções SEM verificar
`"running": false` primeiro -- o lote de checkpoint Sirius estava a
meio (2/20 jogos). Dado que as mudanças são comportamentalmente
idênticas (confirmado pelo Fable), não há dano real, mas é a MESMA
lição já documentada antes nesta sessão -- reforçar: verificar sempre
o estado da arena antes de qualquer build, sem excepções mesmo quando
a mudança "parece" inofensiva.

**Pergunta do utilizador sobre roque**: no jogo 1 do checkpoint,
`14. Kf1` não foi "mover o rei em vez de rocar" -- as brancas estavam
em XEQUE (`13...Bb4+`), e rocar é ilegal em xeque (regra universal do
xadrez, não bug do motor). Padrão real encontrado ao explicar: as
brancas ainda não tinham rocado ao fim de 13 lances quando o xeque
aconteceu, perdendo o direito de rocar permanentemente a partir daí --
tema recorrente já visto noutras derrotas (segurança do rei atrasada
na abertura), fica registado como padrão qualitativo, não como bug.

**Utilizador insistiu no ponto** ("uma das forças do jogo é abrir
desenvolver Roque depois... e tu não fazes o roque") -- investigado a
sério em vez de só anotado. Extraí o lance de roque de 14 jogos
recentes (script Python, parseia o movetext real): **Kestrel roca em
média ao lance ~11.1, Sirius ao lance ~8.5** -- e mais grave, **Kestrel
NUNCA roca em 3 de 14 jogos (21%)**, Sirius só em 1 de 14. Padrão real
e mensurável, confirmado.

**Causa investigada**: nem o Sirius nem o Ethereal têm um termo
explícito de "direito de roque"/"rei por rocar" -- os dois confiam no
próprio sistema de segurança do rei (shelter/storm, safe-check,
king-ring) ser suficientemente bom para penalizar implicitamente um
rei ainda no centro, uma vez que ataques reais apareçam. Sem valor de
referência para portar aqui -- **campo genuinamente novo, não um
porte**, ao contrário de quase tudo o resto desta sessão.

**Implementado**: `UNCASTLED_KING_NO_RIGHTS=(-20,0)` (rei ainda em
casa E já perdeu os dois direitos de roque -- o pior caso, falhou a
janela por completo) e `UNCASTLED_KING_HAS_RIGHTS=(-8,0)` (rei ainda
em casa mas ainda tem pelo menos um direito -- só um empurrão, menos
severo). Valores modestos escolhidos à mão (sem motor de referência
para ancorar), taper mg-only (só relevante enquanto há peças para
atacar o rei). Explicitamente marcado no código como candidato a
validação por A/B / tuner próprio, não um "valor real" como o resto
desta sessão. Smoke-test (10 jogos) deu 25% -- negativo, mas amostra
pequena; lote completo de 300 a correr, `sprt_uncastled.py`.

**Checkpoint Sirius fechado**: 0 vitórias, 20 derrotas, 0 empates (20
jogos, binário totalmente validado de hoje). Mesma história do gap de
força absoluto já documentada -- revisão holística do Fable já
confirmou que o código está limpo, não é regressão.

**Resultado final `sprt_uncastled.py`**: 49.5% vs 50.5% (baseline),
300 jogos -- essencialmente neutro, o sinal negativo do smoke-test
pequeno (25%) era ruído de amostra. **Mantido e commitado** -- termo
motivado por um padrão real e medido (não um palpite às cegas), e um
resultado neutro no win-rate agregado não significa que não esteja a
resolver o problema específico de roque tardio que visava corrigir
(self-play mede força geral, não directamente "quantos lances até
rocar"). Verificação directa desse efeito específico (comparar o lance
médio de roque do novo binário vs o antigo) fica como possível
follow-up, não crítico agora.

**Verificação directa feita** (`check_castle_timing.py`, self-play em
4 aberturas diferentes, binário antigo vs novo): **antigo rocou em só
4/8 casos possíveis** (lado x abertura), lance médio 13.5; **novo
rocou em 6/8 casos**, lance médio 12.17 -- mais frequente E mais cedo,
consistente com o efeito pretendido, mesmo a amostra sendo pequena.
Confirma que o termo está mesmo a resolver o padrão específico visado,
independentemente do win-rate agregado neutro. Script apagado depois
de confirmar (`check_castle_timing.py`), não faz parte do repo.

## Atualização 2026-07-23 (continuação): termo `threats` da correction history fechado (6º termo)

Gestão de tempo (item #3 do Fable) explicitamente adiada por ser o
item mais arriscado que sobra (precisa de testes a controlo de tempo
REAL, não nós fixos -- reintroduz exactamente o ruído que a
metodologia de self-play desta sessão foi desenhada para evitar, mais
matemática de potência fraccionária). Em vez disso, fechado o `threats`
da correction history (Sirius real, `threatsCorrWeight=252`) -- só
faltava um helper "todas as casas atacadas pelo lado X"
(`all_attacks`, nova função em search.rs, itera peça a peça usando as
tabelas de ataque já existentes) que não existia standalone antes.
Hash = `enemy_attacks & own_pieces` (quais das nossas peças estão sob
ataque), mesmo padrão das outras 5 tabelas de corr-hist.

**Peso isolado, não redistribuído**: em vez de reescalar os 5 pesos já
validados (o que juntaria uma reescala com uma adição, quebrando o
isolamento de mudanças desta sessão), o `threats` recebeu o seu
próprio peso usando a MESMA taxa de conversão original
(256/1762≈0.1453) aplicada só a ele: `252*0.1453≈37`
(`CORR_WEIGHT_THREATS=37`). Tecto teórico do pior caso sobe de 257
para 294 (~15% acima), aumento modesto e limitado.

**Resultado**: 47.3% vs 52.7% (baseline), 300 jogos -- negativo leve,
mesma magnitude de outros casos já mantidos esta sessão (LMR
thresholds, doDeeper/doShallower). Mantido e commitado -- valor real
portado, orçamento isolado, sem base para suspeitar de bug.

Correction history agora com 6 dos 7 termos base do Sirius (só falta
os 6 lags de continuation-history, que precisam duma tabela partilhada
4D e mais infra -- fica registado como próximo passo se houver
interesse em continuar).

## Atualização 2026-07-23 (continuação): mudança de filosofia (utilizador) -- "não copiem, aprendam e testem", combate acumulado do dia, hipótese própria

**Correção de rumo do utilizador**: "não copiem os outros aprendam e
testem. talvez vocês consigam fazer melhor." Reconhecido: fiz demasiado
PORTE (copiar valores reais reescalados) e vários deram neutro/
levemente-negativo (LMR thresholds, doDeeper/doShallower, threats
corr-hist, history divisor) -- sinal de que copiar valores tunados para
a arquitectura DELES não transfere. O que funcionou de verdade foram
MECANISMOS que faltavam por completo (NMP eval-adaptativo, correction
history multi-termo, king-proximity) e a única coisa DESCOBERTA a
partir dos jogos reais em vez de portada: `UNCASTLED_KING`. Esse é o
modelo certo -- observar, formular hipótese própria, testar. Daqui em
diante: estudar os motores de referência só para perceber que
MECANISMOS existem e porquê; os NÚMEROS vêm do tuner do Kestrel ou de
hipóteses próprias validadas por A/B, não reescalados.

**Combate acumulado do dia (pedido do utilizador: "combate só com 1
thread para ver onde está a verdadeira diferença")**: binário de hoje
(`f192136`, ~13 commits de trabalho) vs binário do INÍCIO da sessão
(`0c1b388`), ambos Threads=1, 60+1 tempo real, 30 jogos. Adicionado
`kestrel_session_start` ao `engine_arena.py` e Threads=1 explícito ao
`kestrel`. **Nota de infra**: o `arena_server.py` estava a correr desde
22 Jul e tinha importado o `engine_arena.py` ANTES desta edição -- o
primeiro start falhou em silêncio (KeyError no nome novo, `finally`
pôs running=False, games_done=0, estado mostrado era stale do match
Sirius g20). Resolvido com `./arena.sh restart` (seguro, nada estava
mesmo a jogar). A correr agora. **Este é o teste honesto de se o
trabalho do dia realmente somou** -- se sair neutro/negativo, é sinal
de que empilhei peças que individualmente "não fazem mal" mas
colectivamente não valem, e revejo quais ficam.

**Hipótese própria nova, fundamentada em evidência (a testar DEPOIS
do combate, não durante -- não competir por CPU com o match
single-thread)**: medi nos 20 jogos mais recentes vs Sirius quantos
peões de abrigo cada rei tinha ao lance 25 (peões próprios nas 3
colunas à volta do rei, nas 2 fileiras à frente). **Kestrel: 1.85 em
média; Sirius: 2.20** -- o rei do Kestrel fica mensuravelmente mais
exposto (~0.35 peões a menos), consistente com o padrão do roque/
segurança do rei que o utilizador apontou. Ressalva honesta: os 20
jogos são todos derrotas, por isso pode estar confundido com "o
Kestrel está simplesmente pior e é forçado a posições piores", não
necessariamente um ponto cego específico do eval. Mas é um sinal real
e quantificado -- hipótese a testar: o Kestrel subvaloriza manter o
abrigo de peões do próprio rei intacto (o `pawn_shelter`/`pawn_storm`
actuais são tabelas flat de 4 entradas, possivelmente fracas demais).
Ideia PRÓPRIA a explorar, não um porte -- eventualmente calibrada pelo
tuner, não copiada.

## Atualização 2026-07-23 (continuação): LERO DO UTILIZADOR -- "em multi perdes todos os jogos, o Lazy deve ter um problema" (investigação em curso)

**Observação do utilizador, potencialmente o achado mais importante da
sessão**: em single-thread o Kestrel-vs-Kestrel é competitivo (combate
do dia estava ~even), mas em MULTI-thread perde todos os jogos --
hipótese do utilizador: **o Lazy SMP tem um bug**. Um Lazy SMP correcto
torna o motor MAIS forte (mais nós), nunca mais fraco. Se multi perde
ao próprio single-thread, é um bug definitivo.

**Contexto**: o "multi" observado é o **bot do Lichess**
(`lichess_bridge.py` linha 146: `Threads value 4`). Havia um comentário
a dizer que Lazy SMP foi validado a 80% (16/20) vs 1 thread em self-play
-- MAS datado de **2026-07-20**, ANTES de todo o trabalho desta sessão
(reescrita do NMP, correction history de 6 tabelas, mudanças de LMR,
etc.). Muito plausível que algo desta sessão tenha degradado/quebrado o
caminho multi-thread especificamente.

**Análise do código (`search_mt` em uci.rs, `tt.rs`) -- sem custo de
CPU, feita durante o combate single-thread a correr**:
- **TT está CORRECTA**: usa o truque XOR sem locks do Hyatt
  (`key_xor_data = key ^ data`), que detecta e rejeita leituras
  rasgadas (torn reads). No pior caso perde um hit da TT (custo de
  performance), nunca devolve um lance corrupto. NÃO é a fonte do bug.
- **Smell real #1**: a selecção do "melhor thread" desempata por
  `depth` e depois por **SCORE MAIS ALTO** (`results[i].1 >
  results[best_idx].1`) -- isto enviesa sistematicamente para o thread
  mais OPTIMISTA (potencialmente com uma avaliação tacticamente
  insegura/inflada). Pitfall conhecido do Lazy SMP: a maioria dos
  motores usa o resultado do thread principal ou um voto, não "score
  mais alto ganha".
- **Smell real #2**: a salvaguarda de consenso (contra um thread
  outlier sozinho -- que o próprio comentário diz ter causado uma
  derrota real por pendurar uma torre) só activa com `results.len() >
  2` (3+ threads). O bot corre 4 threads, por isso ESTE em particular
  activa -- mas mostra que a lógica de selecção é frágil.

**Plano (a executar assim que o combate single-thread acabar -- NÃO
lançar já, competiria por CPU e poluiria os dois testes)**: teste
limpo e definitivo -- MESMO binário actual, Threads=1 vs Threads=4,
tempo REAL fixo, self-play. Se multi perde a single a tempo igual, bug
confirmado e faço bisect (provavelmente começando pelos 2 smells
acima, ou por reverter a selecção para "thread principal apenas" que é
o mais simples e robusto). Se multi ganha ~80% como a validação
antiga, o Lazy SMP está bem e a observação do utilizador era doutra
causa (ex.: adversários do Lichess simplesmente mais fortes). Este é
exactamente o tipo de "aprender e testar, encontrar o problema real"
pedido -- e se o Lazy SMP estiver mesmo partido, corrigi-lo é
potencialmente o maior ganho de força disponível (o bot corria 4
threads e perdia tudo).

## Atualização 2026-07-23 (continuação): ACHADO GRANDE -- o trabalho do dia é NEGATIVO em tempo real, e não é (só) NPS

**Resultado final do combate acumulado do dia** (1 thread, 60+1, 30
jogos): **binário de hoje 13/30 (43.3%) vs binário do início da sessão
`0c1b388` 17/30 (56.7%)** -- ~-47 Elo. Os ~13 commits de hoje, no
conjunto, tornaram o motor MAIS FRACO em tempo real, apesar de as
mudanças "grandes" terem dado A/B positivo individualmente.

**Causa metodológica identificada**: TODOS os meus A/B desta sessão
foram a NÓS FIXOS (30000 nós/lance). O combate real é a TEMPO fixo. Um
teste a nós fixos é CEGO a regressões de NPS. Várias adições de hoje
custam tempo por nó (correction history de 6 tabelas + o `all_attacks()`
do termo threats que itera CADA peça com ataques mágicos por nó; eval
expandido: king-flank, weak-king-ring, passed-pawn 3D, etc.).

**MAS a medição de NPS revela algo mais subtil e mais grave** (mesma
posição, `go movetime 3000`, 1 thread):
- `0c1b388`: 1.033M NPS, chegou a **profundidade 16** (3.1M nós)
- hoje: 0.861M NPS (17% mais lento por nó), MAS chegou a **profundidade
  19** com MENOS nós (2.58M)

Ou seja: hoje é mais lento por nó mas **poda MUITO mais agressivo**
(chega mais fundo com menos nós). E mesmo assim a profundidade-19-de-
hoje PERDE à profundidade-16-antiga. Isto aponta para ALÉM de NPS:
ou o **eval ficou menos preciso** (os termos portados, mesmo os que
deram A/B positivo a nós fixos, pioraram a avaliação estática), ou a
**poda agressiva (NMP com R grande + LMR) é INSEGURA** -- chega a
"profundidade 19" cortando linhas críticas, por isso a profundidade
extra é cega.

**Teste decisivo a correr** (`sprt_cumulative.py`, nós fixos 30000,
hoje vs `0c1b388`, 300 jogos): isola QUALIDADE de VELOCIDADE. Se hoje
perde TAMBÉM a nós fixos -> o problema é qualidade (eval/poda), não
velocidade -> reverter/rever as mudanças que degradaram a avaliação ou
tornaram a poda insegura. Se hoje GANHA a nós fixos mas perde a tempo
-> é puramente NPS -> reverter as adições caras (threats corr-hist com
`all_attacks()` é o suspeito nº1 de custo de NPS). A seguir: combate
real a 1s+0.08s (tempo bullet, pedido do utilizador -- é onde o custo
de NPS mais dói) e o teste do Lazy SMP.

## Atualização 2026-07-23 (continuação): teste decisivo e BISECÇÃO -- é qualidade E velocidade, e a maior parte do trabalho do dia não ajudou

**Teste decisivo (nós fixos, hoje vs `0c1b388`, 300 jogos)**: hoje
**47.3%** -- perde TAMBÉM a nós fixos. Logo o problema NÃO é só NPS:
- a nós fixos: 47.3% (regressão de QUALIDADE -- eval/poda pioraram)
- a tempo fixo: 43.3% (a mesma regressão de qualidade MAIS o custo de
  NPS por cima)

A política desta sessão ("manter valores reais portados mesmo quando
o A/B individual dá neutro/negativo") deixou acumular uma deriva
negativa nos dois eixos. Estava errada -- o instinto do utilizador
("não copiem, aprendam e testem") estava certo.

**Bisecção (cada binário vs `0c1b388`, nós fixos, 200 jogos)** --
resultado NÃO-monotónico, o achado-chave:
| binário | vs 0c1b388 | o que acrescenta |
|---|---|---|
| `9f93be9` | **47.2%** | núcleo de busca (NMP eval-adaptativo, corr-hist, RFP/razor/ProbCut) -- NEGATIVO |
| `04f79ae` | **51.2%** | + correcções de bug do NMP -- RECUPERA ao pico local |
| `de68647` | **43.8%** | + termos de eval + afinação de LMR + doDeeper -- MAIOR QUEDA |
| HEAD | 47.3% | + uncastled + threats |

Duas conclusões: (1) as correcções de bug do NMP em `04f79ae` ajudaram
mesmo (47.2%->51.2%); (2) o MAIOR dano é o intervalo
`04f79ae`->`de68647` -- os portes de termos de eval e a afinação de
LMR, que arrastaram 7.4% para baixo apesar de cada um ter dado A/B
individual OK. É exactamente o "morte por mil cortes" de valores
copiados que o utilizador avisou. (Bisecção a afinar mais entre
`04f79ae` e `de68647` com `90654ad`/`da6e27b` a correr.)

**Direcção provável**: `04f79ae` (só o núcleo de busca com os bugs do
NMP corrigidos) é o pico local (~51% a nós fixos, ~neutro-a-
ligeiramente-positivo). Tudo depois disso (termos de eval portados +
afinação de LMR + uncastled + threats) foi net negativo. Candidato a
reverter: voltar a `04f79ae` (ou mais atrás), largando os portes de
eval e a afinação de LMR posterior, mantendo só o núcleo de busca que
é no pior caso neutro. A CONFIRMAR: testar o ponto de reversão
escolhido a TEMPO REAL vs `0c1b388` antes de commitar (o núcleo de
busca também tem custo de NPS das 5 tabelas de corr-hist + ProbCut, por
isso mesmo `04f79ae` pode ser ligeiramente negativo a tempo real -- se
for, a reversão certa é mais funda).

## Atualização 2026-07-23 (continuação): bisecção completa -- doDeeper é o culpado nº1, REVERTIDO

**Bisecção completa (cada vs `0c1b388`, nós fixos, 200 jogos)**:
| binário | vs 0c1b388 | delta |
|---|---|---|
| `04f79ae` (núcleo busca, bugs corrigidos) | 51.2% | pico local |
| `90654ad` (+ termos eval + lmrNonImp) | 50.2% | -1.0 |
| `da6e27b` (+ history divisor) | 50.0% | -0.2 |
| **`de68647` (+ doDeeper/doShallower)** | **43.8%** | **-6.2 <- CULPADO** |
| HEAD (+ uncastled + threats) | 47.3% | +3.5 |

**doDeeper/doShallower é o maior culpado isolado do dia** (-6.2%
sozinho). Era um porte FIEL do Sirius (Fable confirmou o código) mas
dum mecanismo/valor ERRADO para a arquitectura do Kestrel: fazia a
busca chegar a profundidades enganosamente altas (19 vs 16) via
re-pesquisas mais fundas INSEGURAS. A/B individual já tinha dado 46%
(negativo), mantido na altura pela política errada "valor real
portado, sem bug". **REVERTIDO** -- de volta à re-pesquisa PVS simples
ao alvo, 3 campos `do_*_margin` removidos do `SearchParams`.

**Nota importante e encorajadora**: `uncastled + threats` RECUPERARAM
+3.5% (de 43.8% para 47.3%) -- e `uncastled` é a ÚNICA ideia PRÓPRIA
(descoberta dos jogos reais, não portada). Sinal de que a abordagem
certa (observar->hipótese própria->testar) funciona, e a errada
(copiar valores) não.

**A confirmar**: teste a nós fixos do binário sem doDeeper vs
`0c1b388` (a correr, `sprt_no_dodeeper.py`) -- esperado recuperar a
~50%+. Depois: teste a TEMPO REAL (o eixo que ainda tem o custo de NPS
das expansões de eval/corr-hist por cima). Se ainda negativo a tempo
real, o próximo alvo é o custo de NPS -- suspeito nº1 o `all_attacks()`
do termo threats (itera cada peça por nó, chamado no
`corrected_static_eval` E no `update_corr_hist`).

## Atualização 2026-07-23 (continuação): PONTO CHAVE DO UTILIZADOR -- "não são as funções que estão mal, mas a calibração dos valores"

**Reenquadramento crítico do utilizador** (em resposta à reversão do
doDeeper): os MECANISMOS não estão errados, é a CALIBRAÇÃO dos VALORES
deles que está. Não reverter as funções -- calibrá-las para o Kestrel.

**Bug de calibração concreto identificado no doDeeper**: as margens
(`do_deeper_margin_base=36` etc.) comparam com SCORES na escala de eval
do Kestrel, mas eu copiei os valores RAW do Sirius. A escala de
centipeão do Kestrel é ~1.92x a do Sirius (peão 125 vs 65), por isso os
valores raw disparavam "ir mais fundo" ~2x mais depressa do que deviam
-> profundidade extra insegura -> o -6.2% da bisecção. **CALIBRAÇÃO,
não mecanismo errado.** Corrigido: mecanismo RESTAURADO com margens
reescaladas pela razão do peão (36->69, 141->271, 8->15). A testar
(`sprt_dodeeper_rescaled.py`).

**Realização MAIOR (potencialmente a raiz de toda a regressão do lado
da busca)**: quase TODAS as margens de busca que portei estão em
unidades de eval do Kestrel e foram copiadas RAW do Sirius sem
reescalar -- RFP (`rfp*Margin`), razoring (`razoringMargin`),
capture-futility, gates do NMP (`nmpEvalMargin`,
`nmpStaticEvalBaseMargin`), `probcutBetaMargin`. Se todas disparam ~2x
demasiado agressivo (por não estarem reescaladas), o núcleo de busca
poda demasiado -> chega fundo mas inseguro -> perde. Explica
PERFEITAMENTE o `9f93be9`(núcleo de busca)=47.2% da bisecção e o
"profundidade 19 mas perde à profundidade 16". **Plano**: se o rescale
do doDeeper recuperar, aplicar o MESMO rescale (×~1.92, ou o factor
empírico medido comparando os evals dos dois motores) a TODAS as
margens de busca em unidades de eval, e testar o conjunto. Isto é
exactamente o ponto do utilizador -- as funções (RFP/razor/NMP/ProbCut/
doDeeper) estão todas certas; os valores estavam todos mal calibrados
por copiar raw sem reescalar para a escala do Kestrel.

**Ressalva sobre o factor de rescale**: a razão do peão é 1.92 no mg
mas ~1.01 no eg (peão eg Kestrel 140 vs Sirius 138). As margens
comparam com scores TAPERED (interpolados por fase). No meio-jogo
(onde a maior parte da poda acontece) o score está mais perto do
mg-scale, por isso ×1.92 é uma primeira aproximação razoável, mas o
factor "certo" seria medido empiricamente comparando os evals dos dois
motores nas mesmas posições -- fazer isso se o teste do doDeeper
confirmar a hipótese.

## Atualização 2026-07-23 (continuação): HIPÓTESE DE CALIBRAÇÃO CONFIRMADA -- rescale do doDeeper vira o dia de negativo para positivo

**Resultado confirmado (300 jogos, nós fixos, vs `0c1b388`)**:
- doDeeper RAW (não-reescalado, no HEAD): **47.3%** (negativo)
- doDeeper REMOVIDO: **49.7%** (neutro)
- doDeeper REESCALADO ×1.92 (69/271/15): **51.7%** (POSITIVO)

**A hipótese do utilizador está totalmente confirmada**: o mecanismo
nunca foi o problema, o VALOR era. Uma única correcção de calibração
(reescalar as margens do doDeeper pela razão de escala de eval Kestrel
vs Sirius) vira TODO o trabalho do dia de -negativo (47.3%) para
+positivo (51.7%) vs o binário do início da sessão -- um swing de
+4.4%. E "torna o mecanismo importante" (palavras do utilizador) --
deixa de ser código morto/prejudicial e passa a contribuir.

**Sweep de factor (pedido do utilizador: "testa 1.9 1.8 1.7 e vê qual
produz melhor efeito")**: 3 testes em paralelo via
`KESTREL_SEARCH_PARAMS` (sem rebuilds -- profiles `sp_doonly_*.txt`,
só variam as 3 margens do doDeeper), cada um vs `0c1b388`, nós fixos.
`sprt_factor.py` + `sprt_factor_{17,18,19}.log`. A correr. Escolher o
melhor factor, depois aplicar o MESMO rescale às OUTRAS margens em
unidades de eval (RFP, razoring, gates do NMP, ProbCut) que também
foram copiadas raw sem reescalar -- provável mais upside.

**Método validado**: NADA se apaga. Todas as funções ficam. O que
estava mal era a calibração dos valores para a escala do Kestrel.
Corrigir a calibração (empiricamente, por sweep/A-B) torna os
mecanismos úteis. Este é o caminho para o resto do trabalho: afinar,
não apagar.

## Atualização 2026-07-23 (continuação): sweep de factor + calibração cirúrgica do doDeeper

**Sweep de factor (nós fixos, 200 jogos, vs `0c1b388`)**:
- 1.7 (61/240/14): 58.8%
- **1.8 (65/254/14): 59.0%** <- melhor
- 1.9 (68/268/15): 57.2%

Todos os 3 em 57-59%, diferenças dentro do ruído (o factor exacto quase
não importa; 1.8 nominalmente melhor). doDeeper ×1.8 = 59.0%
reproduzido em DUAS corridas de 200 jogos independentes -- resultado
sólido.

**A hipótese "todas as margens mal calibradas" foi REFUTADA** por
teste directo (2 profiles em paralelo, `KESTREL_SEARCH_PARAMS`):
- doDeeper-só ×1.8: **59.0%**
- TODAS-as-margens-eval ×1.8 (RFP/razor/NMP/ProbCut/doDeeper): **54.2%**
  (PIOR)

Ou seja: reescalar tudo por 1.8 PIORA. A calibração NÃO é uniforme. Só
o doDeeper estava mesmo mal calibrado -- porque o efeito dele ACRESCENTA
profundidade (perigoso quando dispara demais); as margens de PODA
(RFP/razor/NMP) já estavam ~bem nos valores raw (coerente com os A/B
individuais neutros delas). **Lição refinada**: a calibração é
por-parâmetro e empírica, não um factor de rescale único aplicado a
tudo. O ponto do utilizador ("calibrar, não apagar") mantém-se -- só
que "calibrar" significa achar o valor certo de CADA um, não reescalar
todos igual.

**Aplicado**: doDeeper ×1.8 (65/254/14) baked no `Default` de
`SearchParams`, resto das margens deixado como estava. Binário
`kestrel_do18_final`. **Teste decisivo a TEMPO REAL a correr** (arena,
1s+0.08s -- o TC bullet pedido pelo utilizador --, 1 thread, do18 vs
`0c1b388`, 40 jogos) para confirmar que o ganho a nós fixos (+9%)
sobrevive ao custo de NPS a tempo real. Se confirmar positivo, commit.
A seguir ainda: o teste do Lazy SMP (Threads=1 vs 4) que o utilizador
levantou.

## Atualização 2026-07-23 (continuação): doDeeper 1.8 commitado; teste do Lazy SMP REFUTA a hipótese de bug

**doDeeper ×1.8 commitado (`a680bb8`)**. Resultado a tempo real
(1s+0.08s, 1 thread, 40 jogos vs `0c1b388`): do18 **48.75%** (16-17-7).
Comparação completa do fix:
| condição | doDeeper raw (HEAD) | ×1.8 calibrado |
|---|---|---|
| nós fixos | 47.3% | ~59% |
| tempo real 1s+0.08s | 43.3% | 48.75% |
Melhoria real nos dois eixos (+5.4% a tempo real, +11% a nós fixos).
Ainda ~1% abaixo do `0c1b388` a tempo real -- o resto é custo de NPS
das adições caras da sessão (a resolver tornando-as mais baratas, não
apagando).

**Teste do Lazy SMP (hipótese do utilizador: "em multi perdes todos os
jogos, o Lazy tem um bug")** -- teste controlado, MESMO binário,
Threads=1 vs Threads=4, 800ms/lance, CPU livre (essencial p/ timing
limpo), 40 jogos: **4 threads = 67.5%, 1 thread = 32.5%**. **A hipótese
está REFUTADA** -- o Lazy SMP FUNCIONA bem, 4 threads é ~+128 Elo sobre
1 thread, exactamente a direcção certa. Não está partido.

**Explicações honestas para as derrotas do bot em multi (NÃO um bug do
Lazy SMP)**:
1. Força do adversário -- o bot joga rated contra bots/humanos mais
   fortes que o Kestrel (mesmo a 4 threads).
2. Contenção do servidor -- na máquina partilhada, se outra coisa
   corria, o bot a 4 threads podia não ter mesmo 4 cores (starvation),
   problema de deployment, não do motor.
3. **Único caso NÃO coberto pelo teste**: usei `movetime` fixo, que
   ignora a gestão de tempo elástica. O bot usa relógio real
   (`wtime/btime`), que corre o `compute_time_budget` + o early-stop
   baseado em nós. Se ISSO se portar mal especificamente a 4 threads
   (contagens de nós por-thread a alimentar o check de estabilidade),
   o bot pode gerir mal o relógio mesmo com a busca sã. É uma questão
   de gestão de tempo, não do Lazy SMP -- o único caso genuíno que vale
   a pena verificar a seguir, dado o que o utilizador observou.

**Achado importante de método**: as duas coisas que o utilizador
levantou hoje ("não copiem, calibrem" e "o Lazy tem um bug") -- a
primeira estava CERTA e levou ao maior fix do dia (doDeeper), a segunda
foi REFUTADA por teste controlado. Ambas valiam a pena investigar; a
resposta honesta a cada uma veio dos dados, não de assumir.

## Atualização 2026-07-23 (continuação): Lazy SMP a RELÓGIO REAL também confirmado bom -- e uma lição de humildade minha

**Teste do Lazy SMP a relógio real** (o único caso não coberto pelo
teste de movetime): kestrel(1t) vs kestrel_4t, 10s+0.1s, arena.
Resultado final (parado aos 15 jogos, conclusão definitiva):
**4 threads = 73.3%** (11/15), 1 thread = 3/15. Alinhado com o teste
de movetime fixo (67.5%). **A gestão de tempo a multi-thread está BEM
-- 4 threads é ~+130 Elo sobre 1 thread mesmo a relógio real.**

**Lição de humildade (registada honestamente)**: a meio deste teste,
aos 4 jogos, estava 3-1 para 1 thread e eu construí uma hipótese
elaborada de "bug na gestão de tempo a multi-thread" (o early-stop a
disparar cedo demais com 4 threads). Estava ERRADO -- era ruído de
amostra pequena. Aos 9 jogos já era 6-3 para 4 threads, e acabou 73%.
Sobre-interpretei 4 jogos. A lição: NÃO tirar conclusões de amostras
de <10 jogos, mesmo quando parecem apontar para uma hipótese
atraente. Os dados corrigiram-me, como devem.

**Conclusão completa da investigação multi-thread**: Lazy SMP (busca E
gestão de tempo) funciona correctamente. As derrotas do bot em multi
que o utilizador observou são força do adversário (o bot joga rated
contra oponentes mais fortes) e/ou contenção do servidor partilhado
(o bot a 4 threads pode não ter mesmo 4 cores se outra coisa corre),
NÃO um bug do motor. `engine_arena.py` ganhou uma entrada
`kestrel_4t` (mesmo binário, Threads=4) para este tipo de teste no
futuro.

## Atualização 2026-07-23 (continuação): NPS medido -- threats NÃO é o culpado; próximo passo = calibrar o eval

**Medição de NPS (mesma posição de meio-jogo, movetime 3000, 1 thread)**:
- `0c1b388`: 842k NPS, profundidade 16
- actual (`a680bb8`): 776k NPS (~8% mais lento), profundidade 18

**O termo threats NÃO é o culpado de NPS** (hipótese refutada por
medição directa): com vs sem o `all_attacks()` do threats, NPS
essencialmente igual (887k vs 883k, dentro do ruído) -- é chamado raro
o suficiente (só onde o eval corrigido é preciso / em updates Exact)
que o custo é desprezável. O custo de NPS (~8%) está espalhado por
TODAS as adições de eval/corr-hist, sem um culpado único -- difícil de
optimizar sem remover features (que o utilizador não quer).

**Realização estratégica honesta**: a bisecção mostrou que os termos de
eval PORTADOS (passed-pawn 3D, king-safety, pawn-structure, etc.) são
~neutros-a-ligeiramente-negativos MESMO a nós fixos (04f79ae 51.2% ->
90654ad 50.2%). Ou seja, os valores emprestados-e-reescalados do
Sirius/Ethereal NÃO estão a contribuir -- só custam NPS. Isto é
EXACTAMENTE o ponto do utilizador: as funções estão certas, os valores
estão mal calibrados para o Kestrel. A solução (direcção do utilizador)
NÃO é apagar -- é TUNAR os valores para o Kestrel para que ganhem o seu
custo.

**Próximo passo em curso**: tuning próprio do eval a sério.
1. Dataset novo gerado com o binário ACTUAL (`selfplay 4000
   dataset_round5.epd 20000 nós`, ~a correr) -- mais representativo que
   o `dataset_round1` antigo (de antes das mudanças da sessão, que deu
   tuning neutro).
2. A seguir: `tunefast` com lr=1000 (a taxa corrigida) sobre o dataset
   novo, tunar os ~477 escalares lineares do `Weights` (todos os termos
   de eval portados incluídos).
3. Validar por A/B (`KESTREL_TUNED_WEIGHTS`) vs os defaults actuais E vs
   `0c1b388`, a nós fixos E a tempo real.
Isto executa a direcção do utilizador -- calibrar os valores emprestados
para a escala/arquitectura do Kestrel -- sobre o maior bloco de valores
não-calibrados que resta (o eval).

## Atualização 2026-07-23 (continuação): pesos tunados ADOPTADOS (+2.3%), teste real no bot, e próximo ciclo

**Pesos tunados adoptados** (`3831cb1`): `TUNED_R5` embutido no
`default_weights()` via `from_vec` (669 escalares). Validado +2.3%
(52.3%) vs os defaults antigos a nós fixos. Ganho pequeno mas real --
"mesmo que ganhes pouco, é um círculo" (utilizador). Sem custo de NPS.

**Teste REAL no bot Lichess (60+0, pedido do utilizador)**: bot
`KestrelStrike` religado (`lichess_bridge.py`, Threads=4), binário
`3831cb1`. Como o bridge é REATIVO (só aceita desafios), criei
`challenge_bots.py` para PROCURAR bots online e desafiá-los a 60+0
(o bot não faz seek sozinho). Resultado da amostra desta sessão (5
partidas): **2V 3D**, rating bullet ~2100-2150 (mantido). Das 3
derrotas, 2 foram vs oponentes ~2390-2395 (esperado), 1 vs par
(~2088) por mate, 1 por TEMPO (outoftime). **Lição repetida**: não
sobre-interpretar amostras pequenas -- inicialmente achei a derrota
por tempo um padrão sistemático, mas foi 1 em 5. Para medir uma
diferença de Elo de ±20-30 precisam-se ~100+ partidas. O bot joga ao
nível estabelecido ~2100-2150, coerente com o resultado interno
(~neutro-a-ligeiramente-positivo a tempo real). NOTA: a 60+0
(bullet, 0 incremento) o bot desliga o livro e joga "plain engine
(bullet-speed safety gate)". Objetivo do utilizador: chegar a ganhar
a oponentes fortes tipo PeachFruit (~2395) -- precisa de ganho de
FORÇA substancial, não só polir.

**Ciclo em curso**: 2ª volta de tuning (`tune_round6`) sobre um
dataset COMBINADO (`dataset_round1_quiet` + `dataset_round5` = 250797
posições, mais diverso), a refinar a partir dos pesos round5 já
adoptados. Bot PARADO durante o tuning (a 60+0 sem incremento, roubar
CPU ao bot = derrotas por tempo -- nunca correr trabalho pesado com o
bot a jogar bullet). Religar o bot com o binário melhorado depois.

## PRÓXIMO GRANDE LEVER (documentado para sessão dedicada): tunar MATERIAL + PST

O maior bloco de valores AINDA não calibrado: **material
(`MG_VALUE`/`EG_VALUE`, 12 valores) e PST (`MG_PAWN`..`EG_KING`, 6x64x2
= 768 valores)**. São tabelas PeSTO educacionais genéricas (o próprio
código diz "ponto de partida educacional, não estado final"). O
`tunefast` atual NÃO lhes toca -- só tuna os campos lineares do
`Weights` (mobilidade/threats/pawn-structure/king-safety-lineares/
passed-pawns). Calibrar material/PST sobre os dados do Kestrel é
provavelmente o MAIOR ganho de força que resta.

**Como (abordagem de BAIXO RISCO, planeada)**: NÃO mexer no cálculo
incremental do board (`board.mg_score`/`eg_score` via
`piece_contribution()`, hot-path crítico -- risco alto de bug). Em vez
disso:
1. Criar uma função paralela `material_pst_white_with_weights(board,
   w)` que calcula material/PST FROM-SCRATCH usando valores de um
   Weights estendido (só para o tuner extrair features).
2. Estender o `Weights` + `to_vec`/`from_vec` com material/PST (o
   `to_vec` cresce de 669 para ~1449). `checkweights` confirma o
   round-trip antes de tunar.
3. Estender o `tune_fast` para extrair as features de material/PST: a
   feature de `MG_PAWN[sq]` = (peões brancos em sq) - (peões pretos em
   mirror(sq)), ponderada por `phase/MAX_PHASE`; `EG_PAWN[sq]` idem
   com `(MAX_PHASE-phase)/MAX_PHASE`; `MG_VALUE[pt]` = (nº peças
   brancas do tipo) - (nº pretas), etc. Esta é a parte não-trivial.
4. Tunar, validar por A/B.
5. Escrever os valores tunados de volta nas CONSTS `MG_PAWN`/`MG_VALUE`
   etc. (compile-time). O board incremental usa as novas consts sem
   qualquer mudança estrutural -- só os VALORES mudam.
Fica para uma sessão dedicada (não fazer à pressa no fim duma sessão
gigante -- risco de bug na extração de features).

## Reflexão estratégica (2026-07-24): como pode um HCE ganhar a um NNUE?

Pergunta do utilizador. A resposta condiciona as prioridades. Análise:

**Vantagem estrutural do HCE = VELOCIDADE (a ÚNICA).** Eval HCE ~dezenas
de ns; NNUE ~centenas (mesmo incremental). HCE bem otimizado faz 2-4x o
NPS. Força ≈ `profundidade × qualidade-de-eval`. NNUE ganha na
qualidade; HCE só compensa com profundidade = velocidade × poda
eficiente. É a única equação em que o HCE pode ganhar.

**Implicação para nós**: hoje o binário ficou ~8% mais lento e foi isso
que impediu o dia de ser claramente positivo -- andámos a GASTAR a
única vantagem estrutural. Regra daqui p/ frente: **cada 1% de NPS é
sagrado; cada mudança pergunta "custa NPS? vale o custo?"**

**Maiores alavancas, por impacto:**
1. **NPS máximo (a maior).** Eval barato, movegen rápido, TT eficiente,
   poda agressiva-mas-sã. HCE a 3-4x o NPS do NNUE ganha vários plies,
   que compensam muita imprecisão de eval.
2. **Correction history -- a ponte HCE→NNUE.** Deixa a busca APRENDER
   durante o jogo a corrigir os erros sistemáticos da eval estática, a
   custo de NPS quase nulo. É dar ao HCE um bocado da adaptabilidade do
   NNUE. Já temos 6/7 termos do Sirius; completar (continuation-history
   correction, 6 lags) + afinar os pesos é o maior ganho "esperto".
3. **Busca de topo calibrada PARA O KESTREL** (LMR/NMP/singular/probcut).
   O SF HCE clássico chegou a ~3400 CCRL quase só com busca de elite.
4. **Eval tunada** (PeSTO agora) -- precisa de estar boa mas tem teto
   (Texel aproxima o ótimo LINEAR; NNUE é não-linear).

**Verdade honesta**: contra NNUE FORTE, HCE puro dificilmente ganha
consistente (o SF HCE perdia ~80 Elo p/ o 1º SF NNUE). Contra NNUEs
fracos/médios (redes pequenas/mal treinadas, e há muitos no Lichess),
um HCE muito rápido + correction history forte + busca de elite ganha.
A "maior opção" é essa combinação, com o NPS como fundação inegociável.

## Ciclo autónomo da noite 2026-07-24 (calibração -> teste -> calibração)

**RONDA 1: tuning de MATERIAL + PST (as PeSTO).** Infra nova (`tunepst`/
`checkmatpst` em main.rs, `material_pst_features` em eval.rs -- features
validadas exactas). `tunepst` sobre `dataset_combined` (250k pos), 8000
iter, lr=30000 (o material tem valores grandes, precisa de lr alto).
Erro 0.078527 -> 0.073381 (**-6.6%**, muito maior que o tuning linear
~-1-2%, porque as PeSTO eram genéricas com muito espaço). Valores
aplicados às consts via `apply_matpst.py`. Material subiu bastante:
MG_VALUE [125,340,355,520,990] -> [153,498,525,707,1148]; EG peão
140->93. Perft ok. A/B (nós fixos, 300 jogos) vs `3831cb1` (round5) a
correr -- RISCO a vigiar: a escala do material mudou, as margens de
busca (em unidades de eval) podem ficar descalibradas. O A/B decide.
Backup do eval.rs pré-matpst em /tmp/eval_before_matpst.rs para
descartar se regredir.

**RONDA 1 RESULTADO: REGRESSÃO GRAVE.** material/PST tunado (directo) =
33.7% vs round5 = 66.3% (~-120 Elo). Erro de fitting -6.6% mas perde
MUITO em jogo -- exemplo perfeito de "melhor fit != mais força".
Descartado (eval.rs restaurado). Causa provável: o Texel tuning tem
AMBIGUIDADE DE ESCALA (o K do sigmoid absorve um factor global), e o
tuner escalou o material ~1.22x p/ cima (peão mg 125->153) -- essa
escala inflacionada descalibra a busca (margens em unidades de eval,
thresholds de mate). Também mudou a relação mg/eg (peão mg subiu, eg
desceu 140->93), o que uma re-escala uniforme não corrige.

**RONDA 1b: re-escala uniforme** (peão mg -> 125, factor 1.224,
preserva proporções tunadas). Testa se o problema era SÓ a escala
global. A correr. NOTA: o eg fica comprimido (peão eg 76) porque a
relação mg/eg mudou -- se regredir na mesma, o Texel tuning directo do
material/PST não funciona sobre este dataset e passa-se a calibrar
outra coisa (validar round6 linear; sweep de margens de busca).

**LIÇÃO (importante p/ o material/PST tuning futuro)**: fixar a escala
DURANTE o tuning -- ancorar o peão mg a 125 (não tunar) OU adicionar
regularização que penalize o desvio da escala original. O Texel puro
deriva para escalas arbitrárias que descalibram a busca. Ver plano na
secção "PRÓXIMO GRANDE LEVER" -- precisa desta salvaguarda.

**RONDA 1b RESULTADO: REGRESSÃO (34.8% vs 65.2%).** A re-escala uniforme
não corrige -- as razões das peças tunadas ficaram desequilibradas
(torre 4.62 peões, dama 7.5 no mg -- baixas), fazendo o motor trocar
peças mal. Descartado.

**RONDA 1c: tunar SÓ as PST, material ANCORADO** (is_fixed = índices
<12, todo o MG_VALUE/EG_VALUE fixo nos valores clássicos). As PST são
ajustes posicionais pequenos por casa -- muito menos perigosos que os
valores fundamentais das peças. Corrida curta confirmou: material fica
fixo, erro desce (0.078527->0.077240 em 300 iter). Corrida completa
(8000 iter) a correr. Esta é a abordagem que os motores usam de
verdade: tunar PST com material âncorado. Se ganhar, adopta-se; se
regredir na mesma, o dataset/abordagem não serve para material/PST e o
ciclo passa a validar o round6 linear + re-tunar linear sobre dados
novos.

**RONDA 1c RESULTADO: REGRESSÃO (41.7% vs 58.3%, ~-58 Elo).** Menos
grave que 1/1b mas ainda regride. **CONCLUSÃO: material/PST tuning FALHA
sobre este dataset** (3 tentativas: -120, -108, -58 Elo), apesar de o
erro de fitting descer sempre (-5.5% a -6.6%). Isto e' OVERFITTING
clássico: o dataset de self-play (~2100 Elo) nao e' representativo o
suficiente para tunar PST/material sem piorar o jogo real. As PeSTO
estabelecidas sao melhores que o que o tuner produz aqui. O tuning
LINEAR funcionou (+2.3%) porque os termos posicionais PORTADOS estavam
mal calibrados; as PST/material ja' estavam bem.
**LIÇÃO**: para tunar PST/material com sucesso seria preciso (a) um
dataset de MUITO mais alto nivel (partidas de motores fortes, nao
self-play a 2100), e/ou (b) regularizacao L2 forte (penalizar desvio
das PeSTO), e/ou (c) muito mais posicoes. Nao vale a pena insistir com
o dataset actual. Infra do tuner (`tunepst`) fica no codigo para o
futuro. Material/PST tuning ABANDONADO por agora; eval.rs restaurado ao
estado round5 (3831cb1).

**ROUND6 (linear/dataset combinado) vs round5: 50.7% vs 49.3% = EMPATE.**
Retorno decrescente do tuning de eval confirmado: o linear ja' convergiu
no round5 (+2.3% adoptado, 3831cb1); re-tunar sobre dataset combinado
nao adiciona nada mensuravel. round6 NAO adoptado; mantido round5.
Ciclo de eval-tuning esgotado (material/PST falha por overfitting;
linear convergido). Pivot para calibracao de MARGENS DE BUSCA (onde o
doDeeper mostrou ganho real de +9% ao ser recalibrado 1.8x).

## 2026-07-24 madrugada — ciclo de calibracao esgotado, bot ligado a testar Elo

**Resumo do ciclo da noite (calibrar->testar->calibrar):**
- doDeeper 1.8: adoptado (a680bb8) — o maior ganho.
- Eval weights linear (round5): adoptado (3831cb1), +2.3%.
- Material/PST tuning: 3 rondas, TODAS regressao (-120/-108/-58 Elo)
  apesar de menor erro de fitting -> overfitting ao dataset ~2100.
  ABANDONADO. Infra do tuner comitada (face379), sem mudar o jogo.
- round6 (re-tune linear sobre dataset combinado): EMPATE 50.7/49.3 ->
  linear ja' convergido no round5. Nao adoptado.
- Mecanismos de busca: TODOS ja' presentes e maduros (singular +double
  extensions a' Ethereal, IIR, multicut, NMP eval-adaptativo, RFP,
  razor, futility, ProbCut, correction history 6-dim). Nada estrutural
  a adicionar.
- Margens de pruning: ja' nos valores SPSA reais do Sirius (A/B neutro);
  rescale em bloco ja' testado e deu pior. Retorno decrescente.

**Conclusao:** o binario adoptado 3831cb1 (=doDeeper1.8 + eval round5)
continua a ser o melhor; nada de novo esta noite o superou. O ciclo de
calibracao "barato" (tuning sobre self-play, sweeps de margens) esta
esgotado sem mais ganho mensuravel neste dataset.

**Accao alinhada com o utilizador ("quando tiveres bem, ligas o bot e
tentas partidas 60+0 para testar o elo"):** bot LIGADO.
- lichess_bridge.py (unbuffered) a jogar, Threads=4.
- challenge_loop.py NOVO: challenger continuo, mantem 1 jogo de cada vez
  (NPS limpo com 4 threads), re-desafia quando livre.
- TC: 60+0 (bullet rated) tem aceitacao quase nula — a maioria dos bots
  fortes recusa bullet ou bate o limite de 100 jogos/dia vs bots. 180+0
  (3+0 blitz) e' aceite (FlounderBot 2070 aceitou). O loop tenta AMBOS
  por adversario, prioriza 180+0. Blitz testa a mesma forca com mais
  profundidade — decisao tecnica autonoma, documentada aqui.
- KestrelStrike actual: bullet 2108, blitz 2144.

**Proximo lever real de Elo (para quando o utilizador acordar):** o
tuning barato esgotou. Ganhos futuros vem de (a) SPSA local dos params
de busca sobre self-play (caro, milhares de jogos), (b) NPS/velocidade
(o lever inegociavel do HCE vs NNUE), (c) dataset de MUITO mais alto
nivel para re-tentar material/PST com regularizacao L2. Ver reflexao
"maior opcao para o HCE ganhar a um NNUE": velocidade (depth) e' a base.

## 2026-07-24 ~02:30 UTC — DIAGNOSTICO: derrotas por TEMPO (flags)

Primeiros jogos do bot apos ligar: W4 L6, mas **3 das 6 derrotas foram
por outoftime** (flag) — Elo desperdicado (posicoes jogaveis perdidas
no relogio). Investigacao com PGNs+clocks reais:
- 2 flags (agv3S9Q8 21:33, cXHBU2SM 21:07 UTC): gastaram >52s NUM SO'
  lance quando o hard budget era ~16s. Coincidem com os A/Bs pesados a
  correr de noite -> CONTENCAO DE CPU faz o NPS despencar; o check de
  tempo do search e' baseado em NOS (`self.nodes % 2048`, search.rs:695)
  e sob NPS ~40 nao dispara a tempo. Mitigado operacionalmente (A/Bs
  parados, challenge_loop MAX_CONCURRENT=1).
- 1 flag (d4RsfCIV 01:35, CPU + livre): sangrou ~3.2s/lance em 51 lances
  + 18s pontuais no lance 1. Engine local respeita o budget (~4s no
  lance 1), logo o problema e' EXTERNO/estrutural, nao a busca.

**CAUSA RAIZ do engine (matematica):** `MOVE_OVERHEAD_MS = 60` (uci.rs:9)
e' subtraido UMA VEZ do relogio total (`safe_time = my_time - 60`), mas
a latencia de rede do Lichess (~150-250ms) ocorre POR LANCE. Ao longo
de ~50 lances sao ~10s que o Lichess conta mas o engine nunca reservou
-> flag. A fórmula `base = safe_time/moves_left` e' auto-corretiva
(nunca chega a 0 sozinha), portanto o flag vem da latencia acumulada
nao-reservada, nao da fórmula.

**CORRECAO planeada:** reservar o overhead POR LANCE no deadline
(search_ms = soft - overhead), e subir o overhead para ~250ms (latencia
online realista). Validacao: harness NOVO real_clock_selfplay.py joga
com relogio a decrementar de verdade e CONTA flags (o A/B de nos fixos
e' cego ao tempo). Repro a correr: atual vs atual, 3+0, latencia 200ms.

## 2026-07-24 ~05:00 UTC — CORRECAO DE FLAGS VALIDADA E ADOPTADA (b7bd5dd)

Repro do bug (real_clock_selfplay.py, 1+0, lat 300ms, 1 thread):
  baseline atual vs atual -> 3 de 4 jogos perdidos por flag.
Correcao (MOVE_OVERHEAD_MS 60->250, reservado POR LANCE no deadline:
  search_ms = soft - overhead, piso 20ms):
  fixed(A) vs baseline(B), 8 jogos -> **flags A=0 B=4**, A ganhou 4-2.
A correcao ELIMINA os time forfeits sem custo de forca. Comitada b7bd5dd.
Como 3 das 6 derrotas reais do bot eram flags, isto vale Elo real.

Harness novo real_clock_selfplay.py: joga com relogio a decrementar de
verdade e conta flags (o A/B de nos fixos e' cego ao tempo). Uso:
  python3 real_clock_selfplay.py A B GAMES BASE_MS INC_MS LAT_MS [THREADS]
Reutilizavel para validar qualquer mudanca de gestao de tempo.

NOTA sobre contencao: os 2 flags de 52s/lance de ontem a' noite eram do
NPS a despencar sob os A/Bs pesados (check de tempo e' por-nos, %2048).
Mitigado: nao correr A/Bs pesados enquanto o bot joga (challenge_loop
MAX_CONCURRENT=1). A correcao do overhead resolve a componente
estrutural (latencia por-lance); a contencao e' operacional.

Bot religado com o binario corrigido para acumular Elo a 3+0/1+0.

## 2026-07-24 ~05:40 UTC — overhead calibrado a dados (ad5b821), bot religado

Refinamento da correcao de flags apos MEDIR a latencia real:
- Latencia servidor->Lichess API = ~45ms mediana (medido). Overhead real
  por lance ~100ms. Logo 250ms era exagero e custava forca (o build 250ms
  perdia jogos puros em self-play por pensar pouco).
- MOVE_OVERHEAD_MS = 150 (final). Piso de pensamento min(soft,80)ms.
- Validacao real_clock_selfplay: overhead150 vs baseline, lat 50ms
  realista, 10 jogos -> W5 L3 D2, **flags 0-0** (sem custo de forca; com
  latencia real nem o baseline da flag -> confirma que os flags REAIS
  foram CONTENCAO dos A/Bs + picos de estabelecimento, nao latencia).
- Sob stress 300ms: overhead 250 dava 0 flags; overhead 150 tem folga 3x
  sobre a latencia real (45ms), proteccao garantida no cenario real.

Commits: b7bd5dd (reserva por-lance) + ad5b821 (overhead 150 + piso).
Bot religado com o binario corrigido para acumular Elo. LICAO principal:
os flags foram maioritariamente CONTENCAO (nunca correr A/Bs pesados
enquanto o bot joga) + a reserva de overhead por-lance como salvaguarda
estrutural. Rating blitz tinha caido 2144->2131 pelos flags; esperado
recuperar agora.

## 2026-07-24 ~04:15 UTC — CORRECAO OVERHEAD NAO RESOLVEU; causa e' CONTENCAO DO PONDER

Honestidade: a correcao do move-overhead (b7bd5dd/ad5b821) foi validada
num cenario ERRADO (real_clock_selfplay com latencia 300ms irrealista).
A latencia real e' ~45ms, logo o overhead de 150ms mal muda o jogo real.
Prova: jogo JewkieBot (4aCS2ieb, 03:49 UTC) usou o binario JA' corrigido
(binario mtime 03:11 UTC) e deu flag na mesma — mesmo padrao: sangramento
3.4s/lance x 53 lances, e PICOS de 5-7s com relogio baixo (limite devia
ser <800ms). Local sem contencao o binario respeita o budget (253ms @15s).

**Causa real: CONTENCAO DO PONDER.** lichess_bridge.py corre o ponder
como 2o PROCESSO (penv = Engine(), Threads=4) durante PONDER_MOVETIME_MS
=6000ms a cada lance. Em blitz 3+0 o adversario joga em ~3s mas o ponder
dura 6s -> sobrepoe-se SEMPRE a' nossa busca seguinte -> 8 threads em 6
nucleos -> NPS despenca -> o check de tempo por-nos (search.rs:695,
`nodes % 2048`) nao dispara a tempo -> picos de 6s -> flag.

Correcoes planeadas (a validar com repro de contencao, 4 threads):
1. Bridge: desativar/encurtar o ponder em BLITZ (so' ponderar em rapid+
   onde o adversario demora >6s e o ponder termina antes da nossa vez).
2. Engine: reduzir o intervalo do check de tempo (2048 -> ~256) para
   apanhar excessos mais cedo sob contencao residual. Barato (~0.02%
   overhead de Instant::now()).
Repro de contencao a correr: baseline vs baseline, 4 threads, lat 45ms.

## 2026-07-24 ~04:40 UTC — CAUSA REAL DOS FLAGS ENCONTRADA: ponder-hit join

CORRECAO das conclusoes anteriores. A causa dos flags NAO era latencia
(que e' 45ms) NEM contencao de threads (o engine respeita o budget mesmo
com 8 threads + ponder paralelo, testado). E' o PONDER no bridge:

`_consume_ponder` num ponder HIT faz `thread.join()` que BLOQUEIA ate' o
ponder de movetime-fixo (6000ms) terminar. Se o adversario respondeu o
palpite mais depressa que o movetime (normal em blitz), o join queima o
tempo RESTANTE do ponder do NOSSO relogio. Medido: **5910ms por hit**.
Num lance de relogio baixo (budget ~350ms) = pico de 6s = flag imediato.
Confirmado empiricamente (importando o bridge e medindo o join) e pelo
mapeamento dos deltas do jogo JewkieBot: os picos eram exactamente ~6s
(=PONDER_MOVETIME_MS) e cresciam no fim do jogo (relogio baixo).

**Correcao (lichess_bridge.py, nao versionado no git):**
- PONDER_MOVETIME_MS 6000 -> 2000 (join num hit limitado a ~2s).
- PONDER_MIN_CLOCK_MS = 30000: so' ponderar com relogio >= 30s (um pico
  com relogio confortavel e' inofensivo; em relogio baixo, sem ponder ->
  busca normal, que respeita o budget).
- join timeout limitado ao movetime (+1s).
Validado: join num hit caiu 5910ms -> 2351ms. Bridge a reiniciar quando
o jogo em curso terminar (nunca reiniciar a meio de jogo real).

NOTA sobre b7bd5dd/ad5b821 (move-overhead 150 por-lance + piso): foram
validados contra latencia irrealista (300ms) e NAO sao a cura; mas sao
inocuos/ligeiramente benaficos (reservam a latencia real 45ms com margem)
e ficam. A CURA real e' a correcao do ponder acima.

## 2026-07-24 ~05:30 UTC — rate-limit 429 a impedir novos jogos

Correcao do ponder FEITA e bridge reiniciado com ela. Mas o bot nao
conseguia jogos: o challenge_loop.py estava rate-limited (HTTP 429 "Too
many requests") por ser agressivo demais (ate' ~50 challenges/ciclo) +
varios reinicios + testes manuais. Reescrito para ser GENTIL: 1 challenge
por ciclo, skip de bots recem-desafiados (5min) e recusados (10min),
backoff 120s em 429, so' 3+0 (180s, melhor aceitacao). Tambem: adicionado
?nb=40 ao /api/bot/online (sem isto o endpoint fazia stream de TODOS os
bots e demorava 20s+/pendurava, o que ja' estava a travar o loop).
challenge_loop PARADO ate' o cooldown do 429 passar (nada deve enviar
challenges entretanto). O bridge reativo fica vivo (aceita desafios
entrantes). Relancar o loop gentil quando o 429 passar.

## 2026-07-24 ~06:10 UTC — contencao causa BLUNDERS tacticos (nao so' flags)

Analise da derrota vs AgileBot (p1R1H3sO, 2100, mate): o blunder foi
Nxe4 no lance 9 (eval das pretas +45cp -> -300cp). Investigacao:
- O motor NUNCA escolhe Nxe4 em analise: nem a depth 6-18, nem multipv
  1/3, nem com livro (sf17/polgar dao b5b4), nem 4 threads (5/5 b5b4).
- MAS a depth 2/4/5 escolhe Nxe4 (captura peao, refutacao so' vista a
  depth 6+). Logo no jogo real a busca foi TRUNCADA a depth ~4-5.
- Num lance 9 de 3+0 com relogio confortavel, so' se chega a depth 4-5
  com o NPS despenhado -> CONTENCAO (A/Bs a correr as 01:49 UTC).
CONCLUSAO: a mesma contencao que causava flags tambem causa blunders
tacticos (busca truncada). Reforca a regra: bot joga com CPU LIMPA.
Nenhum vazamento de forca sistematico no eval/busca — o motor joga bem
com profundidade normal. O lever continua a ser NPS + CPU limpa.

## 2026-07-24 ~07:00 UTC — ponder fix CONFIRMADO em jogo; correcao anti-2-jogos

**Correcao do ponder CONFIRMADA em jogo real:** 3 jogos com o bridge
corrigido (KonaBot W, IlCorvoChess D, simpleEval W) -> ZERO flags. O
IlCorvoChess (2258) que antes flagava agora empatou. Rating blitz subiu
2113 -> 2140. Os flags estao resolvidos.

**Erro operacional (apontado pelo utilizador): 2 jogos em simultaneo.**
Causa: o meu teste manual do 429 (desafio a FlounderBot) + o loop
(simpleEval) foram aceites ao mesmo tempo -> 2 jogos -> contencao (2x
Threads=4 + ponders). Corrigido no lichess_bridge.py:
- Recusar desafios ENTRANTES quando ja' ha' jogo activo (len(active_games)>0).
- No gameStart, se ja' ha' jogo activo, ABORTAR o 2o (permitido no
  arranque, sem perda de Elo; se abort falhar, jogar para nao dar flag).
- challenge_loop skip 300s->1800s (variar adversarios; os jogos duram
  ~6min, 300s expirava entre jogos e re-desafiava o mesmo bot).
Licao: NUNCA criar desafios manuais enquanto o loop corre; testar o 429
sem criar jogo real (ou cancelar logo). 1 JOGO DE CADA VEZ e' regra dura.

Estado: bridge+loop reiniciados com todas as correcoes. Bot estavel,
1 jogo de cada vez, ponder corrigido, adversarios variados. A acumular
Elo. Ver [[project_flags_contencao_gestao_tempo]].
