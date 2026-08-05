# The board

The engine's own piece set and squares, kept in the repository because they are
part of what the project looks like, and because a public HTTPS URL is what a
browser will actually load.

## The pieces

`wK wQ wR wB wN wP` and the same with `b`. PNG with an alpha channel, 320x320.

Copper and fire against blackened steel, to match the falcon in `assets/`. The
knight is a bird of prey rather than a horse: Kestrel is a falcon.

**The two sides are not the same shapes in another colour**, and white is the
imposing one. The white pieces fill the square; black stands at 90% of that.
That is a choice, and it is the reverse of what the source sheet gives: there
the black pieces are drawn 35% LARGER, so a single scale for all twelve
inherits it and the wrong side dominates the board.

Within each side nothing is normalised. Relative heights between pieces are
exactly as drawn -- the art was made to fill the square, and the flames count
toward height, which is why the rook stands close to the queen.

One baseline for all twelve. Nothing is fitted to its own square individually:
relative height is how a player tells a rook from a queen without looking
twice, and per-piece fitting would make the pawn as tall as the king.

Heights as shipped, in pixels of the 320 canvas:

| | K | Q | R | B | N | P |
|---|---|---|---|---|---|---|
| white | 296 | 252 | 289 | 252 | 281 | 219 |
| black | 266 | 237 | 213 | 199 | 221 | 146 |

The ratio between the sides is one constant, `PROPORCAO_PRETAS` in
`recorta_pecas.py`.

## Rebuilding them

`pecas-fonte.jpeg` is the generated sheet -- twelve pieces on flat green, two
rows of six. `recorta_pecas.py` turns it into the twelve files:

```
python3 recorta_pecas.py pecas-fonte.jpeg .
```

Two things in there are worth knowing before changing it. The row bands are
**measured** from the image's own row profile rather than guessed, because the
flames rise well above the bodies and a tight crop beheads the king -- which is
only visible once it is published. And the green is un-mixed from the edge
pixels rather than merely keyed out: skip that and every piece carries a green
halo the moment it lands on a board, which is exactly where it lands.

The source is a 1024-wide JPEG, so each piece is about 150 pixels across before
being scaled up. They are softer than a natively-drawn set at this size would
be.

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
