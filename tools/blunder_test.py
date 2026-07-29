"""Blunder regression harness: replay Kestrel's own real mistakes.

Feed it the suite built by build_blunder_suite.py and one or more engine
binaries. For every position where Kestrel really blundered in a rated
game, the engine is asked to move again -- by default under the SAME clock
it had at the time, because a blunder is usually a time-pressure event and
testing it at leisure would measure the wrong thing.

Each position scores as one of:
  FIXED   -- plays a move the suite records as best (the mistake is gone)
  BETTER  -- plays neither, but something the reference rates close to best
  BLUNDER -- plays the same bad move again (or something equally bad)

A position may record several best moves. The first suite was built at depth
16 and kept exactly one, which counted a correct choice as a failure whenever
the position had more than one right answer -- and it had one in a quarter of
them. `also` holds the rest, and any of them scores as FIXED.

The headline number is how many of our own past mistakes a build still
walks into. Unlike an SPRT this answers in minutes and says WHICH position
regressed, so a change can be diagnosed instead of just scored.

usage: python3 blunder_test.py <suite.epd> <engine> [engine2 ...] [--movetime MS | --nodes N] [--tolerance CP]
"""
import os, sys, subprocess, re, time

REF = os.environ.get("KESTREL_REF_ENGINE", "/usr/local/bin/ref-engine")
REF_DEPTH = 16
DEFAULT_TOLERANCE = 60      # cp: "close enough to best" counts as BETTER


class Engine:
    def __init__(self, path):
        # "binary:name=value,name=value" sets UCI options on the way in, so a
        # parameter sweep needs one build instead of one build per value.
        # Comparing separately-compiled binaries also compares whatever else
        # differed between them, which at these effect sizes is not a detail.
        self.opts = []
        if ":" in path:
            path, spec = path.split(":", 1)
            self.opts = [kv.split("=", 1) for kv in spec.split(",") if "=" in kv]
        self.path = path
        self.label = path + (" [" + ",".join(f"{k}={v}" for k, v in self.opts) + "]"
                             if self.opts else "")
        self.p = subprocess.Popen([path], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  text=True, bufsize=1, stderr=subprocess.DEVNULL)
        self._send("uci"); self._wait("uciok")
        self._send("setoption name Hash value 64")
        # Explicit, and before the caller's own options so they can still
        # override it. Same reason as the reference above: a multi-threaded
        # search does not return the same move twice, and a suite that cannot
        # repeat itself cannot measure a small change.
        self._send("setoption name Threads value 1")
        for k, v in self.opts:
            self._send(f"setoption name {k} value {v}")
        self._send("isready"); self._wait("readyok")

    def _send(self, s):
        self.p.stdin.write(s + "\n"); self.p.stdin.flush()

    def _wait(self, tok):
        for _ in range(200000):
            line = self.p.stdout.readline()
            if not line:
                return ""
            if tok in line:
                return line
        return ""

    def best(self, fen, movetime_ms, nodes=None):
        self._send("ucinewgame")
        self._send(f"position fen {fen}")
        # A node limit makes a run reproducible while other things use the
        # machine. Comparing builds by movetime silently compares whatever CPU
        # each run happened to get, so a suite run while the bot is playing
        # measures contention as much as the change under test.
        self._send(f"go nodes {nodes}" if nodes else f"go movetime {movetime_ms}")
        while True:
            line = self.p.stdout.readline()
            if not line:
                return None
            if line.startswith("bestmove"):
                return line.split()[1]

    def quit(self):
        try:
            self._send("quit"); self.p.wait(timeout=5)
        except Exception:
            self.p.kill()


class Ref(Engine):
    def __init__(self):
        super().__init__(REF)
        # One thread, deliberately. Lazy SMP is non-deterministic: helper
        # threads race, so the same binary on the same position can return a
        # different move between runs. Measured over four runs of one build
        # this suite returned 43, 45, 41 and 39 -- a spread of six, which is
        # larger than most changes worth measuring, and every one of those
        # numbers looks like a result. Single-threaded the search is
        # reproducible and a difference of two means something.
        self._send("setoption name Threads value 1")
        self._send("setoption name Hash value 256")
        self._send("isready"); self._wait("readyok")

    def score_of_move(self, fen, move_uci):
        """Reference score (cp, side-to-move view) for playing `move_uci`."""
        self._send("ucinewgame")
        self._send(f"position fen {fen}")
        self._send(f"go depth {REF_DEPTH} searchmoves {move_uci}")
        score = None
        while True:
            line = self.p.stdout.readline()
            if not line:
                break
            m = re.search(r" score cp (-?\d+)", line)
            if m:
                score = int(m.group(1))
            elif " score mate " in line:
                mm = re.search(r" score mate (-?\d+)", line)
                if mm:
                    score = 10000 if int(mm.group(1)) > 0 else -10000
            if line.startswith("bestmove"):
                break
        return score


