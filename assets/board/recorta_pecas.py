#!/usr/bin/env python3
"""Cut chess pieces out of a generated sheet, onto transparent squares.

A sheet is one image: two rows of six pieces (white on top, the other side
below) on a flat green background, in the order K Q R B N P. What comes out
is what the board actually loads -- 320x320 RGBA, one file per piece.

    python3 recorta_pecas.py <folha> <destino> [altura_rei_branco]

The white row is cut first, scaled as one unit so its own internal
proportions -- crests, flames, whatever the art draws taller or shorter --
stay exactly as drawn. `altura_rei_branco` sets the scale (default 308, the
value this set ships at) by fixing the king's own height in the 320 canvas;
every other white piece follows the same ratio.

The second row is then cut and scaled per PIECE TYPE against the white
piece already produced, at RATIO_SEGUNDA_COR (1.02). A flat, dark colour
reads smaller than a light one of the same size, so matching pixel heights
exactly would make the second colour look shrunken; the two percent buys
back the illusion. Scaling by type rather than by row matters too -- scaling
the whole row as a unit inherits whatever internal ratio the sheet happened
to draw between its own pieces, which is not the same number as "how this
rook compares to that rook" and has, on past sheets, come out backwards.

Pieces are isolated by CONNECTED COMPONENT, not by a fixed column grid. Two
pieces in this set draw outside their own nominal column -- the small eagle
finial on the bishop, the knight's head -- and a hard grid cut either
beheads the piece or lets a fragment of the neighbour bleed in. A component
is assigned to whichever nominal cell its centroid falls in.

Background removal is by "greenness" (G channel minus the stronger of R/B),
not by un-mixing against one measured background colour. This source has a
second, faint green-tinted halo around its own black outline stroke that is
not the same colour as the flat background behind it; measuring one colour
and un-mixing against it left that halo visible, while thresholding on
greenness directly -- with a soft band so the antialiased edge doesn't get a
hard cut -- did not. Worth re-checking whichever way round if a future sheet
comes from a different generator: the right fix is whichever one is
actually clean on THAT source, checked against a dark background where a
green fringe cannot hide.
"""
from PIL import Image
import numpy as np
from scipy import ndimage
import os
import sys

NOMES = ["K", "Q", "R", "B", "N", "P"]
LADO = 320
MARGEM = 6
RATIO_SEGUNDA_COR = 1.02


def remove_fundo(im):
    arr = np.array(im.convert("RGBA")).astype(np.int16)
    r, g, b, a = arr[..., 0], arr[..., 1], arr[..., 2], arr[..., 3]
    verdura = g - np.maximum(r, b)
    fundo = verdura > 35
    alfa = a.copy()
    suave = np.clip(1.0 - (verdura - 8) / 27.0, 0.0, 1.0)
    alfa = np.where((~fundo) & (verdura > 8), (alfa * suave).astype(np.uint8), alfa)
    alfa[fundo] = 0
    arr[..., 3] = alfa
    return arr.astype(np.uint8)


def isola_pecas(rgba):
    """One connected component per piece, matched to its nominal grid cell."""
    h, w = rgba.shape[0], rgba.shape[1]
    cell_w, cell_h = w / 6, h / 2
    mascara = rgba[..., 3] > 60
    rotulos, n = ndimage.label(mascara, structure=np.ones((3, 3)))

    def isola(lbl):
        ys, xs = np.where(rotulos == lbl)
        x0, x1, y0, y1 = xs.min(), xs.max(), ys.min(), ys.max()
        sub = rgba[y0:y1 + 1, x0:x1 + 1].copy()
        sub_rot = rotulos[y0:y1 + 1, x0:x1 + 1]
        outra = (sub_rot != lbl) & (sub_rot != 0)
        sub[..., 3] = np.where(outra, 0, sub[..., 3])
        return Image.fromarray(sub, "RGBA")

    pecas = {}
    for row, lado in enumerate(["w", "b"]):
        for col, nome in enumerate(NOMES):
            cx, cy = (col + 0.5) * cell_w, (row + 0.5) * cell_h
            lbl = rotulos[int(cy), int(cx)]
            if lbl == 0:
                x0c, x1c = col * cell_w, (col + 1) * cell_w
                y0c, y1c = row * cell_h, (row + 1) * cell_h
                melhor, melhor_d = None, None
                for cand in range(1, n + 1):
                    ys, xs = np.where(rotulos == cand)
                    if len(xs) < 200:
                        continue
                    mx, my = xs.mean(), ys.mean()
                    if x0c <= mx <= x1c and y0c <= my <= y1c:
                        d = (mx - cx) ** 2 + (my - cy) ** 2
                        if melhor_d is None or d < melhor_d:
                            melhor, melhor_d = cand, d
                lbl = melhor
            pecas[(lado, nome)] = isola(lbl)
    return pecas


def coloca(img, altura_alvo, destino):
    g = altura_alvo / img.size[1]
    nw, nh = max(1, round(img.size[0] * g)), max(1, round(img.size[1] * g))
    r = img.resize((nw, nh), Image.LANCZOS)
    tela = Image.new("RGBA", (LADO, LADO), (0, 0, 0, 0))
    tela.alpha_composite(r, ((LADO - nw) // 2, LADO - MARGEM - nh))
    tela.save(destino)


def main():
    if len(sys.argv) < 3:
        raise SystemExit(__doc__)
    folha, destino = sys.argv[1], sys.argv[2]
    altura_rei = int(sys.argv[3]) if len(sys.argv) > 3 else 308
    os.makedirs(destino, exist_ok=True)

    rgba = remove_fundo(Image.open(folha))
    pecas = isola_pecas(rgba)

    g_branca = altura_rei / pecas[("w", "K")].size[1]
    alturas = {}
    for nome in NOMES:
        alt = round(pecas[("w", nome)].size[1] * g_branca)
        coloca(pecas[("w", nome)], alt, os.path.join(destino, f"w{nome}.png"))
        alturas[nome] = alt
    for nome in NOMES:
        alt = round(alturas[nome] * RATIO_SEGUNDA_COR)
        coloca(pecas[("b", nome)], alt, os.path.join(destino, f"b{nome}.png"))

    print("branca:", "  ".join(f"{n}={alturas[n]:3d}" for n in NOMES))


if __name__ == "__main__":
    main()
