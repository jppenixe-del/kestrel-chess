# Next steps

Written 2026-07-29. Ordered by value over cost, not by appeal.

## 0. The blunder suite was wrong, and every number below it moved

Read this before trusting any measurement in this file or in the commit log.

The suite is the arbiter of nearly every decision this project has made. It was
built at depth 16 and recorded exactly ONE correct move per position. Rebuilt at
depth 22, recording every move within 20cp of the best:

* **83 of 214 best moves changed** (39%).
* **103 of 214 positions have at least one equally good alternative**, which the
  old suite counted as a failure.

What that did to the numbers, same binary, 400k nodes, one thread:

| | old suite | recalibrated |
|---|---|---|
| V3, the engine as it plays | 78 fixed / 131 avoided | **110 fixed / 144 avoided** |
| a fit from zeros | 62 fixed | 104 fixed |

The engine was being charged with a blunder in 32 positions where it played a
correct move -- just not the one move the suite happened to hold.

**And the old suite did not merely understate; it exaggerated differences.** The
gap between V3 and the fit was 16 positions and is 6. When two in five right
answers are arbitrary, a build is penalised for a coin toss, and that noise
appears as a difference between versions. Any past verdict decided by a margin
of a few positions -- the V4 profiles, the hand-chosen buckets, the queen sweep
in section 1b -- was decided partly by that coin.

`blunders_big_v2.epd` is the recalibrated suite; `recalibrar_suite.py` rebuilds
it. `blunder_test.py` reads `also` and counts any of them as fixed.

## 0b. Fitting the weights: what it measured (2026-07-29)

The fitter works, the plumbing that carried its output did not, and the result
is that a fit still does not beat what it would replace.

**`KESTREL_BUCKET_WEIGHTS` had never been read by any binary that ran.**
`weights_for` tested the family factors first and returned there; V3 is compiled
into the defaults, so that branch is always taken. Every per-bucket fit ever
produced could only be measured by an engine ignoring it. Fixed in ec57874 --
and it is the same shape as the advisor forcing multipv 3: a setting that never
reached the code it configured. When a change measures as doing nothing, check
that it arrived before concluding it does nothing.

**Measured on the recalibrated suite, 400k nodes, one thread:**

| | validation loss | avoided | fixed |
|---|---|---|---|
| V3 (what plays) | 0.0972 | **144/214** | **110** |
| fit from V3, material frozen | 0.0785 | 140/214 | 106 |
| fit from zeros, everything free | **0.0780** | 135/214 | 104 |

**The lowest held-out loss belongs to the worst engine.** The held-out split is
real -- 265k positions never trained on -- and it still ranks the three exactly
backwards. It is a diagnostic of the fit, not an arbiter of strength. Nothing
in this file should be decided by it.

Fitted from zeros, the run put a pawn at 330 in the opening and 69 in the
endgame: chess upside down, the signature of Material and PSQT being collinear
and the optimiser splitting their shared explanation wherever it landed. Since
per-bucket material cannot be installed anyway -- the incremental accumulator
holds one set of tables and knows nothing about the pawn count -- `kestrel
tunestart` now writes the engine's own weights in the fitter's layout, and
`--free` freezes the material block. Verified: installing the starting file
reproduces the unconfigured binary to the centipawn on sixty positions, so
epoch zero IS the current engine.

That recovers 5 of the 9 positions the free fit lost, in the predicted
direction, and still does not reach V3.

**What has NOT been tried, and is the obvious next thing:** all of the above was
fitted on `dataset_own.epd` -- 2.65M positions from our own games, labelled by
the result of a game between two ~2200 players. That is the weakest label
available. There are 10M and 5M position datasets sitting unused in the
repository directory. Refit there before concluding anything about whether a fit
can beat V3.

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

## 1b. What hand-tuned buckets taught us (2026-07-29)

Eight buckets are in the engine, partitioned by pawn count, driven from a
profile. The mechanism works. Every set of hand-chosen values for it lost.

**Terminology first, because it misled the measurements for an afternoon.**
There is no "no profile" baseline. The V3 profile is compiled into the
defaults -- king scale 1100, threats 1150, psqt_scale.king 1350, and the
piece-square tables themselves, verified value by value against
perfil_v3_completo.txt. Running with no profile file IS running V3. A V4 entry
setting a family to 1000 therefore does not leave it neutral: it UNDOES V3.
The first V4 draft flattened `scale.king` from 1100 to 800-1280 and silently
erased a tuned setting while appearing to only rescale.

**Measured, blunder suite at a fixed 400k nodes, one thread:**

| profile | suite |
|---|---|
| V3 (the defaults) | **78/214** |
| V4, linear slope flattening | 73 |
| V4, surgical (locked positions + queen) | 71 |
| V4, locked positions only | 75 |
| V4, queen only at 1250 | 60 |

And the queen swept in both directions, which is the result that settles it:

| queen scale, buckets 5-7 | 800 | 900 | 1000 | 1250 |
|---|---|---|---|---|
| suite | 69 | 67 | **78** | 60 |

Symmetric. Both hypotheses -- "amplify to restrain the queen" and "reduce to
flatten the incentive" -- predicted one side would help. Neither did.

