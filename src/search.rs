use crate::attacks::{bishop_attacks, rook_attacks, Attacks};
use crate::bitboard::bb;
use crate::board::Board;
use crate::book::{encode_move, Book};
use crate::eval::evaluate;
use crate::movegen::generate_legal;
use crate::moves::{Move, MoveFlag};
use crate::tt::{Bound, TranspositionTable};
use crate::types::{file_of, rank_of, sq, Color, PieceType};
use crate::zobrist::Zobrist;
use std::time::Instant;

pub const MATE_SCORE: i32 = 30000;
pub const MAX_PLY: usize = 128;

/// Limite de saturacao da history heuristic (bonus/malus acumulados por
/// [cor][from][to]) -- evita que um par from/to muito bem sucedido
/// domine a ordenacao para sempre, sem precisar de "aging"/decay mais
/// complexo.
const HISTORY_MAX: i32 = 16000;

#[derive(Copy, Clone)]
pub struct SearchLimits {
    pub deadline: Option<Instant>,
    pub max_depth: i32,
    pub max_nodes: Option<u64>,
}

pub struct Searcher<'a> {
    pub atk: &'a Attacks,
    pub zob: &'a Zobrist,
    pub tt: &'a TranspositionTable,
    pub nodes: u64,
    pub limits: SearchLimits,
    pub stop: bool,
    pub history: Vec<u64>, // hashes da partida real ate' agora (para repeticao)
    pub killers: [[Option<Move>; 2]; MAX_PLY],
    /// History heuristic ("butterfly boards" classicos): [cor][from][to],
    /// bonus quando um lance tranquilo causa um corte beta, malus nos
    /// lances tranquilos experimentados antes dele no MESMO no' que nao
    /// cortaram -- peca canonica que faltava por completo (so' havia
    /// TT-move/MVV-LVA/killers/livro; todos os outros lances tranquilos
    /// ficavam sem NENHUM sinal de ordenacao). 2026-07-20, ver
    /// project_kestrel_achados_2026-07-20.md. Zerada uma vez por `go`
    /// (o Searcher e' reconstruido a cada `go` em uci.rs), nunca a meio
    /// da busca -- a mesma licao do bug de killers corrigido antes.
    pub history_scores: [[[i32; 64]; 64]; 2],
    /// Countermove heuristic: indexed by [piece type][to square] of the
    /// move that led INTO this node (the opponent's last move) -> a quiet
    /// move that previously caused a beta cutoff in reply to that exact
    /// context. Gives a move-ordering bonus below killers, above plain
    /// history, when the current candidate matches the recorded reply.
    pub countermoves: [[Option<Move>; 64]; 6],
    /// For each ply, the (piece type, to-square) of the move that was
    /// played to reach that ply (i.e. the opponent's last move as seen
    /// from this node) -- set by the parent right before recursing, read
    /// by order_moves() to look up `countermoves`.
    pub ply_last_move: [Option<(PieceType, crate::types::Square)>; MAX_PLY],
    pub root_best: Option<Move>,
    /// MultiPV via the "exclusion" method: root moves listed here are
    /// dropped from the root's legal-move list, so a repeated search at
    /// the same position finds the next-best line instead of the same
    /// one. Empty during normal single-PV search (no behavior change).
    pub excluded_root_moves: Vec<Move>,
    // Livro de "assinatura" da Judit Polgar (ver book.rs) -- so' influencia
    // a ORDEM em que a busca experimenta os lances, nunca substitui a
    // avaliacao real. None se o livro nao carregou (o motor continua a
    // funcionar normalmente sem ele).
    pub style_book: Option<&'a Book>,
}

/// Limiar a partir do qual um score e' considerado "de mate" (nao so'
/// avaliacao normal) -- MATE_SCORE menos a profundidade maxima possivel,
/// para nao confundir avaliacoes normais muito altas com mates reais.
const MATE_THRESHOLD: i32 = MATE_SCORE - MAX_PLY as i32;

/// 2026-07-20 (BUG REAL encontrado por auditoria -- investigacao da
/// queda de resultados, ver NOTAS_PROXIMA_SESSAO.md): a TT guardava e
/// lia scores de mate em BRUTO, sem ajustar pela distancia (ply) entre
/// o no' onde a entrada foi escrita e o no' onde e' reaproveitada --
/// bug classico de "corrupcao de mate score" em qualquer motor
/// alfa-beta com TT. Um "mate em N" escrito a um ply e' relativo a ESSE
/// ply; reaproveitado sem ajuste noutro ply, o motor pode "ver" mates
/// que nao existem dali, ou avaliar mal posicoes decisivas perto de
/// mate -- exatamente onde um estilo agressivo (Polgar) mais precisa de
/// avaliacoes corretas. Converte para "distancia ao no' ATUAL" antes de
/// guardar, converte de volta para "distancia a partir da raiz real"
/// (ou seja, para a escala que negamax() usa) ao ler.
fn score_to_tt(score: i32, ply: i32) -> i32 {
    if score >= MATE_THRESHOLD {
        score + ply
    } else if score <= -MATE_THRESHOLD {
        score - ply
    } else {
        score
    }
}
fn score_from_tt(score: i32, ply: i32) -> i32 {
    if score >= MATE_THRESHOLD {
        score - ply
    } else if score <= -MATE_THRESHOLD {
        score + ply
    } else {
        score
    }
}

