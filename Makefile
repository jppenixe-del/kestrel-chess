# Wraps `cargo build` for OpenBench, which invokes `make -j EXE=<path>`
# regardless of the engine's language. Cargo has no equivalent convention of
# its own, so this file exists to translate one into the other -- it does not
# add anything to how the engine is actually built.
EXE ?= kestrel

rule:
	cargo build --release
	cp target/release/kestrel $(EXE)
