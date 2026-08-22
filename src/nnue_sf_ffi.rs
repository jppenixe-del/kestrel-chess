//! Bridge to the reference NNUE forward pass (GPL-3.0 -- see NOTICES.md),
//! when it is compiled in.
//!
//! Why it exists: measured on one machine, one position, one thread, this
//! project's scalar Rust forward ran at roughly a third of the speed of a
//! vectorised one. The profile puts 73% of the time in evaluation against 5%
//! in search, so that gap is most of the engine. The difference is not in the
//! architecture; it is in what executes the forward pass.
//!
//! Enabled by `KESTREL_NNUE_SF_FFI=<net.nnue>`. Without that variable, or
//! without the library, none of this is reached and the Rust path is used.

#[cfg(tem_sfbridge)]
mod ligado {
    use crate::board::Board;
    use std::sync::OnceLock;

    unsafe extern "C" {
        fn sfb_init(caminho: *const std::ffi::c_char) -> i32;
        fn sfb_eval(bb: *const u64, stm: i32) -> i32;
    }

    static PRONTO: OnceLock<bool> = OnceLock::new();

    pub fn activo() -> bool {
        *PRONTO.get_or_init(|| {
            let Ok(caminho) = std::env::var("KESTREL_NNUE_SF_FFI") else { return false };
            let Ok(c) = std::ffi::CString::new(caminho.clone()) else { return false };
            let ok = unsafe { sfb_init(c.as_ptr()) } == 1;
            if ok {
                eprintln!("nnue-sf-ffi: forward do Stockfish activo ({caminho})");
            }
            ok
        })
    }

    pub fn evaluate(board: &Board) -> i32 {
        // bb[cor*6 + tipo], na ordem P N B R Q K -- a mesma dos nossos bitboards
        let mut bb = [0u64; 12];
        for c in 0..2 {
            for t in 0..6 {
                bb[c * 6 + t] = board.pieces[c][t];
            }
        }
        unsafe { sfb_eval(bb.as_ptr(), board.side as i32) }
    }
}

#[cfg(not(tem_sfbridge))]
mod ligado {
    use crate::board::Board;
    pub fn activo() -> bool { false }
    pub fn evaluate(_board: &Board) -> i32 { 0 }
}

pub use ligado::{activo, evaluate};
