#!/usr/bin/env python3
"""Ponte minima Lichess Bot API <-> motor UCI kestrel.

So' biblioteca standard (urllib/json/threading/subprocess) -- deliberado,
para nao depender de "pip install" de codigo externo (ver decisao da
sessao de 2026-07-20: instalar o lichess-bot oficial via pip foi
bloqueado pelo classificador de seguranca por ser codigo externo nao
pedido explicitamente; esta ponte propria evita o problema por completo).
"""
import json
import os
import statistics
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

# Paths relative to THIS file by default, so the same bridge runs unchanged on
# the server and on the second machine. They used to be absolute to
# /root/kestrel_joao, which meant the box with the spare cores needed an edited
# copy -- and an edited copy is a copy that drifts.
_HERE = os.path.dirname(os.path.abspath(__file__))
TOKEN_PATH = os.environ.get("KESTREL_TOKEN_PATH",
                            os.path.join(_HERE, "secrets", "lichess_token.txt"))
# Dedicated binary, not the build tree. Pointing the bot at
# target/release/kestrel meant every rebuild during development silently
# swapped the engine the bot plays with -- including work that had not
# been validated, or had not even finished compiling. This copy only
# changes when someone deliberately replaces it.
ENGINE_CMD = [os.environ.get("KESTREL_ENGINE", os.path.join(_HERE, "kestrel_bot_bin"))]
BASE = "https://lichess.org"

# 2026-07-20 (migracao napoleon -- pedido do utilizador: "sim faz isso,
# quando tens gpu disponivel"): a consulta ao LLM agora e' feita DENTRO
# do proprio motor Rust (advisor.rs), nao aqui na ponte -- ver o commit
# "Add Rust-native LLM advisor integration". A ponte so' precisa de
# definir estas variaveis de ambiente no subprocesso do motor; o resto
# (decidir se ha empate, reservar tempo, consultar o Ollama local,
# escolher entre as linhas) e' inteiramente interno ao `go`. Ollama do
# napoleon so' ouve em 127.0.0.1 (confirmado nesta sessao), por isso isto
# so' funciona correndo NO napoleon -- e' exactamente onde este ficheiro
# passa a correr.
# DISABLED 2026-07-27, measured. Enabling the advisor makes the engine force
# MultiPV to at least 3 (`multipv.max(3)` in uci.rs), because a tie-breaker
# needs alternatives to choose between. That costs a full ply on the line
# actually played -- depth 20 without, 19 with, repeatably, on the same
# position and clock -- on every move of every game.
#
# And it bought nothing: the model named here, `kestrel-advisor`, IS NOT
# INSTALLED on this host. Ollama is running and answers in 1.5ms with an
# error, because the only model present is `qwen2.5-coder:7b`. So the engine
# has been paying a ply per move to consult an adviser that could never
# reply.
#
# Re-enabling means first creating the model, then measuring whether its
# advice is worth a ply -- which is a real price, not a rounding error.
ADVISOR_ENV = {}

# Create this file to make the bot decline every incoming challenge; delete it
# to resume. Checked per challenge, so it takes effect immediately and needs no
# restart -- which is the point (see the note at the pause check below).
PAUSE_FILE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "BOT_PAUSED")


def token():
    with open(TOKEN_PATH) as f:
        return f.read().strip()


def _headers():
    return {"Authorization": f"Bearer {token()}"}


def api_stream(path, timeout=60):
    # 2026-07-20 (BUG REAL corrigido -- 3 jogos aceites ficaram
    # "aborted" porque a ponte nunca reagiu: com timeout=None, se a
    # ligacao ficar silenciosamente inactiva -- comum em streams HTTP
    # longos atras de NAT/proxy -- o `for raw in resp` bloqueia para
    # sempre sem nunca lancar excepcao nem reconectar. Timeout de
    # leitura real (bem acima do intervalo de keepalive do Lichess,
    # ~9s) para que uma ligacao morta seja detectada e o `main()`
    # possa reconectar.
    req = urllib.request.Request(BASE + path, headers=_headers())
    resp = urllib.request.urlopen(req, timeout=timeout)
    for raw in resp:
        line = raw.decode("utf-8").strip()
        if line:
            yield json.loads(line)


def api_get(path):
    req = urllib.request.Request(BASE + path, headers=_headers())
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.loads(resp.read())


def api_post(path, data=None, timeout=15, attempts=1):
    """`attempts` > 1 retries on a network stall (not on an HTTP error, which
    is an answer and will not change on a retry)."""
    last = None
    for i in range(attempts):
        req = urllib.request.Request(BASE + path, method="POST", data=(data or b""), headers=_headers())
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return resp.read()
        except urllib.error.HTTPError as e:
            print(f"[api_post] {path} -> HTTP {e.code}: {e.read()}", file=sys.stderr)
            return None
        except Exception as e:
            last = e
            print(f"[api_post] {path} tentativa {i+1}/{attempts} falhou: {e}", file=sys.stderr)
    if last is not None:
        print(f"[api_post] {path} desistiu: {last}", file=sys.stderr)
    return None


# As tablebases vivem NO MOTOR, como opcao UCI, e nao aqui.
#
# Estiveram aqui durante uma hora e sairam antes de jogarem um lance. Duas
# implementacoes da mesma coisa e' o modo de falha que este projecto ja teve
# tres vezes num dia: um segundo caminho que DESCREVE o que o primeiro faz em
# vez de o chamar, e que deixa de bater certo sem ninguem dar por isso. No
# motor tambem serve qualquer outro cliente, e o motor pode reportar o lance
# como seu -- com tempo zero, porque nao pensou, perguntou.


def api_post_form(path, form_data):
    """POST com application/x-www-form-urlencoded (Lichess bot chat API
    aceita este formato, nao JSON puro)."""
    body = urllib.parse.urlencode(form_data).encode()
    req = urllib.request.Request(
        BASE + path, method="POST", data=body,
        headers={**_headers(), "Content-Type": "application/x-www-form-urlencoded"},
    )
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            return resp.read()
    except urllib.error.HTTPError as e:
        print(f"[api_post_form] {path} -> HTTP {e.code}: {e.read()}", file=sys.stderr)
        return None


