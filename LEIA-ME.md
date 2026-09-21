# half2k, estado de 13 de Setembro de 2026 23h56

Copiado de `/root/contempt-half2k` no server 1 a 19-09, depois de a ob6 morrer.
**Não tinha controlo de versões nenhum** — era a árvore de half2k mais recente
que existia e vivia só num disco.

## Porque está aqui e não no `half2k.git`

O `half2k.git` é **público** e o seu último commit é de **1 de Setembro**
(`43e88f6`). Esta árvore está doze dias à frente, mas traz o aparelho de
medição por FFI, que pela regra do João não vai para repositório público:

```
nnue_sf.rs        4347 linhas    14 marcas
sf_features.rs    1378 linhas     9
medidor.rs         303 linhas     6
```

## O que dela PODE ir para o repositório público

Estes estão limpos de marcas e são o trabalho de motor entre 1 e 13 de
Setembro:

```
search.rs      2848 linhas diferentes do que esta' publicado    ZERO marcas
nnue.rs         407
uci.rs          264
movegen.rs       44
board.rs         42
main.rs          21
```

As 2848 linhas do `search.rs` são o grosso do valor. Extrair isso para o
`half2k.git` é trabalho a fazer com calma; guardar a árvore inteira era
urgente, e é o que isto é.
