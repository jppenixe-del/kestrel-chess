# The network

KestrelStrike plays with **its own** NNUE network, `ks-cf796d1f923d.nnue`,
embedded in the executable. The name carries the first twelve hex digits of
the file's SHA-256 — the convention Stockfish uses — so anyone can check that
the network they are playing against is the one this engine shipped:

    sha256  cf796d1f923df5f8b65a150d…   95,797,208 bytes

## What is ours and what is not

An engine that runs borrowed inference code on borrowed weights is a
repackaging. The weights here are not borrowed — but much of what surrounds
them is, and this table says which is which.

| | source |
|---|---|
| **the weights** | **trained by this project, from random initialisation.** No Stockfish network, and no other engine's, was used as a starting point |
| the trainer | [nnue-pytorch](https://github.com/official-stockfish/nnue-pytorch), the Stockfish project's trainer (GPLv3), with local changes to how the data reaches it |
| the recipe | Stockfish's: two stages, with the data filters and schedule of its `threats.yaml` |
| stage 1 data | public Stockfish training data — positions scored by Stockfish searches of 5,000 nodes, released by Joost VandeVondele |
| stage 2 data | public Leela Chess Zero self-play data (test60 to test80, leela96, T60T70), rescored with a Leela BT4 network and released by linrock, Joost VandeVondele and xushawn |
| the architecture | Stockfish's current one, with the input feature sets `HalfKAv2_hm`, `FullThreats` and `PP_3Wide` |
| the inference code | Stockfish (GPLv3), in `vendor/nnue`, namespace changed |
| the file format | Stockfish's, as nnue-pytorch writes it — the header still carries the trainer's default description |

Two things follow from the table, and they are said here because they are what
anyone judging originality will want to know:

- **The first stage learned Stockfish's evaluations.** The network did not
  start from Stockfish's weights, but in stage 1 its targets were Stockfish's
  search scores. Stage 2 then retrained it on Leela's data. It is the same
  two-stage recipe the Stockfish project uses for its own networks, run here
  from the beginning.
- **What is ours is the run, not the method.** The weights are the outcome of
  this project's own training — its hardware, its data feed, its checkpoint
  (`f2e199`: stage 2, checkpoint 199) — and not a copy or a fine-tune of
  anybody's file.

## How it was done

The order of work in this project was unusual, and it is the reason the network
came first.

Months went into the question of how to train a network that plays at all —
before any of the search work started. That meant building the data feed,
learning to read a training run honestly, and finding out which of the numbers
on the screen carry information. Several conclusions from that period are worth
passing on, including the failures: a network can load without a single error,
pass every size and checksum test the engine makes, and still evaluate noise,
because one weight matrix was stored in the wrong order. The way to catch that
is to correlate the exported network against the labels of the very positions
it trained on — hand-picked positions do not work, because the recipe filters
openings and rare endgames and the network never saw them.

Only once there was a network worth playing with did the engine that exploits it
get written.

## Verifying

The engine says at startup which network it carries:

    info string embedded network: ks-cf796d1f923d.nnue

`setoption name EvalFile value <file>` replaces it with another. A file that is
missing or does not fit the architecture is reported and the embedded network
stays:

    info string ERROR: network not found: <file>

An executable built without an embedded network refuses to search rather than
evaluate everything as zero, and says why.