def build_eval_bar(cp_from_white, ply):
    """Constroi uma "regua" ASCII da avaliacao actual, do estilo do
    chess.com/Lichess: 20 casas de largura, o "fill" desloca-se para o
    lado favorecido em funcao da magnitude do score. Retorna string
    pronta para enviar ao chat (ex.: `[+1.23] BBBBBBBBBB██████░░░░░░░░`
    -- lado esquerdo = brancas, direito = pretas).

    `cp_from_white` e o score em centipeoes DO PONTO DE VISTA DAS
    BRANCAS (positivo = brancas melhor). Usa uma funcao sigmoide-like
    para nao saturar em vantagens grandes (Elo real corresponde a algo
    tipo tanh(cp/400))."""
    import math
    N = 20
    # sigmoide (aproximacao): winrate para o branco em 0..1
    if abs(cp_from_white) >= 100000 - 1000:
        # mate score -- barra cheia dum lado
        wr = 1.0 if cp_from_white > 0 else 0.0
        mate_n = (100000 - abs(cp_from_white)) // 100
        text = f"#{int(mate_n)}" if mate_n > 0 else "#"
    else:
        wr = 1.0 / (1.0 + math.exp(-cp_from_white / 180.0))
        text = f"{cp_from_white / 100.0:+.2f}"
    white_cells = int(round(wr * N))
    black_cells = N - white_cells
    bar = "█" * white_cells + "░" * black_cells
    return f"[{text}] {bar}  (Kestrel eval, lance {ply // 2 + 1})"


def post_chat(game_id, text, room="spectator"):
    """Envia mensagem no chat da partida. `room=spectator` mostra so'
    a quem estiver a ver o jogo, nao spamma o adversario. `player` seria
    ao adversario tambem."""
    api_post_form(f"/api/bot/game/{game_id}/chat", {"room": room, "text": text[:140]})


class Engine:
    def __init__(self, threads=1):
        # One thread. Lazy SMP here is non-deterministic in a way that is not
        # just a measurement problem: helper threads race and the move played
        # depends on which finished first, so the engine that plays a game is
        # not exactly the engine that was validated. Everything is measured
        # single-threaded (see the blunder suite), and the bot should play the
        # same search that was measured. Parallel search is a separate question
        # and gets tested separately.
        env = os.environ.copy()
        env.update(ADVISOR_ENV)
        self.proc = subprocess.Popen(
            ENGINE_CMD, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, bufsize=1, env=env,
        )
        self._send("uci")
        self._read_until("uciok")
        # 64MB was set before the transposition table's sizing bug was fixed,
        # when a request for 64 actually allocated 32 (tt.rs halved every
        # realistic size). At 4 threads and ~750k nodes/s a single 3s move
        # visits over 2M nodes, so a game overwrites a table this small many
        # times over. 256MB per engine, and two engines run at once (the main
        # one and the ponder's), which is 512MB against ~9GB free here.
        self._send("setoption name Hash value 256")
        # 2026-07-20 (Lazy SMP, ver commit e5832de): validado 80% (16/20)
        # em self-play bullet (Threads=4 vs 1) -- exactamente o cenario
        # onde o motor perdeu material num jogo real. 4 em 6 nucleos, para
        # deixar margem no servidor partilhado (10.0.0.1 corre outras
        # coisas).
        # Threads must leave room for THIS PROCESS to read its sockets.
        # Measured: Lichess sends a keepalive on the event stream every 7.0s
        # like clockwork, so a 60s read timeout can only fire if we are not
        # reading -- and with a 4-thread engine plus a 4-thread ponder on 6
        # cores, the Python reader is exactly what gets starved. The result
        # was constant "read operation timed out" and, once, a game abandoned
        # mid-play. The ponder is a guess about the next move; it does not get
        # the same budget as the search that has to produce this one.
        self._send(f"setoption name Threads value {threads}")
        # Ate' sete pecas o resultado ja esta calculado; perguntar custa 149ms e
        # responde com a verdade em vez de uma avaliacao. 22% dos nossos jogos
        # chegam la. O motor cai para a busca normal em qualquer falha.
        self._send("setoption name OnlineTablebase value true")
        self._send("isready")
        self._read_until("readyok")

    def set_move_overhead(self, ms):
        """Tell the engine what to hold back for everything that is not
        thinking. Sent between searches, which UCI allows, because the value is
        not knowable when the engine starts -- it is measured from our own
        moves."""
        self._send(f"setoption name Move Overhead value {int(ms)}")

    def _send(self, cmd):
        self.proc.stdin.write(cmd + "\n")
        self.proc.stdin.flush()

    def _read_until(self, prefix, timeout=30):
        deadline = time.time() + timeout
        lines = []
        while time.time() < deadline:
            line = self.proc.stdout.readline()
            if not line:
                break
            line = line.strip()
            lines.append(line)
            if line.startswith(prefix):
                return lines
        raise TimeoutError(f"timeout a espera de '{prefix}'")

    def newgame(self):
        self._send("ucinewgame")

    def bestmove(self, moves, wtime_ms, btime_ms, winc_ms, binc_ms):
        """Devolve (mv, score_cp). score_cp e o score da ULTIMA linha
        `info` que apareceu antes do bestmove, do ponto de vista do
        lado a jogar (convencao do motor). None se nao houve info."""
        pos = "position startpos" + (" moves " + " ".join(moves) if moves else "")
        self._send(pos)
        self._send(
            f"go wtime {int(max(1, wtime_ms))} btime {int(max(1, btime_ms))} "
            f"winc {int(winc_ms)} binc {int(binc_ms)}"
        )
        raw = self._read_until("bestmove", timeout=180)
        return self._parse_bestmove(raw), self._parse_last_score(raw)

    @staticmethod
    def _parse_last_score(raw_lines):
        """Extrai o score da ULTIMA linha `info depth ... score ...`
        que apareceu antes do bestmove -- e' a mais profunda e portanto
        a mais fiavel. Score em centipeao (cp) ou convertido de mate."""
        last_score = None
        for line in raw_lines:
            if not line.startswith("info depth"):
                continue
            tok = line.split()
            if "cp" in tok:
                idx = tok.index("cp") + 1
                if idx < len(tok):
                    try:
                        last_score = int(tok[idx])
                    except ValueError:
                        pass
            elif "mate" in tok:
                idx = tok.index("mate") + 1
                if idx < len(tok):
                    try:
                        n = int(tok[idx])
                        last_score = (100000 - n * 100) if n > 0 else (-100000 - n * 100)
                    except ValueError:
                        pass
        return last_score

    def multipv_lines(self, moves, wtime_ms, btime_ms, winc_ms, binc_ms, n=3):
        """Same real time budget as bestmove(), but asks for the top `n`
        lines (MultiPV via exclusion, see search.rs) for LOGGING/visibility
        purposes. Returns (lines, bestmove): `lines` is a list of
        (pv_index, move_uci, score_cp, pv_moves) ordered by pv_index;
        `bestmove` is the engine's own FINAL decision (the authoritative
        "bestmove" UCI line) -- already reflects any internal advisor
        consultation (see advisor.rs), since that happens entirely inside
        the engine's own `go` handling now, not here."""
        pos = "position startpos" + (" moves " + " ".join(moves) if moves else "")
        self._send(pos)
        self._send(
            f"go wtime {int(max(1, wtime_ms))} btime {int(max(1, btime_ms))} "
            f"winc {int(winc_ms)} binc {int(binc_ms)} multipv {max(1, n)}"
        )
        raw = self._read_until("bestmove", timeout=180)
        return self._parse_multipv(raw), self._parse_bestmove(raw)

    def multipv_lines_movetime(self, moves, movetime_ms, n=3):
        """Same as multipv_lines(), but with a fixed compute budget instead
        of a real-clock wtime/btime allocation -- used for pondering, where
        the search runs off the real game clock during the opponent's turn
        and a fixed cap is all that's needed."""
        pos = "position startpos" + (" moves " + " ".join(moves) if moves else "")
        self._send(pos)
        self._send(f"go movetime {int(max(1, movetime_ms))} multipv {max(1, n)}")
        raw = self._read_until("bestmove", timeout=(movetime_ms / 1000.0) + 30)
        return self._parse_multipv(raw), self._parse_bestmove(raw)

    @staticmethod
    def _parse_multipv(raw_lines):
        out = []
        for line in raw_lines:
            if line.startswith("info depth") and " multipv " in line and " pv " in line:
                tok = line.split()
                pv_idx = int(tok[tok.index("multipv") + 1])
                pv_moves = tok[tok.index("pv") + 1:]
                move = pv_moves[0]
                if "cp" in tok:
                    score = int(tok[tok.index("cp") + 1])
                elif "mate" in tok:
                    mate_n = int(tok[tok.index("mate") + 1])
                    score = 100000 - mate_n * 100 if mate_n > 0 else -100000 - mate_n * 100
                else:
                    score = 0
                out.append((pv_idx, move, score, pv_moves))
        out.sort(key=lambda x: x[0])
        return out

    @staticmethod
    def _parse_bestmove(raw_lines):
        for line in reversed(raw_lines):
            if line.startswith("bestmove"):
                parts = line.split()
                return parts[1] if len(parts) >= 2 else None
        return None

    def quit(self):
        try:
            self._send("quit")
            time.sleep(0.2)
            self.proc.terminate()
        except Exception:
            pass


