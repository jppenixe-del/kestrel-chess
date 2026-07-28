"""For each real mistake, which fader fixes it?

The suite knows the position AND the move that should have been played. So
instead of asking "does this fader setting score better overall" -- an average
that hides everything -- ask it per position: with this channel moved, does the
engine now find the right move here?

That turns a number into a map. Twenty errors fixed by the pawn channel, all of
them in locked structures, is a finding about what the evaluation is missing.
Twenty errors fixed by nothing at all is a different finding: those are not
evaluation problems, they are search problems.

Only positions we currently get WRONG are tested, since those are the ones with
room to move, and each is tried against every channel in both directions.

Usage: which_fader_fixes.py [suite.epd] [nodes]
"""
import subprocess, sys, chess
from collections import Counter, defaultdict

SUITE = sys.argv[1] if len(sys.argv) > 1 else "blunders_big.epd"
NODES = int(sys.argv[2]) if len(sys.argv) > 2 else 600_000
ENGINE = "/root/kestrel_joao/kestrel_fam"
FAMILIES = ["mobility", "king", "threats", "pawns", "pieces", "tempo"]
SETTINGS = [("base", {})]
for f in FAMILIES:
    SETTINGS.append((f"{f}-", {f: 800}))
    SETTINGS.append((f"{f}+", {f: 1200}))


def ask(fen, faders):
    cmds = ["uci", "setoption name Threads value 1", "setoption name Hash value 32"]
    for k, v in faders.items():
        cmds.append(f"setoption name scale_{k} value {v}")
    cmds += ["isready", f"position fen {fen}", f"go nodes {NODES}", "quit"]
    try:
        p = subprocess.run([ENGINE], input="\n".join(cmds) + "\n",
                           capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        return None
    for line in p.stdout.splitlines():
        if line.startswith("bestmove"):
            return line.split()[1]
    return None


def categories(board):
    """The same labels error_profile uses, so the two can be read together."""
    out = []
    n = len(board.piece_map())
    out.append("abertura" if n >= 28 else ("meio" if n >= 14 else "final"))
    locked = 0
    for sq in board.pieces(chess.PAWN, chess.WHITE):
        if chess.square_rank(sq) < 7 and chess.square(
                chess.square_file(sq), chess.square_rank(sq) + 1) in board.pieces(chess.PAWN, chess.BLACK):
            locked += 1
    if locked >= 3:
        out.append("travada")
    qs = len(board.pieces(chess.QUEEN, chess.WHITE)) + len(board.pieces(chess.QUEEN, chess.BLACK))
    if qs:
        out.append("com-damas")
    return out


def main():
    rows = []
    for line in open(SUITE):
        line = line.strip()
        if not line:
            continue
        fen = line.split('|')[0].strip()
        best = None
        for part in line.split('|'):
            if part.strip().startswith("best "):
                best = part.strip().split()[1]
        if best:
            rows.append((fen, best))

    fixed_by = defaultdict(list)
    still_wrong = []
    already_ok = 0
    tested = 0
    for fen, best in rows:
        base = ask(fen, {})
        if base is None:
            continue
        if base == best:
            already_ok += 1
            continue
        tested += 1
        winners = []
        for name, faders in SETTINGS[1:]:
            if ask(fen, faders) == best:
                winners.append(name)
        if winners:
            for w in winners:
                fixed_by[w].append(fen)
        else:
            still_wrong.append(fen)
        if tested % 10 == 0:
            print(f"  ... {tested} posicoes erradas testadas", flush=True)

    print(f"\n{len(rows)} posicoes | {already_ok} ja certas | {tested} erradas e testadas\n")
    print("canal que corrige                 erros corrigidos")
    for name, fens in sorted(fixed_by.items(), key=lambda x: -len(x[1])):
        print(f"  {name:28} {len(fens):4}")
    print(f"  {'NENHUM canal corrige':28} {len(still_wrong):4}")

    print("\ncategoria das posicoes que CADA canal corrige:")
    for name, fens in sorted(fixed_by.items(), key=lambda x: -len(x[1]))[:6]:
        c = Counter()
        for f in fens:
            for cat in categories(chess.Board(f)):
                c[cat] += 1
        top = "  ".join(f"{k}={v}" for k, v in c.most_common(4))
        print(f"  {name:12} {top}")

    c = Counter()
    for f in still_wrong:
        for cat in categories(chess.Board(f)):
            c[cat] += 1
    print(f"\ncategoria dos que NENHUM canal corrige (provavelmente busca, nao avaliacao):")
    print("  " + "  ".join(f"{k}={v}" for k, v in c.most_common(5)))


main()
