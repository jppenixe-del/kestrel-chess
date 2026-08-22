<p align="center">
  <img src="assets/banner.png" alt="Kestrel" width="100%">
</p>

[![License: GPLv3](https://img.shields.io/badge/license-GPLv3-blue.svg)](COPYING)
[![Lichess Bot](https://img.shields.io/badge/lichess-KestrelStrike-brightgreen?logo=lichess&logoColor=white)](https://lichess.org/@/KestrelStrike)
[![Rust](https://img.shields.io/badge/language-Rust-orange?logo=rust)](https://www.rust-lang.org/)

A chess engine written in Rust, its search built from scratch — bitboards,
alpha-beta search with PVS, eval-adaptive null-move pruning, late move
reductions, singular extensions, aspiration windows, quiescence search with
Static Exchange Evaluation, a lock-free transposition table with proper
mate-score handling, and a staged move picker built on killer moves,
continuation history and a history heuristic.

Positions are scored by a trained network in Stockfish's **SFNNv16** format.
The format is theirs, and reading it is the one part of this engine derived
from another — which is why this project is GPLv3. See
[License](#-license).

> **This repository is the engine, and only the engine.** No network ships
> here, and no opening book: the engine reads whatever `.nnue` file it is
> pointed at, and without one it has no evaluation at all. The pipeline that
> trains networks, the tooling that tunes the engine and runs the bot, and
> the trained weights themselves are not published.

The evaluation used to be hand-written — some eight thousand lines of
piece-square tables, mobility counts and king-safety curves, tuned on the
engine's own self-play. It is gone. Dissecting real losses fixed four genuine
bugs in it and then stopped finding any: what remained was not one mispriced
term but a hundred terms each slightly wrong in the same direction, which is
the shape of error a network fixes from data and hand tuning does not. The
first trained network beat the best hand-tuned build by **338 Elo**.

> **Developed and maintained autonomously by a Claude AI agent (Anthropic)**,
> as an independent hobby/research project. No affiliation with any real
> person, team, or organization. The commit history and the bot's public
> games are the actual record of that work — nothing here is staged.

---

## ♟️ Play it

Live on Lichess as **[KestrelStrike](https://lichess.org/@/KestrelStrike)**
(BOT account). Challenge it directly, or just watch — it accepts
standard-variant challenges automatically, within a self-imposed rating
margin of its own current strength, so games stay competitive rather than
one-sided.

Ratings are on the profile rather than in this file. A number written into a
README is stale within the week; the profile never is.

## 🏗️ Architecture

- **Move generation** — bitboard-based, validated by perft: `startpos`
  depth 5 = `4,865,609`, depth 6 = `119,060,324`; Kiwipete depth 4 =
  `4,085,603`. `validate.sh` checks the first of these on every run and fails
  on a mismatch.
- **Search** — negamax + PVS with iterative deepening and aspiration windows;
  eval-adaptive null-move pruning, late move reductions (adjusted by history,
  TT-PV, position complexity and the improving signal), reverse futility
  pruning, razoring, futility pruning, ProbCut, singular extensions (with
  double and negative extensions), internal iterative reduction, mate-distance
  pruning, and a lock-free transposition table with ply-correct mate scoring.
- **Static eval in search** — the pruning and reduction decisions use the
  *full* evaluation, not a cheap material-only proxy: cached in the
  transposition table and refined by a six-dimensional correction history
  (pawn, non-pawn per side, minor, major, threats), so the eval used to prune
  tracks what the search actually finds.
- **Move ordering** — staged picker: TT move → SEE-verified good captures →
  killers → history + continuation history (with a countermove bonus) → bad
  captures. Capture history breaks SEE ties.
- **Evaluation** — a quantised network in the SFNNv16 format: `86896` inputs
  into a `1024`-wide accumulator, in three blocks that answer three different
  questions about a position.
  - `HalfKAv2_hm` — *where the pieces are*, indexed jointly with the side's
    own king square, so the same material reads differently depending on
    where the king sits. Mirrored on the king's file, which halves the table.
  - `Full_Threats` (`59808`) — *what is attacking what*, rather than only
    where things stand. Two positions with identical material on identical
    squares are still different positions if the pieces bear on each other
    differently, and this is the block that sees that.
  - `PP_3Wide` (`4560`) — *pawn structure*, as pairs of pawns at most one
    file apart. The restriction is the whole idea: trained on all pawn pairs,
    the ones that carry information turn out to be the near ones. **This
    feature is not this project's invention** — see [License](#-license).
  - The accumulator is carried on the board and updated a move at a time
    rather than rebuilt. What costs is not the number of weight rows touched
    but where they come from: the threat block is the largest and the least
    local, so it is what any speed work here has to attack. Weights are
    stored narrow and widened on the fly.
  - Hand-written terms survive only where they are not evaluation: piece
    values, which Static Exchange Evaluation needs before any score exists to
    judge with, and game phase, which the search uses for time and reductions.
- **Training** — a network is being trained by this project's own pipeline on
  the Stockfish project's published 5000-node positions. It is not finished,
  and nothing is claimed for it until it has been validated in play; until
  then the engine is developed and tested against Stockfish's published
  network. The weights will be this project's, the data is not, and both are
  credited in [NOTICES.md](NOTICES.md). Every candidate network faces SPRT in
  real games before adoption — a lower training loss on its own has more than
  once meant nothing at the board. Search parameters are tuned separately by
  SPSA.
- **Time management** — four-tier adaptive budget (elastic formula, low-clock
  cut, panic mode, death zone) that scales with the real clock and increment,
  not a fixed division.

## 🔧 Building

```bash
cargo build --release
./target/release/kestrel perft 5   # move generation: 4865609
```

**The engine needs a network.** Point `KESTREL_NNUE_SF` at an SFNNv16-format
`.nnue` file:

```bash
export KESTREL_NNUE_SF=/path/to/net.nnue
./target/release/kestrel bench     # with the network: 2716488 nodes
```

Without that variable there is no evaluation at all. The engine does not fall
back to a hand-crafted one — it scores every position as zero and searches
blind. It still runs, and `bench` still prints a node count while doing it: a
different number, and a meaningless one. If you use the node count as a
regression check, take it with the network loaded.

`./validate.sh` runs the gate: perft first, then the node signature and the
tactical suite. It skips the second pair, loudly, when no network is set,
rather than reporting a pass it did not measure.

## 📈 Status

Under active development, in the open. Bugs get found, fixed and validated
against evidence — perft, the node signature, and SPRT self-play against a
fixed reference — before being kept. Treat every number here as "as of the
last update", never as a permanent claim.

## 🧭 Design principle

Kestrel is meant to be an *original* engine, not a clone, and the line falls
in a specific place.

**The search is written from scratch for this codebase**, and so is
everything around it: board representation, move generation, time management.
Concepts come from the public chess-programming literature — the Chess
Programming Wiki, forum discussions, published papers, and other open
engines — but reading how a technique works and writing your own is not the
same act as taking the code, and the difference is the whole point.

**The exception is stated plainly, because it is the kind of thing that
should not have to be discovered.** The NNUE inference side is derived from
Stockfish. Reading a network in the SFNNv16 format means implementing that
format: its quantisation, its feature sets, their index mappings. That is
Stockfish's work, it is GPLv3, and it is why this engine is GPLv3 too.

**The weights are trained here; the data is not.** The network in training is
this project's own, produced by its own pipeline, and no network from another
engine is shipped or adapted. The positions it learns from are the Stockfish
project's published data, and that is credited rather than quietly absorbed.

[NOTICES.md](NOTICES.md) carries the full account: what is third-party, under
which licence, and where the line falls between an idea and a copy. Three
things there are easy to treat as "just data" and are not — bucket layouts,
feature-to-index mappings, and vectorised kernels.

## 📄 License

[![License: GPLv3](https://img.shields.io/badge/license-GPLv3-blue.svg)](COPYING)

**GPLv3** — see [`COPYING`](COPYING).

Only the **NNUE inference side** is derived from **Stockfish** (GPLv3): the
SFNNv16-class network format this engine reads, its quantisation, and the
definition of the input feature sets it is built on — `HalfKAv2_hm`,
`Full_Threats` and `PP_3Wide`, including their index mappings and orientation
tables. **The search is the project's own**, and so is everything around it:
board representation, move generation, and the whole of `search.rs`.

**The network will be the project's own**, and is not yet. One is being
trained by this project's own pipeline; until it is finished and validated in
play, the engine is run and tested against Stockfish's published network. No
network ships in this repository either way. Stockfish supplies the *format*
the weights are stored and evaluated in — and, for now, a set of weights to
test against.

Because the engine incorporates Stockfish's GPL-licensed work, **the whole
project is distributed under GPLv3**, with Stockfish's copyright notices
preserved. Earlier public history carried an MIT notice, and that was correct
at the time: it predates the SFNNv16 evaluation entirely. The licence changed
in the same change that first published that code — not after it.

**Input feature attribution.** The pawn-pair block (`PP_3Wide`, `4560` inputs,
pairs of pawns at most one file apart) is **not an original idea of this
project**. It was invented by **Jonathan Hallström** for
[Pawnocchio](https://github.com/JonathanHallstrom/pawnocchio), was used by
Stormphrax and Viridithas, and was adopted by Stockfish as `PP_3Wide`. This
engine's implementation and its trained weights are its own; the idea is his.

Provenance and third-party credit in full are in [NOTICES.md](NOTICES.md).
