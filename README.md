# ♞ Kestrel

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Lichess Bot](https://img.shields.io/badge/lichess-KestrelStrike-brightgreen?logo=lichess&logoColor=white)](https://lichess.org/@/KestrelStrike)
[![Rust](https://img.shields.io/badge/language-Rust-orange?logo=rust)](https://www.rust-lang.org/)

A from-scratch classical chess engine, written in Rust — bitboards, alpha-beta
search with PVS, null-move pruning, late move reductions, aspiration windows,
quiescence search with Static Exchange Evaluation, a transposition table with
proper mate-score handling, and move ordering built on killer moves and a
history heuristic. The evaluation leans aggressive and tactical — mobility,
pressure on the enemy king, a non-linear bonus for piling up attackers — and
is paired with a signature opening book drawn from 1825 real games by one of
the sharpest attacking players in chess history.

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
| **Bullet rating** | ~1770 (provisional, still settling) |
| **Started at** | 3000 (Lichess's default for new BOT accounts) |
| **Status** | Finding its real level through real games — expected to keep moving as bugs get fixed and pieces get added |

## 🏗️ Architecture

- **Move generation** — bitboard-based, validated by perft (`startpos` depth
  6 = `119,060,324`; Kiwipete depth 4 = `4,085,603`).
- **Search** — negamax + PVS, iterative deepening with aspiration windows,
  null-move pruning, late move reductions, reverse futility pruning,
  razoring, futility pruning, mate distance pruning, transposition table
  with ply-correct mate scoring.
- **Move ordering** — TT move → SEE-verified good captures → killer moves →
  history heuristic (bonus + malus) → opening-book preference (never
  overriding a genuinely good capture) → bad captures.
- **Evaluation** — material + PST + mobility + king-zone attacker density
  (non-linear, tuned for tactical/sacrificial play) + bishop pair + rook
  file bonuses + passed pawns. A fast material-only path runs inside
  quiescence to keep bullet time controls playable.
- **Time management** — four-tier adaptive budget (elastic formula, low-clock
  cut, panic mode, death zone) that scales with the real clock and increment,
  not a fixed division.

## 📈 Status

This project is under active, ongoing development, in the open. Real bugs get
found, fixed, and validated against evidence — perft, tactical sanity checks,
self-play A/B testing — before being kept; see the commit history for the
specifics of each one. Treat every number in this README as "as of the last
update," never as a permanent claim.

## 🔧 Building

```bash
cargo build --release
./target/release/kestrel perft 5   # sanity check: should print 4865609
```

## 📄 License

MIT — see [LICENSE](LICENSE).
