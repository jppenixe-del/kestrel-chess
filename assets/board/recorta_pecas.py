#!/usr/bin/env python3
"""Cut the twelve pieces out of the generated sheet, onto transparent squares.

The sheet is a single image: two rows of six on a flat green, with a caption
under each. What comes out is what the board actually loads -- 320x320 RGBA,
one file per piece.
"""
from PIL import Image
import numpy as np
import sys, os

ORIGEM = sys.argv[1] if len(sys.argv) > 1 else 'pecas_src.jpeg'
DESTINO = sys.argv[2] if len(sys.argv) > 2 else 'pecas_novas'
os.makedirs(DESTINO, exist_ok=True)

a = np.asarray(Image.open(ORIGEM).convert('RGB')).astype(np.float32)
FUNDO = np.array([50.0, 90.0, 63.0])

# Um pixel de borda e' a peca misturada com o verde do fundo. A cobertura sai da
# distancia a essa cor, e a cor tem de ser DESMISTURADA a seguir -- sem isso
# cada peca fica com uma auréola esverdeada assim que assentar num tabuleiro,
# que e' precisamente onde vai assentar.
dist = np.linalg.norm(a - FUNDO[None, None, :], axis=2)
alpha = np.clip((dist - 18.0) / 34.0, 0, 1)
alpha[alpha < 0.10] = 0.0
seguro = np.maximum(alpha, 0.03)[:, :, None]
puro = np.clip((a - FUNDO[None, None, :] * (1 - alpha[:, :, None])) / seguro, 0, 255)
rgba = np.dstack([puro, alpha * 255]).astype(np.uint8)

# Bandas MEDIDAS do perfil de linhas com conteudo, nao adivinhadas. As chamas
# sobem muito acima do corpo: um recorte apertado corta-as, e um rei sem coroa
# so' se ve depois de estar publicado.
NOMES = ["K", "Q", "R", "B", "N", "P"]
FILAS = {"w": (115, 340), "b": (400, 700)}

m = alpha > 0.35
def colunas(y0, y1):
    col = m[y0:y1].sum(axis=0)
    gs, ini = [], None
    for x, v in enumerate(col):
        if v > 3 and ini is None: ini = x
        elif v <= 3 and ini is not None:
            if x - ini > 30: gs.append((ini, x))
            ini = None
    if ini is not None: gs.append((ini, len(col)))
    return gs

rec = {}
for cor, (y0, y1) in FILAS.items():
    gs = colunas(y0, y1)
    if len(gs) != 6:
        raise SystemExit(f"fila {cor}: encontrei {len(gs)} pecas, esperava 6")
    for i, (x0, x1) in enumerate(gs):
        sub = rgba[y0:y1, x0:x1]
        ys, xs = np.nonzero(sub[:, :, 3] > 20)
        rec[cor + NOMES[i]] = Image.fromarray(
            sub[ys.min():ys.max() + 1, xs.min():xs.max() + 1], 'RGBA')

LADO, MARGEM = 320, 12
alvo = LADO - 2 * MARGEM

# UMA escala global para as doze. Nada e' emparelhado nem normalizado.
#
# As alturas, dentro de cada fila e entre as duas, sao como foram desenhadas.
# As brancas e as pretas NAO sao a mesma peca noutra cor -- as pretas tem outro
# porte, e isso e' o desenho. Uma normalizacao "para ficarem iguais" apagaria
# precisamente aquilo que distingue os dois lados.
#
# A unica coisa imposta e' a linha de base: a peca mais alta do conjunto enche
# o quadrado, todas as outras escalam com ela pelo mesmo factor, e todas pousam
# no mesmo sitio.
mais_alta = max(img.size[1] for img in rec.values())
g = alvo / mais_alta

for nome, img in rec.items():
    nw = max(1, round(img.size[0] * g))
    nh = max(1, round(img.size[1] * g))
    r = img.resize((nw, nh), Image.LANCZOS)
    tela = Image.new('RGBA', (LADO, LADO), (0, 0, 0, 0))
    # Centrada na horizontal e assente numa linha de base COMUM. A altura e' o
    # que distingue as pecas de relance, e so' funciona se todas pousarem no
    # mesmo sitio.
    tela.alpha_composite(r, ((LADO - nw) // 2, LADO - MARGEM - nh))
    tela.save(os.path.join(DESTINO, f'{nome}.png'))

print("  alturas finais (uma so escala, como desenhadas):")
for cor in ("w", "b"):
    hs = {n: round(rec[cor + n].size[1] * g) for n in NOMES}
    print(f"    {cor}: " + "  ".join(f"{n}={hs[n]:3d}" for n in NOMES))
