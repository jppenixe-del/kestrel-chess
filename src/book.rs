//! Livro de "assinatura" -- posicoes reais dos jogos da Judit Polgar
//! (fonte: pgnmentor.com/players/PolgarJ.zip, 1825 partidas reais, 100%
//! dela como jogadora, brancas ou pretas -- descarregado e documentado em
//! 2026-07-20). NAO substitui a busca -- so' da' um empurraozinho de
//! ordenacao/preferencia as jogadas que ela realmente jogou em posicoes
//! identicas, para o kestrel puxar para o "estilo dela" quando a busca
//! achar dois lances proximos em valor. Um lance claramente pior nunca
//! vence so' por estar no livro (o bonus e' pequeno face aos valores de
//! ordenacao normais -- ver search.rs).
use crate::moves::Move;
use crate::types::*;
use std::fs::File;
use std::io::Read;

pub const MAGIC: &[u8; 8] = b"KESTBK01";
const HDR: usize = 16;
const RECSZ: usize = 14; // key u64 BE + move16 u16 BE + count u32 BE

pub struct Book {
    data: Vec<u8>,
    n: usize,
}

pub fn encode_move(mv: &Move) -> u16 {
    let promo: u16 = match mv.promotion {
        Some(PieceType::Knight) => 1,
        Some(PieceType::Bishop) => 2,
        Some(PieceType::Rook) => 3,
        Some(PieceType::Queen) => 4,
        _ => 0,
    };
    (mv.from as u16) | ((mv.to as u16) << 6) | (promo << 12)
}

pub fn decode_move16(m16: u16) -> (Square, Square, Option<PieceType>) {
    let from = (m16 & 0x3F) as Square;
    let to = ((m16 >> 6) & 0x3F) as Square;
    let promo = match (m16 >> 12) & 0x7 {
        1 => Some(PieceType::Knight),
        2 => Some(PieceType::Bishop),
        3 => Some(PieceType::Rook),
        4 => Some(PieceType::Queen),
        _ => None,
    };
    (from, to, promo)
}

impl Book {
    pub fn load(path: &str) -> std::io::Result<Self> {
        let mut f = File::open(path)?;
        let mut data = Vec::new();
        f.read_to_end(&mut data)?;
        if data.len() < HDR || &data[0..8] != MAGIC {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "magic invalido"));
        }
        let n = u64::from_be_bytes(data[8..16].try_into().unwrap()) as usize;
        Ok(Book { data, n })
    }

    fn key_at(&self, i: usize) -> u64 {
        let off = HDR + i * RECSZ;
        u64::from_be_bytes(self.data[off..off + 8].try_into().unwrap())
    }

    /// Devolve ate' 8 (move16, count) para a chave, ou vazio.
    pub fn lookup(&self, key: u64) -> Vec<(u16, u32)> {
        let mut lo = 0usize;
        let mut hi = self.n;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if self.key_at(mid) < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let mut out = Vec::new();
        let mut i = lo;
        while i < self.n && self.key_at(i) == key {
            let off = HDR + i * RECSZ;
            let m16 = u16::from_be_bytes(self.data[off + 8..off + 10].try_into().unwrap());
            let cnt = u32::from_be_bytes(self.data[off + 10..off + 14].try_into().unwrap());
            out.push((m16, cnt));
            i += 1;
        }
        out
    }
}
