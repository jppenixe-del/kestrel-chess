mod advisor;
mod attacks;
mod bitboard;
mod board;
mod book;
mod eval;
mod magic;
mod movegen;
mod moves;
mod perft;
mod search;
mod tt;
mod types;
mod uci;
mod zobrist;

use attacks::Attacks;
use board::Board;
use std::collections::HashMap;
use std::env;
use std::io::{BufRead, Write};
use std::time::Instant;
use zobrist::Zobrist;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() >= 2 && args[1] == "perft" {
        let depth: u32 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(5);
        let fen = if args.len() > 3 {
            args[3..].join(" ")
        } else {
            "startpos".to_string()
        };
        let atk = Attacks::new();
        let mut board = if fen == "startpos" {
            Board::startpos()
        } else {
            Board::from_fen(&fen)
        };
        let t0 = Instant::now();
        let n = perft::perft(&mut board, depth, &atk);
        let dt = t0.elapsed();
        println!("perft({}) = {}  ({:.2}s, {:.0} nps)", depth, n, dt.as_secs_f64(), n as f64 / dt.as_secs_f64().max(1e-9));
        return;
    }
    if args.len() >= 2 && args[1] == "verify_incremental" {
        let depth: u32 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(5);
        let fen = if args.len() > 3 { args[3..].join(" ") } else { "startpos".to_string() };
        let atk = Attacks::new();
        let mut board = if fen == "startpos" { Board::startpos() } else { Board::from_fen(&fen) };
        let t0 = Instant::now();
        let (nodes, mismatches) = perft::verify_incremental_eval(&mut board, depth, &atk);
        let dt = t0.elapsed();
        println!("verify_incremental({}) = {} nos, {} discrepancias ({:.2}s)", depth, nodes, mismatches, dt.as_secs_f64());
        std::process::exit(if mismatches == 0 { 0 } else { 1 });
    }
    if args.len() >= 4 && args[1] == "buildbook" {
        build_book(&args[2], &args[3]);
        return;
    }
    if args.len() >= 4 && args[1] == "lookupbook" {
        lookup_book(&args[2], &args[3..].join(" "));
        return;
    }
    if args.len() >= 2 && args[1] == "checkweights" {
        check_weights_roundtrip();
        return;
    }
    if args.len() >= 4 && args[1] == "selfplay" {
        let num_games: u32 = args[2].parse().expect("num_games invalido");
        let out_path = &args[3];
        let node_limit: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(5000);
        let threads: usize = args.get(5).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
        });
        selfplay_datagen(num_games, out_path, node_limit, threads);
        return;
    }
    if args.len() >= 4 && args[1] == "resolvequiet" {
        resolve_quiet_dataset(&args[2], &args[3]);
        return;
    }
    if args.len() >= 4 && args[1] == "tunefast" {
        let iters: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(2000);
        let lr: f64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(2.0);
        tune_fast(&args[2], &args[3], iters, lr);
        return;
    }
    if args.len() >= 4 && args[1] == "tune" {
        let epochs: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(20);
        let lambda: f64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        tune_weights(&args[2], &args[3], epochs, lambda);
        return;
    }
    let mut engine = uci::Engine::new();
    engine.run();
}

/// Debug helper: to_vec()/from_vec() must be exact inverses of each
/// other (same field order both ways) -- checked once here instead of
/// trusting it by inspection, since a mismatch would silently corrupt
/// every tuning run without ever panicking on a length assert.
fn check_weights_roundtrip() {
    let original = eval::default_weights().clone();
    let v = original.to_vec();
    println!("flat vector length: {}", v.len());
    if std::env::var("PRINT_DEFAULT_VEC").is_ok() {
        let s: Vec<String> = v.iter().map(|x| x.to_string()).collect();
        println!("{}", s.join(","));
    }
    let rebuilt = original.from_vec(&v);
    let v2 = rebuilt.to_vec();
    if v == v2 {
        println!("OK: to_vec/from_vec round-trip matches ({} scalars)", v.len());
    } else {
        println!("MISMATCH: round-trip does not match!");
        for (idx, (a, b)) in v.iter().zip(v2.iter()).enumerate() {
            if a != b {
                println!("  index {}: {} != {}", idx, a, b);
            }
        }
    }
    // Also confirm evaluate_with_weights(default) == evaluate() exactly
    // on a handful of real positions (checks the struct itself, not
    // just the vector round-trip).
    let atk = Attacks::new();
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
        "8/1p3Q1p/p3r3/2pk4/8/5K1P/Pb3PP1/7R b - - 0 30",
    ];
    for fen in fens {
        let board = Board::from_fen(fen);
        let a = eval::evaluate(&board);
        let b = eval::evaluate_with_weights(&board, &original);
        println!("fen ok={} eval()={} evaluate_with_weights(default)={}: {}", a == b, a, b, fen);
    }
    let _ = atk;
}

