//! Repeticao A` DISTANCIA DE UM LANCE, por tabela cuckoo.
//!
//! O problema: a busca so' sabe que uma posicao repete DEPOIS de a repetir. Se
//! o adversario pode forcar a repeticao a partir daqui, isso ja' e' um empate
//! garantido para ele -- e uma linha que a busca esta' a pontuar em +0,4 nao
//! vale +0,4 nenhum, porque ele nao tem de a jogar.
//!
//! A ideia (Marcel van Kervinck, "cuckoo hashing for repetition detection"):
//! um lance REVERSIVEL de peca (nao peao, nao captura) muda a chave zobrist
//! sempre da mesma maneira -- `peca[de] XOR peca[para] XOR lado`. Ha' um numero
//! pequeno de tais diferencas (todos os pares de-para de cada peca), portanto
//! podem ser pre-calculadas todas e guardadas numa tabela. Depois, para saber se
//! existe um lance daqui que devolve a uma posicao ja' vista, basta fazer o XOR
//! da chave de agora com a de entao e procurar o resultado na tabela.
//!
//! Escrito de raiz a partir da ideia. O KestrelStrike usa a implementacao
//! vendorizada da referencia, declarada nos creditos; aqui nao ha' vendor, e
//! por isso isto e' nosso -- a tabela, o hash duplo e a varredura do historico.
//!
//! MEDIDO no KestrelStrike: +10,34 +/- 5,59 em 4234 partidas.

use crate::attacks::Attacks;
use crate::board::Board;
use crate::types::{Color, Square};
use crate::zobrist::Zobrist;

const N: usize = 8192;

pub struct Cuckoo {
    /// A chave do LADO A JOGAR, guardada aqui para a varredura nao precisar do
    /// zobrist inteiro. A diferenca que se procura e' `agora ^ lado ^ entao`:
    /// as duas posicoes tem o mesmo lado a jogar, e a chave do lance traz o
    /// termo do lado la' dentro, portanto ele tem de entrar uma vez.
    lado: u64,
    chave: [u64; N],
    /// O lance que produz aquela diferenca, como (de, para).
    lance: [(Square, Square); N],
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
    /// Enche a tabela com TODOS os lances reversiveis de peca.
    ///
    /// Reversivel quer dizer: nao peao (um peao nao volta atras) e a peca fica
    /// na mesma casa de destino -- capturas ficam de fora porque mudam o
    /// material e a posicao nao pode repetir.
    ///
    /// A insercao e' "cuckoo": se o lugar estiver ocupado, o ocupante e'
    /// expulso e vai procurar o SEU outro lugar, e assim por diante. Com duas
    /// funcoes de hash e a tabela com folga, o ciclo acaba sempre.
    pub fn novo(zob: &Zobrist, atk: &Attacks) -> Self {
        let mut c = Cuckoo { lado: zob.side, chave: [0; N], lance: [(0, 0); N] };
        let mut postos = 0usize;
        // 1..=5: cavalo, bispo, torre, dama, rei. O 0 e' o peao e fica de fora.
        for cor in [Color::White, Color::Black] {
            let ic = cor as usize;
            for pt in 1usize..=5 {
                for de in 0u8..64 {
                    let alvos = match pt {
                        1 => atk.knight[de as usize],
                        2 => crate::attacks::bishop_attacks(de, 0),
                        3 => crate::attacks::rook_attacks(de, 0),
                        4 => crate::attacks::queen_attacks(de, 0),
                        _ => atk.king[de as usize],
                    };
                    let mut b = alvos;
                    while b != 0 {
                        let para = b.trailing_zeros() as u8;
                        b &= b - 1;
                        // Cada par conta uma vez: `de < para` evita o duplicado.
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
                        postos += 1;
                    }
                }
            }
        }
        debug_assert!(postos > 0);
        c
    }

    /// Ha' um lance daqui que devolve a uma posicao ja' vista nesta partida?
    ///
    /// `historico` sao as chaves da partida ate' agora, a mais recente no fim.
    /// `halfmove` e' o contador dos cinquenta lances: so' vale a pena procurar
    /// dentro dele, porque para la' houve captura ou lance de peao e a posicao
    /// nao pode repetir.
    pub fn repeticao_a_vista(&self, board: &Board, historico: &[u64], atk: &Attacks) -> bool {
        let fim = historico.len();
        let hm = board.halfmove as usize;
        if hm < 3 || fim < 3 {
            return false;
        }
        let agora = board.hash;
        let ocupadas = board.occ_all;
        // De dois em dois: so' as posicoes com o MESMO lado a jogar podem
        // repetir. Comeca-se em 3 porque uma repeticao precisa de pelo menos
        // tres meios-lances entre as duas visitas.
        let mut i = 3usize;
        while i <= hm.min(fim) {
            let antes = historico[fim - i];
            let dif = agora ^ self.lado ^ antes;
            let mut j = h1(dif);
            if self.chave[j] != dif {
                j = h2(dif);
                if self.chave[j] != dif {
                    i += 2;
                    continue;
                }
            }
            let (de, para) = self.lance[j];
            // O caminho entre as duas casas tem de estar livre -- ou entao uma
            // das pontas e' a peca que se mexe.
            if (atk.between[de as usize][para as usize] & ocupadas) == 0 {
                return true;
            }
            i += 2;
        }
        false
    }
}
