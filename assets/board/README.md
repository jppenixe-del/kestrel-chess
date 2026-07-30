# The board

The engine's own piece set and squares, kept in the repository because they are
part of what the project looks like, and because a public HTTPS URL is what a
browser will actually load.

## The pieces

`wK wQ wR wB wN wP` and the same with `b` — light and dark. PNG with an alpha
channel, 320x320, drawn as carved wood: pale oak against dark red.

The knight is a bird of prey rather than a horse. Kestrel is a falcon.

**Relative heights are preserved and that matters more than it sounds:**

| | K | Q | B | N | R | P |
|---|---|---|---|---|---|---|
| height | 1.00 | 0.89 | 0.81 | 0.79 | 0.70 | 0.64 |

Cutting each piece to fill its own square is the obvious thing to do and it is
wrong: it makes the pawn as tall as the king, and height is how a player tells
them apart at a glance. Every piece here shares one scale and one baseline.

Widened by 14% with the height untouched, so they sit fuller in a square
without any of them changing rank.

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
