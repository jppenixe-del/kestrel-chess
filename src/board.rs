use crate::attacks::*;
use crate::bitboard::*;
use crate::moves::*;
use crate::types::*;

pub const CASTLE_WK: u8 = 1;
pub const CASTLE_WQ: u8 = 2;
pub const CASTLE_BK: u8 = 4;
pub const CASTLE_BQ: u8 = 8;

#[derive(Clone)]
pub struct Board {
    pub pieces: [[Bitboard; 6]; 2], // [color][piece_type]
    pub occ_color: [Bitboard; 2],
    pub occ_all: Bitboard,
    pub side: Color,
    pub castling: u8,
    pub ep_square: Square,
    pub halfmove: u32,
    pub fullmove: u32,
    // Acumuladores incrementais de avaliacao (material+PST, perspetiva das
    // BRANCAS, mg/eg separados -- ver eval::piece_contribution()) --
    // mantidos por add_piece()/remove_piece() em vez de recalculados do
    // zero a cada chamada a evaluate(). `phase` conta so' pecas maiores
    // (ver eval::PHASE_INC), nao inclui peoes.
    pub mg_score: i32,
    pub eg_score: i32,
    pub phase: i32,
    // Mailbox O(1) -- piece_at() fazia uma varredura ate' 12 bitboards
    // (2 cores x 6 tipos) a cada chamada; era uma fatia real do tempo
    // total dentro de make_move/unmake_move (ver perf), alem de ser
    // usado em SEE. Mantido em sincronia por add_piece()/remove_piece().
    pub mailbox: [Option<(PieceType, Color)>; 64],
}

#[derive(Copy, Clone)]
pub struct Undo {
    pub captured: Option<(PieceType, Color)>,
    pub castling: u8,
    pub ep_square: Square,
    pub halfmove: u32,
    // Snapshot inteiro (nao deltas) -- restaurar em unmake_move() e'
    // sempre correcto por construcao, sem precisar de reverter cada
    // captura/promocao/roque individualmente.
    pub mg_score: i32,
    pub eg_score: i32,
    pub phase: i32,
}

/// Undo minimo para um null move (passar a vez): so' muda side + ep_square.
#[derive(Copy, Clone)]
pub struct NullUndo {
    pub ep_square: Square,
}

impl Board {
    pub fn startpos() -> Self {
        Self::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
    }

    pub fn from_fen(fen: &str) -> Self {
        let parts: Vec<&str> = fen.split_whitespace().collect();
        let mut pieces = [[0u64; 6]; 2];
        let mut mailbox: [Option<(PieceType, Color)>; 64] = [None; 64];
        let mut rank = 7i32;
        let mut file = 0i32;
        for ch in parts[0].chars() {
            match ch {
                '/' => {
                    rank -= 1;
                    file = 0;
                }
                '1'..='8' => {
                    file += ch.to_digit(10).unwrap() as i32;
                }
                c => {
                    let color = if c.is_ascii_uppercase() { Color::White } else { Color::Black };
                    let kind = match c.to_ascii_lowercase() {
                        'p' => PieceType::Pawn,
                        'n' => PieceType::Knight,
                        'b' => PieceType::Bishop,
                        'r' => PieceType::Rook,
                        'q' => PieceType::Queen,
                        'k' => PieceType::King,
                        _ => panic!("fen piece invalido: {}", c),
                    };
                    let s = sq(file as u8, rank as u8);
                    pieces[color.idx()][kind.idx()] |= bb(s);
                    mailbox[s as usize] = Some((kind, color));
                    file += 1;
                }
            }
        }
        let side = if parts.get(1) == Some(&"b") { Color::Black } else { Color::White };
        let mut castling = 0u8;
        if let Some(c) = parts.get(2) {
            if c.contains('K') {
                castling |= CASTLE_WK;
            }
            if c.contains('Q') {
                castling |= CASTLE_WQ;
            }
            if c.contains('k') {
                castling |= CASTLE_BK;
            }
            if c.contains('q') {
                castling |= CASTLE_BQ;
            }
        }
        let ep_square = match parts.get(3) {
            Some(s) if *s != "-" => parse_sq(s),
            _ => NO_SQUARE,
        };
        let halfmove = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
        let fullmove = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(1);

        let mut b = Board {
            pieces,
            occ_color: [0, 0],
            occ_all: 0,
            side,
            castling,
            ep_square,
            halfmove,
            fullmove,
            mg_score: 0,
            eg_score: 0,
            phase: 0,
            mailbox,
        };
        b.recompute_occ();
        b.recompute_eval_accumulators();
        b
    }

