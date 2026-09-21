//! Repeticao A` DISTANCIA DE UM LANCE, por tabela cuckoo (Marcel van Kervinck).
//!
//! A busca so' sabe que uma posicao repete DEPOIS de a repetir. Se o adversario pode forcar a
//! repeticao a partir daqui, isso ja' e' um empate garantido para ele, e uma linha pontuada em
//! +0,4 nao vale +0,4 nenhum, porque ele nao tem de a jogar.
//!
//! Um lance REVERSIVEL de peca (nao peao, nao captura) muda a chave zobrist sempre da mesma
//! maneira -- `peca[de] ^ peca[para] ^ lado`. Ha' um numero pequeno de tais diferencas (3668),
//! pre-calculadas numa tabela de dois hashes. Para saber se algum lance daqui devolve a uma
//! posicao ja' vista basta fazer o XOR da chave de agora com a de entao e procurar na tabela.
//!
//! Refeito sobre a API do half2k a partir do `cuckoo.rs` da Kestrel (commit 8a9c36d, 14-09).
//! Ver o cabecalho de `patch_h2k_cuckoo_rfp.py` para as duas guardas acrescentadas.

use crate::attacks::{bishop_attacks, queen_attacks, rook_attacks, Attacks};
use crate::board::Board;
use crate::types::{Color, Square};
use crate::zobrist::Zobrist;

const N: usize = 8192;

pub struct Cuckoo {
    chave: [u64; N],
    /// O lance que produz aquela diferenca, como (de, para).
    lance: [(Square, Square); N],
    /// Quantas diferencas foram postas. Tem de ser 3668, como no Stockfish.
    pub postos: usize,
}

#[inline]
fn h1(k: u64) -> usize {
    (k & 0x1fff) as usize
}
#[inline]
fn h2(k: u64) -> usize {
    ((k >> 16) & 0x1fff) as usize
}

impl Cuckoo {
    pub fn novo(zob: &Zobrist, atk: &Attacks) -> Self {
        let mut c = Cuckoo { chave: [0; N], lance: [(0, 0); N], postos: 0 };
        // 1..=5: cavalo, bispo, torre, dama, rei. O 0 e' o peao e fica de fora.
        for cor in [Color::White, Color::Black] {
            let ic = cor as usize;
            for pt in 1usize..=5 {
                for de in 0u8..64 {
                    let alvos = match pt {
                        1 => atk.knight[de as usize],
                        2 => bishop_attacks(de, 0),
                        3 => rook_attacks(de, 0),
                        4 => queen_attacks(de, 0),
                        _ => atk.king[de as usize],
                    };
                    let mut b = alvos;
                    while b != 0 {
                        let para = b.trailing_zeros() as u8;
                        b &= b - 1;
                        // Cada par conta uma vez.
                        if para <= de {
                            continue;
                        }
                        let mut chave = zob.piece_sq[ic][pt][de as usize]
                            ^ zob.piece_sq[ic][pt][para as usize]
                            ^ zob.side;
                        let mut mv = (de, para);
                        let mut i = h1(chave);
                        loop {
                            std::mem::swap(&mut c.chave[i], &mut chave);
                            std::mem::swap(&mut c.lance[i], &mut mv);
                            if chave == 0 {
                                break;
                            }
                            i = if i == h1(chave) { h2(chave) } else { h1(chave) };
                        }
                        c.postos += 1;
                    }
                }
            }
        }
        c
    }

    /// Ha' um lance daqui que devolve a uma posicao ja' vista?
    ///
    /// `keys`: as chaves da partida e do caminho, a ULTIMA e' a posicao actual (`board.hash`).
    /// `root_keys`: quantas chaves havia na raiz; um indice `>= root_keys` esta' DENTRO da busca.
    pub fn repeticao_a_vista(&self, board: &Board, keys: &[u64], root_keys: usize, atk: &Attacks) -> bool {
        let fim = keys.len();
        let hm = board.halfmove as usize;
        if hm < 3 || fim < 4 {
            return false;
        }
        let agora = board.hash;
        let ocupadas = board.occ_all;
        let meus = board.occ_color[board.side as usize];
        // Posicoes a distancia impar (3, 5, 7...): tem o lado oposto a jogar, e um lance NOSSO
        // leva-nos la'. So' vale a pena olhar dentro do contador dos cinquenta lances.
        let mut i = 3usize;
        while i <= hm.min(fim - 1) {
            let idx = fim - 1 - i;
            // O hash inclui `side` quando jogam as pretas e cada lance inverte-o: duas posicoes a
            // distancia impar diferem de `side` UMA vez, e a chave da tabela ja' o traz. Um `^ side`
            // a mais (como no cuckoo.rs da Kestrel) anula-o e a tabela nunca acerta.
            let dif = agora ^ keys[idx];
            let mut j = h1(dif);
            if self.chave[j] != dif {
                j = h2(dif);
                if self.chave[j] != dif {
                    i += 2;
                    continue;
                }
            }
            let (de, para) = self.lance[j];
            let pontas = (1u64 << de) | (1u64 << para);
            // Caminho livre (o `between` exclui as pontas) e a peca que se mexe e' de quem joga.
            if atk.between[de as usize][para as usize] & ocupadas == 0 && meus & pontas != 0 {
                // Dentro da busca basta uma ocorrencia. Na historia da partida, a posicao de
                // entao tem de ja' se ter repetido, senao contava-se como empate uma so' visita.
                if idx >= root_keys || repetiu_antes(keys, idx) {
                    return true;
                }
            }
            i += 2;
        }
        false
    }
}

