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

/// Nap2Siriux's own `nnue_net.cpp`, vendored and compiled straight in --
/// their real evaluate(), their real SIMD, no Rust reimplementation to keep
/// in sync bug-for-bug. `board.h` here is a ~50-line stand-in (see the file)
/// exposing only the handful of methods nnue_net.cpp actually calls; their
/// real board.h pulls in movegen/attacks/eval_state/cuckoo that a static
/// eval call never needed.
fn build_napv10_cpp() {
    println!("cargo:rerun-if-changed=vendor/napv10");
    cc::Build::new()
        .cpp(true)
        .std("c++20")
        .file("vendor/napv10/napoleon/nnue_net.cpp")
        .file("vendor/napv10/attacks.cpp")
        .file("vendor/napv10/shim.cpp")
        .include("vendor/napv10")
        .flag_if_supported("-mavx2")
        .flag_if_supported("-mfma")
        .flag_if_supported("-w") // vendored code, not ours to keep warning-clean
        .opt_level(3)
        .compile("napv10cpp");
}

fn main() {
    build_napv10_cpp();
    // A escala da rede, decidida em tempo de COMPILACAO e nao pela ponte.
    //
    // As margens de poda sao centipeoes absolutos, portanto a escala e' uma
    // propriedade da REDE que o binario traz -- nao uma preferencia de quem o
    // lanca. Configurada de fora, bastava trocar o binario e esquecer o
    // `setoption` para a busca inteira correr com todas as margens
    // proporcionalmente erradas, sem nada a avisar. Aqui as duas coisas
    // viajam juntas.
    println!("cargo:rerun-if-env-changed=KESTREL_ESCALA");
    let escala = std::env::var("KESTREL_ESCALA").unwrap_or_else(|_| "200".to_string());
    println!("cargo:rustc-env=KESTREL_ESCALA_COMPILADA={escala}");

    // Two LMR terms, in MILLI-PLIES (1/1024 of a ply), baked in at build
    // time so each SPRT arm is a distinct binary -- the house rule is that a
    // test which decides something compares two binaries, never one binary
    // handed two options, because a `setoption` that fails to arrive turns a
    // measurement into a coin toss with nothing to show for it.
    //
    // Both default to 0 = OFF, so a build with neither variable set is
    // bit-identical to the engine before they existed.
    //
    // KESTREL_LMR_MOVE_LINEAR -- reduction subtracted per move index, i.e.
    //   `r -= c * move_index`. Our curve grows with ln(m) only and never
    //   flattens, so late moves get reduced harder and harder; a linear term
    //   is what bends the tail back. Curve analysis against a reference's
    //   published shape put the fit for OUR base curve at ~31 (0.030 ply per
    //   move) -- half of what that reference uses on its own curve, which is
    //   exactly why the number had to be fitted here rather than borrowed.
    //
    // KESTREL_LMR_CUTNODE -- extra reduction at a cutnode, where a fail-high
    //   is expected. Tried once as a flat +2 whole plies and it wrecked the
    //   engine; with the fixed-point accumulator it can finally be a
    //   fraction, which is the only form in which it has ever worked
    //   anywhere.
    for (var, default) in [("KESTREL_LMR_MOVE_LINEAR", "0"), ("KESTREL_LMR_CUTNODE", "0")] {
        println!("cargo:rerun-if-env-changed={var}");
        let v = std::env::var(var).unwrap_or_else(|_| default.to_string());
        v.parse::<i32>()
            .unwrap_or_else(|e| panic!("{var} tem de ser um inteiro em milli-plies: {e}"));
        println!("cargo:rustc-env={var}_COMPILADO={v}");
    }

    embute("KESTREL_V1_EMBUTIDA", "rede_v1_embutida.bin", "v1_embutida");
    embute("KESTREL_THREATS_EMBUTIDA", "rede_threats_embutida.bin", "threats_embutida");
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