    pub fn recompute_occ(&mut self) {
        for c in [Color::White, Color::Black] {
            let mut o = 0u64;
            for pt in ALL_PIECES {
                o |= self.pieces[c.idx()][pt.idx()];
            }
            self.occ_color[c.idx()] = o;
        }
        self.occ_all = self.occ_color[0] | self.occ_color[1];
    }

    /// Recalcula mg_score/eg_score/phase do ZERO, percorrendo todas as
    /// pecas -- so' usado uma vez na construcao (from_fen); depois disso
    /// add_piece()/remove_piece() mantem os campos correctos
    /// incrementalmente.
    pub fn recompute_eval_accumulators(&mut self) {
        self.mg_score = 0;
        self.eg_score = 0;
        self.phase = 0;
        for c in [Color::White, Color::Black] {
            for pt in ALL_PIECES {
                let mut bbp = self.pieces[c.idx()][pt.idx()];
                while bbp != 0 {
                    let s = bbp.trailing_zeros() as Square;
                    bbp &= bbp - 1;
                    let (mg, eg, ph) = crate::eval::piece_contribution(pt, c, s);
                    self.mg_score += mg;
                    self.eg_score += eg;
                    self.phase += ph;
                }
            }
        }
    }

    #[inline]
    #[inline(always)]
    pub fn piece_at(&self, s: Square) -> Option<(PieceType, Color)> {
        self.mailbox[s as usize]
    }

    pub fn king_sq(&self, color: Color) -> Square {
        self.pieces[color.idx()][PieceType::King.idx()].trailing_zeros() as Square
    }

    pub fn is_square_attacked(&self, s: Square, by: Color, atk: &Attacks) -> bool {
        let occ = self.occ_all;
        // pawns: a pawn of `by` attacks `s` if s is in the pawn-attack set
        // of `by`'s color computed FROM s using the opposite color table
        // (symmetry trick: attacker squares = pawn_attacks[opp(by)][s] intersected with by's pawns)
        if atk.pawn[by.opp().idx()][s as usize] & self.pieces[by.idx()][PieceType::Pawn.idx()] != 0 {
            return true;
        }
        if atk.knight[s as usize] & self.pieces[by.idx()][PieceType::Knight.idx()] != 0 {
            return true;
        }
        if atk.king[s as usize] & self.pieces[by.idx()][PieceType::King.idx()] != 0 {
            return true;
        }
        let bishops_queens = self.pieces[by.idx()][PieceType::Bishop.idx()]
            | self.pieces[by.idx()][PieceType::Queen.idx()];
        if bishop_attacks(s, occ) & bishops_queens != 0 {
            return true;
        }
        let rooks_queens = self.pieces[by.idx()][PieceType::Rook.idx()]
            | self.pieces[by.idx()][PieceType::Queen.idx()];
        if rook_attacks(s, occ) & rooks_queens != 0 {
            return true;
        }
        false
    }

    pub fn in_check(&self, color: Color, atk: &Attacks) -> bool {
        self.is_square_attacked(self.king_sq(color), color.opp(), atk)
    }

