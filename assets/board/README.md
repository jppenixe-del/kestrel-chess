# The board

**Provenance.** Every image in this directory is the project's own: the piece
sheet, the boards and the squares were generated for this project, not sourced
from an existing piece set or photograph. This is stated because it is the kind
of thing a reader would otherwise have to take on trust -- chess piece sets in
particular are usually somebody else's (cburnett, merida and the rest carry
their own licences), and these are not.


The engine's own piece set and squares, kept in the repository because they are
part of what the project looks like, and because a public HTTPS URL is what a
browser will actually load.

## The pieces

`wK wQ wR wB wN wP` and the same with `b`. PNG with an alpha channel, 320x320.

White marble and black marble, both with gold. A Spartan helm crowns the
king, a gargoyle perches on the bishop, and a falcon stands in for the
knight -- Kestrel is a falcon, not a horse. Greek meander around every base.

**One sheet, two colours of the same design** -- `aguia-fonte.png`, white on
top and black below, twelve pieces in one image. Every dark piece stands at
**1.02 of the white piece of the same type**, matched by type rather than by
scaling the row as a unit: a flat, dark colour reads smaller than a light one
of the same size, and the two percent buys that back.

Within each colour nothing else is normalised. Relative heights are exactly
as drawn, crests and finials included. One shared baseline for all twelve.

The previous set -- white marble against DEEP RED marble -- is kept in
`../pecas_marmore_vermelho`. Red was the more legible of the two on a dark
board; black is the more coherent with "white against black" and with the
board's own palette. That is a taste call, not a measurement, and the red set
is there to go back to.

## Rebuilding them

```
python3 recorta_pecas.py aguia-fonte.png . 308
```

The third argument is the white king's height in the 320 canvas; every other
white piece follows the same ratio, and the dark side follows per-type at
1.02 of white (see the script's own docstring for why per-type, not per-row).

Heights as cut, in pixels of the 320 canvas:

| | K | Q | R | B | N | P |
|---|---|---|---|---|---|---|
| white | 308 | 287 | 273 | 299 | 259 | 204 |
| black | 314 | 293 | 278 | 305 | 264 | 208 |

Two things worth knowing before changing it. Pieces are isolated by
**connected component**, not by a fixed column grid -- the bishop's gargoyle
wings and the knight's beak both draw outside their own nominal
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

`tabuleiro_batalha.jpg` -- cracked stone paving, one image for all sixty-four
squares, checkerboard baked in. The CSS sets one background and stops.

**It is a board and not a picture, and that is why it won.** Three painted
alternatives came before it and all three had the same defect: they had a
SUBJECT. Soldiers, castles, phalanxes -- things that ask to be looked at,
under pieces that need to be looked at. Stone has texture without subject.

Two measurements decided the crop:

- **Flat light.** Of four candidate regions, this one differs by 3 levels of
  luminance between its left quarter and its right (the worst differed by 42).
  A gradient across the board reads as a shadow lying over the squares and
  competes with the checkerboard itself.
- **5.5:1** between light and dark square. The stone is one colour throughout,
  so the entire checker comes from the veil. The dark wash is **reddish brown**
  (mean RGB 94,59,40 -- R above G above B), not neutral grey: burnt stone is
  actually that colour, and it answers the gold on the pieces. A neutral wash
  read as a shadow lying on sand rather than as a darker stone.

The photograph's own contrast is dialled DOWN before the veil goes on (0.82).
The texture has to be present without asserting itself -- it is floor, not
subject -- and at full strength the cracks were pulling the eye the way the
painted boards did.

It also happens to be the right floor for these pieces: a Spartan helm and a
Greek meander belong on cracked ancient paving.

The alternatives are kept, not deleted, and each is better at something:

- `tabuleiro_300.jpg` -- two phalanxes aligned to ranks 1 and 8, empty middle.
  Tells the story best; 5.7:1, and the painted soldiers crowd the pieces.
- `tabuleiro_molon.jpg` -- the interior of the MOLON LABE painting, enlarged
  past its bronze frame. 7.6:1, the highest, but enlarged and veiled the
  battle stops reading as a battle at all.
- `casa_clara.svg` / `casa_escura.svg` -- generated in the file, two tiles
  under 1.2KB, nothing to fetch. 8.3:1.

The principle every one of them follows: a loud board eats the pieces, and the
board is there for them to be read, not to be looked at. Louder is not the
fix; *separable* is.
