//! A ponte para o aparelho de medicao, e a sua ausencia.
//!
//! Este ficheiro existe para que o resto do motor possa chamar o aparelho de
//! medicao sem saber se ele esta' la'. Com a `feature` **ponte** ligada as
//! chamadas vao ao modulo `medidor`; sem ela sao funcoes vazias que o
//! compilador remove, e o binario fica identico ao caminho que ja' hoje corre
//! sem aparelho -- o do leitor proprio, que da' os mesmos numeros com 0,57
//! centipeoes de desvio em 80 posicoes.
//!
//! Os vinte e tres sitios que chamavam o aparelho **nao mudaram de lugar nem de
//! ordem**: mudaram de nome de modulo e mais nada. E' de proposito: uma
//! separacao que mexesse na busca teria de ser medida outra vez, e esta nao tem.

// `#[path]` porque o ficheiro vive na raiz de `src/` e nao numa pasta com o
// nome deste modulo: e' um ficheiro NAO VERSIONADO, e mante-lo onde esta' evita
// que a arvore publica tenha uma pasta vazia a apontar para nada.
#[cfg(feature = "ponte")]
#[path = "medidor.rs"]
mod medidor;

#[cfg(feature = "ponte")]
pub use medidor::{
    avalia, com, conta_ponte, desfaz, desfaz_nulo, lance, liga_com, ligado, nulo, otimismo, raiz,
    NEGATIVO, PICO,
};

/// Uma avaliacao directa de uma posicao pelo aparelho, para a linha de
/// comandos. Existe para o `main.rs` nao ter de conhecer o tipo interno.
#[cfg(feature = "ponte")]
pub fn avalia_fen(fen: &str) -> Option<i32> {
    raiz(fen);
    com(|c| c.avalia())
}

#[cfg(not(feature = "ponte"))]
mod vazio {
    use std::sync::atomic::{AtomicI64, AtomicU64};

    pub static PICO: AtomicI64 = AtomicI64::new(0);
    pub static NEGATIVO: AtomicU64 = AtomicU64::new(0);

    /// Sem aparelho nao ha' ponte ligada, e todo o resto do motor ja' sabe
    /// responder a isso: e' o caminho normal de quem so' tem o leitor proprio.
    #[inline(always)]
    pub fn ligado() -> bool {
        false
    }
    #[inline(always)]
    pub fn liga_com(_caminho: &str) -> bool {
        false
    }
    #[inline(always)]
    pub fn conta_ponte() -> bool {
        false
    }
    #[inline(always)]
    pub fn raiz(_fen: &str) {}
    #[inline(always)]
    pub fn lance(_m: &crate::moves::Move) {}
    #[inline(always)]
    pub fn desfaz(_m: &crate::moves::Move) {}
    #[inline(always)]
    pub fn nulo() {}
    #[inline(always)]
    pub fn desfaz_nulo() {}
    #[inline(always)]
    pub fn avalia(_fen: &dyn Fn() -> String) -> Option<i32> {
        None
    }
    #[inline(always)]
    pub fn avalia_fen(_fen: &str) -> Option<i32> {
        None
    }
    /// Sem aparelho nao ha' avaliacao a informar.
    #[inline(always)]
    pub fn otimismo(_v: i32) {}
}

#[cfg(not(feature = "ponte"))]
pub use vazio::*;