    fn remove_piece(&mut self, pt: PieceType, c: Color, s: Square) {
        self.pieces[c.idx()][pt.idx()] &= !bb(s);
        self.occ_color[c.idx()] &= !bb(s);
        self.occ_all &= !bb(s);
        self.mailbox[s as usize] = None;
        let (mg, eg, ph) = crate::eval::piece_contribution(pt, c, s);
        self.mg_score -= mg;
        self.eg_score -= eg;
        self.phase -= ph;
    }
    fn add_piece(&mut self, pt: PieceType, c: Color, s: Square) {
        self.pieces[c.idx()][pt.idx()] |= bb(s);
        self.occ_color[c.idx()] |= bb(s);
        self.occ_all |= bb(s);
        self.mailbox[s as usize] = Some((pt, c));
        let (mg, eg, ph) = crate::eval::piece_contribution(pt, c, s);
        self.mg_score += mg;
        self.eg_score += eg;
        self.phase += ph;
    }

    /// Aplica um lance PSEUDO-LEGAL (a legalidade -- nao ficar em xeque --
    /// e' verificada por quem gera os lances, chamando in_check depois).
    pub fn make_move(&mut self, mv: &Move) -> Undo {
        let us = self.side;
        let them = us.opp();
        let (moving_pt, _) = self.piece_at(mv.from).expect("make_move: nada em from");
        let captured = if mv.flag == MoveFlag::EnPassant {
            let cap_sq = if us == Color::White { mv.to - 8 } else { mv.to + 8 };
            Some((PieceType::Pawn, them))
        } else {
            self.piece_at(mv.to)
        };

        let undo = Undo {
            captured,
            castling: self.castling,
            ep_square: self.ep_square,
            halfmove: self.halfmove,
            mg_score: self.mg_score,
            eg_score: self.eg_score,
            phase: self.phase,
        };

        // remove captured piece (normal or en passant)
        match mv.flag {
            MoveFlag::EnPassant => {
                let cap_sq = if us == Color::White { mv.to - 8 } else { mv.to + 8 };
                self.remove_piece(PieceType::Pawn, them, cap_sq);
            }
            _ => {
                if let Some((cpt, cc)) = captured {
                    self.remove_piece(cpt, cc, mv.to);
                }
            }
        }

        // move the piece
        self.remove_piece(moving_pt, us, mv.from);
        let final_pt = mv.promotion.unwrap_or(moving_pt);
        self.add_piece(final_pt, us, mv.to);

        // castling: move the rook too
        match mv.flag {
            MoveFlag::CastleKing => {
                let (rf, rt) = if us == Color::White { (7u8, 5u8) } else { (63u8, 61u8) };
                self.remove_piece(PieceType::Rook, us, rf);
                self.add_piece(PieceType::Rook, us, rt);
            }
            MoveFlag::CastleQueen => {
                let (rf, rt) = if us == Color::White { (0u8, 3u8) } else { (56u8, 59u8) };
                self.remove_piece(PieceType::Rook, us, rf);
                self.add_piece(PieceType::Rook, us, rt);
            }
            _ => {}
        }

        // en passant square update
        self.ep_square = if mv.flag == MoveFlag::DoublePush {
            if us == Color::White { mv.from + 8 } else { mv.from - 8 }
        } else {
            NO_SQUARE
        };

        // castling rights update
        if moving_pt == PieceType::King {
            if us == Color::White {
                self.castling &= !(CASTLE_WK | CASTLE_WQ);
            } else {
                self.castling &= !(CASTLE_BK | CASTLE_BQ);
            }
        }
        for s in [mv.from, mv.to] {
            match s {
                0 => self.castling &= !CASTLE_WQ,
                7 => self.castling &= !CASTLE_WK,
                56 => self.castling &= !CASTLE_BQ,
                63 => self.castling &= !CASTLE_BK,
                _ => {}
            }
        }

        // halfmove clock
        if moving_pt == PieceType::Pawn || captured.is_some() {
            self.halfmove = 0;
        } else {
            self.halfmove += 1;
        }
        if us == Color::Black {
            self.fullmove += 1;
        }

        self.side = them;
        undo
    }

