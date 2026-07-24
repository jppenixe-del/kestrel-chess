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
        // Texel-tuning datagen method the user described (games at a
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
        // streaming Texel tuner (RAM-constant, for large SF-binpack datasets):
        //   tunestream <dataset.epd> <out.txt> [epochs] [lr] [chunk] [threads]
        let epochs: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(6);
        let lr: f64 = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(2.0);
        let chunk: usize = args.get(6).and_then(|s| s.parse().ok()).unwrap_or(50000);
        let threads: usize = args.get(7).and_then(|s| s.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
        tune_stream(&args[2], &args[3], epochs, lr, chunk, threads);
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

/// Real time-control self-play datagen, per the standard Texel-tuning
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
    sentinel.safe_knight_check = (1, 1);
    sentinel.safe_bishop_check = (1, 1);
    sentinel.safe_rook_check = (1, 1);
    sentinel.safe_queen_check = (1, 1);
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

/// Streaming Texel tuner: same linear model as `tune_fast`, but never
/// holds the whole dataset (or its dense feature vectors) in RAM. Reads
/// the EPD in chunks, extracts features + accumulates the gradient for
/// each chunk in PARALLEL (thread::scope, no rayon dependency), applies a
/// mini-batch update per chunk, and discards it. RAM is O(chunk_size), so
/// arbitrarily large Stockfish-binpack-derived datasets can be used (the
/// method the user pointed at: stream, don't load). Material stays fixed
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
    // drifts the table mean (the texel_tuner/Sirius lesson). Default on;
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

/// Tuner Texel dedicado a MATERIAL + PST (as PeSTO genericas). Mesma
/// matematica do `tune_fast` mas com bias/features TROCADOS: o bias e' o
/// positional COMPLETO (fixo, com os pesos ja tunados), e as features
/// tunaveis sao as 780 contagens de material/PST (ver
/// eval::material_pst_features). Ponto de partida = os valores actuais
/// das consts. O material do rei (indices 5 e 11) fica fixo a 0 (o rei
/// nao tem valor material). Output: 780 valores, escritos depois de volta
/// nas consts de eval.rs.
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
    // regrediram -120 Elo: o Texel deu razoes de peca desequilibradas
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
