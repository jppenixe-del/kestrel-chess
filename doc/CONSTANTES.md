# Constantes: de onde veio cada numero

Regra: **a forma transfere-se, o numero nao.** Uma constante herdada esta'
calibrada para outra escala de avaliacao e outras tabelas de historico -- as
nossas sao cerca de cinquenta vezes menores -- e usa-la sem medir e' ao mesmo
tempo clonar e ficar errado.

Cada linha diz se o valor foi VARRIDO na nossa arvore, e se por acaso caiu no
mesmo sitio que o de outro motor, isso e' dito.

| constante | valor | origem |
|---|---|---|
| `RfpBase` | 40 | varrido; outro motor usa 45 |
| `RfpDecl` | 5 | varrido; outro usa 4 |
| `RfpSemTt` | 34 | varrido; outro usa 20 |
| `RfpImpr` | 5600 | varrido; outro usa 2789 |
| `RfpAdv` | 290 | varrido; outro usa 335 |
| `CaptBarDiv` | 60 | varrido; outro usa 18 |
| `OrdemPrincF` | 138 | varrido; outro usa 69 |
| `HistBonusDecl` | 200 | varrido; outro usa 131 |
| `FiabSobe` / `FiabDesce` | 918 / 747 | varridos, caem no mesmo valor, regiao com curvatura |
| `AlphaDesceAmt` | 2 | nosso; a forma de origem e' fixa em 3 sem limiares |
| `AlphaDesceM1` / `M2` | 25 / 90 | nossos; nao existem na origem |
| `AlphaDesceCedo` | 6 | nosso; nao existe na origem |
| `TmEstim` | 7 | nosso |