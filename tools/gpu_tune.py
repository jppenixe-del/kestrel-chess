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

    X = torch.sparse_coo_tensor(
        torch.from_numpy(np.stack([rows, cols])),
        torch.from_numpy(vals), (n_pos, n_par), device=dev).coalesce()
    y = torch.from_numpy(results).to(dev)
    w = torch.zeros(n_par, device=dev, requires_grad=True)

    opt = torch.optim.Adam([w], lr=lr)
    t0 = time.time()
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
        if e % max(1, epochs // 10) == 0 or e == epochs - 1:
            print(f"  epoch {e:5}  loss {loss.item():.6f}  "
                  f"({(time.time() - t0) / (e + 1) * 1000:.1f}ms/epoch)")

    out = w.detach().cpu().numpy()
    # Rounded to integers because that is what the engine's weights are. Doing
    # it here rather than letting the engine truncate keeps the file honest
    # about what will actually be used.
    text = ",".join(str(int(round(float(v)))) for v in out)
    with open(out_path, "w") as fh:
        fh.write(text)
    print(f"wrote {n_par} weights to {out_path} in {time.time() - t0:.1f}s")


main()
