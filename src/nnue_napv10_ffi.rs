//! FFI para o `evaluate()` REAL do Nap2Siriux (`vendor/napv10/`), em vez de
//! uma reimplementação em Rust.
//!
//! Substitui `nnue_napv10.rs`: essa versão foi escrita à mão a partir da
//! leitura do `.cpp` e teve quatro bugs de transcrição confirmados numa
//! sessão (bias dequant, balde de material, warmup em falta, correctivo de
//! rei nunca portado) antes de ainda assim ficar mais fraca do que devia --
//! cada reimplementação é uma nova oportunidade de errar uma fórmula. Isto
//! chama o código deles directamente: mesma matemática, mesmo SIMD, sem
//! haver uma segunda cópia para manter sincronizada.
//!
//! `vendor/napv10/board.h` é a única peça que não é o código deles -- um
//! stand-in de ~50 linhas para a Board real (que arrasta movegen/attacks/
//! eval_state/cuckoo, nada disto necessário para uma chamada de avaliação
//! estática), com exactamente os métodos que `nnue_net.cpp` chama.

use crate::board::Board;

extern "C" {
    fn napv10_load(path: *const std::os::raw::c_char) -> std::os::raw::c_int;
    fn napv10_evaluate(
        pieces: *const [u64; 6],
        side_to_move: std::os::raw::c_int,
        game_ply: std::os::raw::c_int,
    ) -> std::os::raw::c_int;
}

fn carrega(path: &str) -> bool {
    let Ok(c_path) = std::ffi::CString::new(path) else {
        return false;
    };
    unsafe { napv10_load(c_path.as_ptr()) != 0 }
}

static CARREGADA: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn ligada() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("KESTREL_NAPV10").map(|v| v != "0").unwrap_or(true))
}

pub fn rede_carregada() -> bool {
    *CARREGADA.get_or_init(|| {
        let Ok(path) = std::env::var("KESTREL_NNUE_NAPV10") else {
            return false;
        };
        let ok = carrega(&path);
        if ok {
            eprintln!("nnue-napv10 (ffi): rede carregada de {path}");
        } else {
            eprintln!("nnue-napv10 (ffi): falha a carregar {path}");
        }
        ok
    })
}

/// `ligada()` E' a rede que decidiu esta avaliacao -- ver o mesmo par em
/// `nnue_napv10.rs` (a versao Rust, ainda no repo mas fora do dispatch de
/// `evaluation.rs`); `search.rs` usa isto para as margens de poda.
pub fn active() -> bool {
    ligada() && rede_carregada()
}

pub fn evaluate(board: &Board) -> i32 {
    let pieces: [[u64; 6]; 2] = board.pieces;
    let side = board.side.idx() as std::os::raw::c_int;
    unsafe { napv10_evaluate(pieces.as_ptr(), side, board.fullmove as std::os::raw::c_int) }
}

pub fn escala() -> i32 {
    410
}
