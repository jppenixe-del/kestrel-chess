use crate::attacks::Attacks;
use crate::board::Board;
use crate::search::{SearchLimits, Searcher, MATE_SCORE};
use crate::tt::TranspositionTable;
use crate::zobrist::Zobrist;
use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

const MOVE_OVERHEAD_MS: i64 = 60;

/// Gestao de tempo em 4 niveis -- a mesma arquitetura em camadas que
/// validamos esta sessao no Pond (jogos reais, derrotas por bandeira
/// investigadas e corrigidas uma a uma): formula elastica normal, corte
/// para relogio baixo sem vantagem clara, modo panico, zona da morte.
/// `last_score` e' o score (cp, da nossa perspetiva) do ultimo "go" --
/// None no 1o lance do jogo. Sem isto so' havia 2 niveis (normal +
/// panico), sem distinguir "estamos a perder" de "estamos a ganhar" --
/// o bug exato que causou uma derrota real por bandeira no Pond antes de
/// ser corrigido (relaxava o corte tambem quando estavamos a perder).
fn compute_time_budget(
    my_time: i64,
    my_inc: i64,
    opp_time: i64,
    fullmove: u32,
    movestogo: Option<i64>,
    last_score: Option<i32>,
) -> (i64, i64) {
    let safe_time = (my_time - MOVE_OVERHEAD_MS).max(1);

    // Nivel 1: formula elastica normal. Sem movestogo (sudden death, o
    // nosso caso habitual): estima quantos lances proprios ainda faltam
    // a partir do numero do lance atual -- e o incremento conta como
    // "rendimento" ao longo desses lances, nao so' o relogio em bruto
    // (licao central desta sessao com o Pond).
    let moves_left = movestogo.unwrap_or_else(|| {
        let estimate = 45i64 - (fullmove as i64 - 1);
        estimate.clamp(12, 45)
    });
    let base = safe_time / moves_left + my_inc * 3 / 4;
    // 2026-07-20 (BUG REAL/CRASH corrigido -- encontrado ao testar
    // manualmente "go depth N" sem wtime, um pedido perfeitamente valido
    // do protocolo UCI, ex.: ferramentas de analise/debug): com
    // safe_time pequeno (ex. my_time=0, o "else" de cmd_go so' evita
    // este caminho quando "depth" tambem esta ausente -- ver uci.rs
    // cmd_go), "safe_time/2" podia ficar ABAIXO de 10, e
    // "base.clamp(10, safe_time/2)" entra em PANIC em Rust quando
    // min>max (nao e' um clamp normal, e' um erro fatal). Corrigido:
    // o limite superior nunca fica abaixo do limite inferior.
    let soft_max = (safe_time / 2).max(10);
    let mut soft = base.clamp(10, soft_max);
    let mut hard_cap = (safe_time * 3 / 10).max(soft); // nunca mais de ~30% do relogio numa jogada

    // Nivel 1.5: "olhar para o adversario" antes de decidir quanto gastar
    // -- pedido directo depois de reparar que o motor por vezes joga
    // depressa demais em posicoes dificeis mesmo com relogio de sobra.
    // So' ajusta o TECTO extra (hard_cap), nunca o `soft` baseline nem os
    // niveis de panico abaixo -- a seguranca do proprio relogio nunca
    // depende do relogio alheio. Confortavelmente a frente (>=1.5x o
    // deles): podemos dar-nos ao luxo de pensar mais fundo numa posicao
    // dificil, o adversario provavelmente vai precisar de mais tempo do
    // que nos em breve. Confortavelmente atras (eles >=1.5x o nosso):
    // aperta -- preservar o proprio relogio pesa mais quando ja estamos
    // em desvantagem nele.
    if opp_time > 0 {
        if my_time >= opp_time * 3 / 2 {
            hard_cap = hard_cap * 6 / 5;
        } else if opp_time >= my_time * 3 / 2 {
            hard_cap = hard_cap * 4 / 5;
        }
    }

    let clearly_winning = last_score.map(|s| s >= 400).unwrap_or(false);
    let clearly_losing = last_score.map(|s| s <= -400).unwrap_or(false);

    // Nivel 2: relogio baixo (< 20s) e SEM vantagem clara -- corta mais
    // fundo do que a formula normal permitiria. So' se relaxa quando a
    // vantagem e' NOSSA (clearly_winning); nunca quando e' do adversario.
    if safe_time < 20_000 && !clearly_winning {
        let cut = (safe_time / 25).clamp(20, 800);
        soft = soft.min(cut);
        hard_cap = hard_cap.min(cut);
    }

    // Nivel 3: modo panico (< 4s) -- corte agressivo independente de
    // vantagem, mas AINDA MAIS fundo se estivermos claramente a perder
    // (pedido explicito da sessao com o Pond: "em -5 pecas jogar a 0").
    if safe_time < 4000 {
        let panic = if clearly_losing {
            (safe_time / 40).clamp(3, 60)
        } else {
            (safe_time / 20).clamp(5, 150)
        };
        soft = panic;
        hard_cap = panic;
    }

    // Nivel 4: zona da morte (< 1200ms) -- praticamente so' vive do
    // incremento, chao absoluto independente de tudo o resto.
    if safe_time < 1200 {
        let floor = (my_inc * 4 / 5).clamp(2, 40);
        soft = floor;
        hard_cap = floor;
    }

    (soft, hard_cap)
}

