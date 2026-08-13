//! FFI into the REAL `evaluate()` from the napv10 project (`vendor/napv10/`),
//! instead of a Rust reimplementation.
//!
//! Replaces `nnue_napv10.rs`: that version was hand-written from reading the
//! `.cpp` and had four confirmed transcription bugs in one session (bias
//! dequant, material bucket formula, missing warmup, the anti-king-walk
//! corrector never ported) before still underperforming -- every
//! reimplementation is another chance to get a formula wrong. This calls
//! the original code directly instead: same math, same SIMD, no second copy
//! to keep in sync.
//!
//! `vendor/napv10/board.h` is the one piece that is not vendored as-is -- a
//! ~50-line stand-in for the real Board (which pulls in movegen/attacks/
//! eval_state/cuckoo, none of it needed for a static eval call), exposing
//! exactly the methods `nnue_net.cpp` calls.

use crate::board::Board;

extern "C" {
    fn napv10_load(path: *const std::os::raw::c_char) -> std::os::raw::c_int;
    fn napv10_evaluate(
        pieces: *const [u64; 6],
        side_to_move: std::os::raw::c_int,
        game_ply: std::os::raw::c_int,
    ) -> std::os::raw::c_int;
}

fn load(path: &str) -> bool {
    let Ok(c_path) = std::ffi::CString::new(path) else {
        return false;
    };
    unsafe { napv10_load(c_path.as_ptr()) != 0 }
}

static LOADED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("KESTREL_NAPV10").map(|v| v != "0").unwrap_or(true))
}

pub fn net_loaded() -> bool {
    *LOADED.get_or_init(|| {
        let Ok(path) = std::env::var("KESTREL_NNUE_NAPV10") else {
            return false;
        };
        let ok = load(&path);
        if ok {
            eprintln!("nnue-napv10 (ffi): net loaded from {path}");
        } else {
            eprintln!("nnue-napv10 (ffi): failed to load {path}");
        }
        ok
    })
}

/// `enabled()` AND the net actually decided this evaluation -- `search.rs`
/// reads this for the pruning-margin scale.
pub fn active() -> bool {
    enabled() && net_loaded()
}

pub fn evaluate(board: &Board) -> i32 {
    let pieces: [[u64; 6]; 2] = board.pieces;
    let side = board.side.idx() as std::os::raw::c_int;
    unsafe { napv10_evaluate(pieces.as_ptr(), side, board.fullmove as std::os::raw::c_int) }
}

pub fn escala() -> i32 {
    410
}
