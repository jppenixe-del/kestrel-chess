use crate::attacks::Attacks;
use crate::board::Board;
use crate::search::{SearchLimits, Searcher, MATE_SCORE};
use crate::tt::TranspositionTable;
use crate::zobrist::Zobrist;
use std::io::{self, BufRead, Write};
use std::time::{Duration, Instant};

// Time reserved PER MOVE for network + bridge round-trip latency. This is
// an online-bot value: when we run on Lichess the clock keeps ticking
// while our move travels the wire and the Python bridge relays it, and
// that cost recurs on EVERY move -- not once per game. The old value (60)
// was reserved a single time from the whole clock (`safe_time = my_time -
// 60`), so across ~50 moves ~10s of real latency went unreserved and the
// engine flagged in otherwise-playable positions (repro: real_clock_
// selfplay.py, 3/4 games lost on time at 300ms latency). Now reserved
// per move from the search deadline as well (see cmd_go). 250 comfortably
// covers typical Lichess latency with margin; the base formula is
// self-correcting so a rare over-latency move is absorbed on the next.
// Value tuned to MEASURED latency: the server's round-trip to the Lichess
// API is ~45ms median (measured 2026-07-24), so the real per-move
// overhead (receive gamestate + bridge parse + send move POST) is ~100ms.
// 150 covers that ~3x over the median with margin for jitter, without the
// strength cost of an over-large reserve (250 measurably lost pure,
// non-flag games in self-play by thinking too little per move).
const MOVE_OVERHEAD_MS: i64 = 150;
/// Sudden-death base slice: this many moves' worth of the remaining clock.
/// The search scales that allowance by how hard the position is turning out
/// to be (see `time_scale` in search.rs), so this is a pivot, not the amount
/// spent -- and the scaling does NOT average out to one. Measured over 29
/// positions from a real game, the multiplier averaged 1.32 and the time
/// actually spent averaged 1.66x this slice once the last iteration's
/// overshoot is counted. A divisor picked as if the scaling were neutral had
/// the engine spending 6.4% of its remaining clock per move against a healthy
/// 4-5%, which drained the clock and produced a loss on time in self-play.
/// This number is therefore the pivot DIVIDED by the measured multiplier, and
/// it has to be re-measured whenever those curves change.
///
/// Its VALUE is set by matching what the previous, flat scheme actually spent
/// -- 1665ms a move on a fixed set of 14 real positions at a 60s clock, or
/// 2.8% of the clock. An earlier attempt aimed instead at a figure derived
/// from how another engine divides its clock (~4.5%), which made this engine
/// spend 56% more time per move than the version it was being compared
/// against, and it lost 2-7. That comparison could not have answered
/// anything: it varied the SHAPE of the allocation and the TOTAL at once.
/// Matched on the total, the only remaining difference is the shape, which is
/// the whole point of the change.
const TIME_SLICE_DIVISOR: i64 = 55;
/// The hard ceiling, as a percentage of the remaining clock. Guards the game
/// as a whole; on a healthy clock the ceiling below binds long before it.
const HARD_CAP_PERCENT: i64 = 45;
/// The hard ceiling as a multiple of the base slice, in tenths. Sits just
/// above the largest scaling the search can award itself, so a move that has
/// earned every extension still gets cut before it can overshoot.
const HARD_CAP_BUDGET_MULT: i64 = 25;

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
    movestogo: Option<i64>,
    last_score: Option<i32>,
) -> (i64, i64) {
    let safe_time = (my_time - MOVE_OVERHEAD_MS).max(1);

    // Nivel 1: a base allowance. The increment still counts as income spread
    // over the moves ahead rather than raw clock, which is why it is added
    // here rather than to `safe_time`.
    //
    // Sudden death: a fixed slice of what is left, not a guess at how many
    // moves remain. The old estimate (45 - fullmove, floored at 12) had the
    // budget GROW as a share of the clock the longer a game ran -- from
    // 1/45th of it at move 1 to a twelfth of it from move 34 on. In a game
    // that reached move 51 the engine was handing itself 8% of its remaining
    // clock per move while still needing to reach move 73, which it did not:
    // it played the last twenty moves at roughly zero. A constant fraction
    // decays with the clock instead of accelerating against it, and it does
    // not need to know how long the game will be -- which is not knowable.
    let moves_left = movestogo.unwrap_or(TIME_SLICE_DIVISOR);
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
    // Two ceilings, whichever binds first. The percentage of the clock guards
    // against a single move eating the game; the multiple of the allowance
    // guards against something subtler and, measured, more likely -- the soft
    // stop can only act BETWEEN iterations, and a tracked search went from
    // 8.9s to 15.1s inside one iteration, because each iteration can cost as
    // much as every iteration before it put together. Without a ceiling close
    // enough to bite mid-search, the allowance is a suggestion the last
    // iteration is free to overshoot by half.
    let mut hard_cap = (safe_time * HARD_CAP_PERCENT / 100)
        .min(soft * HARD_CAP_BUDGET_MULT / 10)
        .max(soft);

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
    // 2026-07-22: the Judit Polgar signature book was the user's
    // original IDEA for the project's personality, not a fixed
    // requirement -- explicitly set aside once real games against
    // strong opponents showed a pattern of speculative sacrifices
    // without enough calculated backing (see NOTAS_PROXIMA_SESSAO.md,
    // "não é compatível com o jogo entre motores"). Default book is now
    // one built from strong external engine analysis at depth>=16
    // (199 lines/~3.5k positions) instead of human-game frequency.
    // `KESTREL_BOOK_FILE` overrides the filename (same
    // reversible env-var pattern as every other opt-in hook in this
    // codebase) -- set it to `polgar_book.bin` to go back to the
    // original book, which is kept on disk, not deleted.
    let filename = std::env::var("KESTREL_BOOK_FILE").unwrap_or_else(|_| "sf17_book.bin".to_string());
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(&filename)))
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or(filename)
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
    /// Learned tables kept ACROSS moves of a game (one set per search
    /// thread) -- see `HistoryTables`. Cleared on `ucinewgame`, which is
    /// exactly the lifetime the UCI protocol defines for them. Before
    /// this, every `go` rebuilt them from zero and the engine relearned
    /// each position from scratch.
    hist: Vec<crate::search::HistoryTables>,
}