def parse_suite(path):
    out = []
    for line in open(path):
        line = line.strip()
        if not line:
            continue
        parts = [p.strip() for p in line.split("|")]
        rec = {"fen": parts[0]}
        for p in parts[1:]:
            if p.startswith("played "):
                rec["played"] = p.split()[1]
            elif p.startswith("best "):
                rec["best"] = p.split()[1]
            elif p.startswith("also "):
                rec["also"] = p.split()[1].split(",")
            elif p.startswith("drop "):
                rec["drop"] = int(p.split()[1])
            elif p.startswith("clock_ms "):
                rec["clock"] = int(p.split()[1])
            elif p.startswith("think_ms "):
                v = int(p.split()[1])
                rec["think"] = v if v > 0 else None
            elif p.startswith("http"):
                rec["game"] = p
        out.append(rec)
    return out


def main():
    args = [a for a in sys.argv[1:]]
    movetime = None
    nodes = None
    tolerance = DEFAULT_TOLERANCE
    if "--nodes" in args:
        i = args.index("--nodes"); nodes = int(args[i + 1]); del args[i:i + 2]
    if "--movetime" in args:
        i = args.index("--movetime"); movetime = int(args[i + 1]); del args[i:i + 2]
    if "--tolerance" in args:
        i = args.index("--tolerance"); tolerance = int(args[i + 1]); del args[i:i + 2]
    suite_path, engines = args[0], args[1:]
    suite = parse_suite(suite_path)
    print(f"suite: {len(suite)} real blunders  |  tolerance {tolerance}cp")

    ref = Ref()
    # reference score of the best move, once per position (shared by all engines)
    for rec in suite:
        rec["best_score"] = ref.score_of_move(rec["fen"], rec["best"])

    results = {}
    for path in engines:
        eng = Engine(path)
        fixed = better = blunder = 0
        details = []
        t0 = time.time()
        for rec in suite:
            # Replay the move under the EXACT time the engine spent on it
            # in the real game (recorded from the PGN clocks). A blunder is
            # usually a time-pressure event, so giving the position more
            # thought than the game allowed would test a situation that
            # never happened. Only if the suite has no reading for a
            # position do we fall back to a rough per-move share.
            mt = movetime or rec.get("think") or max(200, min(3000, (rec.get("clock") or 60000) // 30))
            mv = eng.best(rec["fen"], mt, nodes)
            if mv is None:
                continue
            if mv == rec["best"] or mv in rec.get("also", ()):
                fixed += 1
                verdict = "FIXED"
            else:
                s = ref.score_of_move(rec["fen"], mv)
                if s is not None and rec["best_score"] is not None and (rec["best_score"] - s) <= tolerance:
                    better += 1
                    verdict = "BETTER"
                else:
                    blunder += 1
                    verdict = "BLUNDER"
            details.append((verdict, rec, mv))
        eng.quit()
        total = fixed + better + blunder
        score = fixed + better
        results[path] = (fixed, better, blunder, total, details)
        print(f"\n=== {path} ===")
        print(f"  avoided: {score}/{total} ({100.0*score/max(1,total):.1f}%)   "
              f"[FIXED {fixed} | BETTER {better} | still BLUNDER {blunder}]   ({time.time()-t0:.0f}s)")
    ref.quit()

    if len(engines) > 1:
        print("\n--- per-position comparison (only where builds differ) ---")
        base = results[engines[0]][4]
        for idx in range(len(base)):
            verdicts = [results[e][4][idx][0] for e in engines if idx < len(results[e][4])]
            if len(set(verdicts)) > 1:
                rec = base[idx][1]
                moves = [results[e][4][idx][2] for e in engines]
                print(f"  {rec['fen'][:40]}... best={rec['best']} "
                      f"| " + " | ".join(f"{v}({m})" for v, m in zip(verdicts, moves)))


if __name__ == "__main__":
    main()