impl<'a> Searcher<'a> {
    /// Reconstructs the full principal variation by walking the TT's
    /// best-move chain from `board` forward. Not a dedicated PV table --
    /// cheap and good enough for UCI `info ... pv` output and for
    /// verifying deep/forced lines (e.g. long mates) actually hold up
    /// move by move, not just at the root. Defensive against a stale or
    /// hash-collided entry pointing at an illegal move (stops the line
    /// there instead of applying it) and against cycles (a repetition
    /// loop in a corrupted chain would otherwise iterate forever).
    pub fn extract_pv(&self, board: &Board, max_len: usize) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut b = board.clone();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..max_len {
            let hash = self.zob.hash(&b);
            if !seen.insert(hash) {
                break;
            }
            let mv = match self.tt.probe(hash).and_then(|e| e.best) {
                Some(m) => m,
                None => break,
            };
            let legal = generate_legal(&b, self.atk);
            if !legal.contains(&mv) {
                break;
            }
            b.make_move(&mv);
            pv.push(mv);
        }
        pv
    }

    fn time_up(&mut self) -> bool {
        if self.stop {
            return true;
        }
        if self.nodes % 2048 == 0 {
            if let Some(d) = self.limits.deadline {
                if Instant::now() >= d {
                    self.stop = true;
                }
            }
            if let Some(mx) = self.limits.max_nodes {
                if self.nodes >= mx {
                    self.stop = true;
                }
            }
        }
        self.stop
    }

    fn is_repetition_or_fifty(&self, board: &Board, hash: u64) -> bool {
        if board.halfmove >= 100 {
            return true;
        }
        // conta ocorrencias da mesma posicao no historico real + no
        // caminho de busca ja percorrido (self.history acumula ambos)
        let mut cnt = 0;
        for &h in self.history.iter().rev().take(board.halfmove as usize + 1) {
            if h == hash {
                cnt += 1;
                if cnt >= 1 {
                    return true; // repeticao simples ja chega para evitar linhas de empate a repetir
                }
            }
        }
        false
    }

    /// O lado a jogar tem alguma peca alem de peoes e rei?
    /// (Condicao anti-zugzwang para o null-move pruning.)
    fn has_non_pawn_material(&self, board: &Board) -> bool {
        let us = board.side.idx();
        board.pieces[us][PieceType::Knight.idx()]
            | board.pieces[us][PieceType::Bishop.idx()]
            | board.pieces[us][PieceType::Rook.idx()]
            | board.pieces[us][PieceType::Queen.idx()]
            != 0
    }

    fn mvv_lva(&self, board: &Board, mv: &Move) -> i32 {
        if !mv.is_capture() {
            return 0;
        }
        let victim = board.piece_at(mv.to).map(|(pt, _)| pt.value()).unwrap_or(100); // en passant = peao
        let attacker = board.piece_at(mv.from).map(|(pt, _)| pt.value()).unwrap_or(0);
        victim * 16 - attacker
    }

    /// Todas as pecas (ambas as cores) que atacam `sq` dada uma
    /// ocupacao HIPOTETICA `occ` (nao necessariamente `board.occ_all`
    /// -- usado pelo SEE para simular a troca a medida que remove
    /// pecas). Ataques de peao usam a tabela do lado CONTRARIO (truque
    /// classico: "que casas atacaria um peao preto aqui" = "que peoes
    /// brancos atacam aqui", por simetria do padrao diagonal).
    fn attackers_to(&self, board: &Board, s: crate::types::Square, occ: crate::bitboard::Bitboard) -> crate::bitboard::Bitboard {
        let a = self.atk;
        let mut att = 0u64;
        att |= a.pawn[Color::Black.idx()][s as usize] & board.pieces[Color::White.idx()][PieceType::Pawn.idx()];
        att |= a.pawn[Color::White.idx()][s as usize] & board.pieces[Color::Black.idx()][PieceType::Pawn.idx()];
        att |= a.knight[s as usize]
            & (board.pieces[Color::White.idx()][PieceType::Knight.idx()] | board.pieces[Color::Black.idx()][PieceType::Knight.idx()]);
        att |= a.king[s as usize]
            & (board.pieces[Color::White.idx()][PieceType::King.idx()] | board.pieces[Color::Black.idx()][PieceType::King.idx()]);
        let diag = board.pieces[Color::White.idx()][PieceType::Bishop.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Bishop.idx()]
            | board.pieces[Color::White.idx()][PieceType::Queen.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Queen.idx()];
        att |= bishop_attacks(s, occ) & diag;
        let orth = board.pieces[Color::White.idx()][PieceType::Rook.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Rook.idx()]
            | board.pieces[Color::White.idx()][PieceType::Queen.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Queen.idx()];
        att |= rook_attacks(s, occ) & orth;
        att & occ
    }

    fn least_valuable_attacker(
        &self,
        board: &Board,
        attackers: crate::bitboard::Bitboard,
        side: Color,
    ) -> Option<(crate::types::Square, PieceType)> {
        for pt in [
            PieceType::Pawn,
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Rook,
            PieceType::Queen,
            PieceType::King,
        ] {
            let bbp = attackers & board.pieces[side.idx()][pt.idx()];
            if bbp != 0 {
                return Some((bbp.trailing_zeros() as crate::types::Square, pt));
            }
        }
        None
    }

    /// Static Exchange Evaluation: simula a sequencia completa de
    /// capturas/recapturas na casa `mv.to`, sempre com o atacante menos
    /// valioso de cada lado (a jogada optima para ambos), e devolve o
    /// ganho material líquido assumindo optimo jogo de ambos os lados
    /// (cada lado escolhe parar ou continuar a troca, o que for melhor
    /// para si -- minimax classico sobre a "swap list"). Nao verifica
    /// se a recaptura deixaria o proprio rei em xeque (limitacao
    /// standard/aceite de SEE simples, presente em praticamente todos
    /// os motores). So' chamar em lances de captura (incl. en passant).
    fn see(&self, board: &Board, mv: &Move) -> i32 {
        let to = mv.to;
        let Some((attacker_pt0, attacker_color0)) = board.piece_at(mv.from) else {
            return 0;
        };
        let victim_val0 = if mv.flag == MoveFlag::EnPassant {
            PieceType::Pawn.value()
        } else {
            match board.piece_at(to) {
                Some((pt, _)) => pt.value(),
                None => return 0,
            }
        };

        let mut occ = board.occ_all;
        occ &= !bb(mv.from);
        if mv.flag == MoveFlag::EnPassant {
            let ep_captured = sq(file_of(to), rank_of(mv.from));
            occ &= !bb(ep_captured);
        }

        let mut gains: Vec<i32> = vec![victim_val0];
        let mut attacker_val = attacker_pt0.value();
        let mut side = attacker_color0.opp();

        loop {
            let attackers = self.attackers_to(board, to, occ);
            let side_attackers = attackers & board.occ_color[side.idx()];
            let Some((lva_sq, lva_pt)) = self.least_valuable_attacker(board, side_attackers, side) else {
                break;
            };
            gains.push(attacker_val - *gains.last().unwrap());
            attacker_val = lva_pt.value();
            occ &= !bb(lva_sq);
            side = side.opp();
            if gains.len() > 32 {
                break;
            }
        }

        for i in (1..gains.len()).rev() {
            gains[i - 1] = (-gains[i]).min(gains[i - 1]);
        }
        gains[0]
    }

    /// Bonus de ordenacao para lances que a Judit Polgar realmente jogou
    /// nesta posicao exata (1825 jogos reais, ver book.rs) -- cresce com
    /// a frequencia mas satura, para nunca competir com uma captura
    /// claramente boa (MVV-LVA fica sempre a frente). So' um empurrao de
    /// preferencia entre lances tranquilos que a busca ja consideraria
    /// razoaveis de qualquer forma. Recebe o hash JA CALCULADO (nunca o
    /// recalcula por lance -- bug de desempenho real corrigido: chegou a
    /// custar 3x o NPS por recalcular o zobrist inteiro por CANDIDATO em
    /// vez de uma vez por posicao).
    fn book_bonus(&self, book_entries: &[(u16, u32)], mv: &Move) -> i32 {
        if book_entries.is_empty() {
            return 0;
        }
        let target = encode_move(mv);
        for &(m16, cnt) in book_entries {
            if m16 == target {
                return 550 + (cnt as i32 * 10).min(200);
            }
        }
        0
    }

    /// Aplica bonus/malus de history heuristic -- ver campo `history_scores`.
    /// `depth*depth` e' a formula classica (peso maior quanto mais fundo o
    /// corte, um corte a profundidade alta diz muito mais sobre a
    /// qualidade real do lance do que um corte raso).
    fn update_history(&mut self, side: usize, mv: &Move, delta: i32) {
        let v = &mut self.history_scores[side][mv.from as usize][mv.to as usize];
        *v = (*v + delta).clamp(-HISTORY_MAX, HISTORY_MAX);
    }

    fn order_moves(&self, board: &Board, mut moves: Vec<Move>, tt_move: Option<Move>, ply: usize, hash: Option<u64>) -> Vec<Move> {
        let killers = self.killers[ply];
        let side = board.side.idx();
        let book_entries: Vec<(u16, u32)> = match (self.style_book, hash) {
            (Some(b), Some(h)) => b.lookup(h),
            _ => Vec::new(),
        };
        // Countermove heuristic: look up whether there's a recorded reply
        // for the exact context that led into this node (the opponent's
        // last move, piece type + destination square).
        let countermove = self
            .ply_last_move
            .get(ply)
            .and_then(|x| *x)
            .and_then(|(pt, to)| self.countermoves[pt.idx()][to as usize]);
        moves.sort_by_key(|m| {
            if Some(*m) == tt_move {
                -1_000_000
            } else if m.is_capture() {
                // SEE replaces plain MVV-LVA for ordering: good/neutral
                // captures (SEE>=0) go to the top, ranked by the real
                // exchange value (not just "bigger piece first"); bad
                // captures (SEE<0, lose material in the full exchange)
                // sink below quiet moves -- MVV-LVA couldn't tell "Bxf7"
                // against a defended bishop (loses the piece) apart from
                // a genuinely good capture.
                let see = self.see(board, m);
                if see >= 0 {
                    -200_000 - see
                } else {
                    100_000 - see
                }
            } else if Some(*m) == killers[0] {
                -700 - self.book_bonus(&book_entries, m)
            } else if Some(*m) == killers[1] {
                -600 - self.book_bonus(&book_entries, m)
            } else {
                // Countermove folded in as an ADDITIVE bonus on top of
                // history, not a hard priority slot -- a single recorded
                // reply can be wrong; letting it outrank every other
                // quiet move unconditionally (as a fixed slot did) can
                // force a bad move to the front. Real engines (see
                // Sirius's "continuation history") treat this as a
                // weighted signal blended into the ordinary history
                // score, not a rigid tier -- same idea here, simplified
                // to a single ply-lag instead of Sirius's multi-lag sum.
                let h = self.history_scores[side][m.from as usize][m.to as usize];
                let cm_bonus = if Some(*m) == countermove { 2000 } else { 0 };
                -h - cm_bonus - self.book_bonus(&book_entries, m)
            }
        });
        moves
    }

    fn quiescence(&mut self, board: &mut Board, alpha: i32, beta: i32, ply: usize) -> i32 {
        let stand_pat = crate::eval::evaluate_fast(board);
        self.quiescence_from(board, alpha, beta, ply, stand_pat)
    }

    /// Nucleo da quiescence, recebendo o stand-pat ja' calculado (completo
    /// na 1a chamada vinda do negamax, rapido nas recursoes seguintes --
    /// ver negamax()).
    fn quiescence_from(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: usize, stand_pat: i32) -> i32 {
        self.nodes += 1;
        if self.time_up() {
            return stand_pat;
        }
        if stand_pat >= beta {
            return beta;
        }
        if stand_pat > alpha {
            alpha = stand_pat;
        }
        if ply >= MAX_PLY - 1 {
            return stand_pat;
        }

        let mut moves = generate_legal(board, self.atk);
        moves.retain(|m| m.is_capture() || m.promotion == Some(PieceType::Queen));
        // Poda por SEE: uma captura que perde material na troca completa
        // (SEE negativo) quase nunca vale a pena dentro da quiescence --
        // e' exactamente o tipo de "captura mal calculada" que antes
        // era sempre pesquisada (MVV-LVA nao filtra nada, so' ordena).
        // Promocoes de dama ficam sempre (mv.to nao e' captura nesse
        // caso, is_capture()==false, so' entram aqui por causa do
        // OR acima -- SEE nao se aplica, `is_capture()` protege isso).
        moves.retain(|m| !m.is_capture() || self.see(board, m) >= 0);
        let moves = self.order_moves(board, moves, None, ply.min(MAX_PLY - 1), None);

        for mv in moves {
            let undo = board.make_move(&mv);
            let score = -self.quiescence(board, -beta, -alpha, ply + 1);
            board.unmake_move(&mv, &undo);
            if self.stop {
                return alpha;
            }
            if score >= beta {
                return beta;
            }
            if score > alpha {
                alpha = score;
            }
        }
        alpha
    }

    fn negamax(&mut self, board: &mut Board, depth: i32, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        self.nodes += 1;
        if self.time_up() {
            return 0;
        }

        let mut beta = beta;

        let hash = self.zob.hash(board);
        if ply > 0 && self.is_repetition_or_fifty(board, hash) {
            return 0;
        }

        // Mate distance pruning: se um mate mais curto do que o melhor
        // possivel a este ply ja' esta' garantido/impossivel de bater,
        // aperta a janela -- corte trivial e sempre correcto (nao
        // interfere com scores normais, so' com scores de mate).
        let mating_value = MATE_SCORE - ply as i32;
        if mating_value < beta {
            beta = mating_value;
            if alpha >= mating_value {
                return mating_value;
            }
        }
        let mated_value = -MATE_SCORE + ply as i32;
        if mated_value > alpha {
            alpha = mated_value;
            if beta <= mated_value {
                return mated_value;
            }
        }

        let orig_alpha = alpha;
        let mut tt_move = None;
        if let Some(e) = self.tt.probe(hash) {
            tt_move = e.best;
            // score_from_tt(): converte o score guardado (relativo ao
            // no' onde foi escrito) para a escala deste no' -- ver nota
            // grande junto de score_to_tt/score_from_tt.
            let tt_score = score_from_tt(e.score, ply as i32);
            // MultiPV: a stored root entry can point at (or bound around)
            // a move we're deliberately excluding for this line -- skip
            // every TT-based shortcut/adjustment at the root while an
            // exclusion list is active, so the real move loop below
            // (which already filters excluded_root_moves) is always
            // reached instead of returning a cached result that ignores
            // the exclusion.
            let multipv_guard = ply == 0 && !self.excluded_root_moves.is_empty();
            if e.depth >= depth && !multipv_guard {
                match e.bound {
                    Bound::Exact => {
                        // 2026-07-20 (BUG REAL corrigido -- achado por
                        // instrumentacao directa num jogo real onde o
                        // motor jogou o "primeiro lance legal gerado" em
                        // vez do lance realmente escolhido pela busca,
                        // numa posicao completamente ganha): quando a TT
                        // ja tem um bound Exact suficiente para a raiz
                        // (ply==0), esta funcao retorna aqui SEM NUNCA
                        // passar pelo loop de lances mais abaixo -- que e'
                        // o unico sitio onde `self.root_best` era
                        // definido. Em jogos longos (TT acumulada ao
                        // longo de muitos `go`), isto podia fazer VARIAS
                        // iteracoes da iterative deepening (todas com
                        // `e.depth` >= profundidade pedida) devolverem
                        // sem NUNCA definir root_best, deixando toda a
                        // decisao do lance final refem da ULTIMA
                        // iteracao -- e se essa tambem fosse interrompida
                        // a meio (ver bug irmao em iterative_deepening()),
                        // `root_best` ficava None e o motor caia no
                        // fallback "primeiro lance legal", ignorando
                        // completamente o que a busca sabia.
                        if ply == 0 {
                            if let Some(tm) = tt_move {
                                self.root_best = Some(tm);
                            }
                        }
                        return tt_score;
                    }
                    Bound::Lower => {
                        if tt_score > alpha {
                            alpha = tt_score;
                        }
                    }
                    // 2026-07-20 (BUG REAL corrigido -- ver nota grande
                    // acima do ScoreFromTT): faltava apertar "beta" aqui
                    // -- o ramo "Upper" real de um alfa-beta com TT
                    // sempre aperta o limite CONTRARIO ao que "Lower"
                    // aperta (Lower sobe alpha, Upper desce beta), para
                    // o corte combinado "alpha>=beta" logo a seguir
                    // conseguir mesmo cortar quando aplicavel. O corpo
                    // vazio anterior fazia este ramo nunca contribuir
                    // para nenhum corte.
                    Bound::Upper => {
                        if tt_score < beta {
                            beta = tt_score;
                        }
                    }
                }
                if alpha >= beta {
                    if ply == 0 {
                        if let Some(tm) = tt_move {
                            self.root_best = Some(tm);
                        }
                    }
                    return tt_score;
                }
            }
        }

        if depth <= 0 {
            // Ponto de entrada na quiescence: usa a avaliacao COMPLETA
            // (com os termos "Polgar") uma unica vez aqui, como stand-pat
            // inicial -- e' aqui que a riqueza posicional realmente
            // influencia a busca. Dentro da propria quiescence (resolucao
            // de capturas, que pode ter varios nos), usa-se a versao
            // rapida (ver quiescence()) para nao pagar o custo repetido.
            let full_stand_pat = evaluate(board);
            return self.quiescence_from(board, alpha, beta, ply, full_stand_pat);
        }

        let in_check = board.in_check(board.side, self.atk);

        // Reverse futility pruning (static null move): se a avaliacao
        // estatica rapida ja' esta' tao acima de beta que nem uma
        // margem generosa por profundidade a apanha, a posicao e' boa
        // demais para precisar de busca real -- corta. So' em nos nao-
        // raiz, fora de xeque, profundidade baixa (a margem cresce
        // linear com a profundidade, torna-se pouco fiavel depressa) e
        // longe de scores de mate (nao mascarar mates reais).
        if !in_check
            && ply > 0
            && depth <= 6
            && beta.abs() < MATE_SCORE - MAX_PLY as i32
        {
            let margin = 90 * depth;
            let static_eval = crate::eval::evaluate_fast(board);
            if static_eval - margin >= beta {
                return static_eval - margin;
            }
        }

        // Null-move pruning: se mesmo passando a vez ao adversario ainda
        // ficamos >= beta numa busca reduzida, a posicao e' tao boa que
        // podemos cortar ja'. Condicoes de seguranca:
        //  - nao em xeque (passar a vez em xeque e' ilegal/absurdo)
        //  - profundidade suficiente para a busca reduzida ter significado
        //  - lado a jogar tem pelo menos uma peca maior que peao (evita
        //    zugzwang, tipico de finais de peoes)
        //  - beta longe de scores de mate (nao mascarar mates)
        //  - nunca na raiz (ply > 0), para root_best ser sempre definido
        if depth >= 3
            && !in_check
            && ply > 0
            && beta.abs() < MATE_SCORE - MAX_PLY as i32
            && self.has_non_pawn_material(board)
        {
            // Adaptive R: a deeper reduction pays off at high depth (the
            // reduced-depth probe is still informative enough relative to
            // a bigger remaining tree), same idea as adaptive LMR below.
            let null_r = if depth > 6 { 3 } else { 2 };
            let undo = board.make_null_move();
            let score = -self.negamax(board, depth - 1 - null_r, -beta, -beta + 1, ply + 1);
            board.unmake_null_move(&undo);
            if self.stop {
                return 0;
            }
            if score >= beta {
                return beta;
            }
        }

        // Razoring: a profundidade muito baixa, se a avaliacao estatica
        // mais uma margem generosa ainda fica abaixo de alfa, e' muito
        // improvavel que exista um lance tranquilo que recupere a
        // diferenca -- verifica-se com uma chamada real a quiescence
        // (nao um corte cego) e so' se aceita o resultado se confirmar
        // o fail-low, para nunca perder uma tactica real.
        if !in_check && ply > 0 && depth <= 3 {
            let margin = 150 + 100 * (depth - 1);
            let static_eval = crate::eval::evaluate_fast(board);
            if static_eval + margin <= alpha {
                let full_stand_pat = evaluate(board);
                let q = self.quiescence_from(board, alpha, beta, ply, full_stand_pat);
                if q <= alpha {
                    return q;
                }
            }
        }

        // Internal Iterative Deepening: sem lance da TT para ordenar por
        // (tipico em nos que nunca foram visitados a esta profundidade),
        // uma pesquisa reduzida barata da' um lance de ordenacao muito
        // melhor do que a ordem crua do gerador -- mais cortes beta mais
        // cedo no loop principal abaixo. So' compensa a profundidades
        // razoaveis (a reducao tem de deixar sobrar pesquisa real) e nunca
        // em xeque (ja' e' extendido, o proprio xeque restringe as opcoes).
        if tt_move.is_none() && depth >= 4 && !in_check {
            self.negamax(board, depth - 2, alpha, beta, ply);
            if self.stop {
                return 0;
            }
            if let Some(e) = self.tt.probe(hash) {
                tt_move = e.best;
            }
        }

        let mut moves = generate_legal(board, self.atk);
        if moves.is_empty() {
            return if in_check { -MATE_SCORE + ply as i32 } else { 0 };
        }
        // MultiPV support (simple exclusion method): at the root only,
        // drop moves already reported by a previous MultiPV line so the
        // next call finds the next-best line instead of repeating the
        // same move. No effect on normal single-PV search (the list is
        // empty then).
        if ply == 0 && !self.excluded_root_moves.is_empty() {
            moves.retain(|m| !self.excluded_root_moves.contains(m));
            if moves.is_empty() {
                return if in_check { -MATE_SCORE + ply as i32 } else { 0 };
            }
        }
        // Staged move picker: substitui o `order_moves` + `for mv in
        // moves` que pontuava TUDO upfront antes de sequer tentar o
        // primeiro lance. Ver `MovePicker` no fim deste ficheiro para as
        // fases e a motivacao (port directo do padrao do Sirius).
        let killers = self.killers[ply.min(MAX_PLY - 1)];
        let mut picker = MovePicker::new(moves, tt_move, killers);

        let mut best_score = -MATE_SCORE - 1;
        let mut best_move = None;
        // Lances tranquilos experimentados neste no' ate' agora, para
        // aplicar malus de history heuristic se um lance POSTERIOR causar
        // o corte beta (ver update_history/history_scores).
        let mut quiets_tried: Vec<Move> = Vec::new();
        let mut futility_eval: Option<i32> = None;
        self.history.push(hash);
        let mut i: usize = 0;
        while let Some(mv) = picker.next_move(self, board, ply.min(MAX_PLY - 1), hash) {
            // Futility pruning: a profundidade baixa, lances tranquilos
            // que nem com uma margem generosa da avaliacao estatica
            // conseguem bater alfa raramente valem a pena explorar --
            // salta-os sem pesquisar. Nunca no 1o lance (pode ser o
            // melhor), nunca em xeque/captura/promocao, nunca perto de
            // scores de mate (a avaliacao estatica nao e' fiavel ai).
            if i > 0
                && !in_check
                && depth <= 6
                && !mv.is_capture()
                && mv.promotion.is_none()
                && alpha.abs() < MATE_SCORE - MAX_PLY as i32
            {
                let margin = 100 * depth;
                let fe = *futility_eval.get_or_insert_with(|| crate::eval::evaluate_fast(board));
                if fe + margin <= alpha {
                    i += 1;
                    continue;
                }
            }
            let undo = board.make_move(&mv);
            if ply + 1 < MAX_PLY {
                if let Some((moved_pt, _)) = board.piece_at(mv.to) {
                    self.ply_last_move[ply + 1] = Some((moved_pt, mv.to));
                }
            }
            let extend = if in_check { 1 } else { 0 };
            let score = if i == 0 {
                -self.negamax(board, depth - 1 + extend, -beta, -alpha, ply + 1)
            } else {
                // LMR: lances tardios na ordenacao (>= 4) tendem a ser maus;
                // pesquisa-os com profundidade reduzida primeiro. Nao reduzir
                // capturas, promocoes, lances que dao xeque, nem quando
                // estamos a escapar de xeque (extend == 1).
                let gives_check = board.in_check(board.side, self.atk);
                let r = if i >= 4
                    && depth >= 3
                    && extend == 0
                    && !mv.is_capture()
                    && mv.promotion.is_none()
                    && !gives_check
                {
                    // Adaptive: lances mais tardios e nos mais profundos
                    // toleram uma reducao maior -- a probabilidade deste
                    // lance ser o melhor cai com a posicao na ordenacao,
                    // e a arvore restante e' grande o suficiente para a
                    // reducao extra ainda deixar profundidade real.
                    let mut r = 1;
                    if depth >= 6 {
                        r += 1;
                    }
                    if i >= 10 {
                        r += 1;
                    }
                    r.min(depth - 1)
                } else {
                    0
                };
                // PVS: janela nula primeiro (reduzida se LMR), re-pesquisa se prometedor
                let mut s = -self.negamax(board, depth - 1 + extend - r, -alpha - 1, -alpha, ply + 1);
                if r > 0 && s > alpha && !self.stop {
                    // a versao reduzida bateu alpha: re-pesquisa a profundidade completa
                    s = -self.negamax(board, depth - 1 + extend, -alpha - 1, -alpha, ply + 1);
                }
                if s > alpha && s < beta && !self.stop {
                    s = -self.negamax(board, depth - 1 + extend, -beta, -alpha, ply + 1)
                }
                s
            };
            board.unmake_move(&mv, &undo);
            if !mv.is_capture() {
                quiets_tried.push(mv);
            }

            // BUG corrigido (2026-07-20, achado num jogo real na Arena --
            // "bestmove 0000" a meio de uma posicao completamente ganha):
            // a busca do 1o lance-filho pode terminar e devolver um
            // resultado valido no EXATO momento em que o relogio esgota
            // (self.stop passa a true dentro da recursao). O codigo antigo
            // verificava self.stop ANTES de guardar o resultado, deitando
            // fora um lance perfeitamente valido -- se isto acontecesse em
            // TODAS as profundidades (incl. profundidade 1), root_best
            // nunca chegava a ser definido e o motor devolvia lance nulo.
            // Agora guarda-se sempre o resultado do lance que JA terminou;
            // so' se para de explorar MAIS lances depois disso.
            if score > best_score {
                best_score = score;
                best_move = Some(mv);
                if ply == 0 {
                    self.root_best = Some(mv);
                }
            }
            if self.stop {
                self.history.pop();
                return best_score;
            }
            if score > alpha {
                alpha = score;
            }
            if alpha >= beta {
                if !mv.is_capture() && ply < MAX_PLY {
                    let k = &mut self.killers[ply];
                    if k[0] != Some(mv) {
                        k[1] = k[0];
                        k[0] = Some(mv);
                    }
                    // History heuristic: bonus para o lance que cortou,
                    // malus para os lances tranquilos anteriores neste
                    // no' que NAO cortaram (quiets_tried inclui `mv` como
                    // ultimo elemento, ja' que foi empurrado logo acima --
                    // excluido do malus).
                    let bonus = (depth * depth).min(HISTORY_MAX);
                    let side = board.side.idx();
                    self.update_history(side, &mv, bonus);
                    let n = quiets_tried.len().saturating_sub(1);
                    for qm in &quiets_tried[..n] {
                        self.update_history(side, qm, -bonus);
                    }
                    // Countermove heuristic: record this quiet move as
                    // the reply to whatever context led into this node
                    // (the opponent's last move, tracked via
                    // ply_last_move) -- read back in order_moves() the
                    // next time that exact context appears.
                    if let Some((ctx_pt, ctx_to)) = self.ply_last_move[ply] {
                        self.countermoves[ctx_pt.idx()][ctx_to as usize] = Some(mv);
                    }
                }
                break;
            }
            i += 1;
        }
        self.history.pop();

        let bound = if best_score <= orig_alpha {
            Bound::Upper
        } else if best_score >= beta {
            Bound::Lower
        } else {
            Bound::Exact
        };
        // score_to_tt(): guarda relativo a ESTE no' (nao a raiz) -- ver
        // nota grande junto de score_to_tt/score_from_tt.
        self.tt.store(hash, depth, score_to_tt(best_score, ply as i32), bound, best_move);

        best_score
    }

    /// Busca na raiz com aspiration windows: profundidade 1 usa sempre
    /// janela total (referencia inicial). Profundidades seguintes tentam
    /// primeiro uma janela estreita centrada no score da iteracao
    /// anterior -- corta muito mais no resto da arvore -- e alarga
    /// (dobra o delta) e repete se falhar por baixo ou por cima, ate'
    /// obter um score dentro da janela ou o tempo esgotar. Testada
    /// isoladamente com resultado negativo (33%); reintroduzida em lote
    /// com futility/RFP/razoring/mate-distance-pruning para testar
    /// possivel sinergia (pedido explicito do utilizador -- pecas
    /// individuais podem parecer negativas isoladas mas positivas em
    /// conjunto).
    fn search_root(&mut self, board: &mut Board, depth: i32, prev_score: i32) -> i32 {
        if depth <= 1 {
            return self.negamax(board, depth, -MATE_SCORE - 1, MATE_SCORE + 1, 0);
        }
        let mut delta: i32 = 25;
        let mut alpha = (prev_score - delta).max(-MATE_SCORE - 1);
        let mut beta = (prev_score + delta).min(MATE_SCORE + 1);
        loop {
            let score = self.negamax(board, depth, alpha, beta, 0);
            if self.stop {
                return score;
            }
            if score <= alpha {
                alpha = (alpha - delta).max(-MATE_SCORE - 1);
                delta *= 2;
            } else if score >= beta {
                beta = (beta + delta).min(MATE_SCORE + 1);
                delta *= 2;
            } else {
                return score;
            }
        }
    }

    pub fn iterative_deepening(&mut self, board: &mut Board) -> (Option<Move>, i32, i32, u64) {
        let mut best_move = None;
        let mut best_score = 0;
        let mut last_depth = 0;
        let mut prev_score = 0;
        self.killers = [[None; 2]; MAX_PLY];
        for depth in 1..=self.limits.max_depth {
            let score = self.search_root(board, depth, prev_score);
            // 2026-07-20 (BUG REAL corrigido -- irmao do bug ja' corrigido
            // dentro do loop de lances de negamax(), "nunca descartar o
            // resultado de um lance-filho ja' terminado so' porque o
            // relogio esgotou a seguir"): aqui a mesma logica falhava um
            // nivel acima -- `if self.stop && depth > 1 { break; }`
            // acontecia ANTES de ler `self.root_best` para `best_move`,
            // descartando uma iteracao que TINHA encontrado um lance
            // valido (root_best ja' actualizado dentro de negamax()) so'
            // porque o relogio esgotou a meio de um lance POSTERIOR dessa
            // mesma iteracao. Reproduzido num jogo real: motor acabou a
            // jogar o "primeiro lance legal gerado" (fallback de
            // uci.rs::cmd_go) em vez do lance vencedor que a busca ja'
            // tinha encontrado e guardado em root_best.
            if let Some(rb) = self.root_best {
                best_move = Some(rb);
                best_score = score;
                last_depth = depth;
                prev_score = score;
            }
            if self.stop {
                break;
            }
        }
        (best_move, best_score, last_depth, self.nodes)
    }
}

