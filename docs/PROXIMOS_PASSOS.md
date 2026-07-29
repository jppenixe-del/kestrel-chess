# Next steps

Written 2026-07-29. Ordered by value over cost, not by appeal.

## 1. Per-phase weight buckets

Everything is built except one piece.

* `NUM_BUCKETS = 4` (`src/eval.rs`), split by PAWN count: 0-5, 6-9, 10-12, 13+.
  The boundaries were measured on 400k positions from our own games so each
  bucket holds a comparable share (23/28/29/19%). Equal-width slices would
  leave the end buckets fitting noise, and a search this wide finds and
  exploits exactly that kind of mirage.
* `weights_for(board)` already returns the right bucket's set.
* `KESTREL_BUCKET_WEIGHTS=<file>` loads `NUM_BUCKETS x to_vec()` values.
* `KESTREL_FLAT_BUCKETS=1` quantises the phase, so the taper stops competing
  with the buckets inside a bucket.
* `kestrel flatbuckets <file>` writes the starting weights as a staircase
  approximating the current line -- switching the mode on is deliberately not
  a strength change by itself.

It is inactive on purpose: every bucket starts as a copy of the single set, so
nothing moves until trained weights exist. A structural change and a strength
change should never arrive together, or there is no telling which did what.

**Missing:** the tuner (`kestrel tune`, `tunestream`) has no notion of buckets.
It needs to extract features per bucket and train the four sets separately.

**Why it is worth it.** Slope of our evaluation against a strong reference on
220 quiet positions (1.00 = same scale):

| pieces | 29-32 | 25-28 | 21-24 | 17-20 | 13-16 | 9-12 | 5-8 |
|---|---|---|---|---|---|---|---|
| slope | 0.78 | 0.97 | 1.05 | 1.52 | 1.50 | 1.57 | 1.37 |

We are quiet where there is most material and shrill where there is least. Two
phases blended linearly cannot express that, and no single global factor fixes
it because the error changes sign in the middle.

2026-07-29 corrected the LOUDNESS only, with eight scale factors by piece count
(`material_bucket_scale`): suite 74/214 -> 78/214 at a fixed 400k nodes. Weight
buckets correct something else -- what the terms BELIEVE in each phase, not just
how loudly they say it. The two are complementary.

**When measuring:** use fixed nodes and one thread. The same binary scored 73
then 69 on the suite at 300ms, and 73-75 at four threads. And note that the
`eval` command now reports what SEARCH sees; before it reported the sum of the
component blocks and disagreed with the engine it exists to explain.

## 2. A network, and where the GPU actually helps

**Training on the GPU: yes.** That is what it is for.

A network in an alpha-beta search is a solved problem -- the strongest engines
in the world are alpha-beta with a neural evaluation. The open question is not
whether a network fits, it is WHERE it runs.

**On the CPU: yes**, and the reason is worth stating exactly, because it
decides the whole design. The first layer -- by far the largest -- is never
recomputed per node. An accumulator is kept and, when a move shifts a piece,
the weight columns for the origin square are subtracted and those for the
destination added: a few hundred integer adds instead of a matrix multiply.
The remaining layers are tiny and run in SIMD over 8/16-bit integers. What
makes a network affordable is not that it is small; it is that it is barely
recomputed. Two accumulators, one per side, because each side reads the board
from its own perspective.

**On the GPU during a game: no**, and this is arithmetic rather than
preference. The search performs ~500,000 evaluations per second, one at a time,
each deciding whether to keep searching -- sequentially dependent, not a batch.
That is ~2 microseconds per evaluation against 50-200 microseconds of latency
for a single GPU call, and nothing incremental survives a trip across the bus.
GPUs pay off on batches of hundreds; alpha-beta does not produce them. Engines
that do use a GPU pair it with MCTS, which batches naturally by expanding many
leaves at once.

So: train on the GPU, infer on the CPU with incremental updates, and keep the
hand-crafted evaluation as the safety net (known endgames, tablebases) and as
what gives the engine its character.

**Order matters.** Buckets first -- same problem, model we already understand,
data we already have. Then data: we have 250k positions and a network wants
tens of millions; the self-play generator exists and needs to run for a long
time. Then the network.

There is nothing special about training "for bullet": the network is the same,
only the search time differs. What helps in bullet is a SMALL, cheap network,
which reinforces the point above rather than working against it.