# 2026-07-20: MultiPV (para visibilidade/log de qual linha foi escolhida)
# custa uma pesquisa ~3x mais cara (metodo de exclusao, ver search.rs) --
# nao vale a pena em bullet, onde cada NPS conta (ver o commit Lazy SMP,
# motivado exactamente por uma derrota de bullet). O motor Rust decide
# sozinho (via ADVISOR_MIN_BUDGET_MS, calculado do wtime/btime reais) se
# vale a pena consultar o LLM -- esta flag so' controla se a PONTE pede
# multipv=3 para poder logar a escolha, nao se o advisor corre.
MULTIPV_UNSAFE_SPEEDS = {"bullet", "ultraBullet"}

# 2026-07-20 (pedido do utilizador: "o advisor estar a analisar o jogo
# enquanto o adversario joga e ter ja' uma ideia das hipoteses" --
# pondering). Kestrel's own UCI loop is fully synchronous (no background
# search thread, "stop" is a no-op -- see uci.rs), so real ponderhit/stop
# semantics aren't available. Poor-man's version instead: right after we
# post our own move, guess the opponent's reply (PV[1] of our own chosen
# line) and run a SEPARATE engine process analysing the resulting position
# in a background thread for up to PONDER_MOVETIME_MS, off our own clock
# entirely. If the opponent's real move matches the guess, that analysis
# is reused directly instead of starting a fresh search -- free thinking
# time recovered. On a miss, it's simply discarded (the background CPU
# time cost nothing on our clock either way).
# 2026-07-24: PONDER_MOVETIME_MS was 6000. Root cause of the bot's time
# forfeits: on a ponder HIT, _consume_ponder joins the ponder thread, which
# blocks until the fixed movetime elapses. If the opponent replied faster
# than the movetime (normal in blitz), the join burns the REMAINING ponder
# time off OUR clock -- measured ~5.9s per hit. On a low clock (per-move
# budget ~350ms) that is a 6s spike = instant flag. Two guards: (a) a short
# movetime so a hit never blocks long, and (b) don't ponder at all once our
# clock is low, where a spike is fatal (a spike with a comfortable clock is
# harmless). This is what actually fixed the flags, not the move-overhead
# change (which was validated against an unrealistic 300ms latency).
# What the engine holds back per move, measured rather than assumed.
#
# The engine's own default is 150ms, right for the ~50ms POST we see against
# people and bots. Against the server's own engine the same POST is measured at
# 560ms -- not our network, which reaches the site in 46ms, but the path on the
# far side. At 150 the engine believes it has half a second per move it does not
# have, and a bullet game is thirty of those: it flags with the search never at
# fault and the clock reading positive.
#
# So it is measured per game. A bigger reserve costs strength when it is not
# needed (250 measurably lost non-flag games in self-play), which is exactly why
# it must not be raised globally to cover the worst opponent.
MOVE_OVERHEAD_FLOOR_MS = 150    # never below the engine's own default
MOVE_OVERHEAD_SAFETY = 1.5      # cover jitter above the median we measured
MOVE_OVERHEAD_MIN_SAMPLES = 3   # two POSTs is not a measurement
MOVE_OVERHEAD_STEP_MS = 100     # only resend when it moved by this much

PONDER_MOVETIME_MS = 2000
PONDER_MIN_CLOCK_MS = 30000  # only ponder with a comfortable clock
# The ponder engine runs alongside the real one. Four plus four on six cores
# starved the bridge's own socket reader (see the Threads note in Engine).
PONDER_THREADS = 1

# --- Decidir o RESULTADO, nao so o lance -------------------------------------
#
# The bridge used to ignore draw offers entirely: it never accepted one and
# never made one. In a game that ran to 149 moves and was lost on time in a
# dead-equal position, an offer either way would have saved the half point --
# and at 3+0 with no increment, a long equal game is exactly where the clock
# decides a game the pieces do not.
#
# The engine's own evaluation is the input. It is not a strong engine's
# evaluation, so the thresholds are wide and the memory is long: a single
# iteration saying 0.00 means little, six in a row mean the position is not
# going anywhere.
# Every threshold below is in the ENGINE'S centipawns, and that unit moved.
#
# The fitted weight set evaluates 1.45x louder than the hand-calibrated one it
# replaced -- measured at fixed nodes over 105 positions, median ratio 1.449.
# These numbers were chosen against the old scale, so read literally they now
# fire at different positions than they were meant to: resigning at -1200 with
# the new engine happens where the old one said -828, which is a savable game
# thrown away.
#
# Divided through by the measured factor rather than re-picked by hand, so they
# keep meaning the position they were calibrated to mean. If the evaluation
# scale is ever refitted again, this factor is what has to move with it.
EVAL_SCALE = 1.45         # new engine's centipawns per old engine's

