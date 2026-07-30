"""Continuous opponent-finder for KestrelStrike (autonomous overnight Elo
testing). The reactive bridge (lichess_bridge.py) plays whatever games
start; this keeps a game always going by re-challenging bots whenever the
board is free.

Design notes:
 - Keeps at most MAX_CONCURRENT games live (1 by default): a single game
   at a time lets the engine use all 4 threads with clean NPS, which is
   where an HCE's speed advantage actually shows.
 - Tries multiple time controls per opponent. 60+0 (bullet rated) has
   very low acceptance -- most strong bots decline bullet or only play
   blitz/rapid -- so we also offer 180+0 (3+0 blitz). Both test the same
   strength; blitz just gets accepted far more often.
 - Skips bots that already hit Lichess's 100-games-vs-bots daily cap
   (the API returns that as a 400 with a wait-until timestamp).
 - Picks opponents by blitz rating closest to ours, so results are
   informative for Elo rather than lopsided.

stdlib only (urllib), same pattern as challenge_bots.py.
"""
import os
import json, sys, time, urllib.request, urllib.parse, urllib.error

BASE = "https://lichess.org"
TOKEN_PATH = "/root/kestrel_joao/secrets/lichess_token.txt"
MAX_CONCURRENT = 1
# Bots that have accepted our challenges before. Rebuilt from real game
# history at startup, so it follows who actually plays us rather than a list
# someone has to remember to edit.
PROVEN = set()
TIME_CONTROLS = [180, 60]  # try 3+0 blitz first (accepted), then 1+0 bullet
POLL_SECONDS = 30  # menos ciclos, menos pedidos: ver challenge_outcome

def token():
    with open(TOKEN_PATH) as f:
        return f.read().strip()

def _headers():
    return {"Authorization": f"Bearer {token()}"}

# Um 429 e' para esperar, nao para morrer.
#
# O gerador rebentava com HTTPError e o processo acabava -- o bot ficava sem
# procurar adversarios ate' alguem reparar, e "alguem reparar" pode ser horas.
# Um limite de pedidos e' temporario por definicao; a resposta certa e' parar
# de pedir durante uns minutos, nao desistir.
_travao = [0.0]


def _espera_travao():
    while time.time() < _travao[0]:
        time.sleep(5)


def _429(segundos=180):
    _travao[0] = max(_travao[0], time.time() + segundos)
    print(f"loop: 429 -- a conta esta no limite, calado {segundos}s", flush=True)


def api_get(path):
    _espera_travao()
    req = urllib.request.Request(BASE + path, headers=_headers())
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError as e:
        if e.code == 429:
            _429()
        raise

def api_get_stream(path):
    _espera_travao()
    req = urllib.request.Request(BASE + path, headers=_headers())
    try:
        resp = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as e:
        if e.code == 429:
            _429()
        raise
    for line in resp:
        line = line.strip()
        if line:
            yield json.loads(line)

def challenge(username, base, inc=0, rated=True):
    # KESTREL_CASUAL=1 forces friendly games. Used when the engine is running
    # in a mode that is not meant to be measured -- heatmap play, experiments --
    # where a rated result would cost the account something the test does not
    # pay back.
    if os.environ.get("KESTREL_CASUAL") == "1":
        rated = False
    body = urllib.parse.urlencode({
        "rated": "true" if rated else "false", "clock.limit": base, "clock.increment": inc,
        "color": "random", "variant": "standard",
    }).encode()
    req = urllib.request.Request(
        f"{BASE}/api/challenge/{username}", method="POST", data=body,
        headers={**_headers(), "Content-Type": "application/x-www-form-urlencoded"})
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            return True, resp.read().decode()
    except urllib.error.HTTPError as e:
        return False, f"HTTP {e.code}: {e.read().decode()[:100]}"
    except Exception as e:
        return False, str(e)

