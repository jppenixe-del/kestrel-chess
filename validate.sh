#!/bin/bash
# Standard validation gate to run after any search/eval/movegen change,
# before committing. Checks run in increasing cost order, so a movegen
# regression fails fast instead of waiting for the full suite.
#
# The checks that do not need a network:
#   1. perft(5) from startpos          -- must equal 4865609
#   2. perft(4) from Kiwipete          -- must equal 4085603
#
# The checks that DO, and are skipped without one:
#   3. bench node signature            -- must equal 2716488
#   4. tactical suite                  -- reports, does not hard-fail
#
# WHY THE NETWORK MATTERS HERE. Without KESTREL_NNUE_SF the engine has no
# evaluation at all: it does not fall back to a hand-crafted one, it scores
# every position as zero and searches blind. `bench` still prints a
# plausible node count while doing it, and the tactical suite still
# produces a percentage. Both are measuring a blind engine. An earlier
# version of this script ran the suite that way and reported a baseline
# from it, which is why the network is now required rather than optional.
set -u
cd "$(dirname "$0")"
source "$HOME/.cargo/env" 2>/dev/null || true

PERFT5_ESPERADO=4865609
PERFT4_ESPERADO=4085603
BENCH_ESPERADO=2716488
KIWIPETE="r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1"

falhas=0
verifica() { # nome, obtido, esperado
    if [ "$2" = "$3" ]; then
        echo "  OK   $1: $2"
    else
        echo "  FALHA $1: $2, esperado $3"
        falhas=$((falhas + 1))
    fi
}

echo "=== building ==="
cargo build --release 2>&1 | tail -3 || exit 1

echo
echo "=== movegen (no network needed) ==="
p5=$(./target/release/kestrel perft 5 2>/dev/null | grep -oE '[0-9]{5,}' | tail -1)
verifica "perft(5) startpos" "$p5" "$PERFT5_ESPERADO"

p4=$(printf 'position fen %s\ngo perft 4\nquit\n' "$KIWIPETE" \
     | ./target/release/kestrel 2>/dev/null | grep -oE '[0-9]{5,}' | tail -1)
verifica "perft(4) Kiwipete" "$p4" "$PERFT4_ESPERADO"

echo
if [ -z "${KESTREL_NNUE_SF:-}" ]; then
    echo "=== evaluation: SKIPPED ==="
    echo "  KESTREL_NNUE_SF is not set, so there is no evaluation to check."
    echo "  Set it to an SFNNv16 .nnue file and run this again. Do not read"
    echo "  the absence of failures below as a pass -- nothing was measured."
else
    echo "=== search signature (network: $KESTREL_NNUE_SF) ==="
    b=$(./target/release/kestrel bench 2>/dev/null | grep -oE '^[0-9]+ nodes' | grep -oE '[0-9]+')
    verifica "bench nodes" "$b" "$BENCH_ESPERADO"
    echo "  (a change in this number means the search tree changed. That is"
    echo "   correct for a search or eval change and wrong for a speed one.)"

    echo
    echo "=== tactical suite ==="
    echo "  Reports a score; does not hard-fail. Some regression is sometimes"
    echo "  an acceptable trade, but it has to be a DELIBERATE one."
    echo "  Treat the number as weak evidence: this suite has been measured"
    echo "  changing 39% of its 'correct' answers between search depths, so"
    echo "  it separates large breakage from working, not good from better."
    echo "  Only SPRT decides strength."
    python3 tests/run_positions.py 2>&1 | tail -3
fi

echo
if [ "$falhas" -gt 0 ]; then
    echo "=== $falhas check(s) FAILED ==="
    exit 1
fi
echo "=== all hard checks passed ==="