DRAW_EQUAL_CP = int(35 * EVAL_SCALE)    # |score| under this counts as "level"
DRAW_MEMORY = 8           # how many of our own moves we look back over
DRAW_OFFER_AFTER = 8      # level for this many moves before we offer
DRAW_ACCEPT_CP = int(60 * EVAL_SCALE)   # accept if we are not better than this
# Offer only once the clock is a real risk. With time in hand there is no
# reason to stop playing -- we win most games we play out.
DRAW_OFFER_CLOCK_FRAC = 0.35
# Resign only when it is beyond doubt and has been for a while. This does not
# change the result -- a lost game is lost -- it buys the time back for the
# next game, which at an 80% score is worth more than the moves it skips.
RESIGN_CP = int(-1200 * EVAL_SCALE)
RESIGN_MOVES = 10
# Entries kept in weblog/eval.jsonl (see _append_eval_log).
EVAL_LOG_LINES = 400
# Plies the engine treats as the opening (mirrors OPENING_PLIES in uci.rs).
# Inside it the book answers instantly and the search is capped hard, so a
# ponder can only cost clock, never save it.
OPENING_PLIES = 16


def new_ponder_state():
    return {"lock": threading.Lock(), "thread": None, "predicted": None, "after_moves_len": None, "result": None}


def _ponder_worker(ponder_state, moves_for_ponder, predicted_reply):
    result = None
    penv = None
    try:
        penv = Engine(threads=PONDER_THREADS)
        penv.newgame()
        # Also one line, and here it matters more than anywhere: a ponder hit
        # is PLAYED straight from this search, so with three lines a quarter of
        # our moves (measured 24/95 in one game) were played on 1s of thinking
        # against a per-move budget of ~3.5s.
        lines, bm = penv.multipv_lines_movetime(moves_for_ponder + [predicted_reply], PONDER_MOVETIME_MS, n=1)
        result = (lines, bm)
    except Exception:
        result = None
    finally:
        if penv is not None:
            penv.quit()
    with ponder_state["lock"]:
        # Only commit if nothing newer superseded this prediction meanwhile.
        if ponder_state.get("predicted") == predicted_reply and ponder_state.get("after_moves_len") == len(moves_for_ponder):
            ponder_state["result"] = result


def _start_ponder(ponder_state, moves_for_ponder, predicted_reply):
    with ponder_state["lock"]:
        ponder_state["predicted"] = predicted_reply
        ponder_state["after_moves_len"] = len(moves_for_ponder)
        ponder_state["result"] = None
    t = threading.Thread(target=_ponder_worker, args=(ponder_state, list(moves_for_ponder), predicted_reply), daemon=True)
    ponder_state["thread"] = t
    t.start()


def _consume_ponder(ponder_state, moves):
    """Returns cached (lines, bestmove) if the opponent's actual last move
    matches what we predicted and pondered on, else None (miss)."""
    with ponder_state["lock"]:
        predicted = ponder_state.get("predicted")
        after_len = ponder_state.get("after_moves_len")
        thread = ponder_state.get("thread")
    if predicted is None or after_len is None:
        return None
    if len(moves) != after_len + 1 or moves[-1] != predicted:
        return None
    if thread is not None:
        # Cap the wait at the movetime (+small margin for spawn): a hit must
        # never block longer than the ponder itself would run. Combined with
        # the low-clock guard in handle_state, this bounds the clock cost of
        # a ponder hit.
        thread.join(timeout=PONDER_MOVETIME_MS / 1000.0 + 1.0)
    with ponder_state["lock"]:
        return ponder_state.get("result")


def decide_result(game_id, state, my_color, scores, initial_clock, my_clock):
    """Accept, offer or resign -- before spending a move on the position.

    `scores` is our own evaluation after each of our moves, in centipawns from
    our side. Returns True if the game is now over as far as we are concerned.
    """
    # Did the opponent offer? Lichess reports the offer under the OFFERING
    # side's colour, so ours is the other one.
    opp_offering = state.get("bdraw" if my_color == "white" else "wdraw", False)
    recent = scores[-DRAW_MEMORY:]
    if opp_offering:
        # Accept unless we are clearly better. A draw offered in a position we
        # are losing or holding is a result, not a concession -- and the games
        # this bot loses on time are precisely the ones it was not losing.
        latest = recent[-1] if recent else 0
        if latest <= DRAW_ACCEPT_CP:
            api_post(f"/api/bot/game/{game_id}/draw/yes", timeout=5, attempts=2)
            print(f"[{game_id}] empate aceite (a nossa aval: {latest}cp)")
            return True
        api_post(f"/api/bot/game/{game_id}/draw/no", timeout=5, attempts=2)
        print(f"[{game_id}] empate recusado (estamos melhores: {latest}cp)")
        return False

    # Nothing to decide until the position has been level for a while AND the
    # clock has become the thing most likely to decide the game.
    if len(recent) >= DRAW_OFFER_AFTER and initial_clock:
        level = all(abs(sc) <= DRAW_EQUAL_CP for sc in recent[-DRAW_OFFER_AFTER:])
        low = my_clock < initial_clock * DRAW_OFFER_CLOCK_FRAC
        if level and low:
            api_post(f"/api/bot/game/{game_id}/draw/yes", timeout=5, attempts=2)
            print(f"[{game_id}] empate proposto -- {DRAW_OFFER_AFTER} lances nivelados "
                  f"e {my_clock/1000:.0f}s de relogio")
            # An offer is not an ending: we keep playing until it is taken.
            return False

    if len(scores) >= RESIGN_MOVES and all(sc <= RESIGN_CP for sc in scores[-RESIGN_MOVES:]):
        api_post(f"/api/bot/game/{game_id}/resign", timeout=5, attempts=2)
        print(f"[{game_id}] desistencia -- {RESIGN_MOVES} lances abaixo de "
              f"{RESIGN_CP}cp (ultimo: {scores[-1]}cp)")
        return True
    return False