def challenge_outcome(cid, seconds=25):
    """What actually happened to a challenge we sent.

    Creating a challenge succeeds even when the opponent will refuse it a
    second later, and for a whole night this loop read that success as a game.
    Every one of the five bots it kept picking declines bot challenges on
    principle -- declineReasonKey "nobot" -- so the log filled with "created"
    and the board stayed empty.

    Returns (state, reason): "accepted", "declined", or "pending".
    """
    # De 9 em 9 segundos, e um pedido de cada vez.
    #
    # Eram dois de 5 em 5 -- dez pedidos por desafio enviado -- e um deles era
    # redundante: o proprio desafio ja diz se foi aceite, nao e' preciso
    # perguntar tambem a conta se esta a jogar. Somado ao resto, a conta
    # ultrapassava o limite da Lichess, e quando isso acontece os streams caem:
    # o POST de um lance esgota o tempo limite e a ligacao do jogo morre ao
    # mesmo tempo. Cinco vezes num dia, duas delas a custar o jogo por bandeira.
    #
    # Medido: a latencia esta perfeita, mediana 49ms em 90 pedidos. Nao era a
    # rede. Eram pedidos a mais.
    for _ in range(max(1, seconds // 9)):
        time.sleep(9)
        try:
            d = api_get(f"/api/challenge/{cid}/show")
        except Exception:
            continue
        st = d.get("status")
        if st == "declined":
            return "declined", d.get("declineReasonKey") or "?"
        if st == "accepted":
            return "accepted", ""
    return "pending", ""


def n_playing():
    try:
        return len(api_get("/api/account/playing").get("nowPlaying", []))
    except Exception:
        return 0

def load_proven():
    """Who has actually played us. A challenge to one of these has a real
    chance of becoming a game; a challenge to a stranger usually does not."""
    try:
        req = urllib.request.Request(
            f"{BASE}/api/games/user/kestrelstrike?max=100",
            headers={**_headers(), "Accept": "application/x-ndjson"})
        with urllib.request.urlopen(req, timeout=30) as resp:
            for line in resp.read().decode().splitlines():
                if not line.strip():
                    continue
                g = json.loads(line)
                for side in ("white", "black"):
                    uid = g.get("players", {}).get(side, {}).get("user", {}).get("id")
                    if uid and uid != "kestrelstrike":
                        PROVEN.add(uid)
    except Exception as e:
        print(f"loop: nao consegui ler o historico ({e})", flush=True)


SKIP_FILE = "/root/kestrel_joao/challenge_skips.json"


class Skips:
    """Quem nao vale a pena desafiar agora, e ate quando -- gravado em disco.

    Estava so' em memoria, e por isso durava ate ao reinicio seguinte. Foi assim
    que uma noite inteira se gastou a mandar 65 desafios a cinco bots que
    recusam bots por principio: o castigo de doze horas apagava-se sempre que a
    ponte reiniciava, e ela reinicia varias vezes por dia.

    O castigo dobra a cada recusa seguida do mesmo adversario, e volta a zero
    quando ele finalmente joga. Um bot que recusa duas vezes provavelmente vai
    recusar sempre, e insistir com ele nao custa so' tempo -- custa o limite de
    pedidos, que e' partilhado com os desafios que valiam a pena.
    """

    def __init__(self, path=SKIP_FILE):
        self.path = path
        self.ate = {}     # uid -> epoch
        self.vezes = {}   # uid -> recusas seguidas
        try:
            d = json.load(open(path))
            self.ate = {k: float(v) for k, v in d.get("ate", {}).items()}
            self.vezes = {k: int(v) for k, v in d.get("vezes", {}).items()}
            vivos = sum(1 for t in self.ate.values() if t > time.time())
            print(f"loop: {vivos} adversarios ainda de castigo (lido de {path})", flush=True)
        except Exception:
            pass

    def bloqueado(self, uid):
        return self.ate.get(uid, 0) > time.time()

    def castiga(self, uid, segundos, agrava=False):
        if agrava:
            n = self.vezes.get(uid, 0) + 1
            self.vezes[uid] = n
            segundos *= 2 ** min(n - 1, 5)   # tecto: 32x, para nao ser eterno
        self.ate[uid] = max(self.ate.get(uid, 0), time.time() + segundos)
        self._grava()

    def jogou(self, uid):
        self.vezes.pop(uid, None)
        self._grava()

    def _grava(self):
        agora = time.time()
        self.ate = {k: v for k, v in self.ate.items() if v > agora}
        self.vezes = {k: v for k, v in self.vezes.items() if k in self.ate}
        try:
            tmp = self.path + ".tmp"
            json.dump({"ate": self.ate, "vezes": self.vezes}, open(tmp, "w"))
            os.replace(tmp, self.path)
        except Exception as e:
            print(f"loop: nao consegui gravar os castigos: {e}", flush=True)


def main():
    rl_backoff = 120
    # O arranque tambem tem de sobreviver a um 429.
    #
    # Se a conta esta no limite quando este processo comeca, ele rebentava aqui
    # e ficava morto -- e a maneira mais provavel de a conta estar no limite e'
    # ter acabado de o atingir, ou seja precisamente o momento em que isto e'
    # relancado.
    me = None
    while me is None:
        try:
            me = api_get("/api/account")
        except Exception as e:
            print(f"loop: nao consegui arrancar ({e}), tento daqui a 60s", flush=True)
            time.sleep(60)
    load_proven()
    print(f"loop: {len(PROVEN)} adversarios ja jogaram connosco: "
          f"{', '.join(sorted(PROVEN))}", flush=True)
    my_id = me.get("id")
    # O nosso rating, relido de tempos a tempos.
    #
    # Era lido UMA vez ao arrancar. O bot subiu de 2290 para 2318 num dia sem a
    # banda se mexer -- procurava adversarios para o motor que ele era de manha.
    # E era sempre o de blitz, mesmo para desafiar bullet, que sao numeros
    # diferentes: 2318 e 2364 hoje.
    ratings = {"bullet": 1500, "blitz": 1500}
    ratings_lidos = [0.0]

    def actualiza_ratings(force=False):
        if not force and time.time() - ratings_lidos[0] < 600:
            return
        try:
            d = api_get("/api/account")
            for k in ("bullet", "blitz"):
                r = d.get("perfs", {}).get(k, {}).get("rating")
                if r:
                    ratings[k] = r
            ratings_lidos[0] = time.time()
        except Exception as e:
            print(f"loop: nao consegui reler o rating: {e}", flush=True)

    actualiza_ratings(force=True)
    base_below = int(os.environ.get("KESTREL_ELO_BELOW", "300"))
    base_above = int(os.environ.get("KESTREL_ELO_ABOVE", "100"))
    max_widen = int(os.environ.get("KESTREL_ELO_WIDEN_MAX", "200"))
    widen = 0
    print(f"loop: sou {me.get('username')} (bullet {ratings['bullet']}, blitz {ratings['blitz']}), "
          f"MAX_CONCURRENT={MAX_CONCURRENT}", flush=True)
    # Alternado, e nao so' blitz.
    #
    # TIME_CONTROLS existia e nunca era usado: o codigo chamava sempre
    # challenge(uid, 180), portanto o bot NUNCA desafiou para bullet -- so'
    # jogava bullet quando alguem o desafiava a ele. Metade do rating que
    # queremos medir vinha de jogos que nunca pediamos.
    rodizio = [0]
    # Gentle by design: Lichess rate-limits challenge spam (HTTP 429). We
    # send AT MOST one challenge per cycle, skip a bot for a while after
    # challenging it (declined ones especially), and back off hard on 429.
    # The old version fired up to ~50 challenges/cycle and got us 429'd.
    skip = Skips()
    while True:
        if n_playing() >= MAX_CONCURRENT:
            time.sleep(POLL_SECONDS); continue
        try:
            # Forty was a window, not a list: the bots that actually play us --
            # halcyonbot, openingsbot -- sat past position forty and were never
            # even considered, while the loop reported "nobody in the band"
            # with them online the whole time.
            bots = list(api_get_stream("/api/bot/online?nb=150"))
        except Exception as e:
            print(f"loop: erro a listar bots: {e}", flush=True)
            time.sleep(POLL_SECONDS); continue
        now = time.time()
        actualiza_ratings()
        # Este ciclo joga a este tempo, e o proximo ao outro.
        base_s = TIME_CONTROLS[rodizio[0] % len(TIME_CONTROLS)]
        rodizio[0] += 1
        perf = "bullet" if base_s < 180 else "blitz"
        my_r = ratings[perf]
        # O rating do ADVERSARIO tambem tem de ser o do mesmo tempo de jogo.
        # Estava a filtrar toda a gente pelo rating de blitz, inclusive para
        # jogos de bullet, e um bot pode ser 2400 a blitz e 2000 a bullet.
        def blitz(b): return b.get("perfs", {}).get(perf, {}).get("rating")
        # Only opponents within reach, in both directions.
        #
        # Sorting by closeness without filtering meant that whenever the
        # nearby bots were busy or declining, the list simply carried on
        # downwards: challenges went out to 1812 and to 2506 on the same
        # evening. Neither teaches anything. Beating a bot 400 points below
        # says only that the rating gap is real, and the rating barely moves;
        # losing to one 300 above produces defeats whose causes are out of
        # reach. What is worth playing is the band where the result is
        # genuinely in doubt.
        # The band widens when nobody is in it, rather than being fixed.
        #
        # Sorting by closeness without any filter sent challenges to 1812 and
        # 2506 on the same evening, and neither result teaches anything: one
        # is decided by the rating gap, the other by causes out of reach. But
        # a hard band is worse still -- with few bots online it matches nobody
        # and the engine simply stops playing, which is what a fixed 150
        # actually did. So: start tight, and give up ground slowly only when
        # there is no one to play, resetting the moment a game is found.
        # Asymmetric, because the two directions are not symmetric in what
        # they teach. A little above us is where a result is genuinely in
        # doubt and a win is worth rating; well below is still worth playing,
        # since those games are the ones that expose conversion failures --
        # being winning and not winning. Far above only produces defeats whose
        # causes are out of reach.
        # Widening only ever reaches UPWARD now. It used to move both edges,
        # which quietly undid whatever floor was configured: within a few
        # empty cycles a deliberately tight lower bound was back to where it
        # had just been raised from, and the bot was playing exactly the
        # weaker opposition it had been told to stop playing. The ceiling can
        # give ground when nobody is around, because a game slightly too
        # strong still teaches something; the floor is a decision.
        below = base_below
        above = base_above + widen
        # Odds bots are excluded. They start every game a piece or an
        # exchange down on purpose, so their rating says nothing about how
        # they play a normal position, beating them says nothing about us,
        # and the games teach nothing because the positions are artificial
        # from move one.
        def is_odds(b):
            n = (b.get("id") or "").lower()
            return "odds" in n or "forknight" in n or "handicap" in n
        cand = [b for b in bots if b.get("id") != my_id and blitz(b) is not None
                and not skip.bloqueado(b["id"])
                and not is_odds(b)
                and -below <= (blitz(b) - my_r) <= above]
        # Opponents who have played us before come first. Most bots decline
        # bot challenges outright, so a proven acceptance is worth more than a
        # closer rating: the nearest-rated stranger is usually a refusal, and
        # a refusal teaches nothing at all.
        cand.sort(key=lambda b: (b["id"] not in PROVEN, abs(blitz(b) - my_r)))
        if not cand:
            if widen < max_widen:
                widen += 50
                print(f"loop: ninguem entre {my_r-below} e {my_r+above}, a alargar", flush=True)
            else:
                print(f"loop: ninguem entre {my_r-below} e {my_r+above}, a aguardar", flush=True)
            time.sleep(POLL_SECONDS); continue
        widen = 0
        # one challenge per cycle, to the closest-rated eligible bot, at 3+0
        # (180s: best acceptance among strong bots; bullet is mostly declined)
        b = cand[0]; uid = b["id"]
        ok, msg = challenge(uid, base_s)
        if ok:
            rl_backoff = 120  # the challenge went out: the limit has cleared
            try:
                cid = json.loads(msg).get("id") or json.loads(msg).get("challenge", {}).get("id")
            except Exception:
                cid = None
            skip.jogou(uid); skip.castiga(uid, 1800)  # variar adversarios mesmo quando corre bem
            state, why = challenge_outcome(cid) if cid else ("pending", "")
            # "casual" means the bot plays, just not for rating. A game we
            # learn from is worth more than a refusal we can log, so take it:
            # the alternative is an empty board. Rated stays the default,
            # because that is the only kind whose result moves a number.
            if state == "declined" and why == "casual":
                ok2, msg2 = challenge(uid, base_s, rated=False)
                if ok2:
                    try:
                        cid2 = json.loads(msg2).get("id")
                    except Exception:
                        cid2 = None
                    state, why = challenge_outcome(cid2) if cid2 else ("pending", "")
                    print(f"loop: {uid} so joga amigaveis -> desafio casual: {state}", flush=True)
            if state == "declined":
                # A bot that refuses bot challenges will refuse the next one
                # too, and the one after that. Parking it for half a day is
                # what turns this loop from cycling five refusals all night
                # into working down the list until it finds someone who plays.
                skip.castiga(uid, 43200 if why == "nobot" else 3600, agrava=True)
                print(f"loop: {uid} recusou ({why}) -- fora por "
                      f"{'12h' if why == 'nobot' else '1h'}", flush=True)
            else:
                print(f"loop: desafio -> {uid} (blitz {blitz(b)}) 180s: {state}", flush=True)
        elif "Too many requests" in msg or "429" in msg:
            # Grow the wait each time instead of retrying at a fixed 120s.
            # A challenge rate limit is a budget we have already overspent,
            # so every retry while it is active can push the recovery further
            # out -- observed after a burst of bridge restarts, where a fixed
            # backoff sat at 429 indefinitely rather than converging. Resets
            # as soon as a challenge is accepted (see `rl_backoff` below).
            print(f"loop: rate-limited (429) -- backoff {rl_backoff}s", flush=True)
            time.sleep(rl_backoff)
            rl_backoff = min(rl_backoff * 2, 900)
        elif "played 100 games" in msg:
            # Lichess says exactly when the cap lifts. Guessing an hour meant
            # re-challenging a bot seven times before it could possibly say
            # yes, and each attempt costs a cycle someone else could have had.
            wait = 3600
            try:
                wait = int(json.loads(msg).get("ratelimit", {}).get("seconds", 3600))
            except Exception:
                pass
            skip.castiga(uid, max(60, wait))
            print(f"loop: {uid} no limite diario -- volta em {wait // 60}min", flush=True)
            time.sleep(3)
        else:
            skip.castiga(uid, 600, agrava=True)  # recusou: 10 min, a dobrar se insistir
            print(f"loop: {uid} recusou/erro ({msg[:50]}) -- skip 10min", flush=True)
            time.sleep(5)

if __name__ == "__main__":
    main()
