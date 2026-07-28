"""The mixing desk: move every channel by hand and hear what changes.

Before optimising anything, move the faders and listen. Our engine opens e2e3
where a strong reference plays e2e4 -- which channel is responsible? No
automated search answers that, because it never asks. Moving a slider and
watching the move change does.

Three groups of faders:
  * evaluation families -- mobility, king safety, threats, pawns, pieces, tempo
  * piece-square tables, one per piece
  * material values, one per piece and phase

The last two matter because the PSQTs were adopted as generic public tables and
never calibrated here. Measured against a strong reference, ours are about
twice as loud for rooks and pawns in the middlegame and HALF as loud for the
king in both phases -- and the king's table is what says "shelter now,
centralise later".

Positions come from the real blunder suite, with the move that should have been
played, so the question is never abstract. Two references answer alongside us,
because they disagree with each other often enough that agreeing with only one
proves nothing.
"""
import http.server, json, socketserver, subprocess, threading

PORT = 8769
ENGINE = "/root/kestrel_joao/kestrel_psqt"
REFS = [("stockfish", "/usr/local/bin/stockfish"),
        ("sirius", "/root/hce_8buckets_experiment/sirius")]
SUITE = "/root/kestrel_joao/blunders_big.epd"
FAMILIES = ["mobility", "king", "threats", "pawns", "pieces", "tempo"]
PIECES = ["pawn", "knight", "bishop", "rook", "queen", "king"]
LOCK = threading.Lock()


def load_suite(limit=260):
    out = []
    try:
        for line in open(SUITE):
            line = line.strip()
            if not line:
                continue
            parts = [p.strip() for p in line.split('|')]
            fen = parts[0]
            played = best = ""
            for p in parts[1:]:
                if p.startswith("played "):
                    played = p.split()[1]
                elif p.startswith("best "):
                    best = p.split()[1]
            out.append({"fen": fen, "played": played, "best": best})
            if len(out) >= limit:
                break
    except OSError:
        pass
    return out


SUITE_POS = load_suite()

PAGE = r"""<!doctype html><meta charset=utf-8>
<title>Kestrel - mesa de mistura</title>
<style>
 body{background:#14161a;color:#d8dde3;font:13px/1.5 -apple-system,Segoe UI,Roboto,sans-serif;margin:0;padding:18px}
 h1{font-size:17px;font-weight:600;margin:0 0 3px}
 .sub{color:#7d8794;font-size:12px;margin-bottom:16px}
 .wrap{display:grid;grid-template-columns:1fr 1fr 1fr;gap:16px;max-width:1500px}
 .panel{background:#1b1e24;border:1px solid #2a2f38;border-radius:8px;padding:14px}
 .panel h2{font-size:12px;text-transform:uppercase;letter-spacing:.5px;color:#7d8794;margin:0 0 10px}
 .fader{margin:9px 0}
 .fader label{display:flex;justify-content:space-between;font-size:12px;margin-bottom:3px}
 .fader .v{color:#6db3f2;font-variant-numeric:tabular-nums}
 .fader .hint{color:#5a626d;font-size:10px}
 input[type=range]{width:100%;accent-color:#6db3f2;margin:0}
 select,input[type=text]{width:100%;background:#0f1115;border:1px solid #2a2f38;color:#d8dde3;
   border-radius:5px;padding:6px 8px;font:11px ui-monospace,monospace;margin-bottom:8px}
 button{background:#2b6cb0;color:#fff;border:0;border-radius:5px;padding:6px 12px;cursor:pointer;font-size:12px}
 button.ghost{background:#2a2f38}
 .move{font:600 26px ui-monospace,monospace;color:#7ee787;margin:4px 0}
 .move.bad{color:#f0883e}
 .row{display:flex;gap:6px;margin-bottom:8px;flex-wrap:wrap}
 table{width:100%;border-collapse:collapse;font:11px ui-monospace,monospace;margin-top:8px}
 th{text-align:left;color:#7d8794;font-weight:400;padding:2px 0;border-bottom:1px solid #2a2f38}
 td{padding:3px 0;border-bottom:1px solid #23272f}
 td.n{text-align:right;color:#7d8794}
 .ok{color:#7ee787} .no{color:#f0883e}
 .meta{color:#7d8794;font-size:11px}
</style>
<h1>Mesa de mistura</h1>
<div class=sub>Mexe um canal e ve o que muda. 1000 = como esta hoje. As posicoes vem da suite de erros reais,
 com o lance que devia ter sido jogado.</div>
<div class=wrap>

 <div class=panel>
  <h2>posicao</h2>
  <select id=sel onchange="pick()"></select>
  <input type=text id=fen>
  <div class=row>
   <button onclick="probe()">Analisar</button>
   <button class=ghost onclick="setAll()">repor faders</button>
   <label class=meta>prof <input type=text id=depth value=12 style="width:38px;display:inline;padding:2px 4px"></label>
  </div>
  <div class=meta id=expect></div>
  <table id=cmp><tr><th>motor</th><th>lance</th><th class=n>aval</th></tr></table>
 </div>

 <div class=panel>
  <h2>familias de avaliacao</h2>
  <div id=fam></div>
  <h2 style="margin-top:16px">tabelas psqt</h2>
  <div id=psqt></div>
 </div>

 <div class=panel>
  <h2>valor das pecas</h2>
  <div id=mat></div>
 </div>
</div>
<script>
const FAM=%%FAM%%, PIECES=%%PIECES%%, POS=%%POS%%;
const HINT={king:"as nossas psqt do rei tem METADE da amplitude da referencia",
            rook:"as nossas psqt da torre tem o DOBRO no meio-jogo",
            pawn:"as nossas psqt do peao tem quase o dobro no meio-jogo"};
let state={};
function mk(host, key, label, hint){
  state[key]=1000;
  const d=document.createElement('div'); d.className='fader';
  d.innerHTML=`<label><span>${label}${hint?` <span class=hint>${hint}</span>`:''}</span>
    <span class=v id=v_${key}>1000</span></label>
    <input type=range min=300 max=2200 step=10 value=1000 id=s_${key}>`;
  host.appendChild(d);
  const r=d.querySelector('input');
  r.addEventListener('input',e=>{state[key]=+e.target.value;
    document.getElementById('v_'+key).textContent=state[key];});
  r.addEventListener('change',probe);
}
FAM.forEach(f=>mk(document.getElementById('fam'),'scale_'+f,f));
PIECES.forEach(p=>mk(document.getElementById('psqt'),'psqt_'+p,p,HINT[p]||''));
PIECES.filter(p=>p!=='king').forEach(p=>{
  mk(document.getElementById('mat'),'mg_'+p,'mg '+p);
  mk(document.getElementById('mat'),'eg_'+p,'eg '+p);
});
const sel=document.getElementById('sel');
sel.innerHTML='<option value=-1>-- posicao inicial --</option>'+
  POS.map((p,i)=>`<option value=${i}>${i+1}. ${p.fen.split(' ')[0].slice(0,26)}  (certo: ${p.best})</option>`).join('');
function pick(){
  const i=+sel.value;
  document.getElementById('fen').value = i<0 ? 'startpos' : POS[i].fen;
  document.getElementById('expect').textContent = i<0 ? '' :
    `devia jogar ${POS[i].best}   |   jogou ${POS[i].played} no jogo real`;
  probe();
}
function setAll(){ Object.keys(state).forEach(k=>{state[k]=1000;
  document.getElementById('s_'+k).value=1000; document.getElementById('v_'+k).textContent=1000;}); probe(); }
async function probe(){
  const t=document.getElementById('cmp');
  t.innerHTML='<tr><th>motor</th><th>lance</th><th class=n>aval</th></tr><tr><td>...</td><td></td><td></td></tr>';
  const i=+sel.value;
  const r=await fetch('/probe',{method:'POST',body:JSON.stringify({
    fen:document.getElementById('fen').value, depth:+document.getElementById('depth').value||12,
    faders:state, want: i<0?'':POS[i].best})});
  const j=await r.json();
  t.innerHTML='<tr><th>motor</th><th>lance</th><th class=n>aval</th></tr>';
  j.engines.forEach(e=>{
    const tr=t.insertRow();
    const cls = j.want ? (e.move===j.want?'ok':'no') : '';
    tr.insertCell().textContent=e.name;
    const c=tr.insertCell(); c.textContent=e.move; c.className=cls;
    tr.insertCell().className='n';
    tr.cells[2].textContent=(e.score>0?'+':'')+(e.score/100).toFixed(2);
  });
}
pick();
</script>
""".replace("%%FAM%%", json.dumps(FAMILIES)).replace("%%PIECES%%", json.dumps(PIECES)).replace(
    "%%POS%%", json.dumps(SUITE_POS))


