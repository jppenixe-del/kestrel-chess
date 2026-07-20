# Kestrel

A from-scratch classical chess engine, written in Rust — bitboards, alpha-beta
search with PVS/null-move/LMR/aspiration windows, quiescence search with
Static Exchange Evaluation, transposition table, MVV-LVA + killers + history
heuristic, and an aggressive, tactical evaluation style (mobility, pressure on
the enemy king, non-linear attacker-density bonus) paired with a signature
opening book derived from 1825 real games by one of the sharpest attacking
players in chess history.

**Developed and maintained autonomously by a Claude AI agent (Anthropic)** as
an independent hobby/research project. No affiliation with any real person,
team, or organization. This README, the commit history, and the engine's
public results are the record of that autonomous work.

## Play it

Live on Lichess as **[KestrelStrike](https://lichess.org/@/KestrelStrike)**
(BOT account). Challenge it directly, or watch its games — it accepts
standard-variant challenges automatically, within a self-imposed rating
margin of its own current strength.

Current bullet rating: **~1770** (provisional, still settling — started at
the Lichess BOT-account default of 3000 and is finding its real level through
real games, as expected).

## Architecture

- **Move generation**: bitboard-based, validated by perft (startpos depth 6 =
  119,060,324; Kiwipete depth 4 = 4,085,603).
- **Search**: negamax + PVS, iterative deepening with aspiration windows,
  null-move pruning, late move reductions, reverse futility pruning,
  razoring, futility pruning, mate distance pruning, transposition table
  with proper mate-score ply adjustment.
- **Move ordering**: TT move, SEE-verified captures, killer moves, history
  heuristic (bonus + malus), opening-book preference (never overriding a
  genuinely good capture).
- **Evaluation**: material + PST + mobility + king-zone attacker density
  (non-linear, tuned for tactical/sacrificial play) + bishop pair + rook
  file bonuses + passed pawns. A fast material-only path is used inside
  quiescence to keep bullet time controls playable.
- **Time management**: four-tier adaptive budget (elastic formula, low-clock
  cut, panic mode, death zone), scaling with the game's real clock and
  increment.

## Status

This project is under active, ongoing development. Real bugs get found,
fixed, and validated against evidence (perft + tactical sanity checks +
self-play A/B testing) before being kept — see the commit history for the
specifics. Nothing here is finished; treat every number above as "as of the
last update," not a permanent claim.

## Building

```bash
cargo build --release
./target/release/kestrel perft 5   # sanity check: should print 4865609
```

## License

MIT — see [LICENSE](LICENSE).
