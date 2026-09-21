# Reconstruir a busca a partir do binário, não do half2k

## Porque isto é preciso

O KestrelStrike que existe hoje **não é o binário perdido reconstruído**. É uma
transcrição do `half2k` — a busca foi escrita a ler o código Rust — à qual se
juntaram os **parâmetros** extraídos do executável.

Isto está admitido no nosso próprio `FIDELIDADE.md`:

> *"Transcrevi a busca lendo o código e não os VALORES POR OMISSÃO. O half2k tem
> noventa e oito interruptores, e muitos dos blocos que estão lá escritos estão
> desligados. Portei vários desses — ou seja portei caminhos que o motor a sério
> nunca percorre — e deixei de fora os que ele percorre mesmo."*

E mede-se. Mesma rede, mesmo hash, profundidade fixa 12, contra o
`ks_1.20260919`:

| posição | nosso | binário | rácio | lance |
|---|---:|---:|---:|---|
| Italiana | 81.522 | 97.547 | 0,84× | igual |
| Kiwipete | 74.904 | 128.056 | 0,58× | igual |
| `2rq1rk1/…` | 826.282 | 998.246 | 0,83× | igual |
| `r2q1rk1/…` | 316.484 | 384.516 | 0,82× | **e1e3 vs a2a4** |
| inicial | 43.387 | 62.436 | 0,69× | igual |
| final | 31.670 | 44.436 | 0,71× | igual |

**Cinco lances em seis, árvore sistematicamente 0,58× a 0,84×.** Em força está
à paridade — **−0,00 ± 9,55 em 1402 partidas** — mas é outro motor que joga tão
bem, não o mesmo motor.

Dito de outra maneira: temos paridade de **força** e não identidade de
**comportamento**. Para publicar, serve. Para responder à pergunta *"o que é
que ele fazia que nós não fazemos"*, não serve — e essa pergunta é a que vale,
porque estamos a 85 Elo da referência e não sabemos onde.

## O que o binário dá, e é muito mais do que eu supunha

`/root/obra_ks/kestrelstrike/binarios/ks_1.20260919`

**Não está despojado.** 8.565 símbolos com os nomes C++ completos. Sem DWARF,
mas com símbolos não é preciso adivinhar onde começa nada.

| função | endereço | bytes |
|---|---|---:|
| `Busca::arranca` | `0x43e630` | 32.541 |
| `Busca::negamax` | `0x436690` | 19.534 |
| `Busca::negamax` *(clone .constprop.0)* | `0x43b2e0` | 13.121 |
| `Busca::quiescencia` | `0x420830` | 4.110 |
| `Busca::limpa` | `0x41f800` | 2.564 |
| `Busca::credita` | `0x412280` | 1.832 |
| `Busca::pontua` | `0x410f20` | 1.351 |
| `Busca::indices` | `0x40d690` | 1.328 |
| `Busca::hist_de` | `0x411dc0` | 1.207 |
| `Busca::garante_ameacas` | `0x4119f0` | 970 |
| `Busca::ordena` | `0x40aa70` | 878 |
| `Busca::aprende` | `0x40dd20` | 805 |
| `Busca::repete_ja` | `0x41f3a0` | 685 |
| **`Busca::reducao`** | **`0x40ade0`** | **469** |
| `Busca::corrigida` | `0x40dbc0` | 344 |
| `Busca::sem_tempo` | `0x420210` | 222 |
| `Busca::credita_captura` | `0x40a9b0` | 180 |
| `Avaliador::avalia_cheia` | `0x41bcb0` | 396 |

Duas coisas a notar já:

- **`reducao` não foi inlined.** São 469 bytes legíveis, e é a função que decide
  o factor de crescimento por ply — exactamente onde a nossa árvore se separa
  da da referência.
- Existe `Busca::nos_das_ajudantes` (`0x7594ca8`, 8 bytes). É prova de que o
  motor perdido tinha SMP com contagem de nós dos ajudantes.

## O método, e o precedente que mostra que funciona

Já se fez uma vez, com resultado: o empacotamento da tabela de transposição foi
recuperado desmontando `sonda` em `0x420790`. Saiu tudo — `Ranhura` = 24 bytes
(`chave_xor` = chave ^ dados, `dados`, `ger`), `Balde` = 72 bytes logo VIAS = 3,
e o esquema de bits: 0-15 lance, 16-31 nota (i16), 32-39 profundidade (i8),
40-41 limite, 42 pv, 43-58 aval (i16).

O mesmo método aplica-se às funções da busca. Em concreto:

```
objdump -d --start-address=0x40ade0 --stop-address=0x40afb5 ks_1.20260919
```

