"""Build an ENDGAME regression suite from Kestrel's own games.

Why a separate suite: the blunder suite is 66% middlegame and holds only 11
endgame positions, so it cannot resolve a change to endgame evaluation -- it
would measure something real with a ruler too short to read it. And endgames
are where the engine is measurably worst: across three games against 2367-rated
bots, Lichess scored our middlegame at or above the opponent every time and our
endgame below it every time, once at 50% against their 99%.

The suspected cause is priced in the tables. Relative to a pawn, in the
endgame, our pieces are worth about three quarters of what they should be -- a
rook at 3.84 pawns instead of 5, a bishop at 2.37 instead of 3.3. An engine
with those numbers cannot tell a good trade from a bad one in exactly the phase
where every trade decides the game. The losing sequence of the third game was
five consecutive bishop moves ending in a trade that dropped 268 centipawns:
the most underpriced piece on our table, played like it was worth nothing.

A position qualifies when it is our turn, at most ENDGAME_PIECES pieces remain,
the reference says the played move dropped more than THRESHOLD centipawns, and
the position was not already decided. Same output format as the blunder suite,
so the same harness reads it.

Usage: build_endgame_suite.py [pgn] [out] [max_games] [threshold] [max_pieces]
"""
import os, subprocess, sys, chess, chess.pgn

PGN = sys.argv[1] if len(sys.argv) > 1 else "kestrel_games.pgn"
OUT = sys.argv[2] if len(sys.argv) > 2 else "endgames.epd"
MAX_GAMES = int(sys.argv[3]) if len(sys.argv) > 3 else 300
THRESHOLD = int(sys.argv[4]) if len(sys.argv) > 4 else 120
ENDGAME_PIECES = int(sys.argv[5]) if len(sys.argv) > 5 else 14
REF = os.environ.get("KESTREL_REF_ENGINE", "/usr/local/bin/ref-engine")
REF_DEPTH = 16
US = "kestrelstrike"
DECIDED = 800          # already won or lost: nothing to learn from the move


class Ref:
    def __init__(self):
        self.p = subprocess.Popen([REF], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  text=True, bufsize=1, stderr=subprocess.DEVNULL)
        self._send("uci"); self._wait("uciok")
        # One thread: the reference must return the same verdict for the same
        # position every time, or the suite it produces is not reproducible.
        self._send("setoption name Threads value 1")
        self._send("setoption name Hash value 256")
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

    def analyse(self, fen, moves=None):
        """Return (best_move, score_cp) from the side-to-move's view."""
        self._send("ucinewgame")
        self._send(f"position fen {fen}")
        self._send(f"go depth {REF_DEPTH}" + (f" searchmoves {moves}" if moves else ""))
        score, best = None, None
        for _ in range(400000):
            line = self.p.stdout.readline()
            if not line:
                break
            if " score cp " in line:
                try:
                    score = int(line.split(" score cp ")[1].split()[0])
                except (IndexError, ValueError):
                    pass
            elif " score mate " in line:
                try:
                    n = int(line.split(" score mate ")[1].split()[0])
                    score = 30000 - abs(n) * 100
                    if n < 0:
                        score = -score
                except (IndexError, ValueError):
                    pass
            if line.startswith("bestmove"):
                best = line.split()[1]
                break
        return best, score

    def quit(self):
        try:
            self._send("quit"); self.p.wait(timeout=5)
        except Exception:
            self.p.kill()


def main():
    ref = Ref()
    out = open(OUT, "w")
    kept = games = 0
    fh = open(PGN)
    while games < MAX_GAMES:
        g = chess.pgn.read_game(fh)
        if g is None:
            break
        games += 1
        white = (g.headers.get("White", "").lower() == US)
        board = g.board()
        node = g
        for mv in g.mainline_moves():
            our_turn = (board.turn == chess.WHITE) == white
            n_pieces = len(board.piece_map())
            node = node.variations[0] if node.variations else node
            if not (our_turn and n_pieces <= ENDGAME_PIECES):
                board.push(mv); continue
            fen = board.fen()
            best, best_sc = ref.analyse(fen)
            if best_sc is None or abs(best_sc) > DECIDED:
                board.push(mv); continue
            played = mv.uci()
            if played == best:
                board.push(mv); continue
            _, played_sc = ref.analyse(fen, played)
            if played_sc is None:
                board.push(mv); continue
            drop = best_sc - played_sc
            if drop >= THRESHOLD:
                clock = node.clock() if hasattr(node, "clock") and node.clock() else 0
                out.write(f"{fen} | played {played} | best {best} | drop {drop} | "
                          f"think_ms 0 | clock_ms {int(clock * 1000)} | "
                          f"https://lichess.org/{g.headers.get('Site','').split('/')[-1]}\n")
                out.flush()
                kept += 1
            board.push(mv)
        print(f"  jogo {games}: {kept} posicoes de final ate agora", flush=True)
    ref.quit(); out.close()
    print(f"{kept} posicoes escritas em {OUT}")


main()
