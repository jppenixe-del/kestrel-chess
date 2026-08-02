"""Fitting evaluation weights to game results, on the GPU.

The engine already writes the hard part: `kestrel gpuextract <dataset> <out.bin>
<max> <buckets> <threads>` decomposes each position into the linear
contributions of every tunable weight, which is minutes of CPU work per pass.
What was missing was the other half -- something to fit those features -- and
fitting them is a sparse logistic regression, which is what a GPU does in
milliseconds what a CPU does in minutes.

That difference is not convenience. It is what makes per-bucket tuning possible
at all: four buckets means four times the parameters on the same positions, and
the answer only becomes trustworthy after many more epochs than anyone wants to
sit through.

Record format written by gpuextract, little-endian, no header:

    u16          number of features
    n x (u16,f32) feature index, value
    f32          phase (1.0 = opening, 0.0 = endgame)
    f32          result (1.0 win, 0.5 draw, 0.0 loss, from White)

Feature indices already carry the bucket offset, so a bucketed file is fitted
exactly like a single-bucket one -- the buckets never interact and the same
matrix multiply handles all of them.

Usage:
    python3 gpu_tune.py feats.bin out.txt [epochs] [K] [lr] [l2] [minutes]

`out.txt` is the comma-separated vector the engine reads back through
KESTREL_BUCKET_WEIGHTS. It is written EVERY time the held-out loss improves,
not once at the end: a run that is interrupted -- and this one runs on a
machine that gets switched off -- must leave behind the best weights it had
rather than nothing at all. A first run wrote nothing for two hours and would
have lost all of it.

`minutes` is a wall-clock budget. The run stops cleanly when it expires.
"""
import os
import sys
import time

import numpy as np
import torch


def parse(path):
    """Read the whole file into per-record arrays, and cache the result.

    Two passes. The first walks the records to collect their offsets and
    lengths -- no numpy per position -- and the second lifts the feature bytes
    out in one go. The obvious version, which builds a small array per
    position, makes three numpy calls per record: eight million of them on
    this dataset, and it spent ten minutes without finishing.

    The second pass masks the bytes it does NOT want rather than listing the
    ones it does. Listing them means an index per byte, and there are 1.2
    billion feature bytes here: 9.6GB of 64-bit indices to extract 1.2GB of
    data, which took the machine to zero free memory and had to be killed. The
    headers and tails being masked out number 26 million, and a boolean mask
    over the buffer costs one byte each.

    Even done well this costs minutes, which is paid again on every restart.
    The result is cached beside the input, and the cache is what makes it
    affordable to stop a run and start another with different settings.
    """
    cache = path + ".npz"
    if os.path.exists(cache) and os.path.getmtime(cache) > os.path.getmtime(path):
        z = np.load(cache)
        print(f"  cache: {cache}", flush=True)
        return z["counts"], z["cols"], z["vals"], z["results"]

    raw = np.fromfile(path, dtype=np.uint8)
    total = raw.size

    counts, starts, tails = [], [], []
    off = 0
    while off + 2 <= total:
        n = int.from_bytes(raw[off:off + 2].tobytes(), "little")
        off += 2
        end = off + n * 6
        if end + 8 > total:
            break
        counts.append(n)
        starts.append(off)
        tails.append(end)
        off = end + 8
    n_pos = len(counts)
    counts = np.asarray(counts, dtype=np.int64)
    print(f"  {n_pos} posicoes lidas, a montar as matrizes...", flush=True)

    starts = np.asarray(starts, dtype=np.int64)
    tails = np.asarray(tails, dtype=np.int64)

    res_idx = (tails[:, None] + 4 + np.arange(4)).ravel()
    results = np.frombuffer(raw[res_idx].tobytes(), dtype="<f4").astype(np.float32)

    keep = np.ones(total, dtype=bool)
    keep[(starts[:, None] - 2 + np.arange(2)).ravel()] = False
    keep[(tails[:, None] + np.arange(8)).ravel()] = False
    keep[max(0, off - 2):] = False
    pairs = raw[keep].view(np.dtype([("i", "<u2"), ("v", "<f4")]))
    cols = pairs["i"].copy()
    vals = pairs["v"].astype(np.float32)

    del raw, keep, pairs
    np.savez(cache, counts=counts, cols=cols, vals=vals, results=results)
    return counts, cols, vals, results


