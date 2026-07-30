"""O que a janela de streaming precisa e a ponte nao tem.

A ponte escreve `weblog/live.json` a cada lance -- posicao, relogios, avaliacao.
Falta o que nao pertence a um jogo: o rating ao longo do tempo, e saber que NAO
ha jogo a decorrer, que e' quando a janela tem de mostrar outra coisa em vez de
congelar no ultimo lance de ontem.

Corre a parte, de meio em meio minuto, e nao toca no stream do jogo. Isso e'
deliberado: um segundo consumidor na ligacao que a ponte usa para jogar ja
custou um jogo por tempo a este bot, e uma janela nao vale esse risco.

Guarda o historico de rating em disco para o grafico ter passado quando arranca,
em vez de comecar sempre numa linha plana.
"""
import json
import os
import time
import urllib.request

BASE = os.path.dirname(os.path.abspath(__file__))
TOKEN = open(os.path.join(BASE, "secrets", "lichess_token.txt")).read().strip()
SAIDA = os.path.join(BASE, "weblog", "bot.json")
HIST = os.path.join(BASE, "weblog", "rating_hist.json")
INTERVALO = 30


def api(path):
    req = urllib.request.Request(
        "https://lichess.org" + path, headers={"Authorization": "Bearer " + TOKEN}
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read())


def carrega_hist():
    try:
        return json.load(open(HIST))
    except Exception:
        return {"bullet": [], "blitz": []}


def grava(caminho, d):
    tmp = caminho + ".tmp"
    with open(tmp, "w") as f:
        json.dump(d, f)
    os.replace(tmp, caminho)


def main():
    hist = carrega_hist()
    while True:
        try:
            me = api("/api/account")
            jogando = api("/api/account/playing").get("nowPlaying", [])
            agora = time.time()
            ratings = {}
            for k in ("bullet", "blitz"):
                r = me.get("perfs", {}).get(k, {}).get("rating")
                if not r:
                    continue
                ratings[k] = r
                serie = hist.setdefault(k, [])
                # So' grava quando MUDA, senao o grafico e' uma linha de pontos
                # identicos e o eixo do tempo deixa de significar nada.
                if not serie or serie[-1][1] != r:
                    serie.append([agora, r])
                    del serie[:-500]
            grava(HIST, hist)
            grava(SAIDA, {
                "nome": me.get("username"),
                "ratings": ratings,
                "a_jogar": len(jogando),
                "jogo": jogando[0].get("gameId") if jogando else None,
                "adversario": (jogando[0].get("opponent", {}) or {}).get("username") if jogando else None,
                "adv_rating": (jogando[0].get("opponent", {}) or {}).get("rating") if jogando else None,
                "hist": {k: v[-120:] for k, v in hist.items()},
                "ts": agora,
            })
        except Exception as e:
            print(f"estado: {e}", flush=True)
        time.sleep(INTERVALO)


main()