/// Staged move picker -- port directo do padrao usado no Sirius
/// (move_ordering.cpp `MovePickStage`, ver esse ficheiro para as fases
/// exactas). Ideia: em vez de pontuar TODOS os lances legais upfront
/// (SEE em todas as capturas, history+livro+countermove em todos os
/// quietos) antes de sequer tentar o primeiro, devolver os lances por
/// fases e pontuar SO' o subconjunto que a fase actual precisa. Se um
/// corte beta acontece no TT-move (muito comum quando a TT tem info),
/// nao pagamos NENHUM SEE nem lookup de history. Se um good-noisy corta,
/// nao pagamos NENHUM history nem lookup de livro.
///
/// Correccao preservada: gera todos os lances LEGAIS uma vez a
/// construcao (mesmo `generate_legal` de antes), so' muda a ordem/
/// timing de pontuacao. `MovePicker::next` devolve `None` quando todos
/// os lances foram devolvidos -- o chamador so' precisa de saber quantos
/// devolveu para distinguir mate/stalemate de fim de loop normal.
#[derive(Copy, Clone, PartialEq, Eq)]
enum PickerStage {
    TtMove,
    ScoreNoisy,
    GoodNoisy,
    Killer1,
    Killer2,
    ScoreQuiet,
    Quiet,
    BadNoisy,
    Done,
}

