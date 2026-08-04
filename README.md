<p align="center">
  <img src="assets/banner.png" alt="Kestrel" width="100%">
</p>

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Lichess Bot](https://img.shields.io/badge/lichess-KestrelStrike-brightgreen?logo=lichess&logoColor=white)](https://lichess.org/@/KestrelStrike)
[![Rust](https://img.shields.io/badge/language-Rust-orange?logo=rust)](https://www.rust-lang.org/)

A from-scratch chess engine, written in Rust — bitboards, alpha-beta search
with PVS, eval-adaptive null-move pruning, late move reductions, singular
extensions, aspiration windows, quiescence search with Static Exchange
Evaluation, a lock-free transposition table with proper mate-score handling,
and a staged move picker built on killer moves, continuation history and a
history heuristic. Positions are scored by a trained network, and the engine
is paired with a signature opening book drawn from 1825 real games by one of
the sharpest attacking players in chess history.

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

| | |
|---|---|
| **Bullet / Blitz rating** | ~2560 / ~2415 (moving as the engine improves) |
| **Status** | Actively developed in the open — the rating tracks a real, changing engine, not a fixed one |

## 🏗️ Architecture

- **Move generation** — bitboard-based, validated by perft (`startpos` depth
  6 = `119,060,324`; Kiwipete depth 4 = `4,085,603`).
- **Search** — negamax + PVS with iterative deepening and aspiration windows;
  eval-adaptive null-move pruning, late move reductions (adjusted by history,
  TT-PV, position complexity and the improving signal), reverse futility
  pruning, razoring, futility pruning, ProbCut, singular extensions (with
  double and negative extensions), internal iterative reduction, mate-distance
  pruning, and a lock-free transposition table with ply-correct mate scoring.
- **Static eval in search** — the pruning and reduction decisions use the
  *full* evaluation (not a cheap material-only proxy), cached in the
  transposition table and refined by a six-dimensional correction history
  (pawn, non-pawn per side, minor, major, threats) so the eval used to prune
  tracks what search actually finds. This was worth a large, SPRT-verified
  strength gain.
- **Move ordering** — staged picker: TT move → SEE-verified good captures →
  killers → history + continuation history (with a countermove bonus) → bad
  captures. Capture history breaks SEE ties.
- **Evaluation** — a quantised network, in two architectures the engine picks
  between by which file it is given rather than by a build flag, so comparing
  them compares networks and not binaries:
  - *piece-square*: `(768 → 512)x2 → 8`, twelve king buckets, eight output
    buckets by piece count. The accumulator is carried on the board and
    updated one piece at a time, with a refresh cache per king bucket — king
    moves are a quarter of all moves and a sixth of those cross a bucket
    boundary, so without that cache bucketed inputs cost more in speed than
    they return in strength.
  - *threats*: `(31744 → 512)x2 → 8`, where 9216 of the inputs encode what is
    attacking what rather than only where the pieces stand. Those weights are
    stored as `i8` and widened on the fly: at 32.5 MB the first layer does not
    fit in cache, so the accumulator is bound by memory bandwidth and halving
    the bytes moved is worth 17%.
  - Hand-written terms survive only where they are not evaluation: piece
    values, which Static Exchange Evaluation needs before any score exists to
    judge, and game phase, which the search uses for time and reductions.
- **Training** — networks are trained with [bullet](https://github.com/jw1912/bullet)
  on positions the engine generates itself, labelled by game result. Every
  candidate is validated by SPRT in real games before adoption; a lower
  training loss on its own has more than once meant nothing at the board.
  Search parameters are tuned separately by SPSA.
- **Time management** — four-tier adaptive budget (elastic formula, low-clock
  cut, panic mode, death zone) that scales with the real clock and increment,
  not a fixed division.

## 📈 Status

This project is under active, ongoing development, in the open. Real bugs get
found, fixed, and validated against evidence — perft, tactical sanity checks,
and SPRT self-play testing against a fixed reference — before being kept; see
the commit history for the specifics of each one. Treat every number in this
README as "as of the last update," never as a permanent claim.

## 🔧 Building

```bash
cargo build --release
./target/release/kestrel perft 5   # sanity check: should print 4865609
```

## 🧭 Design principle

Kestrel is meant to be an *original* engine, not a clone. Concepts are drawn
from the public chess-programming literature — the Chess Programming Wiki,
forum discussions, published papers, and other open engines — but every line
of code is written from scratch for this codebase, and every weight is trained
on Kestrel's own data and validated in play, never copied from another engine.

Reading other engines and naming them is what makes that claim checkable, so
[NOTICES.md](NOTICES.md) lists what was studied, under which licence, and
where the line falls between an idea and a copy. Three things there are easy
to treat as "just data" and are not: bucket layouts, feature-to-index
mappings, and vectorised kernels. Where this engine uses bucketed inputs, the
layout is computed from a stated rule rather than transcribed.

## 📄 License

MIT — see [LICENSE](LICENSE). Provenance and third-party credit are in
[NOTICES.md](NOTICES.md).
