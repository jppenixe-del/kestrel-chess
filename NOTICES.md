# Notices

half2k ultra is distributed under the GNU General Public License, version 3 or
later. The full text is in `COPYING`.

This file records provenance: code that came from elsewhere, and under what
terms. Ideas are not code and are not listed here — a technique that many
programs implement is not anyone's property, and the source comments explain how
each one works rather than where it was first seen.

## Board representation, move generation, and the surrounding machinery

`src/types.rs`, `src/bitboard.rs`, `src/moves.rs`, `src/magic.rs`,
`src/attacks.rs`, `src/movegen.rs`, `src/board.rs`, `src/zobrist.rs`,
`src/perft.rs` and `src/tt.rs` come from Kestrel, our own engine, and carry its
GPL-3-or-later terms. `src/board.rs` has since been decoupled from Kestrel's
evaluation and rebuilt around this program's network.

## The network and its file format

The network is ours. Its file layout, feature indexing and quantisation follow
the format used by `pawn` by Rui Coelho
(<https://github.com/ruicoelhopedro/pawn>), GPL-3, which is the format the
network was trained against. No code was copied — the reader in `src/nnue.rs`
was written from the format's specification and is checked against that program
as an oracle.

## The network reader, when the engine is built with the bridge

`nnue_vendor/` is **derived from the Stockfish NNUE implementation**
(<https://github.com/official-stockfish/Stockfish>), GPL-3. It is real code,
not an idea: the licence header of every original file is kept verbatim at the
top of that file, and `nnue_vendor/README.md` records what was changed — the
namespace, and one accessor added to `position.h`.

It is compiled only with `--features ponte`, which is the production build. The
files that are **ours** inside that directory say so in their own headers:
`ponte.cpp` (the bridge between this program's search and that reader) and
`conta_h2k.cpp` (accumulator counters). `src/cola_rede.cpp` is also ours.

Compatibility: this program is GPL-3-or-later and the vendored reader is GPL-3,
so it may be included. The direction matters and only works one way — GPL-3
code may enter a work under a stricter licence, never the reverse. Code from
AGPL-3 engines (Reckless, Icarus) therefore **cannot** be brought in here: it
would force this whole program to AGPL-3, or be a violation. Verified by
literal search for their constants, 20-09-2026: none present.

## incbin — a gap to close before publishing

`nnue_vendor/incbin/incbin.h` is third-party: it carries `@author Dale Weiler`
and nothing else. **The upstream licence text did not travel with the vendored
copy.** That is a real gap, not a formality: distributing someone's file
without its terms is exactly what a notices file exists to prevent.

It is only reached by the vendored reader's default-network `incbin`, which
this program does not use — the network arrives through `EvalFile`. Whoever
prepares a release should either fetch the upstream licence text and place it
beside the file, or drop the file and the `incbin` path with it. Recorded here
rather than quietly assumed, because guessing a licence is worse than saying it
is unverified.

## Tablebases

Syzygy probing uses Fathom (<https://github.com/jdart1/Fathom>), MIT licensed.
Its licence text travels with the vendored sources.