pub struct MovePicker {
    stage: PickerStage,
    tt_move: Option<Move>,
    killer1: Option<Move>,
    killer2: Option<Move>,
    /// noisy = capturas + promocoes. Pontuado com SEE (ver `score_noisy`)
    /// so' quando entramos em `ScoreNoisy`. Cada entrada guarda o lance
    /// e o SEE score correspondente; SEE>=0 vao primeiro (GoodNoisy),
    /// SEE<0 vao no fim (BadNoisy).
    noisy: Vec<(Move, i32)>,
    noisy_idx: usize,
    /// Marca onde acabam os good noisy (SEE>=0) e comecam os bad noisy
    /// (SEE<0). Definido quando `ScoreNoisy` termina.
    good_noisy_end: usize,
    /// quiet = tudo o que nao e' captura nem promocao. Pontuado com
    /// history + livro + countermove (ver `score_quiet`) so' quando
    /// entramos em `ScoreQuiet`.
    quiet: Vec<(Move, i32)>,
    quiet_idx: usize,
}

impl MovePicker {
    /// `excluded` (usado no MultiPV, ver `excluded_root_moves`) tem de ser
    /// filtrado ANTES da construcao do picker -- basta o chamador passar
    /// `moves` ja' filtrado; o picker nao conhece MultiPV.
    pub fn new(moves: Vec<Move>, tt_move: Option<Move>, killers: [Option<Move>; 2]) -> Self {
        // Separa capturas/promocoes de quietos numa unica passagem.
        // MoveFlag::EnPassant e captura; promocoes contam sempre como
        // noisy (mesmo sem captura -- a promocao propria e "material").
        let mut noisy: Vec<(Move, i32)> = Vec::with_capacity(moves.len() / 4);
        let mut quiet: Vec<(Move, i32)> = Vec::with_capacity(moves.len());
        for m in moves {
            if m.is_capture() || m.promotion.is_some() {
                noisy.push((m, 0));
            } else {
                quiet.push((m, 0));
            }
        }
        MovePicker {
            stage: PickerStage::TtMove,
            tt_move,
            killer1: killers[0],
            killer2: killers[1],
            noisy,
            noisy_idx: 0,
            good_noisy_end: 0,
            quiet,
            quiet_idx: 0,
        }
    }

