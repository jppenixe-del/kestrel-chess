# History

This repository holds two engines, one after the other.

## Kestrel — Rust, 2026

The project began in Rust. Board representation, bitboards, magic attack tables,
move generation, the transposition table and the search were all written from
scratch, and the first NNUE networks were trained against that engine.

That line is preserved: branch [`kestrel-rust`](../../tree/kestrel-rust), tagged
`kestrel-rust-final`. The 467 commits under this one are its history.

An earlier stage of the network work grew out of
[`pawn`](https://github.com/ruicoelhopedro/pawn) by Rui Coelho, whose file
format the networks are still trained against. See [`NOTICES.md`](NOTICES.md).

## KestrelStrike — C++, from 2026

The current engine. The search is a continuation of the Rust one — same ideas,
same measurements, transcribed and then developed further — but it sits on the
Stockfish substrate rather than on our own board code, which buys speed and
keeps the evaluation aligned with the feature sets the network was trained on.

What that means concretely, and what is ours and what is not, is set out in
[`NOTICES.md`](NOTICES.md). The network is ours: [`NETWORKS.md`](NETWORKS.md).