/// A posicao em `idx` ja' tinha ocorrido antes? (mesmo lado a jogar: desvios pares)
fn repetiu_antes(keys: &[u64], idx: usize) -> bool {
    let k = keys[idx];
    keys[..idx].iter().rev().skip(1).step_by(2).take(50).any(|x| *x == k)
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::movegen::generate_legal;

    fn jogar(b: &mut Board, keys: &mut Vec<u64>, atk: &Attacks, uci: &str) {
        let legal = generate_legal(b, atk);
        let mv = *legal.iter().find(|m| m.to_uci() == uci).expect("lance ilegal no teste");
        b.make_move(&mv);
        keys.push(b.hash);
    }

    #[test]
    fn tem_3668_diferencas_como_o_stockfish() {
        let atk = Attacks::new();
        let c = Cuckoo::novo(crate::zobrist::tabelas(), &atk);
        assert_eq!(c.postos, 3668);
    }

    #[test]
    fn ve_a_repeticao_a_um_lance() {
        let atk = Attacks::new();
        let c = Cuckoo::novo(crate::zobrist::tabelas(), &atk);
        let mut b = Board::startpos();
        let mut k = vec![b.hash];
        for m in ["g1f3", "g8f6", "f3g1"] {
            jogar(&mut b, &mut k, &atk, m);
        }
        // Pretos a jogar: f6g8 repete a posicao inicial.
        assert!(c.repeticao_a_vista(&b, &k, 0, &atk), "dentro da busca: uma visita chega");
        assert!(!c.repeticao_a_vista(&b, &k, k.len(), &atk), "na historia: uma so' visita nao chega");
    }

    #[test]
    fn na_historia_conta_se_a_posicao_ja_se_repetiu() {
        let atk = Attacks::new();
        let c = Cuckoo::novo(crate::zobrist::tabelas(), &atk);
        let mut b = Board::startpos();
        let mut k = vec![b.hash];
        for m in ["g1f3", "g8f6", "f3g1", "f6g8", "g1f3", "g8f6", "f3g1"] {
            jogar(&mut b, &mut k, &atk, m);
        }
        // A posicao inicial ja' ocorreu duas vezes: f6g8 daria a terceira.
        assert!(c.repeticao_a_vista(&b, &k, k.len(), &atk));
    }

    #[test]
    fn nao_ve_repeticao_onde_nao_ha() {
        let atk = Attacks::new();
        let c = Cuckoo::novo(crate::zobrist::tabelas(), &atk);
        let mut b = Board::startpos();
        let mut k = vec![b.hash];
        for m in ["e2e4", "e7e5", "g1f3", "b8c6"] {
            jogar(&mut b, &mut k, &atk, m);
        }
        assert!(!c.repeticao_a_vista(&b, &k, 0, &atk));
    }

    #[test]
    fn a_jogada_do_adversario_nao_conta() {
        // Brancas a jogar depois de g1f3 g8f6 f3g1 f6g8: a posicao de ha' 3 plies (apos g8f6)
        // difere da actual por um lance dos PRETOS (f6<->g8). Nao e' um lance nosso.
        let atk = Attacks::new();
        let c = Cuckoo::novo(crate::zobrist::tabelas(), &atk);
        let mut b = Board::startpos();
        let mut k = vec![b.hash];
        for m in ["g1f3", "g8f6", "f3g1", "f6g8"] {
            jogar(&mut b, &mut k, &atk, m);
        }
        // Estamos na posicao inicial (2a visita), brancas a jogar. O unico "repetir" possivel
        // seria um lance das brancas que devolvesse g1f3-like: g1f3 leva a posicao do ply 1.
        // Isso E' um lance nosso e tem de ser visto; o lance dos pretos nao.
        assert!(c.repeticao_a_vista(&b, &k, 0, &atk));
    }
}