    /// Devolve o proximo lance ou `None` quando nao ha mais nada.
    /// `searcher` e usado para SEE (na fase ScoreNoisy) e para
    /// history/livro/countermove (na fase ScoreQuiet). `ply`/`hash` sao
    /// os do no' actual (para lookup de livro por posicao, igual ao
    /// order_moves antigo).
    pub fn next_move(
        &mut self,
        searcher: &Searcher,
        board: &Board,
        ply: usize,
        hash: u64,
    ) -> Option<Move> {
        loop {
            match self.stage {
                PickerStage::TtMove => {
                    self.stage = PickerStage::ScoreNoisy;
                    if let Some(tm) = self.tt_move {
                        // TT-move so' e valido se estiver na lista real
                        // de lances legais (a TT pode conter lixo por
                        // colisao de hash). Procura em noisy+quiet.
                        if self.contains_move(tm) {
                            return Some(tm);
                        }
                    }
                }
                PickerStage::ScoreNoisy => {
                    // Pontua SEE de cada captura, uma unica vez. Nao ha
                    // MVV-LVA em separado -- SEE ja engloba a ideia de
                    // "captura de peca grande com peca pequena", e ainda
                    // rejeita capturas que aparentam ganhar mas perdem no
                    // full exchange (Bxf7 defendido).
                    for i in 0..self.noisy.len() {
                        let m = self.noisy[i].0;
                        if Some(m) == self.tt_move {
                            self.noisy[i].1 = i32::MIN; // marca para saltar depois
                            continue;
                        }
                        self.noisy[i].1 = searcher.see(board, &m);
                    }
                    // Nao ordenamos o vector agora -- selection-sort
                    // in-place em `GoodNoisy` e `BadNoisy` extrai o
                    // maior de cada vez, evitando O(n log n) upfront
                    // quando muitas vezes so' precisamos do primeiro.
                    self.stage = PickerStage::GoodNoisy;
                }
                PickerStage::GoodNoisy => {
                    if let Some(m) = self.pick_best_noisy(true) {
                        return Some(m);
                    }
                    // Terminou os good noisy; a partir daqui o
                    // `noisy_idx` marca o inicio dos bad noisy (que
                    // ficam para o fim).
                    self.good_noisy_end = self.noisy_idx;
                    self.stage = PickerStage::Killer1;
                }
                PickerStage::Killer1 => {
                    self.stage = PickerStage::Killer2;
                    if let Some(k) = self.killer1 {
                        if Some(k) != self.tt_move && self.quiet_contains(k) {
                            self.mark_quiet_used(k);
                            return Some(k);
                        }
                    }
                }
                PickerStage::Killer2 => {
                    self.stage = PickerStage::ScoreQuiet;
                    if let Some(k) = self.killer2 {
                        if Some(k) != self.tt_move
                            && Some(k) != self.killer1
                            && self.quiet_contains(k)
                        {
                            self.mark_quiet_used(k);
                            return Some(k);
                        }
                    }
                }
                PickerStage::ScoreQuiet => {
                    // Livro e' pesquisado por posicao (nao por lance),
                    // por isso uma unica vez aqui em vez de N vezes
                    // no loop.
                    let side = board.side.idx();
                    let book_entries: Vec<(u16, u32)> = match searcher.style_book {
                        Some(b) => b.lookup(hash),
                        None => Vec::new(),
                    };
                    let countermove = searcher
                        .ply_last_move
                        .get(ply)
                        .and_then(|x| *x)
                        .and_then(|(pt, to)| searcher.countermoves[pt.idx()][to as usize]);
                    for i in 0..self.quiet.len() {
                        let m = self.quiet[i].0;
                        if m.from == m.to {
                            // marcador "ja usado" (killer, ver mark_quiet_used)
                            self.quiet[i].1 = i32::MIN;
                            continue;
                        }
                        if Some(m) == self.tt_move {
                            self.quiet[i].1 = i32::MIN;
                            continue;
                        }
                        // Mesma formula do order_moves antigo (para nao
                        // regredir a ordenacao entre iteracoes):
                        // history + countermove bonus + livro.
                        let h = searcher.history_scores[side][m.from as usize][m.to as usize];
                        let cm_bonus = if Some(m) == countermove { 2000 } else { 0 };
                        let book = searcher.book_bonus(&book_entries, &m);
                        self.quiet[i].1 = h + cm_bonus + book;
                    }
                    self.stage = PickerStage::Quiet;
                }
                PickerStage::Quiet => {
                    if let Some(m) = self.pick_best_quiet() {
                        return Some(m);
                    }
                    self.stage = PickerStage::BadNoisy;
                }
                PickerStage::BadNoisy => {
                    if let Some(m) = self.pick_best_noisy(false) {
                        return Some(m);
                    }
                    self.stage = PickerStage::Done;
                }
                PickerStage::Done => return None,
            }
        }
    }

