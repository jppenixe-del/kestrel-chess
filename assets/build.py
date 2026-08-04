#!/usr/bin/env python3
"""Build the logo and banner from the falcon artwork.

The mark itself was generated with Google Gemini (see NOTICES.md). What that
produced was a falcon on a flat white square, which is not usable as-is: an
avatar needs the bird cut out with a real alpha channel, and a banner needs it
placed against typography.

This script is the whole path from that one image to every file the project
ships, so the assets can be rebuilt rather than being binaries nobody can
reproduce. Run it from this directory:

    python3 build.py falcon-source.webp

Outputs `falcon.png` (the cut-out), `logo.png`, `logo-512.png`, `icon-256.png`
and `banner.png`.
"""

import sys
import os
import numpy as np
from PIL import Image, ImageDraw, ImageFont

AQUI = os.path.dirname(os.path.abspath(__file__))

FUNDO = (18, 22, 28)          # #12161C
FUNDO_ESCURO = (11, 14, 19)   # #0B0E13
TEXTO = (244, 246, 248)
TEXTO_2 = (154, 166, 178)
TEXTO_3 = (107, 118, 131)
ACENTO = (232, 132, 60)       # #E8843C


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


def recorta(origem):
    """Cut the falcon off its white background, with a real alpha channel.

    A border pixel is the bird mixed with the white behind it:

        p = alfa*C + (1 - alfa)*255

    so the coverage can be recovered from any channel, and the blue one gives
    the most contrast because the bird is orange (blue near 50) against white
    (blue at 255). Recovering the colour means undoing that mix -- without it
    every edge stays milky and the bird wears a bright halo the moment it sits
    on a dark background, which is exactly where it is going.
    """
    a = np.asarray(Image.open(origem).convert("RGB")).astype(np.float32)
    alpha = np.clip((252.0 - a[:, :, 2]) / 190.0, 0, 1)

    # Compressed white is not exactly 255; it wobbles, and that wobble clears
    # the threshold and fills the image with invisible residual alpha. A pixel
    # with 4% coverage is not bird, it is codec noise -- and left in, the crop
    # below finds a bounding box the size of the whole canvas.
    alpha[alpha < 0.08] = 0.0

    seguro = np.maximum(alpha, 0.02)[:, :, None]
    puro = np.clip((a - 255.0 * (1 - alpha[:, :, None])) / seguro, 0, 255)
    img = Image.fromarray(np.dstack([puro, alpha * 255]).astype(np.uint8), "RGBA")
    return img.crop(img.getbbox())


