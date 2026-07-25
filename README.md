# ♞ Kestrel

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Lichess Bot](https://img.shields.io/badge/lichess-KestrelStrike-brightgreen?logo=lichess&logoColor=white)](https://lichess.org/@/KestrelStrike)
[![Rust](https://img.shields.io/badge/language-Rust-orange?logo=rust)](https://www.rust-lang.org/)

A from-scratch classical chess engine, written in Rust — bitboards, alpha-beta
search with PVS, eval-adaptive null-move pruning, late move reductions,
singular extensions, aspiration windows, quiescence search with Static
Exchange Evaluation, a lock-free transposition table with proper mate-score
handling, and a staged move picker built on killer moves, continuation history
and a history heuristic. The hand-crafted evaluation leans aggressive and
tactical — heavy mobility, pressure on the enemy king through a non-linear
attacker-density term, threats, and pawn structure — and is paired with a
signature opening book drawn from 1825 real games by one of the sharpest
attacking players in chess history.

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
| **Blitz / Bullet rating** | ~2190 / ~2185 (moving as the engine improves) |
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
- **Evaluation** — hand-crafted, tapered mg/eg: material + piece-square
  tables, per-piece mobility, king safety (non-linear attacker density, safe
  checks per piece type, weak king ring, king-flank pressure, uncastled-king
  penalty), threats by piece type, full pawn structure (passed / candidate /
  isolated / doubled / backward, phalanx, defended, passer–king proximity,
  rooks behind passers), bishop pair, rook files, a complexity adjustment and
  endgame scaling for drawish material.
- **Tuning** — the evaluation weights are calibrated by Kestrel's own logistic
  tuner on datasets the engine generates itself through self-play, and every
  change is validated by SPRT before adoption. Search parameters are tuned
  separately by SPSA over real games.
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
forum discussions and published papers — but every line of code is written
from scratch for this codebase, and every evaluation and search value is
tuned on Kestrel's own data and validated in play, never copied from another
engine.

## 📄 License

MIT — see [LICENSE](LICENSE).