def handle_state(engine, game_id, state, my_color, seen_moves, speed, opp_rating=None, ponder_state=None, scores=None, overhead=None):
    status = state.get("status")
    if status not in ("started", "created"):
        return False
    moves_str = state.get("moves", "")
    moves = moves_str.split() if moves_str else []
    if len(moves) == seen_moves[0]:
        return True
    seen_moves[0] = len(moves)
    to_move = "white" if len(moves) % 2 == 0 else "black"
    if to_move != my_color:
        return True
    wtime = state.get("wtime", 60000)
    btime = state.get("btime", 60000)
    winc = state.get("winc", 0)
    binc = state.get("binc", 0)
    # Decide the RESULT before deciding the move. A draw taken here costs
    # nothing; the same draw refused costs the whole game when the clock runs
    # out twenty moves later in a position that never changed.
    if scores is not None:
        # The reference clock is the first one we ever saw on this game, in the
        # same unit as every later reading. Taking it from the game's `clock`
        # field instead mixed seconds with milliseconds and the low-clock test
        # never fired -- caught by a unit test, not by a game.
        if scores.get("initial") is None:
            scores["initial"] = wtime if my_color == "white" else btime
        try:
            if decide_result(game_id, state, my_color,
                             scores["cp"], scores.get("initial"),
                             wtime if my_color == "white" else btime):
                return False
        except Exception as e:
            print(f"[{game_id}] decisao de resultado falhou: {e}", file=sys.stderr)
    bm = None
    top_score = None  # score cp do PONTO DE VISTA DO LADO A JOGAR
    reason = "plain engine (bullet-speed safety gate)"
    my_clock = wtime if my_color == "white" else btime
    # Per-move timing breakdown. A lost game showed two moves eating 17s and
    # 12s off a 34s clock while the engine, replayed on the same position with
    # the same clock, returned in 2.68s -- so the time was spent somewhere
    # outside the search and nothing in the log said where. These three
    # numbers split the move into the parts that can each be blamed
    # separately: `stream` is how much of our clock was already gone when the
    # event reached us (Lichess bills us from the opponent's move, not from
    # our first instruction), `think` is the engine, `post` is the move
    # reaching the server. Whichever one holds the missing seconds is the one
    # to fix, and guessing between them has already cost one wrong diagnosis.
    t_arrive = time.time()
    ph = {"clock_on_arrival": my_clock}
    # Only ponder with a comfortable clock: a ponder-hit join can cost up to
    # PONDER_MOVETIME_MS off our clock, which is fatal on a low clock (see note
    # at PONDER_MOVETIME_MS). Above the threshold a spike is harmless.
    # No pondering during the opening. A ponder HIT is consumed by joining the
    # worker thread, which blocks until its fixed movetime elapses -- measured
    # at 2.80s on move 7 of a real game, against an opening budget of 70ms.
    # Forty times the allowance, and bought nothing: the book answers those
    # positions instantly, so there was no search to save.
    # Pondering is OFF. It ran a SECOND engine process alongside the real
    # one, and the two together starved this process of the CPU it needs to
    # read its own sockets: Lichess sends a keepalive every 7.0s like
    # clockwork, so the constant "read operation timed out" in the log was
    # never the network -- it was us not getting scheduled. One of those
    # drops abandoned a live game at move 31. What pondering bought was a
    # guess at the opponent's reply; what it cost was the bridge's grip on
    # the game.
    ponder_ok = False
    if speed not in MULTIPV_UNSAFE_SPEEDS:
        try:
            cached = _consume_ponder(ponder_state, moves) if ponder_ok else None
            ponder_hit = cached is not None
            if cached is not None:
                lines, bm = cached
            else:
                # One line, not three. MultiPV splits the clock between the
                # lines and the engine gives line 1 only half the budget (see
                # uci.rs), so asking for three alternatives halves the thinking
                # that goes into the move actually played. The two extra lines
                # were never used to decide anything -- they only fed the
                # "rank N of M tied lines" log message below, which reports
                # which line the engine settled on and then plays its bestmove
                # regardless. Half the search on every move of every game is a
                # steep price for a diagnostic, and the eval bar and ponder
                # prediction both read line 1 only.
                lines, bm = engine.multipv_lines(moves, wtime, btime, winc, binc, n=1)
            if lines and bm:
                top_score = lines[0][2]
                # These are ITERATIONS, not alternatives: one entry per
                # depth the search completed, in order. The old wording --
                # "rank 19 of 20 tied lines" -- read as though the engine had
                # picked the nineteenth-best move out of twenty, and cost a
                # real investigation before the log turned out to be lying.
                # Nineteen of twenty is the deepest iteration but one, which
                # is exactly where the played move should come from.
                agree = [mv for _, mv, sc, _ in lines if abs(sc - top_score) <= 30]
                held = sum(1 for mv in agree[-6:] if mv == bm)
                if len(agree) > 1:
                    reason = (f"escolhido nas ultimas {held} de 6 iteracoes "
                              f"({len(agree)} dentro de 30cp): {agree[-6:]}")
                else:
                    reason = "engine top choice, no tie"
                if ponder_hit:
                    reason = f"[ponder hit] {reason}"
                chosen_pv = next((pv for _, mv, _, pv in lines if mv == bm), None)
                if ponder_ok and chosen_pv and len(chosen_pv) > 1:
                    _start_ponder(ponder_state, moves + [bm], chosen_pv[1])
        except Exception as e:
            print(f"[{game_id}] multipv path erro, a usar motor puro: {e}", file=sys.stderr)
            bm = None
    if bm is None:
        bm, top_score = engine.bestmove(moves, wtime, btime, winc, binc)
    ph["think"] = time.time() - t_arrive
    if bm:
        opp_str = f", opp elo {opp_rating}" if opp_rating is not None else ""
        t_post = time.time()
        # Short timeout, retried, rather than one long wait. The move POST
        # runs ON OUR CLOCK: Lichess bills us until it has the move, so a
        # stalled connection is spent thinking time we never got to use. The
        # default 15s wait is longer than a whole move's budget in blitz --
        # by the time it gives up the game can already be decided. Three
        # tries at 3s recover from a stall in milliseconds when the network
        # is merely slow, and cap the damage at 9s when it is genuinely down.
        api_post(f"/api/bot/game/{game_id}/move/{bm}", timeout=3, attempts=3)
        ph["post"] = time.time() - t_post
        # Learn this opponent's latency from our own moves, and tell the engine.
        # Adaptive rather than a bigger constant: the cost differs by a factor of
        # ten between opponents, and a reserve large enough for the worst one
        # throws away thinking time against every other.
        if overhead is not None:
            overhead["samples"].append(ph["post"] * 1000.0)
            if len(overhead["samples"]) >= MOVE_OVERHEAD_MIN_SAMPLES:
                med = statistics.median(overhead["samples"][-12:])
                want = max(MOVE_OVERHEAD_FLOOR_MS, int(med * MOVE_OVERHEAD_SAFETY))
                if abs(want - overhead["sent"]) >= MOVE_OVERHEAD_STEP_MS:
                    try:
                        engine.set_move_overhead(want)
                        print(f"[{game_id}] move overhead {overhead['sent']} -> {want}ms "
                              f"(POST mediano {med:.0f}ms em {len(overhead['samples'])} lances)")
                        overhead["sent"] = want
                    except Exception as e:
                        print(f"[{game_id}] nao consegui ajustar o overhead: {e}", file=sys.stderr)
        # Our own reading of the position after each of our moves. This is the
        # only memory the draw and resign decisions have.
        if scores is not None and top_score is not None:
            scores["cp"].append(top_score)
        print(f"[{game_id}] {time.strftime('%H:%M:%S')} {bm} clock={my_clock/1000:.1f}s "
              f"think={ph['think']:.2f}s post={ph['post']:.2f}s -- {reason} "
              f"(speed={speed}{opp_str})")
        # Regua de avaliacao no chat da partida (spectator) -- so em
        # tempos de jogo que nao sejam apertados demais (bullet/ultra):
        # postar chat custa uma request extra por lance, evitamos em
        # bullet para nao pagar tempo real. Score do motor vem do ponto
        # de vista do LADO A JOGAR (o motor devolve sempre assim); a
        # regua e' relativa a BRANCAS, portanto negamos se estamos como
        # pretas.
        if top_score is not None and speed not in MULTIPV_UNSAFE_SPEEDS:
            cp_from_white = top_score if my_color == "white" else -top_score
            ply = len(moves) + 1
            try:
                post_chat(game_id, build_eval_bar(cp_from_white, ply))
            except Exception as e:
                print(f"[{game_id}] chat post falhou (ignorado): {e}", file=sys.stderr)
        # Log estruturado do eval para o web viewer local consumir --
        # ver weblog/index.html + tampermonkey script na raiz do repo.
        _append_eval_log(game_id, top_score, my_color, len(moves) + 1, bm)
    return True