    /// Se `m` (o lance sugerido) ainda estiver nas listas geradas,
    /// devolve true. Serve para validar TT-move e killers antes de os
    /// emitir -- ambos podem ser lixo (TT colisao ou killer stale que
    /// ja nao aplica a esta posicao).
    fn contains_move(&self, m: Move) -> bool {
        self.noisy.iter().any(|(x, _)| *x == m) || self.quiet.iter().any(|(x, _)| *x == m)
    }
    fn quiet_contains(&self, m: Move) -> bool {
        self.quiet.iter().any(|(x, _)| *x == m)
    }

    /// Marca um quiet como "ja usado" (usei-o como killer, nao repetir
    /// mais tarde no stage Quiet). Truque: guardar `from == to`, que
    /// nunca acontece num lance real; o loop de ScoreQuiet trata como
    /// score = MIN e o pick_best_quiet salta-o.
    fn mark_quiet_used(&mut self, m: Move) {
        for entry in self.quiet.iter_mut() {
            if entry.0 == m {
                entry.0 = Move {
                    from: 0,
                    to: 0,
                    promotion: None,
                    flag: MoveFlag::Quiet,
                };
                return;
            }
        }
    }

    /// Selection-sort in-place: encontra o de maior score a partir de
    /// `noisy_idx`, faz swap para essa posicao, avanca. Devolve o lance;
    /// respeita a fase (good_only=true so' devolve SEE>=0, senao so'
    /// devolve SEE<0). Se nao ha mais na fase actual, devolve None.
    fn pick_best_noisy(&mut self, good_only: bool) -> Option<Move> {
        while self.noisy_idx < self.noisy.len() {
            let mut best_i = self.noisy_idx;
            let mut best_score = self.noisy[best_i].1;
            for i in (self.noisy_idx + 1)..self.noisy.len() {
                if self.noisy[i].1 > best_score {
                    best_score = self.noisy[i].1;
                    best_i = i;
                }
            }
            self.noisy.swap(self.noisy_idx, best_i);
            let (m, score) = self.noisy[self.noisy_idx];
            // score == i32::MIN significa "e' o TT-move, salta"
            if score == i32::MIN {
                self.noisy_idx += 1;
                continue;
            }
            // Boundary entre good e bad: good tem SEE>=0.
            if good_only {
                if score < 0 {
                    return None;
                }
            } else if score >= 0 {
                // Nao devia acontecer (todos os good_noisy ja foram
                // devolvidos), mas por defesa, salta -- os good ja
                // foram devolvidos por definicao.
                self.noisy_idx += 1;
                continue;
            }
            self.noisy_idx += 1;
            return Some(m);
        }
        None
    }

    fn pick_best_quiet(&mut self) -> Option<Move> {
        while self.quiet_idx < self.quiet.len() {
            let mut best_i = self.quiet_idx;
            let mut best_score = self.quiet[best_i].1;
            for i in (self.quiet_idx + 1)..self.quiet.len() {
                if self.quiet[i].1 > best_score {
                    best_score = self.quiet[i].1;
                    best_i = i;
                }
            }
            self.quiet.swap(self.quiet_idx, best_i);
            let (m, score) = self.quiet[self.quiet_idx];
            if score == i32::MIN {
                self.quiet_idx += 1;
                continue;
            }
            self.quiet_idx += 1;
            return Some(m);
        }
        None
    }
}
