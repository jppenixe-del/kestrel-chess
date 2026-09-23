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
