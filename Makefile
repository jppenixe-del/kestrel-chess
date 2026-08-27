# Wraps `cargo build` for OpenBench, which invokes `make -j EXE=<path>
# EVALFILE=<path>` regardless of the engine's language. Cargo has no
# equivalent convention of its own, so this file exists to translate one
# into the other -- it does not add anything to how the engine is actually
# built.
#
# 2026-08-27: EVALFILE goes to the SFNNv16 path, not the v1 one.
#
# It used to be handed to KESTREL_V1_EMBUTIDA, which embeds an OLD-format
# network. The nets we test with are SFNNv16 -- 95 MB read by a completely
# different reader -- so the binary got 95 MB of one format offered as
# another, found no network it could use, and fell back to a flat zero for
# every position. The symptom was a bench of 4354449 nodes where the same
# commit and the same net give 1954973: every heuristic measured against
# noise.
#
# The path is baked in rather than the bytes: the client keeps the network
# in Networks/<sha> and that absolute path stays valid on that machine,
# which costs nothing at build time against embedding 95 MB.
EXE ?= kestrel
EVALFILE ?=
KESTREL_ESCALA ?= 176

rule:
	KESTREL_NNUE_SF_BUILD=$(EVALFILE) KESTREL_ESCALA=$(KESTREL_ESCALA) cargo build --release
	cp target/release/kestrel $(EXE)
