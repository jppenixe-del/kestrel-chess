#!/bin/bash
# Manutencao do bot Kestrel. Ver MANUTENCAO_BOT.md para o porque de cada coisa.
#
#   ./bot.sh start [heatmap]   arranca (heatmap = joga so da avaliacao, sem busca)
#   ./bot.sh stop              pausa e para quando o tabuleiro esvaziar
#   ./bot.sh pause             recusa desafios novos, acaba os jogos a decorrer
#   ./bot.sh resume            volta a aceitar
#   ./bot.sh status            o que esta a correr, onde, e com que opcoes
#   ./bot.sh loop [casual]     procura adversarios (casual = nao mexe no rating)
#   ./bot.sh noloop            deixa de procurar
#   ./bot.sh install           poe o binario compilado no bot e na 2a maquina
set -u
cd "$(dirname "$0")"
TOKEN_FILE=secrets/lichess_token.txt
PAUSE=BOT_PAUSED

playing() {
  curl -s "https://lichess.org/api/account/playing" \
       -H "Authorization: Bearer $(cat $TOKEN_FILE)" 2>/dev/null |
  python3 -c "import json,sys; print(len(json.load(sys.stdin).get('nowPlaying',[])))" 2>/dev/null || echo 0
}

pids() { ps -eo pid,cmd --no-headers | grep -F "$1" | grep -v grep | awk '{print $1}'; }

case "${1:-status}" in
  start)
    if [ -n "$(pids lichess_bridge.py)" ]; then
      echo "ja esta a correr. usa ./bot.sh status"; exit 1
    fi
    # Duas maquinas com o mesmo token disputam os mesmos jogos.
    # ps + grep, nao pgrep -f: o padrao esta na linha de comando do proprio ssh,
    # por isso o pgrep apanha-se a si mesmo e da sempre positivo.
    if [ -n "$(ssh -o ConnectTimeout=5 napoleon 'ps -eo cmd --no-headers | grep -F "lichess_bridge.py" | grep -v grep | grep -v "ps -eo"' 2>/dev/null)" ]; then
      echo "ERRO: o bridge esta a correr no napoleao. Para la primeiro."; exit 1
    fi
    rm -f $PAUSE
    if [ "${2:-}" = "heatmap" ]; then
      export KESTREL_HEATMAP_ONLY=1 KESTREL_HEATMAP_PLIES=${HEATMAP_PLIES:-2}
      echo "modo HEATMAP: joga so da avaliacao, sem busca (plies=${KESTREL_HEATMAP_PLIES})"
    fi
    # Tres, nao quatro, e a razao esta em tres jogos perdidos.
    #
    # Sao seis nucleos. Quatro para o motor mais um para o ponder deixam um
    # para tudo o resto, e "tudo o resto" inclui o processo Python que le o
    # socket do jogo. Quando ele nao e' escalonado, a ligacao morre com "read
    # operation timed out" e o relogio corre sozinho: tres quedas de stream no
    # log, duas delas custaram o jogo, uma por bandeira ao lance 35 de um 1+0.
    #
    # O sinal que o denuncia e' o POST do lance imediatamente antes: 3.05s onde
    # o normal sao 0.05s. Cem vezes mais lento nao e' a rede -- e' o processo a
    # nao apanhar CPU. A rede daqui chega ao site em 46ms.
    #
    # Um ply de profundidade vale menos do que um jogo inteiro.
    export KESTREL_THREADS=${KESTREL_THREADS:-3}
    export KESTREL_ELO_BELOW=${KESTREL_ELO_BELOW:-3000} KESTREL_ELO_ABOVE=${KESTREL_ELO_ABOVE:-3000}
    setsid nohup python3 -u lichess_bridge.py > lichess_bridge.log 2>&1 < /dev/null &
    sleep 6; tail -1 lichess_bridge.log
    ;;
  pause)   touch $PAUSE; echo "pausado: recusa desafios novos, os jogos a decorrer terminam" ;;
  resume)  rm -f $PAUSE; echo "a aceitar desafios" ;;
  stop)
    touch $PAUSE
    while [ "$(playing)" != "0" ]; do echo "  a espera que o jogo acabe..."; sleep 15; done
    for p in $(pids lichess_bridge.py); do kill "$p"; done
    for p in $(pids challenge_loop.py); do kill "$p"; done
    sleep 2; rm -f $PAUSE; echo "parado, sem jogos interrompidos"
    ;;
  loop)
    [ -z "$(pids lichess_bridge.py)" ] && { echo "o bridge nao esta a correr"; exit 1; }
    for p in $(pids challenge_loop.py); do kill "$p"; done
    if [ "${2:-}" = "casual" ]; then export KESTREL_CASUAL=1; echo "desafios AMIGAVEIS (rating protegido)"; fi
    setsid nohup python3 -u challenge_loop.py > challenge_loop.log 2>&1 < /dev/null &
    sleep 3; echo "a procurar adversarios"
    ;;
  noloop)  for p in $(pids challenge_loop.py); do kill "$p"; done; echo "deixou de procurar" ;;
  install)
    [ -f Kestrel/target/release/kestrel ] || { echo "compila primeiro: cd Kestrel && cargo build --release"; exit 1; }
    cp Kestrel/target/release/kestrel kestrel_bot_bin.new && mv -f kestrel_bot_bin.new kestrel_bot_bin
    echo "instalado aqui: $(md5sum kestrel_bot_bin | cut -c1-12)"
    scp -q kestrel_bot_bin napoleon:~/kestrel_bot/kestrel_new 2>/dev/null &&
      echo "copiado para o napoleao (troca no proximo arranque de la)"
    echo "NOTA: os jogos ja a decorrer continuam com o binario antigo."
    ;;
  status)
    b=$(pids lichess_bridge.py); l=$(pids challenge_loop.py)
    echo "bridge (servidor): ${b:-EM BAIXO}"
    [ -n "$b" ] && tr '\0' '\n' < /proc/${b%% *}/environ 2>/dev/null |
      grep -E "KESTREL_(HEATMAP|THREADS|ELO|ALLOW)" | sed 's/^/  /'
    echo "loop de desafios: ${l:-parado}"
    [ -n "$l" ] && tr '\0' '\n' < /proc/${l%% *}/environ 2>/dev/null | grep CASUAL | sed 's/^/  /'
    [ -f $PAUSE ] && echo "PAUSADO (nao aceita desafios novos)"
    echo "jogos a decorrer: $(playing)"
    [ -n "$(ssh -o ConnectTimeout=5 napoleon 'ps -eo cmd --no-headers | grep -F "lichess_bridge.py" | grep -v grep | grep -v "ps -eo"' 2>/dev/null)" ] &&
      echo "AVISO: bridge tambem a correr no napoleao (conflito)"
    true
    curl -s "https://lichess.org/api/user/KestrelStrike" 2>/dev/null | python3 -c "
import json,sys; p=json.load(sys.stdin)['perfs']
print(f\"rating: bullet {p['bullet']['rating']} | blitz {p['blitz']['rating']}\")" 2>/dev/null
    ;;
  *) sed -n '2,14p' "$0" ;;
esac
