#!/usr/bin/env python3
"""Build the published logo and banner from the source artwork.

The artwork was generated with Google Gemini (see NOTICES.md) and lands here
as two pictures that are not yet usable: the logo is 4:3 with the bird off
centre, the banner carries the generator's signature star, and neither has
usable lettering -- an earlier attempt came back reading "writen in Rust" and
"thrat features", so the generator is now asked for the picture alone and the
words are set here.

This script is the whole path from those two files to everything the project
ships, so the assets can be rebuilt and adjusted rather than being binaries
nobody can regenerate. Run it from this directory:

    python3 build.py

Reads `logo-source.webp` and `banner-source.webp`. Writes `logo.png`,
`logo-512.png`, `icon-256.png` and `banner.png`.
"""

import os
import numpy as np
from PIL import Image, ImageDraw, ImageFont

AQUI = os.path.dirname(os.path.abspath(__file__))


def fonte(nomes, tamanho):
    """First font that exists, at the given size.

    Searched rather than hard-coded because the machine that renders this is
    not necessarily the machine that has the fonts, and a banner that crashes
    on a missing file is worse than one set in the fallback.
    """
    caminhos = [
        "/opt/flutter/bin/cache/artifacts/material_fonts/{}.ttf",
        "/usr/share/fonts/truetype/roboto/unhinted/RobotoTTF/{}.ttf",
        "/usr/share/fonts/truetype/{}.ttf",
    ]
    for nome in nomes:
        for padrao in caminhos:
            p = padrao.format(nome)
            if os.path.exists(p):
                return ImageFont.truetype(p, tamanho)
    return ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", tamanho)


def escreve(d, xy, texto, f, cor, espaco=0):
    """Draw text with letter spacing, which PIL does not do on its own."""
    x, y = xy
    for ch in texto:
        d.text((x, y), ch, font=f, fill=cor)
        x += d.textlength(ch, font=f) + espaco
    return x


def largura(d, texto, f, espaco=0):
    return sum(d.textlength(c, font=f) + espaco for c in texto) - espaco


def cantos_redondos(img, raio):
    mascara = Image.new("L", img.size, 0)
    ImageDraw.Draw(mascara).rounded_rectangle([0, 0, img.size[0] - 1, img.size[1] - 1],
                                              radius=raio, fill=255)
    saida = img.copy()
    saida.putalpha(mascara)
    return saida


def encontra_marca(a):
    """Locate the generator's signature star, or return None.

    Found rather than hard-coded, because every regenerated image puts it
    somewhere slightly different and a measured coordinate silently stops
    covering anything the moment the artwork changes -- leaving the mark in the
    published file with the script still reporting success.

    It is identified by being the one thing in that corner that is both bright
    and colourless: the artwork there is either near-black or strongly orange,
    so "light and neutral" picks out the star and nothing else.
    """
    alt, larg = a.shape[:2]
    mx, mn = a.max(axis=2), a.min(axis=2)
    alvo = (mx > 90) & ((mx - mn) < 30)
    alvo[: int(alt * 0.6), :] = False        # so o terco de baixo
    alvo[:, : int(larg * 0.6)] = False       # so a direita
    ys, xs = np.nonzero(alvo)
    if len(xs) < 40:
        return None

    # Trim to the compact cluster before measuring it. The lettering has white
    # cores that also read as bright and neutral, and taken together with the
    # star they gave a centre between the two and a radius of 247 pixels -- a
    # "mark" a quarter of the frame wide. The median survives that; the mean
    # and the bounding box do not.
    for _ in range(4):
        mx_, my_ = np.median(xs), np.median(ys)
        perto = (np.abs(xs - mx_) < 90) & (np.abs(ys - my_) < 90)
        if perto.sum() < 40:
            break
        xs, ys = xs[perto], ys[perto]

    raio = max(xs.max() - xs.min(), ys.max() - ys.min()) / 2
    if raio > 120:
        return None
    return int(np.median(xs)), int(np.median(ys)), raio


