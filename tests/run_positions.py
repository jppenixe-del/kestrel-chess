#!/usr/bin/env python3
"""Runs the Kestrel engine against tests/positions.epd and reports a
score: how many positions the engine solves under a per-position time
budget, plus per-position timing and success/failure.

Also compares against Stockfish (if available) on the same positions
at the same budget -- gives a concrete answer to "how far are we from
a reference at this time control?".

Usage:
  python3 tests/run_positions.py [--movetime MS] [--stockfish]
"""
import argparse
import re
import subprocess
import sys
import threading
import queue
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
KESTREL = REPO / "target/release/kestrel"
STOCKFISH = "/usr/local/bin/stockfish"
EPD = REPO / "tests/positions.epd"


class UciEngine:
    def __init__(self, path):
        self.path = str(path)
        self.proc = subprocess.Popen(
            [self.path], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1,
        )
        self.q = queue.Queue()
        threading.Thread(target=self._reader, daemon=True).start()
        self._send("uci")
        self._wait_for("uciok", 10)

    def _reader(self):
        for line in self.proc.stdout:
            self.q.put(line.rstrip("\n"))

    def _send(self, cmd):
        self.proc.stdin.write(cmd + "\n")
        self.proc.stdin.flush()

    def _wait_for(self, token, timeout):
        end = time.time() + timeout
        lines = []
        while time.time() < end:
            try:
                line = self.q.get(timeout=0.3)
            except queue.Empty:
                continue
            lines.append(line)
            if token in line:
                return lines
        raise TimeoutError(f"timeout waiting for {token}")

    def bestmove(self, fen, movetime_ms):
        self._send("ucinewgame")
        self._send(f"position fen {fen}")
        self._send(f"go movetime {movetime_ms}")
        lines = self._wait_for("bestmove", timeout=movetime_ms / 1000 + 30)
        for line in lines:
            if line.startswith("bestmove"):
                return line.split()[1]
        return None

    def quit(self):
        try:
            self._send("quit")
            self.proc.wait(timeout=3)
        except Exception:
            self.proc.kill()


def parse_epd(path):
    """Yield (fen, best_moves_set, id) tuples from an EPD file."""
    for raw in path.open():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        # split at " bm "
        m = re.match(r"^(.+?)\s+bm\s+([^;]+);\s*id\s*\"([^\"]+)\";", line)
        if not m:
            continue
        fen = m.group(1).strip()
        # If FEN has only 4 fields, add halfmove/fullmove defaults
        parts = fen.split()
        if len(parts) == 4:
            fen = fen + " 0 1"
        bms = m.group(2).strip().split()
        pid = m.group(3).strip()
        yield fen, set(bms), pid


def run_suite(engine_path, movetime_ms, label):
    print(f"\n=== {label}  (movetime {movetime_ms} ms) ===")
    engine = UciEngine(engine_path)
    positions = list(parse_epd(EPD))
    correct = 0
    total_ms = 0
    fails = []
    for fen, bms, pid in positions:
        t0 = time.time()
        try:
            mv = engine.bestmove(fen, movetime_ms)
        except Exception as e:
            mv = None
            print(f"  {pid:40s}  ERROR: {e}")
        dt = int((time.time() - t0) * 1000)
        total_ms += dt
        ok = mv in bms if mv else False
        marker = "OK" if ok else "FAIL"
        if ok:
            correct += 1
        else:
            fails.append((pid, mv, bms))
        print(f"  [{marker}]  {pid:40s}  played={mv}  expected={sorted(bms)}  {dt}ms")
    engine.quit()
    print(f"\n{label} score: {correct}/{len(positions)}  ({100*correct/max(1,len(positions)):.1f}%)  total {total_ms/1000:.1f}s")
    if fails:
        print(f"failed: {len(fails)}")
    return correct, len(positions), fails


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--movetime", type=int, default=2000, help="ms per position")
    ap.add_argument("--stockfish", action="store_true", help="also run Stockfish reference")
    args = ap.parse_args()

    if not KESTREL.exists():
        print(f"missing: {KESTREL}", file=sys.stderr)
        sys.exit(1)

    ker_correct, total, ker_fails = run_suite(KESTREL, args.movetime, "Kestrel")

    if args.stockfish and Path(STOCKFISH).exists():
        sf_correct, _, _ = run_suite(STOCKFISH, args.movetime, "Stockfish")
        print(f"\ngap vs Stockfish: {sf_correct - ker_correct} positions")


if __name__ == "__main__":
    main()