def _append_eval_log(game_id, score_cp, my_color, ply, mv):
    """Ficheiro line-delimited JSON dentro do weblog. Cada linha e' o
    snapshot mais recente da avaliacao de UM lance. O web viewer e o
    userscript le'em o mais recente por game_id."""
    try:
        import os
        weblog = os.path.join(os.path.dirname(os.path.abspath(__file__)), "weblog", "eval.jsonl")
        os.makedirs(os.path.dirname(weblog), exist_ok=True)
        cp_from_white = None
        if score_cp is not None:
            cp_from_white = score_cp if my_color == "white" else -score_cp
        entry = {
            "game_id": game_id,
            "ply": ply,
            "score_cp": cp_from_white,
            "played": mv,
            # Which colour Kestrel is. `score_cp` stays White-relative because
            # that is what an eval bar is, but the NUMBER a reader wants is
            # "how is Kestrel doing", and the bar has to be drawn the way the
            # board is facing. Neither is derivable from the score alone.
            "my_color": my_color,
            "ts": time.time(),
        }
        with open(weblog, "a") as f:
            f.write(json.dumps(entry) + "\n")
        # Keep it short. The browser userscript that draws the eval bar
        # fetches this whole file on every poll and scans backwards for the
        # current game, so an append-only log becomes a megabyte the viewer
        # re-downloads every second. Only the tail is ever read.
        if entry["ply"] % 20 == 0:
            with open(weblog) as f:
                lines = f.readlines()
            if len(lines) > EVAL_LOG_LINES:
                with open(weblog, "w") as f:
                    f.writelines(lines[-EVAL_LOG_LINES:])
    except Exception:
        pass  # log-only, nao afecta o jogo


def adopt_orphans(active_games, my_id):
    """Start playing games the server says are ours and nobody is playing.

    The event stream is the only thing that tells us a game has begun, and it
    drops -- 46 times in one day, measured. A `gameStart` that arrives during a
    reconnect is simply never seen, and the game then sits there with our clock
    running and nobody to move: the exact signature of the losses on time where
    one move ate forty seconds. Reconnecting does not fix it, because the
    stream only replays events from the moment it opens.

    So the board itself is the authority, not the events. Anything live that
    has no thread gets one.
    """
    try:
        data = api_get("/api/account/playing")
    except Exception:
        return
    for g in data.get("nowPlaying", []):
        gid = g.get("gameId")
        if not gid:
            continue
        if gid not in active_games:
            print(f"[{gid}] jogo orfao adoptado (o evento perdeu-se numa queda do stream)")
            active_games.add(gid)
            threading.Thread(target=play_game, args=(gid, my_id), name=f"game-{gid}", daemon=True).start()
            continue
        # Ours, tracked -- and still waiting. A thread that died, or a game
        # stream that went quiet, leaves the clock running with nobody to move.
        # Being tracked is not the same as being played.
        if g.get("isMyTurn") and (g.get("secondsLeft") or 999) < 30:
            live = any(t.name == f"game-{gid}" and t.is_alive() for t in threading.enumerate())
            if not live:
                print(f"[{gid}] a nossa vez, {g.get('secondsLeft')}s no relogio e ninguem a jogar "
                      f"-- a retomar", file=sys.stderr)
                threading.Thread(target=play_game, args=(gid, my_id),
                                 name=f"game-{gid}", daemon=True).start()


def prune_finished(active_games):
    """Drop games from `active_games` that the server says are over, and
    return what is genuinely still running.

    The set is maintained from the event stream, which can and does drop. A
    lost `gameFinish` is unrecoverable from the stream alone, so the state has
    to be reconcilable against the server; otherwise one lost event disables
    the bot permanently and silently.
    """
    if not active_games:
        return active_games
    try:
        live = {g["gameId"] for g in (api_get("/api/account/playing") or {}).get("nowPlaying", [])}
    except Exception as e:
        print(f"[prune] nao consegui confirmar jogos activos ({e}) -- a manter", file=sys.stderr)
        return active_games
    stale = active_games - live
    if stale:
        print(f"[prune] jogos fantasma removidos: {sorted(stale)}")
        active_games -= stale
    return active_games


def game_is_over(game_id):
    """Checks the real game status via a fresh API call -- used after a
    stream drop to decide whether to reconnect or give up."""
    try:
        d = api_get(f"/game/export/{game_id}?moves=false")
        return d.get("status") not in ("started", "created")
    except Exception:
        return False  # unknown -- assume still going, keep trying to reconnect


