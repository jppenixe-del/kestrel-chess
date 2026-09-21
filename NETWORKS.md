# The network

KestrelStrike ships with **its own** NNUE network. The weights were trained by
this project. **No Stockfish network was used as a seed, as a teacher, or as a
starting point.**

This matters enough to be its own file, because an engine that runs borrowed
inference code on borrowed weights is a repackaging, and this is not that.

## What is ours and what is not

| | source |
|---|---|
| **the weights** | trained here, from scratch |
| **the training pipeline** | ours — data generation, filtering, schedule |
| the inference code | Stockfish (GPLv3), in `vendor/nnue`, namespace changed |
| the input feature sets | Stockfish's `HalfKAv2_hm`, `FullThreats`, `PP_3Wide` |
| the file format | follows the format used by [`pawn`](https://github.com/ruicoelhopedro/pawn) by Rui Coelho, which is the format the networks were trained against |

The architecture is the current Stockfish one. That is a deliberate choice: it
is a good architecture, it is well understood, and reimplementing it would have
bought nothing. What distinguishes one engine from another at that point is not
the shape of the network — it is the weights, the search, and the speed.

## How it was done

The order of work in this project was unusual, and it is the reason the network
came first.

Months went into the question of how to train a network that plays at all —
before any of the search work started. That meant building the data pipeline,
learning to read a training run honestly, and finding out which of the numbers
on the screen carry information. Several conclusions from that period are
recorded in the source comments of the tooling, including the failures: a
network can load without a single error, pass every size and checksum test the
engine makes, and still evaluate noise, because one weight matrix was stored in
the wrong order. The way to catch that is to correlate the exported network
against the labels of the very positions it trained on — hand-picked positions
do not work, because the recipe filters openings and rare endgames and the
network never saw them.

Only once there was a network worth playing with did the engine that exploits it
get written.

## Verifying

The engine refuses to search without a network rather than silently evaluating
everything as zero:

    info string ERROR: no network loaded — use `setoption name EvalFile value <file>`

It also checks the file before handing it to the substrate, which otherwise
exits the process when it cannot find one. An engine that dies at startup cannot
tell the arbiter what it is missing; asking first lets it refuse with a sentence.
