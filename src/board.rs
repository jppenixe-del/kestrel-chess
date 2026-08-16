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
    /// The network's accumulator, carried with the position.
    ///
    /// Here rather than in the searcher because every path that changes a
    /// piece already goes through `add_piece`/`remove_piece` -- putting it
    /// anywhere else would mean finding and patching castling, en passant and
    /// promotion separately, and one missed case is an evaluation that is
    /// silently wrong only in rare positions. `None` until a network is
    /// loaded, so the hand-written evaluation costs nothing for it.
    pub acc: Option<Box<crate::nnue::Accumulator>>,
    /// O mesmo para a rede v3 -- ver `nnue_v3::AccV3`. Independente do `acc`:
    /// qual das arquitecturas o `evaluate` le' decide-se pelo ficheiro que foi
    /// carregado, nao por uma bandeira de compilacao, portanto os dois
    /// acumuladores mantem-se vivos.
    pub acc_v3: Option<Box<crate::nnue_v3::AccV3>>,
    pub castling: u8,
    pub ep_square: Square,
    pub halfmove: u32,
    pub fullmove: u32,
    // Acumuladores incrementais de avaliacao (material+PST, perspetiva das
    // BRANCAS, mg/eg separados -- ver eval::piece_contribution()) --
    // mantidos por add_piece()/remove_piece() em vez de recalculados do
    // zero a cada chamada a evaluate(). `phase` conta so' pecas maiores
    // (ver eval::PHASE_INC), nao inclui peoes.
    /// O mesmo, mas com as PSQT lidas do ponto de vista de "o rei desta cor
    /// esta' no flanco do rei". Indexado [cor][flanco].
    ///
    /// Quatro somas em vez de uma porque o valor de cada casa passa a depender
    /// de onde esta' o rei DA PROPRIA COR -- e um lance de rei que atravesse a
    /// coluna e mudaria TODAS as pecas de uma vez, o que mataria o
    /// incremental. Mantendo as duas leituras sempre actualizadas, um lance de
    /// rei nao custa nada: e' so' passar a ler a outra.
    /// Um acumulador por bucket de contagem de peoes. Mantidos os oito em
    /// paralelo: assim um lance que mude o bucket -- uma captura de peao --
    /// nao custa nada, e' so' passar a ler outro indice.
    ///
    /// O ciclo de oito e' desenrolado pelo compilador e os arrays cabem na
    /// cache L1; o custo medido esta' na nota do commit.
    pub phase: i32,
    /// Zobrist key for this position, kept up to date move by move.
    ///
    /// Maintained here rather than recomputed by the search because the
    /// recompute walks all 32 pieces and the search wants a key at every
    /// node. See `zobrist::Zobrist::hash_completo` for why this only became
    /// worth doing once king moves stopped needing a make/unmake.
    pub hash: u64,
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
    pub phase: i32,
    /// Same reasoning as `phase`, and it is what makes the incremental hash
    /// cheap: undoing a move restores one u64 instead of replaying every XOR
    /// backwards, so only `make_move` ever pays.
    pub hash: u64,
}