/// Dependency-free PRNG (same splitmix64 shape already used in
/// zobrist.rs for key generation -- this project deliberately has zero
/// crate dependencies, see Cargo.toml).
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Self-play data generation for Texel Tuning, structured after the
/// real Sirius datagen (src/datagen/datagen.cpp in the reference repo)
/// -- studied its APPROACH, not any tuned value: random opening (a few
/// random legal plies from startpos, discard+retry if that already
/// ends the game or leaves an unbalanced position) so games aren't all
/// the same handful of lines; a NODE limit per move rather than a wall-
/// clock limit, so generation speed and dataset quality are immune to
/// whatever else is running on the machine (the exact self-inflicted
/// CPU-contention problem found earlier this session with live bullet
/// games -- node limits sidestep it entirely for datagen); and
/// adjudication (stop a game early once the score has been decisively
/// one-sided or flat for several plies in a row, instead of always
/// playing to checkmate/50-move) so throughput isn't wasted grinding
/// out already-decided games. Runs `threads` games in parallel across
/// std::thread::scope for real wall-clock throughput.
fn selfplay_datagen(num_games: u32, out_path: &str, node_limit: u64, threads: usize) {
    use crate::search::{MATE_SCORE, MAX_PLY};
    let atk = Attacks::new();
    let zob = zobrist::Zobrist::new();
    let mate_threshold = MATE_SCORE - MAX_PLY as i32;

    println!("generating {} games, {} threads, {} nodes/move", num_games, threads, node_limit);
    let t0 = std::time::Instant::now();

    let games_per_thread = num_games.div_ceil(threads as u32);
    let results: Vec<Vec<(String, f64)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|tid| {
                let atk = &atk;
                let zob = &zob;
                scope.spawn(move || {
                    let mut rng_state: u64 = 0x9E3779B9u64
                        .wrapping_add(tid as u64)
                        .wrapping_add(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64);
                    let mut out = Vec::new();
                    for g in 0..games_per_thread {
                        if tid == 0 && g % 20 == 0 && g > 0 {
                            println!("  thread 0: {}/{} games, {:.1}s elapsed", g, games_per_thread, t0.elapsed().as_secs_f64());
                        }
                        let positions = play_one_selfplay_game(atk, zob, node_limit, &mut rng_state, mate_threshold);
                        out.extend(positions);
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let mut out_file = std::fs::File::create(out_path).expect("nao consegui criar o ficheiro de saida");
    let mut total = 0u64;
    for thread_positions in results {
        for (fen, res) in thread_positions {
            writeln!(out_file, "{}\t{}", fen, res).unwrap();
            total += 1;
        }
    }
    println!(
        "wrote {} positions from {} games in {:.1}s ({:.0} games/s)",
        total, num_games, t0.elapsed().as_secs_f64(), num_games as f64 / t0.elapsed().as_secs_f64()
    );
}

fn play_one_selfplay_game(
    atk: &Attacks,
    zob: &zobrist::Zobrist,
    node_limit: u64,
    rng_state: &mut u64,
    mate_threshold: i32,
) -> Vec<(String, f64)> {
    use crate::search::{Searcher, SearchLimits, CONT_HIST_SIZE, CORR_HIST_SIZE, MAX_PLY};
    use crate::types::Color;

    const MAX_OPENING_SCORE: i32 = 300;
    const WIN_ADJ_THRESHOLD: i32 = 2000;
    const WIN_ADJ_PLIES: i32 = 5;
    const DRAW_ADJ_THRESHOLD: i32 = 7;
    const DRAW_ADJ_MOVE_NUM: i32 = 50;
    const DRAW_ADJ_PLIES: i32 = 8;
    const MAX_GAME_PLIES: i32 = 300;
    const SKIP_OPENING_PLIES: i32 = 16;

    let (board_start, mut hash_history) = 'opening: loop {
        let mut board = Board::startpos();
        let mut hashes = vec![zob.hash(&board)];
        let mut ok = true;
        for _ in 0..8 {
            let legal = movegen::generate_legal(&mut board, atk);
            if legal.is_empty() {
                ok = false;
                break;
            }
            let idx = (splitmix64(rng_state) as usize) % legal.len();
            board.make_move(&legal[idx]);
            hashes.push(zob.hash(&board));
        }
        if !ok || movegen::generate_legal(&mut board, atk).is_empty() {
            continue 'opening;
        }
        break 'opening (board, hashes);
    };

    let mut board = board_start;
    let tt = tt::TranspositionTable::new(8);
    let mut positions: Vec<(String, Color)> = Vec::new();
    let mut win_plies = 0i32;
    let mut draw_plies = 0i32;
    let mut loss_plies = 0i32;
    let mut ply = 0i32;
    let result: f64;

    loop {
        let legal = movegen::generate_legal(&mut board, atk);
        if legal.is_empty() {
            result = if board.in_check(board.side, atk) {
                if board.side == Color::White { 0.0 } else { 1.0 }
            } else {
                0.5
            };
            break;
        }
        if board.halfmove >= 100 {
            result = 0.5;
            break;
        }
        let cur_hash = *hash_history.last().unwrap();
        if hash_history.iter().filter(|&&h| h == cur_hash).count() >= 3 {
            result = 0.5;
            break;
        }
        if ply >= MAX_GAME_PLIES {
            result = 0.5;
            break;
        }

        let mut searcher = Searcher {
            atk,
            zob,
            tt: &tt,
            nodes: 0,
            limits: SearchLimits { deadline: None, max_depth: 64, max_nodes: Some(node_limit), soft_deadline: None },
            stop: false,
            history: hash_history.clone(),
            killers: [[None; 2]; MAX_PLY],
            history_scores: [[[0; 64]; 64]; 2],
            countermoves: [[None; 64]; 6],
            cont_hist: vec![0i32; CONT_HIST_SIZE].into_boxed_slice(),
            corr_hist: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            ply_last_move: [None; MAX_PLY],
            static_evals: [0i32; MAX_PLY],
            root_best: None,
            excluded_move: None,
            excluded_root_moves: vec![],
            style_book: None,
            root_move_nodes: Vec::new(),
            capture_history: [[[0; 6]; 6]; 2],
            dextensions: [0; MAX_PLY],
        };
        let (best, score, _depth, _nodes) = searcher.iterative_deepening(&mut board);
        let Some(mv) = best else {
            result = 0.5;
            break;
        };
        let white_score = if board.side == Color::White { score } else { -score };

        if ply == 0 && white_score.abs() > MAX_OPENING_SCORE {
            // Unbalanced opening -- discard this whole game, start a
            // fresh one instead of forcing a lopsided line into the
            // dataset (same filter Sirius's datagen applies).
            return Vec::new();
        }

        if score.abs() >= mate_threshold {
            result = if white_score > 0 { 1.0 } else { 0.0 };
            break;
        }

        // Quiet-position filter (the real gap found after the first
        // tuning run regressed the tactical suite despite improving
        // held-out win/loss prediction): a position isn't a fair
        // static-eval target if it's in check, or if the engine's own
        // best move here is a capture -- either means the position is
        // still "hot" (its true value depends on resolving a tactic
        // the static eval alone can't see), not the settled quiet
        // position Texel tuning is supposed to be trained on. Classical
        // Texel/quiescence-search datasets specifically exclude these.
        if ply >= SKIP_OPENING_PLIES && !board.in_check(board.side, atk) && !mv.is_capture() && mv.promotion.is_none() {
            positions.push((board.to_fen(), board.side));
        }

        board.make_move(&mv);
        ply += 1;
        hash_history.push(zob.hash(&board));

        win_plies = if white_score >= WIN_ADJ_THRESHOLD { win_plies + 1 } else { 0 };
        draw_plies = if white_score.abs() < DRAW_ADJ_THRESHOLD && ply >= DRAW_ADJ_MOVE_NUM * 2 { draw_plies + 1 } else { 0 };
        loss_plies = if white_score <= -WIN_ADJ_THRESHOLD { loss_plies + 1 } else { 0 };

        if win_plies >= WIN_ADJ_PLIES {
            result = 1.0;
            break;
        }
        if draw_plies >= DRAW_ADJ_PLIES {
            result = 0.5;
            break;
        }
        if loss_plies >= WIN_ADJ_PLIES {
            result = 0.0;
            break;
        }
    }

    positions.into_iter().map(|(fen, _)| (fen, result)).collect()
}

/// Fast gradient-descent tuner: extracts a per-position LINEAR feature
/// vector once, then runs many cheap gradient steps as pure dot
/// products -- no more calls to the actual eval code per step. This is
/// the "convert eval into feature vector/coefficient dot product"
/// technique described on a talkchess.com thread the user pointed at
/// (studied the APPROACH, not any engine's tuned numbers): the
/// original poster reported ~9ms/iteration and full convergence in
/// ~20s after this exact conversion, versus tens of minutes with
/// Texel's per-parameter perturbation method (`tune_weights` above).
///
/// Why this is valid here: `positional_terms()` is linear in every
/// tunable field EXCEPT `king_attacker_weight`/`king_attacks`/
/// `safe_check`, which feed the deliberately nonlinear KING_DANGER_TABLE
/// lookup (see its comment in eval.rs). Those 12 scalars are held fixed
/// at their default values for this fast path -- not tuned here, kept
/// for the slower coordinate-descent tuner if they're ever revisited. For
/// every other field, `positional_terms(board, w) - positional_terms(board, w_with_field_zeroed)`
/// scales exactly linearly with that field's value, which is what lets
/// a single "unit contribution" per field be measured ONCE per
/// position and then reused for every future gradient step.
fn tune_fast(dataset_path: &str, out_path: &str, iters: u32, lr: f64) {
    let text = std::fs::read_to_string(dataset_path).expect("nao consegui ler o dataset");
    let mut boards: Vec<Board> = Vec::new();
    let mut results: Vec<f64> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let fen = parts.next().unwrap();
        let res: f64 = parts.next().unwrap().parse().unwrap();
        boards.push(Board::from_fen(fen));
        results.push(res);
    }
    let n_pos = boards.len();
    println!("dataset: {} positions", n_pos);

    let default = eval::default_weights().clone();
    let default_vec = default.to_vec();
    let dim = default_vec.len();

    // Find the flat indices king_attacker_weight/king_attacks occupy,
    // by marking them with a sentinel and reading to_vec() back --
    // avoids hardcoding offsets that would silently go stale if the
    // struct's field order ever changes.
    let mut sentinel = default.from_vec(&vec![0i32; dim]);
    sentinel.king_attacker_weight = [(1, 1); 4];
    sentinel.king_attacks = (1, 1);
    sentinel.safe_check = (1, 1);
    let sentinel_vec = sentinel.to_vec();
    let is_king_field: Vec<bool> = sentinel_vec.iter().map(|&x| x == 1).collect();
    let king_field_count = is_king_field.iter().filter(|&&b| b).count();
    println!("king-safety fields held fixed (nonlinear path, not tuned here): {}", king_field_count);

    // w_king_only: every non-king field zeroed, king fields at their
    // real default values -- the base point every linear probe is
    // measured relative to.
    let mut king_only_vec = vec![0i32; dim];
    for i in 0..dim {
        if is_king_field[i] {
            king_only_vec[i] = default_vec[i];
        }
    }
    let w_king_only = default.from_vec(&king_only_vec);

    println!("extracting linear features ({} probes/position, {} positions)...", dim - king_field_count + 1, n_pos);
    let t0 = std::time::Instant::now();
    // Per position: bias (material + king-safety-only positional term,
    // both in White's POV) and a feature vector (marginal contribution
    // of each non-king field at value=1, White's POV).
    let mut biases: Vec<f64> = Vec::with_capacity(n_pos);
    let mut features: Vec<Vec<f32>> = Vec::with_capacity(n_pos);
    let mut probe_vec = king_only_vec.clone();
    for board in &boards {
        let p_base = eval::positional_terms(board, &w_king_only);
        let bias = eval::material_pst_white(board) as f64 + p_base as f64;
        let mut f = vec![0f32; dim];
        for i in 0..dim {
            if is_king_field[i] {
                continue;
            }
            probe_vec[i] = 1;
            let w_probe = w_king_only.from_vec(&probe_vec);
            let p_unit = eval::positional_terms(board, &w_probe);
            f[i] = (p_unit - p_base) as f32;
            probe_vec[i] = king_only_vec[i];
        }
        biases.push(bias);
        features.push(f);
    }
    println!("feature extraction done in {:.1}s", t0.elapsed().as_secs_f64());

    if std::env::var("TUNEFAST_DEBUG_CHECK").is_ok() {
        for (i, board) in boards.iter().enumerate() {
            let full = eval::evaluate_with_weights(board, &default);
            let full_white = if board.side == types::Color::White { full } else { -full };
            let mut e = biases[i];
            for j in 0..dim {
                if features[i][j] != 0.0 {
                    e += default_vec[j] as f64 * features[i][j] as f64;
                }
            }
            println!("pos {}: evaluate_with_weights(white)={}  linear_decomp={:.3}  diff={:.3}", i, full_white, e, full_white as f64 - e);
        }
    }

    // Best sigmoid K for the default weights, same coarse scan as the
    // slow tuner -- fixed for the rest of the run.
    fn sigmoid(x: f64, k: f64) -> f64 {
        1.0 / (1.0 + 10f64.powf(-k * x / 400.0))
    }
    let mut w: Vec<f64> = default_vec.iter().map(|&x| x as f64).collect();
    let predict = |w: &[f64], i: usize| -> f64 {
        let mut e = biases[i];
        let f = &features[i];
        for j in 0..dim {
            if f[j] != 0.0 {
                e += w[j] * f[j] as f64;
            }
        }
        e
    };
    let mean_error = |w: &[f64], k: f64| -> f64 {
        let mut sum = 0.0;
        for i in 0..n_pos {
            let d = results[i] - sigmoid(predict(w, i), k);
            sum += d * d;
        }
        sum / n_pos as f64
    };
    let mut best_k = 1.0;
    let mut best_k_err = f64::MAX;
    let mut k = 0.2;
    while k <= 3.0 {
        let e = mean_error(&w, k);
        if e < best_k_err {
            best_k_err = e;
            best_k = k;
        }
        k += 0.1;
    }
    println!("best K = {:.2}  (starting error = {:.6})", best_k, best_k_err);

    let ln10 = std::f64::consts::LN_10;
    let mut grad = vec![0f64; dim];
    let t1 = std::time::Instant::now();
    for iter in 0..iters {
        for g in grad.iter_mut() {
            *g = 0.0;
        }
        for i in 0..n_pos {
            let pred_eval = predict(&w, i);
            let s = sigmoid(pred_eval, best_k);
            // d(loss)/d(eval) for loss=(result-sigmoid(eval))^2
            let d_loss_d_eval = 2.0 * (s - results[i]) * (best_k * ln10 / 400.0) * s * (1.0 - s);
            let f = &features[i];
            for j in 0..dim {
                if f[j] != 0.0 {
                    grad[j] += d_loss_d_eval * f[j] as f64;
                }
            }
        }
        for j in 0..dim {
            if is_king_field[j] {
                continue;
            }
            w[j] -= lr * grad[j] / n_pos as f64;
        }
        if iter % 200 == 0 || iter == iters - 1 {
            println!("iter {}: error={:.6}  ({:.2}s)", iter, mean_error(&w, best_k), t1.elapsed().as_secs_f64());
        }
    }

    let final_err = mean_error(&w, best_k);
    println!("final error: {:.6} (started {:.6}) in {:.2}s, {} iterations", final_err, best_k_err, t1.elapsed().as_secs_f64(), iters);

    let out_vec: Vec<i32> = w.iter().map(|&x| x.round() as i32).collect();
    let serialized: Vec<String> = out_vec.iter().map(|x| x.to_string()).collect();
    std::fs::write(out_path, serialized.join(",")).expect("nao consegui escrever o output");
    println!("wrote tuned weights ({} scalars) to {}", out_vec.len(), out_path);
}

/// Real Texel Tuning: coordinate descent on `Weights::to_vec()`'s flat
/// parameter vector, minimizing squared error between the sigmoid of
/// each position's static eval and the REAL game result it came from
/// (1.0/0.5/0.0 from White's perspective). Classic method (the
/// original Texel tuner and most small engines' tuners work exactly
/// this way -- no autodiff needed): for each parameter, try +step and
/// -step, keep whichever reduces total error over the whole dataset,
/// else leave it unchanged. Dataset format: one line per position,
/// "<FEN>\t<white_score>".
fn tune_weights(dataset_path: &str, out_path: &str, epochs: u32, lambda: f64) {
    let text = std::fs::read_to_string(dataset_path).expect("nao consegui ler o dataset");
    let mut boards: Vec<Board> = Vec::new();
    let mut results: Vec<f64> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let fen = parts.next().unwrap();
        let res: f64 = parts.next().unwrap().parse().unwrap();
        boards.push(Board::from_fen(fen));
        results.push(res);
    }
    println!("dataset: {} positions", boards.len());

    // sigmoid(eval) = 1 / (1 + 10^(-k*eval/400)) -- eval from White's POV.
    fn sigmoid(eval_cp: f64, k: f64) -> f64 {
        1.0 / (1.0 + 10f64.powf(-k * eval_cp / 400.0))
    }
    fn white_eval(board: &Board, w: &eval::Weights) -> f64 {
        let e = eval::evaluate_with_weights(board, w);
        if board.side == types::Color::White { e as f64 } else { -e as f64 }
    }
    fn total_error(boards: &[Board], results: &[f64], w: &eval::Weights, k: f64) -> f64 {
        let mut sum = 0.0;
        for (b, r) in boards.iter().zip(results.iter()) {
            let pred = sigmoid(white_eval(b, w), k);
            let d = r - pred;
            sum += d * d;
        }
        sum / boards.len() as f64
    }

    let base = eval::default_weights().clone();
    let default_vec = base.to_vec();

    // L2 regularization toward the hand-derived defaults: found AFTER
    // the first real tuning run that unregularized coordinate descent
    // let several parameters drift by exactly the same +/-1 every
    // single epoch for all 20 epochs (e.g. mobility_knight's "0 legal
    // moves" penalty roughly halved, its "1 legal move" entry flipped
    // sign from penalty to bonus) -- never reversing direction, i.e.
    // never actually converging to a nearby optimum, just sliding
    // along an unconstrained slope shaped by this specific self-play
    // distribution. That run's tuned weights held up on a held-out
    // set drawn from the SAME weak self-player but regressed the
    // tactical suite hard (82.6% -> 73.9%, reproducible). `lambda`>0
    // adds a quadratic penalty for straying from the reasoned starting
    // point, so a parameter only keeps moving if the fit improvement
    // clearly outweighs the distance traveled -- lambda=0 reproduces
    // the original unregularized behavior exactly.
    fn regularized(err: f64, v: &[i32], default_vec: &[i32], lambda: f64) -> f64 {
        if lambda == 0.0 {
            return err;
        }
        let mut penalty = 0.0;
        for (a, b) in v.iter().zip(default_vec.iter()) {
            let d = (a - b) as f64;
            penalty += d * d;
        }
        err + lambda * penalty / v.len() as f64
    }

    // Find the best sigmoid scale K for the CURRENT (untuned) weights
    // first -- a coarse 1D scan, fixed for the rest of the run (this is
    // what the original Texel tuner does: K only rescales how harshly
    // error is measured, tuning it jointly with every other parameter
    // every step is unnecessary).
    let mut best_k = 1.0;
    let mut best_k_err = f64::MAX;
    let mut k = 0.2;
    while k <= 3.0 {
        let e = total_error(&boards, &results, &base, k);
        if e < best_k_err {
            best_k_err = e;
            best_k = k;
        }
        k += 0.1;
    }
    println!("best K = {:.2}  (error at default weights = {:.6})", best_k, best_k_err);

    let mut v = base.to_vec();
    let mut current = base.from_vec(&v);
    let mut current_err = total_error(&boards, &results, &current, best_k);
    let mut current_obj = regularized(current_err, &v, &default_vec, lambda);
    println!("starting error: {:.6}  (lambda={}, objective={:.6})", current_err, lambda, current_obj);

    for epoch in 0..epochs {
        let mut improved = 0;
        for i in 0..v.len() {
            let orig = v[i];
            v[i] = orig + 1;
            let cand = current.from_vec(&v);
            let err_up = total_error(&boards, &results, &cand, best_k);
            let obj_up = regularized(err_up, &v, &default_vec, lambda);
            if obj_up < current_obj {
                current_err = err_up;
                current_obj = obj_up;
                current = cand;
                improved += 1;
                continue;
            }
            v[i] = orig - 1;
            let cand = current.from_vec(&v);
            let err_down = total_error(&boards, &results, &cand, best_k);
            let obj_down = regularized(err_down, &v, &default_vec, lambda);
            if obj_down < current_obj {
                current_err = err_down;
                current_obj = obj_down;
                current = cand;
                improved += 1;
                continue;
            }
            v[i] = orig;
        }
        println!("epoch {}: error={:.6}  objective={:.6}  params improved={}", epoch, current_err, current_obj, improved);
        if improved == 0 {
            println!("converged (no parameter improved this epoch)");
            break;
        }
    }

    let out_vec = current.to_vec();
    let serialized: Vec<String> = out_vec.iter().map(|x| x.to_string()).collect();
    std::fs::write(out_path, serialized.join(",")).expect("nao consegui escrever o output");
    println!("wrote tuned weights ({} scalars) to {}", out_vec.len(), out_path);
    println!("final error: {:.6}  (started at {:.6}, default-K error {:.6})", current_err, current_err, best_k_err);
}

/// Resolve every position in a `kestrel tune`-format dataset (`<fen>\t
/// <result>` per line) to its quiescence leaf before tuning touches it.
/// Standard Texel practice is to label the QSEARCH-resolved position,
/// not whatever the sampler happened to land on -- a position mid
/// tactical exchange (about to lose/win material next move) has a
/// static eval that doesn't match its true value, and no amount of
/// tuning-loop regularization fixes a mislabeled example. This is a
/// ONE-TIME pass over the dataset (cheap: one quiescence search per
/// position), not per-parameter-trial -- running full quiescence at
/// every coordinate-descent step (~920 trials/epoch x 20 epochs x
/// dataset size) would be 1000x+ more expensive and wasn't tractable
/// in the time available. This gets the main practical benefit
/// (positions are guaranteed tactically settled) without that cost;
/// `tune`/`tunefast` afterward are unchanged, still score with
/// `evaluate_with_weights`, just on cleaner input.
fn resolve_quiet_dataset(in_path: &str, out_path: &str) {
    use crate::search::{Searcher, SearchLimits, CONT_HIST_SIZE, CORR_HIST_SIZE, MAX_PLY, MATE_SCORE};
    let atk = Attacks::new();
    let zob = zobrist::Zobrist::new();
    let tt = tt::TranspositionTable::new(1);

    let text = std::fs::read_to_string(in_path).expect("nao consegui ler o dataset");
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    println!("resolving {} positions to quiescence leaves...", lines.len());
    let t0 = std::time::Instant::now();

    let mut out = String::new();
    let mut skipped = 0u32;
    for (i, line) in lines.iter().enumerate() {
        let mut parts = line.split('\t');
        let fen = parts.next().unwrap();
        let res = parts.next().unwrap();
        let mut board = Board::from_fen(fen);

        let mut searcher = Searcher {
            atk: &atk,
            zob: &zob,
            tt: &tt,
            nodes: 0,
            limits: SearchLimits { deadline: None, max_depth: 64, max_nodes: None, soft_deadline: None },
            stop: false,
            history: Vec::new(),
            killers: [[None; 2]; MAX_PLY],
            history_scores: [[[0; 64]; 64]; 2],
            countermoves: [[None; 64]; 6],
            cont_hist: vec![0i32; CONT_HIST_SIZE].into_boxed_slice(),
            corr_hist: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            ply_last_move: [None; MAX_PLY],
            static_evals: [0i32; MAX_PLY],
            root_best: None,
            excluded_move: None,
            excluded_root_moves: vec![],
            style_book: None,
            root_move_nodes: Vec::new(),
            capture_history: [[[0; 6]; 6]; 2],
            dextensions: [0; MAX_PLY],
        };
        let (score, leaf) = searcher.quiescence_leaf(&mut board, -MATE_SCORE, MATE_SCORE, 0);
        if score.abs() >= MATE_SCORE - MAX_PLY as i32 {
            // Forced mate found inside quiescence -- drop it, same filter
            // tune_weights's own dataset-reading loop would want (a
            // position where the game is already tactically decided
            // isn't useful signal for eval-weight tuning).
            skipped += 1;
            continue;
        }
        out.push_str(&leaf.to_fen());
        out.push('\t');
        out.push_str(res);
        out.push('\n');
        if (i + 1) % 5000 == 0 {
            println!("  {}/{} ({:.0}s)", i + 1, lines.len(), t0.elapsed().as_secs_f64());
        }
    }
    std::fs::write(out_path, &out).expect("nao consegui escrever o output");
    println!("wrote {} quiet-resolved positions ({} skipped as forced mate) to {} in {:.0}s",
        lines.len() as u32 - skipped, skipped, out_path, t0.elapsed().as_secs_f64());
}

/// Debug helper: does `book_path` have an entry for `fen`? Prints the
/// move(s)/counts found or "no entry" -- used to check coverage
/// questions ("was this exact opening position in the source games?")
/// without writing a one-off script each time.
fn lookup_book(book_path: &str, fen: &str) {
    let zob = Zobrist::new();
    let board = Board::from_fen(fen);
    let hash = zob.hash(&board);
    let bk = book::Book::load(book_path).expect("nao consegui carregar o livro");
    let entries = bk.lookup(hash);
    if entries.is_empty() {
        println!("no entry for this position");
        return;
    }
    for (m16, cnt) in entries {
        let (from, to, promo) = book::decode_move16(m16);
        println!("{}{}{} count={}", crate::types::sq_name(from), crate::types::sq_name(to),
            promo.map(|p| format!("={:?}", p)).unwrap_or_default(), cnt);
    }
}

/// Le' um ficheiro de jogos (um por linha, lances UCI separados por
/// espaco -- ver extract_polgar_moves.py) e constroi um livro binario
/// KESTBK01 (posicao -> lance -> contagem), usando o zobrist PROPRIO do
/// kestrel (nao o polyglot do troller) para ser diretamente compativel
/// com a busca. Ver book.rs para o formato exato.
fn build_book(games_path: &str, out_path: &str) {
    let atk = Attacks::new();
    let zob = Zobrist::new();
    let f = std::fs::File::open(games_path).expect("nao consegui abrir o ficheiro de jogos");
    let reader = std::io::BufReader::new(f);

    let mut counts: HashMap<u64, HashMap<u16, u32>> = HashMap::new();
    let mut n_games = 0u64;
    let mut n_moves = 0u64;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut board = Board::startpos();
        let mut ok = true;
        for tok in line.split_whitespace() {
            let hash_before = zob.hash(&board);
            let legal = movegen::generate_legal(&mut board, &atk);
            let mv = match legal.iter().find(|m| m.to_uci() == tok) {
                Some(m) => *m,
                None => {
                    ok = false;
                    break;
                }
            };
            let m16 = book::encode_move(&mv);
            *counts.entry(hash_before).or_default().entry(m16).or_insert(0) += 1;
            board.make_move(&mv);
            n_moves += 1;
        }
        if ok {
            n_games += 1;
        }
    }

    let mut keys: Vec<u64> = counts.keys().copied().collect();
    keys.sort_unstable();

    let mut out = std::fs::File::create(out_path).expect("nao consegui criar o ficheiro de saida");
    let mut n_records = 0u64;
    for &k in &keys {
        n_records += counts[&k].len() as u64;
    }
    out.write_all(book::MAGIC).unwrap();
    out.write_all(&n_records.to_be_bytes()).unwrap();
    for &k in &keys {
        let mut moves: Vec<(&u16, &u32)> = counts[&k].iter().collect();
        moves.sort_by_key(|(m, _)| **m);
        for (m16, cnt) in moves {
            out.write_all(&k.to_be_bytes()).unwrap();
            out.write_all(&m16.to_be_bytes()).unwrap();
            out.write_all(&cnt.to_be_bytes()).unwrap();
        }
    }
    println!(
        "livro construido: {} jogos, {} lances processados, {} posicoes unicas, {} registos -> {}",
        n_games, n_moves, keys.len(), n_records, out_path
    );
}
