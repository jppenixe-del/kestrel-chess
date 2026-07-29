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

**Not missing after all, and this corrects an earlier note in this file:**
`kestrel gpuextract <dataset> <out.bin> <max> <buckets> <threads>` already
decomposes each position into per-weight linear contributions AND already takes
a bucket count -- feature indices carry the bucket offset, so the buckets never
interact. The CPU tuners (`tune`, `tunestream`) are the ones that know nothing
about buckets, and they no longer need to.

`tools/gpu_tune.py` fits those features: sparse logistic regression in PyTorch.
Measured on the second machine's RTX 5060 Ti, 20k positions and 4 buckets
(5824 parameters): 5.4ms per epoch, 5000 epochs in 27 seconds. On a CPU this is
the part that made per-bucket tuning not worth starting.

**Two things to settle before trusting a run:**

* **Overfitting.** Loss fell from 0.223 to 0.0119 over 5000 epochs and was
  still falling -- with 20k positions against 5824 parameters that is the fit
  memorising, not learning. Use the full dataset, hold out a validation split,
  and stop on validation loss rather than epochs.
* **The extractor disagrees with the evaluation by 15cp**, where 3cp is the
  limit integer truncation allows, and this must be settled before any tuning
  run is trusted. A tuner fed features that do not add up will produce weights
  happily, and they will be wrong.

  What is established, so nobody repeats it:

  - It is not rounding. The residual does not move when KESTREL_PROBE_MULT is
    raised a hundredfold, which is the test the code itself prescribes.
  - It appeared in commit 8fdf335 (28/07). Binaries built before it report
    2.6cp; every one after reports 14.8-15.7cp.
  - **It is not the terms that commit added.** Each of the four (hanging via
    SEE, king tropism, blocked pawns, space) was disabled in turn and then all
    together; with all of them off the positional evaluation returns to
    exactly the pre-commit value (-78 on the worst position, matching the old
    binary to the unit) and the residual STILL reads 15.0.
  - Material and piece-square features are fine: `checkmatpst` passes within
    1.5cp.
  - `to_vec`/`from_vec` round-trip cleanly over all 705 scalars.

  So the disagreement is in the extraction path, not in the evaluation. The
  same commit also introduced runtime PSQT scaling (`psqt_factor`,
  `set_psqt_scale`, `psqt_override`) and `load_profile`; those are what is
  left unexamined, and the self-check's own reconstruction is the other half
  worth reading closely.

  Already fixed along the way (29/07): the six weights added on 28-29/07
  (hanging, king_tropism, space, blocked_pawns, king_aim, king_battery) were
  evaluated but never emitted by `to_vec`, so no tuner could see or move them
  -- they are appended now, and older weight files stay valid by being
  shorter. `hanging` applied a hardcoded 3/4 instead of its own weight; it now
  uses it, in thousandths, with defaults that reproduce the old behaviour to
  the unit. And an `env::var_os` sat in the evaluation's hottest loop; it is
  read once now.

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