def run_uci(path, fen, depth, extra=()):
    cmds = ["uci", "setoption name Threads value 1", "setoption name Hash value 64"]
    cmds += list(extra)
    cmds.append("isready")
    cmds.append("position startpos" if fen.strip() in ("", "startpos") else f"position fen {fen}")
    cmds += [f"go depth {int(depth)}", "quit"]
    try:
        p = subprocess.run([path], input="\n".join(cmds) + "\n",
                           capture_output=True, text=True, timeout=90)
    except Exception:
        return {"move": "-", "score": 0}
    move, score = "-", 0
    for line in p.stdout.splitlines():
        if " score cp " in line:
            try:
                score = int(line.split(" score cp ")[1].split()[0])
            except (IndexError, ValueError):
                pass
        elif " score mate " in line:
            try:
                n = int(line.split(" score mate ")[1].split()[0])
                score = (30000 - abs(n) * 100) * (1 if n > 0 else -1)
            except (IndexError, ValueError):
                pass
        if line.startswith("bestmove"):
            move = line.split()[1]
    return {"move": move, "score": score}


def probe(fen, depth, faders, want):
    # A fresh process per probe: the fader values sit behind locks that are
    # sealed on first evaluation, so a reused process would answer every later
    # request with the first request's settings.
    extra = [f"setoption name {k} value {int(v)}" for k, v in faders.items() if int(v) != 1000]
    out = [dict(name="kestrel", **run_uci(ENGINE, fen, depth, extra))]
    for name, path in REFS:
        out.append(dict(name=name, **run_uci(path, fen, depth)))
    return {"engines": out, "want": want}


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
                out = probe(req.get("fen", "startpos"), req.get("depth", 12),
                            req.get("faders", {}), req.get("want", ""))
        except Exception as e:
            out = {"engines": [{"name": "erro", "move": str(e)[:40], "score": 0}], "want": ""}
        body = json.dumps(out).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


socketserver.TCPServer.allow_reuse_address = True
with socketserver.TCPServer(("0.0.0.0", PORT), Handler) as srv:
    print(f"mesa em http://0.0.0.0:{PORT}  ({len(SUITE_POS)} posicoes carregadas)", flush=True)
    srv.serve_forever()
