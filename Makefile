# Wraps `cargo build` for OpenBench, which invokes `make -j EXE=<path>
# EVALFILE=<path>` regardless of the engine's language. Cargo has no
# equivalent convention of its own, so this file exists to translate one
# into the other -- it does not add anything to how the engine is actually
# built.
#
# Found empty until now: every OpenBench test this engine ever ran built
# with plain `cargo build --release`, no EVALFILE handling at all. Without
# KESTREL_V1_EMBUTIDA, evaluate() has no network to fall back to and
# returns a flat 0 for every position (see `sem_rede()` in evaluation.rs) --
# so every test that ever ran through this Makefile measured search
# heuristics against pure noise, not the actual engine.
EXE ?= kestrel
EVALFILE ?= $(CURDIR)/rede_512_e176.bin
KESTREL_ESCALA ?= 176

rule:
	KESTREL_V1_EMBUTIDA=$(EVALFILE) KESTREL_ESCALA=$(KESTREL_ESCALA) cargo build --release
	cp target/release/kestrel $(EXE)
