"""The REAL heatmap: what each LEGAL move with this piece is worth.

The PSQT heatmap is a portrait of a million average positions. It says a knight
on e5 is worth +30 whether an attack is raging or the board is dead. That is
what makes a hand-written evaluation feel static -- and it is only half true,
because most of our terms (mobility, threats, king safety, tropism) do read the
position.

So: for every LEGAL move of the chosen piece, play it and evaluate the position
that follows. The difference is what that move is really worth, with every
dynamic term included.

Legal moves, not teleports. An earlier version placed the piece on any empty
square, which put +58 on c7 for a pawn standing on d4 -- a square it can never
reach. Squares a piece cannot go to are not information.

Captures are included, and their value counts. Taking a defended pawn is an
even trade and should read as roughly neutral; taking a hanging piece should
read as winning material. Hiding that would leave out half of what a move is
worth.

The side to move flips after the move, which is what decides whether the piece
is then hanging or safe.
"""
import http.server, json, socketserver, subprocess, threading

PORT = 8771
# The binary the BOT plays with, deliberately -- a heatmap of a different
# build answers a question nobody asked.
ENGINE = "/root/kestrel_joao/kestrel_bot_bin"
LOCK = threading.Lock()
# Milissegundos de relogio, nao profundidade. O motor decide sozinho ate onde
# ir: para quando a jogada estabiliza, o esforco se concentra e o score deixa
# de cair. Amarrar-lhe a profundidade dava respostas que ele nunca daria num
# jogo -- o Fine #70 com profundidade 14 devolve Kb2, e com relogio de dez
# segundos devolve Kb1 ao fim de 147ms, porque vai ate onde precisa.
SEARCH_MS = 400

PIECES = {"P": "peão", "N": "cavalo", "B": "bispo", "R": "torre", "Q": "dama", "K": "rei"}



class Engine:
    """A long-lived engine process.

    Starting a process per request cost more than the work itself: ~690ms for a
    map that is twenty evaluations. Nothing here needs a fresh process --
    `eval` has no state that carries between positions, unlike the profile
    options, which are read once and would be stuck from the first request.
    """

    def __init__(self):
        self.p = None

    def _start(self):
        self.p = subprocess.Popen([ENGINE], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  text=True, bufsize=1, stderr=subprocess.DEVNULL)
        self._send("uci"); self._wait("uciok")
        self._send("setoption name Threads value 1")
        self._send("setoption name Hash value 128")
        self._send("isready"); self._wait("readyok")

    def _send(self, line):
        self.p.stdin.write(line + "\n"); self.p.stdin.flush()

    def _wait(self, token, limit=400000):
        for _ in range(limit):
            line = self.p.stdout.readline()
            if not line:
                return ""
            if token in line:
                return line
        return ""

    def sync(self):
        """Drain anything left over and prove the engine is listening.

        Without this the session desynchronises: a search leaves `info` lines
        in the pipe after `bestmove`, and the next request waits for a token
        that already went by -- which shows up as the page saying "a calcular"
        forever, for every click, once the first one has gone wrong.
        """
        self._send("isready")
        self._wait("readyok")

    def evals(self, fens):
        """One `eval` per FEN, in order, over a single session."""
        if self.p is None or self.p.poll() is not None:
            self._start()
        self.sync()
        out = []
        for f in fens:
            self._send(f"position fen {f}")
            self._send("eval")
            line = self._wait("eval(white)")
            try:
                out.append(int(line.split("total=")[1].split()[0]))
            except (IndexError, ValueError):
                out.append(0)
        return out

    def search(self, fen, ms, clock=None, inc=0, oppclock=None):
        """Search either for a fixed time, or with a real CLOCK.

        The clock is the interesting one: it lets the engine decide how long
        this position deserves, which is what it does in a game. Giving it a
        fixed movetime measures the search; giving it a clock measures the
        time management -- a position it finds hard gets several times the
        share of an obvious one, and that decision is worth watching.
        """
        if self.p is None or self.p.poll() is not None:
            self._start()
        self.sync()
        self._send("ucinewgame")
        self._send(f"position fen {fen}")
        if clock:
            # The opponent's clock is part of the decision, not decoration:
            # the same position deserves a different amount of thought when
            # we are ahead on time and when we are about to be outlasted.
            opp = oppclock or clock
            self._send(f"go wtime {clock} btime {opp} winc {inc} binc {inc}")
        else:
            self._send(f"go movetime {max(50, ms)}")
        score = None
        depth = 0
        spent = 0
        tm = None
        for _ in range(400000):
            line = self.p.stdout.readline()
            if not line:
                break
            if line.startswith("info string tm "):
                # What the engine allowed itself, and why -- the one part of
                # its reasoning that never shows up in the moves.
                t = line.split()
                try:
                    tm = {t[i]: int(t[i + 1]) for i in range(3, len(t) - 1, 2)}
                except (ValueError, IndexError):
                    tm = None
            if line.startswith("info depth"):
                t = line.split()
                try:
                    depth = int(t[t.index("depth") + 1])
                    if "time" in t:
                        spent = int(t[t.index("time") + 1])
                except (ValueError, IndexError):
                    pass
            if " score cp " in line:
                try: score = int(line.split(" score cp ")[1].split()[0])
                except (IndexError, ValueError): pass
            if line.startswith("bestmove"):
                mv = line.split()[1]
                # Leave the pipe clean for whoever comes next.
                self.sync()
                return mv, score, depth, spent, tm
        return None, None, 0, 0, None