def apaga_marca(a, cx, cy, raio, desloca=130):
    """Paint out a mark by borrowing texture from beside it.

    Darkening it would leave a smudge: it sits on the chessboard rather than on
    flat black, and the eye finds a missing square faster than it finds a star.
    Copying the same image shifted sideways brings that texture with it, so the
    board keeps running through where the mark was.

    The weight reaches 1 well before the edge of the disc. Feathering that
    starts at the centre leaves the outline of what was removed, which is how
    the first attempt still showed a ghost.
    """
    yy, xx = np.mgrid[0:a.shape[0], 0:a.shape[1]]
    dist = np.sqrt(((xx - cx) / raio) ** 2 + ((yy - cy) / raio) ** 2)
    peso = np.clip((1.0 - dist) * 2.2, 0, 1)[:, :, None]
    return a * (1 - peso) + np.roll(a, desloca, axis=1) * peso


def constroi_logo(origem, lado):
    """Square off the logo artwork around the bird.

    The source is 4:3 with the bird left of centre, so it is cropped rather
    than scaled: an icon that keeps the original framing spends half its pixels
    on background, and the bird has to survive being 32 pixels wide.

    The crop is chosen by eye and written down, not detected. There is one
    image; a detector here would be machinery guarding a constant. It does drop
    the corner the signature star sits in -- checked below rather than assumed,
    because silence would publish it.
    """
    a = np.asarray(Image.open(origem).convert("RGB")).astype(np.float32)
    marca = encontra_marca(a)
    img = Image.fromarray(a.astype(np.uint8)).convert("RGBA")

    CX, CY, L = 975, 735, 1150
    caixa = (CX - L // 2, CY - L // 2, CX + L // 2, CY + L // 2)
    if marca and caixa[0] <= marca[0] <= caixa[2] and caixa[1] <= marca[1] <= caixa[3]:
        print(f"  AVISO: marca do gerador em {marca[:2]} cai DENTRO do corte")

    img = img.crop(caixa).resize((lado, lado), Image.LANCZOS)
    return cantos_redondos(img, int(lado * 0.19))


def constroi_banner(origem, W=1920, H=1080):
    """Remove the signature from the banner artwork and set the type."""
    a = np.asarray(Image.open(origem).convert("RGB")).astype(np.float32)
    marca = encontra_marca(a)
    if marca:
        cx, cy, r = marca
        print(f"  marca do gerador em ({cx}, {cy}), raio {r:.0f} -- apagada")
        a = apaga_marca(a, cx, cy, raio=r * 2.4)
    else:
        print("  sem marca do gerador a apagar")

    # A gentle darkening under the type. Not to hide anything -- there is
    # nothing left to hide -- but so the words sit on something quiet instead
    # of on the middle of a chessboard.
    y = np.arange(a.shape[0])[:, None, None].astype(np.float32)
    a = a * (1 - np.clip((y - 850) / 260.0, 0, 1) ** 1.2 * 0.55)
    img = Image.fromarray(a.astype(np.uint8)).convert("RGBA")

    d = ImageDraw.Draw(img)
    f1 = fonte(["Roboto-Regular"], 52)
    f2 = fonte(["Roboto-Light", "Roboto-Regular"], 40)
    for texto, f, cor, yy in [
        ("A chess engine, written in Rust", f1, (238, 176, 100), 930),
        ("Neural evaluation with threat features", f2, (156, 134, 110), 1010),
    ]:
        escreve(d, (1330 - largura(d, texto, f, 2) / 2, yy), texto, f, cor, espaco=2)

    # The source is 1.792 wide, not 1.778. Trim the width rather than the
    # height: there is spare background at the sides and none under the bird.
    novo_w = int(round(img.size[1] * 16 / 9))
    corte = (img.size[0] - novo_w) // 2
    img = img.crop((corte, 0, corte + novo_w, img.size[1]))
    return img.resize((W, H), Image.LANCZOS).convert("RGB")


def main():
    arte_logo = os.path.join(AQUI, "logo-source.webp")
    arte_banner = os.path.join(AQUI, "banner-source.webp")
    for p in (arte_logo, arte_banner):
        if not os.path.exists(p):
            raise SystemExit(f"falta a arte de origem: {p}")

    for lado, nome in [(1024, "logo.png"), (512, "logo-512.png"), (256, "icon-256.png")]:
        constroi_logo(arte_logo, lado).save(os.path.join(AQUI, nome))
        print(f"{nome:<15} {lado}x{lado}")

    constroi_banner(arte_banner).save(os.path.join(AQUI, "banner.png"))
    print("banner.png      1920x1080")


if __name__ == "__main__":
    main()
