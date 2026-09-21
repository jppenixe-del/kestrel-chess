# nnue_vendor/ — the network reader, derived from Stockfish

This directory is **derived from the Stockfish NNUE implementation**
(https://github.com/official-stockfish/Stockfish), licensed under the **GNU
General Public License v3**. The licence header of every original file is kept
**verbatim**, unmodified, at the top of that file. Copyright in the original
machinery belongs to the Stockfish developers, and this project claims none of
it.

## Why it is here

The network this engine plays with was trained with Stockfish's own
`nnue-pytorch` trainer, on that architecture. A network is welded to the code
that reads it: the feature mapping, the quantisation, the layer geometry and the
serialisation are one object, and half of them cannot be re-derived from the
file. Reading such a network with the reader it was trained against is the
coherent choice, and the alternative — a re-implementation kept in step by hand
— drifts without saying so.

So the division in this project is deliberate and it is stated plainly:

- **the reading and evaluation of the network are derived work, credited here**
- **the search is ours** — move generation, ordering, pruning, extensions,
  reductions, the transposition table, time management and everything that
  decides which positions are looked at

## What is ours in the evaluation path

- `src/sf_features.rs` — the feature mapping, written here over plain
  bitboards, deliberately free of any engine type so that the same file can be
  given to the trainer. Verified index-by-index against the reference
  evaluation.
- the accumulator drive: recording what a move changes and materialising it
  once, at the point a score is asked for.
- the pawn-pair block, and whatever further blocks this project adds.

## What was changed in the copied files

Two things, and they are the whole list:

1. **The namespace**, so that these symbols cannot collide with the engine's
   own.
2. **One method made reachable.** `position.h` gains a public one-line method,
   `repoe_estado()`, which calls the existing private `set_state()`. It is
   needed because a position built by placing pieces has to be told to recompute
   the threat information this architecture's evaluation reads; without it the
   evaluation answers about threats that are not on the board — measured, `cp
   506` for the starting position. It exposes what already exists and changes
   nothing about what it does.

No algorithm, no constant and no licence header was touched. Any file here can
be diffed against its upstream original and the difference will be those two
things and nothing else.

## Keeping faith with the licence

GPL-3 asks for the source, the licence and the notice of modification. All three
are here: the files, their headers, and this note. If this engine is ever
submitted to a rating list, this directory is what it declares, and the claim it
makes is the narrow one — that the search is its own work and the reader is not.
