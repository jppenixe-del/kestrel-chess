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

---

# Primeira passagem já feita: `Busca::reducao`

Para o documento não ser só método, o primeiro item da lista foi feito. Levou
minutos, não horas — porque o binário tem símbolos.

```
objdump -d --start-address=0x40ade0 --stop-address=0x40afb5 ks_1.20260919
```

138 linhas de assembly. E o `parametros_19set.h` tem os **deslocamentos
anotados**, portanto cada `mov 0xNNN(%rdi)` fica com nome sem adivinhar nada.

## Os campos que a `reducao` do binário lê, por ordem de leitura

| ordem | deslocamento | parâmetro | valor no binário |
|---|---|---|---:|
| 1 | `0x90` | `cut_cnt_base` | 0 |
| 2 | `0x138` | `lmr_piora_f` | 197 |
| 3 | `0x12c` | **`lmr_delta`** | **0** |
| 4 | `0x14c` | `lmr_ttpv` | 1024 |
| 5 | `0x150` | `lmr_ttpv_pv` | 0 |
| 6 | `0x154` | `lmr_ttpv_alpha` | 0 |
| 7 | `0x158` | `lmr_ttpv_fundo` | 0 |
| 8 | `0x13c` | `lmr_nonpv_f` | 1024 |
| 9 | `0x144` | `lmr_hist_div` | 22000 |
| 10 | `0x164` | `lmr_ext_amort` | 100 |
| 11 | `0x15c` | `lmr_cut_sem_tt` | 0 |
| 12 | `0x140` | `lmr_cut_f` | 2048 |

## O esqueleto, lido das instruções

```
mov  $0x3f,%ecx                 clamp do índice a 63
mov  0x10(%rdi,%rcx,4),%ecx     a tabela lmr, que vive em 0x10
add  0x90(%rdi),%eax            + cut_cnt_base
mov  0x138(%rdi),%r9d           lmr_piora_f
imul %ecx,%r9d / sar $0x9       r += r * piora / 512
mov  0x12c(%rdi),%esi           lmr_delta
test %esi,%esi / jle            se lmr_delta > 0
sub  %ebx,%eax / imul %esi
idiv %r8d / sub %eax,%ecx       r -= (X - Y) * lmr_delta / X
test %r12b,%r12b / jne
shr $0x1f / add / sar $1 / sub  r -= r/2   (a metade das capturas)
```

## O achado

**O `lmr_delta` é o termo `delta/rootDelta`.** É o mesmo que aparece na lista de
peças em falta como *"sim | não usamos"*, a pensar que era mecanismo da
referência que nunca tivemos.

**Tivemos.** Está no nosso próprio binário, com interruptor `KS_LMR_DELTA`, e a
nota no `parametros_19set.h` diz **"NOVO depois de 15-09"** — foi acrescentado
nos últimos quatro dias antes de a máquina morrer. Está a **zero** nas omissões,
portanto inerte também lá, mas o caminho de código existe e a forma lê-se acima.

A forma é `r -= (X - Y) * lmr_delta / X`, com `X` e `Y` a virem de registos
carregados antes da chamada. Identificar `X` e `Y` é o passo seguinte, e faz-se
olhando para quem chama a `reducao` no `negamax`.

## O que a comparação com a nossa diz

A nossa `reducao` lê doze campos: os mesmos onze **menos `lmr_delta`**, mais
`cut_cnt_mais`. Ou seja, a estrutura está quase toda lá — o que falta é um termo
que estava desligado no original de qualquer maneira.

**Isso é boa notícia e má notícia.** Boa, porque a `reducao` não é onde está o
buraco. Má, porque significa que o buraco está numa das funções grandes — o
`negamax` de 19.534 bytes ou a `quiescencia` de 4.110 — e essas são trabalho a
sério.

---

# As vinte manetes dos últimos quatro dias

O `parametros_19set.h` marca **vinte** parâmetros com *"NOVO depois de 15-09"*.
É o trabalho final do motor antes de a máquina morrer, e portanto o sítio onde a
reconstrução tinha mais probabilidade de falhar.

Verificado contra o motor de hoje:

| manete | valor | no nosso motor |
|---|---:|---|
| `ad_min`, `ad_max` | 3, 12 | presentes, lidos |
| `ordem_cont` → `ordem_cont_f` | 32 | presente, lido |
| `ameaca` → `ameaca_f` | 20 | presente, lido |
| `sem_balde` | 0 | presente |
| `cont_n` | 3 | presente, lido |
| `peao_ch` → `peao_chaves` | 8192 | presente, lido |
| `otimismo` → `otimismo_f` | 114 | presente, lido |
| `tt_sup` → `tt_sup_nota` | 1000000 | presente, lido |
| `tm_escala_min` | 650 | presente, lido |
| `tm_curva`, `tm_curva_min`, `tm_curva_f` | 1, 8, 100 | presentes, lidos |
| **`lmp_melhora`** | **0** | não existe — inerte no binário |
| **`poda_red`** | **0** | não existe — inerte no binário |
| **`lmr_delta`** | **0** | não existe — inerte no binário, forma acima |
| **`asp_sf`** | **0** | não existe — inerte no binário |
| **`tm_adv_f`** | **0** | não existe — inerte no binário |
| **`tm_adv_min`**, **`tm_adv_max`** | 70, 140 | não existem — **excluídos de propósito** |

**Catorze das vinte estão presentes e são lidas.** Das seis que faltam, cinco
estão a zero no próprio binário e as duas últimas são o relógio do adversário,
que foi medido a **−14,29 ± 12,22 em 905 partidas** e excluído por decisão.

Portanto a recuperação dos últimos quatro dias está essencialmente completa ao
nível dos parâmetros. **O que falta é comportamento, não valores** — e é por
isso que este documento existe.


---

# Segunda passagem: `Busca::hist_de` — a ordenação, item 2

`Kestrel::Busca::hist_de(Position const&, Move, int) const`
`0x411dc0`, 1.207 bytes, 193 instruções. Lido do binário, sem abrir código nenhum.

## Primeiro, a base ficou provada

Os deslocamentos das manetes foram derivados em `arranca`, onde a base é a
posição de pilha `-0x360(%rbp)`. Na `hist_de` a base é o `this`. Só transferem se
forem o mesmo objecto — e são:

```
43e64c:  mov %rdi,%rbx
43e656:  mov %rdi,-0x360(%rbp)      <- a base das 122 manetes E' o this
```

Portanto `Parametros` está embebido em `Busca`, e `0x174` na `arranca` é o mesmo
campo que `0x174` aqui. Confirmado por três correspondências semânticas
independentes nesta função (`cont_n` a guardar a profundidade da continuação,
`sem_balde` a guardar o cálculo do balde, `peao_ch` a mascarar uma chave de peão).

## O mapa das manetes, derivado do executável

122 cadeias `KS_*`, 122 resolvidas pelo padrão
`lea <str>,%rdi ; getenv ; strtol ; mov %eax,DESL(%reg)`.

**Cuidado que custou uma falsidade evitada:** o argumento do `getenv` nem sempre
vem de um `lea` imediatamente antes — muitas vezes chega por cópia de registo
(`mov %rbx,%rdi` em `43f029`). Recuar cegamente até ao `lea` mais próximo inventa
nomes. É preciso seguir os registos.

### Duas colisões verdadeiras

Dois deslocamentos são escritos por **duas manetes diferentes**, na mesma função,
com a base recarregada do mesmo `-0x360(%rbp)`:

| deslocamento | manete | endereço da escrita | quem ganha |
|---|---|---|---|
| `0x1c8` | `KS_PCP_M` | `43eabe` | — |
| `0x1c8` | `KS_PCP_MARGEM` | `43f022` | **esta**, corre depois |
| `0x160` | `KS_EXT` | `43eba3` | — |
| `0x160` | `KS_LMR_EXT_MAX` | `43f213` | **esta**, corre depois |

Quem afinasse `KS_PCP_M` e `KS_PCP_MARGEM` ao mesmo tempo estava a medir lixo: a
segunda apaga a primeira sem aviso. Mesma família do `KS_AMEACA_F` já registado.

## A função, reconstruída

```cpp
int Busca::hist_de(const Position& pos, Move m, int ply) const
{
    if (m & 0xc000) { /* 412100: lance especial, outro caminho */ }

    const int para = m & 0x3f;           // 411de8  destino: bits 0-5
    const int de   = (m >> 6) & 0x3f;    // 411e02  origem:  bits 6-11

    if (pos.tabuleiro[para] != 0) { /* 4120f3: não é quieto */ }   // 411def

    garante_ameacas(pos, ply);                                     // 411e06

    int balde = 0;                                                 // 411e26
    if (!sem_balde) {                                   // 0x17c    // 411e2c
        const uint64_t am = ameacas[ply];               // this+0x25e98 + ply*8
        balde = ((am >> de) & 1) * 2 + ((am >> para) & 1);          // 411e41
    }

    const int lado = pos.byte[0x26c];                              // 411e14
    const int peca = (pos.tabuleiro[de] & 7) - 1;                  // 411e66

    int n = hist[(((lado*64 + de)*64 + para)*4) + balde];           // 411e8e
                                        // base this+0x43b0, 4 baldes por par

    int c = 0;
    if (cont_n > 0) {                                   // 0x180    // 411e7c
        c  = 2 * cont(ply-1, 0x000);   // peso DOBRADO             // 411ee1
        if (cont_n >= 2) c += cont(ply-2, 0x180);                  // 411f39
        if (cont_n >= 3) c += cont(ply-4, 0x300);                  // 411f98
        if (cont_n >= 4) c += cont(ply-3, 0x480);                  // 411ffd
        if (cont_n >= 5) c += cont(ply-6, 0x600);                  // 412054
    }

    return n + (c * ordem_cont) / 32;   // 0x174        // 412058..41206c
}
```

onde `cont(d, slot)` é

```cpp
    idx = (peca + 6*(slot/0x180*384 + peca_ant*64 + para_ant)) * 64 + para;
```

com `peca_ant` em `this+0x28fc0 + d*4`, `para_ant` em `this+0x29398 + d*4`, e a
tabela apontada por `this+0x28f78`. Quando `d` cai antes da raiz, há um caminho
alternativo que lê o histórico **real da partida** em `this+0x350` (peça) e
`this+0x368` (destino), com a contagem em `this+0x380` — seis entradas cada.
Existe para todas as cinco distâncias (`412118`, `412191`, `4121ff`, `4121c7`,
`41224f`).

## O achado, e é o que se andava a procurar

**As distâncias da história de continuação estão guardadas pela ordem `1, 2, 4,
3, 6`** — não `1, 2, 3, 4`. Lê-se nos deslocamentos das fatias, `0x180` a cada
uma, na ordem em que o código as soma:

| ordem na memória | distância | deslocamento | condição |
|---:|---:|---|---|
| 1.ª | `ply-1` | `0x000` | `cont_n > 0`, **peso ×2** |
| 2.ª | `ply-2` | `0x180` | `cont_n >= 2` |
| 3.ª | **`ply-4`** | `0x300` | `cont_n >= 3` |
| 4.ª | **`ply-3`** | `0x480` | `cont_n >= 4` |
| 5.ª | `ply-6` | `0x600` | `cont_n >= 5` |

Com o valor que o binário traz, **`cont_n = 3`**, o motor perdido soma as
distâncias **`{1, 2, 4}`** — e **salta a 3**.

### As omissões, lidas do molde (não da documentação)

O inicializador estático em `0x409105`..`0x40926d` copia o molde para
`g_busca+0x10`..`+0x240` com AVX. As fontes dizem onde o molde vive:

| bloco `.rodata` | vai para | base nocional |
|---|---|---|
| `0x615600`–`0x615820` | `+0x10` … `+0x230` | `0x6155f0` |
| `0x609eb0` (xmm, 16 b) | `+0x230` … `+0x240` | `0x609c80` |

Derivadas do zero, as duas bases batem com as que o documento já registava. E
`tm_tecto` em `0x23c` lê **55** — o valor verdadeiro, não o `6` que sai de ler a
cauda no bloco errado. A divisão está certa.

| manete | desl. | omissão |
|---|---|---:|
| `cont_n` | `0x180` | **3** |
| `ordem_cont` | `0x174` | 32 |
| `sem_balde` | `0x17c` | 0 — baldes **ligados** |
| `peao_f` | `0x184` | 32 |
| `peao_ch` | `0x188` | 8192 |

Uma reconstrução que assuma `{1, 2, 3}` produz outra ordenação de lances com o
mesmo `cont_n`. É candidato directo aos três pontos de corte ao primeiro lance
que faltam (76,71% contra 85,74%) e à forma da árvore.

## Ainda por ler nesta função

O caminho dos lances não-quietos (`412100`) e o da história de peões
(`412084`..`4120cd`, manetes `peao_f` em `0x184` e `peao_ch` em `0x188`, com a
chave em `pos+0x260`). E a manete `low_f` em `0x1c0`, que em `412070` só actua
quando `ply <= 4` — um histórico de plies rasos que **está** nesta função.


---

# Item 2, segunda metade: `Busca::pontua`

`Kestrel::Busca::pontua(Position const&, Busca::Lista&, int, Move, Limite, int, int) const [clone .constprop.0]`
`0x410f20`, 1.351 bytes, 293 instruções. Lido do binário.

## A forma da `Lista`

| deslocamento | conteúdo |
|---|---|
| `+0x000` | vector de lances, `uint16` cada |
| `+0x200` | **nota** de cada lance, `int32` |
| `+0x600` | segunda nota, `int32` — posta a zero no topo do ciclo |
| `+0xa00` | contagem de lances |

O ciclo sai já se a contagem for `<= 0` (`410f20`).

## A codificação do lance, confirmada aqui e na `hist_de`

```
bits 0-5    casa de destino
bits 6-11   casa de origem
bits 12-13  peça da promoção
bits 14-15  tipo:  0x4000 = en passant / roque,  0x8000 = promoção
```

## A escada de pontuação

Por ordem decrescente, com os valores lidos do binário:

| caso | nota | onde |
|---|---:|---|
| lance da TT | `1000000` | `411150` / `4113a8` |
| en passant / roque | `500000` | `411410` / `411438` |
| captura | `600000 + 16·valor(capturada) + hist` | `411389` |
| promoção | `600000 + 1600 − valor(peça) + hist` | `411290` |
| killer 1 | `killers + 1` | `411190` / `411449` |
| killer 2 | `killers` | `411194` / `41145a` |
| quieto | `−533280` | `410ff4` / `41122e` |
| captura com SEE a falhar | menos `1200000` | `411383` |

Os valores de peça saem de `CSWTCH.758` em `0x5ec4e0`:
**`[100, 320, 330, 500, 900, 20000]`**.

## O termo de histórico das capturas

```
idx  = capturada + 6*((peça − 1)*64 + destino)      // 411302..411314
h    = caphist[idx] * 16                            // 41131a, shl $0x4
q    = h / max(this+0x1ec, 1)                       // 411350
lim  = −((q + base) / max(this+0x1e8, 1)) * 208 / 100   // 41135b..411377
nota = 600000 + base + q  − (see_ge(lance, lim) ? 0 : 1200000)
```

O vector do `caphist` vive em `this+0x28358` (início) e `this+0x28360` (fim); se
estiver vazio o termo é zero (`411420`). Os divisores `this+0x1e8` e `this+0x1ec`
**não são manetes** — não há escrita vinda de `getenv` para eles; são estado de
execução, com chão em 1 imposto por `vpmaxsd` contra a constante `[1,0,0,0]` em
`0x609c90`. O limiar do SEE é portanto **dinâmico e função do histórico**.

## Achado 1: os killers nascem desligados

```
411200:  mov  0x70(%r12),%r10d      <- KS_KILLERS
411205:  test %r10d,%r10d
411208:  jle  41122e                <- salta a verificação INTEIRA
```

A omissão em `0x70`, lida do molde, é **`0`**. O caminho dos killers está todo lá
— as duas casas em `this+0x28b88` e `this+0x28b8a`, a comparação, a nota — mas
com `killers = 0` nunca é percorrido, e os killers caem na nota dos quietos.

E quando acerta, a nota **é o próprio valor da manete**: `killers+1` para o
primeiro, `killers` para o segundo. Nota crua, fora de banda, sem histórico.

Isto fecha a questão que o `PLANO.md` deixou aberta — *"killers fora da banda:
76,6% → 79,7%, três pontos que nunca chegaram ao motor"*. Não chegaram porque no
motor perdido **também não estavam ligados**. Não é peça em falta na
reconstrução: é uma manete que existe nos dois lados e está a zero nos dois.

## Achado 2: `tt_sup` só actua em metade do motor

A função tem dois ramos, escolhidos à entrada por `cmp $0x2,%r8b` (`410f48`):

| ramo | condição | nota do lance da TT |
|---|---|---|
| A | `Limite != 2` | `movl $0xf4240` — **1.000.000 constante** (`411150`) |
| B | `Limite == 2` | `mov 0x190(%r12)` — **manete `tt_sup`** (`4113a8`) |

A omissão de `tt_sup` é `1000000`, igual à constante. Por omissão os dois ramos
concordam, e é por isso que nunca se deu por isto. Mas mexer em `KS_TT_SUP` move
só o ramo B.

É o sintoma que este documento já descrevia: uma manete que se altera e a árvore
não mexe onde devia. Quem tenha medido `KS_TT_SUP` mediu metade do efeito.

### E sabe-se qual metade perde a manete

Há quatro chamadas à `pontua` no binário:

| quem chama | endereço | `Limite` passado | ramo |
|---|---|---|---|
| `quiescencia` | `420dc7` | `mov $0x3,%r8d` | **A** — constante |
| `quiescencia` | `421038` | `mov $0x3,%r8d` | **A** — constante |
| `negamax` | `437b68` | `movzbl 0x10c(%rsp),%r8d` | variável |
| `negamax` *(constprop.0)* | `43b717` | `movzbl 0xbd(%rsp),%r8d` | variável |

**A quiescência passa `3` sempre.** Portanto `KS_TT_SUP` não tem efeito nenhum na
quiescência — um terço da árvore, pelas contas deste documento. Só actua no
`negamax`, e lá só quando a variável da pilha vale `2`.

Quem afinou `KS_TT_SUP` mediu-a em dois terços da árvore a pensar que a media
toda.

## Por ler

O que a variável da pilha do `negamax` contém em `0x10c(%rsp)` / `0xbd(%rsp)` —
provavelmente o limite guardado na TT para o nó, mas isso lê-se no `negamax`, que
é o item 4 da lista.


---

# `hist_de`: os ramos que faltavam, incluindo os inertes

Correcção de método. Na primeira passagem marquei três ramos como "por ler" e
avancei, tratando-os como menos urgentes por estarem desligados por omissão.
**Está errado.** Código desactivado é código perdido na mesma, e é o mais fácil
de deixar cair: com a manete a zero, o motor reconstruído dá exactamente os
mesmos nós com ou sem ele. Nenhum teste de árvore o apanha. Só a leitura o
apanha.

Ficam aqui os três, e a função fica fechada — 290 instruções, `0x411dc0` a
`0x412277`.

## Os tipos de lance, à entrada

Em `411dd5` testa-se `m & 0xc000`; se for diferente de zero, salta para `412100`:

```
412100:  xor  %r10d,%r10d      ; resultado = 0
412103:  sub  $0x4000,%ax
412107:  jns  4120f3           ; devolve 0
412109:  mov  %edx,%r14d
41210c:  and  $0x3f,%r14d
412110:  jmp  411df9           ; segue o caminho dos quietos
```

O `sub` seguido de `jns` testa o sinal do resultado em 16 bits:

| `m & 0xc000` | `− 0x4000` | sinal | destino |
|---|---|---|---|
| `0x4000` | `0x0000` | + | **devolve 0** |
| `0x8000` | `0x4000` | + | **devolve 0** |
| `0xc000` | `0x8000` | − | **cai no histórico dos quietos** |

Dois dos três tipos especiais não têm histórico nenhum; o terceiro tem o dos
quietos por inteiro. *(Pela forma como a `pontua` trata `0x8000` — bits 12-13 a
escolher peça, tabela de valores — `0x8000` é promoção e `0xc000` é o tipo que se
comporta como lance quieto. Inferência, não leitura.)*

## Ramo inerte 1: o histórico de plies rasos (`low_f`)

```
412070:  mov  0x1c0(%rbx),%eax   ; low_f
412076:  cmp  $0x4,%r12d         ; ply
41207a:  jg   412084             ; ply > 4  -> salta
41207c:  test %eax,%eax
41207e:  jg   412140             ; low_f > 0 -> entra
```

```
412140:  mov  0x2e8(%rbx),%r11       ; início do vector
412147:  cmp  %r11,0x2f0(%rbx)       ; fim
41214e:  je   412084                 ; vazio -> salta
412154:  ...                         ; idx = (ply*64 + de)*64 + para
41216c:  imul (%r11,%rbp,4),%eax     ; low_f * tabela[idx]
412172:  idiv %r12d                  ; / (ply + 1)
412175:  add  %eax,%r10d
```

```cpp
if (ply <= 4 && low_f > 0 && !raso.vazio())
    n += low_f * raso[(ply*64 + de)*64 + para] / (ply + 1);
```

**`low_f` vale `0` no molde.** O ramo existe inteiro no binário e nunca corre.
É o "histórico de plies rasos" que este documento dava como *instalado numa
função que o motor não chama* — está na `hist_de`, que o motor chama sempre;
o que o desliga é a manete, não o sítio.

## Ramo inerte 2: os baldes (`sem_balde`)

```
411e2c:  test %r8d,%r8d          ; sem_balde
411e2f:  jne  411e56             ; != 0 -> salta o cálculo, balde fica 0
```

`sem_balde` vale `0`, portanto os baldes estão **ligados** e o ramo que os
dispensa é que nunca corre. Registado por simetria: quem reconstruir tem de pôr
lá o caminho do `balde = 0` fixo, mesmo sabendo que não é percorrido.

## Ramo activo que faltava: o histórico de peões

```
412084:  mov  0x184(%rbx),%r12d   ; peao_f
41208b:  test %r12d,%r12d
41208e:  jle  4120f3              ; <= 0 -> devolve
412090:  mov  0x28f98(%rbx),%r9   ; vector
41209a / 4120a3:  vazio -> devolve
4120a5:  movslq 0x188(%rbx),%rbx  ; peao_ch
4120b7:  mov  0x260(%r13),%r13    ; pos+0x260 -> estrutura
4120c1:  sub  $0x1,%rbx
4120c5:  and  0x8(%r13),%rbx      ; h = chave_de_peoes & (peao_ch - 1)
4120db:  imul (%rcx,%r11,4),%r12d ; peao_f * tabela[idx]
4120ec:  sar  $0x5                ; / 32
```

```cpp
h   = pos.chave_peoes & (peao_ch - 1);
idx = ((h*12 + lado*6 + peca) * 64) + para;
n  += peao_f * peoes[idx] / 32;
```

Dimensões `[peao_ch][2][6][64]`. Com `peao_f = 32` e `peao_ch = 8192`, **este
está activo.**

## O caminho antes da raiz, nas cinco distâncias

Quando `ply − d` cai abaixo de zero, a distância não é abandonada: passa a ler o
histórico **real da partida**, em dois vectores de seis entradas cada,

```
this+0x350   peça do lance     (6 int32)
this+0x368   destino do lance  (6 int32)
this+0x380   quantos há
```

com o índice `~(ply − d)` e o guarda `cmp 0x380(%rbx)`. Existe para as cinco
distâncias, e os pontos de entrada são:

| distância | entra em | volta a |
|---:|---|---|
| `ply-1` | `412118` | `411ec1` |
| `ply-2` | `412191` | `411f11` |
| `ply-4` | `4121ff` | `411f9c` |
| `ply-3` | `4121c7` | `412001` |
| `ply-6` | `41224f` | `412058` |

Cada um confirma outra vez a ordem `1, 2, 4, 3, 6` — os `cmp $0x1/$0x2/$0x3/$0x4`
contra `cont_n` aparecem pela mesma sequência (`412187`, `4121f5`, `4121b8`,
`412245`).

## As manetes inertes desta função, para o inventário

| manete | desl. | omissão | o que fica por percorrer |
|---|---|---:|---|
| `low_f` | `0x1c0` | **0** | histórico de plies rasos, `ply <= 4` |
| `sem_balde` | `0x17c` | **0** | caminho sem baldes |
| `killers` | `0x70` | **0** | (na `pontua`) as duas casas de killer |


---

# Inventário do código inerte: as 34 manetes que nascem a zero

Motivo desta secção: recuperar **cada linha**, esteja ou não activa. O código
desligado é o mais fácil de deixar cair, porque nenhum teste o apanha — com a
manete a zero, o motor reconstruído dá exactamente os mesmos nós com ou sem ele.
A árvore idêntica ao nó **não prova que o código está todo lá**. Só a leitura
prova.

Das 122 manetes, **34 valem `0` no molde**. Todas são lidas — nenhuma é letra
morta. O que muda é *como* o zero as desliga, e há três maneiras.

## Os três modos, e o terceiro é o perigoso

**1. Salto — o bloco inteiro não é percorrido.**
`test %eax,%eax ; jle` sobre zero salta por cima. O bloco existe no binário e a
reconstrução tem de o trazer inteiro.

| manete | onde | guarda |
|---|---|---|
| `killers` | `pontua`, `negamax` | `test+jle` |
| `low_f` | `hist_de`, `credita` | `test+jg` / `test+jle` |
| `pc` | `negamax` | `test+jle` |
| `pcp_m` | `negamax` | `test+jle` |
| `alpha_desc` | `negamax` | `test+jle` |
| `lmr_delta` | `reducao` | `test+jle` |
| `contempt`, `cont_1lado`, `cont_seminc` | `negamax` | `test+je` / `jne` |
| `cuckoo_seminc`, `iir_sem_all`, `ordem_xeque`, `poda_red` | `negamax` | `test+je` |
| `hp_lin`, `sing_lower` | `negamax` | `test+jne` |
| `corr_marg` | `negamax` | `test+jg` |
| `sem_balde` | `hist_de`, `credita` | `test+jne` |
| `qs_recapt` | `quiescencia` | `test+je` |
| `asp_sf`, `recusa_fresca`, `tm_adv_f` | `arranca` | `test+je` |

**2. `cmov` — sem salto nenhum.**
O valor é escolhido por movimento condicional. Não há ramo para notar ao ler a
desmontagem, e é por isso que escapa: procura-se um `j` e não há.

| manete | onde | instrução |
|---|---|---|
| `cutcnt_mais` | `reducao` | `cmovg` |
| `cut_sem_tt` | `reducao` | `cmove` |
| `ext` / `lmr_ext_max` | `negamax` | `cmp` + `cmovge` |
| `rfp_mult` | `negamax` | `cmp` + `cmovg` / `cmovle` |

**3. Sem condição — o termo é sempre calculado e vale zero por multiplicação.**
Este é o pior. Não há salto, não há `cmov`, não há nada a assinalar que o termo
existe: ele é somado sempre, e como o factor é zero, desaparece na aritmética.
Quem reconstrói a olhar para o que a árvore faz **nunca o vê**, e quem reconstrói
a ler o assembly só o vê se reparar num `imul` por um campo que calha ser zero.

| manete | onde |
|---|---|
| `ttpv_pv`, `ttpv_alpha`, `ttpv_fundo` | `reducao` |
| `corr6` | `corrigida` |
| `lmr_pecas_fim` | `negamax` |
| `pior_adv` | `negamax` |
| `cutcnt` | `reducao` (`add 0x90(%rdi),%eax`) |

## Onde está o grosso

Das 34, **mais de vinte são lidas no `negamax`**. O código inerte por recuperar
está concentrado no item 4 da lista, não espalhado. Isso é boa notícia para o
planeamento: uma leitura cuidada do `negamax` fecha a maior parte do inventário
de uma vez.

## Aviso que continua a valer

Zero não quer dizer desligado. O próprio documento já registava: `pc_prof = 0`
com a condição `prof >= pc_prof` **liga sempre**, e a árvore foi a 878.281 nós em
vez de 425.973. A tabela acima dá a comparação exacta de cada uma precisamente
por isso — é ela que decide, não o valor.

## Nota de método, porque quase escrevi o contrário

Numa passagem intermédia este inventário dizia que `rfp_tt`, `cont_1lado` e
`lmp_melhora` eram escritas pelo `getenv` e **nunca lidas** — manetes mortas.
Era falso, e vinha de dois erros meus somados:

1. No `negamax` o `%rbp` **não é apontador de quadro**: é um registo vulgar a
   guardar o `this`. Um rastreador que assuma `%rbp` = pilha fica cego às
   manetes todas desta função — que são a maioria.
2. Um regexp com `(?![,)])` que, ao tentar excluir as formas indexadas,
   excluía exactamente a forma de leitura `0x14(%rbp),%r10d`.

As três são lidas, em `437076`, `437906` e `43818c`. Fica registado porque o modo
de falhar é sedutor: a ferramenta corre, não dá erro, e devolve uma lista curta e
plausível de "descobertas" que são ausências de prova disfarçadas de prova de
ausência.


---

# Reconstrução de baixo para cima: as folhas

Ordem deliberada: as funções chamadas primeiro, quem chama depois. Quando se
chegar ao `negamax` já se sabe o que cada chamada faz, em vez de ter de adivinhar
a meio de 19.534 bytes.

Divisores por constante mágica, calculados e não adivinhados — `d = 2^(N+s)/M`:

| magia | desl. | divisor |
|---|---|---:|
| `0x431bde82d7b634db` | `sar 18` | 1.000.000 |
| `0x73002b20102c0611` | `sar 13` | 18.236 |
| `0x89ae4089ae4089af` | `sar 8` | 476 |
| `0x5c2eb8ae1a3dccd3` | `sar 15` | 91.000 |
| `0x5254e78f` | `sar 38` | 199 |
| `0x51eb851f` | `sar 37` | 100 |

---

## 1. `Busca::credita_captura(Position const&, Move, int)` — `0x40a9b0`, 180 b

```cpp
void Busca::credita_captura(const Position& pos, Move m, int bonus)
{
    if (caphist.vazio()) return;                       // 40a9b0..40a9c0

    const int para = m & 0x3f;                         // 40a9d3
    const int de   = (m >> 6) & 0x3f;                  // 40a9cf
    const int peca = pos.tabuleiro[de] & 7;            // 40a9d9

    int capt;
    if ((m & 0xc000) == 0x8000) capt = 0;              // 40a9e2 -> 40aa60
    else {
        const int c = pos.tabuleiro[para] & 7;         // 40a9ed
        if (c == 0) return;                            // 40a9f6
        capt = c - 1;
    }

    bonus = std::clamp(bonus, -16384, 16384);          // 40aa1d, 40aa2a

    const int idx = capt + 6*((peca - 1)*64 + para);   // 40aa17..40aa27
    int& h = caphist[idx];
    h += bonus - std::abs(bonus) * h / 16384;          // 40aa3d..40aa59
}
```

A subida com gravidade, com `D = 16384` e o mesmo valor a servir de tecto ao
bónus. O índice bate com o que a `pontua` lê em `411302`, o que confirma as duas
leituras uma contra a outra.

**Nota:** na promoção (`0x8000`) o índice da peça capturada é **`0`**, não a peça
realmente capturada. Está no binário assim.

---

## 2. `Busca::sem_tempo()` — `0x420210`, 222 b

```cpp
bool Busca::sem_tempo()
{
    if (parar) return true;                            // 0x390, pegajoso

    if (this->b_260 || (ptr_268 && *ptr_268))          // 420220..42023c
        { parar = true; return true; }

    if (limite_nos && nos >= limite_nos)               // 0x8, 0x280
        { parar = true; return true; }

    if (nos & 0x3ff) return parar;                     // 420253: só de 1024 em 1024

    this->n_288 = nos;                                 // 420278
    if ((uint16_t)(nos & 0x3ff)) return parar;         // 42027f, testw
    if (limite_ms <= 0) return parar;                  // 0x3a8

    auto agora = steady_clock::now();                  // 42029d
    if ((agora - inicio) / 1'000'000 >= limite_ms)     // 0x398, magia /1e6
        { parar = true; return true; }
    return parar;
}
```

O relógio só é consultado **de 1024 em 1024 nós** — e há um segundo teste
`testw` sobre os mesmos bits logo a seguir, redundante com o primeiro. Está no
binário; fica registado como está.

---

## 3. `Busca::corrigida(Position const&, int, int) const` — `0x40dbc0`, 344 b

```cpp
int Busca::corrigida(const Position& pos, int aval, int ply) const
{
    if (!corr || corr->vazio()) return aval;           // 0x28fb8

    int ix[6];
    indices(pos, ply, ix);                             // 40dc02
    const int lado = pos.byte[0x26c];

    int s = 0;
    if (ix[0] >= 0) s += 203 * corr[(lado +  0)*16384 + ix[0]];   // 40dc23
    if (ix[1] >= 0) s += 109 * corr[(lado +  2)*16384 + ix[1]];   // 40dc42
    if (ix[2] >= 0) s += 109 * corr[(lado +  4)*16384 + ix[2]];   // 40dc63
    if (ix[3] >= 0) s += 121 * corr[(lado +  6)*16384 + ix[3]];   // 40dc81
    if (ix[4] >= 0) s +=  72 * corr[(lado +  8)*16384 + ix[4]];   // 40dca7
    if (ix[5] >= 0) s += corr6 * corr[(lado + 10)*16384 + ix[5]]; // 40dcc7

    return std::clamp(aval + s/2048, -31753, 31753);   // 40dcd2..40dcf3
}
```

Pesos `[203, 109, 109, 121, 72, corr6]`, divisor 2048, tecto `±31753`
(`0x7c09` / `0xffff83f7`).

**O sexto termo é o modo 3 do inventário em acto.** `corr6` vale `0`, e o termo é
calculado e somado **em todas as chamadas** — sem salto, sem `cmov`, sem sinal
nenhum. Quem reconstruísse a olhar para o comportamento da árvore nunca o veria:
não faz diferença nenhuma. Existe, e a linha tem de lá estar.

---

## 4. `Avaliador::avalia_cheia(Position const&, int, int)` — `0x41bcb0`, 396 b

```cpp
int Avaliador::avalia_cheia(const Position& pos, int a2, int a3)
{
    const auto* st = pos.estado;                       // pos+0x260
    if (st->c_48 != 0)  return 0;                      // 41bcd1
    if (!rede_pronta)   return 0;                      // this+0x6ede708

    auto [psq, pos_] = rede.evaluate(pos, pilha, caches);   // 41bd2d

    int v = psq + pos_;                                // 41bd4a
    const int dif = std::abs(pos_ - psq);              // 41bd4f..41bd56

    v -= (v * dif) / 18236;                            // 41bd8f..41bda7

    const int m = 534 * (pos.b4 + pos.n94) + (st->m2c + st->m28);   // 41bd65..41bd7d
    const int e = a3 + (dif * a3) / 476;               // 41bdb8..41bdc9

    v += (m * v + 7675 * e) / 91000;                   // 41bdce..41bdf0

    v -= (st->m34 * v) / 199;                          // 41bdf2..41be0b
    v  = std::clamp(v, -31506, 31506);                 // 0x7b12 / 0xffff84ee

    return a2 * v / 100;                               // 41be1c..41be30
}
```

Os dois inteiros que a rede devolve vêm pela pilha (`(%rsp)` e `0x4(%rsp)`).
Tecto `±31506`, diferente do `±31753` da `corrigida` — dois limites parecidos e
**não iguais**, exactamente o género de detalhe que uma transcrição uniformiza
sem dar por isso.


---

# `Busca::reducao` relida do zero — três correcções

A primeira passagem deste documento fez a `reducao`. Reli-a de raiz, sem olhar
para o que lá estava escrito, e só depois comparei. O essencial confirma-se: a
função lê os campos que a primeira passagem identificou, e `lmr_delta` é mesmo o
termo `delta/rootDelta` e está mesmo a zero. Mas há **três erros de leitura** que
mudam o que sai do teclado.

A assinatura da tabela de símbolos dá os catorze argumentos e resolve o que a
primeira passagem não conseguia nomear:

```
Busca::reducao(int a1, int a2, int a3, int a4, bool a5, bool a6, bool a7,
               bool a8, bool a9, Move a10, int a11, bool a12, bool a13,
               int a14) const
```

`a1..a4` em `%esi %edx %ecx %r8d`, `a5` em `%r9b`, e de `a6` a `a14` na pilha,
em `0x30(%rsp)` a `0x70(%rsp)`.

## Correcção 1 — a tabela é bidimensional, não linear

O documento diz *"`mov 0x10(%rdi,%rcx,4),%ecx` — a tabela lmr, que vive em
`0x10`"*, e regista um só `clamp do índice a 63`. São **dois** índices:

```
40adf3:  cmp %ecx,%edx  /  cmovg    a2 = min(a2, 63)
40ae02:  cmp %ecx,%esi  /  cmovg    a1 = min(a1, 63)
40ae20:  shl $0x6,%rsi                 a1 * 64
40ae24:  lea 0xe8(%rdx,%rsi,1),%rcx    0xe8 + a2 + a1*64
40ae2c:  mov 0x10(%rdi,%rcx,4),%ecx
```

O endereço efectivo é `this + 0x10 + 0xe8*4 + (a1*64 + a2)*4`, ou seja uma
tabela **`[64][64]` que começa em `this+0x3b0`**, indexada por `a1` e `a2` —
profundidade e número de ordem do lance. `0x10` é o deslocamento da instrução,
não o da tabela.

Quem reconstrua uma tabela linear em `0x10` a partir do texto do documento
obtém outra árvore.

## Correcção 2 — o termo do `cutcnt` tem duas partes e uma condição

O documento regista `add 0x90(%rdi),%eax  + cut_cnt_base`, sem mais. O que lá
está é:

```
40ae30:  cmp $0x1,%eax           ; a14
40ae33:  jle 40ae4c              ; a14 <= 1 -> nada
40ae35:  cmp $0x2,%eax
40ae38:  mov $0x0,%eax
40ae3d:  cmovg 0x94(%rdi),%eax   ; a14 > 2 ? cutcnt_mais : 0
40ae44:  add 0x90(%rdi),%eax     ; + cutcnt
40ae4a:  add %eax,%ecx
```

```cpp
if (a14 > 1) r += cutcnt + (a14 > 2 ? cutcnt_mais : 0);
```

`cutcnt_mais` entra por `cmovg` — é o **modo 2** do inventário, sem salto. Não
aparece na lista de campos lidos da primeira passagem, e é por isso: procurou-se
um ramo e não há nenhum.

## Correcção 3 — o divisor do `lmr_delta` não é `X`

O documento deixa a fórmula como `r -= (X - Y) * lmr_delta / X`, com *"identificar
X e Y é o passo seguinte"*. `X` e `Y` são `a4` e `a3`. Mas o divisor **não é
`X`**:

```
40ae79:  mov  %r8d,%eax           ; a4
40ae84:  vmovd 0x47f20(%rdi),%xmm0   ; <- o divisor vem daqui
40ae8c:  sub  %ebx,%eax           ; a4 - a3
40ae8e:  imul %esi,%eax           ; * lmr_delta
40ae91:  vpmaxsd %xmm1,%xmm0,%xmm2   ; max(campo, 1)
40ae9c:  idiv %r8d
40ae9f:  sub  %eax,%ecx
```

```cpp
if (lmr_delta > 0)
    r -= (a4 - a3) * lmr_delta / std::max(this->d_47f20, 1);
```

`this+0x47f20` é um campo próprio, iniciado a `1` no inicializador estático
(`409590`) e reescrito por busca no `arranca` (`440148`) — é o *rootDelta*. A
fórmula do documento divide pelo numerador; o binário divide por um campo que
não aparece na fórmula de todo.

## A função inteira

```cpp
int Busca::reducao(int a1, int a2, int a3, int a4, bool a5, bool a6, bool a7,
                   bool a8, bool a9, Move a10, int a11, bool a12, bool a13,
                   int a14) const
{
    int r = lmr[std::min(a1,63)][std::min(a2,63)];        // this+0x3b0

    if (a14 > 1)                                          // 40ae30
        r += cutcnt + (a14 > 2 ? cutcnt_mais : 0);        // 0x90, 0x94

    if (!a5)                                              // 40ae4c
        r += r * lmr_piora_f / 512;                       // 0x138

    if (lmr_delta > 0)                                    // 0x12c
        r -= (a4 - a3) * lmr_delta / std::max(d_47f20, 1);

    if (!a6) r -= r / 2;                                  // 40aea1

    if (lmr_ttpv > 0 || lmr_cut_f > 0) {                  // 0x14c / 0x140
        if (a9) {                                         // 40aec1
            r -= lmr_ttpv;
            if (a8) r -= lmr_ttpv_pv;                     // 0x150
            if (a12) r -= lmr_ttpv_alpha;                 // 0x154, via neg+and
            if (a13) r -= lmr_ttpv_fundo;                 // 0x158
        }
    }

    if (a7) {                                             // 40aef6 -> 40af80
        int t = (a10 == 0) ? lmr_cut_sem_tt : 0;          // 0x15c, cmove
        r += t + lmr_cut_f;                               // 0x140
    }

    if (!a8) r += lmr_nonpv_f;                            // 0x13c

    int h = a11 * 1024 / std::max(lmr_hist_div, 1);       // 0x144
    h = std::clamp(h, -2048, 2048);                       // 0x609da0 / 0x609db0
    r -= h;

    if (r < 0) r = r * lmr_ext_amort / 100;               // 0x164
    return r;
}
```

**Nota sobre o ramo `lmr_ttpv <= 0`:** em `40afa0`, quando `lmr_ttpv <= 0` mas
`lmr_cut_f > 0`, o código salta para `40aec1` **sem recarregar `%r12d`** — e
`sub %r12d,%ecx` subtrai então um `lmr_ttpv` não-positivo, isto é, soma. Está
assim no binário. Não é o que uma leitura em C escreveria, e uma reconstrução
"limpa" apaga-o sem dar por isso.

## O que se confirma da primeira passagem

`lmr_delta` existe, tem interruptor `KS_LMR_DELTA`, vale `0` no molde, e o
caminho de código está lá inteiro. A conclusão de que *"a `reducao` não é onde
está o buraco"* aguenta-se — mas a fórmula com que ela foi escrita não.


---

# `Busca::ordena`, e os 49 campos sem manete

## Primeiro o achado, porque muda o que se escreve

O molde das omissões cobre `0x10` a `0x23c`. As 122 manetes ocupam parte dele.
O resto — **49 campos com valor diferente de zero e sem qualquer cadeia `KS_*`**
— são constantes afinadas que **não se podem mudar em execução**. Não há
`getenv` que lhes chegue.

| desl. | valor | | desl. | valor | | desl. | valor |
|---|---:|---|---|---:|---|---|---:|
| `0x01c` | 150 | | `0x0b4` | 5600 | | `0x110` | 164 |
| `0x028` | 3 | | `0x0b8` | 290 | | `0x114` | 23 |
| `0x040` | 100 | | `0x0bc` | 19 | | `0x118` | 700 |
| `0x044` | 150 | | `0x0c0` | 9 | | `0x120` | 8 |
| `0x048` | 12 | | `0x0c4` | 348 | | `0x124` | 16 |
| `0x04c` | 75 | | `0x0c8` | 5 | | `0x138` | **197** |
| `0x0a4` | 40 | | `0x0dc` | 294 | | `0x13c` | **1024** |
| `0x0a8` | 5 | | `0x0e4` | 54 | | `0x144` | **22000** |
| `0x0ac` | 85 | | `0x0e8` | 3 | | `0x170` | **138** |
| `0x0b0` | 34 | | `0x0ec` | 5 | | `0x1a0` | **2400** |
| `0x1bc` | 335 | | `0x0f4` | 929 | | `0x1c4` | 712 |
| `0x1e0` | 300 | | `0x0f8` | 234 | | `0x1e4` | 20 |
| `0x1e8` | **60** | | `0x0fc` | 247 | | `0x1ec` | **256** |
| `0x1f0` | 550000 | | `0x100` | 134 | | `0x1f4` | 100 |
| `0x1f8` | 3 | | `0x104` | 177 | | `0x1fc` | 75 |
| `0x21c` | 7 | | `0x108` | 965 | | `0x238` | 20 |
| `0x10c` | 119 | | | | | | |

**Correcção ao que este documento já dizia.** A primeira passagem listou
`lmr_piora_f` (`0x138`), `lmr_nonpv_f` (`0x13c`) e `lmr_hist_div` (`0x144`) na
tabela dos "campos que a `reducao` lê", com ar de manetes. Não são: não têm
`KS_*`. São constantes, e valem 197, 1024 e 22000. Quem os torne configuráveis
na reconstrução acrescenta manetes que o motor perdido não tinha.

**E corrige o que eu próprio escrevi na `pontua`.** Disse que `0x1e8` e `0x1ec`
"não são manetes — são estado de execução". A primeira metade está certa, a
segunda não: têm valor no molde, **60** e **256**, e há uma escrita em `49af75`.
Nascem nestes valores e são actualizados durante a busca.

---

## `Busca::ordena(Position const&, Move, int) const` — `0x40aa70`, 878 b

É gémea da `hist_de`: mesma extracção do lance, mesmo balde de ameaças, mesma
tabela principal em `this+0x43b0`, mesma caminhada pelas distâncias
`1, 2, 4, 3, 6` com as mesmas fatias de `0x180` e o mesmo caminho pré-raiz em
`this+0x350`/`0x368`. Difere em **como pesa**, e tem um termo a mais.

```cpp
int Busca::ordena(const Position& pos, Move m, int ply) const
{
    const int lado = pos.byte[0x26c];
    const int para = m & 0x3f;                          // 40aa95
    const int de   = (m >> 6) & 0x3f;                   // 40aa7e
    const int peca = (pos.tabuleiro[de] & 7) - 1;       // 40aaa6

    int balde = 0;                                      // 40ad38
    if (!sem_balde) {                                   // 0x17c
        const uint64_t am = ameacas[ply];               // this+0x25e98
        balde = ((am >> de) & 1)*2 + ((am >> para) & 1);
    }

    const int n = hist[((lado*64 + de)*64 + para)*4 + balde];   // 40ab05
    const int a = n * 138 / 32;                         // 0x170, 40ab09..40ab18

    int c = 0;
    if (cont_n > 0) {                                   // 0x180
        c  = 2 * cont(ply-1, 0x000);                    // 40ab6a
        if (cont_n >= 2) c += cont(ply-2, 0x180);
        if (cont_n >= 3) c += cont(ply-4, 0x300);
        if (cont_n >= 4) c += cont(ply-3, 0x480);
        if (cont_n >= 5) c += cont(ply-6, 0x600);
    }
    c += a;                                             // 40acd7

    return c + pst[peca*64 + para] * 2400 / 100;        // 0x1a0, this+0x27d58
}
```

### As duas assimetrias, que são o que interessa

**1. O peso está trocado entre as duas funções.**

| | história principal | soma das continuações |
|---|---|---|
| `hist_de` | usada **crua** | `× ordem_cont / 32` (manete, vale 32) |
| `ordena` | `× 138 / 32` (constante) | usada **crua** |

São escolhas opostas, e uma delas é configurável e a outra não. Uma
reconstrução que aplique o mesmo peso nas duas — ou que torne as duas
configuráveis — ordena os lances de outra maneira.

**2. A `ordena` tem um termo que a `hist_de` não tem.**

```
40acdd:  mov  0x1a0(%rax),%r10d          ; 2400
40ace9:  lea  0x9f50(%rsi,%r9,1),%r9     ; peca*64 + para
40acf1:  imul 0x18(%rax,%r9,4),%r10d
40acfe:  imul $0x51eb851f ; sar $0x25    ; / 100
```

Uma tabela `[6][64]` em `this + 0x18 + 0x9f50*4 = this+0x27d58`, indexada por
peça e casa de destino, com peso `2400/100 = 24`. Sem condição nenhuma: entra
sempre.


---

# `Busca::garante_ameacas(Position const&, int) const` — `0x4119f0`, 970 b

Gera as ameaças do adversário para um ply, com cache. A `hist_de` e a `ordena`
chamam-na antes de indexar o balde, e lêem só **uma** das quatro coisas que ela
guarda.

## A cache e a sua chave

```
411a12:  mov 0x243b0(%r14),%rbx        ; chave guardada  (this+0x243b0 + ply*8)
411a19:  mov 0x40(%rax),%r15           ; chave da posição (pos.estado+0x40)
411a1d:  mov 0x34(%rax),%eax           ; contador (pos.estado+0x34)
411a20:  cmp $0xd,%eax  /  jle
411a2f:  sub $0xe,%eax                 ; (c - 14)
411a3c:  sar $0x3,%eax                 ;        >> 3
411a41:  imul $0x5851f42d4c957f2d
411a45:  add  $0x14057b7ef767814f
411a48:  xor  %rax,%r15
411a4b:  test %rbx,%rbx  /  je
411a50:  cmp  %r15,%rbx  /  je 411d39  ; acerto -> sai sem fazer nada
```

```cpp
uint64_t k = pos.estado->chave;
const int c = pos.estado->c34;
if (c > 13)
    k ^= ((c - 14) >> 3) * 0x5851f42d4c957f2dULL + 0x14057b7ef767814fULL;
if (cache_chave[ply] != 0 && cache_chave[ply] == k) return;   // acerto
```

O multiplicador `0x5851f42d4c957f2d` é a constante do PCG. O contador em
`estado+0x34` entra em degraus de 8 (`>> 3`) e só acima de 13 — ou seja, a
cache só se torna sensível a ele a partir daí.

## A escada de quatro bitboards

Acumula por tipo de peça, e **guarda quatro cortes cumulativos**:

```
peões    : pos+0x48 & pos+0x88, deslocados 7 e 9 com máscaras
           0x7f7f7f7f7f7f7f e 0xfefefefefefefe        -> -0x8(%rsp)
cavalos  : pos+0x50, PseudoAttacks[0x400 + casa*8]    -> r8 |= peões
bispos   : pos+0x58, DualMagics (AVX2)                -> r8 |=
torres   : pos+0x60, DualMagics                       -> -0x10(%rsp) = r8 |
damas    : pos+0x68, DualMagics                       -> r9 |= -0x10(%rsp)
rei      : pos+0x70, PseudoAttacks[0xc00 + casa*8]    -> r9 |=
```

```
411d09:  mov %r9,0x25e98(%r14)     ; TUDO          <- o que hist_de e ordena lêem
411d15:  mov %r8,0x26df8(%r14)     ; peões|cavalos|bispos
411d21:  mov %r9,0x26648(%r14)     ; só peões       (de -0x8(%rsp))
411d28:  mov %r8,0x275a8(%r14)     ; +torres        (de -0x10(%rsp))
411d2f:  mov %r15,0x243b0(%r14)    ; a chave
```

| tabela | base | conteúdo |
|---|---|---|
| ameaças totais | `this+0x25e98` | peões … rei |
| ameaças de peão | `this+0x26648` | só peões |
| ameaças ≤ bispo | `this+0x26df8` | peões, cavalos, bispos |
| ameaças ≤ torre | `this+0x275a8` | + torres |
| chave da cache | `this+0x243b0` | |

Todas indexadas por `ply*8`.

**Isto é mais do que se sabia.** A `hist_de` e a `ordena` só usam `0x25e98`. Os
outros três cortes existem para quem queira perguntar *"esta casa está atacada
por algo que valha menos do que uma torre?"* — e quem os usa é o `negamax`, que
ainda não foi lido. São três estruturas de dados que uma reconstrução feita a
partir do comportamento da árvore nunca revelaria, porque quem as lê pode estar
num ramo desligado.

**Nota:** `KS_AMEACA` (`0x178`, vale 20) **não é lida aqui**. A manete que tem
"ameaça" no nome vive noutro sítio.

## Os dois caminhos dos peões

`411a6a: cmp $0x1,%bl / je 411d50` — o lado a jogar escolhe entre dois blocos de
deslocamento, com as mesmas duas máscaras mas nos sentidos opostos. O bloco de
`411d50` é o simétrico; está no binário como código separado, não como um
deslocamento parametrizado.


---

# `Busca::aprende` e `Busca::indices` — a história de correcção

## `Busca::aprende(Position const&, int, int, int)` — `0x40dd20`, 805 b

A inversa da `corrigida`: escreve na mesma tabela.

```cpp
void Busca::aprende(const Position& pos, int a2, int a3, int ply)
{
    if (!corr || corr->vazio()) return;                 // 0x28fb8

    int ix[6];
    indices(pos, ply, ix);                              // 40dd65
    const int lado = pos.byte[0x26c];

    const int b = std::clamp((a2 * a3) / 8, -256, 256);  // 40dd6a..40dda4

    for (int i = 0; i < 6; i++)                          // desenrolado no binário
        if (ix[i] >= 0) {
            int& v = corr[(lado + 2*i)*16384 + ix[i]];
            v = std::clamp(v + b - std::abs(b)*v/1024, -1024, 1024);
        }
}
```

| constante | endereço | valor |
|---|---|---:|
| chão do bónus | `0x609dc0` | −256 |
| tecto do bónus | `0x609dd0` | 256 |
| tecto da tabela | `0x609de0` | 1024 |
| chão da tabela | `0x609df0` | −1024 |

Gravidade com `D = 1024` — diferente do `D = 16384` da `credita_captura`. **Dois
divisores de gravidade distintos no mesmo motor.** Uma transcrição que use o
mesmo nos dois sítios muda as duas tabelas.

Os seis compartimentos estão **escritos à mão, não em ciclo** — seis blocos de
código idênticos com os deslocamentos `0`, `2`, `4`, `6`, `8`, `10` a multiplicar
`16384`. O compilador não desenrolou: são seis instâncias distintas no fonte.

---

## `Busca::indices(Position const&, int, int*) const` — `0x40d690`, 1.328 b

Calcula as seis chaves que a `corrigida` e a `aprende` usam. Tem cache própria,
com a mesma mistura PCG da `garante_ameacas`:

```cpp
uint64_t k = pos.estado->chave;
const int c = pos.estado->c34;
if (c > 13)
    k ^= ((c - 14) >> 3) * 0x5851f42d4c957f2dULL + 0x14057b7ef767814fULL;
if (!DIAG && cache[ply] == k && k != 0) { /* 40db8e: devolve o guardado */ }
```

`DIAG` é um global em `0x75ab300` que **desliga a cache inteira** quando está a
1. A chave fica em `this+0x24b60 + ply*8`; os índices guardados ocupam três
palavras por ply (`lea (%r14,%r14,2)` em `40d91f`), isto é seis inteiros.

### O amassador

As chaves parciais saem de uma tabela Zobrist própria, `ALEA` em `0x75a9300`,
com blocos separados por tipo:

```
40d737:  xor 0x200(%rdi,%rsi,8),%rcx     ; peões de um lado   (pos+0x80 & pos+0x48)
40d764:  xor 0x1200(%r11,%r8,8),%r12     ; peões do outro     (pos+0x88 & pos+0x48)
```

mais um bloco que usa uma tabela estática local, `FORCA`, em `0x5ec544`, e as
máscaras `0x5eed00000000` e `0x6e15000000000`.

### O finalizador é splitmix64, e fecha o círculo

```
40d878:  shr $0x1e  / xor          ; x ^= x >> 30
40d892:  imul %r12                 ; x *= 0xbf58476d1ce4e5b9
40d8a4:  shr $0x1b  / xor          ; x ^= x >> 27
40d8b6:  imul %r15                 ; x *= 0x94d049bb133111eb
40d8ce:  shr $0x1f  / xor          ; x ^= x >> 31
40d8e3:  and $0x3fff               ; & 16383
```

As duas constantes são as canónicas do splitmix64. E a máscara `0x3fff` dá
**16.384** — exactamente o passo `*16384` que a `corrigida` e a `aprende` usam
para separar os canais. Três funções lidas em separado e os números batem: a
tabela de correcção tem 12 canais de 16.384 entradas.

Três índices são calculados e guardados em `out[0..2]` neste bloco
(`40d8f2`, `40d8fd`, `40d901`); os outros três saem do bloco seguinte, a partir
de `40d905`, com a constante `0x9e3779b97f4a7c15` em `40da21`.

**Por acabar nesta função:** o conteúdo exacto dos blocos que alimentam os
índices 3 a 5 — que conjuntos de peças entram em cada um. A estrutura, a cache,
o finalizador e as dimensões estão fechados.


---

# `Busca::repete_ja(Position&, Move)` — `0x41f3a0`, 685 b

Responde a *"se eu jogar este lance, repito?"*. Joga o lance a sério, procura, e
desfaz.

```cpp
bool Busca::repete_ja(Position& pos, Move m)
{
    // 1. a chave de agora, amassada, empilhada
    uint64_t k = pos.estado->chave;
    if (pos.estado->c34 > 13)
        k ^= ((pos.estado->c34 - 14) >> 3) * 0x5851f42d4c957f2dULL
           + 0x14057b7ef767814fULL;
    pilha.push_back(k);                          // this+0x29e30, 41f412

    // 2. jogar
    const bool xeque = pos.gives_check(m);       // 41f463
    aval.acumuladores.push();                    // 41f422..41f45c
    pos.do_move(m, estado, xeque, ...);          // 41f478

    // 3. a chave depois, amassada da mesma maneira
    uint64_t k2 = pos.estado->chave;
    if (pos.estado->c34 > 13) k2 ^= /* idem */;  // 41f491..41f4b5

    // 4. recuar de dois em dois
    const long n = pilha.tamanho();              // 41f4b8..41f4c9
    long lim = n - std::min<int>(pos.estado->c34, n);
    if (lim < 0) lim = 0;                        // 41f4dd

    bool achou = false;
    for (long i = n - 2; i >= lim; i -= 2)       // 41f4e0..41f5f0
        if (pilha[i] == k2) { achou = true; break; }   // 41f640

    // 5. desfazer tudo
    pos.undo_move(m);                            // 41f5f8
    aval.acumuladores.pop();                     // 41f604
    pilha.pop_back();                            // 41f60c
    return achou;
}
```

## Detalhes que só se vêem no binário

**O limite do recuo usa o contador `c34` *depois* do lance.** Em `41f485` lê-se
`0x34(%rdx)` com `rdx = pos.estado` já actualizado pelo `do_move`. É esse valor,
e não o de antes, que limita a procura.

**O ciclo está desenrolado 8×** com entrada calculada — `shr $1` e `and $0x7` em
`41f4ef`/`41f4f2`, seguidos de uma escada de `cmp $0..6` que salta para o meio do
bloco. É o compilador, não o fonte; a reconstrução escreve o ciclo simples.

**A primeira comparação acontece antes da escada** (`41f4f6`), fora do
desenrolamento. Quem reconstrua a escada literalmente e esqueça esta faz uma
comparação a menos.

**O amassamento da chave aparece aqui pela terceira vez** — igual ao da
`garante_ameacas` (`411a25`) e ao da `indices` (`40d6df`). São três cópias do
mesmo auxiliar embutido:

```cpp
inline uint64_t amassa(uint64_t chave, int c) {
    return c > 13 ? chave ^ (((c - 14) >> 3) * 0x5851f42d4c957f2dULL
                             + 0x14057b7ef767814fULL)
                  : chave;
}
```

A reconstrução deve ter **uma** função, chamada nos três sítios — mas se alguma
das três cópias divergir no binário, é sinal de que no fonte também eram
distintas. Verifiquei as três: são idênticas, instrução a instrução.

**Mexe na pilha de acumuladores da avaliação.** `this+0x388` aponta para o
avaliador; `0x6ede6c0` é o contador da pilha, incrementado antes do `do_move`
(`41f45c`) e decrementado depois do `undo_move` (`41f604`). Uma reconstrução que
trate `repete_ja` como uma consulta pura deixa a pilha desalinhada.


---

# `Busca::credita(Position const&, Move, int, int)` — `0x412280`, 1.832 b

O lado de escrita da `hist_de`. Actualiza cinco tabelas diferentes, e **cada uma
com a sua escala**.

## As cinco escalas — o achado desta função

| tabela | tecto do bónus | divisor da gravidade | magia / deslocamento |
|---|---:|---:|---|
| história dos quietos | ±15.000 | **15.000** | `0x45e7b273` `sar 44` |
| tabela `[6][64]` (`this+0x27d58`) | ±15.000 | **15.000** | `0x45e7b273` `sar 44` |
| história de peões | 8.192 | **8.192** | `sar 13` |
| história de continuação | ±30.000 | **30.000** | `0x45e7b273` `sar 45` |
| *(na `credita_captura`)* | ±16.384 | **16.384** | `sar 14` |
| *(na `aprende`)* | ±256 | **1.024** | `sar 10` |

Seis actualizações com gravidade no motor e **cinco divisores distintos**. A
mesma magia `0x45e7b273` serve 15.000 e 30.000 conforme o deslocamento seja 44
ou 45 — um bit de diferença entre duas tabelas.

Uma transcrição que unifique isto num `D` só muda todas as histórias ao mesmo
tempo, e a ordenação com elas.

## A função

```cpp
void Busca::credita(const Position& pos, Move m, int ply, int bonus)
{
    garante_ameacas(pos, ply);                          // 41229e

    const int para = m & 0x3f, de = (m >> 6) & 0x3f;
    const int lado = pos.byte[0x26c];
    const int peca = (pos.tabuleiro[de] & 7) - 1;

    if (low_f > 0 && ply <= 4) { /* 412850, ver abaixo */ }   // 0x1c0

    int balde = 0;
    if (!sem_balde) {                                   // 0x17c
        const uint64_t am = ameacas[ply];
        balde = ((am >> de) & 1)*2 + ((am >> para) & 1);
    }

    const int b = std::clamp(bonus, -15000, 15000);     // 0x3a98 / 0xffffc568

    // 1. história dos quietos
    int& h = hist[((lado*64 + de)*64 + para)*4 + balde];
    h += b - std::abs(b)*h/15000;                       // 41235e..412381

    // 2. a tabela [peça][destino], que a `ordena` lê
    int& p = pst[peca*64 + para];                       // this+0x27d58
    p += b - std::abs(b)*p/15000;                       // 4123a9..4123c7

    // 3. história de peões
    if (peao_f > 0 && !peoes.vazio()) {                 // 0x184
        const auto hh = pos.chave_peoes & (peao_ch - 1);      // 0x188
        int& q = peoes[((hh*12 + lado*6 + peca)*64) + para];
        int e = (bonus >= -3) ? std::min(bonus*1104/1024, 8192)
                              : /* 412460: bonus*459, chão −8192 */ ;
        q += e - std::abs(e)*q/8192;                    // 41247e..41249a
    }

    // 4. histórias de continuação, mesmas cinco fatias
    if (cont_n > 0) {                                   // 0x180
        const int c = std::clamp(bonus, -30000, 30000); // 0x7530 / 0xffff8ad0
        actualiza(ply-1, 0x000, c);                     // 4124bd..412536
        if (cont_n >= 2) actualiza(ply-2, 0x180, c);
        if (cont_n >= 3) actualiza(ply-4, 0x300, c);
        if (cont_n >= 4) actualiza(ply-3, 0x480, c);
        if (cont_n >= 5) actualiza(ply-6, 0x600, c);
    }
}
```

As quatro fatias `0x180`, `0x300`, `0x480`, `0x600` estão aqui com os mesmos
valores da `hist_de` e da `ordena`. **A ordem `1, 2, 4, 3, 6` fica triplamente
verificada**, em três funções lidas em separado.

## O ramo inerte, lido por inteiro

```
4122b9:  mov  0x1c0(%rbx),%eax     ; low_f  = 0
4122d3:  test %eax,%eax / jle      ; salta
4122d7:  cmp  $0x4,%ebp / jle      ; e só se ply <= 4
412850:  ...
```

```cpp
// nunca corre com as omissões: low_f = 0
if (!raso.vazio()) {
    int& r = raso[(ply*64 + de)*64 + para];             // this+0x2e8
    int e = bonus * 712 / 1024;                         // 0x1c4 = 712
    e = std::min(e, 15000);                             // 0x3a98
    r += e - std::abs(e)*r/15000;                       // 4128c0, sar 44
}
```

Repare-se na assimetria: a **leitura** (`hist_de`) é regulada por `low_f`
(`0x1c0`, vale 0), mas a **escrita** é escalada por `0x1c4`, que vale **712** e
não tem manete nenhuma. Quem ligasse `KS_LOW_F` herdava um factor de escrita
fixo em 712/1024 que não pode mudar.

Este é o ramo que este documento dava como *"um histórico de plies rasos
instalado numa função que o motor não chama"*. Está em duas funções que o motor
chama sempre. O que o desliga é a manete.


---

# `Busca::limpa()` — `0x41f800`, 2.564 b — e o mapa da estrutura

A `limpa` toca em cada tabela pelo seu endereço e com o seu tamanho. Cruzada com
o que as onze funções já lidas mostraram, dá a **planta completa da `Busca`**.

## O mapa

`g_busca` está em `.bss`, em `0x66e0c0`, com `0x47f28` bytes (294.696).

| desl. | tamanho | conteúdo | visto em |
|---|---:|---|---|
| `0x00000` | 8 | contador, nasce a `30` | `4091eb` |
| `0x00008` | 8 | limite de nós | `sem_tempo` |
| `0x00010`–`0x0023f` | 560 | **`Parametros`** — 122 manetes + 49 constantes | molde `0x6155f0` |
| `0x002e8`/`0x2f0`/`0x2f8` | 24 | vector: história de plies rasos | `hist_de`, `credita` |
| `0x00350` | 24 | peça dos lances pré-raiz (6) | `hist_de` |
| `0x00368` | 24 | destino dos lances pré-raiz (6) | `hist_de` |
| `0x00380` | 4 | quantos pré-raiz | `hist_de` |
| `0x00388` | 8 | ponteiro para o `Avaliador` | `repete_ja` |
| `0x00390` | 1 | bandeira **parar** | `sem_tempo` |
| `0x00398` | 8 | instante de arranque | `sem_tempo` |
| `0x003a8` | 8 | limite em ms | `sem_tempo` |
| `0x003b0` | 16.384 | **tabela LMR `[64][64]`** | `reducao` |
| `0x043b0` | 131.072 | **história dos quietos `[2][64][64][4]`** | `hist_de`, `credita` |
| `0x243b0` | 8/ply | chave da cache de ameaças | `garante_ameacas` |
| `0x24b60` | 8/ply | chave da cache de índices | `indices` |
| `0x25e98` | 8/ply | **ameaças totais** | `hist_de`, `ordena` |
| `0x26648` | 8/ply | ameaças só de peões | `garante_ameacas` |
| `0x26df8` | 8/ply | ameaças até bispo | `garante_ameacas` |
| `0x275a8` | 8/ply | ameaças até torre | `garante_ameacas` |
| `0x27d58` | 1.536 | **tabela `[6][64]`** peça×destino | `ordena`, `credita` |
| `0x28350`–`0x28368` | 24 | vector: **história de capturas** | `pontua`, `credita_captura` |
| `0x28b88` | 4/ply | killer 1 | `pontua` |
| `0x28b8a` | 4/ply | killer 2 (2 bytes ao lado) | `pontua` |
| `0x28f78` | 24 | vector: **histórias de continuação** | `hist_de`, `ordena`, `credita` |
| `0x28f98` | 24 | vector: **história de peões** | `hist_de`, `credita` |
| `0x28fb8` | 24 | vector: **história de correcção** | `corrigida`, `aprende` |
| `0x28fc0` | 4/ply | peça do lance jogado | `hist_de`, `ordena`, `credita` |
| `0x29398` | 4/ply | destino do lance jogado | idem |
| `0x29e30`/`0x29e38` | 16 | pilha de chaves para repetição | `repete_ja` |
| `0x47f20` | 4 | **rootDelta** — nasce a `1` | `reducao`, `arranca` |

Os tamanhos batem: `0x243b0 − 0x43b0 = 0x20000` = 131.072 = `2*64*64*4` inteiros,
e é exactamente o `mov $0x20000,%edx` que alimenta o `memset` em `41f898`. A
tabela `[6][64]` ocupa `0x28358 − 0x27d58 = 0x600` = 1.536 = `6*64*4`.

## O que a `limpa` faz

```cpp
void Busca::limpa()
{
    if (!cont)   { /* 41ff40: aloca */ }                // 0x28f78
    if (!peoes)  { /* 41ff30: aloca */ }                // 0x28f98
    if (!corr)   { /* 41ff20: aloca */ }                // 0x28fb8

    zera(0x28b90 .. 0x28f60);                           // killers e companhia
    memset(this + 0x43b0, 0, 0x20000);                  // 41f898
    zera(0x27d60 .. 0x28358);                           // a tabela [6][64]

    if (caphist.tamanho() <= 0x23fc) { /* 41fdb0: realoca */ }
    ...
}
```

As quatro chamadas a `_Znwm`/`_ZdlPvm` (`4200d0`, `420120`, `42017f`, `4201cc`)
são realocações: quando um dos vectores tem o tamanho errado, é deitado fora e
feito de novo, e só depois posto a zero com `memset`.

## Porque é que este mapa importa mais do que a função

Cada entrada acima foi vista **em duas funções independentes** — uma que lê e
uma que escreve. `0x27d58` sai da `ordena` a ler e da `credita` a escrever;
`0x28350` da `pontua` a ler e da `credita_captura` a escrever; `0x25e98` da
`garante_ameacas` a escrever e da `hist_de` a ler. Onde as duas batem, a
dimensão está certa.

As três que **ainda só foram vistas a escrever** — `0x26648`, `0x26df8`,
`0x275a8`, os cortes de ameaças por valor de peça — não têm leitor conhecido.
Quem as lê está no `negamax` ou na `quiescencia`, que faltam.


---

# `Busca::quiescencia(Position&, int, int, int)` — `0x420830`, 4.110 b

Primeira passagem: entrada, esqueleto, chamadas e os dois ramos de manete.
O interior do ciclo de lances fica marcado no fim como por fechar.

## As chamadas, que dão a forma

| endereço | função |
|---|---|
| `420891` | `Busca::sem_tempo()` |
| `420a62` | `TranspositionTable::sonda(chave, Entrada&)` |
| `420b7c` | `generate<GenType 0>(pos, lista)` |
| `420bd3`…`420d67` | `Position::legal(m)` — **sete vezes, desenrolado** |
| `420dc7` | `Busca::pontua(..., Limite = 3, ...)` |
| `420f1c` | `Avaliador::avalia_cheia(pos, ·, ·)` |
| `420f65` | `generate<GenType 4>(pos, lista)` |
| `421038` | `Busca::pontua(..., Limite = 3, ...)` |
| `42131e` | `Position::see_ge(m, limiar)` |
| `421376` | `pilha.emplace_back(chave)` |
| `4213d4` | `Position::prefetch_key(m)` |
| `4213f6` | `Position::gives_check(m)` |
| `42140e` | `Position::do_move(...)` |
| `42142e` | **`Busca::quiescencia(...)`** — recursão |
| `42143e` | `Position::undo_move(m)` |
| `42155d` | `TranspositionTable::guarda(chave, ·, ·, Limite, Move, ·)` |
| `4215c5`, `4216a5` | `Eval::NNUE::Network::evaluate(...)` — directo |

**Duas gerações distintas:** `GenType 0` em `420b7c` e `GenType 4` em `420f65`,
cada uma com a sua chamada à `pontua`. São os dois regimes da quiescência — com
e sem xeque — e não um só com filtro.

**As duas chamadas à `pontua` passam `Limite = 3` cravado** (`mov $0x3,%r8d` em
`420dbe` e `421029`). É o que fecha o achado já registado: `KS_TT_SUP` **não tem
efeito nenhum na quiescência**, porque o ramo `Limite == 2` nunca é tomado a
partir daqui.

## A entrada, linha a linha

```cpp
int Busca::quiescencia(Position& pos, int a2, int a3, int ply)
{
    ++nos;                                          // 0x280, 420865
    if (DIAG[10]) ++g_forma_qs;                     // 420876, diagnóstico
    if (ply > prof_max) prof_max = ply;             // 0x298, 42087e

    if (sem_tempo()) return /* 4215a0 */;           // 420891

    const auto* st = pos.estado;
    if (ply == 245) { /* 420dd8: profundidade máxima */ }   // 0xf5
    if (st->c34 > 99) return /* 4215a0 */;          // regra dos 50
    uint64_t k = st->chave;
    if (st->c34 > 13) { /* 420e70: amassa a chave */ }

    // repetição, mesmo ciclo desenrolado 8× da `repete_ja`
    for (long i = n-2; i >= lim; i -= 2)            // 4208cc..420921
        if (pilha[i] == k) return /* 4215a0 */;
    ...
}
```

O limite `245` (`0xf5`) é a profundidade máxima de ply; `99` (`0x63`) é a regra
dos 50 lances em meios-lances.

## Ramo da manete `otimismo` (`0x18c`, vale 114) — **activo**

Aparece duas vezes, nas duas entradas da avaliação:

```
420dff:  mov  0x18c(%rbx),%ecx      ; otimismo
420e05:  test %ecx,%ecx / je        ; se 0, salta
420e09:  mov  0x28374(%rbx),%r15d   ; valor de trabalho
420e18:  imul %r15d,%ecx
```

```
420ed7:  mov  0x18c(%rbx),%ecx      ; segunda entrada
420ee1:  mov  0x28374(%rbx),%edx
420ee7:  movzbl 0x28370(%rbx),%r11d ; bandeira em 0x28370
```

Dois campos novos para o mapa: **`this+0x28370`** (byte) e **`this+0x28374`**
(int), que alimentam o termo de optimismo passado à avaliação.

## Ramo da manete `qs_recapt` (`0x98`, vale 0) — **inerte, lido por inteiro**

```
4212fc:  mov  0x98(%r15),%r11d      ; qs_recapt
421303:  test %r11d,%r11d
421306:  je   421316                ; 0 -> não filtra
421308:  mov  %r12d,%ecx            ; o lance
42130f:  and  $0x3f,%ecx            ; casa de destino
42130b:  mov  0x44(%rsp),%eax       ; casa da recaptura
421312:  cmp  %eax,%ecx
421314:  je   42132b                ; igual -> aceita
421316:  xor  %edx,%edx             ; diferente -> rejeita
```

```cpp
if (qs_recapt != 0 && (m & 0x3f) != casa_recaptura)
    continue;                       // só recapturas
```

Com `qs_recapt = 0` a quiescência aceita todas as capturas. O caminho que a
restringe a recapturas **existe inteiro** e nunca corre. É o terceiro ramo
inerte lido por completo, depois do `low_f` e dos `killers`.

## Por fechar nesta função

O interior do ciclo de lances entre `421316` e `42155d`: o limiar exacto passado
ao `see_ge` em `42131e`, a condição que decide entre recursão e corte, e o que é
guardado na TT em `42155d`. A entrada, as chamadas, as duas gerações e os dois
ramos de manete estão fechados.


---

# `Busca::negamax` — o índice, antes das secções

`Kestrel::Busca::negamax(Position&, int, int, int, int, bool, bool)`
`0x436690`, 19.534 bytes, **4.165 instruções**. Mais o clone `.constprop.0` em
`0x43b2e0`, 13.121 bytes.

O documento manda fazer esta função *"por secções, na ordem dos Step — não de
uma vez"*. Para saber quais são as secções, extraí **todas as leituras de
parâmetros com base no `this`** (que aqui vive em `%rbp`), por ordem de
endereço. São 75.

## As chamadas, que dizem o que a função faz

| n.º | função |
|---:|---|
| 17 | `Busca::credita` |
| 16 | `Position::see_ge` |
| 16 | `Busca::credita_captura` |
| 14 | `Position::legal` |
| 8 | **`Busca::negamax`** (recursão) |
| 7 | `Position::undo_move` |
| 6 | `TranspositionTable::guarda` |
| 2 | `Busca::quiescencia`, `Busca::corrigida`, `do_move`, `gives_check`, `prefetch_key`, `Network::evaluate` |
| 1 | `sonda`, `guarda_so_aval`, `do_null_move`, `sem_tempo`, `avalia_cheia`, `Tablebases::search<false>`, `steady_clock::now` |

`do_null_move` uma vez, `Tablebases::search` uma vez, `do_move` só **duas** — o
resto das 7 `undo_move` vem dos clones e dos caminhos de saída.

## O índice, por ordem de endereço

| endereço | manete | omissão | guarda | secção |
|---|---|---:|---|---|
| `436859` | `contempt` | **0** | `test+jne` | empate / cuckoo |
| `436a8d` | `cuckoo` | 1 | `test+je` | |
| `436a9d` | `cuckoo_seminc` | **0** | `test+je` | |
| `436ab1` | `contempt` | **0** | `test+je` | |
| `436abd`…`436da5` | `elo_margin` / `our_elo` | 150 / 3000 | ×3 | limitação de força |
| `437002` | `rfp_mult` | **0** | `cmp+cmovg` | **RFP** |
| `437006` | `rfp_tecto` | 100000 | `cmp+cmovg` | |
| `437009` | `pior_adv` | **0** | — | |
| `43701c` | `rfp_m` | 110 | `cmp+cmovg` | |
| `43702c` | *(sem manete `0x1c`)* | 150 | — | |
| `437040` | *(sem manete `0x1bc`)* | 335 | `test+cmovns` | |
| `437064` | `corr_marg` | **0** | `test+jg` | |
| `437076` | `rfp_tt` | **0** | `cmp+cmovle` | |
| `43708c` | `rfp_prof` | 9 | `cmp+jle` | |
| `4370a1` | `tt_capt` | 1 | — | |
| `4370da` | *(sem manete `0xc8`)* | 5 | `cmp+jl` | **ProbCut** |
| `4370fa` | *(sem manete `0xc4`)* | 348 | `cmp+jle` | |
| `437147` | `pc` | **0** | `test+jle` | |
| `4371e1` | `pc_m` | 205 | — | |
| `437232`…`437253` | *(sem manete `0xec`,`0xe4`,`0xe8`)* | 5, 54, 3 | `cmove` | |
| `4375b8` | `hp_m` | 600 | `cmp+jle` | poda por histórico |
| `437875` | `otimismo` | 114 | `test+je` | avaliação |
| `4378f0` | `cont_seminc` | **0** | `test+je` | |
| `437906` | `cont_1lado` | **0** | `test+je` | |
| `4379b7` | `iir_prof` | 4 | `cmp+jl` | **IIR** |
| `4379d0` | `pcp_m`/`pcp_margem` | **0** | `test+jle` | |
| `438125` | `poda_red` | **0** | `test+je` | **LMP** |
| `43818c` | `lmp_melhora` | **0** | `test+je` | |
| `4381b9` | `lmp_prof` | 6 | — | |
| `4381d2` | `hp_lin` | **0** | `test+jne` | |
| `4381e7` | `hp_prof` | 4 | `cmp+jl` | |
| `4381fc`…`438225` | *(sem manete `0x48`,`0x4c`,`0x44`,`0x40`)* | 12, 75, 150, 100 | | |
| `43857a` | `alpha_desc` | **0** | `test+jle` | descida de alfa |
| `43858f` | `ad_min` | 3 | `cmp+jge` | |
| `438595` | `ad_max` | 12 | `cmp+jle` | |
| `43886d` | `ameaca` | 20 | `test+jne` | **ameaças** |
| `438a53` | `lmr_pecas_fim` | **0** | — | **LMR** |
| `438b74` | `ext`/`lmr_ext_max` | **0** | — | |
| `438dd3` | `reb_fundo` | 53 | — | rebate |
| `438dda` | `reb_raso` | 8 | — | |
| `4391f8` | `sing` | 5 | `cmp+jg` | **singular** |
| `439267` | `iir_prof` | 4 | `cmp+jg` | |
| `4394cd` | `seec` | 70 | — | SEE capturas |
| `43961c` | `otimismo` | 114 | `test+je` | |
| `4396f7` | `pcp_p` | 4 | `cmp+jl` | |
| `439754` | `pc` | **0** | — | |
| `439765` | `iir_sem_all` | **0** | `test+je` | |
| `43988c` | `cont_seminc` | **0** | `test+je` | |
| `4398a3` | `cont_1lado` | **0** | `test+je` | |
| `439c35` | `seeq` | 5 | — | SEE quietos |
| `439cc6` | `sing_lower` | **0** | `test+jne` | |
| `439d25` | `sing_margem` | 2 | — | |
| `439e32` | `ext_dupla` | 40 | `cmp+jle` | |
| `439ef5` | `pc_m` | 205 | — | |
| `439fa2` | `cont_seminc` | **0** | `test+je` | |
| `439fc0` | `cont_1lado` | **0** | `test+je` | |
| `43a0c6` | `nmp_divest` | 200 | — | **lance nulo** |
| `43a0ce` | `nmp_tecto` | 6 | `cmp+cmovg` | |
| `43a0d4` | `nmp_div` | 5 | — | |
| `43a0f0` | `nmp_base` | 4 | — | |
| `43a300` | `killers` | **0** | `test+jle` | |
| `43b29f` | `ext_neg` | 1 | — | |

## O que o índice já diz

**Dezassete das leituras são de manetes que valem zero** — `contempt`,
`cuckoo_seminc`, `rfp_mult`, `pior_adv`, `corr_marg`, `rfp_tt`, `pc`,
`poda_red`, `lmp_melhora`, `hp_lin`, `alpha_desc`, `lmr_pecas_fim`,
`ext`/`lmr_ext_max`, `iir_sem_all`, `cont_seminc`, `cont_1lado`, `sing_lower`,
`pcp_m`, `killers`. Confirma-se o que o inventário previa: **o grosso do código
inerte do motor está nesta função.**

**Doze leituras são de campos sem manete** — `0x1c`=150, `0x1bc`=335, `0xc8`=5,
`0xc4`=348, `0xec`=5, `0xe4`=54, `0xe8`=3, `0x28`=3, `0x48`=12, `0x4c`=75,
`0x44`=150, `0x40`=100. Constantes cravadas, no meio dos cortes, sem forma de
as mexer.

**`cont_seminc` e `cont_1lado` aparecem aos pares, quatro vezes** (`4378f0`,
`43988c`, `439fa2` e `439fc0`). São quatro sítios distintos com o mesmo par de
condições, ambas a zero — quatro blocos inertes com a mesma forma.

**A ordem de endereços não é a ordem de execução.** O lance nulo aparece em
`43a0c6`, depois do singular e do LMR, o que não é a ordem de um `negamax`. O
compilador moveu blocos frios para o fim. A ordem real tem de sair dos saltos,
não dos endereços — e é o que as secções a seguir vão fazer.


---

# `negamax`, secção 1: RFP, razoring, entrada do lance nulo e ProbCut

`436ffe` a `43719e`. Ordem de execução real, seguida pelos saltos.

## RFP — `436ffe`…`4370d0`

```
437002:  mov   0x14(%rbp),%r10d      ; rfp_mult    = 0
437006:  mov   0x18(%rbp),%ecx       ; rfp_tecto   = 100000
437009:  mov   0x1b0(%rbp),%edi      ; pior_adv    = 0
43700f:  movzbl 0xc4(%rsp),%r9d      ; bandeira da pilha
437018:  imul  %edx,%r10d            ; rfp_mult * prof
43701c:  add   0x10(%rbp),%r10d      ; + rfp_m (110)
437023:  cmovg %ecx,%r10d            ; min(·, rfp_tecto)
43702c:  and   0x1c(%rbp),%eax       ; bandeira ? 150 : 0      [0x1c, sem manete]
43702f:  imul  %r10d,%edx            ; prof * margem
437036:  sub   %eax,%r11d
43703e:  je    437064                ; pior_adv = 0 -> salta
437040:  imul  0x1bc(%rbp),%r11d     ; * 335 / 1024            [0x1bc, sem manete]
437064:  mov   0x1b8(%rbp),%edx      ; corr_marg = 0
437070:  jg    439447                ; > 0 -> bloco frio
437076:  mov   0x20(%rbp),%ecx       ; rfp_tt = 0
437082:  cmovle %r10d,%esi           ; rfp_tt <= 0 -> usa a margem toda
43708c:  cmp   %eax,0x24(%rbp)       ; rfp_prof (9)
43708f:  jle   4370d6                ; prof >= 9 -> sem RFP
43709c:  cmp   %r11d,%r10d           ; (aval - margem) vs beta
43709f:  jl    4370d6
```

```cpp
int m = std::min(rfp_mult * prof + rfp_m, rfp_tecto);   // = 110 com as omissões
m = prof * m - (bandeira ? 150 : 0);                    // 0x1c
if (pior_adv) m -= bandeira2 ? m * 335 / 1024 : 0;      // 0x1bc — inerte
if (corr_marg > 0) { /* 439447 */ }                     // inerte
const int margem = (rfp_tt > 0) ? std::min(rfp_tt, m) : m;   // inerte
if (prof < rfp_prof && aval - margem >= beta) {
    if (tt_capt == 0 || lance_tt == 0) {                // 4370a1..4370b8
        if (bandeira && |aval| ... ) return /* 439c4c */;
    }
}
```

Com as omissões a margem é **`prof * 110`**, menos 150 conforme a bandeira em
`0xc4(%rsp)`. Quatro dos cinco modificadores (`rfp_mult`, `pior_adv`,
`corr_marg`, `rfp_tt`) estão a zero: o código está lá, e a margem é linear.

## Razoring — `4370d6`…`437108`

```cpp
if (prof <= 5                       // 0xc8 = 5, sem manete
    && (unsigned)(alpha + 1999) <= 3998        // alpha fora da zona de mate
    && aval + prof * 348 <= alpha)             // 0xc4 = 348, sem manete
    return /* 439c72: quiescência */;
```

Dois limiares sem manete nenhuma: profundidade **5** e margem **348 por ply**.

## Entrada do lance nulo — `437117`…`43713a`

```cpp
if ((arg7 || DIAG[9]) && prof > 2 && aval >= std::max(beta, ebx))
    goto lance_nulo;                // 43a060
```

O corpo está em `43a060`, longe daqui — o compilador afastou o bloco. Por isso
é que a ordem dos endereços enganava: o lance nulo parecia vir depois do
singular e do LMR, e vem antes.

## ProbCut — `437140`…`43719e` — **e o aviso do documento não se aplica**

```
437147:  mov  0xf0(%rbp),%edx        ; pc = 0
437153:  test %edx,%edx
437155:  jle  4379d0                 ; <- salta o ProbCut INTEIRO
437160:  cmpw $0x0,0x29c3e(%rbp,%r14,2)   ; lance excluído deste ply
437170:  cmp  %edx,0x4(%rsp)
437174:  jl   4379d0                 ; prof < pc -> salta
```

Este documento avisa que *"`pc_prof = 0` com a condição `prof >= pc_prof` liga
**sempre**, e a árvore foi a 878.281 nós em vez de 425.973"*.

**No binário isso não acontece.** Antes da comparação `prof >= pc` há um
`test`/`jle` sobre o próprio `pc` (`437153`), que salta o bloco inteiro quando
`pc <= 0`. O motor perdido tinha a guarda; foi a reconstrução que a perdeu.

É uma distinção que importa: a armadilha é real, mas o defeito era **nosso**,
não dele. Quem reconstruir esta secção tem de pôr as **duas** condições, pela
ordem certa.

## Um campo novo para o mapa

`437160`: `cmpw $0x0,0x29c3e(%rbp,%r14,2)` — um vector de 16 bits por ply em
**`this+0x29c3e`**. Pelo uso (o ProbCut é saltado quando não é zero) é o lance
excluído do nó, o que a busca singular põe lá.


---

# `negamax`, secção 2: o lance nulo — `43a060`…

O bloco está longe da sua condição de entrada (`437117`), afastado pelo
compilador. A ordem real é: entra em `437117`, salta para cá, e volta a
`437140` quando recusa.

## As três recusas, antes de reduzir

```
43a060:  imul $0xffffffec,0x4(%rsp),%ecx   ; -20 * prof
43a06d:  and  $0xffffffd8,%r9d             ; bandeira ? -40 : 0
43a074:  lea  0x63(%rcx,%r9,1),%esi        ; + 99
43a07b:  jge  437140                       ; recusa 1
```

```cpp
if (aval < beta - 20*prof + 99 - (bandeira ? 40 : 0)) goto sem_nulo;
```

Três constantes cravadas — **20 por ply, 99 de base, 40 condicionais** — e
nenhuma tem manete.

```
43a090:  mov  0x28(%r10,%rbx,4),%eax   ; material não-peão do lado a jogar
43a097:  jle  437140                   ; recusa 2: só peões e rei
```

```
43a0a2:  lea  -0x1(%r13),%eax          ; ply - 1
43a0a8:  cmpb $0x0,0x29b48(%rbp,%rax,1)
43a0b0:  jne  437140                   ; recusa 3: o ply anterior já fez nulo
```

**Campo novo:** `this+0x29b48`, um byte por ply — a marca de "este ply fez lance
nulo". É o que impede dois nulos seguidos.

## A redução

```
43a0c6:  vmovd 0xd4(%rbp),%xmm3     ; nmp_divest = 200
43a0ce:  mov   0xd0(%rbp),%edi      ; nmp_tecto  = 6
43a0d4:  mov   0xd8(%rbp),%ecx      ; nmp_div    = 5
43a0da:  sub   %r12d,%eax           ; aval - beta
43a0dd:  vpmaxsd %xmm4,%xmm3,%xmm5  ; max(nmp_divest, 1)
43a0e8:  idiv  %r11d
43a0ed:  cmovg %edi,%eax            ; min(·, nmp_tecto)
43a0f0:  add   0xcc(%rbp),%eax      ; + nmp_base = 4
43a0fa:  jle   43a10e               ; nmp_div <= 0 -> pára aqui
43a10a:  idiv  %ecx                 ; prof / nmp_div
43a10c:  add   %eax,%ebx
```

```cpp
int R = std::min((aval - beta) / std::max(nmp_divest, 1), nmp_tecto)
      + nmp_base;
if (nmp_div > 0 && !DIAG[8])
    R += prof / nmp_div;
```

Com as omissões: `R = min((aval − beta)/200, 6) + 4 + prof/5`.

Todas as quatro têm manete (`KS_NMP_DIVEST`, `KS_NMP_TECTO`, `KS_NMP_BASE`,
`KS_NMP_DIV`) e nenhuma está a zero — esta secção está inteira em uso.

## A seguir

`43a10e` em diante: empilha a chave (com o mesmo amassamento PCG, quarta
ocorrência), chama `do_null_move` (`413020`) e recorre. A verificação do
resultado e o eventual *null-move verification search* ficam por ler.


---

# `negamax`, secção 3: LMP, poda por histórico e futilidade — `438125`…`43824b`

## `poda_red` — inerte, e reutiliza a tabela do LMR

```
438125:  mov   0x128(%rbp),%r11d     ; poda_red = 0
438131:  je    43817f                ; 0 -> salta tudo
438141:  cmovle                       ; min(a, 63)
438148:  cmovle                       ; min(b, 63)
438153:  lea   0xe8(%r9,%rsi,1),%rax
43815b:  mov   0x10(%rbp,%rax,4),%r8d ; <- a MESMA tabela [64][64] em this+0x3b0
43816e:  sar   $0xa                   ; / 1024
438172:  sub   %r11d,%edi
43817c:  cmovs                        ; max(0)
```

```cpp
if (poda_red) {
    int r = lmr[std::min(a,63)][std::min(b,63)] / 1024;
    contador = std::max(contador - r, 0);
}
```

O ramo está inteiro e nunca corre. E é uma informação nova: **a tabela LMR de
`this+0x3b0` seria lida por duas funções**, a `reducao` e este bloco do
`negamax`, com divisores diferentes — sem divisor na `reducao`, `/1024` aqui.

## LMP — `438188`…`4381c9`

```cpp
int limite = prof*prof + 3;                       // 0x28 = 3, sem manete
if (lmp_melhora && !bandeira_c4) limite /= 2;     // 0x30 — inerte
if (lmp_prof >= prof && limite <= n_lances)       // 0x2c = 6
    goto podar;                                   // 439ca3
```

Com `lmp_melhora = 0` o limite nunca é dividido. O corte é
`prof² + 3 <= n_lances`, activo até à profundidade 6.

## Poda por histórico — `4381cf`…`4381ea` — **e um interruptor de forma**

```
4381cf:  mov  0x34(%rbp),%eax    ; hp_m   = 600
4381d2:  mov  0x3c(%rbp),%r9d    ; hp_lin = 0
4381d6:  imul %r11d,%eax         ; hp_m * prof
4381da:  test %r9d,%r9d
4381dd:  jne  4381e3             ; hp_lin != 0 -> fica linear
4381df:  imul %r11d,%eax         ; hp_lin == 0 -> * prof OUTRA VEZ
4381e7:  cmp  %esi,0x38(%rbp)    ; hp_prof = 4
```

```cpp
int limiar = hp_lin ? hp_m * prof : hp_m * prof * prof;
if (hp_prof >= prof) { /* usa -limiar */ }
```

**`KS_HP_LIN` não é uma manete de ligar/desligar: é um interruptor de forma.**
A zero, o limiar é **quadrático** (`600 * prof²`); a um, passa a **linear**
(`600 * prof`). Não acrescenta nem tira um bloco — troca a curva.

Este é o caso mais traiçoeiro do inventário. Uma reconstrução que implemente
"poda por histórico com margem `hp_m * prof`" está a implementar o ramo que o
motor **não** percorre, e parece certa: a manete existe, o nome bate, o valor
bate. Só a forma é que é outra.

## Futilidade — `4381fc`…`438242`

```
4381fc:  cmp  %ecx,0x48(%rbp)     ; 12   (sem manete)
43820e:  vmovd 0x4c(%rbp),%xmm6   ; 75   (sem manete)
438213:  mov  0x44(%rbp),%edi     ; 150  (sem manete)
438225:  add  0x40(%rbp),%r8d     ; 100  (sem manete)
438220:  vpmaxsd                  ; max(75, 1)
43822e:  imul %ecx,%edi           ; 150 * prof_red
438236:  idiv %r9d                ; hist / 75
43823f:  cmp  %esi,%r8d           ; vs alpha
438242:  jle  439ee8              ; poda
```

```cpp
if (prof_red <= 12
    && aval + 100 + 150*prof_red + hist / std::max(75, 1) <= alpha)
    goto podar;
```

**Quatro constantes seguidas, nenhuma com manete**: `12`, `100`, `150`, `75`.
A margem de futilidade do motor perdido não é configurável de todo.

E logo a seguir:

```
438248:  cmp $0x8,%ecx
43824b:  jle 439c28              ; prof_red <= 8 -> outro bloco (SEE)
```

## Somatório das constantes sem manete nesta secção

| valor | desl. | onde |
|---:|---|---|
| 3 | `0x28` | base do LMP |
| 100 | `0x40` | futilidade, termo constante |
| 150 | `0x44` | futilidade, por ply |
| 12 | `0x48` | futilidade, profundidade máxima |
| 75 | `0x4c` | futilidade, divisor do histórico |


---

# `negamax`, secção 4: extensão singular — `4391f8`…

## As cinco condições de entrada

```
4391f8:  cmp    %edi,0x80(%rbp)      ; sing = 5, contra prof
4391fe:  jg     438275               ; sing > prof -> não
439204:  cmpl   $0xed,0x48(%rsp)     ; 237
43920c:  jg     438275
439212:  cmpb   $0x0,0xc0(%rsp)      ; bandeira da pilha
43921a:  je     438275
439220:  movzbl 0x130(%rsp),%r11d    ; o Limite guardado na TT
439229:  cmp    $0x3,%r11b
43922d:  je     438275               ; Limite == 3 -> não
439233:  lea    -0x3(%rdi),%r8d      ; prof - 3
439237:  cmp    %r8d,0x128(%rsp)     ; prof_tt >= prof - 3
43923f:  jge    439cc0               ; -> busca singular
439245:  lea    -0x1(%rdi),%edi      ; senão: prof - 1
```

```cpp
if (prof >= sing                     // 0x80 = 5
    && valor_tt <= 237               // 0xed
    && bandeira_c0
    && limite_tt != 3
    && prof_tt >= prof - 3)
    goto singular;                   // 439cc0
prof -= 1;
```

O `237` (`0xed`) é uma constante crua na pilha — está perto do tecto de ply
`245` (`0xf5`) que a `quiescencia` usa, mas não é o mesmo número.

## `Limite == 3` é o valor que exclui

A busca singular **nunca acontece** quando o limite guardado na TT vale `3`.

Isto liga-se ao achado já registado na `pontua`: a `quiescencia` passa
`Limite = 3` cravado nas duas chamadas (`420dbe`, `421029`), e é por isso que
`KS_TT_SUP` não tem efeito nenhum lá. Agora vê-se que `3` é também o valor que
desliga o singular.

Não é coincidência de dois sítios: **`3` é um valor com significado próprio no
enumerado `Limite`** — o que a quiescência produz e o que a busca singular
recusa. Nomeá-lo mal na reconstrução afecta os dois sítios ao mesmo tempo.

## A chamada recursiva de verificação — `439294`…`4392d3`

```
4392a5:  push  $0x0                  ; arg7 = false
4392aa:  push  $0x1                  ; arg6 = true
4392b4:  neg   %ecx                  ; -beta
4392be:  call  436690                ; negamax(...)
4392c3:  neg   %eax                  ; -resultado
```

Os dois argumentos booleanos entram pela pilha com valores cravados — `true` e
`false` — e a janela é invertida (`neg`). É a forma normal de uma janela nula
invertida, e confirma quais são os `a6`/`a7` da assinatura: `a6` é o que aqui
vale `true`, `a7` o que vale `false`.

## Ainda por ler no singular

O corpo em `439cc0`: a margem aplicada (`sing_margem`, `0x84` = 2), o ramo
inerte do `sing_lower` (`0x8c` = 0, em `439cc6`) e a extensão dupla
(`ext_dupla`, `0x88` = 40, em `439e32`).


---

# `negamax`, secção 5: a chamada à `reducao` e os seus catorze argumentos

A `reducao` é chamada **uma vez em cada `negamax`** — `438b65` no principal,
`43c971` no clone — e em mais lado nenhum do motor. Os catorze argumentos ficam
agora identificados, o que fecha o que a primeira passagem deixou em aberto.

## Como os argumentos chegam

Cinco em registos, nove na pilha. Os `push` são pela ordem inversa, portanto o
**último** `push` é o `a6`.

```
438ae7:  sub   $0x8,%rsp                     ; alinhamento
438b00:  mov   0x47b28(%rbp,%rsi,4),%r11d    ; <- campo novo, por ply
438b08:  push  %r11                          ; a14
438b0a:  push  %r9                           ; a13
438b0c:  push  %r12                          ; a12
438b0e:  push  %rdx                          ; a11  (valor do histórico)
438b18:  push  %r10                          ; a10  (Move, 16 bits)
438b22:  push  %rcx                          ; a9
438b2c:  push  %r8                           ; a8
438b37:  push  %r12                          ; a7
438b39:  push  $0x1                          ; a6  <- CONSTANTE
438b5a:  mov   0x54(%rsp),%esi               ; a1
438b53:  mov   0xa8(%rsp),%edx               ; a2
438b5e:  mov   %r12d,%ecx                    ; a3
438b4b:  mov   0x80(%rsp),%r8d               ; a4
438b61:  and   $0x1,%r9d                     ; a5
438b65:  call  40ade0
```

| arg | origem | papel dentro da `reducao` |
|---|---|---|
| `a1` | `0x54(%rsp)` | primeiro índice da tabela `[64][64]` |
| `a2` | `0xa8(%rsp)` | segundo índice |
| `a3`, `a4` | `%r12d`, `0x80(%rsp)` | os dois termos de `(a4 − a3) * lmr_delta / rootDelta` |
| `a5` | `0x114(%rsp) & 1` | se falso, aplica `lmr_piora_f` |
| **`a6`** | **`$0x1` cravado** | **se falso, `r -= r/2`** |
| `a7` | `0xe5(%rsp)` | ramo do `lmr_cut_f` / `cut_sem_tt` |
| `a8` | `0x8b(%rsp)` | ramo do `ttpv_pv` e do `lmr_nonpv_f` |
| `a9` | `0x11b(%rsp)` | ramo do `lmr_ttpv` |
| `a10` | `0xce(%rsp)` | o lance, comparado com zero em `40af83` |
| `a11` | `*(0x8(%rsp))` | o histórico, dividido por `lmr_hist_div` |
| `a12`, `a13` | `%r12`, `prof_tt >= prof` | `ttpv_alpha` e `ttpv_fundo` |
| `a14` | `this[0x47b28 + ply*4]` | contagem de cortes — `if (a14 > 1)` |

**Campo novo para o mapa:** `this+0x47b28`, inteiro por ply. Alimenta a condição
`a14 > 1` que liga o termo do `cutcnt`, e é a única coisa que o liga.

## O achado: um ramo da `reducao` está morto

```
438b39:  push $0x1          ; negamax principal
43c958:  push $0x1          ; clone .constprop.0
```

**Os dois — e únicos — chamadores passam `a6 = true`.** Dentro da `reducao`:

```
40aea1:  test %r12b,%r12b
40aea4:  jne  40aeb1        ; true -> salta
40aea6:  mov  %ecx,%ebx
40aea8:  shr  $0x1f,%ebx
40aeab:  add  %ecx,%ebx
40aead:  sar  $1,%ebx
40aeaf:  sub  %ebx,%ecx     ; r -= r/2
```

A primeira passagem deste documento registou este bloco como
*`shr $0x1f / add / sar $1 / sub → r -= r/2 (a metade das capturas)`*, na lista
do esqueleto, sem indicar que fosse condicional nem morto. **Nunca corre.**

Não é um ramo desligado por manete — é um ramo desligado por **argumento
constante**, que é uma quarta maneira de o código ficar inerte, a juntar às três
já catalogadas:

| modo | como se detecta |
|---|---|
| 1. salto sobre manete a zero | `test`/`jle` |
| 2. `cmov` sobre manete a zero | sem salto |
| 3. termo multiplicado por zero | sem salto nem `cmov` |
| **4. argumento constante no chamador** | **só se vê indo ao sítio da chamada** |

O modo 4 é invisível a quem leia a função sozinha: dentro da `reducao` o ramo
parece vivo e condicional. Só a leitura dos dois chamadores o mata. Isto obriga
a reler os argumentos constantes de todas as chamadas do motor antes de dar
qualquer função por fechada.

## Logo a seguir à chamada

```
438b6a:  add  $0x50,%rsp          ; limpa os 9 argumentos + alinhamento
438b74:  mov  0x160(%rbp),%edi    ; ext / lmr_ext_max = 0
438b7a:  lea  0x3ff(%rax),%r9d    ; o resultado, para dividir por 1024
```

A redução devolvida entra num arredondamento por 1024 e cruza-se com
`lmr_ext_max`, que está a zero.


---

# `negamax`, secção 6: os dois `negamax` são raiz e interior

Uma questão de estrutura que estava por resolver: para que serve o clone
`.constprop.0` de 13.121 bytes, ao lado da `negamax` de 19.534.

## Quem chama quem — respondido

```
440181:  call 43b2e0 <negamax.constprop.0>     <- a UNICA chamada ao clone,
                                                  e vem do `arranca`
```

As dezasseis chamadas recursivas — oito no principal, oito no clone — apontam
**todas** para `436690`, o `negamax` principal. Nenhuma chama o clone.

```
arranca ──uma vez──► negamax.constprop.0  (a raiz)
                          │
                          └──8 chamadas──► negamax  (todo o resto)
                                              │
                                              └──8 chamadas──► negamax
```

**O clone é o nó de raiz.** Não é uma optimização acidental do compilador: é a
raiz especializada, com um dos dois booleanos dobrado para dentro. No sítio da
chamada há um só `push`:

```
440174:  sub   $0x8,%rsp
440178:  push  $0x0          ; só um argumento de pilha
440181:  call  43b2e0
```

Sete argumentos, cinco em registos, e **um** na pilha em vez de dois — o `a6`
foi constante-propagado para dentro do clone. Por isso o clone tem menos 6 KB.

Consequência para a reconstrução: **é uma função só no fonte**, chamada com um
argumento constante na raiz. Escrever duas é multiplicar o trabalho e criar duas
coisas que hão-de divergir.

## O `rootDelta` fica confirmado

Três instruções antes da chamada à raiz:

```
440132:  test   %r11d,%r11d
440137:  cmovle %r10d,%r11d          ; chão
440148:  mov    %r11d,0x47f20(%rdi)  ; <- rootDelta
```

É o mesmo `this+0x47f20` que a `reducao` lê em `40ae84` como divisor do termo
`lmr_delta`, e que o inicializador estático põe a `1` em `409590`. Fecha o
circuito: a secção que corrigiu a fórmula da `reducao` (`/rootDelta`, não `/X`)
tem agora o sítio onde o valor é escrito, uma vez por iteração da raiz.

## As combinações dos dois booleanos

Das dezasseis chamadas recursivas:

| `a6` | `a7` | quantas | onde |
|---|---|---:|---|
| `0` | variável | 10 | ciclo de lances normal |
| `0` | `1` | 4 | `438bed`, `438e95`, `43c9f3`, `43cbf5`, `43cfb5` |
| **`1`** | **`0`** | **2** | `4392be` e `43cd5c` — **só a verificação singular** |

`a6 = 1` acontece **exclusivamente** nas duas chamadas de verificação do
singular. Em todo o resto do motor vale zero.

Isto tem a mesma natureza do achado da `reducao`: qualquer bloco dentro do
`negamax` guardado por `a6` só é percorrido a partir da busca singular. Ao ler
as secções que faltam, um `test` sobre o `a6` não é uma bifurcação normal — é
"isto só acontece dentro do singular".

## O modo 4, agora com método

A quarta maneira de o código ficar inerte — argumento constante no chamador —
exige verificar todos os sítios de chamada antes de dar uma função por fechada.
Para as funções já lidas:

| função | chamadores | argumentos constantes |
|---|---|---|
| `reducao` | 2 | **`a6 = 1` nos dois** → `r -= r/2` morto |
| `pontua` | 4 | `Limite = 3` nas duas da `quiescencia` |
| `negamax` | 17 | `a6 = 1` só nas duas do singular |
| `negamax.constprop.0` | 1 | um booleano dobrado para dentro |

Falta fazer o mesmo às restantes: `credita` (17 chamadas), `credita_captura`
(16), `see_ge` (16), `corrigida`, `aprende`, `hist_de`, `ordena`.


---

# `negamax`, secção 7: correcção à secção 6, e a varredura dos argumentos constantes

## Correcção: o clone é o **ply 1**, não "a raiz"

Na secção 6 escrevi que o clone `.constprop.0` é *"o nó de raiz, com um dos dois
booleanos dobrado para dentro"*. **A segunda metade está errada**, e a primeira
é imprecisa.

O que o binário diz:

```
44012c:  mov   $0x1,%r9d        ; <- arg5 = 1
440178:  push  $0x0
440181:  call  43b2e0
```

e o prólogo do clone **nunca lê `%r9d`**:

```
43b2f7:  mov 0x1010(%rsp),%eax   ; o argumento de pilha
43b2fe:  mov %edx,0x20(%rsp)
43b302:  mov %ecx,0x64(%rsp)
43b306:  mov %r8d,0x40(%rsp)     ; e mais nada
```

O argumento propagado é o **`arg5`**, que no `negamax` principal é o **ply**
(`4366cd: mov %r9d,%r13d`, comparado com a profundidade máxima em `0x298`).
O clone é a especialização para **`ply = 1`**.

Confirma-se por aritmética independente. O `a14` da `reducao`:

| | expressão | ply |
|---|---|---|
| principal | `0x47b28(%rbp,%rsi,4)` | variável |
| clone | `0x47b2c(%rbx)` | `0x47b2c − 0x47b28 = 4` → **ply = 1** |

E pelas chamadas com `ply` cravado dentro do clone — `hist_de` (`43c512`),
`ordena` (`43c536`), `corrigida` (`43c0db`), `aprende` (`43e2f5`), todas com
`xor` a zero, isto é `ply − 1 = 0`.

**A estrutura verdadeira:**

```
arranca ──ciclo dos lances da raiz──► negamax.constprop.0   (ply = 1)
                                            │
                                            └──► negamax    (ply >= 2)
```

Um só sítio de chamada (`440181`), executado uma vez por lance da raiz. O
`arranca` faz o nó de raiz ele próprio — é por isso que tem lá dentro o
`stable_sort` sobre `pair<Move,int>` que aparece na tabela de símbolos.

A conclusão prática da secção 6 mantém-se e reforça-se: **é uma função só no
fonte**. O que muda é o que se propaga — `ply = 1`, não um booleano.

## A varredura dos argumentos constantes

O modo 4 (argumento constante no chamador) obriga a ver os sítios de chamada de
tudo o que já foi dado por fechado. Feito, para as onze funções principais.

**Achados reais:**

| função | sítios | constante | consequência |
|---|---:|---|---|
| `reducao` | 2 | `a6 = 1` nos dois | `r -= r/2` **morto** |
| `pontua` | 4 | `Limite = 3` nas duas da `quiescencia` | `tt_sup` sem efeito na quiescência |
| `negamax` | 17 | `a6 = 1` só nas 2 do singular | blocos sob `a6` só existem no singular |
| `aprende` | 2 | `ply = 0` nos dois | mas ambos estão no clone (ply 1) — **não é achado** |
| `corrigida` | 2 | `ply = 0` nos dois | idem |
| `hist_de`, `ordena` | 1 cada | `ply = 0` | idem |

**Aviso de método, porque quase escrevi o contrário.** A varredura devolveu
`hist_de`, `ordena`, `corrigida` e `aprende` *"sempre chamadas com `ply = 0`"* —
o que seria um achado enorme, e é falso. São chamadas assim **porque os únicos
sítios encontrados estão dentro do clone**, que é o ply 1. Num nó de ply 1,
`ply − 1` é mesmo 0.

Uma ferramenta que varra sítios de chamada sem saber em que especialização está
produz exactamente este género de falso achado: tecnicamente correcto, e
completamente enganador. O `negamax` principal chama as mesmas funções com o ply
variável.

## O que a varredura confirma, e é útil

**`TT::guarda`** é chamada com `r8 = 1` em oito dos nove sítios, e num deles
(`439bc2`) com `rdx = 245`, `rcx = 31507`, `r9 = 0` — a gravação do valor de
profundidade máxima com nota `31507`, um a mais do que o tecto `31506` da
`avalia_cheia`. Dois números adjacentes com papéis distintos.

**`see_ge`** é chamada dez vezes com limiar constante: `0` na `quiescencia`
(`42131e`) e `0xffffffb5` = **−75** no `negamax` (`439796`).

**`credita`** aparece com o bónus `1700` cravado em três sítios (`438d35`,
`43cb14`, `43db23`) — uma constante de bónus sem manete, que não estava no
inventário porque não vive na estrutura `Parametros`.


---

# `negamax`, secção 8: a `Lista` e a escolha do lance seguinte

## A `Lista` vive na pilha, e confirma a planta tirada da `pontua`

```
43b703:  lea 0x5c0(%rsp),%rbp        ; base da Lista
43b6b2:  mov %r8w,0x5c0(%rsp,%r15,2) ; lances, uint16
43b6eb:  mov %edx,0xfc0(%rsp)        ; contagem
43b714:  mov %rbp,%rdx               ; passada à `pontua`
43b717:  call 410f20
```

`0xfc0 − 0x5c0 = 0xa00`. A contagem está no deslocamento `+0xa00` da base — que
é exactamente o que a `pontua` lê em `410f20` (`mov 0xa00(%rdx),%eax`). Duas
leituras independentes, a mesma planta:

| desl. | conteúdo | tamanho |
|---|---|---|
| `+0x000` | lances, `uint16` | 256 × 2 = `0x200` |
| `+0x200` | nota, `int32` | 256 × 4 = `0x400` |
| `+0x600` | segunda nota, `int32` | 256 × 4 = `0x400` |
| `+0xa00` | contagem, `int32` | |

**O tecto é 256 lances**, cravado em `43b6df` (`cmp $0x100,%edx`). A `Lista`
ocupa `0xa04` bytes de pilha e é por isso que o `negamax` reserva `0x1000`.

## A ordenação é por selecção, lance a lance

```
43b7d0:  mov   0x280(%rbx),%rax        ; nós, guardado antes de cada lance
43b7e8:  cmp   %r15d,%ecx
43b7eb:  jle   43ba11                  ; acabaram os lances
43b7fa:  mov   0x200(%rbp,%r10,4),%r12d ; nota do candidato
43b80c:  and   $0x7,%r8d               ; entrada do desenrolamento
43b810:  cmp   %edi,%r12d
43b813:  cmovg %r10d,%r11d             ; guarda o índice do melhor
43b817:  cmovle %edi,%r12d             ; e a nota
```

Não há `sort`. A cada volta varre o que resta do vector de notas à procura do
máximo, com `cmov` e desenrolado 8× — **selecção incremental**. O lance
escolhido é trocado e o ciclo avança.

Isto importa para a reconstrução por duas razões:

1. **A ordem de empate é a ordem de geração.** Uma selecção com `cmovg` só troca
   quando é **estritamente maior**; dois lances com a mesma nota ficam pela
   ordem em que o gerador os produziu. Um `std::sort` não garante isso, e um
   `std::stable_sort` garante outra coisa. A árvore muda.

2. **A varredura é sobre `+0x200`**, a primeira nota. A segunda, em `+0x600`, é
   escrita pela `pontua` mas **não participa na escolha** — ao menos aqui.

O `arranca` usa outra coisa: a tabela de símbolos mostra lá
`__inplace_stable_sort` e `__merge_adaptive` sobre `pair<Move,int>` com um
comparador lambda. **A raiz ordena de forma estável e o interior por selecção.**
São dois mecanismos diferentes no mesmo motor.

## O contador de nós é guardado por lance

```
43b7d0:  mov %rax,0x28(%rsp)   ; nós antes de jogar este lance
```

`this+0x280` é lido e guardado na pilha a cada volta do ciclo. É a contagem por
lance da raiz — o que permite saber quantos nós custou cada lance. No `arranca`
isto é o que alimenta a ordenação estável da iteração seguinte.

## Por ler neste ciclo

O que acontece depois da escolha: o filtro de legalidade (`40efe0`), as podas
por lance (LMP, futilidade, SEE), a chamada à `reducao`, a busca com janela
reduzida e a re-busca. Ficam para a secção seguinte.


---

# `negamax`, secção 9: aplicar a redução, buscar e re-buscar — `438b6a`…`438c7c`

O que acontece ao número que a `reducao` devolve.

```
438b6a:  add    $0x50,%rsp              ; limpa os 9 argumentos
438b74:  mov    0x160(%rbp),%edi        ; ext / lmr_ext_max = 0
438b7a:  lea    0x3ff(%rax),%r9d
438b8a:  neg    %edi                    ; -lmr_ext_max
438b99:  cmovs  %r9d,%eax               ; arredonda para zero
438b9d:  sar    $0xa,%eax               ; r /= 1024
438ba0:  cmp    %eax,%edi
438ba2:  cmovge %edi,%eax               ; r = max(r, -lmr_ext_max)
438ba7:  test   %r11d,%r11d             ; r11 = prof - 1
438bad:  cmovs  %edx,%r11d              ; max(prof-1, 0)
438bb6:  cmp    %r11d,%eax
438bb9:  cmovle %eax,%r11d              ; r = min(r, prof-1)
438bd0:  sub    %r11d,%edx              ; nova_prof = (prof-1) - r
```

```cpp
int r = reducao(...) / 1024;                     // arredondado para zero
r = std::max(r, -lmr_ext_max);                   // 0x160, vale 0 → r >= 0
r = std::min(r, std::max(prof - 1, 0));          // nunca passa da profundidade
const int nova_prof = (prof - 1) - r;
```

**A `reducao` devolve em milésimos de ply.** O valor cru é dividido por 1024
aqui, no chamador — dentro da `reducao` nunca é. Isso explica as escalas lá
dentro: `lmr_piora_f = 197` sobre 512, `lmr_nonpv_f = 1024`, o `clamp` a
`±2048`. Tudo em 1/1024 de ply.

**`lmr_ext_max = 0` transforma o `cmovge` num chão a zero.** Com a manete
ligada, `r` poderia ficar **negativo** — uma *extensão*, não uma redução. O
nome diz isso mesmo, e está desligado. É mais um caso do modo 2: sem salto,
só um `cmov`.

## A busca reduzida e a re-busca

```
438bd3:  push  $0x1                ; a7 = 1
438bd5:  push  $0x0                ; a6 = 0
438bed:  call  436690              ; negamax(nova_prof, janela invertida)
438bf6:  mov   %eax,%r12d
438c07:  neg   %r12d
438c0a:  cmp   %r10d,%r12d         ; contra alpha
438c0d:  jle   438eba              ; não melhorou -> lance seguinte
438c13:  mov   0x88(%rsp),%r8d     ; a redução que foi aplicada
438c22:  test  %r8d,%r8d
438c25:  jle   4391ee              ; r <= 0 -> não há nada a repetir
438c66:  push  %rdi                ; a7 = variável
438c6a:  push  $0x0                ; a6 = 0
438c7c:  call  436690              ; re-busca
```

```cpp
int v = -negamax(pos, nova_prof, -alpha-1, -alpha, ply+1, false, true);
if (v > alpha && r > 0)
    v = -negamax(pos, prof-1, ..., false, a7_variavel);   // re-busca
```

**A condição da re-busca é `r > 0`, e só isso.** Não há segunda condição sobre
a janela nem sobre o tipo de nó neste ponto — se a redução foi zero, o resultado
da primeira busca já é o definitivo e não se repete.

## Os booleanos, agora com significado

Juntando às combinações da secção 6:

| chamada | `a6` | `a7` |
|---|---|---|
| busca reduzida (`438bed`) | `0` | **`1`** |
| re-busca (`438c7c`) | `0` | variável |
| verificação singular (`4392be`) | **`1`** | `0` |
| raiz, do `arranca` | — | `0` |

`a7 = 1` na busca reduzida e `a7 = 0` na raiz e no singular: o comportamento de
`a7` é o de um *cut node*. `a6` vale 1 **só** no singular.

Um bloco do `negamax` guardado por `a6` pertence ao singular; um guardado por
`a7` distingue nó de corte de nó de PV. São duas perguntas diferentes, e a
reconstrução tem de lhes dar nomes diferentes — um único `bool` a fazer as duas
coisas muda a árvore em dois sítios ao mesmo tempo.


---

# O segundo mecanismo de interruptores: `DIAG`

Havia 122 cadeias `KS_*` no binário. A primeira extracção resolveu 108 para
deslocamentos da `Parametros` e deixou catorze por explicar. **Estas catorze não
são parâmetros numéricos — são bandeiras de presença**, e vivem noutro sítio.

## Como funcionam

```
409a3e:  test   %rax,%rax
409a41:  setne  0x71a18c2(%rip)   # 75ab30a <DIAG+0xa>
409a48:  call   53ce30 <getenv>
```

`getenv` → `test` → **`setne`**, não `mov`. O que fica guardado é *se a variável
existe*, não o seu valor. `DIAG` é um vector de bytes em `.bss`, portanto todas
nascem a **zero**.

Foi por isso que a minha primeira varredura as perdeu: procurava
`mov %eax,DESL(%reg)` e estas escrevem-se com `setne` sobre um endereço
absoluto.

## As catorze

| `DIAG` | variável | usos | o que troca |
|---|---|---:|---|
| `+0x00` | `KS_SEM_PRE` | 2 | |
| `+0x01` | **`KS_BONUS_ANTIGO`** | **36** | a fórmula do bónus do histórico |
| `+0x02` | `KS_SEM_CAPT_PEN` | 3 | penalização das capturas |
| `+0x03` | `KS_ORDEM_ESCALADA` | 3 | escala da ordenação |
| `+0x04` | `KS_SING_SEM_CORTE` | 3 | singular sem corte |
| `+0x05` | `KS_SING_CONTA` | 4 | contagem do singular |
| `+0x06` | `KS_OFF_NULL` | 3 | desliga o lance nulo |
| `+0x07` | `KS_OFF_IIR` | 5 | desliga o IIR |
| `+0x08` | `KS_SEM_NMP_PROF` | 3 | termo de profundidade do nulo |
| `+0x09` | `KS_NMP_TODOS` | 3 | |
| `+0x0a` | `KS_FORMA` | 14 | contadores da forma da árvore |
| `+0x0b` | `KS_MARGEM_ESTUDO` | 4 | |
| `+0x0c` | `KS_REB` | 3 | |
| `+0x0d` | `KS_EVAL_CRUA` | 6 | avaliação crua |

## O caso que apareceu primeiro, e é o mais importante

No corte por beta, o bónus dado à história dos lances quietos:

```
43a337:  cmpb $0x0,DIAG+1
43a33e:  je   43b122              ; POR OMISSAO salta para aqui
43a344:  imul $0xa0,0x4(%rsp),%ebx ; 160 * prof     <- caminho do KS_BONUS_ANTIGO
43a351:  sub  $0x64,%ebx           ; - 100
43a356:  cmovg → min(·, 1700)
```

```
43b122:  imul $0xc8,0x4(%rsp),%ebx ; 200 * prof     <- caminho POR OMISSAO
43b136:  cmovle → max(·, 1)
43b13b:  cmovg  → min(·, 4000)
```

```cpp
int bonus = KS_BONUS_ANTIGO ? std::min(160*prof - 100, 1700)
                            : std::clamp(200*prof, 1, 4000);
```

**As duas fórmulas estão inteiras no binário.** A que o motor usa é a segunda;
a primeira é a versão antiga, guardada atrás da bandeira — o nome diz-lo.

E é aqui que o `1700` da varredura dos argumentos constantes se explica: não é
uma constante de bónus normal, é o tecto da fórmula **antiga**, que nunca corre.

## O que isto muda no inventário

Passa a haver **duas** famílias de interruptor, não uma:

| família | quantos | onde | omissão | o que faz |
|---|---:|---|---|---|
| numéricos | 108 | `Parametros`, `this+0x10`…`0x23f` | do molde | afinam valores |
| **presença** | **14** | `DIAG`, bytes em `.bss` | **zero** | **trocam fórmulas** |

E acrescenta uma **quinta maneira** de o código ficar inerte, a juntar às
quatro já catalogadas:

| modo | como se detecta |
|---|---|
| 1 | salto sobre manete a zero |
| 2 | `cmov` sobre manete a zero |
| 3 | termo multiplicado por zero |
| 4 | argumento constante no chamador |
| **5** | **bandeira `DIAG` a zero — troca o bloco todo por outro** |

O modo 5 é diferente dos outros quatro: não *desliga* código, **substitui-o**.
Os dois caminhos são reais e ambos têm de ser reconstruídos, com a condição
certa entre eles. Uma reconstrução que veja só o bloco em `43a344` — que é o que
aparece primeiro ao ler por endereços — escreve a fórmula errada e não dá por
nada, porque a outra está a duzentos bytes de distância.

`KS_BONUS_ANTIGO` sozinha governa **36 sítios**. É a maior bifurcação isolada
encontrada até agora no motor.


---

# As fórmulas de bónus, e as duas versões de cada uma

`KS_BONUS_ANTIGO` governa **33 sítios** — verificados um a um. Não desliga
código: em cada sítio há **duas fórmulas completas** e a bandeira escolhe.
Ambas têm de ser reconstruídas.

## `bonus_capt(int)` — `0x40a970`, 64 bytes, inteira

A função mais pequena do motor, e é toda uma bifurcação:

```
40a970:  cmpb  $0x0,DIAG+1
40a977:  je    40a990                  ; POR OMISSAO

40a979:  lea   (%rdi,%rdi,4),%eax      ; prof * 5
40a981:  shl   $0x5,%eax               ;        * 32   = 160 * prof
40a984:  sub   $0x64,%eax              ; - 100
40a987:  cmp   $0x6a4 / cmovg          ; min(·, 1700)
40a98c:  ret

40a990:  imul  $0x2a8,%edi,%eax        ; 680 * prof
40a99b:  sub   $0xfa,%eax              ; - 250
40a9a0:  cmp   $0x960 / cmovg          ; min(·, 2400)
40a9a7:  test  / cmovs                 ; max(·, 0)
40a9ac:  ret
```

```cpp
int bonus_capt(int prof)
{
    if (KS_BONUS_ANTIGO)
        return std::min(160*prof - 100, 1700);          // antiga
    return std::clamp(680*prof - 250, 0, 2400);         // em uso
}
```

**Seis constantes, nenhuma com manete**: 160, 100, 1700 na antiga; 680, 250,
2400 na nova. E a nova tem um chão a zero que a antiga não tem — a antiga pode
devolver negativo para `prof = 0`.

## O bónus dos lances quietos — `43a337` / `43b122`

```cpp
int bonus_quieto(int prof)
{
    if (KS_BONUS_ANTIGO)
        return std::min(160*prof - 100, 1700);          // igual à antiga acima
    return std::clamp(200*prof, 1, 4000);               // em uso
}
```

Na versão antiga as capturas e os quietos partilhavam **a mesma** fórmula.
Na nova separaram-se: `680·prof − 250` até 2400 para capturas, `200·prof` até
4000 para quietos. A separação é a mudança que a bandeira guarda.

| | antiga (bandeira ligada) | em uso (omissão) |
|---|---|---|
| capturas | `min(160·prof − 100, 1700)` | `clamp(680·prof − 250, 0, 2400)` |
| quietos | `min(160·prof − 100, 1700)` | `clamp(200·prof, 1, 4000)` |

## Os outros 31 sítios: escolha por `cmovne`

A maioria não usa salto. O padrão é:

```
43a80c:  cmpb   $0x0,DIAG+1
43a81b:  cmovne %ebx,%ecx        ; bandeira ligada -> usa o outro valor
43a81e:  neg    %ecx
43a820:  call   credita_captura
```

Dois valores calculados antes, e um `cmov` escolhe. **Sem ramo para notar.**
São os modos 2 e 5 do inventário a agir juntos: uma bandeira que nasce a zero,
lida por um movimento condicional.

Os 31 sítios estão em dois blocos contíguos — `43a80c`…`43ab71` no `negamax`
principal e `43dee6`…`43e21b` no clone — e todos alimentam chamadas a `credita`
ou `credita_captura`. São as penalizações dadas aos lances que **não**
provocaram o corte, com duas tabelas de valores em paralelo.

## Consequência para a reconstrução

Um motor reconstruído a partir só do caminho por omissão fica **correcto** —
é o que o binário corre. Mas fica sem as 33 alternativas, e portanto sem poder
responder à pergunta *"e se voltássemos ao bónus antigo?"*, que é exactamente o
género de pergunta que o `KS_BONUS_ANTIGO` existia para responder.

Como o objectivo declarado é recuperar **cada linha**, esteja ou não activa, os
dois lados de cada uma das 33 bifurcações contam.


---

# `negamax`, secção 10: sonda da TT, IIR e geração dos lances

## A `Entrada` da TT ocupa a pilha em `0x120(%rsp)`

```
4367b1:  movq  $0x0,0x120(%rsp)      ; limpa a Entrada
4367bd:  movb  $0x0,0x134(%rsp)
4367c5:  mov   %r8w,0x136(%rsp)
4367d7:  lea   0x120(%rsp),%rdx      ; &entrada
4367df:  call  420790 <TT::sonda>
4367e4:  mov   %al,0xc0(%rsp)        ; acertou?
4367eb:  test  %al,%al
4367ed:  je    437930                ; falhou -> salta
4367f3:  movzwl 0x132(%rsp),%r12d    ; +0x12 = o lance
4367fc:  movzbl 0x130(%rsp),%edi     ; +0x10 = o Limite
```

A `Entrada` desdobrada, a partir dos deslocamentos usados:

| desl. na `Entrada` | conteúdo | visto em |
|---|---|---|
| `+0x00` | zerado antes da sonda | `4367b1` |
| `+0x08` | profundidade | `0x128(%rsp)`, `439237` |
| `+0x10` | **`Limite`** | `436804`, `4379e3`, `439229` |
| `+0x12` | **o lance** | `4367f3`, `4379bf` |
| `+0x14` | nota | `0x134(%rsp)` |
| `+0x16` | avaliação | `0x136(%rsp)` |

A bandeira do acerto fica em `0xc0(%rsp)` — a mesma que a secção 4 viu como
condição de entrada do singular (`439212`).

**A sonda só acontece se `rule50 <= 13`** (`4367ce: cmp $0xd,%eax / jg 436dd0`).
Acima disso vai por outro caminho, o que amassa a chave — a quinta aparição do
mesmo auxiliar PCG.

## IIR — `4379aa`…`4379c8`

```
4379aa:  cmpb $0x0,DIAG+7          ; KS_OFF_IIR
4379b1:  jne  4379d0               ; ligada -> salta o IIR
4379b7:  cmp  0x1a8(%rbp),%ebx     ; iir_prof = 4
4379bd:  jl   4379d0               ; prof < 4 -> sem IIR
4379bf:  cmpw $0x0,0xa6(%rsp)      ; lance da TT
4379c8:  je   439754               ; sem lance -> reduz
```

```cpp
if (!KS_OFF_IIR && prof >= iir_prof && lance_tt == 0)
    goto reduzir_por_iir;          // 439754
```

`KS_OFF_IIR` é a sexta bandeira `DIAG`, e aqui desliga o IIR inteiro. Confirma
o padrão: as bandeiras com `OFF_` no nome desligam, as outras trocam.

Logo antes, três campos são postos a `0xffff8000` = **−32768**:

```
437988:  movl $0xffff8000,0x29770(%r12)   ; this+0x29770 + ply*4
437994:  movl $0xffff8000,0xe0(%rsp)
43799f:  movl $0xffff8000,0xec(%rsp)
```

**Campo novo:** `this+0x29770`, inteiro por ply, iniciado a −32768 em cada nó.
É o valor "sem avaliação" — o mesmo `-32768` que o ProbCut testa em `437193`.

## `pcp_m` — inerte — `4379d0`

```
4379d0:  mov  0x1c8(%rbp),%r14d    ; pcp_m = 0
4379da:  jle  4379f1               ; salta
4379dc:  cmpb $0x0,0x20(%rsp)
4379e3:  cmpb $0x1,0x130(%rsp)     ; Limite == 1
4379eb:  je   4396f3
```

Outra vez o enumerado `Limite`: aqui o valor **1** é o que interessa. Já se
conhecem três significados distintos — `1` aqui, `2` no ramo da `pontua` que lê
`tt_sup`, `3` o que a quiescência passa e o que exclui o singular.

## A geração dos lances — `4379f1`…`437a46`

```
4379f1:  lea   0x400(%rsp),%rbx        ; o buffer bruto
437a15:  call  40f3c0 <generate<GenType 4>>
437a1f:  cmp   %rbx,%rax
437a22:  je    438eec                  ; nenhum lance -> mate ou empate
437a2b:  movzwl (%r12),%r8d            ; copia para a Lista
437a38:  mov   %r8w,0x610(%rsp,%rax,2)
```

`generate<GenType 4>` escreve num buffer em `0x400(%rsp)` e os lances são
**copiados** um a um para a `Lista` em `0x610(%rsp)`. Dois vectores distintos:
o gerador não escreve directamente na `Lista`.

`GenType 4` é o mesmo que a `quiescencia` usa em `420f65`. O `GenType 0` aparece
só na quiescência (`420b7c`). São dois geradores e três sítios.


---

# `Busca::arranca`: a tabela LMR, construída em vírgula flutuante

`arranca` tem 32.541 bytes, mas **a maior parte é impressão**: 171 chamadas a
`__ostream_insert`, mais `_M_insert<double>`, `<unsigned long>` e `operator<<(int)`.
Mais 105 `getenv` e 97 `strtol` — o bloco das manetes. A lógica de busca é uma
fracção pequena do tamanho.

## A fórmula da tabela `[64][64]`

A tabela que a `reducao` lê em `this+0x3b0` é construída aqui, uma vez por
busca, com `log` em `double`:

```
43fc3d:  vcvtsi2sdl 0x134(%r13),%xmm0      ; (double) lmr_div  = 236
43fc46:  vdivsd     [0x608fd0],%xmm0,%xmm2 ; / 100.0
43fc4e:  vmaxsd     %xmm2,%xmm1,%xmm3      ; max(·, 0.01)
43fc52:  vcvtsi2sdl 0x130(%r13),%xmm14     ; (double) lmr_base = 77
43fc62:  vdivsd     [0x608fd0],%xmm14,%xmm15 ; / 100.0

  43fca1:  call __log          ; log(i)
  43fcb6:  call __log          ; log(j)
  43fcbb:  vmulsd              ; log(i) * log(j)
  43fcc8:  vdivsd              ; / max(lmr_div/100, 0.01)
  43fcd0:  vaddsd              ; + lmr_base/100
  43fcd8:  vmulsd [0x608fd8]   ; * 1024.0
  43fce0:  vcvttsd2si          ; truncar para int
  43fce5:  mov %r9d,0x3b0(%r12,%rbx,4)
```

```cpp
const double div  = std::max(lmr_div  / 100.0, 0.01);   // 0x134 = 236 -> 2.36
const double base =          lmr_base / 100.0;          // 0x130 =  77 -> 0.77

for (int i = 1; i < 64; ++i)
    for (int j = 1; j < 64; ++j)
        lmr[i][j] = int( (std::log(i) * std::log(j) / div + base) * 1024.0 );
```

Três constantes em `double`, todas sem manete: **`100.0`** (o divisor comum que
transforma as manetes inteiras em fracções), **`0.01`** (o chão do divisor) e
**`1024.0`** (a escala).

## Isto fecha a escala da `reducao`

A secção 9 concluiu que a `reducao` trabalha em **milésimos de ply**, porque o
chamador divide o resultado por 1024. A construção da tabela confirma-o do outro
lado: o `* 1024.0` está aqui, na origem. Os dois números são o mesmo 1024, e a
tabela nasce já multiplicada.

Por isso é que `lmr_nonpv_f = 1024` significa "um ply inteiro" e
`lmr_piora_f = 197` sobre 512 significa "mais 38%".

## As manetes inteiras são fracções disfarçadas

`lmr_div = 236` e `lmr_base = 77` **não são 236 e 77** — são `2.36` e `0.77`.
O `/100.0` está no código, não no valor. Uma reconstrução que use os inteiros
directamente na fórmula produz uma tabela sem relação nenhuma com a original.

E são das poucas manetes com nome `LMR_` que **não** são lidas pela `reducao`:
`KS_LMR_BASE` e `KS_LMR_DIV` só aparecem aqui, no `arranca`. A `reducao` lê a
tabela já feita.

## A linha e a coluna zero

O ciclo interior começa em `ebx = 1` (`43fc84`) e o exterior em `r14 = 1`
(`43fc37`). **`lmr[0][*]` e `lmr[*][0]` nunca são escritos** — e a estrutura
está em `.bss`, portanto ficam a zero. Faz sentido (`log(0)` é indefinido), mas
tem de ficar escrito: a `reducao` limita os índices a 63 mas **não** os impede
de ser 0, e nesse caso lê um zero deixado pela inicialização.


---

# `arranca`: a janela de aspiração — `440076`…`4401fd`

## A largura inicial

```
440076:  mov  0x1d8(%r12),%eax    ; asp_delta = 25
44007e:  shl  $0x3,%eax           ; * 8
440082:  idiv %r13d               ; / prof
440085:  lea  0x5(%rax),%r15d     ; + 5
```

```cpp
int delta = asp_delta * 8 / prof + 5;      // = 200/prof + 5
```

O `8` e o `5` são constantes cravadas; só o `25` tem manete.

## Quando há janela, e quando não há

```
440089:  cmp  %r13d,0x1dc(%r12)   ; asp_prof = 4
440091:  jge  4401fd              ; asp_prof >= prof -> janela completa
44009d:  cmovs                    ; |aval|
4400a1:  cmp  $0x7c09,%r8d        ; 31753
4400a8:  jg   4401fd              ; |aval| > 31753 -> janela completa
4400b1:  lea  (%r15,%r14,1),%r14d ; beta  = aval + delta
4400b5:  sub  %r15d,%r9d          ; alpha = aval - delta
```

```cpp
if (prof > asp_prof && std::abs(aval) <= 31753) {
    alpha = aval - delta;
    beta  = aval + delta;
} else {
    alpha = -INF; beta = +INF;
}
```

O `31753` é o mesmo tecto da `corrigida` (`0x7c09`). Aparece agora em dois
sítios distantes com o mesmo papel: "isto ainda não é um mate".

## O alargamento, e o ramo inerte do `asp_sf`

```
4400d0:  mov    0x1d0(%rdi),%r13d   ; asp_sf = 0
4400da:  je     4401de              ; zero -> caminho por omissão
4400e0:  sub    %r15d,%eax
4400e6:  mov    $0xffff82ff,%ebx    ; -31969
4400f0:  cmovge %eax,%ebx           ; max(·, -31969)
4400fc:  imul   $0x55555556,%r10,%rcx
440106:  shr    $0x20,%rcx
44010c:  add    %ecx,%r15d          ; delta += delta/3
44010f:  mov    0x1d4(%rdi),%eax    ; asp_tecto = 1000
```

```cpp
delta += delta / 3;                       // cresce 4/3 a cada falha
delta = std::min(delta, asp_tecto);       // 0x1d4 = 1000
```

A magia `0x55555556` com `shr 32` é a divisão por 3 — verificada, não suposta.

**`asp_sf` está a zero e tem um caminho próprio em `4401de`.** O bloco que se lê
acima (com o chão em `−31969`) é o do `asp_sf` **ligado**; o caminho por omissão
é outro. É o modo 5 outra vez — uma manete que **troca** o esquema de
alargamento, não que o desliga.

O `−31969` (`0xffff82ff`) é mais um número próximo dos outros tectos e diferente
de todos: `31753` na `corrigida` e aqui, `31506` na `avalia_cheia`, `31507` na
gravação da TT, `−31969` neste chão. **Cinco constantes vizinhas e distintas**,
todas cravadas. Qualquer reconstrução que as uniformize num `VALOR_MATE` único
muda o comportamento em cinco sítios.

## Estado do `arranca`

| bloco | endereços | estado |
|---|---|---|
| leitura das 122 manetes | `43e6ef`…`43f960` | mapeado (secção do mapa) |
| construção da tabela LMR | `43fc3d`…`43fd??` | **fechado** |
| janela de aspiração | `440076`…`4401fd` | **fechado**, menos o ramo `asp_sf` |
| chamada à raiz | `440181` | fechado (secção 7) |
| gestão de tempo | `44085f`…`440c96` | por ler |
| impressão UCI | o resto | 171 chamadas a `ostream` |

A gestão de tempo é o que falta de substancial: `tm_queda` (250), `tm_esf_f`
(155), `tm_instab` (9), `tm_instab_max` (200), `tm_escala_min` (650),
`tm_escala_max` (5000), `tm_esf_base` (140), mais `0x21c` sem manete.


---

# `arranca`: a gestão de tempo — `44085f`…`440972`

Toda em `double`, com as manetes inteiras convertidas por divisores fixos. As
constantes de vírgula flutuante do motor são seis, e reaparecem aqui:

| endereço | valor | papel |
|---|---:|---|
| `0x608f88` | `0.01` | chão do divisor do LMR |
| `0x608f90` | `0.75` | limiar da queda |
| `0x608fd0` | `100.0` | divisor comum das manetes |
| `0x608fd8` | `1024.0` | escala do LMR |
| `0x608ff8` | `10.0` | divisor da instabilidade |
| `0x609000` | `0.9` | termo constante da instabilidade |

## A queda da avaliação

```
44085f:  vcvtsi2sdl 0x20c(%r12)      ; tm_queda = 250
44086d:  vdivsd     [100.0]          ; -> 2.50
44087a:  vminsd                      ; min(·, tecto)
44087f:  vdivsd     %xmm9,%xmm7      ; / (2 * prof)
440884:  vsubsd                      ; base - ·
440889:  vcomisd    [0.75],%xmm1
44088d:  ja         440a8c           ; < 0.75 -> outro caminho
```

`tm_queda` entra como `2.50` e o resultado é comparado com **`0.75`**.

## A instabilidade

```
4408b5:  vcvtsi2sdl 0x210(%r12)      ; tm_instab = 9
4408bf:  vdivsd     [10.0]           ; -> 0.9
4408c7:  vmulsd
4408d1:  vcvtsi2sdl 0x214(%r12)      ; tm_instab_max = 200
4408e3:  vdivsd     [100.0]          ; -> 2.00
4408eb:  vdivsd
4408f0:  vaddsd     [0.9]            ; + 0.9
4408f8:  vmaxsd                      ; max(base, ·)
4408fd:  vminsd                      ; min(2.00, ·)
```

```cpp
double f = nos_do_melhor / (prof * (tm_instab/10.0)) + 0.9;
f = std::min(tm_instab_max/100.0, std::max(base, f));
```

**`tm_instab` divide por 10, `tm_instab_max` por 100.** Dois divisores
diferentes para duas manetes vizinhas — mais uma armadilha para quem uniformize.

## O tecto e o chão da escala

```
440938:  vcvtsi2sdl 0x218(%r12)      ; tm_escala_min = 650
440942:  vcvtsi2sdl 0x208(%r12)      ; tm_escala_max = 5000
440951:  vdivsd     %xmm8            ; ambas / o mesmo divisor
44095f:  vmaxsd                      ; max(min, ·)
440964:  vminsd                      ; min(max, ·)
440969:  vmulsd     %xmm4,%xmm0      ; * tempo disponível
44096d:  vcvttsd2si
440972:  cmp        0x3a8(%r12),%r15 ; contra o limite em ms
```

O divisor de ambas (`xmm8`, vindo de `-0x350(%rbp)`) é o mesmo `max(lmr_div/100, 0.01)`
calculado para a tabela LMR — **a mesma variável reaproveitada**. Não é um valor
de tempo; é o divisor do LMR a servir de escala aqui também.

Isto é do género de coisa que só a leitura do binário revela: no fonte podem ser
duas expressões idênticas, ou pode ser mesmo a mesma variável reutilizada. O
compilador guardou-a numa posição de pilha e usa-a nos dois sítios.

## O corte por nós

```
440902:  test %r15d,%r15d            ; tm_esf_f = 155
440905:  jle  440920
440907:  mov  0x280(%r12),%r10       ; nós
44090f:  mov  0x28b80(%r12),%rax     ; limite de nós guardado
44091a:  jb   440b47
```

**Campo novo:** `this+0x28b80`, um contador de 64 bits comparado com o total de
nós. Fica ao lado dos killers (`0x28b88`) no mapa da estrutura.

## Por ler no `arranca`

O ciclo de aprofundamento iterativo propriamente dito, o `stable_sort` dos
lances da raiz, e os 171 sítios de impressão UCI. A impressão não muda a árvore,
mas faz parte do "cada linha" — e é onde estão os formatos que o bot espera.


---

# `quiescencia`, segunda passagem: o ciclo de lances fecha

## O filtro SEE — `421316`…`421325`

```
421316:  xor  %edx,%edx           ; limiar = 0
421318:  mov  %r12d,%esi          ; o lance
42131e:  call 410b40 <see_ge>
421323:  test %al,%al
421325:  je   4214a2              ; falhou -> lance seguinte
```

```cpp
if (!pos.see_ge(m, 0)) continue;
```

**O limiar é zero cravado**, não uma manete. Na quiescência qualquer captura com
SEE negativo é descartada sem margem nenhuma. Compare-se com o `negamax`, onde
o mesmo `see_ge` é chamado com `−75` (`439796`) — dois limiares diferentes, os
dois constantes.

## O que é feito antes de jogar

```
42132b:  ...                      ; amassa a chave (6.ª ocorrência do PCG)
421376:  call emplace_back        ; empilha em this+0x29e30
42137b:  mov  0x388(%r15),%r8     ; o avaliador
4213a1:  mov  %di,0x6dc9900(...)  ; prepara a fatia do acumulador
4213c1:  mov  %r10,0x6ede6c0(%r8) ; sobe o topo da pilha de acumuladores
4213cd:  mov  0x258(%r15),%r14    ; a tabela de transposição
4213d4:  call prefetch_key
4213e1:  and  0x8(%r14),%rax      ; índice = chave & máscara
4213e5:  lea  (%rax,%rax,8),%rax  ; * 9
4213e9:  prefetcht0 (%rdx,%rax,8) ; * 8  -> balde de 72 bytes
```

O `lea ·*9` seguido de `*8` dá **72 bytes por balde** — que é exactamente o que
o documento já registava da desmontagem da `sonda`: *"`Balde` = 72 bytes logo
VIAS = 3"*. Confirmação independente, a partir do cálculo do `prefetch`.

**Campo novo:** `this+0x258` é o ponteiro para a `TranspositionTable`, com a
máscara em `+0x8` e o vector de baldes em `+0x0`.

## A gravação na TT — `421519`…`42155d`

```
421552:  push  %rdx               ; último argumento
421553:  xor   %edx,%edx          ; profundidade = 0
421555:  push  $0x0               ; a penúltima, constante
421557:  movzwl 0x5a(%rsp),%r9d   ; o lance
42155d:  call  4202f0 <TT::guarda>
```

```cpp
tt.guarda(chave, nota, /*prof*/ 0, limite, lance, false, aval);
```

**A quiescência grava sempre com profundidade `0`.** É o que a distingue das
entradas do `negamax`, e o que faz com que o `cmp %r8d,%r10d` da `sonda` as
rejeite para qualquer nó de profundidade positiva.

## O tecto que faltava

```
42150e:  cmp    $0xffff83f7,%r15d
421515:  cmovge %r15d,%ecx
```

`0xffff83f7` = **−31753**, o simétrico do `0x7c09` da `corrigida` e da janela de
aspiração. Os cinco números vizinhos ficam assim:

| valor | onde |
|---:|---|
| `31753` / `−31753` | `corrigida`, aspiração, quiescência |
| `31506` / `−31506` | `avalia_cheia` |
| `31507` | gravação da TT à profundidade máxima |
| `−31969` | chão do alargamento da aspiração |
| `−32768` | "sem avaliação" (`this+0x29770`) |

**Seis constantes distintas**, todas cravadas, nenhuma com manete. Uniformizá-las
num só `VALOR_MATE` muda o comportamento em seis sítios independentes.

## A `quiescencia` fica fechada

| bloco | estado |
|---|---|
| entrada (nós, profundidade máxima, relógio, 50 lances, repetição) | ✅ |
| sonda da TT | ✅ |
| avaliação e `otimismo` | ✅ |
| `generate<0>` e `generate<4>`, com `pontua(Limite=3)` | ✅ |
| ramo inerte do `qs_recapt` | ✅ |
| filtro SEE com limiar 0 | ✅ |
| `do_move` / recursão / `undo_move` | ✅ |
| gravação na TT com profundidade 0 | ✅ |


---

# Correcção: `0xffff82ff` é **−32001**, não −31969

Na secção da janela de aspiração escrevi que `0xffff82ff` vale `−31969`, e
repeti o erro na lista das "seis constantes de mate". **Está errado.** Converti
mal à mão.

Verificado:

| hexadecimal | com sinal |
|---|---:|
| `0x7d01` | **32001** |
| `0xffff82ff` | **−32001** |
| `0x7c09` | 31753 |
| `0xffff83f7` | −31753 |
| `0x7b12` | 31506 |
| `0xffff84ee` | −31506 |
| `0x7b13` | 31507 |
| `0xffff8000` | −32768 |

## A tabela corrigida

Os limites do motor são **simétricos**, ao contrário do que eu tinha escrito:

| par | valor | onde |
|---|---:|---|
| infinito | `±32001` | janela de aspiração (`4401ad`, `4401ee`, `4401fd`) |
| tecto da avaliação corrigida | `±31753` | `corrigida`, aspiração, quiescência |
| tecto da avaliação NNUE | `±31506` | `avalia_cheia` |
| nota da profundidade máxima | `31507` | gravação da TT (`439bc2`) |
| "sem avaliação" | `−32768` | `this+0x29770`, ProbCut |

Continuam a ser **cinco escalas distintas** e continua a valer o aviso — quem
as uniformize num `VALOR_MATE` único muda cinco coisas ao mesmo tempo. Mas a
assimetria que eu tinha apontado não existe: o infinito é `±32001`, certinho.

O `31507` é que é mesmo ímpar: é `31506 + 1`, um a mais do que o tecto da
avaliação, usado só na entrada de profundidade máxima.

## Porque é que isto aconteceu

Converti `0xffff82ff` de cabeça em vez de o calcular. O valor certo sai de
`−(0x7d00 + 1)`, e eu li o `0x82ff` como se fosse um complemento a 16 bits.

Fica registado porque é o segundo erro do mesmo tipo neste trabalho — o
primeiro foi o regexp que excluía a forma de leitura — e os dois têm a mesma
origem: **um passo feito à mão no meio de um trabalho que é todo mecânico.**
O resto das constantes deste documento saiu de `python3` ou de `objdump`; estas
duas saíram da minha cabeça, e as duas estavam erradas.

Para o que falta: nenhuma constante entra no documento sem ser convertida por
ferramenta.

## O ciclo de re-busca da raiz, que é onde isto apareceu

```
440190:  cmpb $0x0,0x390(%rdi)   ; parar?
440199:  cmp  %eax,%ebx          ; resultado <= alpha ?
44019b:  jge  4400d0             ;   -> falhou em baixo, alarga
4401a1:  cmp  %eax,%r14d         ; resultado >= beta ?
4401a4:  jg   440720             ;   -> falhou em cima
4401ad:  mov  $0x7d01,%r14d      ; beta  = +32001
4401c0:  add  $0x1,%r13d         ; conta as falhas
4401cc:  sar  $1,%ecx            ; ... e reduz
```

```cpp
for (;;) {
    v = negamax_raiz(alpha, beta, ...);
    if (parar) break;
    if (v <= alpha) { beta = (alpha + beta) / 2; alpha = max(v - delta, -32001); }
    else if (v >= beta) { beta = min(v + delta, 32001); }
    else break;
    delta += delta / 3;   // até asp_tecto
}
```

O `beta = (alpha + beta) / 2` na falha em baixo lê-se em `4401de`…`4401f3`
(`add %r14d,%ebx` seguido de `sar $1`). É o passo que aproxima a janela por cima
quando a busca falha por baixo — e não estava registado em lado nenhum.


---

# As constantes cravadas, varridas por ferramenta

Depois do erro de conversão à mão, varri **todas** as constantes imediatas das
funções da `Busca`, convertidas com sinal por `python3`. Isto fecha o inventário
dos números do motor, que até aqui estava em três listas separadas:

1. 108 manetes numéricas, no molde `0x6155f0` / `0x609c80`
2. 49 campos do molde **sem** manete
3. 14 bandeiras de presença em `DIAG`

Falta a quarta: **os imediatos**, que não vivem na estrutura nenhuma.

## As que aparecem em três ou mais funções

| valor | funções |
|---:|---|
| `63` | `credita`, `credita_captura`, `garante_ameacas`, `hist_de`, … |
| `49152` (`0xc000`) | `credita_captura`, `hist_de`, `negamax`, `pontua` |
| `32768` (`0x8000`) | `credita_captura`, `negamax`, `pontua`, `quiescencia` |
| `16384` (`0x4000`) | `credita_captura`, `hist_de`, `negamax`, `pontua` |
| `31753` / `−31753` | `arranca`, `corrigida`, `negamax`, `quiescencia` |
| `−32768` | `arranca`, `negamax`, `quiescencia` |
| `−32001` | `arranca`, `negamax`, `quiescencia` |
| `245` | `negamax`, `quiescencia` |
| `99` | `arranca`, `negamax`, `quiescencia` |
| `208` | `negamax`, `pontua` |

`63`, `49152`, `32768`, `16384` são máscaras do lance e da casa — estruturais,
não afináveis. As outras são decisões.

## As grandes, por número de sítios

| valor | sítios | onde nasce |
|---:|---:|---|
| `255` | 32 | `quiescencia` — máscara |
| `100` | 11 | `negamax` — divisor da futilidade e do `lmr_ext_amort` |
| **`1700`** | 8 | `negamax` — tecto do bónus **antigo** (morto) |
| **`4000`** | 7 | `negamax` — tecto do bónus em uso |
| **`200`** | 6 | `negamax` — factor do bónus em uso |
| `30000` / `−30000` | 5 | `credita` — tecto das continuações |
| `208` | 4 | `pontua`, `negamax` — limiar dinâmico do SEE |
| **`160`** | 4 | `negamax` — factor do bónus antigo (morto) |
| `1000` | 4 | `arranca` |
| `15000` / `−15000` | 3 | `credita` — tecto dos quietos |
| `245` | 3 | `quiescencia` — ply máximo |
| `1600` | 2 | `pontua` — base da promoção |
| `41696` | 2 | `pontua` |

## O que isto acrescenta

**Metade das constantes de bónus pertence a código morto.** `1700` em oito
sítios e `160` em quatro são da fórmula que o `KS_BONUS_ANTIGO` guarda e que
nunca corre. Um inventário que conte "constantes do motor" sem separar os dois
lados da bandeira sobreavalia o que há a afinar.

**`208` liga a `pontua` ao `negamax`.** É o factor do limiar dinâmico do SEE
(`*208/100` = `*2.08`), e aparece nas duas funções com o mesmo papel. Não é
coincidência: é a mesma expressão escrita duas vezes no fonte, ou uma função
pequena que o compilador embutiu nas duas.

**`41696` na `pontua`** ainda não tem explicação. Fica assinalado.

## Regra de método, a partir de agora

Nenhum número entra neste documento sem passar por `python3` ou `objdump`. Os
dois erros que tive até agora — o regexp que excluía a forma de leitura, e o
`0xffff82ff` lido como `−31969` — vieram os dois de um passo manual no meio de
um trabalho mecânico. A desmontagem não perdoa aritmética de cabeça.


---

# `negamax`, secção 11: o corpo da busca singular — `439cc0`…`439e5f`

## O ramo inerte do `sing_lower`

```
439cc0:  cmp  $0x1,%r11b
439cc4:  je   439ce6              ; já é o caso 1 -> segue
439cc6:  mov  0x8c(%rbp),%r9d     ; sing_lower = 0
439cd0:  jne  438a2e              ; ligada -> outro caminho
439cd6:  sub  $0x1,%edi           ; prof - 1
439ce0:  jne  43827c              ; e sai
```

Com `sing_lower = 0` o singular só acontece no caso `r11 == 1`. A manete
abriria um segundo caso. **O caminho existe; o destino (`438a2e`) é o ciclo de
lances normal.**

## As guardas de mate

```
439ce6:  mov  0x12c(%rsp),%r12d   ; a nota da TT
439cee:  cmp  $0x7c09,%r12d       ; > 31753
439cf5:  jg   43b119              ; -> fora
439cfb:  cmp  $0xffff83f7,%r12d   ; < -31753
439d02:  jl   43b0ed              ; -> fora
```

O singular não corre sobre notas de mate, nos dois sentidos.

## A margem e a profundidade da busca de verificação

```
439d18:  mov  $0x2,%ecx
439d25:  mov  0x84(%rbp),%r11d    ; sing_margem = 2
439d38:  imul %r10d,%r11d         ; sing_margem * prof
439d3c:  sub  $0x1,%r10d          ; prof - 1
439d61:  idiv %ecx                ; (prof-1) / 2
439d7a:  sub  %r11d,%r8d          ; nota_tt - sing_margem*prof
439d80:  lea  -0x1(%r8),%esi      ; ... - 1
439d93:  call 436690              ; negamax(prof/2, janela nula)
```

```cpp
const int novo_beta = nota_tt - sing_margem * prof;    // 0x84 = 2
const int v = negamax(pos, (prof-1)/2, novo_beta - 1, novo_beta, ply, ...);
```

**A profundidade é `(prof−1)/2` e a margem é `2 * prof`.** O `2` do divisor é
cravado; o `2` da margem tem manete (`KS_SING_MARGEM`). Dois dois diferentes.

## O lance excluído

```
439d51:  add  $0x14e10,%r9              ; r9 = ply + 0x14e10
439d6b:  mov  %r14w,0x1e(%rbp,%r9,2)    ; escreve o lance
...
439db9:  movw $0x0,0x1e(%rbp,%r8,2)     ; limpa depois
```

Endereço efectivo: `this + 0x1e + (ply + 0x14e10)*2` = `this + 0x29c3e + ply*2`.
**É exactamente o campo que a secção 1 encontrou no ProbCut** (`437160`,
`cmpw $0x0,0x29c3e(%rbp,%r14,2)`). Confirma-se por dois usos independentes: é o
lance excluído do nó, posto pelo singular e lido pelo ProbCut.

## Os contadores

```
439daf:  add  0x2a8(%rbp),%r9     ; nós antes
439dc4:  addq $0x1,0x2a0(%rbp)    ; contagem de buscas singulares
439dd3:  mov  %r9,0x2a8(%rbp)     ; nós gastos em singular
439e04:  addq $0x1,0x2b0(%rbp)    ; singulares que confirmaram
439e4c:  addq $0x1,0x2b8(%rbp)    ; extensões duplas
```

Cinco campos novos para o mapa, todos de 64 bits:

| desl. | conteúdo |
|---|---|
| `0x2a0` | quantas buscas singulares |
| `0x2a8` | nós gastos nelas |
| `0x2b0` | quantas confirmaram |
| `0x2b8` | quantas deram extensão dupla |

## A extensão, simples e dupla

```
439df2:  cmp  %r11d,%esi
439df5:  jle  43b263              ; não é singular -> nada
439e0c:  cmpl $0x0,0x9c(%rbp)     ; ext2_exacto
439e13:  setne %r8b
439e1b:  or   %r9b,%r8b
439e1e:  je   43b253
439e32:  sub  0x88(%rbp),%esi     ; - ext_dupla (40)
439e38:  movl $0x1,0x80(%rsp)     ; extensão = 1
439e43:  cmp  %r11d,%esi
439e46:  jle  438287
439e54:  movl $0x2,0x80(%rsp)     ; extensão = 2
```

```cpp
if (v < novo_beta) {                       // é singular
    ext = 1;
    if ((ext2_exacto || ...) && v < novo_beta - ext_dupla)   // 0x88 = 40
        ext = 2;
}
```

**A extensão dupla exige uma segunda margem de 40** abaixo da primeira. E é
guardada por `ext2_exacto` (`0x9c`), que vale **0** — mas aqui está num `or`
com outra condição, portanto o zero **não** mata o ramo: basta a outra ser
verdadeira. É um caso em que uma manete a zero *não* desliga nada, ao contrário
do padrão.

## O `cuckoo` — `439e82`…`439ec0`

```
439e82:  lea  cuckooMove(%rip),%rcx
439e89:  movzwl (%rcx,%r9,2),%ecx
439e93:  shr  $0x6,%si / and $0x3f     ; origem e destino
439eab:  shl  %cl,%r9                  ; bit da casa
439eb1:  lea  BetweenBB(%rip),%r9
439eb8:  xor  (%r9,%rsi,8),%rcx
439ebc:  and  0x40(%r15),%rcx          ; contra a ocupação
439ec0:  jne  436b91                   ; há peça no meio -> não conta
```

Detecção de repetição por ciclo, com as tabelas `cuckooMove` (`0x7594cc0`) e
`BetweenBB` (`0x75d6ea0`). Guardada pela manete `cuckoo` (`0x19c`), que vale
**1** — activa — e pela `cuckoo_seminc` (`0x58`), que vale **0**.


---

# `negamax`, secção 12: o corpo do ProbCut — `4371a4`…`4373e9`

O bloco inteiro está atrás de `pc > 0` (`437153`), e `pc` vale **0**. Nunca
corre. Está aqui por inteiro porque o objectivo é recuperar cada linha.

## As condições de entrada, todas

```
4371a4:  cmpb $0x0,0xc0(%rsp)     ; a sonda acertou?
4371ac:  je   439ef5              ; não -> fora
4371b2:  cmpb $0x3,0x130(%rsp)    ; Limite == 3 ?
4371ba:  je   439ef5              ; -> fora
4371c8:  cmp  $0x7c09,%r12d       ; nota_tt > 31753
4371cf:  jg   43ad58
4371d5:  cmp  $0xffff83f7,%r12d   ; nota_tt < -31753
4371dc:  jge  4371e1
4371de:  add  %r13d,%r12d         ; nota_tt += ply   (ajuste de mate)
4371e1:  mov  0xe0(%rbp),%r10d    ; pc_m = 205
4371ed:  lea  (%r11,%r10,1),%edi  ; beta + pc_m
4371f4:  jg   4379d0              ; beta+pc_m > nota_tt -> fora
437211:  jle  4379d0              ; beta+pc_m <= aval -> fora
```

```cpp
if (!acertou_tt) goto fora;
if (limite_tt == 3) goto fora;                      // o mesmo 3 de sempre
int n = nota_tt;
if (n < -31753) n += ply;                           // desfaz o ajuste de mate
const int limiar = beta + pc_m;                     // 0x e0 = 205
if (limiar > n) goto fora;
if (limiar <= aval) goto fora;
++contador_probcut;                                 // this+0x2c8
```

**`Limite == 3` outra vez.** É o quarto sítio onde o valor 3 do enumerado tem
significado: passado pela quiescência, exclui o singular, e aqui exclui o
ProbCut. Um valor, quatro consequências.

**Campo novo:** `this+0x2c8`, contador de 64 bits das entradas no ProbCut.

## A profundidade e a margem

```
437232:  mov   0xec(%rbp),%r8d    ; 0xec = 5   (sem manete)
437247:  neg   %ecx
437249:  and   0xe4(%rbp),%ecx    ; bandeira ? 54 : 0   (0xe4, sem manete)
43724f:  sub   %ecx,%ebx
437253:  cmove 0xe8(%rbp),%r8d    ; se !bandeira -> 3   (0xe8, sem manete)
437263:  sub   %r8d,%eax          ; prof - (3 ou 5)
```

```cpp
const int recuo = bandeira_c4 ? 5 : 3;              // 0xec / 0xe8
int alvo = beta + pc_m - (bandeira_c4 ? 54 : 0);    // 0xe4
const int prof_pc = prof - recuo;
```

Três constantes sem manete — `5`, `3`, `54` — escolhidas pela mesma bandeira de
pilha `0xc4(%rsp)` que já apareceu na margem do RFP.

## O limiar do SEE, que é dinâmico

```
43728e:  sub   %r12d,%ebx         ; alvo - aval
437291:  imul  $0xd0,%ebx,%eax    ; * 208
437297:  cmp   $0xffffff9d,%eax   ; < -99 ?
43729a:  jl    43ae36
4372a0:  mov   $0x64,%ebx         ; 100
4372ac:  idiv  %ebx               ; / 100
```

```cpp
const int limiar_see = (alvo - aval) * 208 / 100;
```

**É o mesmo `208/100` da `pontua`** (`41135e`), onde escala o limiar do SEE das
capturas. A constante `208` liga as duas funções com o mesmo papel — o factor
`2.08` que converte uma diferença de avaliação num limiar de troca.

## O ciclo dos lances do ProbCut

```
43726d:  call  generate<GenType 0>       ; só capturas
437288:  je    4379d0                    ; nenhuma -> fora
4372cb:  and   $0x3,%esi                 ; desenrolado 4x
4372f3:  call  legal
4372fc:  cmp   0x1e(%rbp,%r14,2),%r12w   ; == lance excluído?
43730e:  call  see_ge(m, limiar_see)
437317:  mov   %r12w,0x610(%rsp)         ; guarda na Lista
437320:  mov   $0x1,%ebx                 ; marca que há pelo menos um
```

```cpp
for (Move m : capturas) {
    if (!pos.legal(m)) continue;
    if (m == excluido[ply]) continue;                // this+0x29c3e
    if (!pos.see_ge(m, limiar_see)) continue;
    lista[n++] = m;
}
```

`generate<GenType 0>` — o mesmo gerador que a `quiescencia` usa em `420b7c`, e
que o `negamax` **só** usa aqui. Confirma o que a secção 10 deixou em aberto:
`GenType 0` é o gerador de capturas, `GenType 4` o geral.

E a comparação com `0x1e(%rbp,%r14,2)` é outra vez o lance excluído em
`this+0x29c3e + ply*2` — terceiro uso independente do mesmo campo.

## O que isto significa para a reconstrução

O ProbCut está **completo no binário e completamente inerte**. Quem o
reconstrua a partir do comportamento da árvore não o encontra; quem o
reconstrua a partir do código tem de o escrever inteiro, com `pc = 0` por
omissão — senão liga uma poda que o motor perdido não fazia.

E há o aviso da secção 1: a guarda `pc > 0` **tem** de vir antes da comparação
`prof >= pc`, ou o bloco liga sozinho.


---

# `TranspositionTable::guarda` — `0x4202f0` — e a política de substituição

A `sonda` já tinha sido recuperada numa passagem anterior deste documento, com
o empacotamento dos bits. Falta a outra metade: **como se escolhe a via a
substituir**. É aqui, e é o que decide o que sobrevive na tabela.

## A geometria, confirmada

```
420334:  and  %rdi,%r11              ; chave & máscara   (tt+0x8)
420337:  lea  (%r11,%r11,8),%rbx     ; índice * 9
42033b:  lea  (%rdx,%rbx,8),%r14     ; ... * 8  -> * 72
420395:  add  $0x18,%rdx             ; passo de 24 bytes
420399:  cmp  $0x3,%r10              ; três vias
```

Balde de **72 bytes**, ranhura de **24**, **três vias**. Bate exactamente com o
que a desmontagem da `sonda` já dizia, e com o cálculo do `prefetch` da
quiescência. Três leituras independentes, o mesmo número.

Campos da `TranspositionTable`: `+0x0` o vector de baldes, `+0x8` a máscara,
`+0x10` a geração actual.

## A ranhura, desempacotada da escrita

```
420348:  mov  (%rdx),%r12            ; dados
42034b:  mov  -0x8(%rdx),%r14        ; chave_xor
420356:  xor  %r12,%r14              ; chave = chave_xor ^ dados
420359:  cmp  %r14,%rdi              ; acerta?
420362:  movzbl 0x8(%rdx),%ebx       ; geração da ranhura
420369:  shr  $0x20,%r12             ; bits 32-39 dos dados
42036d:  movsbl %r12b,%r12d          ; profundidade, com sinal (int8)
```

Confirma o empacotamento já registado: `chave_xor = chave ^ dados`, e a
profundidade nos bits 32-39 como **inteiro de 8 bits com sinal**.

## A fórmula da substituição — o que não estava recuperado

```
420371:  sub  %ebx,%r11d             ; ger_actual - ger_ranhura
420374:  movzbl %r11b,%r14d          ; truncado a 8 bits (idade cíclica)
420378:  lea  0x0(,%r14,4),%ebx      ; idade * 4
420380:  sub  %ebx,%r14d             ; idade - idade*4 = -3 * idade
420383:  add  %r12d,%r14d            ; + profundidade
420386:  cmp  %ebp,%r14d
420389:  jge  420391
42038b:  mov  %r14d,%ebp             ; guarda o menor
42038e:  mov  %r10,%r15              ; e a sua via
```

```cpp
int melhor = INT_MAX, via = 0;
for (int i = 0; i < 3; ++i) {
    if (ranhura[i].chave() == chave) { via = i; goto escreve; }   // 4204e8
    const int idade = uint8_t(ger_actual - ranhura[i].ger);
    const int valor = ranhura[i].profundidade - 3 * idade;
    if (valor < melhor) { melhor = valor; via = i; }
}
```

**O peso da idade é `3`**, cravado — implementado como `x - 4x` para evitar uma
multiplicação. Uma ranhura envelhecida uma geração vale menos 3 plies de
profundidade; ao fim de dez gerações, menos 30, e perde para qualquer coisa.

A idade é calculada em **8 bits sem sinal** (`movzbl`), portanto é cíclica: a
geração dá a volta em 256 e uma ranhura muito velha pode voltar a parecer nova.
Está assim no binário.

## As duas manetes globais

```
4203bb:  mov  G_TT_LANCE(%rip),%r10d
4203c2:  or   G_TT_PROF(%rip),%r10d
4203ce:  je   4203f0
```

`G_TT_LANCE` (`0x7594c0c`) e `G_TT_PROF` (`0x7594c08`) são as duas variáveis
globais que a primeira extracção encontrou ligadas a `KS_TT_LANCE` e
`KS_TT_PROF` — as únicas manetes que **não** vivem na `Parametros`. Estão
combinadas com um `or`: basta uma delas para tomar o caminho alternativo.

São a sexta família de interruptor, depois das manetes numéricas, dos campos sem
manete, das bandeiras `DIAG` e dos imediatos.

## As seis gravações do `negamax`

| endereço | o que grava |
|---|---|
| `437812` | depois do corte pelo RFP |
| `439a76` | o resultado normal do nó |
| `439bc2` | `prof = 245`, nota `31507`, aval `−32768` — a entrada de profundidade máxima |
| `43ac67`, `43ae2a`, `43b0df` | caminhos de corte, todos com o penúltimo argumento a `1` |

A de `439bc2` é a única com os três valores cravados, e é a que marca um nó que
atingiu o tecto de ply.


---

# `TranspositionTable::guarda_so_aval(chave, int16)` — `0x420500`, 144 bytes

Uma gravação **parcial**: escreve só a avaliação estática, sem tocar na nota,
no lance nem na profundidade. Chamada uma vez em cada `negamax` (`4396b3` no
principal, `43bff3` no clone) e em mais lado nenhum.

## A escolha da via, e é diferente da `guarda`

```
42052e:  mov   (%rax),%r8            ; dados
420531:  mov   -0x8(%rax),%rdx       ; chave_xor
420535:  xor   %r8,%rdx              ; chave
420538:  cmp   %rcx,%rdx
42053b:  je    420588                ; acerto -> usa esta
42053d:  test  %r9d,%r9d
420540:  js    420560                ; ainda não há candidata -> avalia
42054a:  cmp   $0x3,%rdi             ; três vias
```

```
420560:  movzbl 0x8(%rax),%edx       ; geração
420564:  test  %r8,%r8
420567:  je    42057f                ; ranhura vazia -> candidata
420569:  shr   $0x20,%r8
42056d:  cmp   $0xf9,%r8b            ; profundidade < -7 ?
420571:  jl    42057f                ;   -> candidata
420573:  mov   %r11d,%r8d
420576:  sub   %edx,%r8d             ; idade
420579:  cmp   $0x2,%r8b
42057d:  jbe   420542                ; idade <= 2 -> NÃO serve
42057f:  movslq %edi,%r9             ; fica candidata
```

```cpp
int via = -1;
for (int i = 0; i < 3; ++i) {
    if (ranhura[i].chave() == chave) { via = i; break; }
    if (via < 0) {
        const bool vazia = (ranhura[i].dados == 0);
        const int prof  = int8_t(ranhura[i].dados >> 32);
        const int idade = uint8_t(ger_actual - ranhura[i].ger);
        if (vazia || prof < -7 || idade > 2)
            via = i;
    }
}
if (via >= 0) escreve_so_aval(via, aval);
```

**Duas constantes novas e cravadas: `−7` e `2`.** Uma ranhura só é sacrificada
para guardar uma avaliação se estiver vazia, se tiver profundidade abaixo de
`−7`, ou se tiver mais de **duas** gerações. Caso contrário a chamada **não
escreve nada** — sai pelo `ret` em `420555`.

## A diferença face à `guarda`, e porque importa

| | `guarda` | `guarda_so_aval` |
|---|---|---|
| escolha da via | sempre há uma: a de menor `prof − 3*idade` | pode não haver nenhuma |
| critério | contínuo, por pontuação | por limiares: vazia, `prof < −7`, `idade > 2` |
| escreve | nota, lance, profundidade, limite, aval | **só a avaliação** |

A `guarda` **nunca falha** — escolhe sempre a pior das três e escreve por cima.
A `guarda_so_aval` **recusa-se a estragar** uma entrada recente e profunda só
para lá pôr uma avaliação.

Uma reconstrução que implemente as duas com a mesma política de substituição —
o que é a coisa natural a fazer, e o que uma transcrição de alto nível faria —
passa a deitar fora entradas boas da tabela em todos os nós onde a avaliação é
guardada. Não muda o resultado de nenhuma busca isolada; muda o que a tabela
contém, e portanto a árvore inteira nas iterações seguintes.

É exactamente o género de divergência que o critério "árvore idêntica ao nó"
apanharia, e que nenhuma leitura de alto nível revelaria.


---

# `negamax`, secção 13: poda por SEE, e o corte do RFP

## Dois limiares de SEE, um para capturas e outro para quietos

Os dois caminhos juntam-se no mesmo código a partir de `4394d2`. Só o cálculo
do limiar difere.

**Capturas** — `4394c8`:

```
4394b8:  cmpl $0x8,0x4(%rsp)       ; prof > 8 -> não poda
4394bd:  jg   438258
4394cd:  imul 0x50(%rbp),%r10d     ; seec = 70,  * prof
4394d2:  imul $0xffffff30,%r10d    ; * (-208)
4394e5:  imul $0x51eb851f / sar 37 ; / 100
4394f2:  call see_ge(m, limiar)
4394ff:  jne  438258               ; passou -> segue
439505:  jmp  437f50               ; falhou -> poda
```

```cpp
if (prof <= 8 && !pos.see_ge(m, -208 * (seec * prof) / 100))
    continue;                      // 0x50 = 70
```

**Quietos** — `439c28`:

```
439c2d:  lea  0x1(%rcx),%r10d      ; prof + 1
439c31:  imul %ecx,%r10d           ; * prof      -> prof*(prof+1)
439c35:  imul 0x54(%rbp),%r10d     ; seeq = 5
439c3a:  jmp  4394d2               ; o mesmo -208/100 a seguir
```

```cpp
if (!pos.see_ge(m, -208 * (seeq * prof * (prof+1)) / 100))
    continue;                      // 0x54 = 5
```

**A diferença de forma é o achado.** Para capturas o limiar é **linear** em
`prof`; para lances quietos é **quadrático**, `prof*(prof+1)`. Com as omissões:

| | fórmula | prof=4 | prof=8 |
|---|---|---:|---:|
| capturas | `−208·70·prof/100` | −582 | −1164 |
| quietos | `−208·5·prof(prof+1)/100` | −208 | −749 |

E o `−208/100` é o **terceiro** sítio com o mesmo factor `2.08`, depois da
`pontua` (`41135e`) e do ProbCut (`437291`). Três expressões distintas no fonte
com a mesma constante — ou uma função pequena embutida nas três.

## O corte do RFP, e não é `beta`

```
439c4c:  mov  0x30(%rsp),%r8d      ; beta
439c51:  sub  %r8d,%r12d           ; aval - beta
439c5b:  imul $0x55555556 / shr 32 ; / 3
439c69:  lea  (%rdx,%r8,1),%r11d   ; beta + (aval-beta)/3
439c6d:  jmp  436873               ; devolve
```

```cpp
return beta + (aval - beta) / 3;
```

**O RFP não devolve `beta`; devolve `beta + (aval − beta)/3`** — um terço do
excesso. Uma reconstrução que devolva `beta`, que é a forma clássica, dá outro
valor a cada corte e portanto outra janela ao nó pai.

## Os contadores globais

```
439482:  addq $0x1,g_corr_n         ; 0x75a90c8
43949a:  addq $0x1,g_corr_zero      ; 0x75a90c0
4394a6:  add  %rdi,g_corr_soma      ; 0x75a90b8
```

Estatística sobre a história de correcção: quantas consultas, quantas deram
zero, e a soma dos valores absolutos.

Os nove contadores encontrados até agora:

| símbolo | endereço |
|---|---|
| `g_corr_soma` | `0x75a90b8` |
| `g_corr_zero` | `0x75a90c0` |
| `g_corr_n` | `0x75a90c8` |
| `g_tt_sup` | `0x75a9088` |
| `g_tt_inf` | `0x75a9080` |
| `g_cortes_tt` | `0x75a9040` |
| `g_tt_fundo_cortou` | `0x75a9038` |
| `g_forma_qs` | `0x75a92e0` |
| `g_cat` / `g_idx` | `0x75a8fe0` / `0x75a8f40` |

Não afectam a busca, mas são linhas do motor — e são o instrumento com que ele
era medido, o que explica as bandeiras `KS_FORMA` e companhia.


---

# `negamax`: os blocos frios, e o aparelho de medida que lá vive

O compilador atirou para o fim da função tudo o que é raro. Os blocos só
alcançáveis por salto, ordenados pela distância a que ficaram da sua condição,
são **34**. Os mais distantes estão a 17.000 bytes do sítio que lhes salta.

| bloco | distância | o que é |
|---|---:|---|
| `43b1f1` | 17.070 | caminho de saída |
| `43acd9` | 16.615 | `cuckoo`, segunda instância |
| `43ae36` | 15.260 | ciclo de lances do ProbCut, segunda instância |
| `43ad58` | 15.241 | ajuste de mate do ProbCut |
| `43b0cd` | 14.583 | gravação na TT |
| `43a060` | 12.070 | **o lance nulo** |
| `439e82` | 13.047 | `cuckoo`, primeira instância |

## O aparelho de medida: 31 contadores globais

Os blocos frios estão cheios de incrementos a globais. A tabela de símbolos
dá-os todos, num bloco contíguo de `0x75a8ea0` a `0x75a92e8`:

| símbolo | tamanho | o que conta |
|---|---:|---|
| `g_idx_tranq` | 136 | histograma por índice, lances tranquilos |
| `g_idx` | 136 | histograma por índice |
| `g_cat` | 40 | cinco categorias de lance |
| `g_cortes` | 8 | cortes totais |
| `g_cortes_1` | 8 | cortes ao primeiro lance |
| `g_cortes_semtt` | 8 | cortes sem lance da TT |
| `g_cortes_1_semtt` | 8 | cortes ao primeiro sem TT |
| `g_cortes_tt` | 8 | cortes pelo lance da TT |
| `g_tt_exa` / `g_tt_exa_cortou` | 8 | entradas exactas, e quantas cortaram |
| `g_tt_inf` / `g_tt_inf_cortou` | 8 | limite inferior |
| `g_tt_sup` / `g_tt_sup_cortou` | 8 | limite superior |
| `g_tt_raso` / `g_tt_raso_cortou` | 8 | entradas rasas |
| `g_tt_fundo` / `g_tt_fundo_cortou` | 8 | entradas profundas |
| `g_tt_dprof` | 8 | soma das diferenças de profundidade |
| `g_sonda_tem` / `g_sonda_lance` | 8 | acertos da sonda, e com lance |
| `g_lances_proc` | 8 | lances processados |
| `g_nos_lista` | 8 | nós que geraram lista |
| `g_nos_com_tt` | 8 | nós com entrada na TT |
| `g_nos_com_tranquilos` | 8 | nós com lances tranquilos |
| `g_forma` | 512 | **histograma da forma da árvore, 64 escalões** |
| `g_forma_qs` | 8 | nós de quiescência |
| `g_corr_n` / `g_corr_zero` / `g_corr_soma` | 8 | a história de correcção |

**São 31 contadores.** `g_forma` tem 512 bytes — 64 entradas de 64 bits,
indexadas pela profundidade (o `cmp $0x3f` em `436728` limita a 63).

## O que isto era

Os pares `g_tt_X` / `g_tt_X_cortou` medem, para cada tipo de entrada da TT,
quantas apareceram e quantas provocaram corte. `g_cortes_1` sobre `g_cortes` dá
directamente **a percentagem de corte ao primeiro lance** — o número
76,71% / 85,74% que este documento persegue desde a primeira página.

**O motor perdido media-se a si próprio.** O indicador que a reconstrução tenta
igualar não era estimado de fora: era contado por dentro, e os contadores estão
todos aqui.

Uma reconstrução que queira comparar-se honestamente com o binário tem de ter
os mesmos 31 contadores, incrementados nos mesmos sítios — senão compara dois
números medidos de maneiras diferentes.

## O `cuckoo` aparece duas vezes

`439e82` e `43acd9` têm o mesmo código: `cuckooMove`, extracção de origem e
destino, bit da casa, `BetweenBB`, `and` com a ocupação. Instrução a instrução,
são iguais.

São **duas instâncias da mesma verificação**, em dois pontos do fluxo — não um
ciclo. A reconstrução escreve uma função e chama-a duas vezes; o compilador
embutiu-a nos dois sítios.

## O bloco `43ad76`: os killers outra vez

```
43ad76:  cmpl $0x0,0x70(%rbp)     ; killers = 0
43ad7a:  jle  43ada3              ; salta
43ad81:  cmp  0x28b88(%rdx),%r14w ; killer 1
43ad94:  cmp  0x28b8a(%rbp,%r8,4),%r14w ; killer 2
43ada3:  ...                      ; histogramas g_idx / g_idx_tranq
```

Terceiro sítio onde os killers são consultados — depois da `pontua` (`410fc6`,
`411200`) e do bloco de actualização (`43a300`). Todos inertes, todos completos.

E logo a seguir, os histogramas: `g_idx[min(i,16)]` e `g_idx_tranq[min(i,16)]`,
com o tecto `16` cravado em `43ada8`.


---

# `negamax`, secção 14: o corte pela TT, o IIR reduzido e o SEE do lance da TT

## O corte pela entrada da TT — `4396f3`…`43974f`

Entra por `4379eb`, quando `pcp_m > 0` (inerte) **ou** pelo caminho normal.

```
4396f7:  sub  0x1cc(%rbp),%edx     ; prof - pcp_p (4)
4396fd:  cmp  %edx,0x128(%rsp)     ; prof_tt >= prof - pcp_p ?
439704:  jl   4379f1               ; não -> sem corte
43970f:  lea  0x7c09(%r9),%r11d    ; beta + 31753
439716:  cmp  $0xf812,%r11d        ; <= 63506  (|beta| não é mate)
43971d:  ja   4379f1
439723:  mov  0x12c(%rsp),%ecx     ; nota da TT
43972e:  cmp  $0x7c09,%ecx / jg    ; > 31753  -> 43abdb
43973a:  cmp  $0xffff83f7,%ecx / jl ; < -31753 -> 43aca8
439746:  cmp  %r11d,%ecx
439749:  jge  436873               ; nota_tt >= beta -> CORTA
```

```cpp
if (prof_tt >= prof - pcp_p                       // 0x1cc = 4
    && (unsigned)(beta + 31753) <= 63506
    && |nota_tt| <= 31753
    && nota_tt >= beta)
    return nota_tt;
```

**A profundidade exigida é `prof − pcp_p`, não `prof`.** Com `pcp_p = 4`, uma
entrada quatro plies mais rasa do que o nó ainda corta. É uma tolerância
generosa, e é uma manete.

As duas saídas para notas de mate (`43abdb` e `43aca8`) são blocos frios
separados — o ajuste `±ply` é feito lá, não aqui.

## O IIR e o `iir_sem_all` — `439754`…`43977d`

```
439754:  mov  0xf0(%rbp),%edx      ; pc = 0
43975a:  movzbl 0x8(%rsp),%r9d     ; a7
439760:  or   0x38(%rsp),%r9b      ; a7 | outra bandeira
439765:  mov  0x1ac(%rbp),%esi     ; iir_sem_all = 0
43976b:  je   439778               ; = 0 -> reduz sempre
43976f:  test %r9b,%r9b
439772:  je   43927e               ; ligada e !a7 -> NÃO reduz
439778:  subl $0x1,0x4(%rsp)       ; prof -= 1
```

```cpp
if (!iir_sem_all || (a7 | outra))     // 0x1ac
    --prof;                            // a redução do IIR
```

**`iir_sem_all` a zero faz o IIR reduzir em todos os nós.** Ligada, passaria a
reduzir só nos nós de corte. O nome diz-lo — *"IIR sem all-nodes"* — e está
desligado, portanto o motor reduz **também** nos all-nodes.

É outra manete que não liga nem desliga: **restringe**. Terceira do género,
depois do `hp_lin` (troca a curva) e do `asp_sf` (troca o esquema).

## O SEE do lance da TT — `439782`…`4397a6`

```
439782:  mov  $0xffffffb5,%edx     ; -75
439796:  call see_ge(lance_tt, -75)
4397a6:  je   4389ee               ; falhou -> outro caminho
```

O limiar **−75** é cravado, e é o valor que a varredura dos argumentos
constantes já tinha assinalado. É o único `see_ge` do motor com limiar
constante não-nulo: a `quiescencia` usa `0`, as podas usam limiares calculados,
e o lance da TT usa `−75` fixo.

## Um quarto significado para o `Limite`

Em `43b26b`:

```
43b263:  mov  0x30(%rsp),%ecx
43b267:  cmp  %ecx,%esi
43b269:  jl   43b274
43b26b:  cmpb $0x0,DIAG+4          ; KS_SING_SEM_CORTE
43b272:  je   43b2be
```

O corte que se segue ao singular é governado por `KS_SING_SEM_CORTE`, a quinta
bandeira `DIAG`. A zero, o corte acontece; ligada, não. É a única bandeira
`DIAG` encontrada até agora no caminho do singular.


---

# `arranca`: o ciclo de aprofundamento iterativo — `43fffa`…`440a27`

## O controlo do ciclo

```
440a13:  mov  -0x4d0(%rbp),%r10d   ; profundidade máxima pedida
440a1a:  add  $0x1,%r13d           ; ++prof
440a1e:  cmp  %r10d,%r13d
440a21:  jle  43fffa               ; volta ao topo
440a27:  jmp  440221               ; acabou
```

O corpo começa em `43fffa` e fecha aqui. A profundidade sobe de um em um e o
tecto vem de fora (`Limites`).

## A paragem antecipada por tempo — `4409ab`…`440a0d`

```
4409b9:  movabs $0x431bde82d7b634db,%rax   ; / 1.000.000  (ns -> ms)
4409c3:  movslq 0x21c(%r12),%r9            ; 0x21c = 7   (sem manete)
4409d2:  sub    %rdi,%r11                  ; agora - inicio
4409e9:  sar    $0x12,%rsi                 ; -> ms decorridos
4409f0:  imul   %r9,%rsi                   ; * 7
4409dc:  movabs $0x6666666666666667,%rax
4409fb:  sar    $0x2,%rdx                  ; / 10
440a02:  add    %rcx,%rdx                  ; + tempo já gasto
440a05:  cmp    0x3a0(%r12),%rdx
440a0d:  jge    440221                     ; -> não vale a pena outra iteração
```

```cpp
const long decorrido_ms = (agora - inicio) / 1'000'000;
if (tempo_gasto + decorrido_ms * 7 / 10 >= limite_optimo)   // this+0x3a0
    break;                                                   // sai do ciclo
```

**A regra é `× 7 / 10`.** Antes de começar a iteração seguinte, o motor estima
que ela vai custar sete décimos do que custou a anterior — e se isso ultrapassar
o tempo óptimo, pára.

O `7` está em `0x21c`, dentro do molde mas **sem manete** — é a quadragésima
nona das constantes não configuráveis. O `10` é o divisor da magia
`0x6666666666666667` com `sar 2`, verificado.

**Campo novo:** `this+0x3a0`, o tempo **óptimo** em ms, distinto do
`this+0x3a8` que a `sem_tempo` usa como limite duro. Dois orçamentos de tempo:
um que interrompe a busca a meio, outro que impede de começar mais uma.

## A bandeira `g_tm_log` — uma décima quinta, fora do `DIAG`

```
409940:  call  getenv
40994f:  setne g_tm_log            ; 0x7594ca0
...
440990:  cmpb  $0x0,g_tm_log
440997:  jne   441795              ; -> bloco de registo do tempo
```

O mesmo padrão `getenv` → `setne` das catorze bandeiras `DIAG`, mas numa
variável **própria**, não no vector. Corresponde ao `KS_TM_LOG`, que a primeira
extracção tinha deixado sem destino.

Ficam assim **quinze** bandeiras de presença: catorze em `DIAG[0..0xd]` e esta
solta. Mais as duas globais `G_TT_LANCE` e `G_TT_PROF`, que são numéricas.

## As duas contagens de nós

```
43f94c:  movq  $0x0,0x280(%rbx)    ; nós, zerado no arranque
43f993:  vmovdqu %ymm8,0x2a0(%r15) ; zera 0x2a0..0x2bf de uma vez
43f9ae:  movl  $0x0,0x298(%r15)    ; profundidade máxima atingida
```

O `vmovdqu` de 32 bytes em `0x2a0` limpa de uma vez os quatro contadores do
singular que a secção 11 encontrou (`0x2a0`, `0x2a8`, `0x2b0`, `0x2b8`).
Confirma que são um grupo contíguo, declarado junto no fonte.

## Estado do `arranca`

| bloco | estado |
|---|---|
| leitura das 122 manetes + 15 bandeiras | ✅ mapeado |
| construção da tabela LMR | ✅ |
| janela de aspiração e re-busca | ✅ |
| chamada à raiz (`ply = 1`) | ✅ |
| gestão de tempo | ✅ |
| **ciclo de aprofundamento e paragem antecipada** | ✅ |
| ordenação estável dos lances da raiz | ✅ comparador identificado |
| impressão UCI | 171 sítios, por ler |


---

# `arranca`: a saída UCI e os relatórios de diagnóstico

134 cadeias literais referenciadas, 84 distintas. Divididas em duas famílias.

## 1. A saída UCI normal

```
info depth <d> seldepth <s> score cp <v> nodes <n> nps <x> time <t> hashfull <h> pv <...>
info depth <d> seldepth <s> score mate <m> ... wdl <w> ... tbhits <b> ...
bestmove <lance>
```

Os pedaços, pela ordem de montagem:

| cadeia | endereço |
|---|---|
| `info depth ` | `440d09` |
| ` seldepth ` | `440d29` |
| ` score ` | `440d50` |
| `cp ` / `mate ` | `441a34` / `440d7a` |
| ` wdl ` | `440e7c` |
| ` nodes ` | `4417e6` |
| ` nps ` | `441842` |
| ` time ` | `441814` |
| ` hashfull ` | `440f6e` |
| ` tbhits ` | `441c45` |
| ` pv` | `44188f` |
| `bestmove ` | `441f1c` |

E uma linha inteira pré-montada para o caso trivial:

```
441e51:  info depth 1 seldepth 1 score cp 0 nodes 1 nps 0 time 0 pv
```

É o que o motor imprime quando só há um lance legal — não calcula nada, despeja
a linha e responde. **Um caminho que nenhuma reconstrução adivinharia**, e que
o bot vê sempre que a posição é forçada.

## 2. Os relatórios `info string`, que são o aparelho de medida

Dezoito relatórios distintos, e cada um corresponde a um grupo dos 31
contadores globais da secção dos blocos frios:

| relatório | contadores que despeja |
|---|---|
| `info string SONDA nos= acertou= \| dessas, com_lance=` | `g_sonda_tem`, `g_sonda_lance` |
| `info string CORTE_POR` | `g_cortes`, `g_cortes_tt` |
| `info string CORTE_EM` · `CORTE_TRANQUILO_EM` | `g_idx[]`, `g_idx_tranq[]`, com escalão ` 8+:` |
| `info string ORDEM cortes= no_primeiro=` | `g_cortes`, `g_cortes_1` |
| `info string ORDEM_TT nos= com_tt= \| cortes_pelo_tt= \| SEM tt: cortes= ao_primeiro=` | `g_nos_com_tt`, `g_cortes_semtt`, `g_cortes_1_semtt` |
| `info string TT_FUNDO com_lance= fundo= (corta ) \| raso= \| media prof_entrada-prof_no= \| corta no total=` | `g_tt_fundo`, `g_tt_raso`, `g_tt_dprof` |
| `info string TT_LIMITE superior= \| inferior= \| exacto=` | `g_tt_sup`, `g_tt_inf`, `g_tt_exa` |
| `info string LANCES procurados= nos_com_laco=` | `g_lances_proc`, `g_nos_lista` |
| `info string TRANQUILOS nos_com_lista= chegaram_a_pontuar_tranquilos=` | `g_nos_com_tranquilos` |
| `info string FORMA total= qs= prof1-3= prof>=14=` + `por profundidade:` | `g_forma[64]`, `g_forma_qs` |
| `info string PC nos_entrados= tentativas= passaram_qs= cortes=` | contadores do ProbCut (`this+0x2c8`) |
| `info string SING chamadas= (% da arvore) ext1= ext2= ext-1=` | `this+0x2a0`, `0x2b0`, `0x2b8` |
| `info string CORR chamadas= a_zero= modulo_medio=` | `g_corr_n`, `g_corr_zero`, `g_corr_soma` |
| `info string AVAL chamadas= nos= por_no= \| busca: calc= tabela= (% poupados) \| qs: calc=` | contadores da avaliação |
| `info string MARGEM` | sob `KS_MARGEM_ESTUDO` |
| `RAIZ prof %d (%zu lances, %zu passagens):` + ` %s=%d` | via `printf`, sob `KS_FORMA` |

## O que isto fecha

**A `ORDEM` imprime literalmente o número que este documento persegue.**

```
info string ORDEM cortes=<g_cortes> no_primeiro=<g_cortes_1> (<pct>%)
```

O `76,71%` contra `85,74%` sai desta linha. Não era calculado por fora nem
estimado: o motor perdido imprimia-o, e a linha está aqui com o formato exacto.

E o `SING` imprime `ext1`, `ext2` e **`ext-1`** — três tipos de extensão, sendo
a terceira negativa. A secção 11 encontrou `ext = 1` e `ext = 2`; o `ext-1`
confirma que existe um terceiro caso, de **redução** vinda do singular, que
ainda não localizei no código.

## As quatro categorias de `g_cat`

```
43e66b  'tt'
43e67e  'captura'
43e68c  'promo'
43e6ab  'killer'
440476  'tranquilo'
```

Cinco nomes, e `g_cat` tem 40 bytes = **cinco** contadores de 64 bits. Batem.
São as categorias com que o motor classificava o lance que provocou o corte —
e `killer` é uma delas, apesar de os killers estarem desligados.


---

# `negamax`, secção 15: a extensão negativa, e o `ext_neg`

A saída UCI do `arranca` imprime `ext1=`, `ext2=` e **`ext-1=`** — três tipos de
resultado do singular. A secção 11 encontrou os dois primeiros. O terceiro
estava no bloco mais frio da função, em `43b294`.

## O caminho

```
43b263:  mov  0x30(%rsp),%ecx      ; beta
43b267:  cmp  %ecx,%esi
43b269:  jl   43b274
43b26b:  cmpb $0x0,DIAG+4          ; KS_SING_SEM_CORTE
43b272:  je   43b2be               ; desligada -> corta e devolve
43b274:  cmpb $0x0,0x53(%rsp)      ; a6 (só verdadeiro no singular)
43b279:  jne  43b294
43b27b:  neg  %eax / cmovs         ; |nota|
43b284:  cmp  $0x7c09,%eax         ; > 31753 -> não é mate
43b289:  jg   43b294
43b28f:  cmp  %edx,%r11d
43b292:  jge  43b2c6
43b294:  cmp  %r12d,0x30(%rsp)     ; beta > valor?
43b299:  jg   43827c               ; -> extensão 0
43b29f:  mov  0xa0(%rbp),%r12d     ; ext_neg = 1
43b2a6:  addq $0x1,0x2c0(%rbp)     ; conta
43b2ae:  neg  %r12d                ; -ext_neg
43b2b1:  mov  %r12d,0x80(%rsp)     ; extensão = -1
```

```cpp
// quando a busca singular mostra que NÃO é singular e o valor bate no beta
if (beta <= valor) {
    ++contador_ext_neg;            // this+0x2c0
    ext = -ext_neg;                // 0xa0 = 1  ->  ext = -1
}
```

**A extensão negativa é uma *redução* vinda do singular.** Quando a busca de
verificação mostra que o lance da TT não é singular — há outros lances tão bons
— e o valor devolvido ainda bate no beta, o nó é **reduzido** em vez de
estendido.

`ext_neg` está em `0xa0`, vale **1**, tem manete (`KS_EXT_NEG`) e está activo.

**Campo novo:** `this+0x2c0`, o quinto contador do grupo do singular. Completa
o bloco de 32 bytes que o `vmovdqu` de `43f993` zera de uma vez:

| desl. | contador |
|---|---|
| `0x2a0` | chamadas ao singular |
| `0x2a8` | nós gastos |
| `0x2b0` | confirmaram (ext = 1) |
| `0x2b8` | extensão dupla (ext = 2) |
| `0x2c0` | **extensão negativa (ext = −1)** |

São cinco de 8 bytes = 40, e o `vmovdqu` limpa 32 — os quatro primeiros. O
quinto é limpo à parte, ou faz parte de outro grupo. Fica assinalado.

## Os três valores da extensão, completos

```cpp
int ext = 0;                                     // 43827c
if (singular) {
    ext = 1;                                     // 439e38 / 43b253
    if (v < novo_beta - ext_dupla) ext = 2;      // 439e54,  0x88 = 40
} else if (beta <= v) {
    ext = -ext_neg;                              // 43b2b1,  0xa0 = 1
}
```

E a profundidade do filho usa `prof - 1 + ext`, lido em `438b4b`
(`mov 0x80(%rsp),%r8d`) já dentro da montagem dos argumentos da `reducao`.

## `KS_SING_SEM_CORTE` governa qual dos dois acontece

A bandeira `DIAG+4` é testada duas vezes, em `43b26b` e `43b2c6`, e decide
entre **cortar e devolver** (`43b2be` / `43b2cf` → `436873`) e **continuar para
a extensão negativa**. A zero — que é o estado normal — o motor **corta**.

Portanto, com as omissões, o caminho da extensão negativa só é alcançado quando
a primeira condição (`43b269: jl`) desvia para `43b274`. O contador `ext-1` que
o `arranca` imprime não é letra morta: conta um caso real.


---

# `negamax`, secção 16: a sequência de jogar um lance

`438258`…`4383fa`. Onze passos, por ordem exacta. É a espinha do ciclo e o
sítio onde uma reconstrução se desalinha mais facilmente, porque a ordem
importa e nada no comportamento a revela.

```
1.  438258:  test %r12w,%r12w              ; há lance excluído? -> salta
2.  438266:  cmp  0xa6(%rsp),%r14w         ; é o lance da TT? -> tenta o singular
3.  438275:  sub  $0x1,%edi                ; prof - 1
4.  43827c:  movl $0x0,0x80(%rsp)          ; extensão = 0
5.  438287:  ...                           ; amassa a chave (PCG, 7.ª ocorrência)
6.  4382e8:  call emplace_back             ; empilha em this+0x29e30
7.  438313:  mov  %edi,0x29398(%r8)        ; destino do lance -> por ply
    43832d:  mov  %r12d,0x28fc0(%r8)       ; peça do lance   -> por ply
8.  438334:  call gives_check(m)
9.  43836f:  ...                           ; sobe a pilha de acumuladores
10. 43839c:  call prefetch_key(m)
    4383c3:  prefetcht0                    ; puxa o balde da TT
11. 4383fa:  call do_move(m, estado, xeque, ...)
```

## O passo 7 é o que importa

```
438313:  mov %edi,0x29398(%r8)     ; destino
43832d:  mov %r12d,0x28fc0(%r8)    ; peça
```

**A peça e o destino são escritos na pilha de busca ANTES do `do_move`.** São
exactamente os dois vectores que a `hist_de`, a `ordena` e a `credita` lêem
para indexar as histórias de continuação (`this+0x28fc0` e `this+0x29398`).

A peça é `(tabuleiro[origem] & 7) - 1`, lida **antes** de o lance ser jogado —
tem de ser, porque depois a casa de origem está vazia. Uma reconstrução que
escreva estes dois campos depois do `do_move` lê a peça errada e envenena todas
as histórias de continuação dos filhos.

## O passo 10, e o encadeamento com a TT

```
43836f:  mov  0x258(%rbp),%r12     ; a TranspositionTable
43839c:  call prefetch_key(m)      ; devolve a chave que o lance vai dar
4383ba:  and  0x8(%r12),%rax       ; & máscara
4383bf:  lea  (%rax,%rax,8),%r11   ; * 9
4383c3:  prefetcht0 (%r9,%r11,8)   ; * 8 -> 72 bytes
```

O `prefetch` é calculado com a chave **depois** do lance, obtida sem o jogar.
`prefetch_key` existe só para isso. É a terceira confirmação independente do
balde de 72 bytes.

## O contador `g_lances_proc` está sob `KS_FORMA`

```
4383c8:  cmpb $0x0,DIAG+0xa        ; KS_FORMA
4383cf:  je   4383d9               ; desligada -> não conta
4383d1:  addq $0x1,g_lances_proc
```

Vários dos 31 contadores só são incrementados com `KS_FORMA` ligada. O relatório
`info string LANCES procurados=` imprime um zero quando a bandeira está
desligada — que é o estado normal.

**Isto tem consequência prática para o trabalho de comparação:** para medir o
motor perdido é preciso correr com `KS_FORMA=1`, e nesse estado ele **não é o
mesmo motor** — os contadores custam tempo e o `DIAG+0xa` aparece em 14 sítios.
Não muda a árvore, mas muda o relógio, e portanto muda a gestão de tempo.

## O que fica da ordem

Sete das onze operações são estado que não afecta o resultado da busca deste
nó — empilhar a chave, marcar a peça, subir o acumulador, o `prefetch`. Mas
todas afectam os **filhos**. É o género de código que uma transcrição de alto
nível reordena sem pensar, e que só a leitura instrução a instrução fixa.


---

# `negamax`, secção 17: a busca do filho e o desfazer

`438460`…`4384f7`. O caminho sem LMR — o primeiro lance, ou quando a redução
não se aplica.

## A profundidade do filho

```
438460:  mov  0x80(%rsp),%r8d      ; a extensão (-1, 0, 1 ou 2)
438468:  prefetcht0 (%rdi)
43846b:  add  %r8d,%r11d           ; prof_filho = (prof-1) + ext
43846e:  cmpq $0x0,0x28(%rsp)
438474:  jne  438a40               ; há redução -> o outro caminho
```

```cpp
const int prof_filho = (prof - 1) + ext;
```

A extensão soma-se à profundidade **já decrementada**, não à original. Com
`ext = 2` o filho fica com `prof + 1`; com `ext = -1`, com `prof - 2`.

## A chamada

```
438491:  push %r12                 ; a7
43849a:  push %rdi                 ; a6
438498:  neg  %ecx                 ; -beta
4384ab:  neg  %r8d                 ; -alpha
4384ae:  call 436690
4384b9:  neg  %eax                 ; -resultado
```

Janela invertida e negada, como manda o negamax. Os dois booleanos vêm de
variáveis, não de constantes — é o caminho geral.

## O desfazer, e a ordem inversa exacta

```
4384be:  call undo_move(m)                  ; 1. desfaz o lance
4384ca:  subq $0x1,0x6ede6c0(%r9)           ; 2. desce a pilha de acumuladores
4384d2:  subq $0x8,0x29e38(%rbp)            ; 3. desempilha a chave
4384da:  cmpb $0x0,0x390(%rbp)              ; 4. verifica a paragem
4384eb:  jne  439b01                        ;    -> abandona tudo
```

**Três desfazeres, pela ordem inversa exacta dos três empilhares** da secção 16
(passos 6, 9, 11). E só depois se olha para a bandeira de paragem.

Repare-se em **o que não é desfeito**: os campos `this+0x28fc0` (peça) e
`this+0x29398` (destino) do passo 7 **não são limpos**. Ficam com o valor do
último lance tentado neste ply — e é assim que os filhos do lance seguinte lêem
a história de continuação. Não é um descuido: é o mecanismo.

Uma reconstrução que os limpe no desfazer — o que é o instinto correcto para
qualquer outra coisa — apaga a informação de que o ply seguinte depende.

## A paragem a meio

```
4384da:  cmpb $0x0,0x390(%rbp)
4384eb:  jne  439b01
```

Se a bandeira `parar` subiu durante a busca do filho, salta para `439b01` — que
abandona **sem** guardar na TT e sem actualizar históricos. O resultado do filho
é descartado.

É o comportamento certo (o valor pode estar truncado pelo tempo), e é uma linha
que uma reconstrução apressada esquece: o instinto é guardar o que já se tem.

## O tecto de 255 lances

```
438509:  mov  0xc8(%rsp),%ecx
438510:  cmp  $0xff,%ecx
438516:  jg   43852e
```

O contador de lances processados satura em **255**. A `Lista` tem espaço para
256 (secção 8), e o contador que alimenta o LMP e o LMR pára um abaixo. Os dois
números são vizinhos e diferentes, e o `255` é o que a varredura das constantes
assinalou com 32 ocorrências.


---

# `negamax`, secção 18: quando se aplica LMR, e quando não

O ciclo bifurca em `43846e` (`cmpq $0x0,0x28(%rsp)`). As duas metades são
`438a40` — com redução — e `438e58` — sem.

## As três condições para reduzir — `438a40`…`438a77`

```
438a40:  cmpl $0x2,0x4(%rsp)       ; prof > 2 ?
438a45:  mov  0x90(%r15),%eax      ; pos+0x90
438a4c:  mov  0xb0(%r15),%ecx      ; pos+0xb0
438a53:  mov  0x148(%rbp),%edx     ; lmr_pecas_fim = 0
438a59:  jle  438e58               ; prof <= 2 -> sem redução
438a5f:  add  %ecx,%eax            ; pos90 + posb0
438a61:  cmp  %eax,%edx
438a63:  setl %r10b                ; lmr_pecas_fim < soma ?
438a67:  and  0xc3(%rsp),%r10b     ; && bandeira
438a6f:  and  %sil,%r10b           ; && outra
438a77:  je   438e58               ; falhou alguma -> sem redução
```

```cpp
const bool reduz = prof > 2
                && lmr_pecas_fim < (pos.n90 + pos.nb0)    // 0x148
                && bandeira_c3
                && bandeira_sil;
```

**`lmr_pecas_fim` a zero torna a segunda condição quase sempre verdadeira** —
basta haver uma peça. Ligada, seria um piso de material abaixo do qual o motor
deixa de reduzir: o nome diz *"LMR peças fim"*, isto é, não reduzir em finais
pobres. Está desligado, portanto **o motor perdido reduz mesmo em finais**.

`pos+0x90` e `pos+0xb0` são duas contagens de peças, somadas. Aparecem também
em `438014`, com a mesma soma — dois sítios, mesma expressão.

## O caminho sem redução — `438e58`

```
438e7b:  push $0x1                 ; a7 = 1
438e7d:  push $0x0                 ; a6 = 0
438e8b:  not  %ecx                 ; -alpha-1
438e95:  call 436690
438ea8:  neg  %r12d
438eaf:  cmp  %r12d,0x40(%rsp)     ; alpha < v ?
438eb4:  jl   4391ee               ; sim -> re-busca com janela cheia
438ebc:  ...                       ; não -> desfaz e segue
```

```cpp
int v = -negamax(pos, prof_filho, -alpha-1, -alpha, ply+1, false, true);
if (v > alpha) goto rebusca_janela_cheia;   // 4391ee
```

**Mesmo sem LMR há busca com janela nula primeiro.** O `not %ecx` produz
`-alpha-1`, que é a janela de scout. Só se o resultado passar o alfa é que há
segunda busca.

Isto significa que o motor faz **sempre** PVS — janela nula em todos os lances
menos o primeiro — e o LMR só acrescenta a redução de profundidade por cima.
São dois mecanismos independentes, e a reconstrução tem de os ter aos dois.

## As três formas de buscar um filho, agora completas

| caminho | profundidade | janela | condição |
|---|---|---|---|
| `4384ae` | `prof-1+ext` | **cheia** | primeiro lance |
| `438e95` | `prof-1+ext` | nula | sem LMR |
| `438bed` | `prof-1+ext-r` | nula | com LMR |
| `438c7c` | `prof-1+ext` | nula→cheia | re-busca após LMR |
| `4391ee` | `prof-1+ext` | cheia | re-busca após scout |

Cinco sítios de chamada distintos para o mesmo filho, cada um com a sua
combinação. Mais os dois do singular (`4392be`) e do lance nulo (`43a1c0`), dão
as **oito** chamadas recursivas que a secção 6 contou.

## O `and` triplo de `438a67`

```
438a67:  and 0xc3(%rsp),%r10b
438a6f:  and %sil,%r10b
```

Duas bandeiras de pilha em `and` com a condição do material. `0xc3(%rsp)` é
escrita em `437978` (`movb $0x0,0xc3(%rsp)`) e em `43b1fc` (`movb $0x1,...`) —
portanto vem do bloco frio `43b1f1`, que é alcançado a partir de `436fa8`.

São as condições "não é nó de PV" e "não está em xeque" — a inferir pelo
contexto, não pela leitura directa. **Fica assinalado como inferência.**


---

# `negamax`, secção 19: as tablebases

`439b09`…`439b72`. Único sítio do motor onde `Tablebases::search<false>` é
chamada.

## A condição de entrada

```
439b09:  testb $0xf,0x30(%rax)     ; bits 0-3 de estado+0x30
439b0d:  jne   43802f              ; algum ligado -> sem tablebases
439b1e:  movl  $0x1,0x120(%rsp)    ; ProbeState = 1
439b29:  call  431310 <Tablebases::search<false>(pos, &estado)>
439b2e:  mov   0x120(%rsp),%edx
439b37:  test  %edx,%edx
439b39:  je    43802f              ; falhou -> segue a busca normal
439b3f:  addq  $0x1,0x290(%rbp)    ; conta o acerto
```

**Campo novo:** `this+0x290`, contador de acertos nas tablebases — é o que
alimenta o ` tbhits ` da saída UCI (`441c45`).

O `testb $0xf,0x30(%rax)` sobre quatro bits de `estado+0x30` é o que decide se
vale a pena sondar. Pelo contexto é a contagem de peças ou um conjunto de
bandeiras; **não é legível daqui, fica assinalado.**

## A nota devolvida

```
439b54:  test  %eax,%eax
439b56:  jle   43b1a6              ; WDL <= 0 -> outro ramo
439b5c:  mov   $0x7b13,%ecx        ; 31507
439b69:  sub   %r13d,%ebx          ; 31507 - ply
439b6c:  cmp   $0x7c09,%ebx        ; vs 31753
439b72:  cmovle %ebx,%ecx          ; min
```

```cpp
int v = 31507 - ply;
if (v > 31753) v = 31507;          // na prática nunca
```

**Aqui está a origem do `31507`.** A varredura das constantes tinha-o
assinalado como *"a nota da profundidade máxima"*, por causa da gravação na TT
em `439bc2`. Está errado — ou melhor, incompleto: `31507` é a **nota de vitória
por tablebase**, ajustada pelo ply, e é gravada na TT logo a seguir.

Corrijo a tabela das constantes de mate:

| valor | papel |
|---:|---|
| `±32001` | infinito da janela de aspiração |
| `±31753` | tecto da avaliação corrigida |
| `±31506` | tecto da avaliação NNUE |
| **`31507`** | **vitória por tablebase**, menos o ply |
| `−32768` | "sem avaliação" |

O `31507` ser `31506 + 1` deixa de ser estranho: é o primeiro valor **acima** do
tecto da avaliação, isto é, o menor valor que nenhuma avaliação pode produzir.
É a escolha natural para "isto não é uma estimativa, é certeza".

## A gravação

```
439bb6:  mov   $0xf5,%edx          ; profundidade 245
439bbb:  push  $0xffffffffffff8000  ; aval = -32768
439bc2:  call  TT::guarda
```

Gravada com **profundidade 245** — o tecto de ply — para que nunca seja
substituída por uma busca normal, e com avaliação `−32768` (a marca de "sem
avaliação"). Faz sentido: uma vitória por tablebase é definitiva, e não tem
avaliação estática associada.

## O que isto fecha

A minha secção sobre as constantes dizia que `439bc2` gravava *"a entrada de
profundidade máxima"*. Grava — mas a razão não é o nó ter chegado ao fundo: é
ser uma sentença de tablebase, que vale a qualquer profundidade e por isso é
marcada com a máxima.

Terceira correcção a coisas que escrevi neste documento, e as três da mesma
natureza: **conclusões tiradas de um sítio antes de ver o outro**. A gravação
em `439bc2` só se entende depois de ler `439b29`, que está a 150 bytes dali e
num bloco frio.


---

# `negamax`, secção 20: mate, afogamento, e a segunda paragem por tempo

## Mate e afogamento — `438eec`

Alcançado quando `generate<GenType 4>` não devolve lance nenhum (`437a22`).

```
438eec:  cmpq $0x0,0x70(%rsp)      ; há xequeadores?
438ef2:  lea  -0x7d00(%r13),%r11d  ; ply - 32000
438ef9:  jne  436873               ; em xeque -> devolve ply - 32000
438eff:  jmp  436870               ; sem xeque -> devolve 0
```

```cpp
if (lista.vazia())
    return em_xeque ? (ply - 32000) : 0;
```

**O mate vale `ply − 32000`, não `−32000 + ply`** — é o mesmo número, mas a
ordem da subtracção diz que o valor base é `32000` e não o `32001` do infinito.
São dois números adjacentes com papéis distintos:

| valor | papel |
|---:|---|
| `32001` | infinito da janela |
| **`32000`** | **base do mate** |
| `31753` | tecto da avaliação corrigida |

Um mate ao ply 0 vale `−32000`; o infinito é um a mais, para que nenhuma janela
possa ser igualada por um mate.

O `0x70(%rsp)` é o `estado+0x48` guardado à entrada (`43676f`) — o bitboard dos
xequeadores. Zero significa sem xeque, e sem xeque com zero lances é
afogamento: **devolve `0`**, não empate por contempt. O `contempt` (`0x6c`) está
a zero e é lido noutro sítio.

## A segunda paragem por tempo — `438f08`

Dentro do ciclo de lances, não à entrada:

```
438f10:  mov  0x3a0(%rbp),%r10     ; tempo óptimo
438f17:  lea  (%r10,%r10,1),%rax   ; * 2
438f20:  call steady_clock::now()
438f25:  sub  0x398(%rbp),%rax     ; - inicio
438f3d:  movabs $0x431bde82d7b634db ; / 1.000.000 -> ms
438f55:  cmp  %r8,%rdx
438f58:  jl   438d69               ; ainda dá -> continua
438f5e:  movb $0x1,0x390(%rbp)     ; PARA
```

```cpp
if (decorrido_ms >= 2 * tempo_optimo)
    parar = true;
```

**É o dobro do tempo óptimo**, e é uma verificação distinta das duas já
conhecidas:

| onde | limite | quando |
|---|---|---|
| `sem_tempo` (`4202c9`) | `this+0x3a8` — **duro** | de 1024 em 1024 nós |
| `arranca` (`440a05`) | `this+0x3a0` — **óptimo** | antes de cada iteração |
| **aqui** (`438f55`) | **`2 × óptimo`** | dentro do ciclo de lances |

Três orçamentos de tempo, três sítios, três regras. O terceiro é o que impede
um único lance da raiz de consumir a partida inteira quando a busca se prolonga.

E chama `steady_clock::now()` **directamente**, sem passar pela `sem_tempo` —
portanto sem a protecção do "só de 1024 em 1024 nós". É uma leitura do relógio
por lance, no caminho quente.

## Contagem das paragens

O motor pode parar em cinco sítios distintos:

| sítio | causa |
|---|---|
| `420210` (`sem_tempo`) | limite duro de tempo, limite de nós, bandeira externa |
| `438f5e` | dobro do tempo óptimo, dentro do ciclo |
| `440a0d` | estimativa da iteração seguinte |
| `4384eb` / `438edf` | propagação da bandeira após o filho |
| `436870` | saída sem valor |

Todos escrevem ou lêem a mesma bandeira `this+0x390`, que é pegajosa: uma vez
a um, a `sem_tempo` devolve `true` para sempre.


---

# `negamax`, secção 21: o castigo dos lances que não cortaram

`43a382`…`43a7e0`. Era a maior lacuna por documentar no `negamax` — 1.118 bytes
— e é **um ciclo desenrolado 8×**, não lógica nova.

## O que faz

```
43a382:  test %r12d,%r12d          ; quantos lances tranquilos tentados
43a385:  jle  43a743               ; nenhum -> nada a castigar
43a38b:  neg  %ebx                 ; o bónus, negado -> castigo
43a394:  and  $0x7,%r8d            ; entrada do desenrolamento
43a3de:  cmp  (%rcx),%r14w         ; é o lance que cortou?
43a3e2:  je   43a40a               ;   -> salta, esse já foi premiado
43a3f2:  movzwl 0x400(%rsp,%r10,2),%edx   ; lance i da lista dos tranquilos
43a400:  call credita(pos, m, ply, -bonus)
```

```cpp
const int castigo = -bonus;
for (int i = 0; i < n_tranquilos; ++i)
    if (tranquilos[i] != lance_que_cortou)
        credita(pos, tranquilos[i], ply, castigo);
```

**O castigo é exactamente o simétrico do bónus** — `neg %ebx` sobre o mesmo
valor calculado em `43b122` (`clamp(200·prof, 1, 4000)`). Não há segunda
fórmula, nem escala diferente.

A lista dos tranquilos vive em `0x400(%rsp)`, separada da `Lista` principal
que está em `0x610(%rsp)`. **São dois vectores distintos na mesma pilha**: a
`Lista` com todos os lances e as suas notas, e esta só com os tranquilos
tentados, por ordem de tentativa.

## O bloco irmão: `43a84d`…`43ab48`

763 bytes, a segunda maior lacuna, e é o **mesmo ciclo para as capturas**:

```
43a80c:  cmpb $0x0,DIAG+1          ; KS_BONUS_ANTIGO
43a81b:  cmovne %ebx,%ecx          ; escolhe entre duas escalas
43a820:  call credita_captura
```

Desenrolado da mesma maneira, com a diferença de que cada iteração testa a
bandeira `KS_BONUS_ANTIGO` — são os 31 sítios de `cmovne` que a secção do
`BONUS` já tinha contado, agora localizados: estão todos aqui e no bloco
equivalente do clone (`43dee6`…`43e21b`).

## O que isto encerra

As duas maiores lacunas do `negamax` eram **código repetido pelo compilador**,
não lógica por descobrir. Juntas são 1.881 dos 19.534 bytes — quase 10% da
função — e reduzem-se a seis linhas de C.

Isto muda a estimativa do que falta. A cobertura por endereços citados dá 48%
no `negamax`, mas uma boa parte do resto é desta natureza: desenrolamentos,
escadas de `cmp $0..7`, e cópias do mesmo bloco para os dois lados de uma
bandeira.

## As lacunas que sobram, por natureza

| região | bytes | o que é |
|---|---:|---|
| `43a382`, `43a84d` | 1.881 | **ciclos desenrolados** — resolvido aqui |
| `437b91`, `4385c0`, `4375e4` | 2.071 | a construir a `Lista` e a pontuar |
| `43689c`, `436bbb`, `436dfb` | 1.286 | entrada: cuckoo, 50 lances, repetição |
| `438f8a`, `4398cd`, `439302` | 1.235 | actualização de alfa e do melhor lance |
| `43ae60` | 578 | ciclo do ProbCut, desenrolado |

Nenhuma delas é um mecanismo novo. São variações e repetições do que as vinte
secções anteriores já descreveram — o que é consistente com o motor ter ~30
mecanismos e 19.534 bytes de código gerado para eles.


---

# `negamax`, secção 22: a variante principal (PV) — `437b91` e `4385c0`

A segunda maior lacuna do `negamax` (917 bytes em `437b91`, mais 639 em
`4385c0`) é a gestão da **linha principal**.

## A estrutura por ply: 492 bytes

```
437c02:  imul $0x1ec,%rax,%r10     ; (ply+1) * 492
437c11:  imul $0x1ec,0x18(%rsp),%r12 ; ply * 492
437c29:  add  %rbp,%r10            ; this + ·
437c38:  mov  %r10,0xd0(%rsp)      ; ponteiro do filho
437c40:  mov  %r12,0xd8(%rsp)      ; ponteiro deste ply
```

**`0x1ec` = 492 bytes por ply**, e a base é `this+0x29e48`:

```
4385c0:  mov %r14w,0x29e48(%rbp,%r8,1)    ; pv[ply][0] = lance
4385e9:  movzwl 0x29e48(%r11),%edx        ; lê do filho
4385f7:  mov %dx,0x29e4a(%rdi)            ; escreve neste, deslocado 2
```

492 = **246 lances de 16 bits**. Com o tecto de ply em 245, é um lance por ply
mais um terminador. A tabela inteira ocupa `246 × 492` ≈ 121 KB, de `0x29e48`
até cerca de `0x47a00` — e encaixa exactamente antes do `0x47af0` que o
`arranca` usa.

**Isto fecha o mapa da `Busca`:** a região entre `0x29e48` e `0x47710`, que era
a maior zona por identificar, é a tabela da variante principal.

## A cópia, desenrolada 8×

```
4385e5:  and   $0x7,%r9d
4385e9:  movzwl 0x29e48(%r11),%edx    ; pv[ply+1][i]
4385f7:  mov   %dx,0x29e4a(%rdi)      ; pv[ply][i+1]
438601:  jle   43880b
```

```cpp
pv[ply][0] = lance;
for (int i = 0; pv[ply+1][i]; ++i)
    pv[ply][i+1] = pv[ply+1][i];
```

O lance actual à cabeça, e a linha do filho copiada a seguir. É o padrão
clássico, e está desenrolado pelo compilador — outra vez.

## A condição de mate na cópia

```
43859b:  lea    0x7c09(%r12),%edx    ; v + 31753
4385a9:  cmp    $0xf813,%edx         ; <= 63507 ?
4385af:  cmovae %r11d,%edi           ; fora -> não ajusta
```

`0xf813` = 63507, que é `2 × 31753 + 1`. O teste `(unsigned)(v + 31753) <= 63507`
é a maneira de perguntar `|v| <= 31754` numa só comparação.

**Repare-se que é `63507` e não `63506`.** O ProbCut usa `$0xf812` = 63506
(`437186`), e aqui é um a mais. Dois testes quase iguais com limites
diferentes por uma unidade — e nenhum dos dois é configurável.

## Estado das lacunas do `negamax`

| região | bytes | resolvida em |
|---|---:|---|
| `43a382` | 1.118 | secção 21 — castigo dos tranquilos |
| `43a84d` | 763 | secção 21 — castigo das capturas |
| `437b91` | 917 | **esta** — montagem da PV |
| `4385c0` | 639 | **esta** — cópia da PV |
| `43ae60` | 578 | ciclo do ProbCut, desenrolado (secção 12) |
| `438f8a` | 569 | actualização de alfa |
| `4375e4` | 515 | por ler |
| `43689c` | 455 | por ler — entrada |
| `436bbb` | 445 | por ler — cuckoo / 50 lances |

Cinco das nove maiores já estão explicadas, e **quatro delas eram repetição do
compilador**. Restam ~1.400 bytes de lógica genuinamente por ler no `negamax`.


---

# `negamax`, secção 23: a selecção do lance, e a troca a três vectores

`438f8a`…`4391b6`. A lacuna de 569 bytes. É a selecção incremental que a secção
8 tinha visto do lado do clone, agora com o detalhe que faltava.

## A varredura

```
438f99:  mov    0x200(%rbx,%rcx,4),%r11d  ; nota do candidato
438fa5:  mov    -0x400(%r12),%esi         ; a melhor até agora
438fb3:  and    $0x7,%r8d                 ; desenrolado 8x
438fb7:  cmp    %esi,%r11d
438fba:  cmovg  %ecx,%edx                 ; guarda o ÍNDICE
438fbd:  cmovle %esi,%r11d                ; e a NOTA
```

`cmovg` estrito: **só troca quando é maior**, nunca quando é igual. Confirma o
que a secção 8 concluiu — os empates ficam pela ordem de geração.

## A troca, e são três vectores ao mesmo tempo

```
43915b:  mov    0x20(%rsp),%r10        ; posição actual do ciclo
439160:  movzwl (%r10),%r11d           ; lance actual
439164:  cmp    %r14d,%edx
439167:  je     437ee6                 ; já é o melhor -> nada a trocar

439185:  movzwl (%rdx),%edi            ; 1. LANCE do melhor
43918c:  mov    %di,(%r10)             ;    -> posição actual
439190:  mov    %r11w,(%rdx)           ;    e o actual -> lá

439188:  mov    (%r12),%ecx            ; 2. NOTA em 0x810(%rsp)
439194:  mov    %ecx,-0x400(%r9)
43919b:  mov    %esi,(%r12)

4391a7:  mov    (%r9),%eax             ; 3. o terceiro vector, em 0xb0(%rsp)
4391ad:  mov    (%r14),%r8d
4391b0:  mov    %r8d,(%r9)
4391b3:  mov    %eax,(%r14)
```

**Três vectores paralelos trocados em conjunto:**

| base | conteúdo | passo |
|---|---|---|
| `%rbx` (`0x610(%rsp)`) | os lances, `uint16` | 2 bytes |
| `0x810(%rsp)` | as notas, `int32` | 4 bytes |
| `0xb0(%rsp)` | um terceiro, `int32` | 4 bytes |

O `shl $0x2,%r14` em `439179` confirma o passo de 4 nos dois últimos, contra o
`lea (%rbx,%r14,2)` de passo 2 no primeiro.

**O terceiro vector é novo.** Não estava na planta da `Lista` da secção 8, que
tinha lances, nota, segunda nota e contagem. Aqui há um vector adicional na
pilha do `negamax`, trocado em sincronia com os outros dois — e a `pontua`
escreve em `+0x200` e `+0x600` da sua base, que correspondem a `0x810` e
`0xc10` daqui.

Portanto: `0xb0(%rsp)` aponta para `0xc10(%rsp)` — a **segunda nota** que a
`pontua` escreve e que a secção 8 dizia "não participa na escolha". Participa
da troca, mas não da comparação. Fica corrigido: é mantida alinhada com os
lances, para ser lida depois.

## Onde o `KS_BONUS_ANTIGO` volta a aparecer

```
4391ce:  mov  $0x6a4,%edi          ; 1700
4391d3:  lea  (%r8,%r8,4),%r8d     ; prof * 5
4391d7:  shl  $0x5,%r8d            ;      * 32   = 160 * prof
4391db:  sub  $0x64,%r8d           ; - 100
4391df:  cmp  %edi,%r8d
```

A fórmula antiga outra vez, inteira, em mais um sítio. É o 34.º — a varredura
tinha contado 33 testes da bandeira, e este usa os valores sem testar (o teste
está em `4391c0`, sobre `%r11b`).

## O que fica desta secção

A `Lista` tem **quatro** vectores paralelos, não três:

| desl. na base | conteúdo | quem escreve | quem lê |
|---|---|---|---|
| `+0x000` | lance, `u16` | o gerador | o ciclo |
| `+0x200` | nota, `i32` | `pontua` | a selecção |
| `+0x600` | segunda nota, `i32` | `pontua` | trocada, lida depois |
| `+0xa00` | contagem | o gerador | todos |

E a troca mantém os três primeiros alinhados. Uma reconstrução que use um
`std::vector<std::tuple<...>>` ou que troque só os lances quebra o alinhamento
da segunda nota — e essa é lida mais à frente no ciclo.


---

# `negamax`, secção 24: a entrada — quiescência, histograma e repetição

`43689c`…`436a8d`. A lacuna de 455 bytes à cabeça da função.

## A chamada à quiescência é um salto, não uma chamada

```
4368a0:  mov  0x1018(%rsp),%rax    ; verifica o canário
4368bb:  add  $0x1028,%rsp         ; DESFAZ o quadro
4368c2:  mov  %r8d,%ecx            ; realinha os argumentos
4368c8:  pop  %rbx ... pop %r15    ; restaura os registos
4368d5:  jmp  420830 <quiescencia>
```

**`jmp`, não `call`.** Quando `prof <= 0`, o `negamax` desmonta o seu quadro
inteiro e salta para a `quiescencia` — chamada de cauda. O `negamax` não
aparece na pilha, e o valor que a `quiescencia` devolver é devolvido
directamente a quem chamou o `negamax`.

Uma reconstrução escreve `return quiescencia(...)` e o compilador decide; mas é
preciso que **seja mesmo um `return` directo**, sem nada a seguir. Qualquer
trabalho depois da chamada — um contador, uma libertação — impede a
optimização e muda o perfil de pilha em 245 plies de profundidade.

## O histograma da forma

```
4368e0:  lea  g_forma(%rip),%rdi
4368e7:  movslq %edx,%r8            ; a profundidade
4368ea:  addq $0x1,(%rdi,%r8,8)     ; g_forma[prof]++
```

Guardado por `DIAG+0xa` (`KS_FORMA`) e pelo `cmp $0x3f` de `436728` — daí os
**64 escalões** dos 512 bytes. Incrementa por profundidade do nó, não por ply.

É o que alimenta o `info string FORMA por profundidade:` do `arranca`.

## A detecção de repetição, à entrada

```
43693d:  mov  0x29e30(%rbp),%rdi    ; início da pilha de chaves
436944:  mov  0x29e38(%rbp),%rsi    ; fim
436954:  sar  $0x3,%rsi             ; quantas
43695b:  cmp  %esi,%edx             ; contra o contador dos 50 lances
436961:  cmovle %edx,%r9d           ; limite = min(c50, n)
436968:  cmovs  %r10d,%esi          ; e não abaixo de zero
43695d:  lea  -0x2(%rsi),%r8d       ; começa em n-2
436982:  cmp  %r14,(%rdi,%r12,8)    ; compara
436986:  je   436859                ; ACHOU
43698c:  sub  $0x2,%r12             ; recua de dois em dois
```

É o **mesmo** ciclo da `repete_ja` (`41f4e0`) e da `quiescencia` (`4208fa`) —
três cópias, todas desenroladas 8×, todas com a primeira comparação fora do
desenrolamento.

**Três instâncias do mesmo código em três funções.** No fonte é uma função; o
compilador embutiu-a nas três. A reconstrução escreve-a uma vez.

## Onde vai dar o acerto

```
436986:  je 436859
436859:  mov  0x6c(%rbp),%r11d      ; contempt = 0
43685d:  test %r11d,%r11d
436860:  jne  436d88                ; ligado -> valor de contempt
436866:  ...
436870:  xor  %r11d,%r11d           ; devolve 0
```

Repetição com `contempt = 0` devolve **zero**. O ramo do contempt está inteiro
e inerte — quarta manete a zero confirmada no caminho da entrada, depois de
`cuckoo_seminc`, `pcp_m` e `pc`.

## O que resta por ler no `negamax`

Depois desta secção, as lacunas ≥ 120 bytes que sobram são:

| região | bytes | natureza provável |
|---|---:|---|
| `4375e4` | 515 | montagem dos argumentos da `pontua` |
| `436bbb` | 445 | segunda instância do `cuckoo` |
| `436dfb` | 386 | caminho do `rule50 > 13` |
| `437413` | 380 | ciclo do ProbCut, terceira instância |
| `43889b` | 294 | continuação do LMR |
| `439302` | 284 | re-busca |

Nenhuma é mecanismo novo. **O `negamax` está lido nos seus mecanismos**; o que
falta são repetições, caminhos simétricos e montagem de argumentos.


---

# `negamax`, secção 25: a verificação do ProbCut, em duas fases

`4375e4`…`437812`. A lacuna de 515 bytes. **Não era montagem de argumentos** —
é a parte do ProbCut que faltava, e explica os quatro contadores que o
`info string PC` imprime.

## A sequência completa do ProbCut

A secção 12 leu a selecção das capturas. O que se faz com elas é isto:

```
1.  437602:  emplace_back(chave)       ; empilha
2.  437624:  mov %r10d,0x29398(%rsi)   ; destino por ply
    437631:  mov %edx,0x28fc0(%rsi)    ; peça por ply
3.  437641:  ...                       ; sobe o acumulador
4.  (do_move)
5.  4376ea:  call quiescencia(...)     ; PRIMEIRA fase
6.  4376ef:  neg %eax
7.  4376f3:  cmp %eax,0x54(%rsp)       ; passou o alvo?
    4376f7:  jg  43ac00                ;   não -> desfaz e lance seguinte
8.  437705:  addq $0x1,0x2d8(%rbx)     ; conta: passou a quiescência
9.  43770d:  test %r10d,%r10d
    437710:  jle 43ac71                ; prof_pc <= 0 -> aceita já
10. 437724:  push %r11 / push $0x0
    437736:  call negamax(prof_pc,...) ; SEGUNDA fase
11. 437770:  cmp %ebp,0x54(%rsp)
    437774:  jg  43b24b                ; falhou -> lance seguinte
12. 43778b:  addq $0x1,0x2e0(%rbx)     ; conta: cortou
13. 437812:  call TT::guarda
```

```cpp
for (Move m : capturas_filtradas) {
    do_move(m);
    int v = -quiescencia(pos, -alvo, -alvo+1, ply+1);
    if (v >= alvo) {
        ++passaram_qs;                              // this+0x2d8
        if (prof_pc > 0)
            v = -negamax(pos, prof_pc, -alvo, -alvo+1, ply+1, false, ...);
        if (v >= alvo) {
            ++cortes_pc;                            // this+0x2e0
            tt.guarda(chave, v, prof_pc+1, LIMITE_INF, m, ...);
            undo_move(m);
            return v;
        }
    }
    undo_move(m);
}
```

**A quiescência é o filtro barato e o `negamax` é a confirmação.** Só as
capturas que sobrevivem à quiescência custam uma busca a sério.

## Os quatro contadores do ProbCut, completos

| desl. | contador | cadeia UCI |
|---|---|---|
| `0x2c8` | nós que entraram no ProbCut | `nos_entrados=` |
| — | tentativas (lances filtrados) | `tentativas=` |
| **`0x2d8`** | **passaram a quiescência** | `passaram_qs=` |
| **`0x2e0`** | **cortaram** | `cortes=` |

A secção 12 tinha encontrado só o primeiro. Os quatro batem exactamente com os
quatro campos que a linha `info string PC nos_entrados= tentativas=
passaram_qs= cortes=` imprime — o que confirma a leitura dos dois lados.

## O ProbCut está inteiro, e inteiramente morto

Com `pc = 0` (`437153: test/jle`), nada disto corre. São agora **três blocos**
completos por reconstruir e nunca executados:

| bloco | bytes | o que é |
|---|---:|---|
| `4371a4`…`4373e9` | ~580 | condições e filtro das capturas |
| `4375e4`…`437812` | 515 | as duas fases da verificação |
| `43ae36`…`43b0a2` | 578 | o mesmo ciclo, outra instância |
| `437413` | 380 | terceira instância |

**Quase 2 KB do `negamax` — 10% da função — são ProbCut que não corre.**

## E o `0x54(%rsp)` é o alvo

Escrito em `43725f` (secção 12) como `beta + pc_m - (bandeira ? 54 : 0)`, e
lido aqui três vezes como o limiar de aceitação. Fecha o circuito entre as duas
secções, que estão a 600 bytes uma da outra.


---

# `negamax`, secção 26: o corte pela TT, e o enumerado `Limite` resolvido

`436dfb`…`436e98`. Era a lacuna de 386 bytes que eu tinha catalogado como
*"caminho do `rule50 > 13`"*. Não é: é **o corte principal pela tabela de
transposição**, e resolve o enumerado `Limite` por inteiro.

## As condições

```
436e09:  cmpw $0x0,0x29c3e(%rbp,%r8,2)  ; há lance excluído neste ply?
436e1b:  jne  436e98                    ;   -> sem corte
436e21:  cmp  %eax,0x128(%rsp)          ; prof_tt >= prof?
436e28:  jl   436e98
436e2a:  cmpb $0x3,0x10c(%rsp)          ; Limite == 3?
436e32:  je   436e98                    ;   -> sem corte
436e3c:  cmp  $0x7c09,%r9d              ; nota_tt > 31753 -> bloco frio
436e49:  lea  (%r9,%r13,1),%ecx         ; nota + ply
436e54:  cmovl %ecx,%r9d                ; se < -31753, ajusta o mate
```

## E aqui está o enumerado, completo

```
436e5b:  movzbl 0x10c(%rsp),%ebx    ; o Limite
436e63:  test %bl,%bl
436e65:  je   436e81                ; == 0 -> corta
436e67:  cmp  $0x1,%bl
436e6a:  je   43a04e                ; == 1 -> ramo próprio
436e70:  cmpb $0x2,0x10c(%rsp)
436e78:  jne  436e98                ; != 2 -> sem corte
436e7a:  cmp  %r11d,0x34(%rsp)      ; == 2 -> compara com alpha
436e7f:  jl   436e98
436e81:  ...
436e88:  cmpl $0x59,0x34(%r10)      ; rule50 <= 89 ?
436e8d:  jle  436873                ; CORTA
```

Juntando aos quatro sítios já conhecidos:

| `Limite` | significado | onde se vê |
|---:|---|---|
| **0** | **exacto** — corta sem condição | `436e65` |
| **1** | **limite inferior** — ramo em `43a04e`, compara com beta | `436e6a`, `4379e3` |
| **2** | **limite superior** — compara com alpha | `436e7a`, e o ramo da `pontua` que lê `tt_sup` |
| **3** | **nenhum / só avaliação** | quiescência, exclui singular, exclui ProbCut, exclui corte |

**O `3` é "esta entrada não tem nota fiável"** — e por isso a quiescência grava
com ele (secção `QUIESCENCIA_2`), e por isso exclui o singular, o ProbCut e o
corte. Os quatro sítios que eu tinha registado como coincidência são uma só
regra.

E o `2` ser o valor que faz a `pontua` ler `tt_sup` encaixa: é o limite
superior, o caso em que a nota da TT é um tecto e o lance merece a nota especial.

## O `89` da regra dos 50

```
436e88:  cmpl $0x59,0x34(%r10)     ; 0x59 = 89
436e8d:  jle  436873               ; corta
```

O corte pela TT **só acontece com `rule50 <= 89`**. Acima disso, mesmo com
entrada exacta e profundidade suficiente, o motor não corta — porque a posição
pode estar a caminho do empate por 50 lances e a nota guardada não sabe disso.

`89` é cravado, e é vizinho do `99` (`0x63`) que a `quiescencia` e a entrada do
`negamax` usam como limite absoluto. **Dois números da regra dos 50, dez a
separá-los**, ambos sem manete.

## O que isto fecha

Esta secção resolve a dúvida mais antiga deste documento. Desde a `pontua` que
eu registava "o `Limite == 3` outra vez" sem saber o que o `3` era. São quatro
valores, com quatro significados, e o `3` é o de "sem informação".

Uma reconstrução que nomeie o enumerado ao contrário — ou que use o `0` para
"nenhum", que é o instinto — inverte o comportamento em **seis** sítios
independentes ao mesmo tempo.


---

# `arranca`: as lacunas, classificadas

O `arranca` tem 6.862 instruções e a cobertura por endereços citados dava 10%.
Fui a todas as lacunas ≥ 120 bytes. **Nenhuma é mecanismo novo.**

## 1. Blocos de `getenv` — 4.266 bytes, 91 chamadas

| lacuna | instruções | chamadas a `getenv` |
|---|---:|---:|
| `43e71c`…`43ea91` | 185 | 17 |
| `43ebd0`…`43eff3` | 236 | 24 |
| `43f052`…`43f1e5` | 89 | 9 |
| `43f241`…`43f920` | 390 | 41 |

**900 instruções, 91 `getenv`.** É o padrão já mapeado, repetido:

```
lea  <nome>,%rdi
call getenv
test %rax,%rax / je  <salta>
mov  $0xa,%edx / xor %esi,%esi
call strtol
mov  -0x360(%rbp),%rX      ; recarrega o `this`
mov  %eax,DESL(%rX)
```

Já está tudo extraído: o mapa das 122 manetes com deslocamento e omissão saiu
precisamente daqui. **As quatro lacunas não contêm nada por ler** — são a
mesma sequência 91 vezes.

## 2. A construção da tabela LMR, desenrolada — `43fd11`…`43ffd1`, 704 bytes

```
43fd3f:  call __log            ; log(j)
43fd55:  call __log            ; log(i)
43fd5a:  vmulsd
43fd67:  vdivsd -0x350(%rbp)   ; / max(lmr_div/100, 0.01)
43fd6f:  vaddsd -0x330(%rbp)   ; + lmr_base/100
43fd77:  vmulsd [1024.0]
43fd83:  mov %r10d,0x3b0(%r12,%r15,4)
```

O corpo do ciclo, **repetido quatro vezes** (`43fd33`, `43fd83`, `43fdbf`, …)
com `lea 0x2(%rbx)`, `lea 0x3(%rbx)` — desenrolamento 4×. É a fórmula da
secção `ARRANCA_LMR`, já reconstruída. Nada novo.

## 3. A impressão dos relatórios — 17.919 bytes numa só lacuna

`441f49`…`446548`. É **mais de metade da função**, e é:

```
4404e4:  call __ostream_insert     ; a cadeia literal
4404e9:  mov  g_nos_lista(%rip),%rsi
4404f3:  call ostream::_M_insert<unsigned long>
```

O padrão repetido 171 vezes: cadeia, contador, cadeia, contador. Já tenho as
84 cadeias distintas inventariadas e ligadas aos 31 contadores na secção
`ARRANCA_UCI`. **A lacuna é volume, não conteúdo.**

Os `vmovdqu` de `g_forma+0x1a8` e adiante (`440292`, `4404aa`) são o histograma
a ser copiado para a pilha antes de ser impresso — 32 bytes de cada vez.

## 4. Cópias desenroladas — `440b72`…`440c6a`, 248 bytes

```
440b7b:  and $0x7,%esi
440b84:  cmp $0x1,%rsi / je
...
```

Escada de `cmp $0..7`. Cópia de um vector, desenrolada 8×.

## 5. O que sobra, e é pouco

| lacuna | bytes | o que é |
|---|---:|---|
| `440f97`…`441769` | 2.002 | montagem da linha `info depth` e da PV |
| `441a5f`, `441c6f`, `4418ba` | 1.218 | formatação de `cp`/`mate`/`wdl`/`tbhits` |
| `44024d`…`440449` | 508 | guarda de `0x47ae8` + início dos relatórios |

O `44024d` tem uma linha que interessa:

```
440252:  movzwl -0x240(%rbp),%r11d
440261:  mov    %r11w,0x47ae8(%rcx)      ; guarda o melhor lance da raiz
```

**Campo:** `this+0x47ae8`, o lance da raiz, escrito no fim de cada iteração e
lido em `440228` para decidir se a iteração seguinte começa. É o que o
`bestmove` imprime.

## Conclusão sobre o `arranca`

| natureza | bytes | estado |
|---|---:|---|
| leitura das manetes | 4.266 | ✅ extraído (mapa das 122) |
| tabela LMR | 704 + | ✅ fórmula reconstruída |
| janela de aspiração | — | ✅ |
| gestão de tempo | — | ✅ |
| ciclo de aprofundamento | — | ✅ |
| ordenação da raiz | — | ✅ comparador lido |
| **impressão UCI e relatórios** | **~21.000** | ✅ **cadeias e contadores inventariados** |

**Os 32.541 bytes do `arranca` são ~65% impressão, ~13% leitura de manetes, e
o resto é a lógica — que está lida.**

Não há lacunas de mecanismo no `arranca`.


---

# O clone `negamax.constprop.0` — o que tem a menos, e porquê

Medido, não suposto. Comparei os dois `negamax` por **perfil estrutural** — a
sequência de chamadas e de manetes lidas — em vez de instrução a instrução,
que a alocação de registos torna inútil.

| | principal | clone |
|---|---:|---:|
| instruções | 4.165 | 2.841 |
| eventos (chamadas + leituras de manete) | 182 | 125 |
| eventos em comum | — | 69 (**55%**) |
| manetes só no principal | **24** | — |
| manetes só no clone | **0** | — |

**O clone é um subconjunto estrito.** Não lê uma única manete que o principal
não leia.

## As 24 que faltam no clone

```
SING  SING_LOWER  SING_MARGEM  EXT_DUPLA  EXT2_EXACTO  EXT_NEG
CUCKOO  CUCKOO_SEMINC  CONTEMPT
ELO_MARGIN  OUR_ELO
ALPHA_DESC  AD_MIN  AD_MAX
LMP_MELHORA  LMP_PROF  HP_LIN  HP_PROF  PODA_RED  PIOR_ADV
SEEC  SEEQ  CONT_SEMINC  CONT_1LADO
```

Agrupadas, dizem exactamente o que o clone não faz:

| grupo | porque não existe no ply 1 |
|---|---|
| **singular** (6) | a extensão singular não se aplica ao primeiro nível |
| **repetição / cuckoo** (3) | com `ply = 1` não há ciclo possível a montante |
| **limitação de força** (2) | decidida uma vez, na raiz, pelo `arranca` |
| **descida de alfa** (3) | a janela vem do `arranca`, não se desce aqui |
| **podas tardias** (6) | LMP e poda por histórico não se aplicam tão perto da raiz |
| **SEE das podas** (2) | idem |
| **continuações sem incremento** (2) | não há ply−2 |

Não é o compilador a cortar código morto ao acaso: **é o `ply = 1` a tornar
metade das decisões impossíveis**, e a propagação de constantes a eliminá-las.

## O que isto vale para a reconstrução

**Não é preciso ler o clone.** Cada mecanismo que ele tem está no principal, e
o que lhe falta falta porque `ply = 1`. As lacunas que eu tinha catalogado no
clone — `43cfe0` com 2.842 bytes, `43ba3f` com 1.417, `43c106` com 993 — são
as mesmas coisas do principal com outra alocação de registos.

Escreve-se **uma** função. O compilador que faça o clone.

Isto encerra 13.121 bytes — 16% de todo o código do motor — sem trabalho
adicional, e com prova em vez de suposição.

## Nota sobre o método

A primeira comparação que fiz foi por sequência de mnemónicas, e deu **7%** de
código comum. Teria concluído que são funções diferentes.

São 7% porque o compilador aloca registos de outra maneira nas duas, e a
sequência `mov/mov/cmp` de uma não bate com a da outra mesmo quando fazem o
mesmo. **A mnemónica é ruído; a estrutura é sinal.** Comparar chamadas e
acessos a campos dá 55% — e a diferença é toda explicável pelo `ply = 1`.

Quarto caso neste trabalho em que a primeira medição diz o contrário da
segunda. Os anteriores: o regexp que excluía a forma de leitura, o
`0xffff82ff` convertido à mão, e o `31507` lido como "profundidade máxima".


---

# `Busca::limpa`: as cinco alocações, com tamanhos e valores de enchimento

A secção do mapa da estrutura descreveu o que a `limpa` zera. Faltavam as
**alocações** — os cinco vectores que vivem fora da `Busca`, no monte, e os
seus tamanhos exactos.

| tamanho pedido | bytes | entradas `int32` | vector |
|---|---:|---:|---|
| `0x2400` | 9.216 | 2.304 | **história de capturas** (`this+0x28358`) |
| `0x14000` | 81.920 | 20.480 | **história de plies rasos** (`this+0x2e8`) |
| `0xc0000` | 786.432 | 196.608 | **história de peões** (`this+0x28f98`) |
| `0x2d0000` | 2.949.120 | 737.280 | **histórias de continuação** (`this+0x28f78`) |
| `0x1800000` | 25.165.824 | 6.291.456 | **história de correcção** (`this+0x28fb8`) |

**25 MB só para a história de correcção.** É de longe a maior estrutura do
motor — mais do que tudo o resto junto, e mais de oitenta vezes a própria
`Busca` (294 KB).

## As dimensões batem com a leitura das funções

**Capturas** — 2.304 = `6 × 64 × 6`. A `credita_captura` indexa
`capt + 6*((peca-1)*64 + para)`, com `capt` em 0..5, `peca-1` em 0..5 e `para`
em 0..63. **Confirma.**

**Peões** — 196.608 = `8192 × 2 × 6 × 2`… não bate à primeira. A `hist_de`
indexa `((h*12 + lado*6 + peca)*64) + para` com `h < peao_ch = 8192`, o que dá
`8192 × 12 × 64` = 6.291.456 entradas, não 196.608.

**Discrepância.** Ou o `peao_ch` efectivo é menor do que 8192, ou o índice não é
o que eu li. `196608 / (12*64)` = **256**. Portanto a tabela tem espaço para
`h < 256`, e o `peao_ch = 8192` mascara para um valor que excede a tabela.

Isto é ou um defeito real do motor perdido — acesso fora dos limites — ou um
erro meu na leitura do índice. **Fica assinalado como questão em aberto, e é a
primeira contradição entre duas leituras independentes neste trabalho.**

**Continuações** — 737.280 = `5 × 6 × 64 × 6 × 64`. Cinco fatias, e as fatias
são de `0x180` = 384 = `6 × 64`. **Confirma exactamente** a estrutura
`cont[slot][peca_ant][para_ant][peca][para]` da secção da `hist_de`.

**Correcção** — 6.291.456 = `12 × 16384 × 32`? Não: `12 × 16384` = 196.608, e
`6291456 / 196608` = **32**. A `corrigida` indexa `(lado + 2i)*16384 + ix[i]`
com `i` em 0..5 e `lado` em 0..1, portanto 12 canais de 16.384 — 196.608
entradas. A tabela tem 32 vezes isso.

**Segunda discrepância**, do mesmo tipo. Ou há um factor de 32 no índice que eu
não li — um `ply` ou um bucket —, ou a tabela é deliberadamente maior.

## O enchimento a `102`

```
41fe10:  mov  $0x66,%r10d          ; 102
41fe22:  vpbroadcastd %xmm0,%ymm1
41fe27:  vmovdqu %ymm1,(%rax)
41fe16:  lea  0x14000(%rax),%r9    ; até 81.920 bytes
```

A **história de plies rasos** não nasce a zero: nasce a **102** em todas as
20.480 entradas, escritas 8 de cada vez com AVX2.

É a única das cinco que tem valor inicial diferente de zero. E é o ramo que
`low_f = 0` mantém inerte — portanto **20.480 inteiros inicializados a 102 que
nunca são lidos**.

## Realocação, não reutilização

```
41fdb0:  mov  $0x2400,%edi
41fdb5:  call _Znwm                ; aloca novo
41fdcb:  call memset               ; zera
41fde5:  mov  %rax,0x28358(%rbx)   ; troca o ponteiro
41fdff:  call _ZdlPvm              ; liberta o antigo
```

Quando o tamanho está errado, **aloca primeiro e liberta depois** — os dois
blocos coexistem. Com 25 MB na tabela de correcção, isso é um pico de 50 MB
durante a troca.


---

# Correcção à secção das alocações: os cinco vectores, bem atribuídos

Na secção anterior atribuí os cinco tamanhos aos vectores pela ordem em que
aparecem, sem verificar o destino de cada um. **Estava errado**, e as duas
"discrepâncias" que assinalei desaparecem quando se lê o ponteiro que cada
alocação escreve.

## Os destinos, lidos instrução a instrução

| alocação | bytes | `int32` | escreve em | vector |
|---|---:|---:|---|---|
| `41fdb0` | `0x2400` = 9.216 | 2.304 | `0x28358`/`0x28360`/`0x28368` | **capturas** |
| `41fe80` | `0x14000` = 81.920 | 20.480 | `0x2e8`/`0x2f0` | **plies rasos** |
| `42006f` | `0xc0000` = 786.432 | 196.608 | `0x28fa0`/`0x28fa8`/`0x28fb0` | **correcção** |
| `4200cb` | `0x2d0000` = 2.949.120 | 737.280 | `0x28f60`/`0x28f68`/`0x28f70` | **continuações** |
| `42017a` | `0x1800000` = 25.165.824 | 6.291.456 | `0x28f80`/`0x28f88`/`0x28f90` | **peões** |

Os ponteiros que eu tinha no mapa (`0x28f78`, `0x28f98`, `0x28fb8`) são o
**terceiro campo** de cada vector — a capacidade — e não o início. A `limpa`
escreve os três: início, fim, capacidade.

## As duas discrepâncias resolvem-se, e as dimensões batem todas

**Peões** — 6.291.456 entradas. O índice é
`((h*12 + lado*6 + peca)*64) + para` com `h < peao_ch = 8192`:

```
8192 × 12 × 64 = 6.291.456      ✔ exacto
```

**Correcção** — 196.608 entradas. O índice é `(lado + 2i)*16384 + ix[i]` com
`i` em 0..5 e `lado` em 0..1:

```
12 × 16.384 = 196.608           ✔ exacto
```

**Continuações** — 737.280:

```
5 fatias × 6 × 64 × 6 × 64 = 737.280   ✔ exacto
```

**Capturas** — 2.304:

```
6 capturadas × 6 peças × 64 casas = 2.304   ✔ exacto
```

**Plies rasos** — 20.480. O índice é `(ply*64 + de)*64 + para`, e
`20480 / (64*64)` = **5**. Cinco plies — que é exactamente o `ply <= 4` que
guarda o acesso em `412076` e `4122d7`.

**As cinco tabelas batem ao inteiro com os índices lidos nas funções.** Não há
acesso fora dos limites; a contradição era minha.

## O que o erro ensina

Atribuí os tamanhos pela ordem de aparição em vez de seguir o ponteiro. Foi a
mesma classe de erro das outras quatro vezes: **conclusão tirada de um sítio
antes de ler o outro**, que aqui estava a doze instruções de distância.

E, ao contrário das anteriores, esta teria sido cara: eu tinha escrito no
documento que o motor perdido fazia acesso fora dos limites na tabela de peões.
Quem lesse isso ia procurar um defeito que não existe — ou, pior, "corrigi-lo".

## O que sobrevive da secção anterior

O enchimento a **102** é real, e é da tabela dos **plies rasos**
(`41fe8a: mov $0x66,%edi` seguido de `mov %rax,0x2f0(%rbx)`). É a única das
cinco que não nasce a zero — e é a que `low_f = 0` mantém inerte. **20.480
inteiros postos a 102 que nunca são lidos.**

E a realocação aloca antes de libertar, portanto os 25 MB da tabela de peões
têm um pico de 50 MB na troca.


---

# `Busca::indices`: os canais 3, 4 e 5 — a última lacuna de mecanismo

`40d905`…`40da94`. Era a única coisa em todo o motor que eu tinha lido pela
metade. Está fechada.

## Os três não são do tabuleiro — são do **histórico de lances**

Os canais 0, 1 e 2 vêm da posição (peões de cada lado, via `ALEA`). Os três
últimos vêm dos **lances já jogados**, e cada um de uma distância diferente:

| canal | fonte | endereço |
|---|---|---|
| 3 | o lance de `ply − 1` | `40d954` |
| 4 | os **dois últimos** da pilha de chaves | `40dab3` |
| 5 | o lance de `ply − 6` | `40d9f8` |

## Canal 3 — o lance anterior

```
40d94b:  lea    -0x1(%rdx),%eax          ; ply - 1
40d954:  movslq 0x28fc0(%rbp,%r15,4),%rbx ; peca_ant
40d95c:  test   %ebx,%ebx / js            ; < 0 -> canal fica a 0
40d960:  movslq 0x29398(%rbp,%r15,4),%r14 ; para_ant
40d968:  shl    $0x8,%rbx                 ; peca_ant << 8
40d96c:  movabs $0x5eed00000000,%rsi      ; MARCADOR
40d994:  or     %r14,%rbx                 ; | para_ant
40d997:  or     %rsi,%rbx                 ; | marcador
40d99a:  add    %rcx,%rbx                 ; + 0x9e3779b97f4a7c15
40d9a0:  shr $0x1e / xor / imul           ; splitmix64
40d9c3:  and    $0x3fff
```

```cpp
if (pilha_peca[ply-1] >= 0) {
    uint64_t x = (uint64_t(pilha_peca[ply-1]) << 8)
               | uint64_t(pilha_para[ply-1])
               | 0x5eed00000000ULL;
    ix[3] = termina(x + 0x9e3779b97f4a7c15ULL);
} else ix[3] = 0;
```

## Canal 5 — o lance de seis plies atrás

Idêntico, com `ply − 6` e **outro marcador**:

```
40d9eb:  sub    $0x6,%r11d                ; ply - 6
40d9ef:  js     40db27                    ; antes da raiz -> this+0x350/0x368
40da13:  movabs $0x6e15000000000,%r11     ; MARCADOR diferente
40da1d:  shl    $0x8,%r8
40da42:  or     %r11,%r8
40da45:  add    %r15,%r8                  ; + golden ratio
40da71:  and    $0x3fff
```

```cpp
int pa, ta;
if (lance_anterior(ply, 6, pa, ta)) {          // com caminho pré-raiz
    uint64_t x = (uint64_t(pa) << 8) | uint64_t(ta) | 0x6e15000000000ULL;
    ix[5] = termina(x + 0x9e3779b97f4a7c15ULL);
} else ix[5] = -1;
```

**O canal 5 tem caminho pré-raiz** (`40db27`: `not %r11d`, depois
`this+0x350`/`0x368` com o guarda em `0x380`) — igual ao das histórias de
continuação. O canal 3 **não tem**: se `ply == 0`, fica a zero.

## Canal 4 — os dois últimos da pilha de chaves, e não tem marcador

```
40d9ca:  mov  0x29e30(%rbp),%rdi      ; pilha de chaves
40d9d1:  mov  0x29e38(%rbp),%r10
40d9de:  cmp  $0x8,%r10               ; ha pelo menos duas?
40dab3:  mov  -0x8(%rdi,%r10,1),%r15  ; a ultima
40dab8:  mov  -0x10(%rdi,%r10,1),%rax ; a penultima
40dac0:  cmp  %rax,%r15
40dac3:  je   40d9e8                  ; IGUAIS -> canal fica a 0
40dad3:  xor  %rax,%r15               ; chave[n-1] ^ chave[n-2]
40daed:  add  %r15,%r9                ; + golden ratio
40db16:  and  $0x3fff
```

```cpp
if (chaves.size() >= 2) {
    const uint64_t a = chaves[chaves.size()-1];
    const uint64_t b = chaves[chaves.size()-2];
    ix[4] = (a == b) ? 0 : termina((a ^ b) + 0x9e3779b97f4a7c15ULL);
} else ix[4] = 0;
```

**É o único dos seis que não usa `ALEA` nem um marcador** — opera directamente
sobre o XOR de duas chaves consecutivas. Isso torna-o sensível a *que par de
posições* levou aqui, não a que peça se moveu.

E a guarda `a == b` é real: duas chaves iguais seguidas significam que o lance
não mudou a posição do ponto de vista do Zobrist — nesse caso o canal fica a
zero em vez de indexar o hash de `0 + golden`.

## Os dois marcadores

| marcador | canal | bits |
|---|---|---|
| `0x00005eed00000000` | 3 | 46, 44-43, 41-40, 38-37, 35, 32 |
| `0x0006e15000000000` | 5 | 50-49, 47, 45-43, 40, 38, 36 |

São constantes de separação de domínio: garantem que o mesmo par
`(peça, casa)` produz índices diferentes em canais diferentes, sem precisar de
uma tabela por canal. Nenhum dos dois tem manete.

## O gravador, e a forma da cache

```
40d917:  mov  %rbx,0x24b60(%rbp,%r14,8)   ; a chave
40d91f:  lea  (%r14,%r14,2),%r14          ; ply * 3
40d923:  lea  0x0(%rbp,%r14,4),%rsi       ; * 4  -> 12 bytes por ply
40d928:  mov  %ecx,0x25310(%rsi)          ; ix[0]
40d932:  mov  %ecx,0x25314(%rsi)          ; ix[1]
40d93c:  mov  %r8d,0x25318(%rsi)          ; ix[2]
40da7e:  mov  %r12d,0xc(%r13)             ; ix[3]  -> na saida
40da82:  mov  %r9d,0x10(%r13)             ; ix[4]
40da86:  mov  %r10d,0x14(%r13)            ; ix[5]
```

**Campo novo:** `this+0x25310`, 12 bytes por ply — e **só os três primeiros
canais são guardados na cache**. Os canais 3, 4 e 5 são recalculados em cada
chamada, porque dependem do histórico de lances e não da posição.

Faz sentido: a chave da cache é a da *posição*, e dois caminhos diferentes
para a mesma posição dão os mesmos canais 0-2 mas canais 3-5 distintos.

## Fecha a tabela de correcção

Os seis canais, agora completos:

| canal | fonte | cache |
|---|---|---|
| 0 | peões do lado a jogar, via `ALEA+0x200` | sim |
| 1 | peões do outro lado, via `ALEA+0x1200` | sim |
| 2 | `FORCA` (`0x5ec544`) com as máscaras de material | sim |
| 3 | o lance de `ply−1`, marcador `0x5eed…` | não |
| 4 | XOR das duas últimas chaves | não |
| 5 | o lance de `ply−6`, marcador `0x6e15…`, com pré-raiz | não |

Com os pesos `[203, 109, 109, 121, 72, corr6]` da `corrigida`. E o sexto peso
é `corr6 = 0` — **o canal 5, o mais caro de calcular, é multiplicado por zero
em todas as chamadas.**

O motor calcula `ply−6`, resolve o caminho pré-raiz, faz o splitmix64, indexa
a tabela — e deita o resultado fora na multiplicação. Sem salto, sem `cmov`,
sem vestígio no comportamento.

**Não há mais lacunas de mecanismo no motor.**


---

# Comparação com o KestrelStrike atual — as divergências

Feita **depois** de a reconstrução estar fechada, como a disciplina deste
trabalho exige. É a primeira vez que o código atual foi aberto.

## Antes das divergências: o que já estava certo

Seis dos dez pontos que eu tinha listado no `LEIA-ME.md` como "o que uma
transcrição erra" **já estão corrigidos no motor atual**:

| ponto | estado |
|---|---|
| distâncias das continuações `{1, 2, 4, 3, 6}` | ✅ `CONT_RECUO[5] = {1,2,4,3,6}` |
| pesos `{2,1,1,1,1}` (a primeira dobra) | ✅ `CONT_PESO[5] = {2,1,1,1,1}` |
| RFP devolve `beta + (aval−beta)/3` | ✅ literal |
| `hp_lin` troca a curva | ✅ `hpoda_lin ? m*prof : m*prof*prof` |
| tabela LMR com `/100.0` e `*1024.0` | ✅ idêntica |
| quatro escalas de gravidade | ✅ `TECTO_HIST/CONT/CAPT/PEAO` = 15000/30000/16384/8192 |

O documento `DESMONTAGEM.md` subestimava o estado do motor. Isso é bom, e é
honesto dizê-lo antes de listar o que falta.

---

## Divergência 1 — a penalização da idade na TT: **4 contra 3**

**Binário**, `420378`:

```
420378:  lea  0x0(,%r14,4),%ebx   ; idade * 4
420380:  sub  %ebx,%r14d          ; idade - idade*4  =  -3 * idade
420383:  add  %r12d,%r14d         ; + profundidade
```

`nota = prof − 3 × idade`. O `×3` está escrito como `x − 4x` para evitar uma
multiplicação, e é fácil ler o `4` e ficar por aí.

**Atual**, `tt.cpp:38` e `tt.cpp:97`:

```cpp
constexpr int PENA_VELHA = 4;
int nota = prof_de(dados) - PENA_VELHA * velhice;
```

**Efeito:** uma entrada com uma geração de idade perde 4 plies de valor em vez
de 3. Ao fim de dez gerações, 40 em vez de 30. O motor atual **deita fora
entradas antigas mais depressa** do que o motor perdido.

Não muda nenhuma busca isolada; muda o que a tabela contém, e portanto a
árvore em todas as iterações seguintes.

---

## Divergência 2 — o limiar do `guarda_so_aval`: **−128 contra −7**

**Binário**, `42056d`:

```
42056d:  cmp  $0xf9,%r8b     ; -7
420571:  jl   42057f         ; prof < -7  ->  a via serve
```

**Atual**, `tt.cpp:182`:

```cpp
if (dados == 0 || prof_de(dados) <= PROF_SO_AVAL || velhice >= PENA_VELHA)
```

com `PROF_SO_AVAL = -128`.

**Efeito:** o binário aceita sacrificar qualquer via com profundidade abaixo
de −7; o atual só aceita a que tenha exactamente −128. O critério do meio
deixa praticamente de existir, e a escolha passa a depender só de "vazia" ou
"velha".

---

## Divergência 3 — a profundidade gravada pelo `guarda_so_aval`: **−128 contra −8**

**Binário**, `4205be`:

```
4205be:  movabs $0x3f800000000,%rax
4205c8:  or     %rax,%rsi
```

Desempacotado: bits 32-39 = `0xf8` = **−8**, bits 40-41 = **3** (`Nenhum`).

**Atual**, `tt.cpp:197`:

```cpp
empacota(PROF_SO_AVAL, 0, Limite::Nenhum, antigo, false, aval)   // -128
```

**Efeito:** as entradas de só-avaliação do motor perdido ficam com
profundidade **−8**, um ply abaixo do limiar de −7 que as torna
substituíveis. É um desenho coerente: escreve-se a −8 para que a entrada
seguinte a possa deitar fora imediatamente.

Com −128 essa relação quebra-se, e junto com a divergência 2 o resultado é que
**as entradas de só-avaliação do motor atual são praticamente imortais** —
nenhuma outra as considera substituíveis pelo critério da profundidade.

**As três divergências da TT compõem-se:** entradas normais expiram mais
depressa (pena 4), e as de só-avaliação nunca expiram (−128 contra −8/−7). A
tabela do motor atual enche-se de avaliações e esvazia-se de buscas.

---

## Divergência 4 — a via vazia: curto-circuito que o binário não tem

**Atual**, `tt.cpp:93`:

```cpp
if (dados == 0)
    return i;          // devolve a PRIMEIRA vazia
```

**Binário**: não há. O ciclo de `420348` a `42039d` percorre **sempre as três
vias** e escolhe a de menor `prof − 3×idade`. Uma via vazia dá
`0 − 3×idade`, que compete com as outras como qualquer outra.

**Efeito:** quando há uma via vazia *e* outra muito velha, o binário pode
escolher a velha (se `prof − 3×idade` for menor) e o atual escolhe sempre a
vazia. Divergem no conteúdo da tabela.

---

## Divergência 5 — o enumerado `Limite` está invertido

| valor | binário (`436e5b`) | atual (`tt.h:16`) |
|---:|---|---|
| 0 | **Exacto** | Nenhum |
| 1 | Inferior | Inferior |
| 2 | Superior | Superior |
| 3 | **Nenhum** | Exacto |

As duas pontas estão trocadas.

**Isto só importa se o valor numérico escapar** — dentro de um motor fechado,
o enumerado é arbitrário desde que usado de forma coerente. Verifiquei: o
`guarda_so_aval` atual usa `Limite::Nenhum` simbolicamente, o que é o
comportamento certo.

**Mas tem uma consequência real:** as duas tabelas não são compatíveis. Uma
entrada gravada pelo `ks_1.20260919` lida pelo motor atual — ou uma tabela
partilhada entre os dois, ou um ficheiro de TT — troca `Exacto` por `Nenhum`.
Fica registado como incompatibilidade de formato, não como defeito de busca.

---

## O que falta verificar

Comparei a TT, as continuações, o RFP, a poda por histórico e a tabela LMR.
Faltam:

- a ordenação: selecção com `cmovg` **estrito** (empates pela ordem de
  geração) contra o que o atual faz
- `pilha_peca` / `pilha_para` não limpos no `undo`
- a assimetria do `tt_sup` na `pontua` (constante num ramo, manete no outro)
- o ramo morto da `reducao` (`a6 = 1` cravado nos dois chamadores)
- os canais 3, 4 e 5 da história de correcção
- os 31 contadores e os 18 relatórios
# A divergência que sobressai: a chave da repetição

## O que o binário faz

Em **seis sítios** — `hist_de` não, mas `negamax` (`436914`), `quiescencia`
(`42132b`), `repete_ja` (`41f3e2`), o lance nulo (`43a122`), o ciclo de lances
(`4382a2`) e a entrada (`436e9f`) — a chave é **amassada com o contador dos
cinquenta lances antes de ser empilhada e antes de ser comparada**:

```cpp
uint64_t amassa(uint64_t chave, int c) {
    if (c <= 13) return chave;
    return chave ^ (((c - 14) >> 3) * 0x5851f42d4c957f2dULL
                    + 0x14057b7ef767814fULL);
}
```

E a comparação usa o mesmo amassamento dos dois lados (`436982`:
`cmp %r14,(%rdi,%r12,8)` com `r14` calculado em `436914`).

## O que o motor atual faz

`busca.cpp`, nos cinco sítios (`1052`, `1355`, `1442`, `1711`, `2058`):

```cpp
chaves.push_back(pos.key());          // chave CRUA
```

e em `repeticao()` (`923`):

```cpp
Key k = pos.key();                    // chave CRUA
for (int i = ...; i -= 2)
    if (chaves[i] == k) return true;
```

**Não há amassamento.** A função `mistura()` existe (`254`) e é o splitmix64
usado nos índices da correcção, mas é outra coisa: não tem o contador dos
cinquenta lances e usa constantes diferentes.

## A consequência, e é grande

No binário, duas ocorrências da mesma posição só se reconhecem como repetição
se estiverem **no mesmo escalão de oito** do contador dos cinquenta lances,
acima de 13:

| `rule50` | escalão `(c−14)>>3` |
|---|---|
| ≤ 13 | sem amassamento — comparam-se cruas |
| 14–21 | 0 |
| 22–29 | 1 |
| 30–37 | 2 |

Uma repetição a quatro plies de distância fica quase sempre no mesmo escalão
e é detectada. Uma a doze ou mais plies cai num escalão diferente e **o
binário não a vê**.

O motor atual vê-as todas.

## Qual dos dois está certo?

**Não sei, e não é a minha decisão.** O que sei é que divergem, e que a
divergência é exactamente do tipo que o critério "árvore idêntica ao nó"
apanharia — mas só em posições onde há ciclos longos, que não são as seis
posições do teste de profundidade 12.

Duas hipóteses, e as duas são plausíveis:

1. **É deliberado.** Uma posição repetida com o contador dos cinquenta lances
   muito diferente não é equivalente para efeitos de busca: uma está mais
   perto do empate do que a outra. Amassar o escalão no valor da chave é uma
   maneira barata de o exprimir, e o mesmo amassamento serve a cache das
   ameaças e a dos índices.

2. **É um defeito.** O amassamento foi feito para as caches — onde o escalão
   dos cinquenta lances *deve* separar entradas — e acabou aplicado também à
   pilha da repetição por partilharem a mesma função embutida. Nesse caso o
   motor perdido perdia repetições longas.

A favor da segunda: a `repete_ja` (`0x41f3a0`), que existe **só** para
responder "se eu jogar isto, repito?", usa o mesmo amassamento. Uma função
dedicada à repetição a usar um filtro que a enfraquece é estranho.

A favor da primeira: são seis sítios, todos coerentes, incluindo o do lance
nulo — que é um caminho onde um erro destes teria sido notado.

## O que isto vale medir

É a única divergência encontrada que muda o **resultado** de posições, e não
só o conteúdo da tabela. Merece um teste próprio: posições com ciclos de doze
ou mais plies, onde os dois motores devem discordar sobre se há empate.

Se o `ks_1.20260919` declarar vitória onde o atual declara empate — ou
vice-versa — a questão fica respondida sem depender de interpretação.


---

# Correcção aplicada: a política de substituição da tabela de transposição

`src/tt.cpp`, quatro alterações. Cópia de segurança em
`src/tt.cpp.bak_20260922_2030`. Compila sem avisos.

## O que mudou

```diff
-constexpr int PROF_SO_AVAL = -128;
-constexpr int PENA_VELHA   = 4;
+constexpr int PROF_SO_AVAL = -8;
+constexpr int PROF_DESCART = -7;
+constexpr int PENA_VELHA   = 3;
```

```diff
 int velhice = ...;
-if (dados == 0)
-    return i;
 int nota = prof_de(dados) - PENA_VELHA * velhice;
```

```diff
-if (dados == 0 || prof_de(dados) <= PROF_SO_AVAL || velhice >= PENA_VELHA)
+if (dados == 0 || prof_de(dados) < PROF_DESCART || velhice > 2)
```

## Porquê cada uma

**`PENA_VELHA` 4 → 3.** O binário faz `prof − 3 × idade`, escrito em `420378`
como `lea (,%r14,4)` seguido de `sub` — isto é, `x − 4x = −3x`. É uma forma de
evitar um `imul`, e o `4` que se lê na instrução não é o peso.

**`PROF_SO_AVAL` −128 → −8.** A `guarda_so_aval` do binário grava a máscara
`0x3f800000000` (`4205be`), cujos bits 32-39 valem `0xf8` = **−8**.

**`PROF_DESCART` = −7, novo.** O limiar de descarte é `prof < −7`
(`42056d`: `cmp $0xf9,%r8b` seguido de `jl`), e era o mesmo `−128` que servia
de marca *e* de limiar. São dois números distintos no binário.

**O `velhice >= PENA_VELHA` passou a `velhice > 2`.** Numericamente dava o
mesmo depois da mudança do `PENA_VELHA`, mas são conceitos diferentes que por
acaso partilham o `3`: o peso da idade na nota, e o limiar de idade para
descarte. O binário tem os dois separados (`420579`: `cmp $0x2` / `jbe` para
**recusar**).

**O curto-circuito da via vazia foi removido.** O ciclo do binário
(`420348`…`42039d`) percorre sempre as três vias; uma via vazia dá
`0 − 3 × idade` e compete como qualquer outra. Devolver a primeira vazia
diverge quando há outra com nota ainda mais baixa.

## O desenho que se recupera

As três constantes são um sistema, não valores soltos:

```
guarda_so_aval grava prof = -8
                          ^
                          |  um ply abaixo
                          v
a via seguinte descarta se prof < -7
```

Uma entrada de só-avaliação **nasce já descartável**. Com `-128` nos dois
sítios essa relação quebrava-se, e o efeito somava-se ao da pena de 4:

| | antes | agora |
|---|---|---|
| entradas normais | expiravam a 4 plies por geração | 3, como o original |
| entradas de só-avaliação | praticamente imortais | descartáveis à primeira |

A tabela estava a encher-se de avaliações e a esvaziar-se de buscas.

## O que isto não corrige

A quinta divergência — o enumerado `Limite` invertido nas pontas — **não foi
tocada**. Dentro do motor é usada simbolicamente e o comportamento está certo;
mudá-la só criaria risco. Fica registada como incompatibilidade de formato
entre as duas tabelas, não como defeito.

## Medir

Nenhuma destas mudanças altera o resultado de uma busca isolada. Todas
alteram o **conteúdo da tabela**, e portanto a árvore a partir da segunda
iteração. O teste próprio é o `go depth 12` nas seis posições: a contagem de
nós deve mexer, e é isso que confirma que a mudança chegou ao motor.

Se a contagem **não** mexer, é sinal de que estas vias nunca são escolhidas —
e aí o que há a investigar é porquê.


---

# A integração dos threads no `ks_1.20260919`

Como a encontrei, e o que diz.

## Os símbolos que a denunciam

A tabela de símbolos tem tudo, com os nomes em português:

| símbolo | endereço | bytes |
|---|---|---:|
| `g_threads` | `0x66c150` (`.data`) | 8 |
| `g_aj` — o vector dos ajudantes | `0x7594bf0` (`.bss`) | 24 |
| `g_aj_vivas` — quantas estão a correr | `0x7594be0` | 8 |
| `para_ajudantes()` | `0x421ac0` | 644 |
| `soma_nos_ajudantes()` | `0x4205e0` | 423 |
| `Busca::nos_das_ajudantes` | `0x7594ca8` | 8 |
| `garante_ajudantes(unsigned long)::{lambda}` | `0x422200` | 438 |
| `arranca_ajudantes(Limites const&)::{lambda}` | `0x446550` | — |
| `Ajudante` (struct, via `unique_ptr`) | — | ≥ `0x318` |

**Foi por aqui que comecei:** `Busca::nos_das_ajudantes` com 8 bytes era a
única pista na primeira passagem deste documento, e a nota dizia *"é prova de
que o motor perdido tinha SMP com contagem de nós dos ajudantes"*. É mais do
que isso — está tudo lá.

## A opção UCI — `44cd47`

```
44cd47:  mov    0x110(%rsp),%rdi
44cd5c:  call   strtol
44cd61:  test   %eax,%eax
44cd63:  cmovle %r14d,%eax        ; r14 = 1  ->  min = 1
44cd6f:  cltq
44cd71:  mov    %rax,g_threads
```

```cpp
long n = strtol(valor, nullptr, 10);
if (n <= 0) n = 1;                    // cmovle, sem tecto
g_threads = n;
```

**Não há tecto.** `Threads` aceita qualquer valor positivo; só o zero e os
negativos são corrigidos para 1. Nenhuma verificação contra o número de
núcleos.

## O arranque — `44659c`

```
44659c:  mov  g_threads(%rip),%rdx
4465bf:  cmp  $0x1,%rdx
4465c3:  jbe  448ae8              ; <= 1  ->  caminho SEM ajudantes
```

Com `Threads <= 1` salta directamente para `448ae8`, que prepara os mesmos
três globais (`g_aval`, `g_pos`, `g_busca`) e cai no mesmo `arranca`. **O
caminho de um thread não toca no código dos ajudantes de todo** — não há
`garante_ajudantes`, não há vector, não há sincronização.

Com `Threads > 1` passa pelo bloco de `4465c9` a `4465ee`, que lê
`g_aval+0x6ede708` (a bandeira de rede carregada) e só então chama o
`arranca`.

## A paragem — `446610`

```
44660b:  call  Busca::arranca
446610:  call  para_ajudantes
```

`para_ajudantes` é chamada **incondicionalmente** logo a seguir ao `arranca`,
nos dois caminhos. Com um thread não há nada a parar e a função sai depressa.

Só há **dois** sítios que a chamam: `446610` (fim do `go`) e `44a97b`.

## A soma dos nós — `0x4205e0`, e não é chamada

```
4205e4:  mov  g_aj+0x8(%rip),%rsi   ; fim do vector
4205eb:  sub  g_aj(%rip),%rsi       ; - inicio
4205f2:  mov  g_aj_vivas(%rip),%rdx
4205f9:  sar  $0x3,%rsi             ; /8 -> quantos ponteiros
4205fd:  cmp  %rsi,%rdx
420600:  cmovbe %rdx,%rsi           ; min(vivas, tamanho)
4206e0:  mov  0x310(%rcx),%r11      ; ajudante->nos
4206e7:  add  %r9,%r11
```

```cpp
uint64_t soma_nos_ajudantes() {
    size_t n = std::min(g_aj_vivas, g_aj.size());
    uint64_t t = 0;
    for (size_t i = 0; i < n; ++i)
        t += g_aj[i]->nos;          // Ajudante+0x310
    return t;
}
```

Desenrolada 8×, com a escada de `cmp $0..7` habitual.

**O campo dos nós de cada ajudante está em `Ajudante+0x310`.** Como a `Busca`
tem `nos` em `0x280`, o `Ajudante` **não é** uma `Busca` — é outra estrutura,
com o contador noutro sítio.

**E não há um único `call` para ela.** O único sítio que a menciona é
`449482`:

```
449482:  lea -0x28ea9(%rip),%r11    # 4205e0 <soma_nos_ajudantes>
```

Um `lea` — **toma o endereço**, não chama. Ou é passada como ponteiro de
função, ou é o compilador a preparar um salto indirecto.

## O que isto significa para a reconstrução

**O SMP está inteiro no binário, e o caminho de um thread é separado.** Não é
um motor de um thread com ajudantes por cima: são dois caminhos distintos a
partir de `4465bf`, e o de um thread não paga nada.

Três coisas a fixar na reconstrução:

1. **`Threads` não tem tecto** — só o mínimo de 1.
2. **`para_ajudantes` é chamada sempre**, mesmo com um thread.
3. **O `Ajudante` é uma estrutura própria**, com os nós em `+0x310`, e pelo
   menos `0x318` bytes. Não é uma `Busca`.

## O que fica por ler

O corpo de `garante_ajudantes` (`0x422200`, 438 bytes) e o lambda de
`arranca_ajudantes` (`0x446550`) — que é onde se vê **o que cada ajudante faz
de diferente**: se varia a profundidade, se varia a ordem dos lances, ou se é
Lazy SMP puro a partilhar só a tabela.

Essa é a pergunta que interessa, e é a próxima.


---

# `garante_ajudantes` e `arranca_ajudantes` — o SMP, por dentro

## `arranca_ajudantes`, o lambda: 40 bytes, e diz tudo

```
446554:  mov  (%rdi),%rdx          ; a captura: {Ajudante*, Limites*}
446557:  mov  (%rdx),%rdi          ; base do Ajudante
44655a:  add  $0x8,%rdx            ; -> Limites
44655e:  lea  0x47fc0(%rdi),%rcx   ; Avaliador
446565:  lea  0x6f26700(%rdi),%rsi ; Position
44656c:  add  $0x88,%rdi           ; Busca
446573:  jmp  43e630 <Busca::arranca>
```

```cpp
[aj, &lim]() {
    aj->busca.arranca(aj->pos, lim, aj->aval);   // chamada de cauda
}
```

**Cada ajudante corre o `arranca` inteiro**, com a sua **própria** `Busca`,
o seu **próprio** `Avaliador` e a sua **própria** `Position`. Não é um
trabalhador especializado: é uma instância completa do motor.

## A estrutura `Ajudante`, pela aritmética

| desl. | conteúdo | tamanho |
|---|---|---:|
| `+0x000` | mutex (`0x08`), condvar (`0x30`), bandeiras (`0x60`, `0x61`), `std::function` (`0x68`) | 136 |
| `+0x088` | **`Busca`** | 294.696 |
| `+0x47fc0` | **`Avaliador`** | 116.254.528 |
| `+0x6f26700` | **`Position`** | |

Os limites fecham ao byte: `0x88 + 0x47f28 = 0x47fb0`, e o `Avaliador` começa
em `0x47fc0` — 16 bytes de alinhamento. `0x47fc0 + 0x6ede740 = 0x6f26700`,
que é exactamente onde a `Position` começa — folga zero.

**Cada ajudante custa 111 MB**, quase tudo por causa do `Avaliador` com as
caches dos acumuladores da rede.

`Threads=4` são **333 MB** de ajudantes, além do motor principal e da tabela
de transposição. `Threads=16` são 1,7 GB. Não há tecto na opção (`44cd61`:
só o mínimo de 1), portanto nada impede de pedir mais do que a máquina tem.

## O contador de nós, e porque é que o `nos_288` existia

`soma_nos_ajudantes` lê `Ajudante+0x310`. Como a `Busca` começa em `+0x88`:

```
0x310 − 0x88 = 0x288
```

E `Busca+0x288` é o campo que eu tinha catalogado como `nos_288` sem lhe
saber a função — o que a `sem_tempo` escreve em `420278`, **de 1024 em 1024
nós**:

```
420253:  test $0x3ff,%r8d
42025a:  je   420278
420278:  mov  %r8,0x288(%rdi)     ; instantaneo dos nos
```

**É deliberado.** O principal lê a contagem dos ajudantes sem tocar no
contador quente `0x280` — lê um instantâneo actualizado de mil em mil nós.
Evita a contenção de cache que a leitura do contador real causaria.

Duas leituras independentes que se fecham: o campo existia sem explicação, e
a explicação estava no SMP.

## `garante_ajudantes`, o ciclo do trabalhador — `0x422200`

```
422230:  call pthread_mutex_lock        ; aj->mutex
422249:  movb $0x0,0x60(%rbx)           ; "estou livre"
422255:  call condition_variable::notify_all
42225a:  cmpb $0x0,0x60(%rbx)           ; ha' trabalho?
422334:  call condition_variable::wait  ; nao -> dorme
422264:  cmpb $0x0,0x61(%rbx)           ; sair?
42226e:  mov  0x80(%rbx),%rax           ; a std::function
4222e5:  call *0x28(%rsp)               ; corre-a  -> arranca_ajudantes
422307:  jmp  422230                    ; e volta a dormir
```

Ciclo clássico: bandeira `0x60` para "tem trabalho", `0x61` para "sair",
mutex e variável de condição. Os ajudantes **nascem uma vez e ficam vivos**
entre buscas — `para_ajudantes` não os destrói, põe-nos a dormir.

## O achado: **não há diversificação nenhuma**

Isto é o que interessa à tua pergunta sobre os Elos em falta.

O lambda passa ao `arranca` **os mesmos `Limites`**, sem modificação. Não há:

- deslocamento de profundidade por ajudante
- baralhamento da ordem dos lances
- janela de aspiração diferente
- semente de aleatoriedade por thread
- índice de thread guardado em lado nenhum da estrutura

Procurei um identificador de ajudante no cabeçalho de 136 bytes e **não
existe**. As únicas coisas antes da `Busca` são o mutex, a condvar, duas
bandeiras e a `std::function`.

**É Lazy SMP puro.** Todos os ajudantes correm a busca idêntica, com tabelas
de histórico próprias e separadas, e divergem **só** pelas corridas na tabela
de transposição partilhada.

## O que isto vale saber

É um desenho legítimo — a referência faz o mesmo — mas tem duas
consequências que convém ter presentes:

1. **Os históricos não são partilhados.** Cada ajudante aprende sozinho, e o
   que aprende morre com a busca. Só a TT atravessa.

2. **A divergência depende inteiramente da TT.** Se a tabela for grande de
   mais para a busca — como no teste de profundidade 12, onde cinco das seis
   posições deram contagem idêntica antes e depois de lhe mexer —, os
   ajudantes fazem trabalho **repetido** em vez de trabalho **diferente**.

O segundo ponto liga-se ao que mediste: um motor que está uns Elos atrás com
vários threads, e cuja tabela nunca enche, pode estar a pagar N threads para
fazer quase uma busca só.

**Vale medir:** `Threads=4` com `Hash=16` contra `Threads=4` com `Hash=256`.
Se o Hash pequeno for melhor, os ajudantes estavam a duplicar trabalho.


---

# Correcção: **há votação**, e eu tinha-a dado como inexistente

Na secção anterior escrevi que os ajudantes correm a busca idêntica e que a
divergência vem só das corridas na tabela. O João corrigiu-me: *"normalmente
cada thread atinge uma jogada e é votada no final"*. **Está certo, e está no
binário.** Fui procurá-la e encontrei-a.

## Onde estava

Logo a seguir a `para_ajudantes`, em `44664d`. Eu tinha visto o bloco e
tomei-o por limpeza de fim de busca.

## A recolha — `44664d`…`446b3b`

```
44664d:  mov  g_aj(%rip),%r15              ; inicio do vector
446654:  mov  g_aj+0x8(%rip),%r13          ; fim
4466c3:  movzwl 0x47b70(%rax),%edx         ; Ajudante+0x47b70
4466ca:  test %dx,%dx  / je                ;   lance vazio -> salta
4466cf:  mov  0x2fc(%rax),%esi             ; Ajudante+0x2fc
4466d5:  test %esi,%esi / jg               ;   tem de ser > 0
449b6c:  mov  0x47b74(%rax),%ecx           ; Ajudante+0x47b74
449bc6:  call vector<pair<Move,int>>::_M_realloc_insert
```

Convertendo para deslocamentos da `Busca` (que começa em `Ajudante+0x88`):

| `Ajudante` | `Busca` | campo |
|---|---|---|
| `0x47b70` | `0x47ae8` | **o melhor lance da raiz** |
| `0x47b74` | `0x47aec` | **o valor** |
| `0x2fc` | `0x274` | guarda: tem de ser `> 0` |

Constrói-se um **`vector<pair<Move,int>>`** com um par por ajudante que tenha
lance e passe a guarda. O principal entra também (`446b53` lê
`g_busca+0x47ae8`).

Isto confirma, do outro lado, o campo `0x47ae8` que eu já tinha catalogado
como "melhor lance da raiz, escrito no fim de cada iteração" — é exactamente
o que a votação lê.

## O mínimo — `446b83`…`446d92`

```
446b83:  mov  $0x7fffffff,%r9d
446c09:  vpminsd %ymm12,%ymm0,%ymm0
```

Inicializa a `INT_MAX` e faz `vpminsd` sobre o campo `int` de todos os pares,
vectorizado com AVX2 e desenrolado. **É o mínimo dos valores de todos os
threads.**

## O peso — `448a6e`

```
448a6e:  vmovd  0x4(%r14),%xmm7     ; o valor deste voto
448a74:  vpsubd %xmm0,%xmm7,%xmm8   ; - o minimo
448a7d:  movslq %r8d,%r9
448a80:  add    $0x18,%r9           ; + 24
448a8d:  add    %r9,0x8(%rax)       ; acumula no total do lance
```

```cpp
const int minimo = *min_element(votos, por valor);
for (auto& [lance, valor] : votos)
    peso[lance] += (valor - minimo) + 24;
```

**O peso de cada voto é `(valor − mínimo) + 24`**, e acumula-se por lance. O
`24` é o piso: mesmo o pior thread contribui 24 para o seu lance, portanto a
contagem de votos conta, não só a diferença de avaliação.

O `cmp 0x10(%rax)` / `0x20` / `0x30` em `446f01`..`446f17` é a procura do
lance na tabela dos pesos, desenrolada — entradas de 16 bytes.

## Como corrigir o que escrevi

A secção `AJUDANTES.md` diz *"não há diversificação nenhuma"*. Isso
**mantém-se** — verifiquei outra vez e o lambda passa os mesmos `Limites`,
não há índice de thread, não há deslocamento de profundidade.

Mas a conclusão que tirei daí — que os ajudantes fazem trabalho repetido — era
precipitada por um segundo motivo, além da votação:

**A votação torna o trabalho repetido útil.** Se N threads convergem no mesmo
lance, o peso acumulado confirma-o; se divergem, o peso escolhe entre eles.
Não é desperdício: é amostragem. Continua a depender da TT para divergirem,
mas a votação extrai valor da divergência que houver.

## O que fica por verificar — a partilha dos históricos

O João diz também que *"a tabela de histórico é partilhada entre threads"*.
Ainda não o confirmei, e é a pergunta certa a seguir.

O que sei:

- cada `Ajudante` tem a sua **própria `Busca`** de 294.696 bytes, e a `Busca`
  contém a história dos quietos **em linha** (`0x43b0`, 128 KB) — essa é
  necessariamente por thread;
- mas as tabelas grandes — continuações, peões, correcção, capturas — são
  **vectores**, isto é, ponteiros. Se os ponteiros dos ajudantes apontarem
  para a mesma memória do principal, essas **são** partilhadas.

E há uma pista forte: a assinatura do `do_move` no binário é

```
Position::do_move(Move, StateInfo&, bool, Dirties&,
                  TranspositionTable const*, SharedHistories const*)
```

**`SharedHistories`** — o tipo existe e tem "partilhadas" no nome. Falta ver o
que lá está dentro e quem o preenche.


---

# Os históricos **são** partilhados — e o mecanismo é uma indirecção

O João disse: *"a tabela de histórico é partilhada entre threads"*. Está
certo, e eu tinha-o negado. O mecanismo é uma **segunda camada de ponteiros**
que eu tinha catalogado mal.

## O erro que eu tinha no mapa

Na secção das alocações escrevi que `0x28f78`, `0x28f98` e `0x28fb8` eram *"o
terceiro campo de cada vector — a capacidade"*. **Não são.** São **ponteiros
para vectores**.

A `Busca` tem, para cada história grande, **duas coisas**:

| | o vector, que POSSUI | o ponteiro, que DIZ QUAL USAR |
|---|---|---|
| continuações | `0x28f60` (begin/end/cap) | **`0x28f78`** |
| peões | `0x28f80` | **`0x28f98`** |
| correcção | `0x28fa0` | **`0x28fb8`** |

E a `hist_de` lê **o ponteiro**, não o vector:

```
412090:  mov  0x28f98(%rbx),%r9     ; o ponteiro
412097:  test %r9,%r9  / je         ; nulo -> sem historia de peoes
42109c:  mov  (%r9),%rcx            ; ->begin
42109f:  cmp  %rcx,0x8(%r9)         ; ->end
```

Duas desreferências. Eu tinha lido isto e escrito `mov 0x28f98(%rbx),%r9` sem
reparar que o `(%r9)` a seguir é uma segunda indirecção.

## O que o criador de ajudantes faz — `449513`, `449563`, `4494d5`

Depois do `operator new` de 111 MB e do `memset` a zero, os **vectores
próprios** do ajudante ficam nulos (`447977: vpxor %xmm7,%xmm7,%xmm7` seguido
dos três `vmovdqu %ymm7` em `447a47`…`447a59`).

E depois os **ponteiros** são postos a apontar para os vectores do principal:

```
449513:  lea  0x24db26(%rip),%rsi   # 697040 <g_busca+0x28f80>   <- peoes DO PRINCIPAL
44951e:  mov  %rsi,0x29020(%rdx)    ; Ajudante+0x29020 = Busca+0x28f98

449563:  lea  0x24daf6(%rip),%rbx   # 697060 <g_busca+0x28fa0>   <- correccao DO PRINCIPAL
449576:  mov  %rbx,0x29040(%rdx)    ; Ajudante+0x29040 = Busca+0x28fb8

4494d5:  mov  %rax,0x29000(%rdx)    ; Ajudante+0x29000 = Busca+0x28f78  <- continuacoes
```

`Ajudante + 0x88 = Busca`, portanto `0x29020 − 0x88 = 0x28f98` ✔,
`0x29040 − 0x88 = 0x28fb8` ✔, `0x29000 − 0x88 = 0x28f78` ✔.

**Os três ponteiros do ajudante apontam para os três vectores do
`g_busca`.** E, ao mesmo tempo, os vectores próprios do ajudante são
libertados (`_ZdlPvm` em `4494ff`, `44954f`, `4495a4`) e postos a zero.

## O desenho, agora inteiro

| tabela | tamanho | por thread ou partilhada? |
|---|---:|---|
| história dos quietos `hist[2][64][64][4]` | 128 KB | **em linha na `Busca` — POR THREAD** |
| tabela `[6][64]` (`pst`) | 1,5 KB | em linha — **por thread** |
| tabela LMR | 16 KB | em linha — por thread |
| capturas | 9 KB | vector próprio — a confirmar |
| plies rasos | 80 KB | vector próprio — a confirmar |
| **continuações** | 2,8 MB | **PARTILHADA** |
| **peões** | 25 MB | **PARTILHADA** |
| **correcção** | 786 KB | **PARTILHADA** |

Faz sentido económico: as três partilhadas somam **28 MB**; replicá-las por
thread seria proibitivo. As pequenas ficam em linha e são baratas de duplicar.

E explica o `111 MB` por ajudante: quase tudo é o `Avaliador` (116 MB com as
caches da rede), não os históricos.

## Corrige duas coisas que escrevi

**1.** Os `0x28f78`/`0x28f98`/`0x28fb8` não são capacidades — são os ponteiros
que tornam a partilha possível. O `busca.h` reconstruído tem isto errado.

**2.** A secção `AJUDANTES.md` diz *"Os históricos não são partilhados. Cada
ajudante aprende sozinho, e o que aprende morre com a busca."* **Falso para as
três grandes.** O que cada ajudante aprende nas continuações, nos peões e na
correcção fica para todos, e sobrevive à busca.

Isso muda a leitura do SMP: não é Lazy SMP com threads isolados. É Lazy SMP
**com históricos partilhados e voto no fim** — os ajudantes cooperam por três
canais (TT, históricos grandes, voto), não por um.

## O que isto pode valer nos Elos

Se a reconstrução replicou os históricos por thread em vez de os partilhar,
perde-se exactamente a cooperação que o motor perdido tinha. Com quatro
threads, quatro motores a aprender separadamente aprendem cada um um quarto do
que o original aprendia em conjunto.

**É verificável sem partidas:** correr `Threads=4` e comparar a contagem de
nós à mesma profundidade. Se o original converge mais depressa, é porque os
históricos partilhados estão a fazer efeito.


---

# Divergências no SMP

Comparação feita a seguir à leitura do `garante_ajudantes`, do
`arranca_ajudantes`, da votação e da partilha dos históricos.

## O que está igual

| ponto | estado |
|---|---|
| votação: `peso = nota − menor + 24` | ✅ idêntica, constante incluída |
| dois fios no mesmo lance somam os pesos | ✅ |
| históricos grandes partilhados por ponteiro | ✅ mesmo desenho |
| históricos pequenos em linha, por thread | ✅ |
| nenhum desvio de profundidade por ajudante | ✅ |

A votação do atual é literalmente a mesma fórmula, com o mesmo piso de 24.

---

## Divergência 1 — **o filtro de profundidade dos votantes**

**Atual**, `busca.cpp`:

```cpp
for (auto& b : ajudantes)
    if (b->melhor_raiz != Move::none() && b->ultima_prof + 2 >= ultima_prof)
        cand.emplace_back(b->melhor_raiz, b->nota_raiz);
```

Um ajudante que tenha ficado **mais de dois plies atrás do principal não
vota**.

**Binário**, `4466c3`…`4466d7`:

```
4466c3:  movzwl 0x47b70(%rax),%edx   ; o lance
4466ca:  test   %dx,%dx  / je        ; vazio -> salta
4466cf:  mov    0x2fc(%rax),%esi     ; Busca+0x274 = ultima profundidade
4466d5:  test   %esi,%esi
4466d7:  jg     449b6c               ; > 0  ->  VOTA
```

`Busca+0x274` é escrito com a profundidade no fim de cada iteração
(`440733: mov %r13d,0x274(%rdi)`), inicializado a zero (`43fa92`) e posto a 1
num caminho de saída (`440ab3`).

**O teste é `> 0`, e mais nada.** Procurei uma comparação entre a
profundidade do ajudante e a do principal em todo o binário:

```
grep "cmp.*0x2fc(%r" -> vazio
grep "cmp.*0x274(%r" -> vazio
```

**Não existe.** No motor perdido, **qualquer ajudante que tenha completado
pelo menos uma iteração vota**, por muito atrás que esteja.

### Porque é que isto importa

O comentário do atual defende o filtro com um argumento que soa bem:

> *"Um fio que o relogio apanhou na 12 nao esta' a ver o que o principal ve na
> 17, e deixa-lo votar e' decidir com menos informacao."*

É uma dedução razoável — e o original não a fazia. O piso de 24 existe
precisamente para que um fio atrasado **continue a contar**, ainda que pouco:
se a sua nota for muito inferior, o peso `nota − menor + 24` dá-lhe quase só o
piso, e ele entra como **um voto de contagem**, não como um voto de força.

O filtro dos dois plies elimina esse voto por completo. Com três ajudantes, se
um ficar atrasado, a votação passa de quatro vozes a três — e o desempate por
contagem, que é onde o método tem força, perde uma amostra.

**É a divergência mais clara do SMP, e é uma dedução da reconstrução.**

---

## Divergência 2 — **o tecto do `Threads`**

**Atual**, `uci_laco.cpp:36`:

```cpp
const int FIOS_MAX = std::max(1, int(std::thread::hardware_concurrency()));
...
"option name Threads type spin default 1 min 1 max " << FIOS_MAX
```

**Binário**, `44cd61`:

```
44cd61:  test   %eax,%eax
44cd63:  cmovle %r14d,%eax     ; r14 = 1
44cd71:  mov    %rax,g_threads
```

Só o mínimo de 1. **Sem tecto nenhum.**

Esta é benigna — anunciar um `max` no `uciok` é melhor prática e não muda a
busca quando se pede um número sensato. Fica registada por ser uma diferença
real de comportamento face a um árbitro que peça mais fios do que há núcleos.

---

## Divergência 3 — **a contagem de nós dos ajudantes**

O binário tem:

- `soma_nos_ajudantes()` (`0x4205e0`, 423 bytes), que soma `Ajudante+0x310` de
  todos os ajudantes vivos — isto é, `Busca+0x288`, o instantâneo que a
  `sem_tempo` escreve de 1024 em 1024 nós;
- o global `Busca::nos_das_ajudantes` (`0x7594ca8`), escrito em `449495`.

No atual **não encontrei equivalente**: nenhuma soma dos nós dos ajudantes,
nenhum campo com esse papel.

**Consequência:** o `nodes` e o `nps` que o motor atual imprime com
`Threads > 1` contam **só o fio principal**. O original contava todos.

Não muda a árvore. Muda o que o árbitro e os testes vêem — e **muda a gestão
de tempo se o limite de nós for usado**, porque `sem_tempo` compara `nos`
contra o limite. Com N fios, o original atingia o limite N vezes mais depressa.

Vale confirmar se algum teste vosso usa `go nodes`.

---

## O que ficou por verificar

- se os ajudantes do atual partilham a **pilha de chaves** da partida
- o comportamento do `para_ajudantes` quando a busca é interrompida a meio
- se o `Ajudante` do atual reutiliza os fios entre buscas (o original mantém-nos
  vivos a dormir numa variável de condição, não os recria)


---

# Correcção aplicada: o filtro de profundidade dos votantes

`src/busca.cpp`, uma linha. Cópia de segurança em
`src/busca.cpp.bak_20260922_2218`. Compila sem avisos.

```diff
 for (auto& b : ajudantes)
-    if (b->melhor_raiz != Move::none() && b->ultima_prof + 2 >= ultima_prof)
+    if (b->melhor_raiz != Move::none() && b->ultima_prof > 0)
         cand.emplace_back(b->melhor_raiz, b->nota_raiz);
```

## A prova

`4466cf`, no `faz_go`, imediatamente antes de empilhar o voto:

```
4466c3:  movzwl 0x47b70(%rax),%edx   ; o melhor lance do ajudante
4466ca:  test   %dx,%dx  / je        ; vazio -> salta
4466cf:  mov    0x2fc(%rax),%esi     ; Ajudante+0x2fc = Busca+0x274
4466d5:  test   %esi,%esi
4466d7:  jg     449b6c               ; > 0  ->  VOTA
```

`Busca+0x274` é a última profundidade completada: escrita em `440733`
(`mov %r13d,0x274(%rdi)`) no fim de cada iteração, zerada em `43fa92`.

E não há comparação com a profundidade do principal em parte nenhuma:

```
grep "cmp.*0x274(%r"  ->  vazio
grep "cmp.*0x2fc(%r"  ->  vazio
```

## Porque é que o filtro fazia mal

O piso de 24 da `vota()` existe **precisamente** para que um fio atrasado
continue a contar. Com `peso = nota − menor + 24`, um fio muito abaixo dos
outros recebe quase só o piso: entra como **voto de contagem**, não de força.

E é a contagem que dá força ao método — o lance que vários caminhos
independentes encontraram vale mais do que o que só um encontrou. O filtro dos
dois plies deitava fora essa amostra inteira: com três ajudantes, um atrasado
fazia a votação passar de quatro vozes a três.

Os dois mecanismos estavam a trabalhar um contra o outro. O piso dizia "conta
sempre"; o filtro dizia "este não conta".

## Quando é que isto muda alguma coisa

Só com `Threads > 1`, e só quando algum ajudante fica mais de dois plies atrás
do principal — o que acontece precisamente nos controlos de tempo apertados,
onde o relógio apanha os fios em pontos diferentes.

**Não muda nada com um thread.** Os testes de profundidade fixa não a vêem.

## Medir

Partidas com `Threads=4` contra `Threads=4`, novo contra antigo. É a única
maneira: a votação não aparece em contagens de nós nem em buscas de
profundidade fixa.


---

# Comparação exaustiva dos parâmetros: binário contra atual

Comparei os **66 parâmetros** do binário que têm equivalente identificável no
motor atual, valor a valor.

## O resultado

```
comparadas: 66    diferentes: 0    sem equivalente: 7
```

**Zero divergências de valor.** Os 59 que existem nos dois lados batem todos,
incluindo os que só se percebem lendo o binário:

| | binário | atual |
|---|---:|---:|
| `rfp_m` / `rfp_margem` | 110 | 110 |
| `hp_m` / `hist_poda` | 600 | 600 |
| `seec` / `see_poda` | 70 | 70 |
| `seeq` / `see_poda_tranq` | 5 | 5 |
| `sing_margem` | 2 | 2 |
| `ext_dupla` | 40 | 40 |
| `nmp_divest` / `nmp_div_est` | 200 | 200 |
| `razor_margem` | 348 | 348 |
| `peao_ch` / `peao_chaves` | 8192 | 8192 |
| `tm_tecto` / `tm_tecto_x10` | 55 | 55 |
| `killers` / `usa_killers` | 0 | 0 |
| `pc` / `pc_prof` | 0 | 0 |
| … mais 47 | | |

O `tm_tecto = 55` merece nota: este documento regista que ler a cauda do molde
no bloco errado dava `6`. O atual tem **55**, portanto essa armadilha já tinha
sido resolvida.

## Os sete sem equivalente — quatro são só nome diferente

| binário | atual | estado |
|---|---|---|
| `corr6` | `corr6_peso = 0` | ✅ |
| `cuckoo` | `usa_cuckoo = 1` | ✅ |
| `ordem_xeque` | `ordem_xeque_f = 0` | ✅ |
| `tm_tecto` | `tm_tecto_x10 = 55` | ✅ |

## Três estão mesmo em falta

`grep KS_ASP_SF|KS_LMP_MELHORA|KS_LMR_DELTA` no `busca.cpp` → **zero
ocorrências**.

| manete | valor no molde | o que faz |
|---|---:|---|
| **`KS_ASP_SF`** | 0 | esquema alternativo de alargamento da janela de aspiração (modo 5: **troca** o esquema, não o desliga) |
| **`KS_LMP_MELHORA`** | 0 | divide por 2 o limite do LMP quando não está a melhorar |
| **`KS_LMR_DELTA`** | 0 | o termo `(a4 − a3) · lmr_delta / rootDelta` na `reducao` |

**As três valem zero no binário**, portanto a ausência **não muda o
comportamento de hoje**. Não são candidatas aos Elos em falta.

Mas pelo critério que definiste — *recuperar cada linha, esteja ou não
activa* — são três blocos de código que existem no `ks_1.20260919` e não
existem no motor atual. E o `lmr_delta` é precisamente aquele que a primeira
passagem deste documento apontou como o grande achado:

> *"O `lmr_delta` é o termo `delta/rootDelta`. Está na lista de peças em falta
> como 'sim | não usamos', a pensar que era mecanismo da referência que nunca
> tivemos. **Tivemos.**"*

Continua sem estar lá. A fórmula está reconstruída na secção `REDUCAO_REVISTA`:

```cpp
if (lmr_delta > 0)
    r -= (a4 - a3) * lmr_delta / std::max(root_delta, 1);
```

com `root_delta` em `this+0x47f20`, escrito no `arranca` em `440148`.

## O que isto diz sobre os Elos em falta

**Não estão nos parâmetros.** Sessenta e seis valores comparados, zero
diferenças. Quem afinou isto acertou.

As divergências encontradas até agora — TT (quatro), filtro de voto,
contagem de nós dos ajudantes — são todas **estruturais**, não de afinação. E
das seis posições a profundidade 12, cinco dão contagem idêntica: o motor
atual e o perdido percorrem quase a mesma árvore.

Se há Elos a faltar, é mais provável que estejam em algo que **não se vê numa
busca de profundidade fixa** — gestão de tempo, comportamento com vários fios,
ou o que o motor faz quando o relógio o apanha a meio.


---

# Divergência na gestão de tempo: a cauda da curva

## O que difere

A tabela `FALTA` — quantos lances se estima que faltam, por ply jogado.

| índice | 0 | 1 | 2 | 3 | 4 | 5 | 6 | **7** | **8** |
|---|---|---|---|---|---|---|---|---|---|
| ply | 0 | 20 | 40 | 60 | 80 | 100 | 120 | **160** | **220** |
| **binário** | 131 | 111 | 91 | 74 | 59 | 47 | 38 | **24** | **16** |
| **atual** (`tm_curva_pct=0`) | 131 | 111 | 91 | 74 | 59 | 47 | 38 | **31** | **28** |

Os sete primeiros batem ao valor. **Os dois últimos não.**

## A prova

O símbolo tem tamanho declarado:

```
00000000005ec520  0000000000000024  r  Busca::arranca(...)::FALTA
```

`0x24` = 36 bytes = **9 inteiros de 32 bits**. Uma linha, não cinco. Lidos:

```
[131, 111, 91, 74, 59, 47, 38, 24, 16]
```

E a escada de plies está no código, não numa tabela — foi por isso que a minha
primeira procura por bytes falhou e eu quase concluí que o binário não tinha
curva nenhuma:

```
446355:  cmp $0x4f,%eax   ; 79  = 80 - 1
44635e:  cmp $0x63,%eax   ; 99  = 100 - 1
446367:  cmp $0x77,%eax   ; 119 = 120 - 1
446370:  cmp $0x9f,%eax   ; 159 = 160 - 1
446392:  cmp $0xdb,%eax   ; 219 = 220 - 1
```

Mesma escada do `PLY[]` do atual. O compilador desfez a tabela em
comparações.

## O efeito, com números

No atual, a seguir à interpolação:

```cpp
lc = std::max(faltam / 2, p.tm_curva_min);      // tm_curva_min = 8
lc = std::max(lc * p.tm_curva_f / 100, 1);      // tm_curva_f  = 100
```

e o orçamento é `bolo / n`, com `n` vindo daqui. **Mais `faltam` → menos tempo
por lance.**

Ao ply 220, com 10 segundos no relógio:

| | `faltam` | `lc = faltam/2` | tempo por lance |
|---|---:|---:|---:|
| binário | 16 | 8 | **1250 ms** |
| atual | 28 | 14 | **714 ms** |

**O motor atual joga a 57% do tempo que o original gastava nos finais
longos.** Ao ply 160 a diferença é menor mas na mesma direcção: 24 contra 31,
isto é 12 contra 15 — o atual usa 80% do tempo.

E o `p.tm_curva_min = 8` é o chão: com `faltam = 16` o binário fica exactamente
no chão (`16/2 = 8`), e com `faltam = 28` o atual fica bem acima dele. O
binário foi afinado para **tocar o chão** ao ply 220; o atual nunca lá chega.

## Porque é que isto pode valer Elo

É a primeira divergência encontrada que **não se vê em nenhum teste de
profundidade fixa** — e que actua exactamente onde o teu motor está a ser
medido: partidas a tempo, nas fases longas.

Um motor que joga ao ritmo certo até ao lance 80 e depois passa a usar metade
do tempo nos finais está a deitar fora a parte da partida onde o cálculo
profundo mais decide. E é invisível a tudo o que este trabalho mediu até aqui:
contagens de nós iguais, parâmetros todos iguais, árvore igual.

## O que o atual tem a mais, e é legítimo

Cinco percentis (`tm_curva_pct`), e um termo de crescimento
(`p.tm_cresce`) para além do ply 220. O binário não tem nenhum dos dois —
uma linha só, e nada depois do último escalão.

São acrescentos posteriores, não divergências. **A divergência é o conteúdo
da linha que corresponde ao original: `24` e `16`, não `31` e `28`.**

## A correcção

```diff
-  {131, 111,  91,  74,  59,  47,  38,  31,  28},   // p50
+  {131, 111,  91,  74,  59,  47,  38,  24,  16},   // p50 — do ks_1.20260919
```

E vale a pena verificar o que o `tm_cresce` faz depois do ply 220, porque o
original não cresce: fica em 16.


---

# Correcção aplicada: a cauda da curva de tempo

`src/busca.cpp`, dois números. Compila sem avisos. Cópia de segurança em
`src/busca.cpp.bak_tempo_*`.

```diff
-  {131, 111,  91,  74,  59,  47,  38,  31,  28},   // p50
+  {131, 111,  91,  74,  59,  47,  38,  24,  16},   // p50 — do binario
```

## A verificação da interpolação, que também bate

O binário faz, em `446423`…`446442`:

```
446423:  mov  (%r8,%rcx,4),%r12d    ; FALTA[k]
446427:  mov  (%r8,%rbx,4),%edx     ; FALTA[k+1]
44642b:  sub  %r12d,%edx
44642e:  imul %edx,%eax             ; * (jogados - PLY[k])
446432:  idiv %r10d                 ; / (PLY[k+1] - PLY[k])
446435:  add  %r12d,%eax            ; + FALTA[k]
44643b:  shr $0x1f / add / sar $1   ; / 2
```

Idêntico ao atual, incluindo o `/2` que o atual faz a seguir
(`lc = max(faltam/2, tm_curva_min)`).

## E o crescimento depois do ply 220: **não existe**

Para `jogados >= 220` o binário não interpola — cai em `446399` com
`r9 = 8` cravado (`44637b: mov $0x8,%r9d`), e `8` é exactamente
`FALTA[8]/2 = 16/2`. Fica no chão, não cresce.

O atual tem `tm_cresce = 0` por omissão, portanto **também não cresce**. Sem
divergência aqui.

## O que fica igual e o que fica diferente

| | binário | atual |
|---|---|---|
| escada de plies | `{0,20,40,60,80,100,120,160,220}` | igual |
| linha p50 | `[…,38,24,16]` | **corrigida** |
| interpolação linear + `/2` | sim | igual |
| chão `tm_curva_min = 8` | sim | igual |
| factor `tm_curva_f = 100` | sim | igual |
| crescimento pós-220 | não | `tm_cresce = 0` → não |
| outros quatro percentis | **não existem** | acrescento posterior, fica |

## Nota de método — o sexto erro do mesmo tipo

Procurei a tabela por padrão de bytes (`int32` e `int16`) e não a encontrei.
Escrevi, num rascunho, que *"o binário não tem esta tabela"*. **Errado.** O
compilador desfez a escada dos plies em comparações com imediatos
(`cmp $0x4f`, `$0x63`, `$0x77`, `$0x9f`, `$0xdb`), e a tabela `FALTA` está lá,
com símbolo próprio e tamanho declarado.

Só voltei atrás porque o `KS_TM_CURVA = 1` do molde não batia certo com a
conclusão. **A ausência de um padrão não é ausência da coisa** — é o mesmo
erro do regexp que excluía a forma de leitura, do `0xffff82ff` convertido à
mão, e da comparação por mnemónicas que dava 7% de código comum entre os dois
`negamax`.

## Medir

Esta é a primeira divergência encontrada que **não aparece em nenhum teste de
profundidade fixa**. Só se vê em partidas a tempo, e sobretudo nas longas.

Um teste próprio: controlo de tempo longo o suficiente para as partidas
passarem do lance 80 — a 10+0.1 muitas acabam antes de a diferença existir.
Algo como 60+0.6, ou posições de abertura que levem a finais.


---

# Correcção: a "divergência 3 do SMP" não existe

Na secção `DIVERGENCIAS_SMP` escrevi:

> *"No atual **não encontrei equivalente**: nenhuma soma dos nós dos
> ajudantes... o `nodes` e o `nps` que o motor atual imprime com `Threads > 1`
> contam **só o fio principal**."*

**Falso.** O motor atual tem, em `busca.h`:

```cpp
std::uint64_t nos_totais() const {
    std::uint64_t t = nos;
    for (const auto& b : ajudantes)
        t += b->nos;
    return t;
}
```

e usa-a na linha do `info` (`busca.cpp:2628`):

```cpp
<< " hashfull " << p_tab->cheia() << " nodes " << nos_totais()
```

Procurei por `soma_nos`, `nos_das_ajudantes` e `nos_ajudantes` — os nomes do
binário — e não por `nos_totais`, que é o nome do atual.

## E o limite de nós também não diverge

Era o meu argumento de que a gestão de tempo mudava com `go nodes`. Não muda.

**Binário**, `sem_tempo` em `42023e`:

```
42023e:  mov  0x8(%rdi),%rsi     ; limite de nos
420242:  mov  0x280(%rdi),%r8    ; nos DESTA Busca
42024e:  cmp  %rsi,%r8
420251:  jae  420268             ; >= -> parar
```

**Atual**, `busca.cpp:941`:

```cpp
if (nos_limite > 0 && nos >= nos_limite) {
```

Os dois comparam a contagem do **próprio fio** contra o limite, não a soma.
Comportamento idêntico.

## O que sobra das três divergências do SMP

| | estado |
|---|---|
| filtro de profundidade dos votantes | **real** — corrigida |
| contagem de nós dos ajudantes | **falsa** — esta secção |
| tecto do `Threads` | real, benigna — não tocada |

## Porque é que fica registado

É o sétimo erro do mesmo tipo neste trabalho, e todos têm a mesma forma:
**procurei por um padrão, não o encontrei, e tomei a ausência do padrão pela
ausência da coisa.**

A lista, para quem vier a seguir:

1. o regexp `(?![,)])` que excluía a forma de leitura — quase declarou três
   manetes como "nunca lidas"
2. `0xffff82ff` convertido de cabeça como −31969, quando é −32001
3. o `31507` lido como "nota de profundidade máxima", quando é vitória por
   tablebase
4. os tamanhos de alocação atribuídos por ordem em vez de pelo ponteiro —
   quase declarou acesso fora dos limites na tabela de peões
5. a comparação dos dois `negamax` por mnemónicas, que dava 7% de código
   comum quando a estrutura dá 55%
6. a tabela `FALTA` procurada por bytes, quando o compilador a desfez em
   comparações
7. **esta** — `nos_totais` procurada pelos nomes do binário

Cinco dos sete davam um **falso negativo**: "isto não existe". Quando a
resposta é negativa, o custo de a verificar por um segundo caminho é sempre
menor do que o de a escrever.

## RETRACTACAO: a chave da repeticao NAO diverge

A seccao `DIVERGENCIA_CHAVE` esta **errada** e fica sem efeito.

Afirmava que o binario amassa a chave com o contador dos cinquenta lances
antes de a empilhar, e que o motor atual empilha a chave crua. A primeira
metade esta certa; a segunda esta errada.

### A prova

`vendor/position.h`:

```cpp
template<bool AfterMove = false>
Key adjust_key50(Key k) const;

inline Key Position::key() const { return adjust_key50(st->key); }

template<bool AfterMove>
inline Key Position::adjust_key50(Key k) const {
    return st->rule50 < 14 - AfterMove ? k : k ^ make_key((st->rule50 - (14 - AfterMove)) / 8);
}
```

e `vendor/types.h:421`:

```cpp
constexpr Key make_key(u64 seed) { return seed * 6364136223846793005ULL + 1442695040888963407ULL; }
```

Os numeros sao os mesmos que o binario tem em hexadecimal:

```
6364136223846793005 == 0x5851f42d4c957f2d
1442695040888963407 == 0x14057b7ef767814f
```

E o limiar e a divisao tambem: `rule50 < 14` contra `cmp $0xd / jle`, e
`(rule50 - 14) / 8` contra `sub $0xe / sar $0x3`.

**`pos.key()` ja devolve a chave amassada.** O `chaves.push_back(pos.key())`
do `busca.cpp` empilha o mesmo valor que o binario empilha, e a `repeticao()`
compara o mesmo dos dois lados. Nao ha divergencia nenhuma.

### Onde o binario usa o amassamento

A constante aparece 29 vezes, em oito funcoes -- o que confirma que e o

## RETRACTACAO: a chave da repeticao NAO diverge

A seccao `DIVERGENCIA_CHAVE` esta **errada** e fica sem efeito.

Afirmava que o binario amassa a chave com o contador dos cinquenta lances
antes de a empilhar, e que o motor atual empilha a chave crua. A primeira
metade esta certa; a segunda esta errada.

### A prova

`vendor/position.h`:

```cpp
template<bool AfterMove = false>
Key adjust_key50(Key k) const;

inline Key Position::key() const { return adjust_key50(st->key); }

template<bool AfterMove>
inline Key Position::adjust_key50(Key k) const {
    return st->rule50 < 14 - AfterMove ? k : k ^ make_key((st->rule50 - (14 - AfterMove)) / 8);
}
```

e `vendor/types.h:421`:

```cpp
constexpr Key make_key(u64 seed) { return seed * 6364136223846793005ULL + 1442695040888963407ULL; }
```

Os numeros sao os mesmos que o binario tem em hexadecimal:

```
6364136223846793005 == 0x5851f42d4c957f2d
1442695040888963407 == 0x14057b7ef767814f
```

E o limiar e a divisao tambem: `rule50 < 14` contra `cmp $0xd / jle`, e
`(rule50 - 14) / 8` contra `sub $0xe / sar $0x3`.

**`pos.key()` ja devolve a chave amassada.** O `chaves.push_back(pos.key())`
do `busca.cpp` empilha o mesmo valor que o binario empilha, e a `repeticao()`
compara o mesmo dos dois lados. Nao ha divergencia nenhuma.

### Onde o binario usa o amassamento

A constante aparece 29 vezes, em oito funcoes -- o que confirma que e' o
`Position::key()` inlined em todo o lado, e nao um tratamento especial da
repeticao:

| funcao | sitios |
|---|---:|
| `negamax` | 11 |
| `negamax [clone .constprop.0]` | 8 |
| `quiescencia` | 3 |
| `repete_ja` | 2 |
| `indices` | 2 |
| `laco_uci` | 1 |
| `garante_ameacas` | 1 |
| `Position::prefetch_key` | 1 |

### O erro, e e o mesmo de sempre

Li `pos.key()` no sitio da chamada e presumi que devolvia o campo cru
`st->key`. Nao abri o `position.h`. E a duodecima vez neste trabalho que uma
procura ou uma leitura parcial me faz declarar uma diferenca que nao existe --
e a segunda em que o que falta e' seguir uma funcao ate a sua definicao.

**Regra que fica:** antes de dizer que o atual "usa a forma crua", abrir a
definicao da funcao que produz o valor. O nome no sitio da chamada nao diz o
que a funcao faz.

### Consequencia para os empates por repeticao

As 9 partidas em 21 que acabaram por tripla repeticao no SPRT **nao tem aqui
a sua causa**. Fica por explicar.

## SYZYGY: integracao verificada, sem divergencia

O binario sonda as tabelas uma vez, da `negamax`. As quatro guardas
(`438008`-`439b0d`):

```
438008:  mov  Tablebases::MaxCardinality(%rip),%r9d
438012:  jle  skip                      ; MaxCardinality <= 0
438014:  mov  0x90(%r15),%ecx
43801b:  add  0xb0(%r15),%ecx           ; contagem de pecas
438025:  jl   skip                      ; MaxCardinality < contagem
438029:  je   439b09                    ; rule50 == 0
439b09:  testb $0xf,0x30(%rax)
439b0d:  jne  skip                      ; algum direito de roque
```

O atual (`busca.cpp:1132`) tem as mesmas quatro, pela mesma ordem.

Os tres ramos do resultado:

| WDL | binario | atual |
|---|---|---|
| `> 0` | `ecx = 31507 - ply`, `r8d = 1` | `VALUE_TB_WIN_IN_MAX_PLY - ply`, `Limite::Inferior` |
| `== 0` (`43b211`) | `xor %ecx,%ecx` / `xor %r8d,%r8d` | `VALUE_DRAW`, `Limite::Exacto` |
| `< 0` (`43b1a8`) | `lea -0x7b13(%r13),%ebx` = `ply - 31507`, `r8d = 2` | `VALUE_TB_LOSS_IN_MAX_PLY + ply`, `Limite::Superior` |

E a guarda na TT: `mov $0xf5,%edx` = **245**, que e' `MAX_PLY - 1` com
`MAX_PLY = 246` (`vendor/types.h:116`). O `++tb_hits` esta em `Busca+0x290`
(`439b3f`).

**Nada a corrigir.** Nota lateral util: a contagem de pecas e' a soma de
`pos+0x90` com `pos+0xb0` -- os mesmos dois campos que o `lmr_pecas_fim` usa,
o que confirma a leitura que este documento ja tinha feito desses offsets.

## QUIESCENCIA: verificada de ponta a ponta, sem divergencia

Comparacao instrucao a instrucao de `Busca::quiescencia` (`0x420830`, 904
linhas) contra `src/busca.cpp:958-1078`.

### A entrada

| binario | atual |
|---|---|
| `420865: addq $0x1,0x280(%rdi)` | `++nos` |
| `42086d: cmpb $0x0,DIAG+0xa` / `420876: addq $0x1,g_forma_qs` | `if (DIAG.forma) ++g_forma_qs` |
| `42087e: cmp %r13d,0x298(%rbx)` / `jge` / `mov %r13d,0x298(%rbx)` | `if (ply > sel_prof) sel_prof = ply` |
| `420891: call sem_tempo` / `jne -> 0` | `if (sem_tempo()) return 0` |
| `4208a5: cmp $0xf5,%r13d` / `je` | `if (ply >= MAX_PLY - 1)` (245; o compilador provou que so' da igualdade) |
| `4208b6: cmp $0x63,%eax` / `jg` | `pos.rule50_count() >= 100` |

### A `repeticao()` inlined -- `4208cc`-`42091a`

```
4208cc:  mov 0x29e30(%rbx),%r11    ; chaves.begin
4208d3:  mov 0x29e38(%rbx),%r12    ; chaves.end
4208e0:  sub %r11,%r12 / sar $0x3  ; chaves.size()
4208ea:  lea -0x2(%r12),%r14d      ; i = size - 2
4208ef:  cmovg %r12d,%eax          ; limite = min(rule50, size)
4208f3:  sub %eax,%r12d
4208f6:  cmovs %r15d,%r12d         ; max(0, size - limite)   <- o `&& i >= 0`
420910:  cmp %rsi,(%r11,%rax,8)/je ; chaves[i] == k
42091a:  sub $0x2,%rax             ; i -= 2
```

Identica a `Busca::repeticao`, incluindo o `++n >= 1` (sai a primeira).

### A sonda e o chao -- `420a20`-`420b77`

`nota_da_tt` inlined em `420a90`-`420aa6`:

```
nota > 31753   -> nota - ply
|nota| <= 31753 -> nota
nota < -31753  -> nota + ply
```

Os tres limites (`420aaa`-`420ac8`): `Exacto` devolve, `Inferior` exige
`sc >= beta`, `Superior` exige `sc <= alpha`. Igual.

O chao (`420afd`-`420b33`), que e' a parte menos obvia do atual:

```
420b28:  cmp %esi,%r15d / cmovg %esi,%r15d   ; chao = min(ts, estatica)   [Superior]
420b2f:  cmp $0x2,%r14b / cmovne %esi,%r15d  ; limite != Superior -> chao = estatica
```

com `Exacto` a cair directamente em `chao = ts`. E' exactamente o
`melhor = Exacto || (Inferior && ts > estatica) || (Superior && ts < estatica)`
do atual.

Depois `420b44: cmp %edx,%r15d / jge` = `if (chao >= beta) return chao`, e
`420b70: cmovge` = `alpha = max(alpha, chao)`.

### O filtro SEE -- `4212ef`-`421325`

```
4212fa:  jne 42132b              ; em_xeque -> sem filtro
4212fc:  mov 0x98(%r15),%r11d    ; qs_recaptura   (Busca+0x98)
421312:  cmp %eax,%ecx / je      ; m.to_sq() == casa_ant -> sem filtro
421316:  xor %edx,%edx           ; limiar 0
42131e:  call see_ge(m, 0)
421325:  je -> continue
```

Igual, manete `qs_recaptura` e `casa_ant` incluidos.

### A guarda final -- `4214d7`-`42155d`

`prof = 0` (`421553: xor %edx,%edx`), `pv = false` (`421555: push $0x0`),
`aval = em_xeque ? sentinela : estatica` (`4214fc: cmovne`), e o
`nota_para_tt` com os mesmos limiares. Igual.

### Conclusao

**Zero divergencias na `quiescencia`.** A reconstrucao desta funcao esta certa
linha a linha. E' a primeira funcao grande verificada de fio a pavio.

Nota lateral: `$0x55` = 85 aparece quatro vezes na funcao -- e' o
`otimismo_de` inlined (`otimismo_f * a / (|a| + 85)`), o que confirma a
chamada `avalia_cheia(pos, DIAG.eval_escala, otimismo_de(pos))`.

---

## O sentinela "sem avaliacao" difere -- e e' cosmetico

| | valor |
|---|---:|
| binario (`420a20`, `420ae0`, `4214e6`) | `0xffff8000` = **-32768** |
| atual (`tt.h:20`) | `TT_SEM_AVAL` = **32002** |

Os dois estao fora do alcance de uma avaliacao real (`VALUE_INFINITE` = 32001),
portanto nenhum colide. O que importava era se o valor chega a entrar em
aritmetica em vez de so ser comparado -- e nao chega: os cinco sitios que lem
`aval_ply` no `busca.cpp` (`1223`, `1225`, `1235`, `1265`) passam todos pelo
`usavel()` antes de usar o numero.

**Nao se corrige.** Fica registado porque e' uma constante do binario que a
reconstrucao nao acertou.

---

## `valor_empate`: a formula esta certa, o sitio onde se aplica nao

### A formula bate inteira

O binario nao tem simbolo `valor_empate` -- foi toda inlined. Reconstrui-a dos
tres sitios de empate da `negamax`:

```
436ab1:  mov 0x6c(%rbp),%r14d      ; contempt        (Busca+0x6c)
436ab8:  test / je                 ; == 0 -> devolve 0
436abd:  mov 0x68(%rbp),%esi       ; elo_margin      (Busca+0x68)
436ac0:  test / jle 43988c         ; <= 0 -> cauda
436ac8:  mov 0x340(%rbp),%edi      ; opponent_elo    (Busca+0x340)
436ad0:  test / jle 43988c         ; <= 0 -> cauda
436ad6:  mov 0x64(%rbp),%r10d      ; our_elo         (Busca+0x64)
436ada:  sub %edi,%r10d            ; our_elo - opponent_elo
436add:  cmp %r10d,%esi / jg       ; >= elo_margin ?  senao 0
436ae2:  movzbl 0x28370(%rbp),%ebx ; lado_raiz
436aec:  cmp %bl,0x26c(%r15) / jne ; side_to_move == lado_raiz
436af5:  neg %r12d                 ;   -> -contempt ; senao +contempt

43988c:  mov 0x5c(%rbp),%ecx       ; contempt_sem_inc (Busca+0x5c)
439896:  cmpb $0x0,0x28378(%rbp)   ; tem_incremento
43989d:  jne -> 0
4398a3:  mov 0x60(%rbp),%r8d       ; contempt_um_lado (Busca+0x60)
4398b0:  mov 0x28374(%rbp),%r9d    ; nota_raiz_ant
4398ba:  test / jns                ; < 0 -> 0 ; senao +-contempt
```

Isto e', ramo a ramo e pela mesma ordem, o `Busca::valor_empate` de
`busca.cpp:1974-1991`. **Nada a corrigir na funcao.**

(A manete `KS_CONTEMPT_SEM_INC` nao existe entre as strings do binario, mas o
campo `Busca+0x5c` e' lido -- o valor vem por outra via, nao por ambiente. Nao
e' divergencia.)

### O que diverge e' onde ela se aplica

`Busca+0x6c` (o `contempt`) e' lido em **exactamente dois sitios de todo o
executavel**, `436859` e `436ab1`, **ambos dentro da `negamax`**.

```
quiescencia (0x420830)            : 0 leituras de 0x6c
negamax [clone .constprop.0]      : 0 leituras de 0x6c
```

E os tres caminhos de empate da `quiescencia` -- `sem_tempo` (`420898`),
`rule50 > 99` (`4208b9`) e repeticao (`420914`) -- convergem todos em:

```
4215a0:  xor %r15d,%r15d
4215a3:  jmp 421564          ; return 0
```

**No motor perdido a quiescencia devolve zero seco para os empates.** O atual
chama `valor_empate(pos)` (`busca.cpp:967`), que aplica o contempt.

### Vale Elo?

**Hoje nao**, porque `contempt = 0` por defeito (`busca.h:695`) e entao o
`valor_empate` devolve 0 a primeira linha -- identico ao binario.

Mas se alguem puser `KS_CONTEMPT` diferente de zero, os dois motores deixam de
concordar: no atual a mesma posicao empatada vale `+-contempt` na `quiescencia`
e na `negamax`; no perdido vale `0` na `quiescencia` e `+-contempt` na
`negamax`. Como as duas partilham a tabela de transposicao, essa discordancia
e' instabilidade de busca, nao uma afinacao.

Pelo criterio de recuperar cada linha esteja ou nao activa: **na `quiescencia`,
os tres retornos de empate eram `return VALUE_DRAW;`, nao
`return valor_empate(pos);`**.

## A ASPIRACAO: verificada, e o `asp_sf` reconstruido inteiro

`Busca::aspiracao` nao tem simbolo -- foi toda inlined no `arranca`. O ciclo
esta em `43ffea`-`440210`.

### Offsets das manetes

| manete | offset na `Busca` |
|---|---|
| `asp_sf` | `+0x1d0` |
| `asp_tecto` | `+0x1d4` |
| `asp_delta` | `+0x1d8` |
| `asp_prof` | `+0x1dc` |
| `delta_raiz` (escrito) | `+0x47f20` |
| `ultima_prof` (escrito ao sair) | `+0x274` |

### O que bate ao atual

```
440076:  mov 0x1d8(%r12),%eax     ; asp_delta
44007e:  shl $0x3,%eax            ; *8
440082:  idiv %r13d               ; / prof
440085:  lea 0x5(%rax),%r15d      ; delta = 5 + asp_delta*8/prof
```
= `int delta = 5 + p.asp_delta * 8 / std::max(prof, 1);`

```
440089:  cmp %r13d,0x1dc(%r12) / jge 4401fd   ; asp_prof >= prof -> janela infinita
440097:  neg / cmovs / cmp $0x7c09 / jg       ; |anterior| > 31753 -> janela infinita
4400b1:  lea (%r15,%r14,1),%r14d              ; beta  = anterior + delta
4400b5:  sub %r15d,%r9d                       ; alpha = anterior - delta
```
= `if (prof > p.asp_prof && !e_mate(anterior))`

```
440123:  sub %ebx,%r11d / test / cmovle $1    ; delta_raiz = max(beta - alpha, 1)
440148:  mov %r11d,0x47f20(%rdi)
44013b:  neg %ecx / cmp %ecx,%ebx / cmovl     ; alpha < -asp_tecto -> -INFINITO
440142:  cmp %eax,%r14d / cmovg $0x7d01       ; beta  >  asp_tecto -> +INFINITO
```
= as quatro linhas do atual, com `INFINITO = 32001` (`0x7d01`) e
`-INFINITO = -32001` (`0xffff82ff`).

E as duas saidas de falha, no ramo `asp_sf == 0`:

```
4401de:  add %r14d,%ebx
4401e4:  shr $0x1f / add / sar $1   ; beta = (alpha + beta) / 2
4401f6:  cmp $0xffff82ff,%eax / cmovge  ; alpha = max(nota - delta, -INFINITO)

4401aa:  add %r15d,%eax
4401b9:  cmp $0x7d01,%eax / cmovle      ; beta = min(nota + delta, INFINITO)

4401cc:  shr $0x1f / add / sar $1       ; delta += delta / 2
```

**Tudo igual ao `busca.cpp:2005-2052`.** A aspiracao nao diverge.

---

### O `asp_sf`: o bloco que falta, agora completo

O `asp_sf` e' lido em tres sitios (`4400ba` antes do ciclo, `4400d0` na falha
baixa, `4401b3` na falha alta) e **troca tres coisas ao mesmo tempo**, nao uma.

#### 1. A profundidade dada a `negamax`

```
440153:  sub %r13d,%edx        ; prof - falhas_altas
44015c:  test %edx,%edx
440160:  cmovle %r11d,%edx     ; max(..., 1)
440164:  test %esi,%esi        ; asp_sf == 0 ?
440170:  cmove %r12d,%edx      ;   sim -> edx = prof
440181:  call negamax(...)     ; edx e' o argumento da profundidade
```

`%r13d` e' um contador: zerado antes do ciclo (`4400c8`), **zerado a cada
falha baixa** (`4400f3: xor %r13d,%r13d`) e **incrementado a cada falha alta**
(`4401c0: add $0x1,%r13d`).

#### 2. O beta na falha baixa

```
4400d0:  mov 0x1d0(%rdi),%r13d
4400da:  je 4401de             ; asp_sf == 0 -> o ramo normal
4400e3:  mov %ebx,%r14d        ; beta = alpha        <-- nao (alpha+beta)/2
4400f0:  cmovge %eax,%ebx      ; alpha = max(nota - delta, -INFINITO)
4400f3:  xor %r13d,%r13d       ; falhas_altas = 0
```

#### 3. O crescimento do delta

```
4400f6:  movslq %r15d,%r10
4400fc:  imul $0x55555556,%r10,%rcx
440106:  shr $0x20,%rcx
44010a:  sub %eax,%ecx         ; ecx = delta / 3
44010c:  add %ecx,%r15d        ; delta += delta / 3
```

`0x55555556` = 1431655766 ~= 2^32/3, portanto **`delta/3`**, contra o
`delta/2` do ramo normal (`4401cc`).

#### Em codigo

```cpp
int delta = 5 + p.asp_delta * 8 / std::max(prof, 1);
int alpha = -INFINITO, beta = INFINITO;
if (prof > p.asp_prof && !e_mate(anterior)) {
    alpha = anterior - delta;
    beta  = anterior + delta;
}
int falhas_altas = 0;                                    // so' serve com asp_sf
for (;;) {
    delta_raiz = std::max(beta - alpha, 1);
    if (alpha < -p.asp_tecto) alpha = -INFINITO;
    if (beta  >  p.asp_tecto) beta  =  INFINITO;

    // Com `asp_sf`, uma iteracao que ja' falhou alto varias vezes nao merece a
    // profundidade inteira: o lance ja' se mostrou, falta so' confirma-lo.
    int prof_ajust = p.asp_sf ? std::max(prof - falhas_altas, 1) : prof;

    int nota = negamax(pos, prof_ajust, alpha, beta, 0, true, false);
    if (parado) return nota;

    if (nota <= alpha) {
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
```

### O que isto muda na leitura

A seccao `DIVERGENCIAS_PARAMETROS` chamou ao `asp_sf` "esquema alternativo de
alargamento da janela". **E' mais do que isso**: e' um ciclo de aspiracao
inteiro em alternativa, com reducao de profundidade por falha alta -- um
mecanismo que o motor atual nao tem sob nenhuma forma.

**Inerte no ajuste actual** (`asp_sf = 0` no molde do binario), como as outras
duas manetes em falta. Mas e' o unico dos tres cujo codigo ausente e' um bloco
estrutural e nao uma linha, e e' o unico que da' uma experiencia por fazer:
ligar o `asp_sf` e medir.

## A `reducao` inteira, termo a termo -- e o unico termo que faltava

`Busca::reducao` e' pequena (`0x40ade0`, 469 bytes) e tem catorze argumentos.
Comparei-a toda contra `busca.cpp:845-918`.

| # | binario | atual | bate? |
|---|---|---|---|
| 1 | `r = tab[min(p1,63)*64 + min(p2,63)]`, tabela em `Busca+0x3b0` (`40ae24`) | `lmr[min(prof,63)][min(i,63)]` | sim |
| 2 | `p14 > 1 -> r += [0x90] + (p14>2 ? [0x94] : 0)` (`40ae30`) | `cut_cnt_base` + `cut_cnt_mais` | sim |
| 3 | `!melhorando -> r += r*[0x138]/512` (`40ae4c`) | `lmr_piora_f` | sim |
| **4** | **`[0x12c] > 0 -> r -= (p4-p3)*[0x12c] / max(delta_raiz,1)`** (`40ae6f`) | **nao existia** | **NAO** |
| 5 | `!p6 -> r -= r/2` (`40aea1`) | `!tranquilo` | sim |
| 6 | bloco `[0x14c]`/`[0x140]` com `[0x150]`, `[0x154]`, `[0x158]`, `[0x15c]` (`40aeb1`-`40af98`) | `lmr_ttpv`, `lmr_cut_f`, `lmr_ttpv_pv`, `lmr_ttpv_alpha`, `lmr_ttpv_fundo`, `lmr_cut_sem_tt` | sim |
| 7 | `!p8 -> r += [0x13c]` (`40aeff`) | `lmr_nonpv_f` | sim |
| 8 | `r -= clamp(p11*1024/max([0x144],1), -2048, 2048)` (`40af0a`) | `lmr_hist_div` | sim |
| 9 | `r<0 -> r = r*[0x164]/100` (`40af55`) | `lmr_ext_amort` | sim |

Os grampos do termo 8 estao em `.rodata` e conferem ao valor:

```
609c90:  01000000    ->     1   (o max(...,1))
609da0:  00080000    ->  2048
609db0:  00f8ffff    -> -2048
```

E o `/100` do termo 9 e' o `imul $0x51eb851f / sar $0x25` em `40af64`.

Offsets que isto fixa, e que nao estavam no mapa: **`0x138` = `lmr_piora_f`,
`0x13c` = `lmr_nonpv_f`, `0x144` = `lmr_hist_div`**.

### O que faltava, e onde

O termo 4 -- o `lmr_delta`. A posicao na sequencia importa e agora esta
fixada: **entre o termo do `melhorando` e a divisao por dois das capturas**,
porque essa divisao corta metade do que estiver acumulado ate' ai'.

Corrigido no commit `503326e`.

### Uma diferenca que fica por corrigir

A `reducao` do binario **nao tem instrumentacao nenhuma**: zero referencias a
`DIAG` ou a `conta_q` em 469 bytes. A nossa tem oito `if (DIAG.quem)
conta_q(...)` mais o `if (DIAG.sem_lmr)` a entrada.

Nao muda a arvore. Muda o custo de um caminho que corre em quase todos os nos.
Fica registado; tirar isso e' decisao de quem manda no codigo, nao minha.

---

## O `lmp_melhora`: um `if`, e confirma a formula da contagem

`negamax` em `43818c`:

```
43818c:  mov 0x30(%rbp),%r9d    ; lmp_melhora
438190:  imul %edx,%eax         ; prof*prof
438195:  add 0x28(%rbp),%eax    ; + lmp_base    (Busca+0x28)
43819b:  je 4381b0              ; lmp_melhora == 0 -> salta
43819d:  cmpb $0x0,0xc4(%rsp)   ; melhorando
4381a5:  jne 4381b0             ; melhorando -> salta
4381a7:  shr $0x1f / add / sar $1   ; conta /= 2
4381b0:  mov 0x28(%rsp),%edi    ; i
4381b9:  cmp %r11d,0x2c(%rbp)   ; lmp_prof      (Busca+0x2c)
4381bd:  setge %r8b
4381c1:  cmp %edi,%eax / setle %dl
4381c6:  and %dl,%r8b / jne     ; prof <= lmp_prof && i >= conta -> corta
```

Fixa **`lmp_base` = `0x28`, `lmp_prof` = `0x2c`, `lmp_melhora` = `0x30`**, e
confirma que a formula `lmp_base + prof*prof` do atual esta certa.

Corrigido no commit `12fde85`, junto com o `asp_sf`.

### Nota: o `poda_red`, logo acima

Em `438125`-`43817c`, imediatamente antes do LMP, o binario le
`[0x128]` (`poda_red`) e, se for diferente de zero, tira a` profundidade o
valor da MESMA tabela LMR dividido por 1024, com chao em zero:

```
438125:  mov 0x128(%rbp),%r11d
438153:  lea 0xe8(%r9,%rsi,1),%rax      ; a tabela LMR outra vez
43815b:  mov 0x10(%rbp,%rax,4),%r8d
43816e:  sar $0xa,%r11d                 ; / 1024
438172:  sub %r11d,%edi
43817c:  cmovs %edi,%ecx                ; max(..., 0)
```

Fica por comparar com o atual.

---

## Estado das tres manetes que faltavam

| manete | offset | onde vive | estado |
|---|---|---|---|
| `KS_LMR_DELTA` | `Busca+0x12c` | `reducao`, termo 4 | **reposto** (`503326e`) |
| `KS_LMP_MELHORA` | `Busca+0x30` | `negamax`, antes do LMP | **reposto** (`12fde85`) |
| `KS_ASP_SF` | `Busca+0x1d0` | ciclo de aspiracao, tres sitios | **reposto** (`12fde85`) |

As tres valem zero no molde do binario. Verificado depois de cada commit:
`go depth 12` nas mesmas tres posicoes da' **59458 / 72742 / 30599** nos, igual
ao no, antes e depois. Com as tres ligadas
(`KS_ASP_SF=1 KS_LMP_MELHORA=1 KS_LMR_DELTA=100`) a posicao inicial cai para
40908 nos -- o codigo e' alcancado.

## A AUDITORIA COMPLETA DOS PARAMETROS -- e a armadilha do molde, resolvida

Esta seccao substitui a `DIVERGENCIAS_PARAMETROS`, que comparava por NOME e
por isso nao podia ver o que se segue.

### Primeiro: onde esta o molde, exactamente

O inicializador estatico (`409105`-`40926d`) copia **dezassete blocos de 32
bytes** de `0x615600` para `g_busca+0x10`, e depois a cauda:

```
40915b:  vmovdqa 0x615600,%ymm0   ->  4091b7:  vmovdqu %ymm0,g_busca+0x10
...                                   (dezassete blocos, ate' +0x210)
40923d:  vmovdqa 0x609eb0,%xmm1   ->  40926d:  vmovdqa %xmm1,g_busca+0x230
```

Portanto:

- **corpo**: `0x615600` .. `0x615820`  ->  `Busca+0x10` .. `+0x230`
- **cauda**: `0x609eb0` .. `0x609ec0`  ->  `Busca+0x230` .. `+0x240`

Duas armadilhas que este trabalho ja' pisou, agora fechadas:

1. **O molde NAO comeca em `0x6155f0`.** Esses dezasseis bytes sao a cadeia
   ASCII `"8889909192939495"`. Comeca em `0x615600`, e o primeiro inteiro e'
   `0x6e` = 110 = `rfp_margem`, que confere.
2. **A cauda NAO esta em `0x609c80`** (sao espacos ASCII). Esta em `0x609eb0`:

```
609eb0  46000000 8c000000 14000000 37000000
        0x230=70  0x234=140 0x238=20  0x23c=55
```

`0x23c = 55` e' o `tm_tecto_x10`, que o motor atual ja' tinha em 55 -- e' essa
a verificacao de que o endereco esta certo.

### Como a comparacao foi feita desta vez

Por POSICAO, nao por nome, em tres passos que nao dependem de adivinhar
nomes:

1. o codigo de leitura do ambiente no `arranca` da' **`KS_X` -> offset**
   (padrao `lea <str> ; getenv ; strtol ; mov %eax,OFF(%reg)`);
2. as linhas `getenv` do nosso `busca.cpp` dao **`KS_X` -> `p.campo`**;
3. o `busca.h` da' **`p.campo` -> valor por omissao**.

Isto e' necessario porque os nomes NAO batem, e e' por isso que a passagem
anterior deu "zero diferencas":

| nome no ambiente | campo no fonte |
|---|---|
| `KS_CUT` | `lmr_cut_f` |
| `KS_NMP_TECTO` | `nmp_div` |
| `KS_NMP_DIV` | `nmp_prof_div` |
| `KS_TT_SUP` | `tt_sup_nota` |
| `KS_PEAO_CH` | `peao_chaves` |
| `KS_QS_RECAPT` | `qs_recaptura` |
| `KS_SEEC` / `KS_SEEQ` | `see_poda` / `see_poda_tranq` |

### O resultado

**Uma unica diferenca de valor em 140 parametros: o `lmr_cut_f`.**

| | binario | atual (antes) |
|---|---:|---:|
| `lmr_cut_f` (`Busca+0x140`) | **2048** | 1024 |

Corrigido no commit `33ad073`. O alinhamento nao e' suposicao: os sete
vizinhos imediatos batem todos ao valor -- `lmr_base=77` (`0x130`),
`lmr_div=236` (`0x134`), `lmr_piora_f=197` (`0x138`), `lmr_nonpv_f=1024`
(`0x13c`), `lmr_hist_div=22000` (`0x144`), `lmr_pecas_fim=0` (`0x148`),
`lmr_ttpv=1024` (`0x14c`).

Tres "diferencas" que apareceram e NAO eram: `KS_OFF_HPODA`, `KS_OFF_FUT` e
`KS_OFF_SING` sao INTERRUPTORES, nao parametros. Escrevem constantes em
ranhuras de outros (`movl $0x3b9aca00,0x34`, `movl $0x0,0x48`,
`movl $0x3e7,0x80`) e o meu leitor atribuiu-lhes essas ranhuras.

### A prova de que nao falta nenhum parametro

Comparacao de MULTICONJUNTO entre os 140 valores do molde e os defaults da
nossa `struct Parametros` (independente da ordem, que difere entre os dois):

```
valores que o BINARIO tem a mais: (nenhum)
valores que o ATUAL tem a mais:   ordem_xeque_see = -75
                                  tm_chao_ms      = 10
                                  tres campos a zero
```

Como os zeros se podem mascarar uns aos outros, os **35 offsets a zero** foram
verificados um a um: 32 tem dono identificado pelo nome da manete, e os tres
restantes foram resolvidos pelo uso:

| offset | o que e' |
|---|---|
| `0x11c` | **nenhum leitor em todo o binario** -- morto la' tambem |
| `0x1ac` | `iir_sem_all`: `439765`, `if (knob == 0 \|\| all) --prof` |
| `0x1c0` | porta um ramo da `hist_de` para fatias <= 4 (`412070`) |

**Zero parametros em falta.** A ordem de declaracao da nossa `struct` nao e' a
do binario, mas isso nao muda comportamento: cada campo e' acedido pelo nome.

---

## OS BLOCOS DE PODA, VERIFICADOS

### Futilidade inversa (RFP) -- `436ffe`-`4370ba`

```
437002:  mov 0x14(%rbp),%r10d     ; rfp_mult
437018:  imul %edx,%r10d          ; * prof
43701c:  add 0x10(%rbp),%r10d     ; + rfp_margem
437023:  cmovg %ecx,%r10d         ; min(..., rfp_tecto)
43702c:  and 0x1c(%rbp),%eax      ; melhorando ? rfp_melhorando : 0
437036:  sub %eax,%r11d
43703c:  test 0x1b0 / je          ; usa_pior_adv
437040:  imul 0x1bc(%rbp),%r11d   ; * rfp_adv_f
43705c:  sar $0xa,%ecx            ; / 1024
43705f:  and %esi,%ecx            ; & pior_adv
437064:  test 0x1b8 / jg          ; corr_marg_div > 0
437076:  mov 0x20(%rbp),%ecx      ; rfp_tecto_tot > 0 -> min
43708c:  cmp %eax,0x24(%rbp)/jle  ; prof < rfp_prof
4370a1:  mov 0x1b4(%rbp),%edi     ; usa_tt_capt
```

Fixa quatro offsets que nao tinham nome: **`0x1b0` = `usa_pior_adv` (0),
`0x1b4` = `usa_tt_capt` (1), `0x1b8` = `corr_marg_div` (0), `0x1bc` =
`rfp_adv_f` (335)** -- os quatro batem aos defaults do fonte. **Sem
divergencia.**

### Lance nulo -- `43a0b6`-`43a10c`

```
43a0c6:  vmovd 0xd4(%rbp),%xmm3   ; nmp_div_est
43a0e8:  idiv %r11d               ; (aval - beta) / max(nmp_div_est, 1)
43a0ed:  cmovg %edi,%eax          ; min(..., nmp_div)      [0xd0]
43a0f0:  add 0xcc(%rbp),%eax      ; + nmp_base             [0xcc]
43a0fa:  test 0xd8 / jle          ; nmp_prof_div > 0       [0xd8]
43a0fc:  cmpb $0x0,DIAG+0x8       ; DIAG.sem_nmp_prof
43a10a:  idiv %ecx                ; prof / nmp_prof_div
43a10c:  add %eax,%ebx
```

`nmp_base=4`, `nmp_div=6`, `nmp_div_est=200`, `nmp_prof_div=5`. **Sem
divergencia**, incluindo o sinalizador de diagnostico.

### Razoring

`KS_OFF_RAZOR` escreve `$0x0` em `0xc8`, logo **`0xc8` = `razor_prof` = 5** e
**`0xc4` = `razor_margem` = 348**. Batem aos defaults. `KS_SING` escreve em
`0x80`, logo **`0x80` = `sing_prof` = 5**, que tambem bate.

---

## O QUE JA' ESTA VERIFICADO DE PONTA A PONTA

| parte | estado |
|---|---|
| `quiescencia` inteira | verificada, sem divergencia |
| `reducao` inteira | verificada; faltava o `lmr_delta` (reposto) |
| ciclo de aspiracao | verificado; faltava o `asp_sf` (reposto) |
| Syzygy | verificado, sem divergencia |
| `valor_empate` | formula verificada; nao se aplica na `quiescencia` do binario |
| chave da repeticao | **retractacao**: nao diverge |
| RFP, lance nulo, razoring | verificados |
| LMP, futilidade, poda por historico, SEE | verificados; faltavam `lmp_melhora` e `poda_red` (repostos) |
| gestao de tempo | faltavam `tm_adv_f/min/max` (repostos); cauda do molde resolvida |
| os 140 parametros | **um so' divergia: `lmr_cut_f`** |

## A ORDENACAO E O CREDITO, VERIFICADOS -- e a segunda divergencia viva

### `credita` (`0x412280`, 1832 bytes)

O censo dos divisores fecha a estrutura inteira sem sobrar nada:

| instrucao | divisor | quantas | a que tabelas corresponde |
|---|---:|---:|---|
| `imul $0x45e7b273` + `sar $0x2c` | 15000 | 3 | `low_ply`, `principal`, `hist_pc` (`TECTO_HIST`) |
| `imul $0x45e7b273` + `sar $0x2d` | 30000 | **5** | as cinco fatias de continuacao (`TECTO_CONT`) |
| `sar $0xd` | 8192 | 1 | peoes (`TECTO_PEAO`) |
| `sar $0xa` | 1024 | 3 | `low_bonus` + os DOIS pesos dos peoes |

As cinco fatias confirmam o `CONT_RECUO[5] = {1,2,4,3,6}` do fonte: o
compilador desenrolou o ciclo ate' ao maximo, e o `cont_n = 3` escolhe as tres
primeiras em execucao.

E os unicos `imul` com imediato em toda a funcao sao `$0x450`, `$0x1cb` e o
magic da gravidade. **Nao ha mais nenhuma escala escondida** -- o que torna a
dos peoes um achado isolado e nao a ponta de um padrao.

### A divergencia: o bonus dos peoes nao e' o bonus cru

```
41242c:  cmp $0xfffffffd,%r12d    ; bonus vs -3
412430:  jl 412460
412432:  imul $0x450,%r12d,%edx   ; bonus * 1104
412444:  sar $0xa,%eax            ; / 1024
412460:  imul $0x1cb,%r12d,%r15d  ; bonus * 459
```

Tres coisas foram presas antes de acreditar nisto:

1. **`%r12d` e' o bonus.** `412290: mov %r8d,%r12d` guarda o quinto argumento
   inteiro a` entrada e nada lhe toca ate' `41242c`.
2. **O bloco e' o dos peoes.** `4123cc` le' o `peao_f` (`0x184`), `4123f7` le'
   o `peao_chaves` (`0x188`), e `41249d` le' o `cont_n` (`0x180`) para comecar
   o ciclo seguinte.
3. **O tecto e' 8192** e a aritmetica a` volta e' exactamente o nosso
   `soma_hist`: grampo, depois `e += b - e*|b|/tecto`.

Corrigido no commit `27c038c`. E' a segunda divergencia VIVA, e a primeira que
e' codigo em falta e nao uma constante.

### `credita_captura` (`0x40a9b0`)

Tecto 16384 (`$0x4000` / `$0xffffc000`), mascara `$0xc000` sobre o tipo do
lance com `$0x8000` = en passant, e retorno quando a peca comida e' nenhuma.
Sem escala. **Igual ao fonte.**

### `ordena` (`0x40aa70`)

Le `0x170` (`ordem_princ_f` = 138), `0x17c`, `0x180` e `0x1a0`
(`hist_pc_f` = 2400), mais o `$0x51eb851f` que e' o `/100` do termo da
peca-casa.

**Nao le' o `0x174`** -- e isso e' a confirmacao, nao uma falta: o
`ordem_cont_f` vale 32 e o compilador dobrou o `conts(...) * 32 / 32` para
nada. **Igual ao fonte.**

### `hist_de` (`0x411e00`)

Le `0x174`, `0x17c`, `0x180`, `0x184`, `0x188` e `0x1c0`. O
`411e52: lea (%r10,%r9,2),%r11d` e' o peso **[2,1,1]** do `CONT_PESO`.

Isto fecha o ultimo offset que tinha ficado sem dono: **`0x1c0` = `low_f`**. O
`412076: cmp $0x4,%r12d` e' o `ply < LOW_PLY` com `LOW_PLY = 5`
(`busca.h:1407`), e o `peao_f` vem logo a seguir -- a mesma ordem do fonte.

### Cuckoo (`436a8d`)

```
436a8d:  mov 0x19c(%rbp),%r14d   ; usa_cuckoo; se 0 -> salta
436a9d:  mov 0x58(%rbp),%ebx     ; cuckoo_sem_inc
436aa4:  cmpb $0x0,0x28378(%rbp) ; tem_incremento
436aab:  je -> salta
```

= `if (p.usa_cuckoo && !(p.cuckoo_sem_inc && !tem_incremento))`. **Igual.** A
varredura das chaves acima esta desenrolada de oito em oito.

---

## A EXTENSAO SINGULAR E O PROBCUT, VERIFICADOS

### Singular (`439ce6`-`43b2b9`)

Bate inteira, contadores incluidos: `Busca+0x2b0` (singulares), `+0x2b8`
(duplas), `+0x2c0` (negativas).

```
439cee/439cfb:  as duas guardas de mate sobre o ts
439d25:  mov 0x84(%rbp),%r11d    ; sing_margem
439d38:  imul %r10d,%r11d        ; * prof
439d7a:  sub %r11d,%r8d          ; alvo = ts - sing_margem*prof
439d61:  idiv %ecx (=2)          ; (prof-1)/2
439d6b:  mov %r14w,0x1e(%rbp,%r9,2)  ; excluido[ply] = m
439df2:  cmp %r11d,%esi / jle    ; alvo <= sc -> NAO singular
439e0c:  cmpl $0x0,0x9c(%rbp)    ; ext_dupla_exacto
439e32:  sub 0x88(%rbp),%esi     ; alvo - ext_dupla
439e54:  movl $0x2,0x80(%rsp)    ; ext = 2
43b294:  cmp %r12d,0x30(%rsp)/jg ; ts >= beta
43b2ae:  neg %r12d               ; ext = -ext_neg
```

Uma diferenca de forma: o binario poe `$0x1` e `$0x2` directamente, sem ler
nenhum sinalizador. O nosso tem `ext_ligadas = !DIAG.sem_ext` a volta dos
tres. E' instrumentacao, como o `DIAG.quem` na `reducao`.

### Probcut (`437186`-`437245`)

`skip se ts < beta + pc_margem` (`4371f1`) e `skip se estatica >= beta +
pc_margem` (`43720e`) -- as duas guardas do fonte a` letra. `0xec` = 5 =
`pc_red_impr`. **Igual.**

---

## O MAPA DO `DIAG`, CONFIRMADO POR TRES SITIOS

| offset | campo | onde o binario o le' |
|---|---|---|
| `DIAG+0x4` | `sing_sem_corte` | `43b26b`, a porta do multi-corte |
| `DIAG+0x8` | `sem_nmp_prof` | `43a0fc`, o termo da profundidade do lance nulo |
| `DIAG+0xa` | `forma` | `42086d`, o contador da `quiescencia` |

Batem aos tres na ordem de declaracao de `struct Diag` (`busca.cpp:72`).

## O LADO DA AVALIACAO, VERIFICADO -- e o varrimento fecha

### `corrigida` (`0x40dbc0`, 344 bytes)

Os seis pesos saem todos do codigo, e batem ao `CORR_PESO` do fonte:

```
40dc23:  imul $0xcb,...          ; 203   -> CORR_PESO[0]
40dc42:  imul $0x6d,...          ; 109   -> CORR_PESO[1]
40dc63:  imul $0x6d,...          ; 109   -> CORR_PESO[2]
40dc81:  imul $0x79,...          ; 121   -> CORR_PESO[3]
40dca7:  lea (%rsi,%rsi,8),%edi  ; x9
40dcaa:  lea (%rdx,%rdi,8),%edx  ; +x8  => 72  -> CORR_PESO[4]
40dcc7:  imul 0x194(%rbp),%r13d  ; corr6_peso   -> o sexto, que e' manete
40dc1b:  shl $0xe                ; x16384 = CORR_TAM
40dcdd:  sar $0xb                ; /2048  = CORR_DIV
```

`CORR_PESO[4] = 72` **nao aparece como imediato** -- o compilador fez o
`*72` com dois `lea`. Foi preciso ler a aritmetica para o ver, e e' por isso
que um censo de constantes sozinho nao chega.

### `aprende` (`0x40dd20`, 805 bytes)

```
40dd6a:  imul %ebp,%r12d     ; dif * prof
40dd7e:  cmp $0xfffff7f9     ; -2055, a fronteira do grampo a -256
40dd9b:  sar $0x3            ; / 8
40ddc7:  shl $0xe (x6)       ; x CORR_TAM, as seis familias desenroladas
40ddf3:  sar $0xa (x6)       ; / 1024 = CORR_TECTO
```

Os grampos estao em `.rodata`: `609dc0` = `0xffffff00` = **-256** e
`609dd0` = `0x00000100` = **+256**, que e' o `CORR_PASSO = CORR_TECTO/4`.

O `-2055` merece nota: e' exactamente a fronteira onde `dif*prof/8` passa
abaixo de `-256` com truncatura para zero. Se fosse `-2056` ou `-2048` a
leitura estaria errada.

### `indices` (`0x40d690`, 1328 bytes)

Splitmix64, com as tres constantes e os tres deslocamentos:

```
$0x9e3779b97f4a7c15   ->  z = x + 0x9e37...
$0xbf58476d1ce4e5b9   ->  z = (z ^ (z>>30)) * 0xbf58...
$0x94d049bb133111eb   ->  z = (z ^ (z>>27)) * 0x94d0...
$0x1e / $0x1b / $0x1f ->  30, 27, 31
$0x3fff (x6)          ->  CORR_TAM - 1, a mascara das seis familias
```

Igual a `mistura()` (`busca.cpp:254`). **Sem divergencia.**

### `avalia_cheia` (`0x41bcb0`, 396 bytes)

O nosso delega no `Eval::evaluate` do substrato; no binario essa funcao esta
inlined dentro dele. As constantes que a identificam batem:

```
41bd65:  imul $0x216   ; 534   -> 534 * count<PAWN>() + non_pawn_material()
41bdce:  imul $0x1dfb  ; 7675  -> + optimism * 7675
```

e a estrutura tambem: a discordancia das duas cabecas, a escala pelo material,
e o amortecimento linear pelo `rule50` no fim (`41bdf2: imul %ecx,%r12d` com
`r12d` vindo de `0x34(%r12)`, que e' o contador).

E' codigo do substrato (`vendor/evaluate.cpp:42-63`), nao da reconstrucao, e
esta intacto.

### `garante_ameacas` (`0x4119f0`, 970 bytes)

Mascaras de tabuleiro (`0xfefefefefefefe00`, `0x00fefefefefefefe`) para os
ataques de peao, e os indices de peca. Nada que destoe.

---

## O VARRIMENTO, FECHADO

Todas as funcoes da busca e da avaliacao foram comparadas contra o binario:

| funcao | resultado |
|---|---|
| `negamax` -- RFP, lance nulo, razoring, LMP, futilidade, poda por historico, SEE, singular, probcut, IIR, cuckoo, Syzygy | sem divergencia; faltavam `lmp_melhora` e `poda_red` |
| `quiescencia` | sem divergencia |
| `reducao` | faltava o `lmr_delta`; `lmr_cut_f` diferia (2048 vs 1024) |
| ciclo de aspiracao | faltava o `asp_sf` |
| `pontua`, `ordena`, `hist_de` | sem divergencia |
| **`credita`** | **o bonus dos peoes ia cru; o binario escala-o** |
| `credita_captura` | sem divergencia |
| `corrigida`, `aprende`, `indices` | sem divergencia |
| `avalia_cheia` | sem divergencia (substrato) |
| `valor_empate` | sem divergencia na formula; nao se aplica na `quiescencia` do binario |
| gestao de tempo | faltavam `tm_adv_f/min/max`; a cauda do molde estava no endereco errado |
| os 140 parametros | **um so' divergia: `lmr_cut_f`** |

**Duas divergencias vivas em todo o motor**: o `lmr_cut_f` e a escala do bonus
dos peoes. Tudo o resto que faltava valia zero por omissao.
