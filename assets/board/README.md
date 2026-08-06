# The board

The engine's own piece set and squares, kept in the repository because they are
part of what the project looks like, and because a public HTTPS URL is what a
browser will actually load.

## The pieces

`wK wQ wR wB wN wP` and the same with `b`. PNG with an alpha channel, 320x320.

White marble with gold; deep red marble with gold, a bird of prey standing
in for both knights -- Kestrel is a falcon, not a horse.

**One sheet, two colours of the same design** -- `aguia-fonte.png`, white on
top and red below, twelve pieces in one 2814x1536 image. Every red piece
stands at **1.02 of the white piece of the same type**, matched by type
rather than by scaling the row as a unit: a flat, dark colour reads smaller
than a light one of the same size, and the two percent buys that back. One
ratio for the whole set this time -- the earlier two-sheet version needed
per-piece overrides because its sides did not agree on some shapes; this one
does.

Within each colour nothing else is normalised. Relative heights are exactly
as drawn, crests and finials included, which is why the rook stands well
below the bishop. One shared baseline for all twelve.

Heights as shipped, in pixels of the 320 canvas:

| | K | Q | R | B | N | P |
|---|---|---|---|---|---|---|
| white | 308 | 287 | 272 | 301 | 258 | 204 |
| red | 314 | 293 | 277 | 307 | 263 | 208 |

## Rebuilding them

```
python3 recorta_pecas.py aguia-fonte.png . 308
```

The third argument is the white king's height in the 320 canvas; every other
white piece follows the same ratio, and the red side follows per-type at
1.02 of white (see the script's own docstring for why per-type, not per-row).

Two things worth knowing before changing it. Pieces are isolated by
**connected component**, not by a fixed column grid -- the bishop's small
eagle finial and the knight's head both draw outside their own nominal
column, and a hard grid cut either beheads one or lets a fragment of the
neighbour bleed into it. And the background is removed by thresholding
**greenness** (G minus the stronger of R/B) rather than by un-mixing against
one measured background colour -- this source has a faint green-tinted halo
around its own outline stroke that is a different colour from the flat
background behind it, and un-mixing against a single sampled colour left
that halo visible. Check whichever way round against a dark test
background if a future sheet comes from a different generator: a green
fringe cannot hide there.

## The squares

`tabuleiro_batalha.jpg` -- one painting for all sixty-four squares, not a pair
of tiles repeated. A battlefield: a dark phalanx along the top, a light one
along the bottom, and open ground between them.

The checkerboard is baked into the image, so nothing has to be masked or
tiled at the other end -- the CSS sets one background and stops.

Three things were measured into it, and each fixed something visible:

- **The phalanxes are aligned to ranks 1 and 8.** In the source they sat
  1604px apart while those ranks are 1792 apart, so both were cut across the
  middle and the soldiers stood at the same height as the pieces, competing
  with them. Stretched 1.117 vertically and offset 158px, each phalanx now
  centres on its own rank -- behind the pieces of that colour, where it reads
  as backdrop.
- **Ranks 1 and 8 are veiled harder than the rest.** Sixteen of the
  thirty-two pieces stand there, and it is also the only place the painting
  has detail. Contrast there is 10.39:1 against 4.57:1 in the middle six
  ranks: most legible exactly where most pieces are, and left alone where the
  painting has nothing to compete with.
- **The palette is the pieces' own** -- bone white, deep red, black. Light
  squares warm bone, dark squares near-black rather than brown, because the
  red pieces are dark and were sinking into earth tones. Square edges carry a
  faint ember red.

The earlier generated pair (`casa_clara.svg`, `casa_escura.svg`) is still in
the repository and still works: two files under 1.2KB, no image to fetch. It
went from 2.78:1 to 8.32:1 between light and dark for the same reason -- below
that the grid was being inferred from the pieces rather than seen.

The principle both versions follow: a loud board eats the pieces, and the board
is there for them to be read, not to be looked at. Louder is not the fix;
*separable* is.
