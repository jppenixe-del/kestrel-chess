"""Fitting the evaluation weights to game results, on the GPU.

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
    n_par = int(cols.max()) + 1
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
    if stride and free:
        m = (np.arange(n_par) % stride) < free
        trainable = torch.from_numpy(m.astype(np.float32)).to(dev)
        print(f"  a treinar {int(m.sum())} de {n_par} pesos "
              f"(os primeiros {free} de cada {stride}); os restantes ficam como estao",
              flush=True)

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


main()
