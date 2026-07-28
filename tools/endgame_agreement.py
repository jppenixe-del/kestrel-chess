"""How often do we pick the same endgame move as a strong reference?

The blunder suite cannot answer this. It is 66% middlegame and holds 11
endgame positions, so a change to endgame evaluation moves it by less than its
own resolution -- and worse, a change to endgame values also shifts every
interpolated middlegame score, so the suite reports the side effect and misses
the effect. Measured: scaling endgame pieces by 1.3 took the suite from 43 to
40, which says nothing about whether endgames improved.

This asks the narrow question directly. Take many endgame positions from real
games, ask both engines for a move under the same node budget, and count
agreement. Many positions rather than a few, because the effect size is small
and the resolution has to come from sample size.

Agreement with a reference is not the same as playing well, and a position with
several equally good moves punishes disagreement unfairly. So we score two
ways: exact agreement, and the reference's own evaluation of OUR move versus
its best -- the second is the honest one, because it prices disagreement by how
much it actually costs.

Usage: endgame_agreement.py <engine[:opt=val,...]> [positions] [nodes]
"""
import subprocess, sys, chess, chess.pgn

ENGINE = sys.argv[1]
WANT = int(sys.argv[2]) if len(sys.argv) > 2 else 120
NODES = int(sys.argv[3]) if len(sys.argv) > 3 else 400_000
REF = "/usr/local/bin/stockfish"
REF_DEPTH = 18
MAX_PIECES = 12


class Uci:
    def __init__(self, path, opts=(), threads=1, hash_mb=64):
        self.p = subprocess.Popen([path], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  text=True, bufsize=1, stderr=subprocess.DEVNULL)
        self._send("uci"); self._wait("uciok")
        self._send(f"setoption name Threads value {threads}")
        self._send(f"setoption name Hash value {hash_mb}")
        for k, v in opts:
            self._send(f"setoption name {k} value {v}")
        self._send("isready"); self._wait("readyok")

    def _send(self, s):
        self.p.stdin.write(s + "\n"); self.p.stdin.flush()

    def _wait(self, tok):
        for _ in range(400000):
            line = self.p.stdout.readline()
            if not line:
                return ""
            if tok in line:
                return line
        return ""

    def best(self, fen, limit):
        self._send("ucinewgame")
        self._send(f"position fen {fen}")
        self._send(f"go {limit}")
        for _ in range(400000):
            line = self.p.stdout.readline()
            if not line:
                return None
            if line.startswith("bestmove"):
                return line.split()[1]
        return None

    def score_of(self, fen, move):
        """Reference score for playing `move`, from the mover's view."""
        self._send("ucinewgame")
        self._send(f"position fen {fen}")
        self._send(f"go depth {REF_DEPTH} searchmoves {move}")
        sc = None
        for _ in range(400000):
            line = self.p.stdout.readline()
            if not line:
                break
            if " score cp " in line:
                try: sc = int(line.split(" score cp ")[1].split()[0])
                except (IndexError, ValueError): pass
            elif " score mate " in line:
                try:
                    n = int(line.split(" score mate ")[1].split()[0])
                    sc = (30000 - abs(n) * 100) * (1 if n > 0 else -1)
                except (IndexError, ValueError): pass
            if line.startswith("bestmove"):
                break
        return sc

    def quit(self):
        try:
            self._send("quit"); self.p.wait(timeout=5)
        except Exception:
            self.p.kill()


def positions(want):
    """Endgame positions from our own games, thinned so adjacent plies -- which
    are near-duplicates and would inflate the sample without adding evidence --
    do not both appear."""
    out, seen = [], set()
    for path in ("kestrel_games.pgn", "recent40.pgn", "gauntlet_60_0.pgn"):
        try: fh = open(path)
        except OSError: continue
        while len(out) < want:
            g = chess.pgn.read_game(fh)
            if g is None:
                break
            b = g.board()
            for i, mv in enumerate(g.mainline_moves()):
                b.push(mv)
                if b.is_game_over() or b.is_check():
                    continue
                if len(b.piece_map()) > MAX_PIECES:
                    continue
                if i % 5:
                    continue
                fen = b.fen()
                key = fen.rsplit(" ", 2)[0]
                if key in seen:
                    continue
                seen.add(key)
                out.append(fen)
                if len(out) >= want:
                    break
        fh.close()
        if len(out) >= want:
            break
    return out


def main():
    path, opts = ENGINE, []
    if ":" in ENGINE:
        path, spec = ENGINE.split(":", 1)
        opts = [kv.split("=", 1) for kv in spec.split(",") if "=" in kv]
    fens = positions(WANT)
    ours = Uci(path, opts)
    ref = Uci(REF)
    same = 0
    loss = []
    for fen in fens:
        our_mv = ours.best(fen, f"nodes {NODES}")
        ref_mv = ref.best(fen, f"depth {REF_DEPTH}")
        if our_mv is None or ref_mv is None:
            continue
        if our_mv == ref_mv:
            same += 1
            loss.append(0)
            continue
        a = ref.score_of(fen, ref_mv)
        b = ref.score_of(fen, our_mv)
        if a is not None and b is not None:
            loss.append(max(0, a - b))
    ours.quit(); ref.quit()
    n = len(loss)
    if not n:
        print("sem posicoes"); return
    # Clamp before averaging. Mate scores enter as tens of thousands, so one
    # position where the reference sees a forced mate and we do not swamps the
    # mean and makes the metric non-monotonic in the thing being swept -- the
    # first version of this reported 591, 872, 1422 and 306 for four
    # increasing factors, which is noise wearing a number's clothes. Anything
    # past CAP is "this move loses", and losing by more than that is not a
    # meaningfully worse decision.
    CAP = 300
    clamped = [min(x, CAP) for x in loss]
    avg = sum(clamped) / n
    ordered = sorted(loss)
    median = ordered[n // 2]
    bad = sum(1 for x in loss if x >= 100)
    terrible = sum(1 for x in loss if x >= CAP)
    print(f"=== {ENGINE} ===")
    print(f"  posicoes de final:        {n}")
    print(f"  lance igual a referencia: {same} ({100*same/n:.1f}%)")
    print(f"  perda media (limitada a {CAP}): {avg:.1f} cp")
    print(f"  perda mediana:            {median} cp")
    print(f"  lances a perder >=100cp:  {bad} ({100*bad/n:.1f}%)")
    print(f"  lances a perder >={CAP}cp:  {terrible} ({100*terrible/n:.1f}%)")


main()
