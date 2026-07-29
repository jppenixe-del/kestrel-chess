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
TIME_CONTROLS = [180, 60]  # try 3+0 blitz first (accepted), then 1+0 bullet
POLL_SECONDS = 20

def token():
    with open(TOKEN_PATH) as f:
        return f.read().strip()

def _headers():
    return {"Authorization": f"Bearer {token()}"}

def api_get(path):
    req = urllib.request.Request(BASE + path, headers=_headers())
    with urllib.request.urlopen(req, timeout=20) as resp:
        return json.loads(resp.read())

def api_get_stream(path):
    req = urllib.request.Request(BASE + path, headers=_headers())
    resp = urllib.request.urlopen(req, timeout=30)
    for line in resp:
        line = line.strip()
        if line:
            yield json.loads(line)

def challenge(username, base, inc=0, rated=True):
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
    for _ in range(max(1, seconds // 5)):
        time.sleep(5)
        if n_playing() >= MAX_CONCURRENT:
            return "accepted", ""
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

def main():
    rl_backoff = 120
    me = api_get("/api/account")
    my_id = me.get("id")
    my_r = me.get("perfs", {}).get("blitz", {}).get("rating", 1500)
    base_below = int(os.environ.get("KESTREL_ELO_BELOW", "300"))
    base_above = int(os.environ.get("KESTREL_ELO_ABOVE", "100"))
    max_widen = int(os.environ.get("KESTREL_ELO_WIDEN_MAX", "200"))
    widen = 0
    print(f"loop: sou {me.get('username')} (blitz {my_r}), MAX_CONCURRENT={MAX_CONCURRENT}, "
          f"banda {my_r-base_below} a {my_r+base_above}", flush=True)
    # Gentle by design: Lichess rate-limits challenge spam (HTTP 429). We
    # send AT MOST one challenge per cycle, skip a bot for a while after
    # challenging it (declined ones especially), and back off hard on 429.
    # The old version fired up to ~50 challenges/cycle and got us 429'd.
    skip = {}  # uid -> epoch until which to skip this bot
    while True:
        if n_playing() >= MAX_CONCURRENT:
            time.sleep(POLL_SECONDS); continue
        try:
            bots = list(api_get_stream("/api/bot/online?nb=40"))
        except Exception as e:
            print(f"loop: erro a listar bots: {e}", flush=True)
            time.sleep(POLL_SECONDS); continue
        now = time.time()
        def blitz(b): return b.get("perfs", {}).get("blitz", {}).get("rating")
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
                and skip.get(b["id"], 0) < now
                and not is_odds(b)
                and -below <= (blitz(b) - my_r) <= above]
        cand.sort(key=lambda b: abs(blitz(b) - my_r))
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
        ok, msg = challenge(uid, 180)
        if ok:
            rl_backoff = 120  # the challenge went out: the limit has cleared
            try:
                cid = json.loads(msg).get("id") or json.loads(msg).get("challenge", {}).get("id")
            except Exception:
                cid = None
            skip[uid] = now + 1800  # vary opponents even when it works
            state, why = challenge_outcome(cid) if cid else ("pending", "")
            # "casual" means the bot plays, just not for rating. A game we
            # learn from is worth more than a refusal we can log, so take it:
            # the alternative is an empty board. Rated stays the default,
            # because that is the only kind whose result moves a number.
            if state == "declined" and why == "casual":
                ok2, msg2 = challenge(uid, 180, rated=False)
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
                skip[uid] = now + (43200 if why == "nobot" else 3600)
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
            skip[uid] = now + 3600  # daily cap: skip 1h
            time.sleep(3)
        else:
            skip[uid] = now + 600  # declined/other: skip 10 min
            print(f"loop: {uid} recusou/erro ({msg[:50]}) -- skip 10min", flush=True)
            time.sleep(5)

if __name__ == "__main__":
    main()
