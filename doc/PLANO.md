# Plano

## O que ja' esta' medido e vem do `half2k`

Tudo o que se segue foi medido em 8 de Setembro de 2026 e entra ja' com o valor
encontrado, nao com o de origem.

| peca | medida |
|---|---|
| gerador por familias (conjuntos, nao peca a peca) | mesma ordem de emissao da referencia; perft ao no' |
| killers fora da banda | corte ao primeiro lance 76,6% -> 79,7% |
| ordenacao com continuacoes a dominar | 73,0% -> 75,7% e arvore -20% |
| historia de capturas na ordem | +1,8 pontos de acerto |
| corte captura boa/ma pelo MERITO do lance | os dois eixos melhoram |
| futilidade inversa com multiplicador que satura | arvore 2,76x -> 1,25x |
| fiabilidade da tabela na extensao singular | 1,13x -> 1,05x |
| gestao de tempo: deixar COMECAR a iteracao seguinte | 3,7% -> 5,9% do relogio, +1 ply |
| descida de profundidade quando o alpha sobe, PROPORCIONAL a` evidencia | 2,05x -> 0,81x |

## O que foi medido e REJEITADO -- nao repetir

| ideia | porque' |
|---|---|
| alinhar a arvore pela forma | -65 Elo. A forma nao e' qualidade |
| geracao por etapas | -4,5% de instrucoes mas -3,9 pontos de ordem |
| bonus de killer no historico | -16,4% de arvore e +0,8 de acerto, e -2,48 +/- 9,62 em partidas |
| transplantar a reducao inteira | arvore x5. As constantes nao transferem |

## Ordem de trabalho

1. Esqueleto que compila e joga: posicao e rede do `vendor/`, busca minima nossa.
2. Portar a busca do `half2k` passo a passo, com o perfil a decidir a ordem.
3. Cada peca entra atras de interruptor com `0 = OFF = byte-identico`.
4. So' depois afinacao, e sempre em partidas.