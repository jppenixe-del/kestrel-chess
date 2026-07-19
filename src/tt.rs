use crate::moves::Move;

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Copy, Clone)]
pub struct TtEntry {
    pub key: u64,
    pub depth: i32,
    pub score: i32,
    pub bound: Bound,
    pub best: Option<Move>,
}

pub struct TranspositionTable {
    entries: Vec<Option<TtEntry>>,
    mask: usize,
}

impl TranspositionTable {
    pub fn new(mb: usize) -> Self {
        let bytes = mb * 1024 * 1024;
        let entry_size = std::mem::size_of::<Option<TtEntry>>();
        let mut count = (bytes / entry_size).max(1024);
        count = count.next_power_of_two() / 2; // fica um pouco abaixo do teto
        if count == 0 {
            count = 1024;
        }
        TranspositionTable { entries: vec![None; count], mask: count - 1 }
    }

    #[inline]
    pub fn probe(&self, key: u64) -> Option<TtEntry> {
        let idx = (key as usize) & self.mask;
        match self.entries[idx] {
            Some(e) if e.key == key => Some(e),
            _ => None,
        }
    }

    #[inline]
    pub fn store(&mut self, key: u64, depth: i32, score: i32, bound: Bound, best: Option<Move>) {
        let idx = (key as usize) & self.mask;
        // sempre substitui (simples; substituicao por profundidade fica
        // para uma versao seguinte)
        self.entries[idx] = Some(TtEntry { key, depth, score, bound, best });
    }

    pub fn clear(&mut self) {
        for e in self.entries.iter_mut() {
            *e = None;
        }
    }
}
