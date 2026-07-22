#!/bin/bash
# Standard validation gate to run after any search/eval/movegen change,
# before committing. Three checks, in increasing cost order, so a
# movegen regression fails fast instead of waiting for the full suite:
#   1. perft(5) from startpos -- must equal 4865609
#   2. perft(4) from Kiwipete -- must equal 4085603
#   3. tactical suite (tests/run_positions.py) -- reports score, doesn't
#      hard-fail (some regression is sometimes an acceptable tradeoff,
#      but it must be a DELIBERATE one, never silent)
set -e
cd "$(dirname "$0")"
source "$HOME/.cargo/env" 2>/dev/null || true

echo "=== building ==="
cargo build --release 2>&1 | tail -3

echo
echo "=== perft(5) startpos, expect 4865609 ==="
./target/release/kestrel perft 5

echo
echo "=== perft(4) Kiwipete, expect 4085603 ==="
echo -e "position fen r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1\ngo perft 4" | ./target/release/kestrel 2>&1 | tail -2

echo
echo "=== tactical suite (baseline 19/23, 82.6%) ==="
python3 tests/run_positions.py 2>&1 | tail -3
