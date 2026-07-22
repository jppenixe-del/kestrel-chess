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
