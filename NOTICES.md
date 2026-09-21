# Notices

KestrelStrike is distributed under the GNU General Public License, version 3 or
later. The full text is in [`COPYING`](COPYING).

This file records provenance: code that came from elsewhere, and under what
terms. **Ideas are not code and are not listed here** — a technique that many
programs implement is not anyone's property, and the source comments explain how
each one works rather than where it was first seen.

## The substrate — `vendor/`

`vendor/` is **derived from Stockfish**
(<https://github.com/official-stockfish/Stockfish>), GPLv3. It is real code, not
an idea. It provides:

- board representation, bitboards, attack generation (`bitboard.cpp`,
  `attacks.cpp`, `movegen.cpp`, `position.cpp`)
- the NNUE inference machinery (`nnue/`), including the `HalfKAv2_hm`,
  `FullThreats` and `PP_3Wide` input feature sets
- Syzygy tablebase probing (`syzygy/`)
- supporting machinery (`memory.cpp`, `misc.cpp`, `score.cpp`, `ucioption.cpp`)

**What was changed:** the namespace, from `Stockfish` to `Kestrel`. The original
licence header of every file is kept verbatim at the top of that file.

**What is not compiled:** the Stockfish engine layer — `search.cpp`,
`movepick.cpp`, `thread.cpp`, `engine.cpp`, `timeman.cpp`, `uci.cpp`. This
program has its own search and its own transposition table, and theirs expect an
interface (`probe`, `new_search`, `hashfull`) that ours does not have and should
not have.

## The pawn-pair features

`vendor/nnue/features/pp_3wide.cpp` implements pawn-pair features, and those are
not anonymous. They were **invented by Jonathan Hallström for
[Pawnocchio](https://github.com/JonathanHallstrom/pawnocchio)**, out of the observation
that in a network trained on every pair of pawns, the pairs that carried the
information were the ones at most one file apart — which is what makes the
feature set small enough to be worth having. **sscg13** wrote the trainer
support and the first inference code, with **anematode** (Timothy Herchen) as
co-author.

Stockfish credits them and so does this program. The file itself carries only
the project's licence header, which names nobody, so the attribution would
otherwise be lost on the way here — and the network this engine ships was
trained with those features.

## What is ours — `src/`

`src/busca.cpp`, `src/busca.h`, `src/tt.cpp`, `src/tt.h`, `src/aval.cpp`,
`src/aval.h`, `src/uci_laco.cpp` and `src/uci_compat.cpp` are original work and
carry this program's GPL-3-or-later terms.

`src/uci_compat.cpp` deserves a note: it provides three printing helpers that
Stockfish's `uci.cpp` also defines. They are written here rather than taken,
because taking them would mean compiling their whole engine layer to get three
small functions.

## The network

The network is **ours** — see [`NETWORKS.md`](NETWORKS.md). Its file layout,
feature indexing and quantisation follow the format used by
[`pawn`](https://github.com/ruicoelhopedro/pawn) by Rui Coelho, GPLv3, which is
the format the network was trained against.

An earlier line of this work grew directly out of `pawn`, and for a period the
bot that ran it identified itself as such and credited its author. That lineage
is recorded here because it is real, even though none of that code remains in
this program.

## Syzygy

Tablebase probing comes from [Fathom](https://github.com/jdart1/Fathom), MIT,
through the Stockfish integration in `vendor/syzygy`.

## Licence compatibility

This program is GPL-3. That fixes what may be brought in and what may not:

| project | licence | may enter |
|---|---|---|
| Stockfish | GPL-3 | yes |
| pawn | GPL-3 | yes |
| Coda | GPL-3 | yes |
| Cinder | GPL-3 | yes |
| Fathom | MIT | yes |
| Reckless | **AGPL-3** | **no** |
| Icarus | **AGPL-3** | **no** |

The direction only works one way: GPL-3 code may enter an AGPL-3 work, not the
reverse. Code from an AGPL-3 project inside this one would force the whole
engine to become AGPL — or be a violation. From those projects one reads and
learns; one does not transcribe. Verified by literal search for their constants
before release.
