"""Recalibrate the blunder suite: deeper, and with more than one right answer.

The suite was built at depth 16 and records exactly one best move per position.
Two problems, both measured: at depth 22 that move changes in 40% of a sample,
and in many positions several moves are equally good, so insisting on one
counts a correct choice as a failure.

Writes `best` as the deepest engine's choice, plus `also` -- every other move
within TOL centipawns of it. A run that plays any of them is right.
"""
import os, chess, chess.engine, sys

SRC = "blunders_big.epd"
OUT = "blunders_big_v2.epd"
DEPTH = int(sys.argv[1]) if len(sys.argv) > 1 else 22
TOL = 20

rows = []
for l in open(SRC):
    l = l.strip()
    if l:
        rows.append(l)

e = chess.engine.SimpleEngine.popen_uci(os.environ.get("KESTREL_REF_ENGINE", "/usr/local/bin/ref-engine"))
e.configure({"Threads": 6, "Hash": 1024})
out, changed, multi = [], 0, 0
for i, line in enumerate(rows):
    parts = [p.strip() for p in line.split("|")]
    fen = parts[0]
    old = next((p.split()[1] for p in parts[1:] if p.startswith("best ")), None)
    try:
        b = chess.Board(fen)
        info = e.analyse(b, chess.engine.Limit(depth=DEPTH), multipv=6)
    except Exception:
        out.append(line)
        continue
    top = info[0]["score"].pov(b.turn).score(mate_score=30000)
    best = info[0]["pv"][0].uci()
    also = []
    for alt in info[1:]:
        sc = alt["score"].pov(b.turn).score(mate_score=30000)
        if sc is not None and top is not None and top - sc <= TOL:
            also.append(alt["pv"][0].uci())
    if old != best:
        changed += 1
    if also:
        multi += 1
    keep = [p for p in parts if not p.startswith("best ") and not p.startswith("also ")]
    keep.insert(1, f"best {best}")
    if also:
        keep.insert(2, "also " + ",".join(also))
    out.append(" | ".join(keep))
    if (i + 1) % 25 == 0:
        print(f"  {i+1}/{len(rows)}  ({changed} mudaram, {multi} com alternativas)", flush=True)
e.quit()
open(OUT, "w").write("\n".join(out) + "\n")
print(f"\n  {len(out)} posicoes -> {OUT}")
print(f"  {changed} 'best' mudaram a profundidade {DEPTH} ({100*changed/len(out):.0f}%)")
print(f"  {multi} tem pelo menos um lance alternativo igualmente bom")