    /// Passa a vez ao adversario sem mover peca (para null-move pruning).
    /// So' altera `side` e limpa `ep_square`; tudo o resto fica intacto.
    /// NUNCA chamar em xeque (o rei poderia ser "capturado" na resposta).
    pub fn make_null_move(&mut self) -> NullUndo {
        let undo = NullUndo { ep_square: self.ep_square };
        self.side = self.side.opp();
        self.ep_square = NO_SQUARE;
        undo
    }

    pub fn unmake_null_move(&mut self, undo: &NullUndo) {
        self.side = self.side.opp();
        self.ep_square = undo.ep_square;
    }

    pub fn unmake_move(&mut self, mv: &Move, undo: &Undo) {
        let them = self.side; // side that is about to move again = the one who just moved's opponent... wait: after make_move, self.side = opponent of mover. So "us" (who made mv) = self.side.opp()
        let us = them.opp();
        self.side = us;

        let (final_pt, _) = self.piece_at(mv.to).expect("unmake: nada em to");
        let moving_pt = if mv.promotion.is_some() { PieceType::Pawn } else { final_pt };

        self.remove_piece(final_pt, us, mv.to);
        self.add_piece(moving_pt, us, mv.from);

        match mv.flag {
            MoveFlag::EnPassant => {
                let cap_sq = if us == Color::White { mv.to - 8 } else { mv.to + 8 };
                self.add_piece(PieceType::Pawn, us.opp(), cap_sq);
            }
            MoveFlag::CastleKing => {
                let (rf, rt) = if us == Color::White { (7u8, 5u8) } else { (63u8, 61u8) };
                self.remove_piece(PieceType::Rook, us, rt);
                self.add_piece(PieceType::Rook, us, rf);
            }
            MoveFlag::CastleQueen => {
                let (rf, rt) = if us == Color::White { (0u8, 3u8) } else { (56u8, 59u8) };
                self.remove_piece(PieceType::Rook, us, rt);
                self.add_piece(PieceType::Rook, us, rf);
            }
            _ => {
                if let Some((cpt, cc)) = undo.captured {
                    self.add_piece(cpt, cc, mv.to);
                }
            }
        }

        self.castling = undo.castling;
        self.ep_square = undo.ep_square;
        self.halfmove = undo.halfmove;
        if us == Color::Black {
            self.fullmove -= 1;
        }
        // Restauro explicito (nao so' confiar nos remove/add_piece acima
        // se cancelarem exactamente): garante correccao mesmo que algum
        // caso futuro deixe de espelhar make_move perfeitamente.
        self.mg_score = undo.mg_score;
        self.eg_score = undo.eg_score;
        self.phase = undo.phase;
    }

    pub fn to_fen(&self) -> String {
        let mut s = String::new();
        for rank in (0..8i32).rev() {
            let mut empty = 0;
            for file in 0..8u8 {
                let sqi = sq(file, rank as u8);
                match self.piece_at(sqi) {
                    None => empty += 1,
                    Some((pt, c)) => {
                        if empty > 0 {
                            s.push_str(&empty.to_string());
                            empty = 0;
                        }
                        s.push(pt.to_char(c));
                    }
                }
            }
            if empty > 0 {
                s.push_str(&empty.to_string());
            }
            if rank > 0 {
                s.push('/');
            }
        }
        s.push(' ');
        s.push(if self.side == Color::White { 'w' } else { 'b' });
        s.push(' ');
        let mut cr = String::new();
        if self.castling & CASTLE_WK != 0 {
            cr.push('K');
        }
        if self.castling & CASTLE_WQ != 0 {
            cr.push('Q');
        }
        if self.castling & CASTLE_BK != 0 {
            cr.push('k');
        }
        if self.castling & CASTLE_BQ != 0 {
            cr.push('q');
        }
        s.push_str(if cr.is_empty() { "-" } else { &cr });
        s.push(' ');
        s.push_str(&sq_name(self.ep_square));
        s.push(' ');
        s.push_str(&self.halfmove.to_string());
        s.push(' ');
        s.push_str(&self.fullmove.to_string());
        s
    }
}
