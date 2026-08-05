#!/usr/bin/env python3
"""Cut chess pieces out of a generated sheet, onto transparent squares.

A sheet is one image: rows of six pieces on a flat colour, in the order
K Q R B N P. What comes out is what the board actually loads -- 320x320 RGBA,
one file per piece.

    python3 recorta_pecas.py <folha> <destino> <bandas> <altura_max>
    python3 recorta_pecas.py <folha> <destino> <bandas> ref:<dir>:<razao>

`bandas` says what each row of the sheet is, top to bottom, comma separated:
`w` white, `b` black, `-` skip. So `w,b` takes both rows of one sheet, and
`-,b` takes only the second row and leaves the first alone -- which is what a
sheet offering two alternatives needs.

`altura_max` is how tall the tallest piece of that row ends up, in pixels of
the 320 canvas. It is given rather than derived because the two sides can come
from DIFFERENT sheets, and then nothing inside one sheet can say how big it
should be relative to the other.

`ref:<dir>:<razao>` sizes each piece against the one already in `<dir>` of the
same type, at that ratio -- so a black rook is a fixed fraction of the white
rook rather than of the black king. That is not the same thing, and the
difference shows: with each row scaled as a unit, the two sheets' internal
proportions came out at ratios from 0.77 to 1.07 between the sides, which put
the black rook TALLER than the white one. A board is read as piece types, not
as sheets.
"""
from PIL import Image
import numpy as np
import sys
import os

NOMES = ["K", "Q", "R", "B", "N", "P"]
LADO = 320
MARGEM = 6


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


def colunas_de(m, y0, y1, quantas=6):
    """The six pieces of a row, split apart.

    Blank columns are enough when the pieces stand clear of each other. They do
    not always: on one sheet two pieces in a flaming row overlap at the tips,
    which merges them into a single group and silently yields five pieces
    instead of six. When that happens the widest group is split at the deepest
    valley in its own column profile -- the narrowest point between two bodies
    -- and the split repeats until the count is right.
    """
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

    while len(gs) < quantas:
        i = max(range(len(gs)), key=lambda k: gs[k][1] - gs[k][0])
        x0, x1 = gs[i]
        # Only the middle is a candidate: the valley between two bodies is
        # never within a quarter-width of either end.
        m0, m1 = x0 + (x1 - x0) // 4, x1 - (x1 - x0) // 4
        if m1 - m0 < 4:
            break
        corte = m0 + int(np.argmin(col[m0:m1]))
        gs[i:i + 1] = [(x0, corte), (corte, x1)]
        gs.sort()
    return gs


def altura_visivel(im):
    """How tall the drawing inside a 320x320 tile actually is."""
    a = np.asarray(im.convert("RGBA"))
    ys = np.nonzero((a[:, :, 3] > 20).any(axis=1))[0]
    return int(ys.max() - ys.min() + 1)


def main():
    if len(sys.argv) < 4:
        raise SystemExit(__doc__)
    folha, destino, spec = sys.argv[1], sys.argv[2], sys.argv[3].split(",")
    arg = sys.argv[4] if len(sys.argv) > 4 else str(LADO - 2 * MARGEM)
    ref = None
    if arg.startswith("ref:"):
        _, refdir, razao = arg.split(":")
        ref = {n: Image.open(os.path.join(refdir, f"w{n}.png")) for n in NOMES}
        ref = {n: altura_visivel(im) * float(razao) for n, im in ref.items()}
        alvo = None
    else:
        alvo = int(arg)
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

        # Either one scale for the whole row -- relative heights exactly as
        # drawn -- or one height per piece, taken from the reference set. The
        # first keeps a sheet honest to itself; the second keeps two sheets
        # honest to each other, which is what a board needs when the sides were
        # generated separately.
        g_fila = None if alvo is None else alvo / max(im.size[1] for im in rec.values())
        for nome, img in rec.items():
            g = g_fila if g_fila is not None else ref[nome] / img.size[1]
            nw = max(1, round(img.size[0] * g))
            nh = max(1, round(img.size[1] * g))
            r = img.resize((nw, nh), Image.LANCZOS)
            tela = Image.new("RGBA", (LADO, LADO), (0, 0, 0, 0))
            # Centred, and standing on a baseline shared by every piece.
            tela.alpha_composite(r, ((LADO - nw) // 2, LADO - MARGEM - nh))
            tela.save(os.path.join(destino, f"{cor}{nome}.png"))
        hs = {n: round(rec[n].size[1] * (g_fila if g_fila is not None else ref[n] / rec[n].size[1]))
              for n in NOMES}
        print(f"  {cor}: " + "  ".join(f"{n}={hs[n]:3d}" for n in NOMES))


if __name__ == "__main__":
    main()
