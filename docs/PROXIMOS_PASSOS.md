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
* **The extractor now agrees with the evaluation.** It did not, by 15cp against
  a 3cp bar, and it turned out to be two independent faults rather than the
  non-linearity it was reported as:

  1. `material_pst_current_vec` returned the raw piece-square tables while the
     evaluation applies `psqt_factor` to them. Five centipawns a position, in
     the material half, reported as a positional non-linearity.

     The king's factor being 1350 is deliberate and came from the project's
     own PSQT simulator, not from any tuning run: our king table has half the
     amplitude of a strong reference's, and 1350 is the recorded compromise
     between a fixed-time reading and one blind to castling. See
     perfil_v3.txt. It is a decision, not a number to be tidied away.

     **This matters for tuning.** If a run fits the piece-square tables
     themselves, the fitted values already contain whatever the factor was
     doing, and leaving `psqt_scale.king` at 1350 would apply it twice. Either
     tune with the factors at 1000, or divide them out of the result -- and
     that is a decision for whoever owns the profile, not a detail to settle
     in passing.
  2. The probe multiplier defaulted to `10 * MAX_PHASE`, and MAX_PHASE is 24,
     so it probed at 240. At 240 the residual is 14.5cp; at 1024 it drops to
     3.2 and stays there through 240000. Every value tried by hand happened to
     be above the knee, so the residual looked scale-independent -- which is
     precisely the signature of "not rounding" -- and was read that way for a
     day.

  Residual now: 2.4cp on the quiet dataset, 3.2 on our own games, 2.4 on the
  combined one. The bar is 3.5, set just above the measured floor rather than
  just below it.

  **The test that matters is invariance, not the number.** A residual that
  shrinks as the probe grows is rounding; one that does not is the model
  failing to be the function. Sweep KESTREL_PROBE_MULT before concluding
  anything.

* **Extractor and engine now partition alike.** `gpuextract` bucketed by phase
  while `eval::bucket_of` buckets by pawn count. A run would have trained each
  set on one population and the engine applied it to another. They call the
  same function now.

* **Overfitting.** Loss fell from 0.223 to 0.0119 over 5000 epochs on 20k
  positions against 5824 parameters and was still falling -- that is
  memorisation. Use the full data, hold out a validation split, stop on
  validation loss.

* **How much data there is: 12.5 million positions**, not the 250k an earlier
  note in this file claimed. `dataset_own.epd` alone holds 2.65M from our own
  games. That changes what is affordable: at 4 buckets the smallest bucket
  gets ~3400 positions per weight, at 8 buckets ~1357. Eight buckets fit the
  data comfortably; the earlier worry that they would not was based on the
  wrong figure.

  Boundaries the distribution asks for, measured on our own games: 4 buckets
  at [5, 9, 12] pawns -- which is exactly what `bucket_of` already uses, an
  independent confirmation -- and 8 buckets at [3, 6, 8, 9, 11, 12, 13].

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
