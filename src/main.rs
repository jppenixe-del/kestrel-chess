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
    if args.len() >= 4 && args[1] == "tune" {
        let epochs: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(20);
        tune_weights(&args[2], &args[3], epochs);
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

/// Real Texel Tuning: coordinate descent on `Weights::to_vec()`'s flat
/// parameter vector, minimizing squared error between the sigmoid of
/// each position's static eval and the REAL game result it came from
/// (1.0/0.5/0.0 from White's perspective). Classic method (the
/// original Texel tuner and most small engines' tuners work exactly
/// this way -- no autodiff needed): for each parameter, try +step and
/// -step, keep whichever reduces total error over the whole dataset,
/// else leave it unchanged. Dataset format: one line per position,
/// "<FEN>\t<white_score>".
fn tune_weights(dataset_path: &str, out_path: &str, epochs: u32) {
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
    println!("starting error: {:.6}", current_err);

    for epoch in 0..epochs {
        let mut improved = 0;
        for i in 0..v.len() {
            let orig = v[i];
            v[i] = orig + 1;
            let cand = current.from_vec(&v);
            let err_up = total_error(&boards, &results, &cand, best_k);
            if err_up < current_err {
                current_err = err_up;
                current = cand;
                improved += 1;
                continue;
            }
            v[i] = orig - 1;
            let cand = current.from_vec(&v);
            let err_down = total_error(&boards, &results, &cand, best_k);
            if err_down < current_err {
                current_err = err_down;
                current = cand;
                improved += 1;
                continue;
            }
            v[i] = orig;
        }
        println!("epoch {}: error={:.6}  params improved={}", epoch, current_err, improved);
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
            let legal = movegen::generate_legal(&board, &atk);
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