def play_game(game_id, my_id):
    # 2026-07-20 (BUG REAL corrigido -- causou uma derrota real por
    # tempo: um timeout de leitura no stream DESTE jogo especifico
    # terminava a funcao por completo ("erro"/"terminou"), abandonando
    # o jogo mesmo que ele continuasse activo no Lichess -- a stream
    # PRINCIPAL (main()) ja reconectava sozinha num caso semelhante, mas
    # essa reconexao nunca tinha sido replicada aqui. Agora reconecta
    # ate' 5 vezes, confirmando via game_is_over() se vale a pena
    # continuar a tentar antes de desistir de verdade.
    print(f"[{game_id}] a iniciar")
    # Threads come from the environment, because the right number is a
    # property of the MACHINE, not of the engine. The server shares six cores
    # with tests and tooling; the dedicated box has twelve and nothing else to
    # do with them. Defaulting to 1 was silently costing the bot every game it
    # played on hardware that had more.
    engine = Engine(threads=int(os.environ.get("KESTREL_THREADS", "1")))
    engine.newgame()
    my_color = None
    speed = None
    opp_rating = None
    seen_moves = [-1]
    # Per-game memory for the draw/resign decisions.
    scores = {"cp": [], "initial": None}
    # Per-game memory of how long our own move POSTs take, which is what the
    # engine has to hold back. Per game and not global: it is a property of the
    # opponent's side of the connection, and it differs by a factor of ten
    # between a bot and the server's own engine.
    overhead = {"samples": [], "sent": MOVE_OVERHEAD_FLOOR_MS}
    ponder_state = new_ponder_state()
    game_over = False
    attempts = 0
    try:
        # Keep reconnecting for as long as the SERVER says the game is live.
        # The budget used to be five attempts for the whole game, never reset,
        # and a stream that closed cleanly instead of raising consumed one
        # silently -- so a game could be abandoned after a single logged drop
        # followed by four invisible ones. It happened mid-game, at move 31
        # with 33s on the clock, against a 2366. The only thing that should
        # end this loop is the game actually being over; everything else is a
        # connection to re-open.
        while not game_over:
            attempts += 1
            if attempts > 1:
                if game_is_over(game_id):
                    print(f"[{game_id}] jogo terminado no servidor -- a sair")
                    break
                if attempts > 40:
                    print(f"[{game_id}] demasiadas reconexoes -- a desistir", file=sys.stderr)
                    break
            try:
                # Reprocess the state after a reconnect. `handle_state` skips
                # any position whose move count it has already seen, which is
                # right while the stream is healthy and fatal after it drops:
                # Lichess re-sends the full game on reconnect, the count is
                # unchanged from the last event we got, and the handler
                # returned without moving -- so the reconnect succeeded and
                # the bot still sat there until it flagged. Twice, in games it
                # was winning.
                if attempts > 1:
                    seen_moves[0] = -1
                # 15s. Measured directly: Lichess sends a keepalive on this
                # stream every 7.0s, without variance, so 15s tolerates one
                # missed beat and no more. A 60s timeout meant a silent stream
                # went unnoticed for a minute -- longer than an entire 60+0
                # game's clock, which is exactly how one was lost. The earlier
                # 20s attempt looked like it caused drops; it did not, the
                # ponder engine was starving this process of the CPU it needed
                # to read the socket, and that is fixed at the source now.
                for ev in api_stream(f"/api/bot/game/stream/{game_id}", timeout=15):
                    attempts = 1  # a live connection: forget earlier failures
                    t = ev.get("type")
                    if t == "gameFull":
                        white = ev.get("white", {})
                        black = ev.get("black", {})
                        my_color = "white" if white.get("id") == my_id else "black"
                        opp = black if my_color == "white" else white
                        opp_rating = opp.get("rating")
                        speed = ev.get("speed")
                        if not handle_state(engine, game_id, ev.get("state", {}), my_color, seen_moves, speed, opp_rating, ponder_state, scores, overhead):
                            game_over = True
                            break
                    elif t == "gameState":
                        if my_color is None:
                            continue
                        if not handle_state(engine, game_id, ev, my_color, seen_moves, speed, opp_rating, ponder_state, scores, overhead):
                            game_over = True
                            break
            except Exception as e:
                if game_is_over(game_id):
                    print(f"[{game_id}] stream caiu mas o jogo ja terminou: {e}", file=sys.stderr)
                    game_over = True
                else:
                    print(f"[{game_id}] stream do jogo caiu (tentativa {attempts}), a reconectar: {e}", file=sys.stderr)
                    # Was 2s. Mid-game this pause is spent off our own clock
                    # for no benefit -- reconnecting is what tells us whether
                    # it is our move.
                    time.sleep(0.5)
    finally:
        engine.quit()
    print(f"[{game_id}] terminou")


# 2026-07-20 (pedido do utilizador: "afinar a condicao de aceitares
# desafios a mais de x elo que tu"): recusar desafios recebidos de
# adversarios cujo rating (na mesma variante de tempo, ex. bullet) esteja
# mais de ELO_MARGIN acima do nosso rating actual -- evita derrotas
# lopsided so' porque um adversario muito mais forte nos desafiou. Limiar
# escolhido por mim (300), ajustavel.
# Filtro de Elo dependente do controlo de tempo. Em bullet a busca chega
# a profundidade muito baixa (< 500ms por lance), o motor perde tacticas
# obvias contra adversarios ~200 Elo acima (ver jogo N7671Omx, 2026-07-20:
# perdeu 3 peoes seguidos por nao recapturar). Margem muito menor em
# bullet; em blitz/rapid/classical damos mais espaco.
ELO_MARGIN_BY_SPEED = {
    "bullet": 100,
    "ultraBullet": 50,
    "blitz": 250,
    "rapid": 300,
    "classical": 350,
    "correspondence": 400,
}


def challenge_elo_ok(ch):
    speed = ch.get("speed")
    challenger_rating = ch.get("challenger", {}).get("rating")
    if speed is None or challenger_rating is None:
        return True
    try:
        acct = api_get("/api/account")
        my_rating = acct.get("perfs", {}).get(speed, {}).get("rating")
    except Exception as e:
        print(f"[challenge_elo_ok] erro a obter rating proprio, a aceitar por defeito: {e}", file=sys.stderr)
        return True
    if my_rating is None:
        return True
    # Same band in both directions of travel. This filter used to have a
    # ceiling and NO FLOOR, so while the launcher was told to look for
    # opponents in a narrow band, anyone weaker who challenged us was still
    # accepted -- and when the launcher is rate-limited, incoming challenges
    # are the only games being played, which made this the filter that
    # actually decided the opposition. Reads the launcher's own environment
    # variables so there is one band, not two.
    # The per-speed ceiling still applies on top of the configured band. It is
    # tighter at the fast end deliberately, and a band configured for the
    # blitz the launcher hunts should not loosen bullet as a side effect --
    # which is what reading the environment variable alone did.
    #
    # Note this filter governs INCOMING challenges only. A challenge issued
    # from the account itself, by hand on the website, arrives as a gameStart
    # and is played whatever its rating: launching a game directly is an
    # instruction, not a request.
    below = int(os.environ.get("KESTREL_ELO_BELOW", "300"))
    above = min(int(os.environ.get("KESTREL_ELO_ABOVE", "300")),
                ELO_MARGIN_BY_SPEED.get(speed, 300))
    return my_rating - below <= challenger_rating <= my_rating + above


