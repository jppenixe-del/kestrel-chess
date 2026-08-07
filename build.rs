//! Embeds a v3 network into the binary when `KESTREL_V3_EMBUTIDA` is set at
//! build time.
//!
//! A/B tests that decide something must compare two *binaries*, never one
//! binary handed two files: an env var that fails to reach the child process,
//! or a path that moves under a running match, turns a measurement into a
//! coin toss without saying so. Building the weights in makes the two arms
//! impossible to mix up.
//!
//! Without the variable the binary is unchanged and still reads
//! `KESTREL_NNUE_V3` at runtime, which is what the diagnostics want.

use std::path::PathBuf;

/// Copies the network named by `var` into OUT_DIR and turns on `cfg`.
/// Always leaves a file behind, because `include_bytes!` is resolved even on
/// the branch that is not compiled.
fn embute(var: &str, ficheiro: &str, cfg: &str) {
    println!("cargo:rerun-if-env-changed={var}");
    let saida = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join(ficheiro);
    match std::env::var(var) {
        Ok(p) if !p.is_empty() => {
            println!("cargo:rerun-if-changed={p}");
            let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("nao consegui ler {p}: {e}"));
            std::fs::write(&saida, bytes).unwrap();
            println!("cargo:rustc-cfg={cfg}");
        }
        _ => {
            std::fs::write(&saida, []).unwrap();
        }
    }
}

fn main() {
    embute("KESTREL_V1_EMBUTIDA", "rede_v1_embutida.bin", "v1_embutida");
    println!("cargo:rerun-if-env-changed=KESTREL_V3_EMBUTIDA");
    let saida = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("rede_v3_embutida.bin");
    match std::env::var("KESTREL_V3_EMBUTIDA") {
        Ok(p) if !p.is_empty() => {
            println!("cargo:rerun-if-changed={p}");
            let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("nao consegui ler {p}: {e}"));
            std::fs::write(&saida, bytes).unwrap();
            println!("cargo:rustc-cfg=v3_embutida");
        }
        // include_bytes! needs the file to exist even on the path we don't
        // compile, so leave an empty one behind.
        _ => {
            std::fs::write(&saida, []).unwrap();
        }
    }
}
