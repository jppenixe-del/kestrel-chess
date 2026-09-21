// The UCI front end.
//
// The search runs on this thread rather than a worker, so `stop` is only acted
// on between moves. That is deliberate for now: the engine limits its own time
// and never relies on being told to stop, which is the behaviour that matters
// at a time control. Pondering and `go infinite` need the worker and will get
// it when there is something to ponder with.

use crate::board::Board;
use crate::movegen::generate_legal;
use crate::nnue;
use crate::search::{Limits, Searcher};
use crate::types::Color;
use std::io::BufRead;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub const NAME: &str = "half2k";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where to look for the weights, in order: the UCI option, the environment,
/// then next to the binary. The last one is what lets a worker that only knows
/// how to build and run the binary find them without being told.
fn find_network(explicit: &str) -> Option<String> {
    let mut tries: Vec<String> = Vec::new();
    if !explicit.is_empty() && explicit != "<empty>" {
        tries.push(explicit.to_string());
    }
    if let Ok(p) = std::env::var("HALF2K_NET") {
        tries.push(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            tries.push(dir.join("network.dat").to_string_lossy().into_owned());
        }
    }
    tries.into_iter().find(|p| std::path::Path::new(p).exists())
}

pub fn main_loop() {
    let stop = Arc::new(AtomicBool::new(false));
    let mut hash_mb = 16usize;
    let mut searcher = Searcher::new(hash_mb, stop.clone());
    let mut board = Board::startpos();
    let mut net_path = String::new();
    let mut net_loaded = false;
    let mut avisou_rede = false;

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        // End of input ends the program. Reading a failed line as an empty
        // command and going round again would spin a core forever, which is a
        // real way to make a build system hang with no error at all.
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let mut parts = line.split_whitespace();
        let cmd = match parts.next() {
            Some(c) => c,
            None => continue,
        };

        match cmd {
            "uci" => {
                println!("id name {} {}", NAME, VERSION);
                println!("id author the half2k authors");
                println!("option name Hash type spin default 16 min 1 max 65536");
                println!("option name Threads type spin default 1 min 1 max 64");
                println!(
                    "option name Move Overhead type spin default {} min 0 max 5000",
                    searcher.move_overhead
                );
                println!("option name EvalFile type string default <empty>");
                println!("option name SyzygyPath type string default <empty>");
                // Off by default, every one of them: out of the box the search
                // uses the smaller, settled set of ideas, so anything switched
                // on has a number of its own rather than being lost in a pile
                // of simultaneous changes.
                for name in crate::search::Features::EXTRA {
                    println!("option name {} type check default false", name);
                }
                // These are part of the baseline and start on. The switch is
                // for measuring them, not for leaving them out.
                for name in crate::search::Features::BASELINE {
                    println!("option name {} type check default true", name);
                }
                // Every number the search compares against. Not one was
                // measured, so every one is a candidate.
                let d = crate::search::Params::default();
                for (name, get, _, lo, hi) in crate::search::PARAM_SPECS {
                    println!(
                        "option name {} type spin default {} min {} max {}",
                        name,
                        get(&d),
                        lo,
                        hi
                    );
                }
                println!("uciok");
            }
            "isready" => {
                if !net_loaded {
                    match find_network(&net_path) {
                        // A ponte primeiro: se o ficheiro for do formato delas,
                        // e' ela que avalia e a rede propria nao se carrega. Se
                        // nao for, cai no nosso leitor como sempre caiu.
                        Some(p) if crate::ponte::liga_com(&p) => {
                            net_loaded = true;
                        }
                        Some(p) => match nnue::load(&p) {
                            Ok(n) => {
                                nnue::install(n);
                                net_loaded = true;
                                // Rebuild: the position was made before there
                                // was a network to build an accumulator with.
                                board = Board::from_fen(&board.to_fen());
                            }
                            Err(e) => eprintln!("info string network {}: {}", p, e),
                        },
                        None => {
                            // Uma vez, e nunca quando a ponte esta' a dar a
                            // avaliacao -- ai' a queixa nem sequer e' verdade.
                            // Com `isready` antes de cada lance isto enchia
                            // 8.487 linhas em dezanove partidas, e o aviso que
                            // importa naquele fluxo -- `sf: NAO carregou` --
                            // ficava enterrado. Um aviso que ninguem ve nao e'
                            // um aviso.
                            if !avisou_rede && !crate::ponte::ligado() {
                                avisou_rede = true;
                                eprintln!("info string no network found");
                            }
                        }
                    }
                }
                println!("readyok");
            }
            "setoption" => {
                // `setoption name <words> value <words>` -- the name can carry
                // spaces, so it is everything between the two keywords.
                let rest: Vec<&str> = line.split_whitespace().collect();
                let name_at = rest.iter().position(|w| *w == "name");
                let value_at = rest.iter().position(|w| *w == "value");
                if let (Some(n), Some(v)) = (name_at, value_at) {
                    let name = rest[n + 1..v].join(" ").to_lowercase();
                    let value = rest[v + 1..].join(" ");
                    match name.as_str() {
                        // ANUNCIADA desde sempre, LIDA desde 20-09-2026.
                        //
                        // A opcao aparecia no `uci` e nao era tratada em lado
                        // nenhum: o motor respondia `min 1 max 1` e ninguem
                        // escrevia o valor. Quando a busca paralela entrou, o
                        // `searcher.threads` ficou preso em 1 e os ajudantes
                        // nunca nasceram -- as contagens a 1 e a 4 fios davam o
                        // MESMO numero ao byte, que foi como se deu por isto.
                        //
                        // Uma opcao anunciada e nao lida e' pior do que uma
                        // opcao que nao existe: quem a poe fica convencido de
                        // que mudou alguma coisa.
                        "threads" => {
                            if let Ok(n) = value.parse::<usize>() {
                                searcher.threads = n.clamp(1, 64);
                            }
                        }
                        "hash" => {
                            if let Ok(mb) = value.parse::<usize>() {
                                hash_mb = mb.clamp(1, 65536);
                                let mo = searcher.move_overhead;
                                let ft = searcher.features;
                                let pr = searcher.params;
                                let th = searcher.threads;
                                searcher = Searcher::new(hash_mb, stop.clone());
                                searcher.threads = th;
                                searcher.move_overhead = mo;
                                searcher.features = ft;
                                searcher.params = pr;
                                searcher.params_changed();
                            }
                        }
                        "move overhead" => {
                            if let Ok(ms) = value.parse::<u64>() {
                                searcher.move_overhead = ms.min(5000);
                            }
                        }
                        "evalfile" => {
                            net_path = value;
                            net_loaded = false;
                        }
                        "syzygypath" => {
                            let n = crate::tb::carregar(&value);
                            if n > 0 {
                                println!("info string tablebases up to {} pieces", n);
                            } else {
                                println!("info string no tablebases at {}", value);
                            }
                        }
                        other => {
                            if let Ok(n) = value.parse::<i32>() {
                                if searcher.params.set(other, n) {
                                    searcher.params_changed();
                                    continue;
                                }
                            }
                            let on = value.eq_ignore_ascii_case("true");
                            searcher.features.set(other, on);
                        }
                    }
                }
            }
            "ucinewgame" => {
                searcher.clear();
                board = Board::startpos();
                searcher.set_game_history(vec![board.hash]);
            }
            "position" => {
                let rest: Vec<&str> = line.split_whitespace().collect();
                let mut i = 1;
                if rest.get(i) == Some(&"startpos") {
                    board = Board::startpos();
                    i += 1;
                } else if rest.get(i) == Some(&"fen") {
                    let end = rest
                        .iter()
                        .position(|w| *w == "moves")
                        .unwrap_or(rest.len());
                    board = Board::from_fen(&rest[i + 1..end].join(" "));
                    i = end;
                }
                // Every position along the way is kept, because that is what a
                // repetition is measured against. Losing the history here makes
                // the search blind to a draw it is one move away from.
                let mut keys = vec![board.hash];
                let mut played: Vec<crate::moves::Move> = Vec::new();
                if rest.get(i) == Some(&"moves") {
                    let atk = &searcher.atk;
                    for token in &rest[i + 1..] {
                        let legal = generate_legal(&mut board, atk);
                        if let Some(mv) = legal.iter().find(|m| m.to_uci() == *token) {
                            board.make_move(mv);
                            keys.push(board.hash);
                            played.push(*mv);
                        } else {
                            // Erro CRITICO, e em `stdout`: em `stderr` nenhum
                            // arbitro ve' a mensagem, e truncar a posicao faz
                            // o motor responder a partir de uma que nao e' a
                            // do jogo. Ver a nota igual no repo publico.
                            println!(
                                "info string ERRO CRITICO: lance '{}' nao existe nesta posicao \
-- a posicao ficaria errada, nao jogo",
                                token
                            );
                            use std::io::Write as _;
                            let _ = std::io::stdout().flush();
                            std::process::exit(1);
                        }
                    }
                }
                searcher.set_game_history(keys);
                searcher.set_game_moves(&played);
            }
            "go" => {
                // Sem rede a avaliacao e' zero em toda a parte e a busca
                // continua na mesma, a anunciar `cp 0` e a escolher lances que
                // nao querem dizer nada. Nada na saida diz que o motor esta'
                // partido, por isso a partida joga-se ate' ao fim e o resultado
                // parece falta de forca. Uma rede em falta e' erro fatal, nao
                // aviso: diz-se nos dois canais e sai-se, que um arbitro de
                // combates reporta logo como desligamento.
                // O `nnue_sf` conta como avaliador, e a ponte so' conta
                // quando esta' mesmo a ser usada.
                //
                // A condicao antiga conhecia dois dos tres avaliadores. Com
                // `H2K_SEM_PONTE=1` e um `EvalFile` do formato deles, a ponte
                // liga (logo `ligado()` e' verdade e o erro nao dispara), a
                // busca ignora-a por causa do `sem_ponte()`, e o `nnue_sf`
                // nunca recebeu caminho nenhum -- `uci.rs` nao chama
                // `nnue_sf::define_evalfile`. Resultado: `cp 0` em toda a
                // parte, sem uma linha de erro. E' exactamente o desfecho que
                // esta guarda existe para impedir, pela porta do lado.
                let ponte_a_valer =
                    crate::ponte::ligado() && !crate::search::sem_ponte();
                if !ponte_a_valer
                    && nnue::net().is_none()
                    && crate::nnue_sf::rede().is_none()
                {
                    let m = "ERRO: nenhuma rede carregada -- \
                             use `setoption name EvalFile value <ficheiro>`. \
                             Sem rede a avaliacao seria zero em toda a parte.";
                    println!("info string {m}");
                    eprintln!("{m}");
                    std::process::exit(1);
                }
                // Uma regua ligada diz-se alto, e uma vez.
                if let Some(aviso) = crate::nnue_sf::regua_activa() {
                    static DITO: std::sync::atomic::AtomicBool =
                        std::sync::atomic::AtomicBool::new(false);
                    if !DITO.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        println!("info string {aviso}");
                        eprintln!("{aviso}");
                    }
                }
                let mut limits = Limits::default();
                let rest: Vec<&str> = line.split_whitespace().collect();
                let mut i = 1;
                while i < rest.len() {
                    let val = rest.get(i + 1).and_then(|v| v.parse::<u64>().ok());
                    match rest[i] {
                        "wtime" => limits.wtime = val,
                        "btime" => limits.btime = val,
                        "winc" => limits.winc = val.unwrap_or(0),
                        "binc" => limits.binc = val.unwrap_or(0),
                        "movestogo" => limits.movestogo = val,
                        "movetime" => limits.movetime = val,
                        "depth" => limits.depth = val.map(|v| v as u32),
                        "nodes" => limits.nodes = val,
                        "infinite" => limits.infinite = true,
                        _ => {}
                    }
                    i += 1;
                }
                let best = searcher.go(&mut board, &limits, true);
                if std::env::var_os("HALF2K_DBG").is_some() {
                }
                match best {
                    Some(m) => {
                        // A resposta que esperamos, do nosso proprio PV. Quem
                        // pondera precisa dela, e nos ja' a temos.
                        match searcher.pv_resposta() {
                            Some(r) => println!("bestmove {} ponder {}", m.to_uci(), r.to_uci()),
                            None => println!("bestmove {}", m.to_uci()),
                        }
                    }
                    // Nothing legal: say so rather than go quiet, which reads
                    // to whoever is waiting as a hung engine.
                    None => println!("bestmove 0000"),
                }
            }
            "stop" => {}
            "quit" => {
                if crate::ponte::conta_ponte() {
                    use std::sync::atomic::Ordering::Relaxed;
                    eprintln!(
                        "PONTE pico {} desequilibrios {}",
                        crate::ponte::PICO.load(Relaxed),
                        crate::ponte::NEGATIVO.load(Relaxed)
                    );
                }
                // Os contadores do acumulador vivem do lado da ponte; so' se
                // imprimem se o ambiente os tiver ligado.

                if crate::search::ttb_ligado() {
                    use std::sync::atomic::Ordering::Relaxed;
                    let v = &crate::search::TTB;
                    let nm = ["exacto  ", "inferior", "superior"];
                    for k in 0..3 {
                        let pri = v[k * 2].load(Relaxed);
                        let cor = v[k * 2 + 1].load(Relaxed);
                        eprintln!("  limite {}  primeiro {:>9}   cortou {:>9}  {:5.1}%",
                                  nm[k], pri, cor, 100.0 * cor as f64 / pri.max(1) as f64);
                    }
                }
                if crate::search::est_ligado() {
                    use std::sync::atomic::Ordering::Relaxed;
                    let e = &crate::search::EST;
                    let n = e[0].load(Relaxed).max(1);
                    let pont = e[1].load(Relaxed);
                    let proc = e[2].load(Relaxed);
                    eprintln!("  nos com lista            {:>12}", n);
                    eprintln!("  lances pontuados         {:>12}   {:5.1} por no", pont, pont as f64 / n as f64);
                    eprintln!("  lances procurados        {:>12}   {:5.1} por no", proc, proc as f64 / n as f64);
                    eprintln!("  pontuados para nada      {:>12}   {:5.1}%",
                              pont.saturating_sub(proc),
                              100.0 * pont.saturating_sub(proc) as f64 / pont.max(1) as f64);
                }
                if crate::search::reb_ligado() {
                    use std::sync::atomic::Ordering::Relaxed;
                    let r = &crate::search::REB;
                    let n = r[0].load(Relaxed).max(1);
                    eprintln!("  lances procurados      {:>12}", n);
                    eprintln!("  re-busca por reducao   {:>12}  {:5.1}%",
                              r[1].load(Relaxed), 100.0 * r[1].load(Relaxed) as f64 / n as f64);
                    eprintln!("  re-busca por janela    {:>12}  {:5.1}%",
                              r[2].load(Relaxed), 100.0 * r[2].load(Relaxed) as f64 / n as f64);
                    eprintln!("  nos gastos a repetir   {:>12}", r[3].load(Relaxed));
                }
                if crate::search::cego_ligado() {
                    let c: Vec<u64> = (0..7)
                        .map(|i| crate::search::CORTES[i].load(std::sync::atomic::Ordering::Relaxed))
                        .collect();
                    let tot: u64 = c.iter().sum();
                    let nm = ["1o", "2o", "3o", "4o", "5-8", "9-16", "17+"];
                    if tot > 0 {
                        for (i, n) in nm.iter().enumerate() {
                            eprintln!("  corte no {:<5} {:>10}  {:>5.1}%", n, c[i], 100.0 * c[i] as f64 / tot as f64);
                        }
                        eprintln!("  cortes totais {}", tot);
                        let cd: Vec<u64> = (0..4)
                            .map(|k| crate::search::CAUDA[k].load(std::sync::atomic::Ordering::Relaxed))
                            .collect();
                        let ct: u64 = cd.iter().sum();
                        if ct > 0 {
                            let nm = ["tabela", "captura", "killer", "tranquilo"];
                            eprintln!("  --- a CAUDA (corte ao 17o ou depois), pelo 1o lance tentado");
                            for k in 0..4 {
                                eprintln!("  {:<12} {:>8}  {:>5.1}%", nm[k], cd[k],
                                    100.0 * cd[k] as f64 / ct as f64);
                            }
                            let sem = crate::search::CAUDA_TT[0].load(std::sync::atomic::Ordering::Relaxed);
                            let com = crate::search::CAUDA_TT[1].load(std::sync::atomic::Ordering::Relaxed);
                            eprintln!("  com lance da tabela {:>6}  {:>5.1}%", com,
                                100.0 * com as f64 / ct.max(1) as f64);
                            eprintln!("  SEM lance da tabela {:>6}  {:>5.1}%", sem,
                                100.0 * sem as f64 / ct.max(1) as f64);
                        }
                    }
                    if crate::search::lmrd_ligado() {
                        let d: Vec<u64> = (0..9)
                            .map(|k| crate::search::LMRD[k].load(std::sync::atomic::Ordering::Relaxed))
                            .collect();
                        let tot: u64 = d.iter().sum();
                        if tot > 0 {
                            let nomes = ["<=-20", "-19..-10", "-9..-5", "-4..-1",
                                         "0", "1..3", "4..7", "8..11", "12+"];
                            eprintln!("  --- lmr_depth do PodaSF (ramo dos tranquilos)");
                            let mut neg = 0u64;
                            for k in 0..9 {
                                if k < 4 { neg += d[k]; }
                                eprintln!("  lmr_depth {:<9} {:>10}  {:>5.1}%",
                                    nomes[k], d[k], 100.0 * d[k] as f64 / tot as f64);
                            }
                            eprintln!("  NEGATIVO {:>21}  {:>5.1}%", neg,
                                100.0 * neg as f64 / tot as f64);
                            let s = crate::search::LMRD_DEV[0].load(std::sync::atomic::Ordering::Relaxed);
                            let n = crate::search::LMRD_DEV[1].load(std::sync::atomic::Ordering::Relaxed).max(1);
                            let a = crate::search::LMRD_DEV[2].load(std::sync::atomic::Ordering::Relaxed);
                            eprintln!("  devolucao do historico: media {:.2} plies, |media| {:.2}, n={}",
                                s as f64 / n as f64, a as f64 / n as f64, n);
                        }

                        // Quem corta, e o que estava a` frente quando falhou.
                        // A distribuicao por indice diz QUANTO se erra; isto
                        // diz ONDE -- que estagio da ordenacao e' que poe a`
                        // frente um lance que nao corta.
                        use std::sync::atomic::Ordering::Relaxed;
                        let nomes = ["tabela", "captura", "killer", "tranquilo"];
                        let ct: Vec<u64> = (0..4)
                            .map(|i| crate::search::CORTE_TIPO[i].load(Relaxed))
                            .collect();
                        let ctt: u64 = ct.iter().sum();
                        if ctt > 0 {
                            eprintln!("  --- quem corta");
                            for (i, n) in nomes.iter().enumerate() {
                                eprintln!("  {:<10} {:>10}  {:>5.1}%", n, ct[i],
                                          100.0 * ct[i] as f64 / ctt as f64);
                            }
                        }
                        let pc: Vec<u64> = (0..4)
                            .map(|i| crate::search::PRIMEIRO_CERTOU[i].load(Relaxed))
                            .collect();
                        let pf: Vec<u64> = (0..4)
                            .map(|i| crate::search::PRIMEIRO_FALHOU[i].load(Relaxed))
                            .collect();
                        let pft: u64 = pf.iter().sum();
                        if pft > 0 {
                            eprintln!("  --- primeiro lance quando NAO cortou ao 1o ({} vezes)", pft);
                            for (i, n) in nomes.iter().enumerate() {
                                eprintln!("  {:<10} {:>10}  {:>5.1}%", n, pf[i],
                                          100.0 * pf[i] as f64 / pft as f64);
                            }
                            eprintln!("  --- killers, por posicao (todos pontuam acima de qualquer tranquilo)");
                            for k in 0..crate::search::NUM_KILLERS {
                                let c1 = crate::search::KILLER_CERTOU[k].load(Relaxed);
                                let f1 = crate::search::KILLER_FALHOU[k].load(Relaxed);
                                if c1 + f1 > 0 {
                                    eprintln!("  killer {}   {:>10} de {:>10}  {:>5.1}%", k + 1, c1, c1 + f1,
                                              100.0 * c1 as f64 / (c1 + f1) as f64);
                                }
                            }
                            eprintln!("  --- taxa de acerto de cada estagio em PRIMEIRO");
                            for (i, n) in nomes.iter().enumerate() {
                                let t = pc[i] + pf[i];
                                if t > 0 {
                                    eprintln!("  {:<10} {:>10} de {:>10}  {:>5.1}%", n, pc[i], t,
                                              100.0 * pc[i] as f64 / t as f64);
                                }
                            }
                        }
                    }
                }
                if crate::search::escala_ligada() {
                    use std::sync::atomic::Ordering::Relaxed;
                    let e = &crate::search::ESCALA;
                    let n = e[0].load(Relaxed).max(1);
                    eprintln!("  lances tranquilos pontuados {}", n);
                    eprintln!("  |hist| medio {:>12}   maior {:>12}",
                              e[1].load(Relaxed) / n, e[2].load(Relaxed));
                    eprintln!("  |principal| medio {:>7}", e[3].load(Relaxed) / n);
                    for k in 0..6 {
                        let v = e[4 + k].load(Relaxed);
                        if v > 0 { eprintln!("  |cont {}| medio {:>10}", k, v / n); }
                    }
                }
                if crate::search::quem_ligado() {
                    for (i, n) in crate::search::NOMES_QUEM.iter().enumerate() {
                        let v = crate::search::QUEM[i].0.load(std::sync::atomic::Ordering::Relaxed);
                        let m = crate::search::QUEM[i].1.load(std::sync::atomic::Ordering::Relaxed);
                        let ab = crate::search::QUEM_ABS[i].load(std::sync::atomic::Ordering::Relaxed);
                        eprintln!(
                            "  {:<10} {:>8} vezes  sinal {:>13}  modulo {:>13}  medio |{}|",
                            n, v, m, ab, if v > 0 { ab / v } else { 0 }
                        );
                    }
                }
                break;
            }
            "eval" => {
                let side = board.side;
                let s = crate::search::debug_eval(&mut board, searcher.features.rule50_fade);
                let (w, d, l) = nnue::wdl(s);
                println!(
                    "eval {} (side to move: {}) wdl {} {} {}",
                    s,
                    if side == Color::White { "white" } else { "black" },
                    w,
                    d,
                    l
                );
            }
            _ => {}
        }
    }
}
