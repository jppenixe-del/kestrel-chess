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