/// Undo minimo para um null move (passar a vez): so' muda side + ep_square.
#[derive(Copy, Clone)]
pub struct NullUndo {
    pub ep_square: Square,
    pub hash: u64,
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
            acc: None,
            acc_v3: None,
            side,
            castling,
            ep_square,
            halfmove,
            fullmove,
            phase: 0,
            hash: 0,
            mailbox,
        };
        b.hash = crate::zobrist::tabelas().hash_completo(&b);
        b.recompute_occ();
        b.recompute_eval_accumulators();
        // Built once here, from the finished position. Every later change goes
        // through add_piece/remove_piece and updates it a piece at a time.
        if let Some(net) = crate::nnue::rede() {
            b.acc = Some(Box::new(crate::nnue::Accumulator::fresh(net, &b)));
        }
        if let Some(net) = crate::nnue_v3::rede() {
            b.acc_v3 = Some(Box::new(crate::nnue_v3::AccV3::fresh(net, &b)));
        }
        b
    }

    /// Repovoa o mailbox a partir dos bitboards. So' e' preciso quando um
    /// tabuleiro e' montado de fora (ler bitboards de um ficheiro, por
    /// exemplo) em vez de vir do from_fen ou de um make_move.
    pub fn rebuild_mailbox(&mut self) {
        self.mailbox = [None; 64];
        for c in [Color::White, Color::Black] {
            for pt in ALL_PIECES {
                let mut bb = self.pieces[c.idx()][pt.idx()];
                while bb != 0 {
                    let s = bb.trailing_zeros() as usize;
                    bb &= bb - 1;
                    self.mailbox[s] = Some((pt, c));
                }
            }
        }
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
        self.phase = 0;
        for c in [Color::White, Color::Black] {
            for pt in ALL_PIECES {
                let mut bbp = self.pieces[c.idx()][pt.idx()];
                while bbp != 0 {
                    let s = bbp.trailing_zeros() as Square;
                    bbp &= bbp - 1;
                    let ph = pt.phase_inc();
                    self.phase += ph;
                    // Os acumuladores novos tem de ser preenchidos AQUI
                    // tambem, senao o recalculo do zero da' zeros e a
                    // verificacao acusa uma divergencia que nao existe --
                    // ou pior, deixa de acusar as que existem.
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

    /// Would our king be in check on `to`, having come from `from`?
    ///
    /// Answers what `generate_legal` used to answer with a make/unmake, and
    /// the two differences from a plain `is_square_attacked(to, ...)` are
    /// exactly what made the naive version wrong:
    ///
    /// - **The king is lifted off `from` first.** Standing on the board it
    ///   blocks the very ray it would be fleeing along, so a king running
    ///   directly away from a rook or bishop would look safe on the new
    ///   square when it is still checked.
    /// - **A piece captured on `to` stops attacking.** It is removed from the
    ///   attacker sets, not merely stepped over -- otherwise capturing the
    ///   checker would look illegal because the dead piece still "attacks"
    ///   its own square.
    ///
    /// King moves are the largest group `generate_legal` could not settle
    /// without a make/unmake (up to eight per node, every node, even when not
    /// in check), and make/unmake is expensive here because it drags the
    /// board, the mailbox and the accumulator with it.
    pub fn king_move_leaves_check(&self, from: Square, to: Square, atk: &Attacks) -> bool {
        let by = self.side.opp();
        let occ = (self.occ_all & !bb(from)) | bb(to);
        // The captured piece, if any, is gone -- so drop `to` from every
        // attacker set below rather than only from the occupancy.
        let inimigos = !bb(to);
        if atk.pawn[by.opp().idx()][to as usize]
            & self.pieces[by.idx()][PieceType::Pawn.idx()]
            & inimigos
            != 0
        {
            return true;
        }
        if atk.knight[to as usize] & self.pieces[by.idx()][PieceType::Knight.idx()] & inimigos != 0 {
            return true;
        }
        if atk.king[to as usize] & self.pieces[by.idx()][PieceType::King.idx()] & inimigos != 0 {
            return true;
        }
        let bishops_queens = (self.pieces[by.idx()][PieceType::Bishop.idx()]
            | self.pieces[by.idx()][PieceType::Queen.idx()])
            & inimigos;
        if bishop_attacks(to, occ) & bishops_queens != 0 {
            return true;
        }
        let rooks_queens = (self.pieces[by.idx()][PieceType::Rook.idx()]
            | self.pieces[by.idx()][PieceType::Queen.idx()])
            & inimigos;
        if rook_attacks(to, occ) & rooks_queens != 0 {
            return true;
        }
        false
    }

    pub fn in_check(&self, color: Color, atk: &Attacks) -> bool {
        self.is_square_attacked(self.king_sq(color), color.opp(), atk)
    }

    pub(crate) fn remove_piece(&mut self, pt: PieceType, c: Color, s: Square) {
        self.pieces[c.idx()][pt.idx()] &= !bb(s);
        self.occ_color[c.idx()] &= !bb(s);
        self.occ_all &= !bb(s);
        self.mailbox[s as usize] = None;
        self.hash ^= crate::zobrist::tabelas().piece_sq[c.idx()][pt.idx()][s as usize];
        let ph = pt.phase_inc();
        if let (Some(a), Some(net)) = (self.acc.as_mut(), crate::nnue::rede()) {
            a.push_dirty(net, c, pt, s, false);
        }
        if let (Some(a), Some(net)) = (self.acc_v3.as_mut(), crate::nnue_v3::rede()) {
            a.remove_piece(net, pt, c, s, self.occ_all, self.pieces, self.mailbox);
        }
        // Os acumuladores por flanco so' sao LIDOS com a feature `psqtmirror`
        // ligada (ver `material_pst_white`). Sem ela isto era trabalho morto
        // pago em cada peca colocada ou retirada, ou seja, em cada lance da
        // busca -- duas consultas de tabela por peca para um valor que
        // ninguem consultava. Sendo uma const de compilacao, com a feature
        // desligada o compilador apaga o bloco por inteiro.
        self.phase -= ph;
    }
    fn add_piece(&mut self, pt: PieceType, c: Color, s: Square) {
        self.pieces[c.idx()][pt.idx()] |= bb(s);
        self.occ_color[c.idx()] |= bb(s);
        self.occ_all |= bb(s);
        self.mailbox[s as usize] = Some((pt, c));
        self.hash ^= crate::zobrist::tabelas().piece_sq[c.idx()][pt.idx()][s as usize];
        let ph = pt.phase_inc();
        if let (Some(a), Some(net)) = (self.acc.as_mut(), crate::nnue::rede()) {
            a.push_dirty(net, c, pt, s, true);
        }
        if let (Some(a), Some(net)) = (self.acc_v3.as_mut(), crate::nnue_v3::rede()) {
            a.add_piece(net, pt, c, s, self.occ_all, self.pieces, self.mailbox);
        }
        // Os acumuladores por flanco so' sao LIDOS com a feature `psqtmirror`
        // ligada (ver `material_pst_white`). Sem ela isto era trabalho morto
        // pago em cada peca colocada ou retirada, ou seja, em cada lance da
        // busca -- duas consultas de tabela por peca para um valor que
        // ninguem consultava. Sendo uma const de compilacao, com a feature
        // desligada o compilador apaga o bloco por inteiro.
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
            phase: self.phase,
            hash: self.hash,
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
        // Everything a Zobrist key depends on that is NOT a piece on a
        // square. The pieces took care of themselves in add_piece/
        // remove_piece; these are the three pieces of state that also
        // belong in the key, and each is XORed out at its old value and in
        // at its new one. Castling rights are indexed by the whole 4-bit
        // mask rather than per-right, so one XOR pair covers any number of
        // rights lost at once.
        let z = crate::zobrist::tabelas();
        self.hash ^= z.side;
        if undo.castling != self.castling {
            self.hash ^= z.castling[(undo.castling & 0xF) as usize];
            self.hash ^= z.castling[(self.castling & 0xF) as usize];
        }
        if undo.ep_square != NO_SQUARE {
            self.hash ^= z.ep_file[file_of(undo.ep_square) as usize];
        }
        if self.ep_square != NO_SQUARE {
            self.hash ^= z.ep_file[file_of(self.ep_square) as usize];
        }
        // A king that crossed a bucket boundary invalidates every feature for
        // its own perspective: the same piece on the same square is a
        // different input number now. Everything above updated the
        // accumulator under the OLD bucket, so that perspective has to be
        // rebuilt -- but from the cache, not from nothing. See CacheRefresh.
        self.corrige_bucket();
        // Regista este ply para o acumulador da rede SF: ele precisa do
        // tabuleiro de CADA ply, nao so' dos que a busca avalia -- sem isso nao
        // ha' como encadear deltas atraves de nos nunca avaliados, e a busca so'
        // avalia ~0.65 nos por no'.
        crate::nnue_sf::regista_ply(&self.pieces);
        undo
    }

    /// Rebuild a perspective whose king changed bucket, through the cache.
    ///
    /// Cheap to call and cheap to skip: with an unbucketed network it returns
    /// immediately, and with a bucketed one it does nothing unless a boundary
    /// was actually crossed -- measured at 16.5% of all moves, which is why it
    /// goes through the cache rather than rebuilding from the bias.
    fn corrige_bucket(&mut self) {
        // A guarda da rede SIMPLES nao pode barrar as outras.
        //
        // Isto era `let net = match rede() { Some(n) if n.buckets > 1 => n,
        // _ => return }` -- um `return` no topo. Sem `KESTREL_NNUE` definido a
        // funcao saia na primeira linha e o `fix_bucket` da v3, que vive no
        // fim, nunca corria: os buckets de rei dela nunca se corrigiam. 2849
        // divergencias em 3906 lances, todas a partir do primeiro rei que sai
        // da casa inicial.
        //
        // Mesma familia do bug do `busy` na ponte e do `unmake_move` sem
        // correccao de bucket: uma condicao de UM caminho a decidir por todos.
        let pecas = self.pieces;
        if let Some(net) = crate::nnue::rede().filter(|n| n.buckets > 1) {
            self.corrige_bucket_simples(net);
        }
        if let (Some(net), Some(acc)) = (crate::nnue_v3::rede(), self.acc_v3.as_mut()) {
            acc.fix_bucket(net, pecas);
        }
    }

    fn corrige_bucket_simples(&mut self, net: &'static crate::nnue::Network) {
        // Read everything the refresh needs BEFORE taking the accumulator,
        // because the accumulator lives inside the same struct. Rust says so
        // and it is right to: reading the board through a stale copy taken
        // around a mutable borrow is how an accumulator ends up describing a
        // position that no longer exists.
        let pecas = self.pieces;
        let rb = self.king_sq(Color::White);
        let rp = self.king_sq(Color::Black);
        let quer = [
            crate::nnue::bucket_do_rei_de(self, Color::White),
            crate::nnue::bucket_do_rei_de(self, Color::Black),
        ];
        // The mirror travels with the bucket: both are functions of the king
        // square, and a king crossing the d/e file changes the mirror even
        // when it stays inside the same bucket.
        let quer_esp = [
            crate::nnue::espelha_perspectiva(self, Color::White),
            crate::nnue::espelha_perspectiva(self, Color::Black),
        ];
        let acc = match self.acc.as_mut() {
            Some(a) => a,
            None => return,
        };
        for cor in [Color::White, Color::Black] {
            let quer = quer[cor.idx()];
            let esp = quer_esp[cor.idx()];
            if acc.bucket[cor.idx()] == quer && acc.espelha[cor.idx()] == esp {
                continue;
            }
            // The refresh below rewrites every column for this perspective, so
            // it must not run against values that are still missing recorded
            // changes -- and the recorded changes are indexed under the OLD
            // bucket, which is about to stop existing. Fold them in first.
            acc.materialise(net);
            acc.bucket[cor.idx()] = quer;
            acc.espelha[cor.idx()] = esp;
            let destino: &mut [i16; crate::nnue::HIDDEN] = if cor == Color::White {
                &mut acc.white
            } else {
                &mut acc.black
            };
            crate::nnue::com_cache(|c| c.refresca(net, pecas, rb, rp, cor, quer, esp, destino));
        }
    }

    /// Passa a vez ao adversario sem mover peca (para null-move pruning).
    /// So' altera `side` e limpa `ep_square`; tudo o resto fica intacto.
    /// NUNCA chamar em xeque (o rei poderia ser "capturado" na resposta).
    pub fn make_null_move(&mut self) -> NullUndo {
        let undo = NullUndo { ep_square: self.ep_square, hash: self.hash };
        self.side = self.side.opp();
        self.ep_square = NO_SQUARE;
        let z = crate::zobrist::tabelas();
        self.hash ^= z.side;
        if undo.ep_square != NO_SQUARE {
            self.hash ^= z.ep_file[file_of(undo.ep_square) as usize];
        }
        undo
    }

    pub fn unmake_null_move(&mut self, undo: &NullUndo) {
        self.side = self.side.opp();
        self.ep_square = undo.ep_square;
        self.hash = undo.hash;
    }

    pub fn unmake_move(&mut self, mv: &Move, undo: &Undo) {
        crate::nnue_sf::desregista_ply();
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
        self.phase = undo.phase;
        // One assignment instead of replaying every XOR backwards -- and it
        // also repairs whatever the add_piece/remove_piece calls above did to
        // the key while restoring the board.
        self.hash = undo.hash;
        // make_move ends with corrige_bucket(); unmake_move never did. After
        // undoing a king move that crossed a bucket boundary, the accumulator
        // described the king on the right square under the WRONG bucket, and
        // any evaluation asked for in that window silently read weights for a
        // king that is not there. Only actually does work when the bucket
        // really changed -- same guard make_move relies on -- so it costs
        // nothing on the moves that are not king moves.
        self.corrige_bucket();
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
