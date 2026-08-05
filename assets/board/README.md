# The board

The engine's own piece set and squares, kept in the repository because they are
part of what the project looks like, and because a public HTTPS URL is what a
browser will actually load.

## The pieces

`wK wQ wR wB wN wP` and the same with `b`. PNG with an alpha channel, 320x320.

Copper and warm flame for white; blackened steel and cold violet fire, with
runes, for black. The knight is a bird of prey rather than a horse: Kestrel is
a falcon.

**The two sides are not one set in two colours** -- they are separate designs,
from separate sheets, and white is the imposing one. White fills the square at
296 pixels; black stands at 266, nine tenths of that. That ratio is a choice
and it has to be stated, because with the sides coming from different sheets
there is nothing inside either one that could imply it.

Within each side nothing is normalised. Relative heights between pieces are
exactly as drawn, flames and crests included, which is why the rook stands
close to the queen. One baseline for all twelve. Nothing is fitted to its own
square individually: relative height is how a player tells a rook from a queen
without looking twice, and per-piece fitting would make the pawn as tall as
the king.

Heights as shipped, in pixels of the 320 canvas:

| | K | Q | R | B | N | P |
|---|---|---|---|---|---|---|
| white | 296 | 252 | 289 | 252 | 281 | 219 |
| black | 266 | 250 | 227 | 219 | 235 | 132 |

The black pawn is the one place the two sheets disagree sharply: it is drawn at
half its king where white's is at three quarters. Left as drawn.

## Rebuilding them

Two sheets, because the sides were generated separately:

```
python3 recorta_pecas.py pecas-fonte.jpeg  . "w,-"  296   # brancas: 1a fila
python3 recorta_pecas.py pretas-fonte.png  . "-,b"  266   # pretas:  2a fila
```

The third argument says what each row of the sheet is, top to bottom -- `w`,
`b`, or `-` to skip. `pretas-fonte.png` offers two alternative designs and only
the second is used. The fourth is how tall that row's tallest piece ends up.

Two things in there are worth knowing before changing it. The row bands are
**measured** from the image's own row profile rather than guessed, because the
flames rise well above the bodies and a tight crop beheads the king -- which is
only visible once it is published. And the green is un-mixed from the edge
pixels rather than merely keyed out: skip that and every piece carries a green
halo the moment it lands on a board, which is exactly where it lands.

The black sheet is 2400x1792 PNG, so its pieces are reduced into place. The
white one is a 1024-wide JPEG and is being enlarged -- about 150 pixels across
before scaling to 320 -- and is visibly softer. Regenerating white at the same
size as black is the outstanding job here.

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