impl Engine {
    pub fn new() -> Self {
        let atk = Attacks::new();
        let zob = Zobrist::new();
        let style_book = crate::book::Book::load(&default_style_book_path()).ok();
        // Build every lazily-initialised global now, while no clock is
        // running -- see eval::warmup().
        // Profile before warm-up: warm-up evaluates a position, which seals
        // the lazily-built tables, so anything the profile sets has to be in
        // place first.
        if let Ok(path) = std::env::var("KESTREL_PROFILE") {
            match crate::eval::load_profile(&path) {
                Ok(n) => eprintln!("perfil: {} valores carregados de {}", n, path),
                Err(e) => eprintln!("perfil: nao consegui ler {} -- {}", path, e),
            }
        }
        crate::eval::warmup();
        crate::search::warmup();
        Engine {
            board: Board::startpos(),
            atk,
            zob,
            tt: TranspositionTable::new(64),
            history: Vec::new(),
            last_score: None,
            style_book,
            threads: 1,
            hist: Vec::new(),
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
            let parsed = Board::from_fen(&fen);
            // Reject a FEN where the side NOT to move is already in
            // check -- impossible in any position reachable by legal
            // play (the mover would have had to leave their own king
            // in check), but nothing in FEN parsing itself rejects it.
            // Found by review (2026-07-22): feeding such a FEN in
            // crashes deep in search/eval (`board.rs` king_sq() reads
            // an empty king bitboard -- `trailing_zeros()` on an empty
            // u64 returns 64, out of bounds for a 64-entry attack
            // table) once the search reaches a position where that
            // "already illegal" king similarly ends up captured. Not
            // reachable through normal play (a real game/GUI never
            // sends this), but `position fen <arbitrary>` is untrusted
            // input from whatever's driving the UCI connection -- fail
            // safe to startpos instead of crashing.
            if parsed.in_check(parsed.side.opp(), &self.atk) {
                eprintln!("position fen: rejected (side not to move is in check, illegal position) -- falling back to startpos");
                self.board = Board::startpos();
            } else {
                self.board = parsed;
            }
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
        let mut restrict_root: Vec<crate::moves::Move> = Vec::new();
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
                // `go searchmoves <m1> <m2> ...`: restrict the root to the
                // listed moves. Standard UCI, and the only way to ask the
                // engine what IT thinks a specific move is worth -- without
                // it, scoring one candidate meant reading the PV and hoping
                // it happened to start with that move.
                "searchmoves" => {
                    i += 1;
                    while i < tokens.len() {
                        match self.find_move(tokens[i]) {
                            Some(mv) => { restrict_root.push(mv); i += 1; }
                            None => break, // next keyword, not a move
                        }
                    }
                }
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
        // Measured against the model this was built for -- 1.5B parameters,
        // local -- a consultation takes 400 to 500ms warm. The reserve was
        // 1500ms, set before anyone had timed it, and every millisecond of it
        // comes out of the search rather than being added to the clock.
        const ADVISOR_RESERVE_MS: i64 = 700;
        let advisor_enabled = crate::advisor::Advisor::from_env().is_some();
        let mut soft_budget_ms: Option<i64> = None;
        // `soft_budget`: the allowance the search aims at, which it then
        // scales up or down between iterations (see `time_scale` in
        // search.rs). Only set on the real-clock branch -- `movetime`,
        // `infinite` and fixed-depth searches mean exactly what they say and
        // must not be second-guessed by time management.
        let mut soft_budget: Option<Duration> = None;
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
            let (soft, hard_cap) = compute_time_budget(my_time, my_inc, opp_time, movestogo, self.last_score);
            soft_budget_ms = Some(soft);
            // `hard_cap` is live again, and this time it is a ceiling rather
            // than a target. It was tried as the deadline once (2026-07-21)
            // and reverted the same day: the search ran to it whenever the
            // root move had not settled, and under Lazy SMP that is partly
            // thread timing rather than position difficulty -- 5 runs on one
            // real position gave 1.0s, 6.8s, 2.5s, 1.5s, 10.6s, and live
            // games showed 10-16s burned on ordinary moves followed by panic
            // for the rest of the game. What changed is what ends a normal
            // search: no longer this number, but the soft budget scaled by a
            // continuous reading of the position (see `time_scale`), which
            // shrinks back as soon as the move settles instead of committing
            // the whole ceiling the moment it does not. The ceiling now only
            // catches the case where that judgement is badly wrong.
            // Reserve MOVE_OVERHEAD_MS from THIS move's think time (not just
            // once from the whole clock) so search stops early enough for the
            // move to reach Lichess before our clock would expire. But never
            // subtract so much that we play blind: when `soft` itself is below
            // the overhead (low clock but many estimated moves left, e.g. after
            // spending big early), a flat `soft - overhead` would collapse to
            // ~0 and throw away a move at depth 1. Floor the think time at
            // min(soft, 80ms) so we still get a sane ~depth-6 move; the base
            // formula is self-correcting, so this rare case doesn't compound.
            let think_floor = soft.min(80);
            let search_ms = if advisor_enabled {
                (soft - ADVISOR_RESERVE_MS - MOVE_OVERHEAD_MS).max(think_floor)
            } else {
                (soft - MOVE_OVERHEAD_MS).max(think_floor)
            };
            let hard_ms = (hard_cap - MOVE_OVERHEAD_MS).max(search_ms);
            soft_budget = Some(Duration::from_millis(search_ms.max(1) as u64));
            Some(Instant::now() + Duration::from_millis(hard_ms.max(1) as u64))
        };

        let max_depth = depth.unwrap_or(64);
        let limits = SearchLimits { deadline, max_depth, max_nodes: nodes, soft_budget };
        let board_now = self.board.clone();
        let history_now = self.history.clone();
        let mut excluded_root_moves: Vec<crate::moves::Move> = Vec::new();
        // searchmoves is expressed through the root-exclusion list the
        // search already honours: exclude every legal root move that was
        // not asked for.
        if !restrict_root.is_empty() {
            let mut b = self.board.clone();
            for mv in crate::movegen::generate_legal(&mut b, &self.atk) {
                if !restrict_root.contains(&mv) {
                    excluded_root_moves.push(mv);
                }
            }
        }

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
            // Each line gets its own share of the clock.
            //
            // `limits` carries an ABSOLUTE deadline, computed once. Reusing
            // it meant the first line spent the entire budget and every line
            // after it began already past its deadline, returning whatever
            // depth 7 or 8 could reach. Those shallow lines then came back
            // with HIGHER scores than the deep one -- a position looks better
            // the less you understand it -- which put them inside the 30cp
            // window the advisor treats as a tie. The advisor was choosing
            // among candidates nobody had searched.
            //
            // Splitting the budget costs line 1 some depth, which is the
            // honest price of asking for more than one line: three lines mean
            // a third of the time each, not one line and two guesses.
            let mut line_limits = limits;
            if effective_multipv > 1 {
                // Not an equal split. Line 1 is the move that gets played
                // unless something overrules it, so it keeps half the
                // budget; the alternatives share the rest, which only has
                // to be enough for them to be trustworthy, not enough to
                // match it. An even three-way split cost the played move
                // two thirds of its thinking to buy depth on lines that
                // exist only to be compared against it.
                //
                // Both bounds are divided, and the soft one is what matters:
                // `deadline` is now a hard ceiling at a large fraction of the
                // clock, so slicing that alone would have handed line 1 half
                // of the emergency limit instead of half of the allowance.
                let divisor = if pv_index == 1 { 2 } else { 2 * (effective_multipv as u32 - 1) };
                line_limits.soft_budget = limits.soft_budget.map(|b| b / divisor);
                if let Some(end) = limits.deadline {
                    let total = end.saturating_duration_since(t0);
                    // Each line's deadline is measured from the START of the
                    // whole search, not from now. Taking `now + share` per
                    // line let every line's overrun carry into the next, so
                    // the lines together ran well past the hard deadline that
                    // was supposed to bound them -- a live game showed a move
                    // costing 18.6s against a 13.5s ceiling, three lines
                    // overshooting in sequence. Anchored to `t0`, the last
                    // line ends where the ceiling says, whatever the earlier
                    // ones did.
                    let rest = if effective_multipv > 1 { effective_multipv as u32 - 1 } else { 1 };
                    let used = total / 2 + (pv_index as u32 - 1) * (total / 2 / rest);
                    line_limits.deadline = Some(t0 + used.min(total));
                }
            }
            let (best, score, depth_reached, nodes_searched, pv_line) =
                self.search_mt(&board_now, &history_now, &excluded_root_moves, line_limits);
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
        &mut self,
        board: &Board,
        history: &[u64],
        excluded: &[crate::moves::Move],
        limits: SearchLimits,
    ) -> (Option<crate::moves::Move>, i32, i32, u64, Vec<crate::moves::Move>) {
        let n = self.threads.max(1);
        // Hand the learned tables (one set per thread) into this search;
        // they come back at the end and survive to the next move. Missing
        // sets (first search, or after a Threads change) start empty.
        let mut pool: Vec<crate::search::HistoryTables> = std::mem::take(&mut self.hist);
        while pool.len() < n {
            pool.push(crate::search::HistoryTables::default());
        }
        pool.truncate(n);
        let mut pool_iter = pool.into_iter();
        // One flag for the whole search: when the thread that reports decides
        // the position is settled, every thread stops, not just that one.
        let stop_flag = std::sync::atomic::AtomicBool::new(false);
        let stop_ref = &stop_flag;
        let tt_ref = &self.tt;
        let atk_ref = &self.atk;
        let zob_ref = &self.zob;
        let book_ref = self.style_book.as_ref();
        let (result, returned) = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..n)
                .map(|ti| {
                    let mut b = board.clone();
                    // Learned tables carried in from the previous move.
                    // Deliberately NOT faded: decaying them 3/4 per move
                    // was tried and measured worse on the blunder replay
                    // (45/60 avoided vs 47/60, and 74 improving deviations
                    // vs 83), so the statistics are carried over intact.
                    // `HistoryTables::fade` is kept for a future retry with
                    // a gentler factor or applied to fewer tables.
                    let ht = pool_iter.next().unwrap_or_default();
                    let searcher = Searcher {
                        atk: atk_ref,
                        zob: zob_ref,
                        tt: tt_ref,
                        nodes: 0,
                        limits,
                        stop: false,
                        stop_flag: stop_ref,
                        cut_nodes: 0,
                        cut_first: 0,
                        nmp_tried: 0,
                        nmp_tried_pv: 0,
                        nmp_failed_pv: 0,
                        nmp_cutoff_raw: 0,
                        nmp_cut_taken: 0,
                        nmp_verify_tried: 0,
                        nmp_verify_ok: 0,
                        nmp_verify_failed: 0,
                        nmp_failed_low: 0,
                        qnodes: 0,
                        cut_rfp: 0,
                        cut_razor: 0,
                        cut_futility: 0,
                        nodes_shallow: 0,
                        lmr_quiet_total: 0,
                        lmr_skip_check: 0,
                        lmr_skip_depth: 0,
                        lmr_skip_extend: 0,
                        lmr_skip_early: 0,
                        lmr_tried: 0,
                        lmr_research: 0,
                        lmr_sum: 0,
                        history: history.to_vec(),
                        // killers stay per-search: they are ply-indexed, and
                        // ply N means a different point once a move is played
                        killers: [[None; 2]; crate::search::MAX_PLY],
                        history_scores: ht.history_scores,
                        countermoves: ht.countermoves,
                        cont_hist: ht.cont_hist,
                        corr_hist: ht.corr_hist,
                        corr_hist_np_stm: ht.corr_hist_np_stm,
                        corr_hist_np_nstm: ht.corr_hist_np_nstm,
                        corr_hist_minor: ht.corr_hist_minor,
                        corr_hist_major: ht.corr_hist_major,
                        corr_hist_threats: ht.corr_hist_threats,
                        ply_last_move: [None; crate::search::MAX_PLY],
                        static_evals: [0i32; crate::search::MAX_PLY],
                        root_best: None,
                        root_scores: Vec::new(),
                        nmp_min_ply: 0,
                        excluded_move: None,
                        excluded_root_moves: excluded.to_vec(),
                        style_book: book_ref,
                        root_move_nodes: Vec::new(),
                        capture_history: ht.capture_history,
                        dextensions: [0; crate::search::MAX_PLY],
                        // only thread 0 narrates, or each depth would be
                        // announced once per Lazy-SMP thread
                        report: ti == 0,
                    };
                    scope.spawn(move || {
                        let mut searcher = searcher;
                        let (best, score, depth_reached, nodes) = searcher.iterative_deepening(&mut b);
                        (best, score, depth_reached, nodes, searcher)
                    })
                })
                .collect();
            // Weighted vote across threads, not a head count.
            //
            // Two designs exist in the wild. One has the main thread decide and
            // helpers only fill the shared table; the other votes. Measured
            // here, main-thread-decides lost at 40% over 50 games, because our
            // helpers are near-copies of the main search and so contribute
            // nothing but variance to whoever reads them.
            //
            // The vote is the right model, but counting heads is the wrong
            // vote: it treats a thread that reached depth 8 with a score of
            // -200 as equal to one that reached depth 14 at +50. Weight each
            // thread by how much better its score is than the worst thread's,
            // times the depth it actually completed. A thread that is both
            // deep and convinced dominates; one that is shallow or pessimistic
            // barely registers.
            //
            // The +14 offset keeps a thread that ties the minimum score from
            // voting with zero weight -- it still saw something, and at equal
            // scores depth should decide.
            let mut results: Vec<_> = handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .collect();
            if results.is_empty() {
                return ((None, 0, 0, 0, Vec::new()), Vec::new());
            }
            let best_idx = if results.len() < 2 {
                0
            } else {
                let min_score = results.iter().map(|r| r.1).min().unwrap_or(0);
                let weight = |r: &(Option<crate::moves::Move>, i32, i32, u64, Searcher)| {
                    ((r.1 - min_score + 14) as i64) * (r.2.max(1) as i64)
                };
                let mut votes: Vec<(Option<crate::moves::Move>, i64)> = Vec::new();
                for r in &results {
                    match votes.iter_mut().find(|(m, _)| *m == r.0) {
                        Some((_, v)) => *v += weight(r),
                        None => votes.push((r.0, weight(r))),
                    }
                }
                let winner = votes
                    .iter()
                    .max_by_key(|(_, v)| *v)
                    .map(|(m, _)| *m)
                    .unwrap_or(results[0].0);
                // Among the threads that agree on the winning move, take the
                // one with the strongest individual claim: its PV is the one
                // worth reporting.
                (0..results.len())
                    .filter(|&i| results[i].0 == winner)
                    .max_by_key(|&i| weight(&results[i]))
                    .unwrap_or(0)
            };
            let nodes_total: u64 = results.iter().map(|r| r.3).sum();
            let (best, score, depth_reached, _, winner) = results.remove(best_idx);
            let pv_line = winner.extract_pv(board, depth_reached.max(1) as usize + 4);
            // Reclaim every thread's learned tables so the next move in
            // this game starts from what this search learned, instead of
            // from zero. The winner's set is kept first so thread 0 (the
            // one that decided) keeps its own statistics next time.
            let mut back: Vec<crate::search::HistoryTables> = Vec::with_capacity(n);
            let harvest = |sr: Searcher| crate::search::HistoryTables {
                history_scores: sr.history_scores,
                countermoves: sr.countermoves,
                capture_history: sr.capture_history,
                cont_hist: sr.cont_hist,
                corr_hist: sr.corr_hist,
                corr_hist_np_stm: sr.corr_hist_np_stm,
                corr_hist_np_nstm: sr.corr_hist_np_nstm,
                corr_hist_minor: sr.corr_hist_minor,
                corr_hist_major: sr.corr_hist_major,
                corr_hist_threats: sr.corr_hist_threats,
            };
            back.push(harvest(winner));
            for (_, _, _, _, sr) in results {
                back.push(harvest(sr));
            }
            ((best, score, depth_reached, nodes_total, pv_line), back)
        });
        self.hist = returned;
        result
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
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[3] == "value" {
                        // Search parameters by name. Sweeping a pruning margin
                        // used to mean editing a constant and waiting for a
                        // build; this makes it one line into a running engine,
                        // which is the difference between trying three values
                        // and trying thirty.
                        //
                        // An unknown name is reported, never ignored. A silent
                        // typo looks exactly like "this parameter has no
                        // effect", and a sweep that reads that way produces
                        // five identical results and a confident conclusion
                        // drawn from none of them.
                        if let Ok(v) = tokens[4].parse::<i32>() {
                            // Search parameters first, then material values
                            // (mg_rook, eg_queen, ...). Both are just numbers
                            // the engine compares things against, and both are
                            // worth sweeping without a rebuild.
                            // Family factors arrive as `scale_<family>` in
                            // per-mille, so a whole class of evaluation terms
                            // can be moved at once and the question becomes
                            // "where is the evaluation mispriced" rather than
                            // "is this one number right".
                            let fam = tokens[2].strip_prefix("scale_");
                            if !crate::search::set_param(tokens[2], v)
                                && !crate::eval::set_material(tokens[2], v)
                                && !fam.map_or(false, |f| crate::eval::set_family_scale(f, v))
                                && !tokens[2]
                                    .strip_prefix("psqt_")
                                    .map_or(false, |p| crate::eval::set_psqt_scale(p, v))
                            {
                                eprintln!("setoption: unknown parameter '{}'", tokens[2]);
                            }
                        } else {
                            eprintln!("setoption: value for '{}' is not an integer", tokens[2]);
                        }
                    }
                }
                "ucinewgame" => {
                    self.board = Board::startpos();
                    self.tt.clear();
                    self.hist.clear(); // new game -> forget learned tables
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
                "eval" | "evalbreak" => {
                    // Robust breakdown via to_vec/from_vec index ranges (no
                    // per-field typing). Contribution of a range = positional
                    // with that range zeroed, subtracted from full positional.
                    let b = &self.board;
                    // The set this position's phase actually uses. Reading
                    // default_weights() here reported numbers the engine was
                    // not playing with the moment buckets arrived -- a
                    // diagnostic that lies is worse than none.
                    let full = crate::eval::weights_for(b);
                    let mat = crate::eval::material_pst_white(b);
                    let pos = crate::eval::positional_terms(b, full);
                    let base = full.to_vec();
                    let contrib = |lo: usize, hi: usize| -> i32 {
                        let mut v = base.clone();
                        for i in lo..hi.min(v.len()) { v[i] = 0; }
                        pos - crate::eval::positional_terms(b, &full.from_vec(&v))
                    };
                    // to_vec order: pieces 0..16, mobility 16..240,
                    // king(attackers/checks/shelter/storm) 240..276,
                    // rest(threats+pawn-structure+passers) 276..end
                    let pieces = contrib(0, 16);
                    let mobility = contrib(16, 240);
                    // King safety is not read off by zeroing a slice of the
                    // weight vector any more: part of it lives outside that
                    // vector, so a slice leaves inputs behind and the leftover
                    // still feeds the danger curve. Silence the whole block
                    // instead and take the difference, which is exact.
                    let king = pos - crate::eval::positional_terms(b, &full.with_king_silenced());
                    // Whatever the remaining ranges do not account for. Taken
                    // as a remainder so the four numbers always add up to
                    // `positional`, however the blocks are computed.
                    let rest = pos - pieces - mobility - king;
                    let _ = writeln!(out, "eval(white) total={}  material_pst={}  positional={}", mat + pos, mat, pos);
                    let _ = writeln!(out, "  pieces={} mobility={} king={} threats+pawns={}", pieces, mobility, king, rest);
                    let _ = out.flush();
                }
                "quit" => break,
                _ => {}
            }
        }
    }
}
