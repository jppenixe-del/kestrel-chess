# The board

The engine's own piece set and squares, kept in the repository because they are
part of what the project looks like, and because a public HTTPS URL is what a
browser will actually load.

## The pieces

`wK wQ wR wB wN wP` and the same with `b`. PNG with an alpha channel, 320x320.

Copper and warm flame for white; blackened steel with cold blue light for
black, a raven on its king. The knight is a bird of prey rather than a horse: Kestrel is
a falcon.

**The two sides are not one set in two colours** -- they are separate designs
from separate sheets, and white is the imposing one. The king fills all but a
few pixels of its square, and every black piece stands at **1.02 of the white
piece of the same type** -- very slightly taller, on purpose. A dark shape
reads smaller than a light one of the same size, so matching the numbers
exactly makes black look shrunken. The two percent buys back the illusion.

Two pieces carry their own ratio on top of that: the black rook at 1.131 and
the bishop at 0.96. The sheets do not agree about those two shapes, and the
disagreement is visible when a rook stands next to its counterpart. Fixed by a
number rather than by regenerating the art.

That last part is the whole trick, and it is not the obvious way to do it. The
obvious way scales each row as a unit, so every piece inherits the internal
proportions its own sheet drew. Done that way here, the ratio between the
sides came out anywhere from 0.77 to 1.07 depending on the piece -- which put
the black rook TALLER than the white one. A board is read as piece types, not
as sheets: what has to match across the sides is rook against rook.

Within white nothing is normalised. Relative heights between white pieces are
exactly as drawn, flames and crests included, which is why the rook stands
well below the bishop. One baseline for all twelve.

Heights as shipped, in pixels of the 320 canvas:

| | K | Q | R | B | N | P |
|---|---|---|---|---|---|---|
| white | 308 | 304 | 221 | 298 | 250 | 167 |
| black | 314 | 310 | 250 | 286 | 255 | 170 |

Both sides come from the second row of their sheet -- the one with flame and
runes. The first row of each is a quieter alternative, still there.

## Rebuilding them

Two sheets, because the sides were generated separately:

```
python3 recorta_pecas.py brancas-fonte.png . "-,w" 308            # 2a fila
python3 recorta_pecas.py pretas-fonte.png  . "-,b" ref:.:1.02:R=1.131,B=0.96
```

The third argument says what each row of the sheet is, top to bottom -- `w`,
`b`, or `-` to skip. The fourth is either a height for that row's tallest
piece, or `ref:<dir>:<razao>` to size each piece against the white one of the
same type already in `<dir>`. White is cut first; black refers to it.

Two things in there are worth knowing before changing it. The row bands are
**measured** from the image's own row profile rather than guessed, because the
flames rise well above the bodies and a tight crop beheads the king -- which is
only visible once it is published. And the green is un-mixed from the edge
pixels rather than merely keyed out: skip that and every piece carries a green
halo the moment it lands on a board, which is exactly where it lands.

Both sheets are 2400x1792 PNG, so every piece is reduced into place rather
than enlarged. `pecas-fonte.jpeg` is the original 1024-wide sheet the set
started from, kept for the record.

## The squares

`casa_clara.svg` and `casa_escura.svg` -- dry plain and turned earth. The grain
is fractal turbulence generated in the file itself, under 1KB each, so the
browser rasterises them once and never fetches an image.

Deliberately low contrast. A loud board eats the pieces, and the board is there
for them to be read, not to be looked at.

## Using them elsewhere

Served raw from GitHub over HTTPS, which is what a page on a secure site will
load without complaint:

```
https://raw.githubusercontent.com/jppenixe-del/kestrel-chess/main/assets/board/wK.png
```
