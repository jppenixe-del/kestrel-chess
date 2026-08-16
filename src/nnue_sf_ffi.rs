//! Ponte para o forward NNUE do Stockfish (GPL-3.0), quando compilada.
//!
//! Porquê: medido na mesma máquina, mesma posição, 1 thread -- Stockfish
//! 138k NPS, Triumviratus 202k, a nossa versão em Rust escalar 46k. O perfil
//! diz 73% do tempo em avaliação, 5% em busca. A diferença não está na
//! arquitectura, está em quem executa o forward.
//!
//! Ligado por `KESTREL_NNUE_SF_FFI=<rede.nnue>`. Sem essa variável, ou sem a
//! biblioteca, nada disto entra em jogo e o motor usa o caminho em Rust.

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
