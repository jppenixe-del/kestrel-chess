<div align="center">

<img src="assets/banner.png" alt="KestrelStrike" width="760">

# KestrelStrike

**A UCI chess engine in C++** — NNUE evaluation · original alpha-beta search · Syzygy tablebases

[![License: GPLv3](https://img.shields.io/badge/license-GPLv3-blue.svg)](COPYING)
[![C++](https://img.shields.io/badge/language-C%2B%2B20-00599C.svg)](src/)
![UCI](https://img.shields.io/badge/protocol-UCI-brightgreen.svg)
![NNUE](https://img.shields.io/badge/evaluation-NNUE-orange.svg)

**by João Penixe**

</div>

---

## What this is

A chess engine written by someone who plays chess, not by a team.

It started the other way round from most projects. The first question was not
"how do I write a search" but "how do I train a network that actually plays" —
and answering it took months: building the data pipeline, understanding what a
training run is really telling you, learning which of the numbers on the screen
mean something and which are decoration. Only once there was a network worth
using did the engine that exploits it get written.

That order left its mark on the code. The search is ours, and every parameter in
it carries the measurement that chose it — in comments, next to the line, with
the number of games behind it. Where a value came from somewhere else and was
kept, the comment says so. Where a value was swept here and came out different
from everyone else's, the comment says that too, and why.

## Provenance, plainly

KestrelStrike is **built on the Stockfish substrate**. Board representation,
move generation, position handling, the NNUE inference code and the Syzygy
probing in `vendor/` are derived from Stockfish (GPLv3), with the namespace
changed to `Kestrel` and the original licence header kept verbatim at the top of
every file. This is stated here rather than left to be discovered.

**What is ours** is in `src/`:

| file | what it is |
|---|---|
| `busca.cpp` / `busca.h` | the search — iterative deepening, pruning, extensions, ordering, time management |
| `tt.cpp` / `tt.h` | the transposition table |
| `aval.cpp` / `aval.h` | the evaluator wrapper and the scaling that sits above the network |
| `uci_laco.cpp` | the UCI loop |

The Stockfish engine layer — its `search.cpp`, `movepick.cpp`, `thread.cpp`,
`engine.cpp`, `timeman.cpp`, `uci.cpp` — is **not compiled**. The search is
ours and so is the transposition table; bringing theirs in would mean carrying
two of each.

**The network is ours.** `f2e189.nnue` was trained from scratch for this engine.
No Stockfish network was used as a seed or as a teacher. See
[`NETWORKS.md`](NETWORKS.md).

## Our own scale

One design decision runs through the whole search and is worth stating, because
it is what makes the parameters mean something.

The engine keeps **its own evaluation scale** — classical centipawns, pawn = 100
— and converts at the boundary, rather than adopting the substrate's internal
units (pawn = 208). Every pruning margin, every exchange threshold, every
ordering weight was swept in that scale and is written in it. The conversion
happens in one place, `lim_see()`, and the comment there explains what it cost
to discover that it was needed.

Inheriting the substrate's numbers instead would have been easier and would have
meant inheriting someone else's tuning along with them.

## Building

    make                 build for this machine
    make ARCH=bmi2       x86-64 with AVX2 and PEXT — Intel since Haswell, AMD since Zen 3
    make ARCH=avx2       x86-64 with AVX2, no PEXT — runs well on almost any modern x86
    make ARCH=avx512     x86-64 with AVX-512  — only where the CPU has it
    make ARCH=sse41      older x86-64, no AVX
    make todos           all four, each named after its architecture

PEXT is what separates `bmi2` from `avx2`. AMD Zen 1 and Zen 2 implement it in
microcode, many times slower than on other processors, and a PEXT build runs
far below its speed there. If you do not know the processor, use `avx2`.

The SIMD macros are not optional and they are not visible in the compiler
flags. The NNUE reader in `vendor/nnue` does not rely on the compiler's
auto-vectorisation: its routines are written in intrinsics behind
`#if defined(USE_AVX2)` and friends. Without the macros the engine compiles,
runs, plays — and does the network in scalar. Measured: 50,393 nodes per second
against 419,883. Eight times, with no error message anywhere. Each architecture
target defines its own macros and none of them is optional.

## Running

The engine needs a network. It will refuse to search without one rather than
silently evaluate everything as zero:

    setoption name EvalFile value /path/to/f2e189.nnue

UCI options: `Hash`, `Threads`, `EvalFile`, `SyzygyPath`, plus the search
parameters listed by `uci`.

## Credits

- **[Stockfish](https://github.com/official-stockfish/Stockfish)** (GPLv3) —
  the substrate in `vendor/`: board, move generation, position, NNUE inference.
- **[pawn](https://github.com/ruicoelhopedro/pawn)** by Rui Coelho (GPLv3) —
  the network file format this project's networks are trained against, and the
  engine an earlier line of this work grew out of.
- **[Fathom](https://github.com/jdart1/Fathom)** (MIT) — Syzygy tablebase
  probing.
- **Jonathan Hallström**, for the pawn-pair features, invented for
  [Pawnocchio](https://github.com/JonathanHallstrom/pawnocchio) and used by the network
  this engine ships; and **sscg13** and **anematode** (Timothy Herchen), who
  wrote the trainer support and the inference code for them.

Engines read and learned from, without code taken: Stockfish, Triumviratus,
Coda, Cinder. Ideas are not code; the comments in the search explain how each
technique works rather than where it was first seen.

## Licence

GPLv3 — see [`COPYING`](COPYING). The Stockfish copyright headers in `vendor/`
are intact and this program inherits their terms.
