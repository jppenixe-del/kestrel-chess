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
    let mut soft = base.clamp(10, safe_time / 2);
    let mut hard_cap = (safe_time * 3 / 10).max(soft); // nunca mais de ~30% do relogio numa jogada

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
        let legal = crate::movegen::generate_legal(&self.board, &self.atk);
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
                "infinite" => { infinite = true; i += 1; }
                _ => { i += 1; }
            }
        }

        let side_white = self.board.side == crate::types::Color::White;
        let (my_time, my_inc) = if side_white { (wtime, winc) } else { (btime, binc) };

        let deadline: Option<Instant> = if let Some(mt) = movetime {
            Some(Instant::now() + Duration::from_millis(mt.max(1) as u64))
        } else if infinite || (my_time == 0 && movetime.is_none() && depth.is_none()) {
            None
        } else {
            let (soft, _hard) = compute_time_budget(my_time, my_inc, self.board.fullmove, movestogo, self.last_score);
            Some(Instant::now() + Duration::from_millis(soft.max(1) as u64))
        };

        let max_depth = depth.unwrap_or(64);
        let mut searcher = Searcher {
            atk: &self.atk,
            zob: &self.zob,
            tt: &mut self.tt,
            nodes: 0,
            limits: SearchLimits { deadline, max_depth, max_nodes: nodes },
            stop: false,
            history: self.history.clone(),
            killers: [[None; 2]; crate::search::MAX_PLY],
            root_best: None,
            style_book: self.style_book.as_ref(),
        };

        let t0 = Instant::now();
        let (best, score, depth_reached, nodes_searched) = searcher.iterative_deepening(&mut self.board);
        self.last_score = Some(score);
        let dt = t0.elapsed();
        let nps = if dt.as_secs_f64() > 0.0 { (nodes_searched as f64 / dt.as_secs_f64()) as u64 } else { 0 };

        let score_str = if score.abs() >= MATE_SCORE - 1000 {
            let mate_in = ((MATE_SCORE - score.abs() + 1) / 2).max(1);
            format!("mate {}", if score > 0 { mate_in } else { -mate_in })
        } else {
            format!("cp {}", score)
        };
        let _ = writeln!(
            out,
            "info depth {} score {} nodes {} nps {} time {}",
            depth_reached, score_str, nodes_searched, nps, dt.as_millis()
        );
        // Rede de seguranca absoluta: mesmo que a busca nao tenha
        // conseguido terminar profundidade nenhuma (relogio esgotado
        // mesmo em cima do 1o lance, caso extremo), NUNCA devolver lance
        // nulo se houver lances legais -- joga o primeiro legal em vez de
        // "0000" (que a arena/arbitro trata como derrota imediata).
        let final_move = best.or_else(|| {
            crate::movegen::generate_legal(&self.board, &self.atk).into_iter().next()
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
