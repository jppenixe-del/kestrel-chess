use crate::attacks::Attacks;
use crate::board::Board;
use crate::book::{encode_move, Book};
use crate::eval::evaluate;
use crate::movegen::generate_legal;
use crate::moves::Move;
use crate::tt::{Bound, TranspositionTable};
use crate::types::PieceType;
use crate::zobrist::Zobrist;
use std::time::Instant;

pub const MATE_SCORE: i32 = 30000;
pub const MAX_PLY: usize = 128;

/// Limite de saturacao da history heuristic (bonus/malus acumulados por
/// [cor][from][to]) -- evita que um par from/to muito bem sucedido
/// domine a ordenacao para sempre, sem precisar de "aging"/decay mais
/// complexo.
const HISTORY_MAX: i32 = 16000;

pub struct SearchLimits {
    pub deadline: Option<Instant>,
    pub max_depth: i32,
    pub max_nodes: Option<u64>,
}

pub struct Searcher<'a> {
    pub atk: &'a Attacks,
    pub zob: &'a Zobrist,
    pub tt: &'a mut TranspositionTable,
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
    pub root_best: Option<Move>,
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
        moves.sort_by_key(|m| {
            if Some(*m) == tt_move {
                -1_000_000
            } else if m.is_capture() {
                -100_000 - self.mvv_lva(board, m)
            } else if Some(*m) == killers[0] {
                -700 - self.book_bonus(&book_entries, m)
            } else if Some(*m) == killers[1] {
                -600 - self.book_bonus(&book_entries, m)
            } else {
                let h = self.history_scores[side][m.from as usize][m.to as usize];
                -h - self.book_bonus(&book_entries, m)
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

        let hash = self.zob.hash(board);
        if ply > 0 && self.is_repetition_or_fifty(board, hash) {
            return 0;
        }

        let orig_alpha = alpha;
        let mut beta = beta;
        let mut tt_move = None;
        if let Some(e) = self.tt.probe(hash) {
            tt_move = e.best;
            // score_from_tt(): converte o score guardado (relativo ao
            // no' onde foi escrito) para a escala deste no' -- ver nota
            // grande junto de score_to_tt/score_from_tt.
            let tt_score = score_from_tt(e.score, ply as i32);
            if e.depth >= depth {
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
            const NULL_R: i32 = 2;
            let undo = board.make_null_move();
            let score = -self.negamax(board, depth - 1 - NULL_R, -beta, -beta + 1, ply + 1);
            board.unmake_null_move(&undo);
            if self.stop {
                return 0;
            }
            if score >= beta {
                return beta;
            }
        }

        let moves = generate_legal(board, self.atk);
        if moves.is_empty() {
            return if in_check { -MATE_SCORE + ply as i32 } else { 0 };
        }
        let moves = self.order_moves(board, moves, tt_move, ply.min(MAX_PLY - 1), Some(hash));

        let mut best_score = -MATE_SCORE - 1;
        let mut best_move = None;
        // Lances tranquilos experimentados neste no' ate' agora, para
        // aplicar malus de history heuristic se um lance POSTERIOR causar
        // o corte beta (ver update_history/history_scores).
        let mut quiets_tried: Vec<Move> = Vec::new();
        self.history.push(hash);
        for (i, mv) in moves.iter().enumerate() {
            let undo = board.make_move(mv);
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
                    1
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
            board.unmake_move(mv, &undo);
            if !mv.is_capture() {
                quiets_tried.push(*mv);
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
                best_move = Some(*mv);
                if ply == 0 {
                    self.root_best = Some(*mv);
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
                    if k[0] != Some(*mv) {
                        k[1] = k[0];
                        k[0] = Some(*mv);
                    }
                    // History heuristic: bonus para o lance que cortou,
                    // malus para os lances tranquilos anteriores neste
                    // no' que NAO cortaram (quiets_tried inclui `mv` como
                    // ultimo elemento, ja' que foi empurrado logo acima --
                    // excluido do malus).
                    let bonus = (depth * depth).min(HISTORY_MAX);
                    let side = board.side.idx();
                    self.update_history(side, mv, bonus);
                    let n = quiets_tried.len().saturating_sub(1);
                    for qm in &quiets_tried[..n] {
                        self.update_history(side, qm, -bonus);
                    }
                }
                break;
            }
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

    pub fn iterative_deepening(&mut self, board: &mut Board) -> (Option<Move>, i32, i32, u64) {
        let mut best_move = None;
        let mut best_score = 0;
        let mut last_depth = 0;
        self.killers = [[None; 2]; MAX_PLY];
        for depth in 1..=self.limits.max_depth {
            let score = self.negamax(board, depth, -MATE_SCORE - 1, MATE_SCORE + 1, 0);
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
            }
            if self.stop {
                break;
            }
        }
        (best_move, best_score, last_depth, self.nodes)
    }
}