**Os três padrões que dão quase tudo:**

1. **Constantes imediatas.** `cmp $0x6e,%eax` é um limiar de 110. Cruzar com o
   molde das omissões (ver abaixo) diz qual parâmetro é.
2. **Ordem das condições.** A cadeia de `cmp`/`jle`/`jg` dá a ordem em que os
   testes acontecem — e a ordem é o que mantém os ramos vivos. Uma cadeia
   `else-if` mal ordenada mata o terceiro ramo sem ninguém dar por isso.
3. **`getenv` → `strtol` → `mov %eax,OFFSET(%reg)`.** Dá o deslocamento de cada
   campo dentro de `Parametros`, e portanto liga cada constante ao seu nome.

**O molde das omissões está partido em dois** em `.rodata`: o bloco em
`0x6155f0` cobre a estrutura até ao deslocamento `0x22c`, e a **cauda** vive em
`0x609c80`. Ler a cauda do bloco errado dá lixo — já aconteceu, e produziu
`tm_bolo_n=4` e `tm_tecto_x10=6` quando os verdadeiros são 20 e 55.

## A ordem de trabalho

Por valor esperado, não por tamanho:

**1. `reducao` (`0x40ade0`, 469 bytes).** A mais pequena das importantes e a que
decide a forma da árvore. Comparar termo a termo com a nossa: quantos termos,
por que ordem, com que limiares, e quais os que estão lá e nós não temos. A
nossa radiografia diz que a tabela cega vale 53,7% e o mérito do lance 3,0% —
saber se a dele tem a mesma proporção responde à pergunta central.

**2. `pontua` (`0x410f20`) e `hist_de` (`0x411dc0`).** A ordenação. O corte ao
primeiro lance é 76,71% nosso contra 85,74% da referência, e o `PLANO.md`
regista *"killers fora da banda: 76,6% → 79,7%"* — três pontos que nunca
chegaram ao motor. Estas duas funções dizem como ele os obteve.

**3. `quiescencia` (`0x420830`, 4.110 bytes).** Um terço da nossa árvore.

**4. `negamax` (`0x436690`, 19.534 bytes).** O grande. Fazer por secções, na
ordem dos `Step` — não de uma vez.

**5. `arranca` (`0x43e630`, 32.541 bytes).** Gestão de tempo e o elástico. Menos
urgente: o relógio já está medido e corrigido.

## Como saber que resultou

**O critério não é "compila" nem "é mais forte".** É **árvore idêntica ao nó**.

```
mesma rede (f2e189), mesmo Hash, go depth 12, seis posições
```

Hoje estamos em 0,58×–0,84× com um lance diferente. Cada função reconstruída
tem de aproximar esse rácio de 1,00×, e o teste é barato — segundos.

**E o teste que apanha o erro mais comum:** contagens de nós **idênticas ao
último dígito** com valores diferentes de uma manete não significam "não faz
diferença" — significam que a manete **não está a ser lida**. Apanhou três
coisas num só dia: um histórico de plies rasos instalado numa função que o motor
não chama, um `KS_AMEACA_F` que se escreve `KS_AMEACA`, e um `rfp_adv_f` sem
interruptor.

## Armadilhas, todas pagas já

- **O molde partido em dois.** `0x6155f0` até `0x22c`, cauda em `0x609c80`.
- **Valores de desligar que ligam.** `pc_prof = 0` com a condição `prof >= pc_prof`
  liga **sempre**. A árvore foi a 878.281 nós em vez de 425.973.
- **Um defeito antigo é absorvido pela afinação feita por cima dele.** Corrigir
  uma coisa sozinha pode piorar o motor — aconteceu com a margem do RFP, onde a
  versão "corrigida" com medida e argumento perdeu 20 Elo em 2.602 partidas.
  Reconstruir uma função inteira evita isto; reconstruir metade dela não.
- **Nenhum indicador prevê Elo.** Nem nós, nem tempo até à profundidade, nem
  forma da árvore, nem corte ao primeiro lance. Servem para escolher onde
  procurar. Só partidas decidem.

## O que não fazer

Não importar peças da referência uma a uma. Está medido: **seis tentativas, seis
fracassos**. E não transplantar as constantes deles em bloco: **−45 Elo**, e o
`CONSTANTES.md` já o dizia — *"a forma transfere-se, o número não"*.

O que pagou, as duas vezes, foi **restaurar coisas do nosso próprio motor que o
porte deixou cair**: +18 Elo cada. Este documento é sobre encontrar o resto
delas, no único sítio onde ainda existem.