/// Caminho do livro relativo ao proprio executavel (nao fixo a esta
/// maquina) -- pedido depois de mover o motor para o servidor remoto:
/// "/mnt/d/..." nao existe la'. Espera polgar_book.bin ao lado do binario.
fn default_style_book_path() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("polgar_book.bin")))
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "polgar_book.bin".to_string())
}

pub struct Engine {
    board: Board,
    atk: Attacks,
    zob: Zobrist,
    tt: TranspositionTable,
    history: Vec<u64>,
    last_score: Option<i32>, // score (cp, nossa perspetiva) do ultimo "go" -- para os niveis 2/3 de compute_time_budget
    style_book: Option<crate::book::Book>, // "assinatura" da Judit Polgar -- ver book.rs
    threads: usize, // Lazy SMP -- ver search_mt(). 1 = sem paralelismo (comportamento antigo).
}

impl Engine {
    pub fn new() -> Self {
        let atk = Attacks::new();
        let zob = Zobrist::new();
        let style_book = crate::book::Book::load(&default_style_book_path()).ok();
        Engine {
            board: Board::startpos(),
            atk,
            zob,
            tt: TranspositionTable::new(64),
            history: Vec::new(),
            last_score: None,
            style_book,
            threads: 1,
        }
    }

    fn set_position(&mut self, tokens: &[&str]) {
        let mut i = 0;
        if tokens.get(i) == Some(&"startpos") {
            self.board = Board::startpos();
            i += 1;
        } else if tokens.get(i) == Some(&"fen") {
            i += 1;
            let start = i;
            while i < tokens.len() && tokens[i] != "moves" {
                i += 1;
            }
            let fen = tokens[start..i].join(" ");
            self.board = Board::from_fen(&fen);
        }
        self.history.clear();
        self.history.push(self.zob.hash(&self.board));
        if tokens.get(i) == Some(&"moves") {
            i += 1;
            while i < tokens.len() {
                if let Some(mv) = self.find_move(tokens[i]) {
                    self.board.make_move(&mv);
                    self.history.push(self.zob.hash(&self.board));
                }
                i += 1;
            }
        }
    }

    fn find_move(&self, uci: &str) -> Option<crate::moves::Move> {
        // Not a hot path (one-off UCI move-string lookup, not called
        // per search node) -- a local clone here is fine; the
        // generate_legal() fix is about the search's own move loops.
        let mut b = self.board.clone();
        let legal = crate::movegen::generate_legal(&mut b, &self.atk);
        legal.into_iter().find(|m| m.to_uci() == uci)
    }

