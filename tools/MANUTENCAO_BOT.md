# Manutenção do bot

O essencial em quatro comandos. O porquê de cada regra está na secção final —
todas foram aprendidas a perder jogos.

```
./bot.sh status                 o que está a correr, onde, com que opções
./bot.sh start                  arranca a jogar a sério
./bot.sh start heatmap          arranca sem busca (só a avaliação)
./bot.sh stop                   pausa e para quando o tabuleiro esvaziar
```

---

## Onde é que ele corre

Em **uma** das duas máquinas, nunca nas duas:

| | |
|---|---|
| **servidor** (esta) | `~/kestrel_joao/`, 6 cores partilhados com testes |
| **Napoleão** (`ssh napoleon`) | `~/kestrel_bot/`, 6c/12t, 25% mais rápido por thread |

**Dois clientes com o mesmo token disputam os mesmos jogos.** Não é degradação,
é caos: ambos respondem ao mesmo lance. O `./bot.sh start` recusa-se a arrancar
se detectar o outro lado a correr, e o `status` avisa se houver conflito.

Para trocar de máquina: `./bot.sh stop` de um lado, arrancar do outro.

---

## Ligar e desligar o modo heatmap

```
./bot.sh start heatmap                    # 2 plies (por omissão)
HEATMAP_PLIES=1 ./bot.sh start heatmap    # só a avaliação, nada mais
./bot.sh stop && ./bot.sh start           # voltar ao normal
```

O que muda:

| | suite (214 erros reais) |
|---|---|
| 1 ply — avaliação pura, **zero nós** | 37 |
| 2 plies — com a melhor resposta dele | 43 |
| busca a sério | 78 |

Serve para **ver a avaliação sem nada à frente**. Um termo com o sinal trocado
ou uma amplitude que esmaga o resto aparece logo no lance. Não serve para
ganhar: joga a menos de metade da força.

Em modo heatmap, **desafia em amigável**:

```
./bot.sh loop casual
```

---

## Procurar adversários

```
./bot.sh loop            desafia a valer (mexe no rating)
./bot.sh loop casual     desafia em amigável (rating intacto)
./bot.sh noloop          deixa de procurar
```

O bot **aceita** desafios sempre que o bridge está de pé; o loop é só para ele
próprio ir à procura. Se ninguém joga com ele, é quase sempre porque o loop
está parado.

---

## Instalar uma versão nova

```
cd Kestrel && cargo build --release && cd ..
./bot.sh install
```

Instala aqui e copia para o Napoleão (que a usa no próximo arranque de lá).
**Os jogos já a decorrer continuam com o binário antigo** — o motor é lançado
uma vez por jogo.

Confirmar que as duas máquinas têm o mesmo:

```
md5sum kestrel_bot_bin; ssh napoleon 'md5sum ~/kestrel_bot/kestrel_bot_bin'
```

Já jogámos partidas inteiras com uma versão velha sem dar por isso.

---

## Variáveis

| variável | para quê | omissão |
|---|---|---|
| `KESTREL_HEATMAP_ONLY=1` | joga só da avaliação, sem busca | desligado |
| `KESTREL_HEATMAP_PLIES` | 1 = só o nosso lance, 2 = com a resposta dele | 1 |
| `KESTREL_THREADS` | threads do motor | 4 |
| `KESTREL_ELO_BELOW/ABOVE` | banda de rating que aceita | 3000/3000 (todos) |
| `KESTREL_CASUAL=1` | o loop desafia em amigável | desligado |
| `KESTREL_ALLOW_RATED=1` | aceita desafios **recebidos** a valer | desligado |
| `KESTREL_PROFILE` | ficheiro de perfil de avaliação | nenhum (usa o V3 do binário) |

Duas que confundem:

**`ELO_BELOW/ABOVE`** decide de quem ele aceita desafios. Estava em 300/300 e
recusava em silêncio quem estivesse fora de 1990-2590 — parecia avaria.

**`ALLOW_RATED`** só afecta desafios **recebidos**. Os que o loop **envia** vão
a valer salvo com `KESTREL_CASUAL=1`.

---

## Pausar sem matar

```
./bot.sh pause      recusa desafios novos, os jogos a decorrer terminam
./bot.sh resume
```

**Nunca matar o processo para "parar o bot".** Reinícios seguidos levam a HTTP
429 durante horas e o bot fica fora do ar. A pausa é um ficheiro (`BOT_PAUSED`)
precisamente para isso.

---

## Quando algo corre mal

**Ninguém joga com ele** → `./bot.sh status`. Quase sempre o loop está parado,
ou a banda de Elo está fechada.

**Perdeu por bandeira** → ver o relógio do jogo. Se houver **um lance com um
gasto enorme** (40s+ num lance com 40s no relógio), não foi gestão de tempo: foi
falta de CPU. Alguém pôs testes pesados a correr na mesma máquina. Foi assim
que se perdeu um jogo real.

**Joga como uma versão antiga** → comparar os md5. O `install` não mexe em jogos
a decorrer, e um `scp` que falha é silencioso.

**Está a jogar mal de repente** → confirmar se o modo heatmap está ligado:
`./bot.sh status` mostra as variáveis do processo.

---

## Regras que custaram jogos

- **O bridge fica sempre de pé.** Pausar é recusar desafios, não matar.
- **Um cliente de cada vez.** Mesmo token = mesmos jogos.
- **Testes pesados na máquina do bot custam jogos.** Bot numa, testes na outra.
- **Ponder desligado.** Um segundo processo do motor rouba CPU ao principal e já
  fez o bridge perder o controlo de um jogo a meio.
- **O motor gere o seu tempo.** O cliente manda `wtime`/`btime` e não impõe
  `movetime`. O que se vê em `info string tm` é a decisão dele.