def csr(crow, col, val, height, width, dev):
    """Build a CSR matrix, with both index arrays in the SAME dtype.

    They must match, and nothing says so out loud: a matrix built with 64-bit
    row pointers and 32-bit column indices constructs without complaint, and
    then the first product on the GPU dies with an illegal memory access. int32
    is the right width here -- the largest index is 200 million -- and it also
    halves what the indices cost in GPU memory.
    """
    idx = torch.int32
    m = torch.sparse_csr_tensor(
        torch.from_numpy(crow).to(idx).to(dev), torch.from_numpy(col).to(idx).to(dev),
        torch.from_numpy(val).to(dev), (height, width))
    assert m.crow_indices().dtype == m.col_indices().dtype
    return m


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return
    argv = sys.argv[:]

    def flag(name, default=None):
        if name in argv:
            i = argv.index(name)
            v = argv[i + 1]
            del argv[i:i + 2]
            return v
        return default

    init_path = flag("--init")
    stride = int(flag("--stride", 0))
    free = int(flag("--free", 0))

    sys.argv = argv
    feats_path, out_path = argv[1], argv[2]
    epochs = int(argv[3]) if len(argv) > 3 else 4000
    # Scaling constant of the logistic. 1/400 is the usual starting point; it
    # is not a free parameter to taste, it sets what "one pawn" means in the
    # probability the fit is matching, so changing it changes every weight.
    K = float(argv[4]) if len(argv) > 4 else 1.0 / 400.0
    lr = float(argv[5]) if len(argv) > 5 else 1.0
    l2 = float(argv[6]) if len(argv) > 6 else 0.0
    budget = float(argv[7]) * 60 if len(argv) > 7 else float("inf")

    dev = "cuda" if torch.cuda.is_available() else "cpu"
    print(f"device: {dev}" + (f" ({torch.cuda.get_device_name(0)})" if dev == "cuda" else ""))

    t0 = time.time()
    counts, cols, vals, results = parse(feats_path)
    n_pos = len(counts)
    # O tamanho vem do maior indice VISTO nos dados, e isso engana-se quando o
    # dataset nao cobre todos os buckets: um conjunto so' de finais ocupa 5 dos
    # 8 buckets (os buckets sao por contagem de peoes e num final ha poucos),
    # e o vector de pesos saia 7430 em vez de 11888 -- incompativel com o
    # ficheiro de arranque e com o que o motor le.
    n_par = int(cols.max()) + 1
    npar_forcado = int(flag("--npar", 0))
    if npar_forcado:
        if npar_forcado < n_par:
            raise SystemExit(f"--npar {npar_forcado} e' menor que o maior indice nos dados ({n_par})")
        n_par = npar_forcado
    nnz = len(vals)
    print(f"{n_pos} positions, {n_par} parameters, {nnz} non-zeros "
          f"({nnz / max(1, n_pos):.0f} per position), read in {time.time() - t0:.1f}s")

    # A validation split, held out and never trained on.
    #
    # Without it there is no way to tell fitting from memorising, and the
    # difference is not academic: 5000 epochs on 20k positions against 5824
    # parameters drove the loss from 0.223 to 0.0119 and it was still falling.
    # That is not a model of chess, it is a lookup table for those positions.
    # Split by POSITION INDEX rather than at random per non-zero, so a
    # position's features never straddle the two sets.
    n_val = max(1, n_pos // 10)
    n_tr = n_pos - n_val
    crow = np.zeros(n_pos + 1, dtype=np.int64)
    np.cumsum(counts, out=crow[1:])
    cut = int(crow[n_tr])

    # CSR, not COO, and the transpose built by hand.
    #
    # COO costs a sort on every product and carries 64-bit indices for both
    # coordinates: the first version of this ran at 15 seconds an epoch, which
    # is sixteen hours for the run it was asked to do. The records already
    # arrive grouped by position, so the row pointers are just a running total
    # of the feature counts -- no sort, no coalesce.
    #
    # The gradient needs X transposed. Autograd would transpose it on every
    # backward pass; there are only 11888 columns, so a counting sort builds it
    # once and it is reused for every epoch after.
    Xcrow, Xcol, Xval = crow[: n_tr + 1], cols[:cut], vals[:cut]
    Vcrow = (crow[n_tr:] - crow[n_tr]).copy()
    X = csr(Xcrow.astype(np.int64), Xcol.astype(np.int32), Xval, n_tr, n_par, dev)
    Xv = csr(Vcrow, cols[cut:].astype(np.int32), vals[cut:], n_val, n_par, dev)

    order = np.argsort(Xcol, kind="stable")
    Tcrow = np.zeros(n_par + 1, dtype=np.int64)
    np.cumsum(np.bincount(Xcol.astype(np.int64), minlength=n_par), out=Tcrow[1:])
    rows = np.repeat(np.arange(n_tr, dtype=np.int32), counts[:n_tr])
    Xt = csr(Tcrow, rows[order].astype(np.int32), Xval[order], n_par, n_tr, dev)
    del order, rows

    # Check both matrices before spending hours on them. A malformed sparse
    # tensor does not raise: it either dies with an illegal memory access
    # partway through -- which is how the index dtype above was found -- or,
    # worse, returns numbers. Ten random positions, evaluated against a random
    # weight vector, compared with the same sum done in numpy from the raw
    # arrays.
    probe = np.random.default_rng(0).standard_normal(n_par).astype(np.float32)
    pw = torch.from_numpy(probe).to(dev)
    for name, M, base, off in (("X", X, 0, 0), ("Xv", Xv, n_tr, cut)):
        got = torch.mv(M, pw).cpu().numpy()
        for r in np.random.default_rng(1).integers(0, M.shape[0], 10):
            s = int(crow[base + r]) - off
            e = s + int(counts[base + r])
            want = float(np.dot(vals[off + s:off + e].astype(np.float64),
                                probe[cols[off + s:off + e].astype(np.int64)]))
            assert abs(want - got[r]) < 1e-2 * max(1.0, abs(want)), \
                f"{name} linha {r}: {got[r]} != {want}"
    # The transpose has to agree with X, not merely be well formed: x.Xt(d)
    # is the gradient, and a wrong one trains happily to the wrong answer.
    d = torch.from_numpy(np.random.default_rng(2).standard_normal(n_tr).astype(np.float32)).to(dev)
    lhs = torch.dot(torch.mv(X, pw), d).item()
    rhs = torch.dot(pw, torch.mv(Xt, d)).item()
    assert abs(lhs - rhs) < 1e-3 * max(1.0, abs(lhs)), f"Xt nao e' a transposta: {lhs} != {rhs}"
    print(f"  matrizes verificadas (X, Xv, e Xt contra X)", flush=True)

    y = torch.from_numpy(results[:n_tr]).to(dev)
    yv = torch.from_numpy(results[n_tr:]).to(dev)
    print(f"treino {n_tr}, validacao {n_val} (nunca treinada)", flush=True)

    # Where the fit starts, and what it is allowed to move.
    #
    # Starting from zeros makes the fit rediscover chess from scratch, and it
    # does so badly where two feature families explain the same thing: the
    # first run put a pawn at 330 in the opening and 69 in the endgame, which
    # is Material and PSQT collinear, splitting their shared explanation
    # wherever the optimiser happened to land. Measured on the blunder suite
    # against the weights it replaced: 78 positions fixed became 62.
    #
    # Started from `kestrel tunestart`, epoch zero IS the current engine. A fit
    # that finds nothing changes nothing, and everything it does change, it
    # changed for a reason in the data.
    #
    # `--free N` trains only the first N of every `--stride` parameters. The
    # material and piece-square block cannot be installed per bucket anyway --
    # the incremental accumulator has one set of tables and knows nothing about
    # the pawn count -- so letting the fit move it produces weights balanced
    # against material the engine will not be using. Frozen, the positional
    # weights are fitted in the presence of the real thing.
    if init_path:
        init = np.asarray([int(x) for x in open(init_path).read().strip().split(",")],
                          dtype=np.float32)
        assert len(init) == n_par, f"--init tem {len(init)} valores, esperava {n_par}"
        w = torch.from_numpy(init.copy()).to(dev)
        print(f"  arranque em {init_path}", flush=True)
    else:
        w = torch.zeros(n_par, device=dev)

    trainable = None
    # `--faixas a-b,c-d` treina apenas esses deslocamentos dentro de cada
    # `--stride`, e serve para o que `--free N` nao consegue exprimir: treinar
    # os posicionais E as PSQT deixando o MATERIAL congelado no meio.
    #
    # O layout de cada bucket e' 705 posicionais | 12 de material | 768 de
    # PSQT | 1 de bias. Tunar o material junto com o resto e' o erro classico:
    # os valores das pecas absorvem qualquer erro de escala do resto e a
    # avaliacao inteira desloca-se, o que deixa as margens da busca -- que nao
    # sao reajustadas -- calibradas para outro motor.
    ancora = float(flag("--ancora", "0"))
    ancora_par = float(flag("--ancora-par", "0"))
    _pf = flag("--fixa-peao", "")
    peao_fixo = tuple(float(x) for x in _pf.split(",")) if _pf else None
    faixas = flag("--faixas", "")
    if stride and faixas:
        m = np.zeros(n_par, dtype=bool)
        off = np.arange(n_par) % stride
        for parte in faixas.split(","):
            a, b = parte.split("-")
            m |= (off >= int(a)) & (off <= int(b))
        trainable = torch.from_numpy(m.astype(np.float32)).to(dev)
        print(f"  a treinar {int(m.sum())} de {n_par} pesos (faixas {faixas} de cada {stride})",
              flush=True)
    elif stride and free:
        m = (np.arange(n_par) % stride) < free
        trainable = torch.from_numpy(m.astype(np.float32)).to(dev)
        print(f"  a treinar {int(m.sum())} de {n_par} pesos "
              f"(os primeiros {free} de cada {stride}); os restantes ficam como estao",
              flush=True)

    # --- Ancora por sinal observado ---------------------------------------
    #
    # Um peso so' aprende na medida em que os dados o LEEM. Com avaliacao
    # interpolada, o gradiente de um peso de final e' multiplicado por
    # (1 - fase); num bucket cujas posicoes sao quase todas de abertura, esse
    # peso quase nao recebe sinal nenhum. Medido nestes dados: o bucket dos
    # 14+ peoes tem 21,21% das posicoes em fase de abertura e 0,10% em fase de
    # final -- 212 vezes menos. Os pesos de final desse bucket sao treinados
    # com ~0,3% dos dados e depois lidos a 100% na posicao rara que tenha
    # muitos peoes e nenhuma peca. Foi assim que o tempo de final ficou em -5
    # e uma posicao perfeitamente simetrica passou a valer -108.
    #
    # A correccao nao e' inventar dados nem amarrar buckets uns aos outros: e'
    # deixar quieto o que os dados nao veem. O suporte de cada peso e' a soma
    # dos valores absolutos da sua coluna -- que ja' existe, e' a linha
    # correspondente da transposta. Abaixo de uma fraccao da mediana, o peso
    # fica onde estava.
    # --- Ancora POR PAR mg/eg (a que corresponde ao mecanismo) -----------
    #
    # Um limiar global nao serve: medido nestes dados, TODOS os pesos em causa
    # estao acima da mediana global (24x a 443x). O que os distingue e' o
    # racio dentro do proprio par: no bucket dos poucos peoes o lado de
    # MEIO-JOGO recebe 1/5 do que o de final recebe; no bucket dos muitos
    # peoes e' o lado de FINAL que recebe 1/13. E' sempre o lado cuja fase e'
    # rara naquele bucket.
    #
    # Pares no layout de 1486: o bloco posicional usa pair!/pairs!, portanto
    # mg e eg sao ADJACENTES (i, i+1); o material tem os 6 mg seguidos dos 6
    # eg (desvio 6); a PSQT tem as 6 tabelas mg seguidas das 6 eg (desvio 384).
    if ancora_par > 0.0:
        vt2 = np.abs(Xval[np.argsort(Xcol, kind="stable")])
        sup2 = np.add.reduceat(vt2, Tcrow[:-1].astype(np.int64))
        sup2[np.diff(Tcrow) == 0] = 0.0
        parceiro = np.arange(n_par)
        for bk in range(n_par // stride):
            b = bk * stride
            for i in range(0, 705, 2):
                parceiro[b + i], parceiro[b + i + 1] = b + i + 1, b + i
            for p_ in range(6):
                parceiro[b + 705 + p_], parceiro[b + 711 + p_] = b + 711 + p_, b + 705 + p_
            for k in range(384):
                parceiro[b + 717 + k], parceiro[b + 717 + 384 + k] = b + 717 + 384 + k, b + 717 + k
        sup_par = sup2[parceiro]
        cego = (sup2 < ancora_par * sup_par) & (sup_par > 0)
        m_par = ~cego
        if trainable is None:
            trainable = torch.from_numpy(m_par.astype(np.float32)).to(dev)
        else:
            trainable = trainable * torch.from_numpy(m_par.astype(np.float32)).to(dev)
        print(f"  ancora-par: {int(cego.sum())} de {n_par} pesos ficam quietos "
              f"(suporte < {ancora_par:g} x o do seu par mg/eg)", flush=True)

    if ancora > 0.0:
        vt = np.abs(Xval[np.argsort(Xcol, kind="stable")])
        sup = np.add.reduceat(vt, Tcrow[:-1].astype(np.int64))
        vazios = np.diff(Tcrow) == 0
        sup[vazios] = 0.0
        med = float(np.median(sup[sup > 0])) if (sup > 0).any() else 0.0
        m_anc = sup >= ancora * med
        n_fixos = int((~m_anc).sum())
        if trainable is None:
            trainable = torch.from_numpy(m_anc.astype(np.float32)).to(dev)
        else:
            trainable = trainable * torch.from_numpy(m_anc.astype(np.float32)).to(dev)
        print(f"  ancora: {n_fixos} de {n_par} pesos ficam quietos "
              f"(suporte < {ancora:g} x mediana {med:.3g})", flush=True)

    # --- Alvo em centipeoes, e nao resultado de partida ------------------
    #
    # Com etiquetas do `rotula` a segunda coluna e' o score da NOSSA busca a
    # profundidade fixa, na NOSSA escala -- nao um resultado W/D/L.
    #
    # Quando o alvo e a previsao estao nas mesmas unidades nao ha traducao a
    # fazer, e o K deixa de ser um factor de conversao entre reguas. Passam-se
    # AMBOS pela mesma sigmoide: mantem-se a virtude dela -- posicoes a +-2000
    # nao dominam o gradiente -- e a degenerescencia desaparece, porque o alvo
    # move-se com o K tal como a previsao.
    #
    # Isto e' o que torna o self-labelling superior a treinar contra a
    # avaliacao de outro motor: a regua e' a mesma dos dois lados por
    # construcao, e nao por calibracao.
    alvo_score = "--alvo-score" in sys.argv
    if alvo_score:
        print(f"  alvo em centipeoes (nao resultado): media |alvo| = "
              f"{float(y.abs().mean()):.0f}cp", flush=True)
        y = torch.sigmoid(y * K)
        yv = torch.sigmoid(yv * K)

    # --- Ajustar o K aos dados, antes de tocar num peso -----------------
    #
    # O K fixo em 1/400 vem do metodo original e pressupoe que a escala da
    # avaliacao e' a do Elo. A nossa nao e': tem escalas por familia
    # (scale.king 1100, scale.threats 1150), correccao de complexidade e
    # factores de final. O K que minimiza o erro dos NOSSOS scores pode nao
    # ser 1/400, e se nao for, os pesos vao inflar ou encolher so' para
    # compensar um K errado.
    #
    # Ajustado SOZINHO e depois CONGELADO, e nao como parametro livre a par
    # dos pesos. Livres, os dois sao degenerados: multiplicar todos os pesos
    # por c e o K por c da' exactamente as mesmas previsoes, portanto existe
    # uma familia inteira de solucoes com a mesma perda. O optimizador para
    # numa qualquer -- e a escala dos pesos IMPORTA ao motor, porque as
    # margens de poda sao comparadas com scores. Foi assim que um ajuste
    # anterior chegou a pesos de 4264 e a -51 Elo com a perda a descer.
    if "--ajusta-k" in sys.argv:
        with torch.no_grad():
            melhor_k, melhor_l = K, None
            base = torch.mv(X, w)
            for cand in [K * f for f in (0.6, 0.7, 0.8, 0.9, 1.0, 1.1, 1.2, 1.4, 1.7, 2.0)]:
                pred = torch.sigmoid(base * cand)
                l = float(((pred - y) ** 2).mean())
                if melhor_l is None or l < melhor_l:
                    melhor_k, melhor_l = cand, l
            # refinar a' volta do melhor
            for cand in [melhor_k * f for f in (0.92, 0.96, 1.0, 1.04, 1.08)]:
                pred = torch.sigmoid(base * cand)
                l = float(((pred - y) ** 2).mean())
                if l < melhor_l:
                    melhor_k, melhor_l = cand, l
            print(f"  K ajustado aos dados: {melhor_k:.6f} (era {K:.6f}, "
                  f"1/{1/melhor_k:.0f} contra 1/{1/K:.0f}); perda {melhor_l:.6f}", flush=True)
            K = melhor_k

    opt = torch.optim.Adam([w], lr=lr)
    w.grad = torch.zeros_like(w)

    def forward(M, weights):
        return torch.sigmoid(torch.mv(M, weights) * K)

    def save(weights):
        text = ",".join(str(int(round(float(v)))) for v in weights.cpu().numpy())
        tmp = out_path + ".part"
        with open(tmp, "w") as fh:
            fh.write(text)
        os.replace(tmp, out_path)

    _projeccao = os.environ.get("KESTREL_PROJECCAO", "") not in ("", "0")
    if _projeccao:
        print("  projeccao canonica LIGADA (PSQT de media zero)", flush=True)
    _tracar = [int(x) for x in os.environ.get("KESTREL_TRACA", "").split(",") if x.strip().isdigit()]
    if _tracar:
        print(f"  a tracar os indices {_tracar}", flush=True)
    t0 = time.time()
    best_val, best_epoch, since, stop = float("inf"), 0, 0, ""
    for e in range(epochs):
        # The whole of the method is these four lines. The sigmoid turns the
        # evaluation into the probability White wins; the loss is against the
        # game's actual result. The gradient is written out rather than left to
        # autograd only so the transpose above can be reused.
        pred = forward(X, w)
        d = (pred - y) * (pred * (1.0 - pred))
        w.grad.copy_(torch.mv(Xt, d) * (2.0 * K / n_tr))
        if l2:
            w.grad.add_(w, alpha=2.0 * l2 / n_par)
        if trainable is not None:
            # Zeroing the gradient is not enough on its own with Adam, which
            # carries momentum: a weight frozen after some steps would keep
            # drifting on what it had accumulated. Zeroed before the step, it
            # never accumulates any.
            w.grad.mul_(trainable)
        opt.step()

        # --- Projeccao canonica: PSQT de media zero, material carrega a media
        #
        # Material e PSQT sao COLINEARES: a PSQT de uma peca ja' contem um
        # offset constante igual ao valor dela, portanto (material+c, psqt-c)
        # e' uma familia inteira de solucoes com a MESMA perda. O optimizador
        # nao tem como escolher entre elas e o material passeia por essa
        # direccao plana -- foi assim que apareceu "peao a 330 na abertura e 69
        # no final".
        #
        # A projeccao escolhe um representante canonico: a media de cada tabela
        # vai para o valor de material e a tabela fica com o desvio. NAO altera
        # a avaliacao em ponto nenhum -- material+psqt e' exactamente o mesmo --
        # so' elimina a direccao degenerada. Verificado no motor de referencia:
        # as PSQT dele somam praticamente zero (media <1cp por casa) nas quatro
        # pecas que tem valor de material.
        #
        # Rei e peao ficam de fora: o rei nao tem valor de material (a tabela
        # dele carrega tudo por definicao) e o peao so' ocupa 48 das 64 casas,
        # com as filas 1 e 8 forcadas a zero, portanto a media nao e' o
        # parametro certo para ele.
        if _projeccao and stride:
            with torch.no_grad():
                wv = w.view(-1, stride)
                base_mat = 705
                base_psqt = 705 + 12
                # O PEAO entra, mas so' nas 48 casas legais (filas 2 a 7 --
                # indices 8..55). As filas 1 e 8 estao forcadas a zero e
                # inclui-las na media deslocava-a por um sexto sem razao.
                # Sem isto a colinearidade material/PSQT do peao fica intacta,
                # que e' exactamente onde o colapso acontecia.
                for fase in range(2):
                    ini_p = base_psqt + (fase * 6 + 0) * 64
                    bloco_p = wv[:, ini_p + 8:ini_p + 56]
                    media_p = bloco_p.mean(dim=1, keepdim=True)
                    bloco_p.sub_(media_p)
                    wv[:, base_mat + fase * 6 + 0] += media_p.squeeze(1)
                for fase in range(2):          # 0 = meio-jogo, 1 = final
                    for peca in range(1, 5):   # cavalo, bispo, torre, dama
                        ini = base_psqt + (fase * 6 + peca) * 64
                        bloco = wv[:, ini:ini + 64]
                        media = bloco.mean(dim=1, keepdim=True)
                        bloco.sub_(media)
                        wv[:, base_mat + fase * 6 + peca] += media.squeeze(1)
                # O peao e' o PADRAO DE MEDIDA. Com ele pregado nao ha
                # liberdade de escala nenhuma: tudo o resto encontra o seu
                # valor em unidades de peao, e o K deixa de ser um numero a
                # deriva para passar a significar alguma coisa.
                if peao_fixo is not None:
                    wv[:, base_mat + 0] = peao_fixo[0]
                    wv[:, base_mat + 6] = peao_fixo[1]

        # DIAGNOSTICO (KESTREL_TRACA=indices separados por virgula): regista a
        # trajectoria de parametros escolhidos. Com Adam o passo e' normalizado
        # pelo gradiente, portanto um parametro SEM sinal anda ~lr por iteracao
        # na mesma -- passeia em vez de convergir. A paragem antecipada e'
        # global e nao o trava. Isto mostra qual dos dois casos e' cada um.
        if _tracar and (e % 10 == 0 or e == epochs - 1):
            vals = " ".join(f"{i}={float(w[i]):+8.2f}" for i in _tracar)
            print(f"  traca e={e:5d} {vals}", flush=True)

        # Validation decides when to stop, not the epoch count. Training loss
        # only ever falls; the held-out set is what says whether anything was
        # learned.
        if e % 25 == 0 or e == epochs - 1:
            with torch.no_grad():
                tloss = torch.mean((pred - y) ** 2).item()
                vloss = torch.mean((forward(Xv, w) - yv) ** 2).item()
            better = vloss < best_val - 1e-7
            if better:
                best_val, best_epoch, since = vloss, e, 0
                save(w)
            else:
                since += 25
            print(f"  epoch {e:5}  treino {tloss:.6f}  validacao {vloss:.6f}"
                  f"{'  <- melhor, guardado' if better else ''}"
                  f"  [{time.time() - t0:.0f}s]", flush=True)
            # Stop when the held-out loss has not improved for a while: past
            # that point every further epoch is fitting noise in the training
            # half, and the weights get worse while the number on screen keeps
            # looking better.
            if since >= max(200, epochs // 10):
                stop = f"validacao sem melhorar ha {since} epocas"
            if time.time() - t0 > budget:
                stop = "orcamento de tempo esgotado"
            if stop:
                print(f"  paragem na epoca {e}: {stop}", flush=True)
                break

    print(f"  melhor validacao {best_val:.6f} na epoca {best_epoch} "
          f"-- e' esse o ficheiro em {out_path}", flush=True)

    # --- Em que regua e' que o motor ficou? ------------------------------
    #
    # Com o peao pregado nao ha liberdade de escala: o K que melhor ajusta a
    # validacao com os pesos FINAIS diz em que unidades a avaliacao ficou.
    # A regua padrao dos motores e' K = 1/400. Se o K optimo sair MENOR que
    # aquele com que se treinou, a avaliacao ficou mais alta do que devia.
    with torch.no_grad():
        base_v = torch.mv(Xv, w)
        melhor_k, melhor_e = K, float("inf")
        cand = K * 0.05
        while cand <= K * 8.0:
            e = float(torch.mean((torch.sigmoid(base_v * cand) - yv) ** 2))
            if e < melhor_e:
                melhor_e, melhor_k = e, cand
            cand *= 1.02
        print(f"  ESCALA FINAL: K classico {1.0 / melhor_k:.0f}"
              f"   (padrao 400; treinou-se a {1.0 / K:.0f})", flush=True)


main()