def main():
    acct = api_get("/api/account")
    my_id = acct.get("id")
    print(f"ligado como {acct.get('username')} (id={my_id}), title={acct.get('title')}")
    # Is the engine we are about to play with the one that was built?
    #
    # Games have been played with a binary older than the repository more than
    # once, and nothing in the output said so -- the engine answers perfectly
    # well, it is just not the engine anyone thinks it is.
    try:
        built = os.path.join(_HERE, "Kestrel", "target", "release", "kestrel")
        if os.path.exists(built) and os.path.getmtime(built) > os.path.getmtime(ENGINE_CMD[0]) + 5:
            age = (os.path.getmtime(built) - os.path.getmtime(ENGINE_CMD[0])) / 60
            print(f"AVISO: ha um binario compilado {age:.0f} min mais recente que o do bot. "
                  f"Correr ./bot.sh install para o usar.", file=sys.stderr)
    except Exception:
        pass
    active_games = set()
    backoff = 5
    while True:
        try:
            # Every reconnect, before reading a single event: whatever we
            # missed while disconnected is on the board, not in the stream.
            adopt_orphans(active_games, my_id)
            last_sweep = time.time()
            for ev in api_stream("/api/stream/event"):
                backoff = 5  # connected: forget any previous rate-limit wait
                # And periodically while connected, because a dropped event
                # on a live connection leaves no trace at all.
                if time.time() - last_sweep > 90:
                    last_sweep = time.time()
                    adopt_orphans(active_games, my_id)
                t = ev.get("type")
                if t == "challenge":
                    ch = ev["challenge"]
                    cid = ch["id"]
                    challenger = ch.get("challenger", {}).get("id", "?")
                    if challenger == my_id:
                        # desafio que a propria conta enviou -- o evento
                        # aparece no proprio stream, mas tentar
                        # aceitar/recusar da' 404 ("nao se pode aceitar o
                        # que se propos"). So' esperar pelo "gameStart".
                        continue
                    variant_ok = ch.get("variant", {}).get("key") == "standard"
                    elo_ok = challenge_elo_ok(ch)
                    # Casual only, while the engine is being changed daily.
                    # Every rated game spends rating to buy information we are
                    # already getting for free from the blunder suite, which
                    # measures the same thing in three minutes and without
                    # noise. Rated play is worth it once a build is settled and
                    # the question is "how strong is this", not "did this
                    # change help". Set KESTREL_ALLOW_RATED=1 to open it back
                    # up -- an env var, so it is a decision someone makes on
                    # purpose rather than a default that drifts back on.
                    allow_rated = os.environ.get("KESTREL_ALLOW_RATED") == "1"
                    rated_ok = allow_rated or not ch.get("rated", False)
                    # ONE game at a time: running two games at once means two
                    # engine processes (each Threads=4) plus their ponders
                    # fighting over the CPU -- the exact contention that
                    # truncates search (flags + tactical blunders). Decline any
                    # incoming challenge while a game is already live.
                    busy = len(active_games) > 0
                    # Pause switch. Pausing the bot must never mean stopping
                    # this process: the event stream allows one connection per
                    # token, and a burst of bridge restarts left the account
                    # rate-limited (HTTP 429) for hours with no games at all.
                    # So pausing is a file, not a kill -- `touch PAUSE_FILE`
                    # declines everything arriving from now on, `rm` resumes,
                    # and games already running finish undisturbed.
                    paused = os.path.exists(PAUSE_FILE)
                    print(f"desafio de {challenger} (variant_ok={variant_ok}, elo_ok={elo_ok}, "
                          f"rated_ok={rated_ok}, busy={busy}, paused={paused}): {cid}")
                    if variant_ok and elo_ok and rated_ok and not busy and not paused:
                        api_post(f"/api/challenge/{cid}/accept")
                    else:
                        api_post(f"/api/challenge/{cid}/decline")
                elif t == "gameStart":
                    gid = ev["game"]["id"]
                    if gid in active_games:
                        pass  # already playing it
                    elif prune_finished(active_games):
                        # A game is already live. Running a second one means
                        # CPU contention (two Threads=4 engines + ponders) that
                        # truncates search -> flags and blunders. Abort this
                        # second game (allowed at the very start, no rating
                        # hit); if abort fails (it already has moves) play it
                        # rather than flag.
                        #
                        # `prune_finished` asks the server rather than trusting
                        # this set, and that is not a refinement -- it is the
                        # difference between working and not. The set is only
                        # emptied by a `gameFinish` event, so ONE missed event
                        # leaves a phantom game in it forever and every future
                        # game gets aborted as if it were a second one. That
                        # happened: the event stream was dropping repeatedly
                        # under CPU load, a finish was lost, and the bot
                        # abandoned five games in a row without playing a move.
                        print(f"[{gid}] ja ha jogo activo -- a abortar este 2o")
                        try:
                            api_post(f"/api/bot/game/{gid}/abort")
                        except Exception as e:
                            print(f"[{gid}] abort falhou ({e}) -- jogar na mesma")
                            active_games.add(gid)
                            threading.Thread(target=play_game, args=(gid, my_id), name=f"game-{gid}", daemon=True).start()
                    else:
                        active_games.add(gid)
                        th = threading.Thread(target=play_game, args=(gid, my_id), name=f"game-{gid}", daemon=True)
                        th.start()
                elif t == "gameFinish":
                    gid = ev["game"]["id"]
                    active_games.discard(gid)
        except Exception as e:
            # Back off, and back off HARD on a rate limit. Reconnecting every
            # 5s after a 429 is what keeps the 429 alive: each attempt spends
            # more of the budget we have already exhausted, so the bot sits
            # locked out indefinitely while looking like it is retrying. Seen
            # after several bridge restarts in quick succession. Anything
            # else (a dropped connection, a timeout) is transient and worth
            # retrying promptly, so only the rate limit gets the long wait.
            is_rate_limit = "429" in str(e)
            if is_rate_limit:
                backoff = min(backoff * 2 if backoff >= 60 else 60, 600)
            elif "timed out" in str(e):
                # The commonest failure by far -- 46 in one day -- and the
                # least serious: a long-lived HTTP stream that went quiet.
                # Five seconds of waiting buys nothing and is five seconds in
                # which a challenge goes unanswered.
                backoff = 1
            else:
                backoff = 5
            print(f"[main] stream caiu, a reconectar em {backoff}s: {e}", file=sys.stderr)
            time.sleep(backoff)


if __name__ == "__main__":
    main()
