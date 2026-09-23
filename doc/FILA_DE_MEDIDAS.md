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
