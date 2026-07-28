"""Where does the engine actually play badly?

"The endgame is weak" is not actionable, and it is usually not even true --
it is an average over situations that have nothing to do with each other. This
takes the real mistakes from real games and asks what they have in common:
which piece was moved, what material was on the board, whether the position was
open or closed, whether a queen was present, whether the mistake lost material
or missed an opponent's threat.

The output is a table of categories with how often we err in each, compared
against how often that category appears at all. A category where we make 30% of
our mistakes but which is only 10% of positions is a real weakness. One where
both numbers match is just a common situation.

Usage: error_profile.py <suite.epd> [games.pgn ...]
"""
import sys, chess, chess.pgn
from collections import Counter

SUITE = sys.argv[1] if len(sys.argv) > 1 else "blunders_big.epd"
PGNS = sys.argv[2:] or ["kestrel_games.pgn", "gauntlet_60_0.pgn", "recent40.pgn"]


def categories(board, move=None):
    """Every label that applies to this position (and move, when given)."""
    out = []
    n = len(board.piece_map())
    out.append("fase:abertura" if n >= 28 else ("fase:meio" if n >= 14 else "fase:final"))

    us = board.turn
    # material presente -- o que está no tabuleiro muda o tipo de jogo mais do
    # que a contagem de peças
    qs = len(board.pieces(chess.QUEEN, chess.WHITE)) + len(board.pieces(chess.QUEEN, chess.BLACK))
    rs = len(board.pieces(chess.ROOK, chess.WHITE)) + len(board.pieces(chess.ROOK, chess.BLACK))
    bs = len(board.pieces(chess.BISHOP, chess.WHITE)) + len(board.pieces(chess.BISHOP, chess.BLACK))
    ns = len(board.pieces(chess.KNIGHT, chess.WHITE)) + len(board.pieces(chess.KNIGHT, chess.BLACK))
    out.append("damas:sim" if qs else "damas:nao")
    if qs and rs >= 2:
        out.append("material:damas+torres")
    elif not qs and rs >= 2:
        out.append("material:torres")
    elif not qs and rs == 0 and (bs or ns):
        out.append("material:so-menores")
    if bs == 2 and ns == 0:
        wb = board.pieces(chess.BISHOP, chess.WHITE)
        bb = board.pieces(chess.BISHOP, chess.BLACK)
        if len(wb) == 1 and len(bb) == 1:
            same = (chess.square_rank(list(wb)[0]) + chess.square_file(list(wb)[0])) % 2 == \
                   (chess.square_rank(list(bb)[0]) + chess.square_file(list(bb)[0])) % 2
            out.append("bispos:mesma-cor" if same else "bispos:cores-opostas")

    # estrutura: um tabuleiro travado joga-se de outra maneira
    pawns = board.pieces(chess.PAWN, chess.WHITE) | board.pieces(chess.PAWN, chess.BLACK)
    locked = 0
    for sq in board.pieces(chess.PAWN, chess.WHITE):
        if chess.square_rank(sq) < 7 and chess.square(chess.square_file(sq), chess.square_rank(sq) + 1) in board.pieces(chess.PAWN, chess.BLACK):
            locked += 1
    out.append("estrutura:travada" if locked >= 3 else "estrutura:aberta")
    out.append(f"peoes:{'muitos' if len(pawns) >= 10 else ('poucos' if len(pawns) <= 5 else 'medios')}")

    # rei exposto
    for color, tag in ((us, "nosso"), (not us, "deles")):
        k = board.king(color)
        if k is None:
            continue
        shield = sum(1 for sq in board.pieces(chess.PAWN, color)
                     if abs(chess.square_file(sq) - chess.square_file(k)) <= 1)
        if shield == 0:
            out.append(f"rei-{tag}:exposto")

    out.append("em-xeque" if board.is_check() else "sem-xeque")

    if move is not None:
        pc = board.piece_at(move.from_square)
        if pc:
            out.append("lance:" + chess.piece_name(pc.piece_type))
        if board.is_capture(move):
            out.append("lance:captura")
        elif board.gives_check(move):
            out.append("lance:xeque")
        else:
            out.append("lance:quieto")
    return out


def main():
    err = Counter()
    for line in open(SUITE):
        line = line.strip()
        if not line:
            continue
        fen = line.split('|')[0].strip()
        played = None
        for part in line.split('|'):
            if part.strip().startswith("played "):
                played = part.strip().split()[1]
        try:
            b = chess.Board(fen)
            mv = chess.Move.from_uci(played) if played else None
            if mv and mv not in b.legal_moves:
                mv = None
        except Exception:
            continue
        for c in categories(b, mv):
            err[c] += 1

    # linha de base: com que frequência cada categoria aparece nas partidas
    base = Counter()
    total_pos = 0
    for path in PGNS:
        try:
            fh = open(path)
        except OSError:
            continue
        while total_pos < 12000:
            g = chess.pgn.read_game(fh)
            if g is None:
                break
            b = g.board()
            for i, mv in enumerate(g.mainline_moves()):
                if i % 4 == 0 and not b.is_game_over():
                    for c in categories(b, mv):
                        base[c] += 1
                    total_pos += 1
                b.push(mv)
        fh.close()

    n_err = sum(1 for line in open(SUITE) if line.strip())
    print(f"{n_err} erros reais  vs  {total_pos} posicoes de referencia\n")
    print(f"{'categoria':26} {'%erros':>7} {'%normal':>8} {'racio':>7}")
    rows = []
    for c, k in err.items():
        pe = 100 * k / n_err
        pb = 100 * base.get(c, 0) / max(1, total_pos)
        if pb < 1.0:
            continue
        rows.append((pe / pb, c, pe, pb, k))
    for ratio, c, pe, pb, k in sorted(rows, reverse=True):
        flag = "  <<<" if ratio >= 1.25 and k >= 8 else ""
        print(f"  {c:24} {pe:6.1f}% {pb:7.1f}% {ratio:7.2f}{flag}")


main()