ENG = Engine()

def parse_fen(fen):
    parts = fen.split()
    rows = parts[0].split("/")
    board = {}
    for r, row in enumerate(rows):
        f = 0
        for ch in row:
            if ch.isdigit():
                f += int(ch)
            else:
                board[(7 - r) * 8 + f] = ch
                f += 1
    return board, parts[1:] if len(parts) > 1 else ["w", "-", "-", "0", "1"]


def to_fen(board, rest):
    rows = []
    for r in range(7, -1, -1):
        row, empty = "", 0
        for f in range(8):
            p = board.get(r * 8 + f)
            if p is None:
                empty += 1
            else:
                if empty:
                    row += str(empty); empty = 0
                row += p
        if empty:
            row += str(empty)
        rows.append(row)
    return "/".join(rows) + " " + " ".join(rest)


def heatmap(fen, square, depth=SEARCH_MS, clock=None, inc=0):
    try:
        import chess
    except ImportError:
        return {"error": "python-chess em falta"}
    try:
        b = chess.Board(fen)
    except ValueError:
        return {"error": "FEN invalido"}
    piece = b.piece_at(square)
    if piece is None:
        return {"error": "casa vazia"}
    if piece.color != b.turn:
        return {"error": "nao e' a vez dessa cor"}

    moves = [m for m in b.legal_moves if m.from_square == square]
    if not moves:
        return {"error": "esta peca nao tem lances legais nesta posicao"}

    variants, targets, caps = [], [], []
    for m in moves:
        b.push(m)
        variants.append(b.fen())
        b.pop()
        targets.append(m.to_square)
        caps.append(b.is_capture(m))

    vals = ENG.evals([b.fen()] + variants)
    if len(vals) < 2:
        return {"error": "motor nao respondeu"}
    base = vals[0]
    sign = 1 if piece.color == chess.WHITE else -1
    cells, captures = {}, {}
    for sq, v, c in zip(targets, vals[1:], caps):
        cells[sq] = sign * (v - base)
        captures[sq] = c
    # O que a BUSCA escolhe, que nao e' o mesmo que a avaliacao estatica prefere.
    # A diferenca entre os dois e' a informacao que interessa: mostra onde a
    # avaliacao sozinha erraria e a procura corrige -- e onde nao corrige.
    best_uci = best_san = None
    if depth > 0 or clock:
        best_uci, _, _, _, _ = ENG.search(fen, depth, clock or None, inc)
        if best_uci:
            try:
                m = chess.Move.from_uci(best_uci)
                best_san = b.san(m) if m in b.legal_moves else None
                if best_san is None:
                    best_uci = None
            except ValueError:
                best_uci = None

    bf = bt = None
    if best_uci:
        mv = chess.Move.from_uci(best_uci)
        bf, bt = mv.from_square, mv.to_square
    return {"piece": piece.symbol(), "from": square, "base": sign * base,
            "cells": cells, "captures": captures,
            "best_from": bf, "best_to": bt, "best_san": best_san}


def best_move(fen, ms, clock=None, inc=0, oppclock=None):
    """The move the search plays. Its own endpoint so the board can show it the
    moment a position is loaded, without waiting for a 60-variant heatmap."""
    try:
        import chess
        b = chess.Board(fen)
    except (ImportError, ValueError):
        return {}
    uci, score, d, spent, tm = ENG.search(fen, ms if not clock else 0, clock, inc, oppclock)
    if not uci:
        return {}
    try:
        m = chess.Move.from_uci(uci)
    except ValueError:
        return {}
    if m not in b.legal_moves:
        return {}
    return {"best_from": m.from_square, "best_to": m.to_square,
            "best_san": b.san(m), "score": score, "depth": d, "spent": spent,
            "clock": clock, "tm": tm}


