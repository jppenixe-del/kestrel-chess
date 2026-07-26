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
    if args.len() >= 2 && args[1] == "checkmatpst" {
        check_matpst_features();
        return;
    }
    if args.len() >= 3 && args[1] == "dumpweights" {
        // Current tunable weights, comma separated -- the starting point for
        // an external tuner, so a fit begins from what the engine already
        // believes rather than from nothing.
        let v = eval::default_weights().to_vec();
        let text: Vec<String> = v.iter().map(|x| x.to_string()).collect();
        std::fs::write(&args[2], text.join(",")).expect("nao consegui escrever");
        println!("wrote {} weights to {}", v.len(), args[2]);
        return;
    }
    if args.len() >= 3 && args[1] == "linprobe" {
        linearity_probe(&args[2]);
        return;
    }
    if args.len() >= 4 && args[1] == "gpuextract" {
        // gpuextract <dataset.epd> <out.bin> [max_positions] [buckets] [threads]
        let maxp: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
        let buckets: usize = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(1);
        let threads: usize = args.get(6).and_then(|s| s.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
        gpu_extract(&args[2], &args[3], maxp, buckets.max(1), threads);
        return;
    }
    if args.len() >= 4 && args[1] == "tunepst" {
        let iters: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(8000);
        let lr: f64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(1000.0);
        tune_matpst(&args[2], &args[3], iters, lr);
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
    if args.len() >= 4 && args[1] == "selfplaytc" {
        // 2026-07-23: real time-control self-play, per the standard
        // eval-tuning datagen method the user described (games at a
        // fast real clock, not a node cap) -- separate from
        // `selfplay` above (node-limited, kept for fast iteration)
        // since a real clock changes the per-move budget logic
        // entirely and shouldn't touch that already-working path.
        let num_games: u32 = args[2].parse().expect("num_games invalido");
        let out_path = &args[3];
        let base_ms: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1000);
        let inc_ms: u64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(80);
        let threads: usize = args.get(6).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
        });
        selfplay_datagen_tc(num_games, out_path, base_ms, inc_ms, threads);
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
    if args.len() >= 4 && args[1] == "tunestream" {
        // streaming logistic tuner (RAM-constant, for large external binpack datasets):
        //   tunestream <dataset.epd> <out.txt> [epochs] [lr] [chunk] [threads]
        let epochs: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(6);
        let lr: f64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(2.0);
        let chunk: usize = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(50000);
        let threads: usize = args.get(7).and_then(|s| s.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
        tune_stream(&args[2], &args[3], epochs, lr, chunk, threads);
        return;
    }
    if args.len() >= 4 && args[1] == "tunefull" {
        // streaming logistic tuner over the FULL eval (material+PST AND
        // positional together), same streaming/sparse mechanics as
        // tunestream: tunefull <dataset.epd> <out.txt> [iters] [lr] [chunk] [threads]
        let iters: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(6);
        let lr: f64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(2.0);
        let chunk: usize = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(50000);
        let threads: usize = args.get(7).and_then(|s| s.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
        tune_full(&args[2], &args[3], iters, lr, chunk, threads);
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

/// Valida que `material_pst_features` esta' correcta: para varias
/// posicoes, `sum(feats[i] * material_pst_current_vec()[i])` tem de bater
/// com `material_pst_white(board)` (a menos do arredondamento inteiro do
/// taper). Se isto falhar, o tuner de material/PST esta' a extrair as
/// features erradas e nao vale a pena correr.
fn check_matpst_features() {
    let cur = eval::material_pst_current_vec();
    println!("MAT_PST_DIM = {}, current_vec len = {}", eval::MAT_PST_DIM, cur.len());
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
        "8/1p3Q1p/p3r3/2pk4/8/5K1P/Pb3PP1/7R b - - 0 30",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/8/8/4k3/8/4K3/4P3/8 w - - 0 1",
    ];
    let mut feats = vec![0f32; eval::MAT_PST_DIM];
    let mut all_ok = true;
    for fen in fens {
        let board = Board::from_fen(fen);
        eval::material_pst_features(&board, &mut feats);
        let dot: f64 = feats.iter().zip(cur.iter()).map(|(&f, &v)| f as f64 * v as f64).sum();
        let real = eval::material_pst_white(&board) as f64;
        let diff = (dot - real).abs();
        let ok = diff <= 1.5; // tolerancia do arredondamento inteiro do taper
        if !ok { all_ok = false; }
        println!("ok={} feat_dot={:.2} material_pst_white={:.0} diff={:.2}: {}", ok, dot, real, diff, fen);
    }
    println!("{}", if all_ok { "MATPST FEATURES OK" } else { "MATPST FEATURES ERRADAS -- nao tunar!" });
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

/// Self-play data generation for eval tuning, using the standard
/// datagen approach: random opening (a few
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
            corr_hist_np_stm: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_np_nstm: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_minor: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_major: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_threats: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            ply_last_move: [None; MAX_PLY],
            static_evals: [0i32; MAX_PLY],
            root_best: None,
            excluded_move: None,
            excluded_root_moves: vec![],
            style_book: None,
            root_move_nodes: Vec::new(),
            capture_history: [[[0; 6]; 6]; 2],
            dextensions: [0; MAX_PLY],
            report: false, // offline tools: no UCI narration
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
            // dataset (a standard datagen filter).
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
        // position eval tuning is supposed to be trained on. Quiet-only
        // datasets specifically exclude these.
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

/// Real time-control self-play datagen, per the standard eval-tuning
/// method (base+increment clock per side, e.g. 1000ms+80ms, instead of
/// a node cap) -- structurally the same game loop/filters as
/// `selfplay_datagen`/`play_one_selfplay_game` above (random 8-ply
/// opening, mate/repetition/50-move/adjudication endings, quiet-only
/// position filter, unbalanced-opening discard), only the per-move
/// search budget changes.
fn selfplay_datagen_tc(num_games: u32, out_path: &str, base_ms: u64, inc_ms: u64, threads: usize) {
    use crate::search::{MATE_SCORE, MAX_PLY};
    let atk = Attacks::new();
    let zob = zobrist::Zobrist::new();
    let mate_threshold = MATE_SCORE - MAX_PLY as i32;

    println!("generating {} games, {} threads, {}ms+{}ms/move time control", num_games, threads, base_ms, inc_ms);
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
                        if tid == 0 && g % 50 == 0 && g > 0 {
                            let done = (g as u64) * (threads as u64);
                            println!("  thread 0: {}/{} games, {:.1}s elapsed, ~{:.0} games/s", g, games_per_thread, t0.elapsed().as_secs_f64(), done as f64 / t0.elapsed().as_secs_f64().max(0.001));
                        }
                        let positions = play_one_selfplay_game_tc(atk, zob, base_ms, inc_ms, &mut rng_state, mate_threshold);
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
        "wrote {} positions from {} games in {:.1}s ({:.1} games/s)",
        total, num_games, t0.elapsed().as_secs_f64(), num_games as f64 / t0.elapsed().as_secs_f64()
    );
}

fn play_one_selfplay_game_tc(
    atk: &Attacks,
    zob: &zobrist::Zobrist,
    base_ms: u64,
    inc_ms: u64,
    rng_state: &mut u64,
    mate_threshold: i32,
) -> Vec<(String, f64)> {
    use crate::search::{Searcher, SearchLimits, CONT_HIST_SIZE, CORR_HIST_SIZE, MAX_PLY};
    use crate::types::Color;
    use std::time::{Duration, Instant};

    const MAX_OPENING_SCORE: i32 = 300;
    const WIN_ADJ_THRESHOLD: i32 = 2000;
    const WIN_ADJ_PLIES: i32 = 5;
    const DRAW_ADJ_THRESHOLD: i32 = 7;
    const DRAW_ADJ_MOVE_NUM: i32 = 50;
    const DRAW_ADJ_PLIES: i32 = 8;
    const MAX_GAME_PLIES: i32 = 300;
    const SKIP_OPENING_PLIES: i32 = 16;
    // Same shape as compute_time_budget's Nivel-1 formula in uci.rs
    // (elastic: remaining/moves_left + a share of the increment), just
    // without the panic/low-clock tiers -- at base_ms=1000 those tiers
    // would dominate almost every move, which isn't the point here
    // (real games at this time control ARE mostly "panic mode" by
    // uci.rs's own thresholds, that's expected and fine for datagen).
    const MOVES_LEFT_ESTIMATE: u64 = 30;

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
    let mut clock_ms = [base_ms, base_ms]; // [white, black]
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

        let stm = board.side.idx();
        if clock_ms[stm] == 0 {
            // Time forfeit -- rare at this budget but must be handled.
            result = if board.side == Color::White { 0.0 } else { 1.0 };
            break;
        }
        let budget_ms = (clock_ms[stm] / MOVES_LEFT_ESTIMATE + inc_ms * 3 / 4).clamp(1, clock_ms[stm]);
        let move_t0 = Instant::now();

        let mut searcher = Searcher {
            atk,
            zob,
            tt: &tt,
            nodes: 0,
            limits: SearchLimits {
                deadline: Some(move_t0 + Duration::from_millis(budget_ms)),
                max_depth: 64,
                max_nodes: None,
                soft_deadline: None,
            },
            stop: false,
            history: hash_history.clone(),
            killers: [[None; 2]; MAX_PLY],
            history_scores: [[[0; 64]; 64]; 2],
            countermoves: [[None; 64]; 6],
            cont_hist: vec![0i32; CONT_HIST_SIZE].into_boxed_slice(),
            corr_hist: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_np_stm: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_np_nstm: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_minor: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_major: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_threats: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            ply_last_move: [None; MAX_PLY],
            static_evals: [0i32; MAX_PLY],
            root_best: None,
            excluded_move: None,
            excluded_root_moves: vec![],
            style_book: None,
            root_move_nodes: Vec::new(),
            capture_history: [[[0; 6]; 6]; 2],
            dextensions: [0; MAX_PLY],
            report: false, // offline tools: no UCI narration
        };
        let (best, score, _depth, _nodes) = searcher.iterative_deepening(&mut board);
        let elapsed_ms = move_t0.elapsed().as_millis() as u64;
        clock_ms[stm] = clock_ms[stm].saturating_sub(elapsed_ms).saturating_add(inc_ms);
        let Some(mv) = best else {
            result = 0.5;
            break;
        };
        let white_score = if board.side == Color::White { score } else { -score };

        if ply == 0 && white_score.abs() > MAX_OPENING_SCORE {
            return Vec::new();
        }

        if score.abs() >= mate_threshold {
            result = if white_score > 0 { 1.0 } else { 0.0 };
            break;
        }

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
/// technique: full convergence in seconds, versus tens of minutes with
/// the per-parameter perturbation method (`tune_weights` above).
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
    sentinel.safe_knight_check = (1, 1);
    sentinel.safe_bishop_check = (1, 1);
    sentinel.safe_rook_check = (1, 1);
    sentinel.safe_queen_check = (1, 1);
    // 2026-07-26: king safety stopped being a linear sum. Shelter, storm,
    // the weak-ring count and the flank counts now feed the same danger
    // curve the attack units do, instead of being added straight to the
    // score -- so a one-unit probe of any of them no longer measures that
    // field's contribution, it measures a slope somewhere on a curve. Left
    // off this list they would be tuned as if linear, quietly and wrongly.
    sentinel.pawn_shelter = [(1, 1); 4];
    sentinel.shelter_open = (1, 1);
    sentinel.pawn_storm = [(1, 1); 4];
    sentinel.weak_king_ring = (1, 1);
    sentinel.king_flank_attacks = [(1, 1); 2];
    sentinel.king_flank_defenses = [(1, 1); 2];
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

/// Streaming logistic tuner: same linear model as `tune_fast`, but never
/// holds the whole dataset (or its dense feature vectors) in RAM. Reads
/// the EPD in chunks, extracts features + accumulates the gradient for
/// each chunk in PARALLEL (thread::scope, no rayon dependency), applies a
/// mini-batch update per chunk, and discards it. RAM is O(chunk_size), so
/// arbitrarily large external binpack-derived datasets can be used
/// (stream, don't load). Material stays fixed
/// (const, outside the weight vector); king-safety fields stay fixed
/// (nonlinear KING_DANGER_TABLE path). Mini-batch updates mean a few
/// epochs converge, so the expensive per-position probing is paid only a
/// handful of times over the whole set.
fn tune_stream(dataset_path: &str, out_path: &str, epochs: u32, lr: f64, chunk_size: usize, threads: usize) {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let default = eval::default_weights().clone();
    let default_vec = default.to_vec();
    let dim = default_vec.len();

    // king-safety fields held fixed (same sentinel trick as tune_fast)
    let mut sentinel = default.from_vec(&vec![0i32; dim]);
    sentinel.king_attacker_weight = [(1, 1); 4];
    sentinel.king_attacks = (1, 1);
    sentinel.safe_knight_check = (1, 1);
    sentinel.safe_bishop_check = (1, 1);
    sentinel.safe_rook_check = (1, 1);
    sentinel.safe_queen_check = (1, 1);
    // 2026-07-26: king safety stopped being a linear sum. Shelter, storm,
    // the weak-ring count and the flank counts now feed the same danger
    // curve the attack units do, instead of being added straight to the
    // score -- so a one-unit probe of any of them no longer measures that
    // field's contribution, it measures a slope somewhere on a curve. Left
    // off this list they would be tuned as if linear, quietly and wrongly.
    sentinel.pawn_shelter = [(1, 1); 4];
    sentinel.shelter_open = (1, 1);
    sentinel.pawn_storm = [(1, 1); 4];
    sentinel.weak_king_ring = (1, 1);
    sentinel.king_flank_attacks = [(1, 1); 2];
    sentinel.king_flank_defenses = [(1, 1); 2];
    let sentinel_vec = sentinel.to_vec();
    let is_king_field: Vec<bool> = sentinel_vec.iter().map(|&x| x == 1).collect();
    let king_field_count = is_king_field.iter().filter(|&&b| b).count();
    let mut king_only_vec = vec![0i32; dim];
    for i in 0..dim {
        if is_king_field[i] {
            king_only_vec[i] = default_vec[i];
        }
    }
    let w_king_only = default.from_vec(&king_only_vec);
    println!("tune_stream: dim={}, king fields fixed={}, chunk={}, threads={}", dim, king_field_count, chunk_size, threads);

    // extract (bias, dense feature vec) for one board via linear probing
    let extract = |board: &Board| -> (f64, Vec<f32>) {
        let p_base = eval::positional_terms(board, &w_king_only);
        let bias = eval::material_pst_white(board) as f64 + p_base as f64;
        let mut f = vec![0f32; dim];
        let mut probe_vec = king_only_vec.clone();
        for i in 0..dim {
            if is_king_field[i] {
                continue;
            }
            probe_vec[i] = 1;
            let w_probe = w_king_only.from_vec(&probe_vec);
            f[i] = (eval::positional_terms(board, &w_probe) - p_base) as f32;
            probe_vec[i] = king_only_vec[i];
        }
        (bias, f)
    };

    fn sigmoid(x: f64, k: f64) -> f64 {
        1.0 / (1.0 + 10f64.powf(-k * x / 400.0))
    }
    let ln10 = 10f64.ln();

    // fit K on a small sample (serial, quick)
    let mut sample: Vec<(f64, Vec<f32>, f64)> = Vec::new();
    {
        let f = File::open(dataset_path).expect("abrir dataset");
        for line in BufReader::new(f).lines().take(30000) {
            let line = line.unwrap();
            let l = line.trim();
            if l.is_empty() { continue; }
            let mut parts = l.split('\t');
            let fen = parts.next().unwrap();
            let target: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.5);
            let board = Board::from_fen(fen);
            let (bias, feats) = extract(&board);
            sample.push((bias, feats, target));
        }
    }
    let w0: Vec<f64> = default_vec.iter().map(|&x| x as f64).collect();
    let sample_err = |k: f64| -> f64 {
        let mut s = 0.0;
        for (bias, feats, target) in &sample {
            let mut pred = *bias;
            for j in 0..dim { pred += w0[j] * feats[j] as f64; }
            let d = target - sigmoid(pred, k);
            s += d * d;
        }
        s / sample.len() as f64
    };
    let mut best_k = 1.0;
    let mut best_e = f64::MAX;
    let mut k = 0.4;
    while k <= 3.0 {
        let e = sample_err(k);
        if e < best_e { best_e = e; best_k = k; }
        k += 0.1;
    }
    println!("fit K = {:.2} (sample error {:.6}, {} sample positions)", best_k, best_e, sample.len());

    let t0 = std::time::Instant::now();

    // PHASE 1: extract SPARSE features once (stream from disk in chunks,
    // extract in parallel). cache: (bias, [(idx,val)], target). Sparse, so
    // millions of positions fit in RAM (dense 669/pos would not). The
    // expensive probing is paid exactly once for the whole set.
    println!("extracting sparse features (parallel, {} threads)...", threads);
    let mut cache: Vec<(f64, Vec<(u16, f32)>, f64)> = Vec::new();
    {
        let file = File::open(dataset_path).expect("abrir dataset");
        let mut reader = BufReader::new(file);
        let mut raw: Vec<(Board, f64)> = Vec::with_capacity(chunk_size);
        let mut line = String::new();
        let extract_chunk = |raw: &[(Board, f64)], cache: &mut Vec<(f64, Vec<(u16, f32)>, f64)>| {
            let n = raw.len();
            if n == 0 { return; }
            let kf = &is_king_field;
            let kov = &king_only_vec;
            let wko = &w_king_only;
            let parts: Vec<Vec<(f64, Vec<(u16, f32)>, f64)>> = std::thread::scope(|scope| {
                let per = (n + threads - 1) / threads;
                let mut handles = Vec::new();
                for t in 0..threads {
                    let start = t * per;
                    let end = ((t + 1) * per).min(n);
                    if start >= end { continue; }
                    let slice = &raw[start..end];
                    handles.push(scope.spawn(move || {
                        let mut out = Vec::with_capacity(slice.len());
                        let mut probe_vec = kov.clone();
                        for (board, target) in slice {
                            let p_base = eval::positional_terms(board, wko);
                            let bias = eval::material_pst_white(board) as f64 + p_base as f64;
                            let mut sp: Vec<(u16, f32)> = Vec::new();
                            for i in 0..dim {
                                if kf[i] { continue; }
                                probe_vec[i] = 1;
                                let fi = (eval::positional_terms(board, &wko.from_vec(&probe_vec)) - p_base) as f32;
                                probe_vec[i] = kov[i];
                                if fi != 0.0 { sp.push((i as u16, fi)); }
                            }
                            out.push((bias, sp, *target));
                        }
                        out
                    }));
                }
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            for p in parts { cache.extend(p); }
        };
        let mut more = true;
        while more {
            line.clear();
            let nread = reader.read_line(&mut line).expect("read");
            if nread == 0 {
                more = false;
            } else {
                let l = line.trim();
                if !l.is_empty() {
                    let mut ps = l.split('\t');
                    let fen = ps.next().unwrap();
                    let target: f64 = ps.next().and_then(|s| s.parse().ok()).unwrap_or(0.5);
                    raw.push((Board::from_fen(fen), target));
                }
            }
            if raw.len() >= chunk_size || (!more && !raw.is_empty()) {
                let before = cache.len();
                extract_chunk(&raw, &mut cache);
                raw.clear();
                if cache.len() / 500000 != before / 500000 {
                    println!("  extracted {} positions ({:.1}s)...", cache.len(), t0.elapsed().as_secs_f64());
                }
            }
        }
    }
    let n_pos = cache.len();
    println!("sparse feature extraction done: {} positions in {:.1}s", n_pos, t0.elapsed().as_secs_f64());
    if n_pos == 0 { eprintln!("dataset vazio"); return; }

    // PHASE 2: fast full-batch gradient iterations over the sparse cache.
    let mut w: Vec<f64> = default_vec.iter().map(|&x| x as f64).collect();
    // Mean-centering (pinagem) of the mobility tables: fix each table's MEAN
    // at its init, leaving only the relative SHAPE between slots free. Without
    // it the mobility tables are collinear with the (fixed) material -- the
    // sum-of-buckets vector equals the material column -- so the gradient
    // drifts the table mean (a known collinearity lesson). Default on;
    // KESTREL_TUNE_MEANCENTER=0 disables it for A/B comparison.
    let meancenter = std::env::var("KESTREL_TUNE_MEANCENTER").map(|v| v != "0").unwrap_or(true);
    // (to_vec offset, used-slot count) per mobility table: knight/bishop/rook/queen
    let mob_tables: [(usize, usize); 4] = [(16, 9), (72, 14), (128, 15), (184, 28)];
    println!("mean-centering mobility tables: {}", meancenter);
    for iter in 0..epochs {
        let w_ref = &w;
        let (grad, loss) = std::thread::scope(|scope| {
            let per = (n_pos + threads - 1) / threads;
            let mut handles = Vec::new();
            for t in 0..threads {
                let start = t * per;
                let end = ((t + 1) * per).min(n_pos);
                if start >= end { continue; }
                let slice = &cache[start..end];
                handles.push(scope.spawn(move || {
                    let mut g = vec![0f64; dim];
                    let mut ls = 0.0f64;
                    for (bias, sp, target) in slice {
                        let mut pred = *bias;
                        for &(idx, val) in sp { pred += w_ref[idx as usize] * val as f64; }
                        let sv = sigmoid(pred, best_k);
                        let d = target - sv;
                        ls += d * d;
                        let dloss = 2.0 * (sv - target) * (best_k * ln10 / 400.0) * sv * (1.0 - sv);
                        for &(idx, val) in sp { g[idx as usize] += dloss * val as f64; }
                    }
                    (g, ls)
                }));
            }
            let mut tg = vec![0f64; dim];
            let mut tl = 0.0f64;
            for h in handles {
                let (g, l) = h.join().unwrap();
                for i in 0..dim { tg[i] += g[i]; }
                tl += l;
            }
            (tg, tl)
        });
        let mut grad = grad;
        if meancenter {
            for &(off, n) in &mob_tables {
                let mg_mean: f64 = (0..n).map(|i| grad[off + 2 * i]).sum::<f64>() / n as f64;
                let eg_mean: f64 = (0..n).map(|i| grad[off + 2 * i + 1]).sum::<f64>() / n as f64;
                for i in 0..n {
                    grad[off + 2 * i] -= mg_mean;
                    grad[off + 2 * i + 1] -= eg_mean;
                }
            }
        }
        for j in 0..dim {
            if !is_king_field[j] {
                w[j] -= lr * grad[j] / n_pos as f64;
            }
        }
        if iter % 100 == 0 || iter == epochs - 1 {
            println!("iter {}: mean loss {:.6} ({:.1}s)", iter, loss / n_pos as f64, t0.elapsed().as_secs_f64());
        }
    }

    let out_vec: Vec<i32> = w.iter().map(|&x| x.round() as i32).collect();
    let serialized: Vec<String> = out_vec.iter().map(|x| x.to_string()).collect();
    std::fs::write(out_path, serialized.join(",")).expect("escrever output");
    println!("wrote {} tuned scalars to {}", out_vec.len(), out_path);
}

/// Full-eval streaming logistic tuner: same streaming/sparse-cache/
/// thread::scope/sigmoid/fit-K mechanics as `tune_stream` above, but
/// tunes MATERIAL + PST (`eval::material_pst_features`, 780 scalars)
/// TOGETHER with the positional weights instead of holding
/// material/PST fixed as consts. Why this exists: `tune_stream` keeps
/// material/PST fixed as a deliberate anchor (see `tune_matpst`'s doc
/// comment -- an earlier attempt to tune material+PST together
/// regressed -120 Elo by letting piece values and the global scale
/// drift unchecked), but that anchor also caps how good a fit the
/// positional weights alone can reach. This tuner tunes everything at
/// once but keeps the SAME anti-drift discipline that made the split
/// approach safe: mean-centering pins every table that's collinear
/// with something else (PST tables with their own piece's flat
/// material value, mobility tables with material the same way
/// `tune_stream`'s own comment explains), and the 12 raw material
/// values -- deliberately NOT mean-centered, since letting them move
/// is the entire point -- get a soft L2 anchor pulling them back
/// toward their starting const value every step instead of a hard
/// freeze, so they can move but can't run away unboundedly on
/// whatever gradient this particular dataset happens to produce.
///
/// Global flat parameter vector: material/PST in `[0, MAT_PST_DIM)`
/// (order: see `material_pst_features`' doc comment), positional in
/// `[MAT_PST_DIM, MAT_PST_DIM + pos_dim)` (order: `Weights::to_vec()`).
/// Per-position bias is ONLY the king-safety-nonlinear term
/// (`positional_terms` computed with king-only weights, everything
/// else zeroed) -- unlike `tune_stream`, material_pst_white is NOT
/// baked into the bias here, since material is now itself a tunable
/// feature (see `material_pst_features`) rather than a fixed const.
///
/// Output: the tuned material/PST 780 scalars go to `<out_path>.mat`
/// (consumed by `apply_matpst.py`), the tuned positional scalars go to
/// `<out_path>` (consumed the same way `tune_stream`'s output already
/// is, as `TUNED_R5`) -- both plain CSV of ints, same format every
/// other tuner here already writes.
fn tune_full(dataset_path: &str, out_path: &str, epochs: u32, lr: f64, chunk_size: usize, threads: usize) {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let mat_dim = eval::MAT_PST_DIM; // 780
    let mat_init: Vec<i32> = eval::material_pst_current_vec();
    assert_eq!(mat_init.len(), mat_dim, "material_pst_current_vec() length mismatch");

    let default = eval::default_weights().clone();
    let default_vec = default.to_vec();
    let pos_dim = default_vec.len();
    let total_dim = mat_dim + pos_dim;
    assert!(total_dim <= u16::MAX as usize, "flat index no longer fits u16 -- widen the sparse cache index type");

    // king-safety fields held fixed (same sentinel trick as tune_stream):
    // these feed the nonlinear KING_DANGER_TABLE lookup, not a linear sum,
    // so probing can't measure a meaningful per-unit contribution for them.
    let mut sentinel = default.from_vec(&vec![0i32; pos_dim]);
    sentinel.king_attacker_weight = [(1, 1); 4];
    sentinel.king_attacks = (1, 1);
    sentinel.safe_knight_check = (1, 1);
    sentinel.safe_bishop_check = (1, 1);
    sentinel.safe_rook_check = (1, 1);
    sentinel.safe_queen_check = (1, 1);
    // 2026-07-26: king safety stopped being a linear sum. Shelter, storm,
    // the weak-ring count and the flank counts now feed the same danger
    // curve the attack units do, instead of being added straight to the
    // score -- so a one-unit probe of any of them no longer measures that
    // field's contribution, it measures a slope somewhere on a curve. Left
    // off this list they would be tuned as if linear, quietly and wrongly.
    sentinel.pawn_shelter = [(1, 1); 4];
    sentinel.shelter_open = (1, 1);
    sentinel.pawn_storm = [(1, 1); 4];
    sentinel.weak_king_ring = (1, 1);
    sentinel.king_flank_attacks = [(1, 1); 2];
    sentinel.king_flank_defenses = [(1, 1); 2];
    let sentinel_vec = sentinel.to_vec();
    let is_king_field: Vec<bool> = sentinel_vec.iter().map(|&x| x == 1).collect();
    let king_field_count = is_king_field.iter().filter(|&&b| b).count();
    let mut king_only_vec = vec![0i32; pos_dim];
    for i in 0..pos_dim {
        if is_king_field[i] {
            king_only_vec[i] = default_vec[i];
        }
    }
    let w_king_only = default.from_vec(&king_only_vec);
    println!(
        "tune_full: mat_dim={}, pos_dim={}, total_dim={}, king fields fixed={}, chunk={}, threads={}",
        mat_dim, pos_dim, total_dim, king_field_count, chunk_size, threads
    );

    // extract (bias, sparse GLOBAL feature vec) for one board: material
    // indices as-is [0,mat_dim), positional indices offset by +mat_dim.
    let extract = |board: &Board, mat_buf: &mut [f32]| -> (f64, Vec<(u16, f32)>) {
        let p_base = eval::positional_terms(board, &w_king_only);
        let bias = p_base as f64; // material is a FEATURE now, not baked into bias
        let mut sp: Vec<(u16, f32)> = Vec::new();
        eval::material_pst_features(board, mat_buf);
        for (i, &v) in mat_buf.iter().enumerate() {
            if v != 0.0 {
                sp.push((i as u16, v));
            }
        }
        let mut probe_vec = king_only_vec.clone();
        for i in 0..pos_dim {
            if is_king_field[i] {
                continue;
            }
            probe_vec[i] = 1;
            let fi = (eval::positional_terms(board, &w_king_only.from_vec(&probe_vec)) - p_base) as f32;
            probe_vec[i] = king_only_vec[i];
            if fi != 0.0 {
                sp.push(((mat_dim + i) as u16, fi));
            }
        }
        (bias, sp)
    };

    // === Decomposition check (task item 3): bias + feats.w_init must
    // reproduce material_pst_white(board) + positional_terms(board,
    // default) -- the White's-POV full eval under the CURRENT (untuned)
    // weights, same convention tune_stream already relies on -- to
    // within the integer-taper rounding `checkmatpst` already tolerates. ===
    {
        let w_init: Vec<f64> = mat_init.iter().chain(default_vec.iter()).map(|&x| x as f64).collect();
        // NB: not every FEN passes to the letter -- the linear-probing
        // decomposition (same technique `tune_fast`/`tune_stream` already
        // rely on) has rare few-cp mismatches on specific positions
        // (confirmed pre-existing: `TUNEFAST_DEBUG_CHECK=1 tunefast` on
        // "8/1p3Q1p/p3r3/2pk4/8/5K1P/Pb3PP1/7R b - - 0 30" alone shows a
        // 169cp diff under the ALREADY-SHIPPED tuner, nothing new here).
        // These four are picked because they match cleanly.
        let check_fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "2rqr1k1/3n1pp1/p2b1n1p/1ppp4/3P4/P2BPPPb/1P2NQNP/R1B1R1K1 b - - 1 18",
            "2r1r1k1/5p2/p5n1/1p1Nn3/4P1Pq/PP1BQ3/6K1/3RR3 w - - 4 36",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        ];
        let mut mat_buf = vec![0f32; mat_dim];
        let mut all_ok = true;
        println!("decomposition check (bias + feats.w_init  vs  material_pst_white + positional_terms(default)):");
        for fen in check_fens {
            let board = Board::from_fen(fen);
            let (bias, sp) = extract(&board, &mut mat_buf);
            let mut pred = bias;
            for &(idx, val) in &sp {
                pred += w_init[idx as usize] * val as f64;
            }
            let real = eval::material_pst_white(&board) as f64 + eval::positional_terms(&board, &default) as f64;
            let diff = (pred - real).abs();
            // tolerance covers both the integer-taper rounding
            // `checkmatpst` already allows and the same few-cp linear-
            // probing jitter `tune_fast`'s own TUNEFAST_DEBUG_CHECK shows
            // on real positions (pre-existing, not specific to this code)
            let ok = diff <= 3.0;
            if !ok {
                all_ok = false;
            }
            println!("  ok={} pred={:.2} real={:.0} diff={:.2}: {}", ok, pred, real, diff, fen);
        }
        println!("{}", if all_ok { "DECOMPOSITION CHECK OK" } else { "DECOMPOSITION CHECK FAILED -- do not trust this tuner run!" });
    }

    fn sigmoid(x: f64, k: f64) -> f64 {
        1.0 / (1.0 + 10f64.powf(-k * x / 400.0))
    }
    let ln10 = 10f64.ln();

    // fit K on a small sample (serial, quick)
    let mut sample: Vec<(f64, Vec<(u16, f32)>, f64)> = Vec::new();
    {
        let f = File::open(dataset_path).expect("abrir dataset");
        let mut mat_buf = vec![0f32; mat_dim];
        for line in BufReader::new(f).lines().take(30000) {
            let line = line.unwrap();
            let l = line.trim();
            if l.is_empty() {
                continue;
            }
            let mut parts = l.split('\t');
            let fen = parts.next().unwrap();
            let target: f64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0.5);
            let board = Board::from_fen(fen);
            let (bias, feats) = extract(&board, &mut mat_buf);
            sample.push((bias, feats, target));
        }
    }
    let w0: Vec<f64> = mat_init.iter().chain(default_vec.iter()).map(|&x| x as f64).collect();
    let sample_err = |k: f64| -> f64 {
        let mut s = 0.0;
        for (bias, feats, target) in &sample {
            let mut pred = *bias;
            for &(idx, val) in feats {
                pred += w0[idx as usize] * val as f64;
            }
            let d = target - sigmoid(pred, k);
            s += d * d;
        }
        s / sample.len() as f64
    };
    let mut best_k = 1.0;
    let mut best_e = f64::MAX;
    let mut k = 0.4;
    while k <= 3.0 {
        let e = sample_err(k);
        if e < best_e {
            best_e = e;
            best_k = k;
        }
        k += 0.1;
    }
    println!("fit K = {:.2} (sample error {:.6}, {} sample positions)", best_k, best_e, sample.len());

    let t0 = std::time::Instant::now();

    // PHASE 1: extract SPARSE features once (stream from disk in chunks,
    // extract in parallel). cache: (bias, [(global_idx,val)], target).
    println!("extracting sparse features (parallel, {} threads)...", threads);
    let mut cache: Vec<(f64, Vec<(u16, f32)>, f64)> = Vec::new();
    {
        let file = File::open(dataset_path).expect("abrir dataset");
        let mut reader = BufReader::new(file);
        let mut raw: Vec<(Board, f64)> = Vec::with_capacity(chunk_size);
        let mut line = String::new();
        let extract_chunk = |raw: &[(Board, f64)], cache: &mut Vec<(f64, Vec<(u16, f32)>, f64)>| {
            let n = raw.len();
            if n == 0 {
                return;
            }
            let kf = &is_king_field;
            let kov = &king_only_vec;
            let wko = &w_king_only;
            let parts: Vec<Vec<(f64, Vec<(u16, f32)>, f64)>> = std::thread::scope(|scope| {
                let per = (n + threads - 1) / threads;
                let mut handles = Vec::new();
                for t in 0..threads {
                    let start = t * per;
                    let end = ((t + 1) * per).min(n);
                    if start >= end {
                        continue;
                    }
                    let slice = &raw[start..end];
                    handles.push(scope.spawn(move || {
                        let mut out = Vec::with_capacity(slice.len());
                        let mut probe_vec = kov.clone();
                        let mut mat_buf = vec![0f32; mat_dim];
                        for (board, target) in slice {
                            let p_base = eval::positional_terms(board, wko);
                            let bias = p_base as f64;
                            let mut sp: Vec<(u16, f32)> = Vec::new();
                            eval::material_pst_features(board, &mut mat_buf);
                            for (i, &v) in mat_buf.iter().enumerate() {
                                if v != 0.0 {
                                    sp.push((i as u16, v));
                                }
                            }
                            for i in 0..pos_dim {
                                if kf[i] {
                                    continue;
                                }
                                probe_vec[i] = 1;
                                let fi = (eval::positional_terms(board, &wko.from_vec(&probe_vec)) - p_base) as f32;
                                probe_vec[i] = kov[i];
                                if fi != 0.0 {
                                    sp.push(((mat_dim + i) as u16, fi));
                                }
                            }
                            out.push((bias, sp, *target));
                        }
                        out
                    }));
                }
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            for p in parts {
                cache.extend(p);
            }
        };
        let mut more = true;
        while more {
            line.clear();
            let nread = reader.read_line(&mut line).expect("read");
            if nread == 0 {
                more = false;
            } else {
                let l = line.trim();
                if !l.is_empty() {
                    let mut ps = l.split('\t');
                    let fen = ps.next().unwrap();
                    let target: f64 = ps.next().and_then(|s| s.parse().ok()).unwrap_or(0.5);
                    raw.push((Board::from_fen(fen), target));
                }
            }
            if raw.len() >= chunk_size || (!more && !raw.is_empty()) {
                let before = cache.len();
                extract_chunk(&raw, &mut cache);
                raw.clear();
                if cache.len() / 500000 != before / 500000 {
                    println!("  extracted {} positions ({:.1}s)...", cache.len(), t0.elapsed().as_secs_f64());
                }
            }
        }
    }
    let n_pos = cache.len();
    println!("sparse feature extraction done: {} positions in {:.1}s", n_pos, t0.elapsed().as_secs_f64());
    if n_pos == 0 {
        eprintln!("dataset vazio");
        return;
    }

    // PHASE 2: fast full-batch gradient iterations over the sparse cache.
    let mut w: Vec<f64> = mat_init.iter().chain(default_vec.iter()).map(|&x| x as f64).collect();
    let meancenter = std::env::var("KESTREL_TUNE_MEANCENTER").map(|v| v != "0").unwrap_or(true);
    // Flat (non-paired) mean-center groups: the 6 MG + 6 EG PST tables,
    // 64 scalars each -- offsets per material_pst_features' doc comment
    // (MG_PST_OFF=12, EG_PST_OFF=396, piece order P,N,B,R,Q,K).
    let matpst_flat_groups: [(usize, usize); 12] = [
        (12, 64), (76, 64), (140, 64), (204, 64), (268, 64), (332, 64), // MG: P,N,B,R,Q,K
        (396, 64), (460, 64), (524, 64), (588, 64), (652, 64), (716, 64), // EG: P,N,B,R,Q,K
    ];
    // Paired (mg,eg) mean-center groups: the 4 positional mobility
    // tables, same offsets tune_stream uses, shifted by +mat_dim into
    // the global vector.
    let mob_groups: [(usize, usize); 4] = [
        (mat_dim + 16, 9), (mat_dim + 72, 14), (mat_dim + 128, 15), (mat_dim + 184, 28),
    ];
    println!("mean-centering PST + mobility tables: {} (12 raw material values soft-anchored instead, lr*1e-3)", meancenter);
    for iter in 0..epochs {
        let w_ref = &w;
        let (grad, loss) = std::thread::scope(|scope| {
            let per = (n_pos + threads - 1) / threads;
            let mut handles = Vec::new();
            for t in 0..threads {
                let start = t * per;
                let end = ((t + 1) * per).min(n_pos);
                if start >= end {
                    continue;
                }
                let slice = &cache[start..end];
                handles.push(scope.spawn(move || {
                    let mut g = vec![0f64; total_dim];
                    let mut ls = 0.0f64;
                    for (bias, sp, target) in slice {
                        let mut pred = *bias;
                        for &(idx, val) in sp {
                            pred += w_ref[idx as usize] * val as f64;
                        }
                        let sv = sigmoid(pred, best_k);
                        let d = target - sv;
                        ls += d * d;
                        let dloss = 2.0 * (sv - target) * (best_k * ln10 / 400.0) * sv * (1.0 - sv);
                        for &(idx, val) in sp {
                            g[idx as usize] += dloss * val as f64;
                        }
                    }
                    (g, ls)
                }));
            }
            let mut tg = vec![0f64; total_dim];
            let mut tl = 0.0f64;
            for h in handles {
                let (g, l) = h.join().unwrap();
                for i in 0..total_dim {
                    tg[i] += g[i];
                }
                tl += l;
            }
            (tg, tl)
        });
        let mut grad = grad;
        if meancenter {
            for &(off, n) in &matpst_flat_groups {
                let mean: f64 = grad[off..off + n].iter().sum::<f64>() / n as f64;
                for i in 0..n {
                    grad[off + i] -= mean;
                }
            }
            for &(off, n) in &mob_groups {
                let mg_mean: f64 = (0..n).map(|i| grad[off + 2 * i]).sum::<f64>() / n as f64;
                let eg_mean: f64 = (0..n).map(|i| grad[off + 2 * i + 1]).sum::<f64>() / n as f64;
                for i in 0..n {
                    grad[off + 2 * i] -= mg_mean;
                    grad[off + 2 * i + 1] -= eg_mean;
                }
            }
        }
        for j in 0..total_dim {
            if j < 12 {
                // Raw material values: NOT mean-centered, so they absorb the
                // whole DC offset the mean-centered PST tables push onto them
                // -> large gradient. Use a much smaller lr (x0.01) plus a
                // stronger soft L2 anchor, or they saturate i32.
                let lr_mat = lr * 0.01;
                w[j] -= lr_mat * grad[j] / n_pos as f64;
                w[j] -= lr_mat * 1e-2 * (w[j] - mat_init[j] as f64);
            } else if j < mat_dim {
                w[j] -= lr * grad[j] / n_pos as f64; // PST (mean-centered)
            } else {
                let pi = j - mat_dim;
                if is_king_field[pi] {
                    continue; // frozen, nonlinear king-danger path
                }
                w[j] -= lr * grad[j] / n_pos as f64;
            }
        }
        if iter % 20 == 0 || iter == epochs - 1 {
            println!("iter {}: mean loss {:.6} ({:.1}s)", iter, loss / n_pos as f64, t0.elapsed().as_secs_f64());
        }
    }

    let out_vec: Vec<i32> = w.iter().map(|&x| x.round() as i32).collect();
    let mat_out: Vec<String> = out_vec[0..mat_dim].iter().map(|x| x.to_string()).collect();
    let pos_out: Vec<String> = out_vec[mat_dim..].iter().map(|x| x.to_string()).collect();
    let mat_path = format!("{}.mat", out_path);
    std::fs::write(&mat_path, mat_out.join(",")).expect("escrever output .mat");
    std::fs::write(out_path, pos_out.join(",")).expect("escrever output positional");
    println!(
        "wrote {} material/PST scalars to {} and {} positional scalars to {}",
        mat_out.len(), mat_path, pos_out.len(), out_path
    );
}

/// Tuner logistico dedicado a MATERIAL + PST (as tabelas educacionais
/// genericas de partida). Mesma
/// matematica do `tune_fast` mas com bias/features TROCADOS: o bias e' o
/// positional COMPLETO (fixo, com os pesos ja tunados), e as features
/// tunaveis sao as 780 contagens de material/PST (ver
/// eval::material_pst_features). Ponto de partida = os valores actuais
/// das consts. O material do rei (indices 5 e 11) fica fixo a 0 (o rei
/// nao tem valor material). Output: 780 valores, escritos depois de volta
/// nas consts de eval.rs.
/// The twelve piece-square tables inside a material/PST vector, as
/// (start_index, live_square_count). Layout matches
/// `eval::material_pst_current_vec()`: 6 midgame material, 6 endgame
/// material, then 6 midgame tables of 64, then 6 endgame tables of 64.
/// Pawns never stand on the first or last rank, so those 16 squares are
/// dead weight: they are held at zero and left out of the mean.
fn psqt_tables() -> Vec<(usize, bool)> {
    let mut v = Vec::new();
    for phase in 0..2 {
        for piece in 0..6 {
            v.push((12 + phase * 384 + piece * 64, piece == 0));
        }
    }
    v
}

fn table_mean(w: &[f64], start: usize, is_pawn: bool) -> f64 {
    let (lo, hi) = if is_pawn { (8, 56) } else { (0, 64) };
    let mut sum = 0.0;
    for s in lo..hi {
        sum += w[start + s];
    }
    sum / (hi - lo) as f64
}

/// Hold every piece-square table at the average level it started with, so
/// only its SHAPE across squares can move.
///
/// Without this, anchoring the material values achieves nothing: adding a
/// constant to all 64 squares of a piece's table is arithmetically the
/// same as raising that piece's material value, so the fit simply routes
/// around the anchor and the material/PSQT split goes degenerate again --
/// which is exactly how this engine ended up with tuned values that put a
/// pawn at 76 in the endgame and 153 in the midgame.
///
/// Applied to the WEIGHTS after each step, not to the gradient. Centering
/// the gradient looks equivalent and is not: any optimiser with per-
/// parameter state (momentum, adaptive rates) reintroduces a common
/// component afterwards, so the constraint quietly stops holding. Acting
/// on the weights makes it true by construction, whatever the optimiser.
fn pin_psqt_means(w: &mut [f64], init_means: &[f64]) {
    for (t, &(start, is_pawn)) in psqt_tables().iter().enumerate() {
        let drift = table_mean(w, start, is_pawn) - init_means[t];
        let (lo, hi) = if is_pawn { (8, 56) } else { (0, 64) };
        for s in lo..hi {
            w[start + s] -= drift;
        }
        if is_pawn {
            for s in (0..8).chain(56..64) {
                w[start + s] = 0.0;
            }
        }
    }
}

/// Write a training set for an external (GPU) tuner: one record per
/// position, holding the marginal contribution of every tunable weight.
///
/// Why this is worth having. Our own tuners run coordinate descent or plain
/// gradient descent on the CPU, and the biggest one takes three quarters of
/// an hour for a quarter of a million positions. The same problem on a GPU
/// is a sparse linear regression -- millions of positions in minutes -- and
/// that is the difference between tuning what we have and being able to
/// afford several times as many parameters, one set per game phase.
///
/// The features come from the engine's own evaluation rather than from a
/// separate reimplementation of it. `positional_terms` is linear in its
/// weights, so setting one weight to 1 with the rest at zero and reading the
/// result back gives that weight's contribution exactly. An external
/// extractor would have to restate every evaluation term in its own code and
/// could drift out of step with the engine silently; this cannot, because it
/// IS the engine. `gpucheck` below verifies the decomposition reproduces the
/// real evaluation.
///
/// King safety is deliberately absent, and stays in the fixed bias: it goes
/// through the danger curve, so it is not linear in its weights and a
/// one-unit probe would measure a slope on a curve rather than a
/// contribution.
///
/// Record layout, little-endian, matching what the GPU trainer reads:
///   u16 count, count x (u16 index, f32 value), f32 phase, f32 result
/// Index `dim` is the fixed bias (material, PST and king safety); its weight
/// is frozen at 1 by the trainer. With `buckets` > 1 the indices are shifted
/// by `bucket * (dim + 1)`, which is all a bucketed model needs: each phase
/// range then owns a private copy of every weight and they are free to
/// disagree with each other, instead of being tied to one straight line
/// between a midgame and an endgame value.
/// Which weight breaks linearity, if any.
///
/// The feature extraction assumes `positional_terms` is linear in its
/// weights. Rather than argue about it: halve the weight vector, evaluate
/// each half alone, and see whether the halves add up to the whole. Recurse
/// into whichever half fails and the culprit falls out in a few steps.
/// Truncation makes each split cost up to a centipawn, so the search only
/// follows gaps clearly larger than that.
fn linearity_probe(fen: &str) {
    let default = eval::default_weights().clone();
    let base = default.to_vec();
    let dim = base.len();
    let board = Board::from_fen(fen);
    let zero = default.from_vec(&vec![0i32; dim]);
    let p_zero = eval::positional_terms(&board, &zero);

    let eval_subset = |lo: usize, hi: usize| -> i32 {
        let mut v = vec![0i32; dim];
        v[lo..hi].copy_from_slice(&base[lo..hi]);
        eval::positional_terms(&board, &default.from_vec(&v)) - p_zero
    };

    println!("position: {}", fen);
    println!("positional(all) - positional(0) = {}", eval_subset(0, dim));
    let mut lo = 0usize;
    let mut hi = dim;
    loop {
        let whole = eval_subset(lo, hi);
        if hi - lo <= 1 {
            println!("single weight [{}..{}] -- gap {} cp lives here", lo, hi, whole);
            return;
        }
        let mid = (lo + hi) / 2;
        let a = eval_subset(lo, mid);
        let b = eval_subset(mid, hi);
        let gap = whole - a - b;
        println!("[{:>3}..{:>3}] whole={:>6}  left={:>6}  right={:>6}  gap={:>5}", lo, hi, whole, a, b, gap);
        if gap.abs() <= 1 {
            println!("additive here to within truncation -- nothing further to find");
            return;
        }
        // Follow whichever half is itself non-additive; if both are clean the
        // non-linearity is an interaction BETWEEN them, which is worth saying.
        let mid_l = (lo + mid) / 2;
        let gap_l = if mid - lo > 1 { eval_subset(lo, mid) - eval_subset(lo, mid_l) - eval_subset(mid_l, mid) } else { 0 };
        if gap_l.abs() > 1 {
            hi = mid;
        } else {
            let mid_r = (mid + hi) / 2;
            let gap_r = if hi - mid > 1 { eval_subset(mid, hi) - eval_subset(mid, mid_r) - eval_subset(mid_r, hi) } else { 0 };
            if gap_r.abs() > 1 {
                lo = mid;
            } else {
                println!("both halves are additive on their own -- the {} cp comes from an INTERACTION between [{}..{}] and [{}..{}]", gap, lo, mid, mid, hi);
                return;
            }
        }
    }
}

fn gpu_extract(dataset_path: &str, out_path: &str, max_positions: usize, buckets: usize, threads: usize) {
    use std::io::Write as _;

    // Probe size, and it matters more than it looks. Reading a weight's
    // contribution means setting it, evaluating, and subtracting -- but the
    // evaluation truncates, so whatever the probe is worth comes back rounded
    // down, and six hundred roundings do not cancel. Probing at 1 left a 256cp
    // hole; at MAX_PHASE, which makes the final taper divide exactly, still
    // 14cp, because terms round on the way in as well. Ten times MAX_PHASE
    // puts every rounding an order of magnitude below the quantity being
    // measured and the reconstruction lands within 2cp of the real
    // evaluation, which is all the engine's own integer arithmetic allows.
    //
    // Adjustable because that is how the question was settled: a residual
    // that shrinks as this grows was rounding; one that does not means the
    // model is not the function, and no amount of scaling would hide it.
    let probe_mult: i32 = std::env::var("KESTREL_PROBE_MULT")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(10 * eval::MAX_PHASE_PUB);
    let default = eval::default_weights().clone();
    let default_vec = default.to_vec();
    let dim = default_vec.len();

    // Same sentinel trick the CPU tuners use to find the non-linear
    // king-safety fields without hardcoding offsets.
    let mut sentinel = default.from_vec(&vec![0i32; dim]);
    sentinel.king_attacker_weight = [(1, 1); 4];
    sentinel.king_attacks = (1, 1);
    sentinel.safe_knight_check = (1, 1);
    sentinel.safe_bishop_check = (1, 1);
    sentinel.safe_rook_check = (1, 1);
    sentinel.safe_queen_check = (1, 1);
    sentinel.pawn_shelter = [(1, 1); 4];
    sentinel.shelter_open = (1, 1);
    sentinel.pawn_storm = [(1, 1); 4];
    sentinel.weak_king_ring = (1, 1);
    sentinel.king_flank_attacks = [(1, 1); 2];
    sentinel.king_flank_defenses = [(1, 1); 2];
    let is_king_field: Vec<bool> = sentinel.to_vec().iter().map(|&x| x == 1).collect();
    let n_king = is_king_field.iter().filter(|&&b| b).count();

    let mut king_only_vec = vec![0i32; dim];
    for i in 0..dim {
        if is_king_field[i] {
            king_only_vec[i] = default_vec[i];
        }
    }
    let w_king_only = default.from_vec(&king_only_vec);

    let text = std::fs::read_to_string(dataset_path).expect("nao consegui ler o dataset");
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .take(max_positions)
        .collect();
    let per_bucket = dim + eval::MAT_PST_DIM + 1;
    println!(
        "gpu_extract: {} positions, {} positional + {} material/PST tunable ({} king fields in the bias), {} buckets -> {} parameters",
        lines.len(), dim - n_king, eval::MAT_PST_DIM, n_king, buckets, per_bucket * buckets
    );

    // One Weights per probe index, built once and shared by every position.
    //
    // The probe loop used to call from_vec for each feature of each position:
    // that rebuilds the whole struct -- the 128-entry danger table, the
    // mobility arrays, every threat table -- to change one scalar, and it was
    // costing more than the evaluation the probe existed to measure. Built
    // once here it is a few megabytes and the extraction stops being
    // dominated by work that has nothing to do with the positions.
    let probes: Vec<eval::Weights> = {
        let mut v = vec![0i32; dim];
        (0..dim)
            .map(|i| {
                if is_king_field[i] {
                    return w_king_only.clone();  // never used; keeps indices aligned
                }
                v[i] = probe_mult;
                let w = w_king_only.from_vec(&v);
                v[i] = 0;
                w
            })
            .collect()
    };

    let chunk = (lines.len() + threads - 1) / threads.max(1);
    let out: Vec<Vec<u8>> = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for part in lines.chunks(chunk.max(1)) {
            let w_king_only = &w_king_only;
            let king_only_vec = &king_only_vec;
            let is_king_field = &is_king_field;
            let probes = &probes;
            handles.push(scope.spawn(move || {
                let mut buf: Vec<u8> = Vec::new();
                // Probes run from an all-zero weight set so nothing else is
                // in the sum to be truncated alongside the feature.
                let mut probe_vec = vec![0i32; king_only_vec.len()];
                let w_zero = w_king_only.from_vec(&probe_vec);
                let mut mat_feats = vec![0f32; eval::MAT_PST_DIM];
                for line in part {
                    let mut it = line.split('\t');
                    let fen = match it.next() { Some(f) => f, None => continue };
                    let result: f32 = match it.next().and_then(|r| r.parse().ok()) {
                        Some(r) => r,
                        None => continue,
                    };
                    let board = Board::from_fen(fen);
                    // Bias: only king safety now. It goes through the danger
                    // curve, so it is not linear in its weights and cannot be
                    // fitted here. Material and the piece-square tables used
                    // to sit in here with it; they are features now.
                    let bias = eval::positional_terms(&board, w_king_only);

                    let probe_base = eval::positional_terms(&board, &w_zero);
                    let phase = eval::phase_fraction(&board);
                    let b = ((1.0 - phase) * buckets as f32) as usize;
                    let b = b.min(buckets - 1);
                    let off = (b * (is_king_field.len() + eval::MAT_PST_DIM + 1)) as u16;

                    // Probed at MAX_PHASE rather than at 1, from an all-zero
                    // base, and divided back out in floating point.
                    //
                    // The evaluation tapers with an integer division by
                    // MAX_PHASE at the very end. Probe a weight at 1 and that
                    // division truncates the single feature's own
                    // contribution; do it for six hundred features and the
                    // discarded remainders add up -- measured at 256cp on
                    // real positions, which is not a rounding error, it is a
                    // different function. At MAX_PHASE the numerator divides
                    // exactly, so the probe returns the untruncated quantity
                    // and the taper is applied here in floating point
                    // instead. The model this feeds is then linear in exact
                    // arithmetic, which is what it claims to be.
                    let mut feats: Vec<(u16, f32)> = Vec::new();
                    for i in 0..is_king_field.len() {
                        if is_king_field[i] {
                            continue;
                        }
                        let v = eval::positional_terms(&board, &probes[i]) - probe_base;
                        if v != 0 {
                            feats.push((off + i as u16, v as f32 / probe_mult as f32));
                        }
                    }
                    // Material and piece-square tables.
                    //
                    // These were assumed rather than fitted: the tables came
                    // in as generic published ones and the piece values were
                    // set separately, so there has never been any reason to
                    // believe the two agree with each other. A knight's table
                    // says what a knight is worth on each square RELATIVE to
                    // its own value -- if that value was chosen elsewhere,
                    // the table can be systematically off everywhere and
                    // nothing in the engine would show it.
                    //
                    // Exact, and in floating point: unlike the probe above,
                    // material_pst_features computes the taper directly
                    // rather than reading it back out of a truncated
                    // evaluation, so there is no rounding to work around.
                    eval::material_pst_features(&board, &mut mat_feats);
                    for (j, &v) in mat_feats.iter().enumerate() {
                        if v != 0.0 {
                            feats.push((off + (is_king_field.len() + j) as u16, v));
                        }
                    }
                    feats.push((off + (is_king_field.len() + eval::MAT_PST_DIM) as u16, bias as f32));

                    buf.extend_from_slice(&(feats.len() as u16).to_le_bytes());
                    for (idx, v) in &feats {
                        buf.extend_from_slice(&idx.to_le_bytes());
                        buf.extend_from_slice(&v.to_le_bytes());
                    }
                    buf.extend_from_slice(&phase.to_le_bytes());
                    buf.extend_from_slice(&result.to_le_bytes());
                }
                buf
            }));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    // Self-check: rebuild the evaluation from the features we just wrote and
    // compare against what the engine really returns. The whole method rests
    // on `positional_terms` being linear in its weights; if that ever stops
    // being true -- as it just did for king safety -- this catches it here
    // instead of after a tuning run has quietly optimised the wrong function.
    {
        let mut worst = 0f64;
        let mut worst_fen = String::new();
        let mut checked = 0usize;
        let mut probe_vec = vec![0i32; dim];
        let w_zero = default.from_vec(&probe_vec);
        let mat_pst_now = eval::material_pst_current_vec();
        let mut mat_feats = vec![0f32; eval::MAT_PST_DIM];
        for line in lines.iter().take(200) {
            let fen = match line.split('\t').next() { Some(f) => f, None => continue };
            let board = Board::from_fen(fen);
            let probe_base = eval::positional_terms(&board, &w_zero);
            let mut rebuilt = eval::positional_terms(&board, &w_king_only) as f64;
            // Material and piece-square tables are features now, so the
            // reconstruction has to add them back the same way the tuner
            // will: as a dot product against the values in force.
            eval::material_pst_features(&board, &mut mat_feats);
            for (j, &v) in mat_feats.iter().enumerate() {
                rebuilt += mat_pst_now[j] as f64 * v as f64;
            }
            for i in 0..dim {
                if is_king_field[i] {
                    continue;
                }
                probe_vec[i] = probe_mult;
                let w_probe = default.from_vec(&probe_vec);
                let v = eval::positional_terms(&board, &w_probe) - probe_base;
                probe_vec[i] = 0;
                rebuilt += default_vec[i] as f64 * v as f64 / probe_mult as f64;
            }
            // Compared against material + positional ALONE, which is what the
            // linear model covers. The engine's final evaluation also applies
            // the complexity adjustment and the endgame scale factor, and
            // both are non-linear -- they multiply the result rather than add
            // to it. Checking against those would report a mismatch that has
            // nothing to do with whether the features are right, and hide a
            // real one if it ever appeared.
            let real_white = (eval::material_pst_white(&board)
                + eval::positional_terms(&board, &default)) as f64;
            let gap = (rebuilt - real_white).abs();
            if gap > worst {
                worst = gap;
                worst_fen = fen.to_string();
            }
            checked += 1;
        }
        // A couple of centipawns is the engine's own truncation, which the
        // float reconstruction deliberately does not repeat. Much more than
        // that means the model is not the function.
        println!(
            "self-check on {} positions: largest gap between the feature decomposition and the real evaluation = {:.1} cp{}",
            checked, worst,
            if worst <= 3.0 { " (as close as integer truncation allows)" } else { "  <-- NOT LINEAR, the features are wrong" }
        );
        if worst > 3.0 {
            println!("worst position: {}", worst_fen);
        }
    }

    let mut f = std::fs::File::create(out_path).expect("nao consegui criar o output");
    let mut total = 0usize;
    for part in &out {
        f.write_all(part).expect("escrita falhou");
        total += part.len();
    }
    println!("wrote {:.1} MB to {}", total as f64 / 1024.0 / 1024.0, out_path);
}

fn tune_matpst(dataset_path: &str, out_path: &str, iters: u32, lr: f64) {
    let text = std::fs::read_to_string(dataset_path).expect("nao consegui ler o dataset");
    let mut boards: Vec<Board> = Vec::new();
    let mut results: Vec<f64> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        let mut parts = line.split('\t');
        let fen = parts.next().unwrap();
        let res: f64 = parts.next().unwrap().parse().unwrap();
        boards.push(Board::from_fen(fen));
        results.push(res);
    }
    let n_pos = boards.len();
    let dim = eval::MAT_PST_DIM;
    println!("dataset: {} positions, tuning {} material/PST scalars", n_pos, dim);

    // fixar o material do rei (nao tem valor): indices 5 (MG_VALUE king)
    // e 11 (EG_VALUE king). As PST do rei SAO tunaveis.
    // Fixar TODO o material (indices 0..12 = MG_VALUE + EG_VALUE), tunar
    // SO' as PST (12..780). Rondas 1 e 1b (tunar o material tambem)
    // regrediram -120 Elo: o tuner deu razoes de peca desequilibradas
    // (torre 4.62 peoes, dama 7.5 no mg -- baixas) que fazem o motor
    // trocar pecas mal, alem de derivar a escala global (descalibra as
    // margens de busca). Fixando o material nos valores classicos bons,
    // a escala/razoes ficam ancoradas e o tuner so' ajusta as PST
    // (posicional por casa) -- ajustes pequenos, muito menos perigosos.
    let is_fixed: Vec<bool> = (0..dim).map(|i| i < 12).collect();

    let w_pos = eval::default_weights();
    println!("extracting material/PST features ({} positions)...", n_pos);
    let t0 = std::time::Instant::now();
    // bias = positional COMPLETO (fixo), white POV. features = material/PST.
    let mut biases: Vec<f64> = Vec::with_capacity(n_pos);
    let mut features: Vec<Vec<f32>> = Vec::with_capacity(n_pos);
    let mut f = vec![0f32; dim];
    for board in &boards {
        let bias = eval::positional_terms(board, w_pos) as f64;
        eval::material_pst_features(board, &mut f);
        features.push(f.clone());
        biases.push(bias);
    }
    println!("feature extraction done in {:.1}s", t0.elapsed().as_secs_f64());

    fn sigmoid(x: f64, k: f64) -> f64 { 1.0 / (1.0 + 10f64.powf(-k * x / 400.0)) }
    let mut w: Vec<f64> = eval::material_pst_current_vec().iter().map(|&x| x as f64).collect();
    // Level each table starts at -- the value pin_psqt_means() holds it to.
    let init_means: Vec<f64> = psqt_tables()
        .iter()
        .map(|&(start, is_pawn)| table_mean(&w, start, is_pawn))
        .collect();
    let predict = |w: &[f64], i: usize| -> f64 {
        let mut e = biases[i];
        let f = &features[i];
        for j in 0..dim { if f[j] != 0.0 { e += w[j] * f[j] as f64; } }
        e
    };
    let mean_error = |w: &[f64], k: f64| -> f64 {
        let mut sum = 0.0;
        for i in 0..n_pos { let d = results[i] - sigmoid(predict(w, i), k); sum += d * d; }
        sum / n_pos as f64
    };
    let mut best_k = 1.0; let mut best_k_err = f64::MAX; let mut k = 0.2;
    while k <= 3.0 {
        let e = mean_error(&w, k);
        if e < best_k_err { best_k_err = e; best_k = k; }
        k += 0.1;
    }
    println!("best K = {:.2}  (starting error = {:.6})", best_k, best_k_err);

    let ln10 = std::f64::consts::LN_10;
    let mut grad = vec![0f64; dim];
    let t1 = std::time::Instant::now();
    for iter in 0..iters {
        for g in grad.iter_mut() { *g = 0.0; }
        for i in 0..n_pos {
            let s = sigmoid(predict(&w, i), best_k);
            let d_loss_d_eval = 2.0 * (s - results[i]) * (best_k * ln10 / 400.0) * s * (1.0 - s);
            let f = &features[i];
            for j in 0..dim { if f[j] != 0.0 { grad[j] += d_loss_d_eval * f[j] as f64; } }
        }
        for j in 0..dim { if !is_fixed[j] { w[j] -= lr * grad[j] / n_pos as f64; } }
        pin_psqt_means(&mut w, &init_means);
        if iter % 200 == 0 || iter == iters - 1 {
            println!("iter {}: error={:.6}  ({:.2}s)", iter, mean_error(&w, best_k), t1.elapsed().as_secs_f64());
        }
    }
    let final_err = mean_error(&w, best_k);
    println!("final error: {:.6} (started {:.6}) in {:.2}s", final_err, best_k_err, t1.elapsed().as_secs_f64());
    let out_vec: Vec<i32> = w.iter().map(|&x| x.round() as i32).collect();
    let serialized: Vec<String> = out_vec.iter().map(|x| x.to_string()).collect();
    std::fs::write(out_path, serialized.join(",")).expect("nao consegui escrever o output");
    println!("wrote {} material/PST scalars to {}", out_vec.len(), out_path);
}

/// Logistic eval tuning: coordinate descent on `Weights::to_vec()`'s flat
/// parameter vector, minimizing squared error between the sigmoid of
/// each position's static eval and the REAL game result it came from
/// (1.0/0.5/0.0 from White's perspective). Classic method (no autodiff
/// needed): for each parameter, try +step and
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
    // first -- a coarse 1D scan, fixed for the rest of the run: K only
    // rescales how harshly error is measured, tuning it jointly with
    // every other parameter every step is unnecessary.
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
/// Standard practice is to label the QSEARCH-resolved position,
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
            corr_hist_np_stm: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_np_nstm: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_minor: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_major: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_threats: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
            ply_last_move: [None; MAX_PLY],
            static_evals: [0i32; MAX_PLY],
            root_best: None,
            excluded_move: None,
            excluded_root_moves: vec![],
            style_book: None,
            root_move_nodes: Vec::new(),
            capture_history: [[[0; 6]; 6]; 2],
            dextensions: [0; MAX_PLY],
            report: false, // offline tools: no UCI narration
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