**Two things this proves, and one it does not.**

Proven: 1000 is not a neutral origin for these tables, it is where V3 left
them after its own calibration. Moving away from a calibrated point in either
direction loses, whichever direction that is.

Proven: nobody can predict the interaction between evaluation scale and search
margins by hand. The margins (`rfp`, `nmp`, `razoring`, `futility`) are fixed
centipawn quantities tuned against the scale the evaluation actually produces.
Change that scale for a subset of features and the margins mean something
different, in a way no amount of reasoning about chess anticipates.

NOT proven: that the buckets are wrong. What is wrong is choosing their values
by hand. A fit over 12.5M positions learns the interaction instead of guessing
it, which is the whole argument for the tuner.

**A measurement trap worth remembering.** One hypothesis along the way was
that per-bucket PSQT creates a discontinuity -- trade a pawn, cross a boundary,
the evaluation jumps. Tested directly on identical positions either side of a
boundary: the jump is 115cp without any profile and 113cp with one. It is the
pawn, not the bucket. A plausible mechanism, measured, and false.

**And a real bug the exercise found**: `evaluate_fast` -- used by quiescence
and the pruning margins -- read the incremental accumulator without the bucket
correction, so the search saw the queen at one amplitude while the evaluation
saw it at another. 13 suite positions, and it looked exactly like the profile
being wrong.

## 1c. A Rust Lichess client, with our options and our machines

The Python bridge has grown everything the engine needs and nothing it needs to
be reliable. Rewrite it in Rust, using BotLi (github.com/Torom/BotLi, cloned
and configured at /root/kestrel_joao/BotLi, NOT running) as a reference for
features -- not as a base to copy, since the engine is original work.

**Machine orchestration is a first-class requirement, not a convenience.**
There are two machines -- the server (6 cores, shared with tests) and the
second box (ssh napoleon, 6c/12t, RTX 5060 Ti) -- and today the choice of where
the bot plays is made by hand, with scp and manual restarts. The client should
own it:

* machines declared in config, with core counts and which one plays;
* choice by time control -- the second box is 25% faster per thread (620k vs
  495k nps), so bullet belongs there while the other runs suites;
* threads per machine and per speed, not one global constant;
* verify the binary is identical on both (md5) before playing, and refuse to
  play one older than the repository -- we have played old versions without
  noticing;
* switch machines WITHOUT killing games: pause, wait for the board to empty,
  start on the other side;
* never two clients on the same token at once -- they fight over the same
  games;
* assume the machine it is NOT playing on may be running heavy tests, and do
  not treat its CPU as free.

**What has to come with us** (all in lichess_bridge.py, all of it measured):
time management left to the engine via wtime/btime with no imposed movetime;
the draw and resignation logic; the instant opening book; the pause-by-file
that refuses new challenges without killing live games (restarts cost hours of
429); per-move telemetry, which produced every diagnosis we have; and the short
retried move POST, since that request runs on OUR clock.

**What BotLi does better and is worth taking**: matchmaking -- but following
each challenge to its outcome, not just its creation, since most bots decline
with declineReasonKey "nobot"; online tablebases; and stream reconnection that
does not drop ~46 times a day.

**Traps already paid for**: ponder OFF, because a second engine process starves
the first and once cost a live game; heavy tests on the playing machine cost a
loss on time (44s on one move with 44s on the clock -- not time management,
missing CPU); and clock.initial is in seconds while wtime/btime are in
milliseconds.

## 1d. Playing with no search at all, and what it measured (2026-07-29)

`HeatmapOnly` plays straight from the evaluation: every legal move played, the
resulting position evaluated once, the best returned. `HeatmapPlies 2` counts
his best reply too. It exists to see the evaluation with nothing in front of
it -- a term with the wrong sign or an amplitude that swamps the rest shows up
directly in the move.

What it measured, blunder suite, 214 positions:

| mode | suite |
|---|---|
| 1 ply, evaluation alone, zero nodes | 37 |
| 2 plies, with his reply | **42** |
| `go depth 1` (quiescence, ordering, TT) | 41 |
| `go depth 2` | 48 |
| full search, 400k nodes | 78 |

**The evaluation alone finds 47% of what the full search finds.** Seeing the
immediate reply is worth five positions; the rest of the gap is depth.

**Two plies of static evaluation equals a real search at depth 1.** That is not
a compliment to the evaluation, it is a question about the quiescence search:
with ordering, a transposition table and quiescence, depth 1 should beat a
static reading of the reply comfortably. It does not. Worth investigating on
its own.

**Threat detection by null move works, and buying depth with it does not.**
Passing the turn and reading his heatmap names the threat exactly -- on a
knight attacked twice it reports the capture. But spending an extra ply on the
moves that answer the threat takes the suite from 42 to 23, and giving every
move that third ply takes it to 5.

The cause is parity, not the mixture of depths. An odd depth ends on OUR move,
so the evaluation sees the piece we just took and never the recapture: the
horizon effect, undiluted. At two plies he has the last word. **Without a
quiescence search, only even depths mean anything** -- and that is precisely
why the engine's own depth 1 holds up at 41, since it has one.

The null move stays, reporting the threat as an `info string`. It must not buy
depth.

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