PAGE_TEMPLATE = r"""<!doctype html><meta charset=utf-8>
<title>Kestrel - heatmap real</title>
<style>
 body{background:#14161a;color:#d8dde3;font:13px/1.5 -apple-system,Segoe UI,Roboto,sans-serif;margin:0;padding:18px}
 h1{font-size:17px;margin:0 0 3px}
 .sub{color:#7d8794;font-size:12px;margin-bottom:14px;max-width:820px}
 .row{display:flex;gap:8px;margin-bottom:9px;flex-wrap:wrap;align-items:center}
 input[type=text]{background:#0f1115;border:1px solid #2a2f38;color:#d8dde3;
  border-radius:5px;padding:6px 8px;font:12px ui-monospace,monospace}
 #fen{flex:1;min-width:340px}
 select{background:#0f1115;border:1px solid #2a2f38;color:#d8dde3;border-radius:5px;padding:6px 8px;font:12px sans-serif}
 button{background:#2b6cb0;color:#fff;border:0;border-radius:5px;padding:6px 13px;cursor:pointer}
 label{color:#7d8794;font-size:12px;white-space:nowrap}
 .wrap{display:flex;gap:26px;flex-wrap:wrap}
 .board{display:grid;grid-template-columns:repeat(8,58px);grid-template-rows:repeat(8,58px);
  border:2px solid #333;border-radius:5px;overflow:hidden}
 .sq{display:flex;flex-direction:column;align-items:center;justify-content:center;
  font:11px ui-monospace,monospace;cursor:pointer;position:relative}
 .sq .pc{font-size:24px;line-height:1}
 .sq .v{font-weight:700;text-shadow:0 1px 2px #000}
 .sel{outline:3px solid #6db3f2;outline-offset:-3px;z-index:2}
 .best{box-shadow:inset 0 0 0 4px #2f6fed;z-index:3}
 .bestfrom{box-shadow:inset 0 0 0 4px rgba(47,111,237,.45);z-index:3}
 .legend{color:#7d8794;font-size:12px;margin-top:8px;max-width:520px}
 .info{max-width:340px}
 table{border-collapse:collapse;font:12px ui-monospace,monospace}
 td{padding:2px 8px 2px 0}
</style>
<h1>Heatmap real</h1>
<div class=sub>A moldura AZUL e' o lance que a busca escolhe, e aparece assim que carregas a posicao.
 Clica numa peca para ver quanto vale cada lance legal dela -- o motor joga-o e avalia a posicao que
 resulta, com mobilidade, ameacas, seguranca do rei e tropismo. Verde vale mais, x marca captura.
 Clica outra vez na mesma peca para desligar o mapa.</div>
<div class=row>
 <select id=preset onchange="usePreset()"><option value="">-- posicoes conhecidas --</option>%%POS%%</select>
</div>
<div class=row>
 <input type=text id=fen value="r1bq1rk1/pp2bppp/2n1pn2/2pp4/3P1B2/2PBPN2/PP1N1PPP/R2Q1RK1 w - - 0 9">
 <button onclick="load()">Carregar</button>
</div>
<div id=tmpanel style="margin:8px 0;padding:9px 11px;border:1px solid #2a2f38;border-radius:7px;
  background:#1b1e24;font:12px ui-monospace,monospace;color:#9aa4b0;display:none"></div>
<div class=row>
 <label>busca <input type=text id=depth value="400" style="width:56px"> ms</label>
 <label>ou RELOGIO <input type=text id=clock value="" placeholder="60000" style="width:76px"> ms</label>
 <label>adversario <input type=text id=oppclock value="" placeholder="igual" style="width:70px"> ms</label>
 <label>incremento <input type=text id=inc value="0" style="width:52px"> ms</label>
 <button onclick="showBest()">so a melhor jogada</button>
</div>
<div class=wrap>
 <div><div class=board id=board></div><div class=legend id=leg></div></div>
 <div class=info><table id=tbl></table></div>
</div>
<script>
const GLYPH={P:'♙',N:'♘',B:'♗',R:'♖',Q:'♕',K:'♔',
             p:'♟',n:'♞',b:'♝',r:'♜',q:'♛',k:'♚'};
let board={}, sel=null, cells=null, caps=null, best=null;

function parse(fen){
  const b={}, rows=fen.trim().split(' ')[0].split('/');
  rows.forEach((row,r)=>{let f=0; for(const ch of row){
    if(ch>='1'&&ch<='8') f+=+ch; else {b[(7-r)*8+f]=ch; f++;}}});
  return b;
}
function usePreset(){
  const v=document.getElementById('preset').value;
  if(v){ document.getElementById('fen').value=v; load(); }
}
function opts(){
  const g=id=>document.getElementById(id);
  return {fen:g('fen').value,
          depth:(g('depth').value===''?400:+g('depth').value),
          clock:+g('clock').value||0,
          oppclock:+g('oppclock').value||0,
          inc:+g('inc').value||0};
}
function load(){
  board=parse(document.getElementById('fen').value);
  sel=null; cells=null; caps=null; best=null; draw(); showBest();
}
async function showBest(){
  const leg=document.getElementById('leg');
  leg.textContent='a procurar a melhor jogada...';
  try{
    const o=opts();
    const r=await fetch('/heat',{method:'POST',body:JSON.stringify(
      {fen:o.fen,square:0,only_best:true,depth:o.depth,clock:o.clock,
       oppclock:o.oppclock,inc:o.inc})});
    const j=await r.json();
    if(j.best_to===undefined||j.best_to===null){ leg.textContent=j.error||'sem lance'; return; }
    best={from:j.best_from,to:j.best_to,san:j.best_san}; draw();
    const sc=(j.score>0?'+':'')+((j.score||0)/100).toFixed(2);
    let extra='';
    if(j.clock){ extra=' | relogio '+(j.clock/1000).toFixed(0)+'s -> gastou '+j.spent+'ms ('+
                       (100*j.spent/j.clock).toFixed(1)+'%) ate a profundidade '+j.depth; }
    else if(j.depth){ extra=' | profundidade '+j.depth; }
    leg.textContent='a busca joga '+j.best_san+' ('+sc+')'+extra;
    // What the clock decided, and on what grounds. A move played in 1s tells
    // you nothing about whether the engine weighed spending more; this does.
    const tp=document.getElementById('tmpanel');
    if(j.tm){
      const t=j.tm, used=j.spent, pct=(100*used/t.soft).toFixed(0);
      const gap=t.oppclock? (t.myclock/t.oppclock) : 1;
      const gapTxt = gap>1.2 ? 'a frente no relogio (+'+((gap-1)*100).toFixed(0)+'%), tecto alargado'
                   : gap<0.83 ? 'atras no relogio (-'+((1-gap)*100).toFixed(0)+'%), tecto cortado'
                   : 'relogios equilibrados';
      const mode = t.hard===t.soft ? '<b style="color:#f0883e">metralhadora</b> (sem tecto: pouco relogio)'
                                   : 'normal (pode esticar ate '+(t.hard/t.soft).toFixed(1)+'x)';
      tp.style.display='block';
      tp.innerHTML='<b style="color:#d8dde3">gestao de tempo</b> &nbsp; modo '+mode+
        '<br>orcamento <b style="color:#6db3f2">'+t.soft+'ms</b>'+
        ' &nbsp; tecto <b style="color:#6db3f2">'+t.hard+'ms</b>'+
        ' &nbsp; gastou <b style="color:'+(used>t.hard?'#f0883e':'#7ee787')+'">'+used+'ms</b> ('+pct+'% do orcamento)'+
        '<br>faltam ~<b>'+t.horizon+'</b> lances (estimado por '+t.pieces+' pecas no tabuleiro)'+
        ' &nbsp;|&nbsp; '+gapTxt;
    } else { tp.style.display='none'; }
  }catch(e){ leg.textContent='erro: '+e; }
}
function draw(){
  const el=document.getElementById('board'); el.innerHTML='';
  let min=0,max=0;
  if(cells){ const v=Object.values(cells); min=Math.min(...v); max=Math.max(...v); }
  for(let r=7;r>=0;r--) for(let f=0;f<8;f++){
    const sq=r*8+f, d=document.createElement('div'); d.className='sq';
    let bg=((r+f)%2===1)?'#3a4048':'#2b3038';
    if(cells && cells[sq]!==undefined){
      const v=cells[sq], rng=Math.max(Math.abs(min),Math.abs(max))||1, n=v/rng;
      bg = n>=0 ? 'rgb('+Math.round(40+30*(1-n))+','+Math.round(80+150*n)+',60)'
                : 'rgb('+Math.round(80+150*-n)+','+Math.round(50+20*(1+n))+',55)';
    }
    d.style.background=bg;
    if(sq===sel) d.classList.add('sel');
    if(best){ if(sq===best.to) d.classList.add('best');
              else if(sq===best.from) d.classList.add('bestfrom'); }
    const p=board[sq];
    if(p){ const g=document.createElement('div'); g.className='pc'; g.textContent=GLYPH[p];
           g.style.color=(p===p.toUpperCase())?'#f5f5f5':'#12141a'; d.appendChild(g); }
    if(cells && cells[sq]!==undefined){ const v=document.createElement('div'); v.className='v';
      v.textContent=(cells[sq]>0?'+':'')+cells[sq]+((caps&&caps[sq])?'x':''); d.appendChild(v); }
    d.onclick=()=>pick(sq);
    el.appendChild(d);
  }
}
async function pick(sq){
  if(!board[sq]) return;
  if(sq===sel){ sel=null; cells=null; caps=null; draw();
    document.getElementById('tbl').innerHTML=''; showBest(); return; }
  sel=sq; cells=null; caps=null; draw();
  const leg=document.getElementById('leg'); leg.textContent='a calcular...';
  try{
    const o=opts();
    const r=await fetch('/heat',{method:'POST',body:JSON.stringify(
      {fen:o.fen,square:sq,depth:o.depth,clock:o.clock,inc:o.inc})});
    const j=await r.json();
    if(j.error){ leg.textContent=j.error; sel=null; draw(); return; }
    cells=j.cells; caps=j.captures;
    if(j.best_to!==undefined&&j.best_to!==null) best={from:j.best_from,to:j.best_to,san:j.best_san};
    draw();
    const nm=s=>'abcdefgh'[s%8]+(1+Math.floor(s/8));
    const v=Object.entries(cells).map(([k,x])=>[+k,x]).sort((a,b)=>b[1]-a[1]);
    leg.textContent=j.piece+' em '+nm(j.from)+' | avaliacao '+(j.base>0?'+':'')+j.base+'cp'+
      (best?' | a busca joga '+best.san:'');
    const t=document.getElementById('tbl'); t.innerHTML='';
    const add=(a,b)=>{const tr=t.insertRow(); tr.insertCell().textContent=a; tr.insertCell().textContent=b;};
    v.forEach(([s,x])=>add(nm(s)+((caps&&caps[s])?' x':''), (x>0?'+':'')+x));
  }catch(e){ leg.textContent='erro: '+e; }
}
load();
</script>
"""

