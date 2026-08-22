# Licences, provenance, and credit

Kestrel is an independent implementation. This file records what is
third-party in it, under what terms, and where the line falls between an idea
and a copy — so that anyone can check rather than guess.

## What is in this repository

The **search and everything around it** — board representation, move
generation, `search.rs` — is written for this project.

The **NNUE inference side is derived from Stockfish**. Reading a network in
the SFNNv16 format means implementing that format: its quantisation, its
layer layout, and the definition of its input feature sets, including their
index mappings and orientation tables. That is Stockfish's work and it is
GPLv3. It is listed below and it is why this project is GPLv3.

Everything third-party is in the table below. Nothing else in `src/` is a
port, a translation or a copy of another engine's code.

## Third-party code and data

| what | where | licence | how it is used |
|---|---|---|---|
| [Stockfish](https://github.com/official-stockfish/Stockfish) — SFNNv16 NNUE evaluation | `src/nnue_sf.rs` | GPL-3.0 | The network format this engine reads: quantisation, layer layout, and the `HalfKAv2_hm` / `Full_Threats` / `PP_3Wide` input feature sets with their index mappings and orientation tables. |
| Stockfish — training data (5000-node evaluations) | not in this repo | data, published by the Stockfish project | The positions and evaluations the network in training learns from. |
| [bullet](https://github.com/jw1912/bullet) | not in this repo; build-time trainer | MIT | One of the two trainers used to produce the network. |

Anything added here must be listed above **before** it is committed, with its
licence and its notice preserved.

## Input feature attribution

The pawn-pair block (`PP_3Wide`, `4560` inputs — pairs of pawns at most one
file apart) is **not an original idea of this project**. It was invented by
**Jonathan Hallström** for
[Pawnocchio](https://github.com/JonathanHallstrom/pawnocchio), from the
observation that in a network trained on *all* pawn pairs the ones that
mattered were those at most one file apart. It was also used by Stormphrax and
Viridithas, and Stockfish adopted it as `PP_3Wide`. This engine's
implementation and its trained weights are its own; the idea is his.

## Network training

The evaluation network is **trained by this project**, and the weights are
ours: Stockfish supplies the *format* the weights are stored and evaluated in,
not the weights themselves.

The data, however, is **not** ours. The network is trained on the
**Stockfish project's published data** — positions evaluated at 5000 nodes —
filtered by this project's own pipeline. Credit for that data belongs to the
Stockfish project and to the people who generated it.

If openly-licensed external data is added later — for example the LCZero
project's self-play data, which is published under the Open Database Licence
(ODbL-1.0) with contents under DBCL-1.0 — it will be listed above and credited
here. Note that a network trained on an ODbL database is a *Produced Work*: the
share-alike terms attach to redistributing the database, not to the network or
to the code that loads it. Attribution is still required, and would be given.

## Ideas we studied

Every modern engine builds on a body of published technique that belongs to
nobody in particular: what a term measures, why a curve has the shape it does,
how a mechanism is usually structured. That literature — the Chess Programming
Wiki, forum threads, papers, and the source of open engines — is where the
questions come from.

The implementations here are this project's own, and the rule is the one
stated further down: **the question may come from anywhere public; the number
must come from our own data.** Where a source comment reasons a value out
rather than measuring it, it says so.

No values from another engine are used verbatim. Where an early version of
this engine did carry reference values directly — the mobility and threat
tables were imported at their original scale — that was found to be a defect,
not a shortcut: values calibrated against another engine's material scale are
wrong in ours, and the cost of that is documented in the source comments.

## The threat feature set

The threat inputs in `src/features.rs` are a five-dimensional tuple — side,
relation, attacker type, victim type, victim square — flattened by mixed-radix
packing. Threat features as an NNUE input are common practice across modern
engines and originate with Stockfish; the packing arithmetic is the obvious
way to flatten a tuple and belongs to nobody.

The mapping is this project's own. It does not follow any other engine's
construction: it does not index on the attacker's square at all, uses no
precomputed pair or offset tables, and does not densely enumerate attack
patterns — all of which are the substance of how other engines build theirs.

The compact 640-input set (`make_threat_640`, direction x victim type x
square) is a different and much coarser choice, and is also our own.

## AGPL

**No file under `src/` derives from an AGPL-licensed source.** This is the one
line that cannot be crossed by attribution alone: AGPL-3.0 §13 requires
offering source to anyone who interacts with the program *over a network*, and
this engine runs a bot on a public server. A single AGPL-derived file would
pull the whole project under that obligation.

The rule that governs everything else, in one line: **the question may come
from anywhere public; the number must come from our own data.**

- **Allowed**: reading how a technique works and why a configuration was
  chosen. Understanding the reasoning is what engineering is; it is not
  anybody's property, and it is what tells us which question to go and
  measure.
- **Not allowed**: taking the values. Constants another engine tuned belong to
  that engine's own scale, its own data and its own search. Lifting them is
  both a licence problem and a technical one — values calibrated against
  another engine's material scale are simply wrong in ours, which this project
  has measured the cost of.

Where a shape is shared with another engine because both descend from the same
published family, the source comment says so plainly rather than implying
independent invention.

## What counts as taking, beyond copying code

Three things are easy to treat as "just data" and are not:

- **Layout tables.** A bucket layout copied as values is copied work, even
  though it looks like a list of numbers.
- **Feature-to-index mappings.** Worse than a table, because a trained network
  is bound to the mapping it was trained on -- adopting someone's mapping means
  the network cannot be freed from it later without retraining from scratch.
- **Vectorised kernels.** The algorithm is free; a SIMD routine whose structure
  follows another's line by line is not independent just because the identifiers
  differ.

Where this project introduces bucketed inputs or output buckets, the layouts are
computed here from a stated rule, not transcribed.

## Artwork

The logo and the banner in `assets/` were generated with Google Gemini from
prompts written for this project, and then assembled here.

This is recorded for the same reason everything else on this page is: someone
looking at the mark should be able to find out where it came from without
having to ask. Google's terms place no attribution requirement on the images
themselves, so this note is not a licence obligation being discharged. It is
simply true, and a credits page that lists only the obligations is not a
credits page.

## Reporting a problem

If you believe anything here is inaccurate or that this project contains code
it should not, open an issue with the specifics and it will be investigated
and corrected.
