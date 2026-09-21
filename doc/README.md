# The working record

These are the engineering documents of this engine, in Portuguese, because that
is the language they were written in and translating a measurement is a good way
to lose it. The published code and its comments are in English; these are the
notes behind them.

They are here for one reason. On 19 September 2026 the machine holding them
died, and with it the reasoning behind most of this engine's numbers. What
survived was what had been written next to the code. Two days later, a full day
of work went into re-discovering things these files already said -- including
two entries from a table headed "measured and REJECTED -- do not repeat".

| file | what it is |
|---|---|
| `PLANO.md` | the order of work, what was measured and kept, and what was measured and rejected |
| `CONSTANTES.md` | where every constant came from, and whether it was swept HERE or inherited |
| `VALIDACOES.md` | the index of differences against the reference engine, with their status |

`FIDELIDADE.md` -- 293 lines on where this port diverges from the Rust engine it
came from -- has not been recovered. It is the one that matters most: it is the
list of blocks this engine implements that the Rust engine keeps switched off,
and the list of blocks the Rust engine uses that this one never got. Two of the
entries on that second list were restored on 21 September and were worth +18 Elo
each.

The rule at the top of `CONSTANTES.md` is the one that governs everything else:

> **The shape transfers, the number does not.** An inherited constant is
> calibrated for another evaluation scale and other history tables -- ours are
> about fifty times smaller -- and using it without measuring is cloning and
> being wrong at the same time.
