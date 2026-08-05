#!/usr/bin/env python3
"""Cut chess pieces out of a generated sheet, onto transparent squares.

A sheet is one image: rows of six pieces on a flat colour, in the order
K Q R B N P. What comes out is what the board actually loads -- 320x320 RGBA,
one file per piece.

    python3 recorta_pecas.py <folha> <destino> <bandas> [altura_max]

`bandas` says what each row of the sheet is, top to bottom, comma separated:
`w` white, `b` black, `-` skip. So `w,b` takes both rows of one sheet, and
`-,b` takes only the second row and leaves the first alone -- which is what a
sheet offering two alternatives needs.

`altura_max` is how tall the tallest piece of that row ends up, in pixels of
the 320 canvas. It is given rather than derived because the two sides can come
from DIFFERENT sheets, and then nothing inside one sheet can say how big it
should be relative to the other. White fills the square at 296; black is
deliberately smaller, at 266.
"""
from PIL import Image
import numpy as np
import sys
import os

NOMES = ["K", "Q", "R", "B", "N", "P"]
LADO = 320
MARGEM = 12


def carrega(caminho):
    """The sheet, and the background colour read off its own corners.

    Measured, not written down: two sheets from the same generator came back
    on different greens -- (50,90,63) and (59,117,76) -- and a hard-coded
    colour keys out nothing at all, producing twelve opaque squares that look
    fine until they reach a board.
    """
    a = np.asarray(Image.open(caminho).convert("RGB")).astype(np.float32)
    cantos = [a[3, 3], a[3, -4], a[-4, 3], a[-4, -4]]
    return a, np.median(np.stack(cantos), axis=0)


def alfa(a, fundo):
    """Coverage per pixel, with the background un-mixed out of the edges.

    An edge pixel is the piece blended with the sheet behind it. Recovering
    coverage is the easy half; un-mixing the colour is the half that matters,
    because an edge left part-background wears a halo of that background the
    moment it lands on a board -- which is exactly where it lands.
    """
    dist = np.linalg.norm(a - fundo[None, None, :], axis=2)
    al = np.clip((dist - 18.0) / 34.0, 0, 1)
    al[al < 0.10] = 0.0
    seguro = np.maximum(al, 0.03)[:, :, None]
    puro = np.clip((a - fundo[None, None, :] * (1 - al[:, :, None])) / seguro, 0, 255)
    return al, np.dstack([puro, al * 255]).astype(np.uint8)


def bandas_de(m, minimo=40):
    """Rows holding pieces, found from the sheet's own row profile.

    Measured rather than guessed. Flames and crests rise well above the
    bodies, hand-written bands came out fifteen pixels too tight, and a
    beheaded king is the kind of thing only visible once it is published.
    """
    linhas = m.sum(axis=1)
    bs, ini = [], None
    for y, v in enumerate(linhas):
        if v > 3 and ini is None:
            ini = y
        elif v <= 3 and ini is not None:
            if y - ini > minimo:
                bs.append((ini, y))
            ini = None
    if ini is not None and len(linhas) - ini > minimo:
        bs.append((ini, len(linhas)))
    return bs


def colunas_de(m, y0, y1):
    col = m[y0:y1].sum(axis=0)
    gs, ini = [], None
    for x, v in enumerate(col):
        if v > 3 and ini is None:
            ini = x
        elif v <= 3 and ini is not None:
            if x - ini > 30:
                gs.append((ini, x))
            ini = None
    if ini is not None:
        gs.append((ini, len(col)))
    return gs


def main():
    if len(sys.argv) < 4:
        raise SystemExit(__doc__)
    folha, destino, spec = sys.argv[1], sys.argv[2], sys.argv[3].split(",")
    alvo = int(sys.argv[4]) if len(sys.argv) > 4 else LADO - 2 * MARGEM
    os.makedirs(destino, exist_ok=True)

    a, fundo = carrega(folha)
    al, rgba = alfa(a, fundo)
    print(f"  fundo medido: {fundo.astype(int)}")

    bandas = bandas_de(al > 0.10)
    if len(bandas) != len(spec):
        raise SystemExit(f"a folha tem {len(bandas)} filas, a spec tem {len(spec)}: {bandas}")

    m = al > 0.35
    for (y0, y1), cor in zip(bandas, spec):
        if cor == "-":
            continue
        gs = colunas_de(m, y0, y1)
        if len(gs) != 6:
            raise SystemExit(f"fila {cor}: encontrei {len(gs)} pecas, esperava 6")
        rec = {}
        for i, (x0, x1) in enumerate(gs):
            sub = rgba[y0:y1, x0:x1]
            ys, xs = np.nonzero(sub[:, :, 3] > 20)
            rec[NOMES[i]] = Image.fromarray(
                sub[ys.min():ys.max() + 1, xs.min():xs.max() + 1], "RGBA")

        # One scale for the whole row. Nothing is normalised piece by piece:
        # relative height between pieces is exactly as drawn, and it is how a
        # player tells a rook from a queen without looking twice. Fitting each
        # piece to its own square would make the pawn as tall as the king.
        g = alvo / max(im.size[1] for im in rec.values())
        for nome, img in rec.items():
            nw = max(1, round(img.size[0] * g))
            nh = max(1, round(img.size[1] * g))
            r = img.resize((nw, nh), Image.LANCZOS)
            tela = Image.new("RGBA", (LADO, LADO), (0, 0, 0, 0))
            # Centred, and standing on a baseline shared by every piece.
            tela.alpha_composite(r, ((LADO - nw) // 2, LADO - MARGEM - nh))
            tela.save(os.path.join(destino, f"{cor}{nome}.png"))
        hs = {n: round(rec[n].size[1] * g) for n in NOMES}
        print(f"  {cor}: " + "  ".join(f"{n}={hs[n]:3d}" for n in NOMES))


if __name__ == "__main__":
    main()
