"""Texel tuning on the GPU.

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
    python3 gpu_tune.py feats.bin out.txt [epochs] [K] [lr] [l2]

`out.txt` is the comma-separated vector the engine reads back through
KESTREL_BUCKET_WEIGHTS.
"""
import struct
import sys
import time

import numpy as np
import torch


def load(path):
    """Read the whole file into COO arrays.

    Parsed with numpy rather than a Python loop: at a quarter million positions
    and several hundred features each, the loop is the slowest part of the job
    by a wide margin -- minutes against seconds, to prepare work the GPU then
    does in milliseconds.
    """
    raw = np.fromfile(path, dtype=np.uint8)
    rows, cols, vals, results = [], [], [], []
    off = 0
    total = raw.size
    row = 0
    # One position at a time, but each position parsed in one numpy call.
    while off < total:
        n = int(struct.unpack_from("<H", raw, off)[0])
        off += 2
        block = raw[off:off + n * 6]
        if block.size < n * 6:
            break
        idx = np.frombuffer(block.tobytes(), dtype=np.dtype([("i", "<u2"), ("v", "<f4")]))
        off += n * 6
        _phase = struct.unpack_from("<f", raw, off)[0]
        off += 4
        res = struct.unpack_from("<f", raw, off)[0]
        off += 4
        rows.append(np.full(n, row, dtype=np.int64))
        cols.append(idx["i"].astype(np.int64))
        vals.append(idx["v"].astype(np.float32))
        results.append(res)
        row += 1
    return (np.concatenate(rows), np.concatenate(cols), np.concatenate(vals),
            np.array(results, dtype=np.float32), row)


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return
    feats_path, out_path = sys.argv[1], sys.argv[2]
    epochs = int(sys.argv[3]) if len(sys.argv) > 3 else 400
    # Scaling constant of the logistic. 1/400 is the usual starting point; it
    # is not a free parameter to taste, it sets what "one pawn" means in the
    # probability the fit is matching, so changing it changes every weight.
    K = float(sys.argv[4]) if len(sys.argv) > 4 else 1.0 / 400.0
    lr = float(sys.argv[5]) if len(sys.argv) > 5 else 1.0
    l2 = float(sys.argv[6]) if len(sys.argv) > 6 else 0.0

    dev = "cuda" if torch.cuda.is_available() else "cpu"
    print(f"device: {dev}" + (f" ({torch.cuda.get_device_name(0)})" if dev == "cuda" else ""))

    t0 = time.time()
    rows, cols, vals, results, n_pos = load(feats_path)
    n_par = int(cols.max()) + 1
    print(f"{n_pos} positions, {n_par} parameters, {len(vals)} non-zeros "
          f"({len(vals) / max(1, n_pos):.0f} per position), read in {time.time() - t0:.1f}s")

    # A validation split, held out and never trained on.
    #
    # Without it there is no way to tell fitting from memorising, and the
    # difference is not academic: 5000 epochs on 20k positions against 5824
    # parameters drove the loss from 0.223 to 0.0119 and it was still falling.
    # That is not a model of chess, it is a lookup table for those positions.
    # Split by POSITION INDEX rather than at random per non-zero, so a
    # position's features never straddle the two sets.
    n_val = max(1, n_pos // 10)
    val_rows = set(range(n_pos - n_val, n_pos))
    is_val = np.isin(rows, np.fromiter(val_rows, dtype=np.int64))

    def build(mask, offset, height):
        return torch.sparse_coo_tensor(
            torch.from_numpy(np.stack([rows[mask] - offset, cols[mask]])),
            torch.from_numpy(vals[mask]), (height, n_par), device=dev).coalesce()

    X = build(~is_val, 0, n_pos - n_val)
    Xv = build(is_val, n_pos - n_val, n_val)
    y = torch.from_numpy(results[: n_pos - n_val]).to(dev)
    yv = torch.from_numpy(results[n_pos - n_val :]).to(dev)
    print(f"treino {n_pos - n_val}, validacao {n_val} (nunca treinada)")
    w = torch.zeros(n_par, device=dev, requires_grad=True)

    opt = torch.optim.Adam([w], lr=lr)
    t0 = time.time()
    best_val, best_w, best_epoch, since = float("inf"), None, 0, 0
    for e in range(epochs):
        opt.zero_grad()
        # Evaluation of every position at once. The sigmoid turns it into the
        # probability White wins, and the loss is against the game's actual
        # result -- the whole of Texel tuning is this line and the next.
        pred = torch.sigmoid(torch.sparse.mm(X, w.unsqueeze(1)).squeeze(1) * K)
        loss = torch.mean((pred - y) ** 2)
        if l2:
            loss = loss + l2 * torch.mean(w ** 2)
        loss.backward()
        opt.step()
        # Validation decides when to stop, not the epoch count. Training loss
        # only ever falls; the held-out set is what says whether anything was
        # learned.
        if e % 25 == 0 or e == epochs - 1:
            with torch.no_grad():
                pv = torch.sigmoid(torch.sparse.mm(Xv, w.unsqueeze(1)).squeeze(1) * K)
                vloss = torch.mean((pv - yv) ** 2).item()
            if vloss < best_val - 1e-7:
                best_val, best_w, best_epoch, since = vloss, w.detach().clone(), e, 0
            else:
                since += 25
            if e % max(25, (epochs // 10 // 25) * 25) == 0 or e == epochs - 1:
                print(f"  epoch {e:5}  treino {loss.item():.6f}  validacao {vloss:.6f}"
                      f"{'  <- melhor' if vloss <= best_val else ''}")
            # Stop when the held-out loss has not improved for a while: past
            # that point every further epoch is fitting noise in the training
            # half, and the weights get worse while the number on screen
            # keeps looking better.
            if since >= max(200, epochs // 10):
                print(f"  paragem antecipada na epoca {e}: validacao sem melhorar ha {since} epocas")
                break

    if best_w is not None:
        print(f"  melhor validacao {best_val:.6f} na epoca {best_epoch} -- sao esses os pesos guardados")
        w = best_w
    out = w.detach().cpu().numpy()
    # Rounded to integers because that is what the engine's weights are. Doing
    # it here rather than letting the engine truncate keeps the file honest
    # about what will actually be used.
    text = ",".join(str(int(round(float(v)))) for v in out)
    with open(out_path, "w") as fh:
        fh.write(text)
    print(f"wrote {n_par} weights to {out_path} in {time.time() - t0:.1f}s")


main()