    fn cmd_go(&mut self, tokens: &[&str], out: &mut impl Write) {
        let mut wtime = 0i64;
        let mut btime = 0i64;
        let mut winc = 0i64;
        let mut binc = 0i64;
        let mut movetime: Option<i64> = None;
        let mut depth: Option<i32> = None;
        let mut movestogo: Option<i64> = None;
        let mut infinite = false;
        let mut nodes: Option<u64> = None;
        let mut multipv: usize = 1;
        let mut i = 0;
        while i < tokens.len() {
            match tokens[i] {
                "wtime" => { wtime = tokens.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0); i += 2; }
                "btime" => { btime = tokens.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0); i += 2; }
                "winc" => { winc = tokens.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0); i += 2; }
                "binc" => { binc = tokens.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0); i += 2; }
                "movestogo" => { movestogo = tokens.get(i + 1).and_then(|s| s.parse().ok()); i += 2; }
                "movetime" => { movetime = tokens.get(i + 1).and_then(|s| s.parse().ok()); i += 2; }
                "depth" => { depth = tokens.get(i + 1).and_then(|s| s.parse().ok()); i += 2; }
                "nodes" => { nodes = tokens.get(i + 1).and_then(|s| s.parse().ok()); i += 2; }
                "multipv" => { multipv = tokens.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(1).max(1); i += 2; }
                "infinite" => { infinite = true; i += 1; }
                _ => { i += 1; }
            }
        }

        let side_white = self.board.side == crate::types::Color::White;
        let (my_time, my_inc) = if side_white { (wtime, winc) } else { (btime, binc) };
        let opp_time = if side_white { btime } else { wtime };

        // `soft_budget_ms` tracks the real per-move time budget derived
        // from wtime/btime specifically (None for movetime/infinite/depth
        // requests, which aren't live-clock scenarios) -- used below as a
        // hard safety gate on the optional advisor: a real game clock
        // that leaves less than ADVISOR_MIN_BUDGET_MS for this move is
        // bullet-speed territory, where an LLM round-trip (hundreds of ms
        // even best-case) is unsafe regardless of what the external
        // bridge/caller believes the time control to be.
        //
        // ADVISOR_RESERVE_MS is reserved from the SEARCH's own deadline
        // (not added on top of it) whenever the advisor is active, so
        // search-time + advisor-round-trip together still respect the
        // clock's real budget for this move -- otherwise every advisor
        // consultation would silently overspend the move's allotment.
        const ADVISOR_RESERVE_MS: i64 = 1500;
        let advisor_enabled = crate::advisor::Advisor::from_env().is_some();
        let mut soft_budget_ms: Option<i64> = None;
        // `soft_deadline`: an EARLIER, purely-advisory checkpoint used
        // only by iterative_deepening()'s best-move-stability early
        // exit (see search.rs) -- always <= `deadline`, so worst case
        // (move never stabilizes) search behaves exactly as before,
        // identical hard cutoff. Only set in the real-clock branch,
        // half of that branch's own search budget.
        let mut soft_deadline: Option<Instant> = None;
        let deadline: Option<Instant> = if let Some(mt) = movetime {
            Some(Instant::now() + Duration::from_millis(mt.max(1) as u64))
        } else if infinite || my_time == 0 {
            // No wtime/btime given (and no movetime, handled above): this
            // is a fixed-depth or "go infinite" analysis request, not a
            // live clock. Must NOT fall through to compute_time_budget(0,
            // ...), which was handing "go depth N" a near-zero budget and
            // cutting the search off many plies short of N (found while
            // investigating an apparent root-move flip-flop that turned
            // out to be this: "go depth 18" was actually stopping at
            // depth 6). depth.is_some() still caps iterative_deepening's
            // loop via max_depth below -- this only removes the artificial
            // time cutoff for that case.
            None
        } else {
            let (soft, _hard_cap) = compute_time_budget(my_time, my_inc, opp_time, self.board.fullmove, movestogo, self.last_score);
            soft_budget_ms = Some(soft);
            // REVERTED (2026-07-21, same day): using `hard_cap` as the
            // real deadline looked safe in isolated single-threaded
            // tests, but the live bot runs Threads=4 (see
            // lichess_bridge.py), and Lazy SMP has genuine run-to-run
            // variance in when a thread's root move stabilizes. Repro
            // on the exact position from a real lost game (5 runs, same
            // FEN, same clock, Threads=4): 1.0s, 6.8s, 2.5s, 1.5s,
            // 10.6s. That matches a real pattern seen across 3 games in
            // a 4-game series played right after this shipped: 10-16s
            // burned on an ordinary move around move 4-8, then panic
            // mode for the rest of the game. `hard_cap` is thread-count-
            // sensitive noise, not a genuine difficulty signal -- back
            // to the flat `soft` baseline until this can be built on a
            // real complexity signal instead of per-thread stability
            // timing.
            let search_ms = if advisor_enabled { (soft - ADVISOR_RESERVE_MS).max(1) } else { soft };
            soft_deadline = Some(Instant::now() + Duration::from_millis((search_ms / 2).max(1) as u64));
            Some(Instant::now() + Duration::from_millis(search_ms.max(1) as u64))
        };

        let max_depth = depth.unwrap_or(64);
        let limits = SearchLimits { deadline, max_depth, max_nodes: nodes, soft_deadline };
        let board_now = self.board.clone();
        let history_now = self.history.clone();
        let mut excluded_root_moves: Vec<crate::moves::Move> = Vec::new();

        // Optional LLM tie-breaker (see advisor.rs): entirely opt-in via
        // KESTREL_ADVISOR_HOST. When set, always search at least 3 root
        // lines internally (regardless of what the UCI caller asked for
        // via "multipv"), so there is something real to consult when the
        // engine itself is indifferent between candidates. When the env
        // var is unset -- the default for every deployment, including
        // the live bot unless explicitly configured -- `advisor` is
        // `None` and `effective_multipv` equals whatever was requested
        // (1 by default): zero behavior change from before this feature
        // existed.
        const ADVISOR_MIN_BUDGET_MS: i64 = 2000;
        let advisor_time_ok = soft_budget_ms.map(|ms| ms >= ADVISOR_MIN_BUDGET_MS).unwrap_or(true);
        let advisor = crate::advisor::Advisor::from_env().filter(|_| advisor_time_ok);
        let effective_multipv = if advisor.is_some() { multipv.max(3) } else { multipv };

        let t0 = Instant::now();
        let mut top_move: Option<crate::moves::Move> = None;
        let mut nodes_total: u64 = 0;
        let mut collected: Vec<(char, crate::moves::Move, i32)> = Vec::new();
        for pv_index in 1..=effective_multipv {
            let (best, score, depth_reached, nodes_searched, pv_line) =
                self.search_mt(&board_now, &history_now, &excluded_root_moves, limits);
            nodes_total += nodes_searched;
            if pv_index == 1 {
                self.last_score = Some(score);
                top_move = best;
            }
            let dt = t0.elapsed();
            let nps = if dt.as_secs_f64() > 0.0 { (nodes_total as f64 / dt.as_secs_f64()) as u64 } else { 0 };
            let score_str = if score.abs() >= MATE_SCORE - 1000 {
                let mate_in = ((MATE_SCORE - score.abs() + 1) / 2).max(1);
                format!("mate {}", if score > 0 { mate_in } else { -mate_in })
            } else {
                format!("cp {}", score)
            };
            match best {
                Some(mv) => {
                    collected.push((((b'A' + (pv_index - 1) as u8)) as char, mv, score));
                    if pv_index <= multipv {
                        let pv_str = if pv_line.is_empty() {
                            mv.to_uci()
                        } else {
                            pv_line.iter().map(|m| m.to_uci()).collect::<Vec<_>>().join(" ")
                        };
                        let _ = writeln!(
                            out,
                            "info depth {} multipv {} score {} nodes {} nps {} time {} pv {}",
                            depth_reached, pv_index, score_str, nodes_total, nps, dt.as_millis(), pv_str
                        );
                    }
                    // MultiPV via exclusion: this line's move is dropped
                    // from the root move list before the next call, so
                    // the search finds the next-best line instead of
                    // repeating the same one -- see excluded_root_moves.
                    excluded_root_moves.push(mv);
                }
                None => break, // fewer legal root moves than requested lines
            }
        }
        let _ = out.flush();

        // Optional advisor consultation: only when enabled AND the top
        // lines are close enough to call it a tie -- the engine's own
        // search remains the sole decision-maker otherwise. Any failure
        // here (unreachable host, malformed response, no candidate
        // named) silently keeps `top_move` as the engine's own line 1.
        if let Some(adv) = &advisor {
            if collected.len() > 1 {
                let top_score = collected[0].2;
                let tied: Vec<(char, String, i32)> = collected
                    .iter()
                    .filter(|(_, _, sc)| (sc - top_score).abs() <= 30)
                    .map(|(lab, mv, sc)| (*lab, mv.to_uci(), *sc))
                    .collect();
                if tied.len() > 1 {
                    let fen = self.board.to_fen();
                    if let Some(chosen_label) = adv.ask(&fen, &tied) {
                        if let Some((_, mv, _)) = collected.iter().find(|(lab, _, _)| *lab == chosen_label) {
                            top_move = Some(*mv);
                        }
                    }
                }
            }
        }

        // Rede de seguranca absoluta: mesmo que a busca nao tenha
        // conseguido terminar profundidade nenhuma (relogio esgotado
        // mesmo em cima do 1o lance, caso extremo), NUNCA devolver lance
        // nulo se houver lances legais -- joga o primeiro legal em vez de
        // "0000" (que a arena/arbitro trata como derrota imediata).
        let final_move = top_move.or_else(|| {
            crate::movegen::generate_legal(&mut self.board, &self.atk).into_iter().next()
        });
        match final_move {
            Some(mv) => {
                let _ = writeln!(out, "bestmove {}", mv.to_uci());
            }
            None => {
                let _ = writeln!(out, "bestmove 0000");
            }
        }
        let _ = out.flush();
    }

    /// Lazy SMP: spawns `self.threads` independent search threads on the
    /// SAME position, all sharing the lock-free TT (see tt.rs) but each
    /// with its own move-ordering state (killers/history/countermoves) --
    /// different threads naturally explore the tree in slightly different
    /// orders (thread-local heuristics diverge from the first node they
    /// disagree on), which finds tactics/refutations sooner than a single
    /// thread alone, on top of raw nodes/sec scaling with core count.
    /// `threads == 1` degenerates to a single call with thread::scope's
    /// overhead but otherwise identical behavior to the pre-Lazy-SMP code.
    ///
    /// All threads share the SAME `limits.deadline` (real wall-clock
    /// instant) rather than a cross-thread stop signal -- simpler, and
    /// sufficient: every thread naturally stops within one time-check
    /// interval of every other, without needing a shared atomic flag.
    ///
    /// Result selection ("best thread"): the thread that reached the
    /// greatest depth wins (ties broken by score, then by thread index),
    /// with a consensus safeguard against that thread being a lone
    /// outlier the other threads disagree with (see the comment at the
    /// vote-counting block below). Returns the winning thread's own
    /// `Searcher` so the caller can still call `extract_pv()` against the
    /// TT it populated.
    fn search_mt(
        &self,
        board: &Board,
        history: &[u64],
        excluded: &[crate::moves::Move],
        limits: SearchLimits,
    ) -> (Option<crate::moves::Move>, i32, i32, u64, Vec<crate::moves::Move>) {
        let n = self.threads.max(1);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..n)
                .map(|_| {
                    let mut b = board.clone();
                    let searcher = Searcher {
                        atk: &self.atk,
                        zob: &self.zob,
                        tt: &self.tt,
                        nodes: 0,
                        limits,
                        stop: false,
                        history: history.to_vec(),
                        killers: [[None; 2]; crate::search::MAX_PLY],
                        history_scores: [[[0; 64]; 64]; 2],
                        countermoves: [[None; 64]; 6],
                        cont_hist: vec![0i32; crate::search::CONT_HIST_SIZE].into_boxed_slice(),
                        corr_hist: vec![0i32; crate::search::CORR_HIST_SIZE * 2].into_boxed_slice(),
                        ply_last_move: [None; crate::search::MAX_PLY],
                        static_evals: [0i32; crate::search::MAX_PLY],
                        root_best: None,
                        excluded_move: None,
                        excluded_root_moves: excluded.to_vec(),
                        style_book: self.style_book.as_ref(),
                    };
                    scope.spawn(move || {
                        let mut searcher = searcher;
                        let (best, score, depth_reached, nodes) = searcher.iterative_deepening(&mut b);
                        (best, score, depth_reached, nodes, searcher)
                    })
                })
                .collect();
            let mut results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
            let mut best_idx = 0;
            for i in 1..results.len() {
                let better = results[i].2 > results[best_idx].2
                    || (results[i].2 == results[best_idx].2 && results[i].1 > results[best_idx].1);
                if better {
                    best_idx = i;
                }
            }
            // Consensus safeguard (2026-07-21): "deepest thread wins" is a
            // lone-outlier risk -- traced a real bullet loss to exactly
            // this: one thread's search settled on a move (hanging a
            // rook) that no other thread agreed with, won the tie-break
            // by reaching one ply deeper, and got played. 20 repeated
            // trials of that exact position/clock never reproduced it
            // again (17/20 gave the objectively good move, 3/20 a
            // different-but-sound trade), confirming it was thread-
            // selection variance, not a deterministic eval/search bug.
            // If the naive winner's move is a lone outlier that most
            // OTHER threads disagree with, prefer whichever move the
            // plurality of threads actually settled on instead (using
            // the deepest thread among those that agree with it) --
            // costs nothing when threads agree (the common case), only
            // overrides the rare case where the deepest thread is
            // alone against the rest.
            if results.len() > 2 {
                let vote_count = |mv: Option<crate::moves::Move>| results.iter().filter(|r| r.0 == mv).count();
                let winner_move = results[best_idx].0;
                let winner_votes = vote_count(winner_move);
                let mut plurality_move = winner_move;
                let mut plurality_votes = winner_votes;
                for r in &results {
                    let v = vote_count(r.0);
                    if v > plurality_votes {
                        plurality_votes = v;
                        plurality_move = r.0;
                    }
                }
                if plurality_votes > winner_votes && plurality_move != winner_move {
                    if let Some(alt_idx) = (0..results.len())
                        .filter(|&i| results[i].0 == plurality_move)
                        .max_by_key(|&i| (results[i].2, results[i].1))
                    {
                        best_idx = alt_idx;
                    }
                }
            }
            let nodes_total: u64 = results.iter().map(|r| r.3).sum();
            let (best, score, depth_reached, _, winner) = results.remove(best_idx);
            let pv_line = winner.extract_pv(board, depth_reached.max(1) as usize + 4);
            (best, score, depth_reached, nodes_total, pv_line)
        })
    }

    pub fn run(&mut self) {
        let stdin = io::stdin();
        let mut out = io::stdout();
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let tokens: Vec<&str> = line.split_whitespace().collect();
            match tokens[0] {
                "uci" => {
                    let _ = writeln!(out, "id name kestrel");
                    let _ = writeln!(out, "id author claude (fable5), projeto proprio");
                    let _ = writeln!(out, "option name Hash type spin default 64 min 1 max 4096");
                    let _ = writeln!(out, "option name Threads type spin default 1 min 1 max 64");
                    let _ = writeln!(out, "uciok");
                    let _ = out.flush();
                }
                "isready" => {
                    let _ = writeln!(out, "readyok");
                    let _ = out.flush();
                }
                "setoption" => {
                    if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "Hash" && tokens[3] == "value" {
                        if let Ok(mb) = tokens[4].parse::<usize>() {
                            self.tt = TranspositionTable::new(mb.max(1));
                        }
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "Threads" && tokens[3] == "value" {
                        if let Ok(n) = tokens[4].parse::<usize>() {
                            self.threads = n.max(1);
                        }
                    }
                }
                "ucinewgame" => {
                    self.board = Board::startpos();
                    self.tt.clear();
                    self.history.clear();
                    self.last_score = None;
                }
                "position" => {
                    self.set_position(&tokens[1..]);
                }
                "go" => {
                    self.cmd_go(&tokens[1..], &mut out);
                }
                "stop" => {}
                "quit" => break,
                _ => {}
            }
        }
    }
}
