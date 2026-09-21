# Lista de validacoes -- transcricao contra a referencia

Cada linha e' uma diferenca ENCONTRADA entre a busca da referencia e a nossa,
com o estado da sua resolucao. Nada de prosa: o que interessa e' a coluna do
estado e o efeito medido quando existir.

Legenda do estado: `falta` (nao temos), `parcial` (temos incompleto),
`desligado` (temos mas nao corre), `transcrito` (ja' esta' no `refbusca.rs`),
`igual` (nao ha' diferenca).

| # | passo | diferenca | estado | efeito medido |
|---|---|---|---|---|
| 1 | 0 | `upcoming_repetition`: repeticao que o adversario pode forcar sobe o alpha antes de procurar | falta | por medir |
| 2 | 2 | valor de empate com ruido de paridade em vez de zero | transcrito | por medir |
| 3 | 2 | teste de empate so' tem regra dos 50; falta repeticao e material insuficiente | parcial | por medir |
| 4 | 1 | `followPV` -- "este no' esta' na linha que a iteracao anterior deu como melhor" | falta | por medir |
| 5 | 4 | `ttPv` e' calculado mas o termo que o usa na reducao esta' desligado (`TtpvLmr`) | desligado | por medir |
| 6 | 18 | `r` da reducao: 4 dos 9 termos nunca disparam (janela, captura, cut node, tt-pv) | desligado | modulo medio: tabela 2915, piora 1126, nao-PV 1024, historico 164 |
| 7 | 9 | margem da futilidade inversa cresce sem tecto (`110*prof`); a deles satura | ARRANJADO | arvore 2,76x -> 1,19x a referencia |
| 8 | 15 | `skip_quiets` parava todos os tranquilos; a deles poda so' aquele lance | ARRANJADO | -17,4 -> -4,1 Elo |
| 9 | 15 | faltava a guarda do PV no ramo dos tranquilos | ARRANJADO | incluido no anterior |
| 10 | 15 | historico de capturas nao era lido (dois termos a zero) | ARRANJADO | os dois parametros passaram a mexer |
| 11 | ordem | historico de capturas nao entra na ordem por omissao (`CaptureHist` desligado) | desligado | corte ao 1o 73,9% -> 77,0%, mas -11 +/- 17 Elo |
| 12 | ordem | o corte captura boa/ma vem da PROFUNDIDADE; o deles vem do valor do lance | falta | arvore +40 a +90% quando transcrito com a nossa escala |
| 13 | infra | tabela de transposicao: semantica de guarda/leitura por alinhar | falta | por medir |
| 14 | infra | ordem de geracao de lances por alinhar | falta | por medir |
| 15 | eval | avaliacao -- **igual**, e' a deles pela ponte, inteiro a inteiro | igual | -- |
| 16 | 5 | com entrada em falta, a referencia GUARDA ja' a avaliacao na tabela (limite nenhum) para a proxima visita nao reavaliar | falta | por medir |
| 17 | 5 | em xeque nao se avalia: copia-se a estatica de dois plies atras | por conferir | por medir |
| 18 | 5b | `priorReduction >= 3 && !opponentWorsening` -> **profundidade +1** | falta | mexe na profundidade, logo nos nos |
| 19 | 5b | `priorReduction >= 2 && depth >= 2 && soma das estaticas > 166` -> **profundidade -1** | falta | mexe na profundidade, logo nos nos |
| 20 | 5 | `opponentWorsening` (estatica > -estatica do ply anterior) nao existe em nos | falta | alimenta 18 e outras |
| 21 | 6 | corte pela tabela com VERIFICACAO: a prof>=7 joga-se o lance da tabela e confere-se o filho antes de cortar | falta | por medir |
| 22 | 6 | ao cortar pela tabela, CREDITA-SE o historico do lance e penaliza-se o do ply anterior | por conferir | por medir |
| 23 | 8 | razoring com margem QUADRATICA (`482*d*d`) | por conferir | por medir |
| 24 | 9 | margem sem entrada na tabela leva `-20` no multiplicador | falta | por medir |
| 25 | 9 | desconto por `improving`/`opponentWorsening` e' PROPORCIONAL (1024 avos), o nosso e' degrau fixo | falta | por medir |
| 26 | 9 | o corte devolve media pesada `(661*beta + 363*eval)/1024`, nao a avaliacao | por conferir | por medir |
| 27 | 10 | nulo so' se tenta em `cutNode` | por conferir | por medir |
| 28 | 10 | busca de VERIFICACAO do nulo com `nmpMinPly` a partir da profundidade 16 | por conferir | por medir |
| 29 | 11 | o nosso IIR dispara a prof>=4 sem guardas; o deles a prof>=6 com `!followPV && !allNode` | diferente | somos MAIS agressivos |
| 30 | 12 | ProbCut le' capturas pela quiescencia primeiro e so' depois pela busca | por conferir | por medir |
| 31 | 13 | segunda leitura da tabela a `beta+428` ja' dentro do ciclo | por conferir | por medir |
| 32 | 14 | historico de continuacao com **6** plies de contexto; nos temos 3 | falta | alimenta ordem e reducao |
| 33 | 18 | `statScore`: tranquilos = 2252*principal + 1126*cont0 + 1093*cont1, /1024 -- as duas continuacoes juntas pesam MAIS que a principal | falta | as nossas continuacoes valem 301/234 contra 1374 da principal |
| 34 | 18 | `cutoffCnt` -- quantas vezes o ply SEGUINTE ja' cortou; muda o `r` em ate' 2497 | falta | por medir |
| 35 | 18 | `-2179` quando o lance e' o da tabela e o ply seguinte ainda nao cortou | falta | por medir |
| 36 | 18 | `+3 * clamp(alpha - eval, -64, 96)` -- reduz mais quanto mais abaixo de alpha | falta | por medir |
| 37 | 18 | `r += r * 276/(256*d+268)` em `allNode` -- reducao proporcional a si propria | falta | nao temos nocao de allNode |
| 38 | 18 | o `ttPv` desconta ate' 5728 no `r` (4 termos), o nosso desconta um valor fixo e esta' desligado | falta | por medir |
| 39 | 18 | `r -= moveCount * 65` -- desconto pelo numero do lance | por conferir | por medir |
| 40 | 18b | profundidade reduzida grampeada por CIMA (`min(.., newDepth+2)`) e minimo 1 | falta | por medir |
| 41 | 18b | re-busca com **tres** desfechos (mais fundo / igual / mais raso), pelo valor obtido | falta | nos refazemos sempre a` inteira -- o caso mais caro |
| 42 | 18b | sem reducao, a profundidade leva ate' 2 plies de desconto por limiares do `r` | falta | por medir |
| 43 | 14 | legalidade testada TARDE, dentro do ciclo -- a geracao devolve pseudo-legais e a maioria morre por poda antes | por conferir | trabalho poupado por no' |
| 44 | 14 | o `r` e' calculado UMA vez antes de qualquer poda e serve todas | por conferir | nos temos duas contas |
| 45 | 16 | a extensao singular tem QUATRO desfechos: estender 1/2/3, cortar o no', reduzir -3, ou nada | por conferir | uma busca paga tres decisoes |
| 46 | 16 | ao falhar acima de beta a singular CORTA o no' inteiro e credita a correccao | por conferir | por medir |
| 47 | 16 | extensao de **-3** quando o valor guardado ja' passava beta ou e' no' de corte | falta | por medir |
| 48 | 23 | o corte devolve `(melhor*prof + beta)/(prof+1)` -- atenuado pela profundidade | falta | nos devolvemos o numero cheio |
| 49 | 23 | credito ao ply anterior com escala de SEIS parcelas, distribuido por tres tabelas com pesos diferentes | falta | por medir |
| 50 | 23 | `ttMoveHistory` -- tabela propria que mede se o lance da tabela costuma ser o melhor | falta | por medir |
| 51 | 24 | o `ttPv` PROPAGA-SE para tras quando o no' falha abaixo | falta | por medir |
| 52 | 22 | esforco por lance da raiz com peso adaptativo (12 a 24 em 32) para a media movel | falta | alimenta a gestao de tempo |
| 53 | geral | **o valor devolvido e' proporcional a` confianca**: passo 9 media beta/eval, passo 23 media pela profundidade | falta | principio, nao parametro |
| 54 | ordem | maquina de ETAPAS: gera o menos possivel, o mais tarde possivel; tranquilos so' se nenhuma captura boa cortar | falta | 92% dos nossos nos cortam sem tranquilo, e geramos tranquilos em todos |
| 55 | ordem | capturas que falham o SEE sao ADIADAS para depois dos tranquilos bons, nao deitadas fora | falta | por medir |
| 56 | ordem | ordenacao dos tranquilos e' PARCIAL, com corte a `-3560*prof` | falta | por medir |
| 57 | ordem | limiar tranquilo bom/mau fixo em -14000, independente da profundidade | falta | por medir |
| 58 | qsearch | corte por CONTAGEM na quiescencia | **TEMOS** (`TravaoQs`), com as mesmas isencoes | verificar valor e se esta ligado |
| 59 | qsearch | a RECAPTURA na casa onde o adversario comeu escapa a todos os filtros | falta | por medir |
| 60 | qsearch | ao podar por futilidade, o melhor valor SOBE para a futilidade -- nao se perde informacao | falta | por medir |
| 61 | qsearch | SEE fixo a -74 nas capturas da quiescencia | por conferir | por medir |
| 62 | qsearch | em xeque, todos os filtros ficam desligados | por conferir | por medir |
| 63 | qsearch | entradas da quiescencia guardadas com profundidade propria (`DEPTH_QS`) | por conferir | por medir |

## 2026-09-09  half2k 1.20260009 contra o binario que estava no bot

**+6,60 +/- 4,99 em 4.840 partidas, LLR 2,09, intervalo inteiro acima de zero.**

O que mudou em relacao ao `h2k_novo5`:
  * `AlphaDesce` DESLIGADO -- estava ligado por omissao e mediu -21,37 +/- 9,41
    contra este mesmo adversario. Sozinho, -44,06 +/- 11,92 em 888 partidas.
  * o anuncio das opcoes passou a ser DERIVADO do valor real em vez de sair de
    duas listas escritas a` mao. O motor anunciava `Razoring default false` com
    o razoring ligado; um arbitro que ponha as opcoes no valor anunciado
    desligava-as.
  * `Features::set` deixou de ter efeito lateral fora da estrutura (o
    `semponte` escrevia num global), o que o torna seguro para perguntar.
  * `RecusaLimiar` 0 -> 300 e `RecusaMargem` 30 -> 20 gravados por omissao.

===== outra leitura =====

# Lista de validacoes -- transcricao contra a referencia

Cada linha e' uma diferenca ENCONTRADA entre a busca da referencia e a nossa,
com o estado da sua resolucao. Nada de prosa: o que interessa e' a coluna do
estado e o efeito medido quando existir.

Legenda do estado: `falta` (nao temos), `parcial` (temos incompleto),
`desligado` (temos mas nao corre), `transcrito` (ja' esta' no `refbusca.rs`),
`igual` (nao ha' diferenca).

| # | passo | diferenca | estado | efeito medido |
|---|---|---|---|---|
| 1 | 0 | `upcoming_repetition`: repeticao que o adversario pode forcar sobe o alpha antes de procurar | falta | po
| 2 | 2 | valor de empate com ruido de paridade em vez de zero | transcrito | por medir |
| 3 | 2 | teste de empate so' tem regra dos 50; falta repeticao e material insuficiente | parcial | por medir |
| 4 | 1 | `followPV` -- "este no' esta' na linha que a iteracao anterior deu como melhor" | falta | por medir |
| 5 | 4 | `ttPv` e' calculado mas o termo que o usa na reducao esta' desligado (`TtpvLmr`) | desligado | por medir 
| 6 | 18 | `r` da reducao: 4 dos 9 termos nunca disparam (janela, captura, cut node, tt-pv) | desligado | modulo me
| 7 | 9 | margem da futilidade inversa cresce sem tecto (`110*prof`); a deles satura | ARRANJADO | arvore 2,76x -> 
| 8 | 15 | `skip_quiets` parava todos os tranquilos; a deles poda so' aquele lance | ARRANJADO | -17,4 -> -4,1 Elo 
| 9 | 15 | faltava a guarda do PV no ramo dos tranquilos | ARRANJADO | incluido no anterior |
| 10 | 15 | historico de capturas nao era lido (dois termos a zero) | ARRANJADO | os dois parametros passaram a mex
| 11 | ordem | historico de capturas nao entra na ordem por omissao (`CaptureHist` desligado) | desligado | corte a
| 12 | ordem | o corte captura boa/ma vem da PROFUNDIDADE; o deles vem do valor do lance | falta | arvore +40 a +90
| 13 | infra | tabela de transposicao: semantica de guarda/leitura por alinhar | falta | por medir |
| 14 | infra | ordem de geracao de lances por alinhar | falta | por medir |
| 15 | eval | avaliacao -- **igual**, e' a deles pela ponte, inteiro a inteiro | igual | -- |
| 16 | 5 | com entrada em falta, a referencia GUARDA ja' a avaliacao na tabela (limite nenhum) para a proxima visit
| 17 | 5 | em xeque nao se avalia: copia-se a estatica de dois plies atras | por conferir | por medir |
| 18 | 5b | `priorReduction >= 3 && !opponentWorsening` -> **profundidade +1** | falta | mexe na profundidade, logo
| 19 | 5b | `priorReduction >= 2 && depth >= 2 && soma das estaticas > 166` -> **profundidade -1** | falta | mexe n
| 20 | 5 | `opponentWorsening` (estatica > -estatica do ply anterior) nao existe em nos | falta | alimenta 18 e out
| 21 | 6 | corte pela tabela com VERIFICACAO: a prof>=7 joga-se o lance da tabela e confere-se o filho antes de cor
| 22 | 6 | ao cortar pela tabela, CREDITA-SE o historico do lance e penaliza-se o do ply anterior | por conferir | 
| 23 | 8 | razoring com margem QUADRATICA (`482*d*d`) | por conferir | por medir |
| 24 | 9 | margem sem entrada na tabela leva `-20` no multiplicador | falta | por medir |
| 25 | 9 | desconto por `improving`/`opponentWorsening` e' PROPORCIONAL (1024 avos), o nosso e' degrau fixo | falta
| 26 | 9 | o corte devolve media pesada `(661*beta + 363*eval)/1024`, nao a avaliacao | por conferir | por medir |
| 27 | 10 | nulo so' se tenta em `cutNode` | por conferir | por medir |
| 28 | 10 | busca de VERIFICACAO do nulo com `nmpMinPly` a partir da profundidade 16 | por conferir | por medir |
| 29 | 11 | o nosso IIR dispara a prof>=4 sem guardas; o deles a prof>=6 com `!followPV && !allNode` | diferente | 
| 30 | 12 | ProbCut le' capturas pela quiescencia primeiro e so' depois pela busca | por conferir | por medir |
| 31 | 13 | segunda leitura da tabela a `beta+428` ja' dentro do ciclo | por conferir | por medir |
| 32 | 14 | historico de continuacao com **6** plies de contexto; nos temos 3 | falta | alimenta ordem e reducao |
| 33 | 18 | `statScore`: tranquilos = 2252*principal + 1126*cont0 + 1093*cont1, /1024 -- as duas continuacoes junta
| 34 | 18 | `cutoffCnt` -- quantas vezes o ply SEGUINTE ja' cortou; muda o `r` em ate' 2497 | falta | por medir |
| 35 | 18 | `-2179` quando o lance e' o da tabela e o ply seguinte ainda nao cortou | falta | por medir |
| 36 | 18 | `+3 * clamp(alpha - eval, -64, 96)` -- reduz mais quanto mais abaixo de alpha | falta | por medir |
| 37 | 18 | `r += r * 276/(256*d+268)` em `allNode` -- reducao proporcional a si propria | falta | nao temos nocao 
| 38 | 18 | o `ttPv` desconta ate' 5728 no `r` (4 termos), o nosso desconta um valor fixo e esta' desligado | falta
| 39 | 18 | `r -= moveCount * 65` -- desconto pelo numero do lance | por conferir | por medir |
| 40 | 18b | profundidade reduzida grampeada por CIMA (`min(.., newDepth+2)`) e minimo 1 | falta | por medir |
| 41 | 18b | re-busca com **tres** desfechos (mais fundo / igual / mais raso), pelo valor obtido | falta | nos refa
| 42 | 18b | sem reducao, a profundidade leva ate' 2 plies de desconto por limiares do `r` | falta | por medir |
| 43 | 14 | legalidade testada TARDE, dentro do ciclo -- a geracao devolve pseudo-legais e a maioria morre por poda
| 44 | 14 | o `r` e' calculado UMA vez antes de qualquer poda e serve todas | por conferir | nos temos duas contas 
| 45 | 16 | a extensao singular tem QUATRO desfechos: estender 1/2/3, cortar o no', reduzir -3, ou nada | por confe
| 46 | 16 | ao falhar acima de beta a singular CORTA o no' inteiro e credita a correccao | por conferir | por medir
| 47 | 16 | extensao de **-3** quando o valor guardado ja' passava beta ou e' no' de corte | falta | por medir |
| 48 | 23 | o corte devolve `(melhor*prof + beta)/(prof+1)` -- atenuado pela profundidade | falta | nos devolvemos 
| 49 | 23 | credito ao ply anterior com escala de SEIS parcelas, distribuido por tres tabelas com pesos diferentes 
| 50 | 23 | `ttMoveHistory` -- tabela propria que mede se o lance da tabela costuma ser o melhor | falta | por medi
| 51 | 24 | o `ttPv` PROPAGA-SE para tras quando o no' falha abaixo | falta | por medir |
| 52 | 22 | esforco por lance da raiz com peso adaptativo (12 a 24 em 32) para a media movel | falta | alimenta a g
| 53 | geral | **o valor devolvido e' proporcional a` confianca**: passo 9 media beta/eval, passo 23 media pela pro
| 54 | ordem | maquina de ETAPAS: gera o menos possivel, o mais tarde possivel; tranquilos so' se nenhuma captura b
| 55 | ordem | capturas que falham o SEE sao ADIADAS para depois dos tranquilos bons, nao deitadas fora | falta | p
| 56 | ordem | ordenacao dos tranquilos e' PARCIAL, com corte a `-3560*prof` | falta | por medir |
| 57 | ordem | limiar tranquilo bom/mau fixo em -14000, independente da profundidade | falta | por medir |
| 58 | qsearch | corte por CONTAGEM na quiescencia | **TEMOS** (`TravaoQs`), com as mesmas isencoes | verificar val
| 59 | qsearch | a RECAPTURA na casa onde o adversario comeu escapa a todos os filtros | falta | por medir |
| 60 | qsearch | ao podar por futilidade, o melhor valor SOBE para a futilidade -- nao se perde informacao | falta 
| 61 | qsearch | SEE fixo a -74 nas capturas da quiescencia | por conferir | por medir |
| 62 | qsearch | em xeque, todos os filtros ficam desligados | por conferir | por medir |
| 63 | qsearch | entradas da quiescencia guardadas com profundidade propria (`DEPTH_QS`) | por conferir | por medir

## 2026-09-09  half2k 1.20260009 contra o binario que estava no bot

**+6,60 +/- 4,99 em 4.840 partidas, LLR 2,09, intervalo inteiro acima de zero.**