def xadrez(img, tam, opacidade, x0, y0, x1, y1):
    """A chessboard, drawn faint enough to read as texture and not as a board.

    It is the only thing in the mark that says what the engine plays. Loud
    enough to notice, quiet enough that the silhouette still carries it.
    """
    camada = Image.new("RGBA", img.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(camada)
    for i, x in enumerate(range(x0, x1, tam)):
        for j, y in enumerate(range(y0, y1, tam)):
            if (i + j) % 2 == 0:
                d.rectangle([x, y, x + tam - 1, y + tam - 1],
                            fill=(255, 255, 255, opacidade))
    return Image.alpha_composite(img, camada)


def cantos_redondos(img, raio):
    mascara = Image.new("L", img.size, 0)
    ImageDraw.Draw(mascara).rounded_rectangle([0, 0, img.size[0] - 1, img.size[1] - 1],
                                              radius=raio, fill=255)
    saida = img.copy()
    saida.putalpha(mascara)
    return saida


def escreve(d, xy, texto, f, cor, espaco=0):
    """Draw text with letter spacing, which PIL does not do on its own."""
    x, y = xy
    for ch in texto:
        d.text((x, y), ch, font=f, fill=cor)
        x += d.textlength(ch, font=f) + espaco
    return x


def largura(d, texto, f, espaco=0):
    return sum(d.textlength(c, font=f) + espaco for c in texto) - espaco


def constroi_logo(falcao, lado):
    img = Image.new("RGBA", (lado, lado), FUNDO + (255,))
    # The board only goes on the large sizes. Below about 256 pixels its
    # squares are smaller than the eye resolves and it stops reading as a
    # chessboard, turning into dirt around the bird instead.
    if lado > 256:
        m = lado // 8
        img = xadrez(img, lado // 16, 7, m, m, lado - m, lado - m)

    # Two thirds of the height, which leaves the margin an icon needs before
    # a platform crops it to a circle.
    alvo_h = int(lado * 0.60)
    w = int(falcao.size[0] * alvo_h / falcao.size[1])
    ave = falcao.resize((w, alvo_h), Image.LANCZOS)

    # Nudged left of centre: the beak juts right, so geometric centring reads
    # as sitting too far right.
    x = (lado - w) // 2 - int(lado * 0.015)
    y = (lado - alvo_h) // 2
    img.alpha_composite(ave, (x, y))
    return cantos_redondos(img, int(lado * 0.19))


def constroi_banner(falcao, W=1920, H=1080):
    # Diagonal gradient, dark to darker. Flat black reads as a hole; a slope
    # this shallow is not seen so much as felt.
    xs = np.linspace(0, 1, W)[None, :]
    ys = np.linspace(0, 1, H)[:, None]
    t = np.clip((xs + ys) / 2, 0, 1)[:, :, None]
    grad = (np.array(FUNDO)[None, None, :] * (1 - t)
            + np.array(FUNDO_ESCURO)[None, None, :] * t)
    img = Image.fromarray(np.dstack([grad, np.full((H, W), 255)]).astype(np.uint8), "RGBA")

    img = xadrez(img, 90, 6, 1180, 640, 1900, 1080)

    # The bird sits in the left third. Kept well inside the vertical middle:
    # GitHub crops the top and bottom off this image on the repository page,
    # so anything near an edge is something nobody will see.
    alvo_h = 620
    w = int(falcao.size[0] * alvo_h / falcao.size[1])
    ave = falcao.resize((w, alvo_h), Image.LANCZOS)
    img.alpha_composite(ave, (300, (H - alvo_h) // 2))

    d = ImageDraw.Draw(img)
    f_nome = fonte(["Roboto-Bold"], 170)
    f_sub = fonte(["Roboto-Regular"], 48)
    f_sub2 = fonte(["Roboto-Light", "Roboto-Regular"], 34)

    X = 820
    escreve(d, (X, 330), "KESTREL", f_nome, TEXTO, espaco=8)

    # A rule under the name, fading out to the right so it ends rather than
    # stops.
    #
    # Drawn on its own layer and composited. Painting a low-alpha colour
    # straight onto the image does not blend it -- PIL replaces the pixel,
    # alpha and all -- so the fade came out white instead of vanishing.
    lg = int(largura(d, "KESTREL", f_nome, 8))
    regua = Image.new("RGBA", img.size, (0, 0, 0, 0))
    dr = ImageDraw.Draw(regua)
    for i in range(lg):
        alfa = int(255 * (1 - i / lg) ** 1.4)
        dr.line([(X + i, 545), (X + i, 550)], fill=ACENTO + (alfa,))
    img = Image.alpha_composite(img, regua)
    d = ImageDraw.Draw(img)

    escreve(d, (X, 600), "A chess engine, written in Rust", f_sub, TEXTO_2, espaco=1)
    escreve(d, (X, 690), "Neural evaluation with threat features", f_sub2, TEXTO_3, espaco=1)
    return img


def main():
    origem = sys.argv[1] if len(sys.argv) > 1 else os.path.join(AQUI, "falcon-source.webp")
    if not os.path.exists(origem):
        sys.exit(f"nao encontro a arte de origem: {origem}")

    falcao = recorta(origem)
    falcao.save(os.path.join(AQUI, "falcon.png"))
    print(f"falcon.png      {falcao.size[0]}x{falcao.size[1]}")

    for lado, nome in [(1024, "logo.png"), (512, "logo-512.png"), (256, "icon-256.png")]:
        constroi_logo(falcao, lado).save(os.path.join(AQUI, nome))
        print(f"{nome:<15} {lado}x{lado}")

    constroi_banner(falcao).save(os.path.join(AQUI, "banner.png"))
    print("banner.png      1920x1080")


if __name__ == "__main__":
    main()