PRESETS = [
    ("posicao inicial", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
    ("italiana -- Bxf7+ possivel", "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4"),
    ("meio-jogo calmo", "r1bq1rk1/pp2bppp/2n1pn2/2pp4/3P1B2/2PBPN2/PP1N1PPP/R2Q1RK1 w - - 0 9"),
    ("Kiwipete (taticas)", "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1"),
    ("Deep Blue - Kasparov 1997, antes de Re1", "r2k1b1r/pb1nq1p1/2p1pnBp/1p6/P2P1B2/5N2/1PP2PPP/R2Q1RK1 w - - 1 13"),
    ("Morphy - Opera, antes de Qb8+", "1n1Rkb1r/p4ppp/4q3/4p1B1/4P3/8/PPP2PPP/2K5 w k - 1 17"),
    ("Fine #70 -- precisa de profundidade", "8/k7/3p4/p2P1p2/P2P1P2/8/8/K7 w - - 0 1"),
    ("estrutura travada", "rnbqkb1r/pp3ppp/4pn2/2ppP3/3P4/2P2N2/PP3PPP/RNBQKB1R w KQkq - 0 6"),
    ("final de torres", "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1"),
    ("bispos de cores opostas", "8/3k4/2p1b3/8/8/4B3/4K3/8 w - - 0 1"),
]

PAGE = PAGE_TEMPLATE.replace(
    "%%POS%%",
    "".join('<option value="{}">{}</option>'.format(f, n) for n, f in PRESETS),
)



class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.end_headers()
        self.wfile.write(PAGE.encode())

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        try:
            req = json.loads(self.rfile.read(n))
            with LOCK:
                if req.get("only_best"):
                    out = best_move(req["fen"], int(req.get("depth", SEARCH_MS)),
                                    int(req.get("clock") or 0) or None,
                                    int(req.get("inc") or 0),
                                    int(req.get("oppclock") or 0) or None)
                else:
                    out = heatmap(req["fen"], int(req["square"]),
                                  int(req.get("depth", SEARCH_MS)),
                                  int(req.get("clock") or 0) or None,
                                  int(req.get("inc") or 0))
        except Exception as e:
            out = {"error": str(e)[:120]}
            try:
                if ENG.p:
                    ENG.p.kill()
                ENG.p = None
            except Exception:
                pass
        body = json.dumps(out).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


socketserver.TCPServer.allow_reuse_address = True
with socketserver.TCPServer(("0.0.0.0", PORT), Handler) as srv:
    print(f"heatmap real em http://0.0.0.0:{PORT}", flush=True)
    srv.serve_forever()
