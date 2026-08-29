mod advisor;
mod attacks;
mod bitboard;
mod board;
mod book;
mod endgame;
mod magic;
mod movegen;
mod nnue;
mod nnue_sf;
mod nnue_sf_ffi;
mod sf_features;
mod evaluation;
mod moves;
mod perft;
mod search;
mod syzygy;
mod tablebase;
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
    // Buckets de PSQT: ligados por feature. Arrancam com as OITO tabelas
    // iguais a' compilada, que e' a condicao do invariante da identidade --
    // nesse estado o motor com buckets tem de avaliar exactamente como o motor
    // sem eles, em qualquer posicao.
    if cfg!(feature = "psqtbuckets") {
        // Nada a fazer aqui: as tabelas enchem-se sozinhas na primeira
        // leitura, com as compiladas, e um carregamento afinado que chegue
        // antes disso substitui-as.
    }

    let args: Vec<String> = env::args().collect();
    if args.len() >= 4 && args[1] == "para_bullet" {
        // para_bullet <net SF (ex.: torch serializado .nnue)> <saida_pesos.bin>
        // Converte um net SF para os pesos do bullet, no formato do
        // load_weights_from_file, para warm-start do treino.
        let bytes = match std::fs::read(&args[2]) {
            Ok(b) => b,
            Err(e) => { eprintln!("nao consegui ler {}: {e}", args[2]); return; }
        };
        let net = match nnue_sf::carrega_pub(&bytes) {
            Some(n) => n,
            None => { eprintln!("net SF invalido"); return; }
        };
        let pesos = nnue_sf::para_bullet_pesos(&net);
        let out = nnue_sf::escreve_pesos_bullet(&pesos);
        match std::fs::write(&args[3], &out) {
            Ok(()) => println!("escrito: {} ({} bytes, {} tensores)", args[3], out.len(), pesos.len()),
            Err(e) => eprintln!("nao consegui escrever: {e}"),
        }
        return;
    }

    if args.len() >= 2 && args[1] == "bench" {
        // Fixed-work benchmark, in the shape a distributed test framework
        // expects: a total node count and a rate, on a fixed position set at
        // a fixed depth. The node count is a build signature -- two commits
        // that search identically must report the same number, which is how a
        // framework detects that a change meant to be non-functional was not,
        // or that a worker built something other than what it was asked to.
        let depth: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(11);
        bench(depth);
        nnue_sf::imprime_rels();
        return;
    }
    // Confronta see_ge(mv, t) com see(mv) >= t em todas as capturas de todas
    // as posicoes do bench, para cada limiar num intervalo largo. Uma so'
    // discordancia e' um bug -- as duas TEM de ser a mesma funcao.
    if args.len() >= 2 && args[1] == "seetest" {
        let atk = attacks::Attacks::new();
        let mut testes = 0u64;
        let mut falhas = 0u64;
        for fen in BENCH_FENS.iter() {
            let mut b = board::Board::from_fen(fen);
            let moves = movegen::generate_legal(&mut b, &atk);
            for mv in moves.iter() {
                for t in [-1000, -500, -330, -100, -1, 0, 1, 100, 330, 500, 900, 1000] {
                    let exacto = search::see::see(&atk, &b, mv) >= t;
                    let rapido = search::see::see_ge(&atk, &b, mv, t);
                    testes += 1;
                    if exacto != rapido {
                        falhas += 1;
                        if falhas <= 5 {
                            println!(
                                "DISCORDA fen={} lance={}->{} limiar={} exacto={} rapido={} see={}",
                                fen,
                                mv.from,
                                mv.to,
                                t,
                                exacto,
                                rapido,
                                search::see::see(&atk, &b, mv)
                            );
                        }
                    }
                }
            }
        }
        println!("seetest: {} comparacoes, {} discordancias", testes, falhas);
        return;
    }

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
    // SPSA config for an external tuning harness (OpenBench's format). We do
    // not run our own SPSA: this exports "name, int, default,
    // min, max, step, C_end" and lets OpenBench (or WeatherFactory) play
    // the games and do the math. `PARAM_NAMES`/`SearchParams::to_vec` are
    // already the same kind of registry that reference's `SEARCH_PARAM`
    // macro builds; this just prints it in the shape OpenBench expects,
    // reusing the exact min/max band the UCI option listing already uses
    // (see the `option name ... type spin` loop) so the two never disagree
    // about a parameter's legal range.
    // List the feature switches and what they cost, so "which of the 74 is
    // paying for this" has an answer that does not require reading eval.rs.

    // The full starting vector, in exactly the layout the extractor emits.
    //
    // Two tuning runs were lost to this. The positional block came from
    // `dumpweights` and the material/PST block from `dumpmatpst`, and if the
    // two were produced by binaries that disagreed -- one before a change, one
    // after -- the fit started from a set of weights no engine ever held. Once
    // that meant material counted twice (3-797); once it meant the tables were
    // exported against material they had not been fitted with (0-799-1).
    //
    // One command, one binary, one read of the weights the engine ACTUALLY
    // uses (gated, fitted, whatever this build resolves to), laid out as
    //   dim positional | MAT_PST_DIM material and PST | 1 bias
    // per bucket. If the file and the .bin come from the same executable they
    // cannot disagree.
    // Converte o formato .data2 (registos de 112 bytes com BITBOARDS e
    // rotulos de um motor forte) para o `FEN<TAB>resultado` que o gpuextract le.
    //
    // Le directamente os bitboards: sao a mesma estrutura que Board.pieces,
    // portanto nao ha' descodificacao nenhuma, so' montar o tabuleiro e pedir
    // o FEN. O rotulo usado e' o wdl (resultado), nao o cp, porque e' a
    // resultados que o ajuste que ganhou 152 Elo foi feito.
    //
    //   data2epd <ficheiro.data2> <saida.epd> [n_posicoes] [passo]
    //
    // `passo` salta registos para amostrar o ficheiro todo em vez dos
    // primeiros N: o ficheiro ja' vem baralhado, mas amostrar em passo
    // protege contra qualquer ordem residual.
    // Mede o K do motor contra rotulos externos.
    //
    //   medek <ficheiro.epd>     (FEN<TAB>resultado, resultado no POV das brancas)
    //
    // O sigmoide e' 1/(1+10^(-k*eval/400)), portanto o K CLASSICO e' 400/k.
    // k=1 significa que um centipeao nosso vale o mesmo que um centipeao do
    // os motores de referencia, que usam K=400. k<1 significa que avaliamos
    // mais alto do que a probabilidade de vitoria justifica -- e como as
    // margens de poda sao em centipeoes FIXOS, uma avaliacao inflacionada
    // deixa-as efectivamente mais apertadas do que quem as calibrou queria.
    // Mede a QUIETUDE de um conjunto de treino.
    //
    //   quietude <ficheiro.epd> [n]
    //
    // Fitting weights to game results demands quiet positions: avaliar estaticamente uma
    // posicao onde ha' uma peca pendurada e compara-la com o resultado da
    // partida injecta ruido enorme, e o ruido cai desproporcionadamente no
    // tempo -- porque numa posicao com material pendurado e' QUEM JOGA que o
    // captura.
    // Radiografia de uma avaliacao: mete-se um FEN e ve-se TUDO o que
    // interfere ate' ao numero final -- os termos, os multiplicadores que se
    // sobrepoem a eles, a interpolacao, e o que sobra no fim.
    //
    //   raiox "<fen>"

    // `filtraquieto <in> <out> [best_move_col]` -- o filtro das duas referencias.
    //
    // Porque existe: avaliar estaticamente uma posicao onde uma peca esta
    // pendurada e comparar com o rotulo injecta o tamanho dessa peca como
    // ruido. Numa posicao onde se esta uma dama acima MAS ela vai ser
    // recapturada, a avaliacao e' ~0 -- e o modelo aprende, correctamente para
    // aqueles dados, que uma dama nao vale nada. Medido: descartar tacticas
    // subiu a dama de 5,75 para 6,75 peoes num so' superbatch.
    //
    // Os criterios sao os das duas referencias, que coincidem no essencial:
    //
    //   min_ply       16   as aberturas repetem-se milhoes de vezes e valem
    //                      todas ~0; treinar nelas e' ensinar "esta igual"
    //   min_pieces     4   em finais nus decide a tecnica, nao a avaliacao
    //   tactical      sim  o LANCE ser captura ou promocao (nao a posicao ter
    //                      capturas disponiveis -- isso descartava demasiado)
    //   check         sim
    //
    // Sem a coluna do melhor lance nao se sabe se ELE e' tactico; nesse caso
    // cai-se no criterio da posicao, que e' mais agressivo e fica dito.




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
    if args.len() >= 4 && args[1] == "funde" {
        // funde <quantised.bin> <saida.bin>
        //
        // NAO soma nada. O treinador ja' fundiu a factorizacao nos pesos
        // reais antes de gravar -- os blocos dos buckets ja' trazem o fator
        // comum incluido. O bloco virtual continua la' no ficheiro, mas e'
        // residuo, nao parcela.
        //
        // Somei-o na mesma durante uma tarde. A rede carregava, jogava e
        // perdia oitenta e oito jogos em oitenta e oito, com um desvio de
        // ~350 centipeoes que NAO descia com o treino (358 aos dez
        // superbatches, 385 aos vinte, 388 aos cinquenta) -- que e'
        // precisamente a assinatura de um erro de leitura e nao de uma rede
        // por treinar. Descartar o bloco: +6 onde a referencia diz +32.
        // Somar o bloco: +388.
        let cru = std::fs::read(&args[2]).expect("nao consegui ler");
        let vals: Vec<i16> = cru.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        let bloco = nnue::INPUTS * nnue::HIDDEN;
        let (cauda, blocos) = {
            let mut r = None;
            for ob in [8usize, 1] {
                let c = nnue::HIDDEN + 2 * nnue::HIDDEN * ob + ob;
                if vals.len() <= c { continue; }
                let n = (vals.len() - c) / bloco;
                if n >= 1 && (vals.len() - c) - n * bloco < 64 { r = Some((c, n)); break; }
            }
            match r {
                Some(v) => v,
                None => { eprintln!("funde: forma desconhecida, {} valores", vals.len()); return; }
            }
        };
        if blocos < 2 {
            eprintln!("funde: {} bloco(s) -- nada a descartar", blocos);
            return;
        }
        let reais = blocos - 1;
        let mut saida: Vec<i16> = Vec::with_capacity(reais * bloco + cauda);
        saida.extend_from_slice(&vals[bloco..blocos * bloco]);
        saida.extend_from_slice(&vals[blocos * bloco..blocos * bloco + cauda]);
        let bytes: Vec<u8> = saida.iter().flat_map(|v| v.to_le_bytes()).collect();
        nnue::escreve_com_cabecalho(&bytes, reais, &args[3]).expect("nao consegui escrever");
        println!("limpa: {} blocos -> {} buckets reais (virtual descartado) -> {}", blocos, reais, args[3]);
        return;
    }
    if args.len() >= 4 && args[1] == "selarede" {
        // selarede <quantised.bin> <saida.bin> [buckets]
        //
        // Poe o nosso cabecalho a frente do dump cru do treinador, para o
        // motor poder VERIFICAR a rede em vez de adivinhar a forma dela pelo
        // tamanho do ficheiro.
        let cru = std::fs::read(&args[2]).expect("nao consegui ler");
        let buckets: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
            let cauda = nnue::HIDDEN + 2 * nnue::HIDDEN + 1;
            let total = cru.len() / 2;
            if total > cauda { ((total - cauda) / (nnue::INPUTS * nnue::HIDDEN)).max(1) } else { 1 }
        });
        nnue::escreve_com_cabecalho(&cru, buckets, &args[3]).expect("nao consegui escrever");
        println!("selada: {} buckets, HIDDEN={} -> {}", buckets, nnue::HIDDEN, args[3]);
        return;
    }
    if args.len() >= 4 && args[1] == "empacota" {
        // empacota <rede.bin> <saida.leb128.bin>
        //
        // Recomprime uma rede ja selada (v1, dump cru + cabecalho) para o
        // formato v2 (LEB128+zigzag). So' o ficheiro em disco muda de forma;
        // o array i16 em memoria depois de carregado e' byte-a-byte o mesmo,
        // por isso a verificacao a seguir (nao so' o tamanho) e' o que prova
        // que isto e' seguro para o bot usar.
        let bytes = std::fs::read(&args[2]).expect("nao consegui ler");
        let net = match nnue::load(&bytes) {
            Some(n) => n,
            None => { eprintln!("nao consegui interpretar {} como rede", args[2]); return; }
        };
        nnue::empacota_rede(&net, &args[3]).expect("nao consegui escrever");
        let antes = bytes.len();
        let depois = std::fs::metadata(&args[3]).map(|m| m.len()).unwrap_or(0);
        println!(
            "empacotada: {} -> {} bytes ({:.1}% do original) -> {}",
            antes, depois, 100.0 * depois as f64 / antes as f64, args[3]
        );
        return;
    }
    if args.len() >= 3 && args[1] == "carregatest" {
        // carregatest <rede.bin> [repeticoes]
        //
        // Isola SO' o custo de ler+interpretar o ficheiro, para responder a
        // "fica mais rapido a carregar" sem o ruido de uma pesquisa inteira
        // por cima (foi isso que aconteceu na primeira medicao: bench()
        // gasta a maior parte do tempo em milhoes de nos, nao no parse).
        let n: usize = args.get(2 + 1).and_then(|s| s.parse().ok()).unwrap_or(2000);
        let path = &args[2];
        // Um read() de aquecimento poe o ficheiro na page cache do SO antes
        // de cronometrar -- sem isto o PRIMEIRO ficheiro medido pagaria I/O
        // de disco real e o segundo nao, o que mediria a ordem e nao o
        // formato.
        let _ = std::fs::read(path);
        let start = std::time::Instant::now();
        for _ in 0..n {
            let bytes = std::fs::read(path).expect("nao consegui ler");
            let net = nnue::load(&bytes).expect("nao consegui interpretar");
            std::hint::black_box(&net);
        }
        let total = start.elapsed();
        println!(
            "{}: {} iteracoes, {:.1} us/iteracao (ler+interpretar)",
            path, n, total.as_secs_f64() * 1e6 / n as f64
        );
        return;
    }
    if args.len() >= 4 && args[1] == "verificaempacota" {
        // verificaempacota <original.bin> <empacotado.bin>
        //
        // Carrega os dois e compara os quatro tensores VALOR A VALOR -- nao
        // so' o tamanho do ficheiro, nao so' um checksum. Isto e' o que
        // decide se o formato novo pode substituir o ficheiro que o bot le.
        let a = nnue::load(&std::fs::read(&args[2]).expect("nao consegui ler original"))
            .expect("original nao carrega");
        let b = nnue::load(&std::fs::read(&args[3]).expect("nao consegui ler empacotado"))
            .expect("empacotado nao carrega");
        let iguais = a.l0w == b.l0w && a.l0b == b.l0b && a.l1w == b.l1w && a.l1b == b.l1b
            && a.buckets == b.buckets && a.output_buckets == b.output_buckets;
        if iguais {
            println!("identico: {} valores, byte a byte", a.l0w.len() + a.l0b.len() + a.l1w.len() + a.l1b.len());
        } else {
            println!("DIFERENTE -- nao substituir a rede em producao com isto");
            std::process::exit(1);
        }
        return;
    }
    if args.len() >= 2 && args[1] == "verificahash" {
        // Compares the incrementally maintained Board::hash against a full
        // recompute at EVERY node of a perft, forwards AND after every undo.
        // Perft is the right driver: it enumerates castling, en passant,
        // promotion and check evasion exhaustively rather than sampling them,
        // and those are exactly the moves where an incremental key drifts.
        let atk = Attacks::new();
        let z = zobrist::tabelas();
        let depth: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4);
        let mut board = match args.get(3) {
            Some(f) => Board::from_fen(f),
            None => Board::startpos(),
        };
        fn caminha(b: &mut Board, atk: &Attacks, z: &zobrist::Zobrist, d: i32,
                   mal: &mut u64, n: &mut u64) {
            if d == 0 {
                return;
            }
            for mv in movegen::generate_legal(b, atk) {
                let undo = b.make_move(&mv);
                *n += 1;
                if b.hash != z.hash_completo(b) {
                    *mal += 1;
                    if *mal <= 3 {
                        eprintln!("DIVERGE apos {} -> {}", mv.to_uci(), b.to_fen());
                    }
                }
                caminha(b, atk, z, d - 1, mal, n);
                b.unmake_move(&mv, &undo);
                if b.hash != z.hash_completo(b) {
                    *mal += 1;
                    if *mal <= 3 {
                        eprintln!("DIVERGE ao desfazer {} -> {}", mv.to_uci(), b.to_fen());
                    }
                }
            }
        }
        let (mut mal, mut n) = (0u64, 0u64);
        caminha(&mut board, &atk, z, depth, &mut mal, &mut n);
        println!("hash: {} lances verificados (ida e volta), {} divergencias", n, mal);
        if mal > 0 {
            std::process::exit(1);
        }
        return;
    }
    if args.len() >= 2 && args[1] == "verificacache" {
        // A cache de refresh contra a reconstrucao do zero, em posicoes reais.
        let net = match nnue::rede() {
            Some(n) => n,
            None => { eprintln!("precisa de KESTREL_NNUE"); return; }
        };
        let ficheiro = args.get(2).map(|s| s.as_str()).unwrap_or("blunders.epd");
        let fens: Vec<String> = std::fs::read_to_string(ficheiro)
            .expect("nao consegui ler")
            .lines()
            .filter_map(|l| l.split('|').next().map(|f| f.trim().to_string()))
            .filter(|f| !f.is_empty())
            .collect();
        let (t, e) = nnue::verifica_cache(net, &fens);
        println!("cache de refresh: {} verificacoes, {} erradas", t, e);
        return;
    }
    if args.len() >= 4 && args[1] == "bulletdata" {
        // bulletdata <in.epd> <out.txt> [depth] [threads]
        //
        // Turns `FEN<TAB>result` into the `FEN | score | result` the network
        // trainer reads. The score is our own search, shallow: the label the
        // net fits is a blend of the game result and this score, so the score
        // only has to order positions sensibly, not be deep. Depth 8 over a
        // million and a half positions is minutes; depth 16 would be hours and
        // buys nothing the result label does not already carry.
        let d: i32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(8);
        let t: usize = args.get(5).and_then(|s| s.parse().ok())
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));
        bullet_data(&args[2], &args[3], d, t);
        return;
    }
    // psqtbase <saida.bin> -- a semente do PSQT para o treino do SFNNv16.
    //
    // O treinador de referencia nao manda a rede descobrir quanto vale uma dama:
    // escreve os valores no PSQT a' partida (`halfka_psqts`) e o treino so' os
    // AFINA. O bullet inicializa `psqtw` a zeros de proposito e espera este
    // ficheiro em `SF_PSQT_BASE`.
    //
    // Gerado AQUI, com `sf_features::indice_peca`, e nao por um script a' parte:
    // a semente que existia (`E:/psqt_base.bin`, 19 Ago) tinha os valores certos
    // mas noutra ordem de indices, e como o ficheiro carrega sem erro ninguem
    // dava por isso. Medido na rede treinada a 28 Ago: PSQT indiferenciado entre
    // peao e dama, |media| 0.0012 -- e o motor avaliava BRANCAS A MENOS UMA DAMA
    // em +49. Enquanto o gerador viver ao lado da funcao de indexacao, as duas
    // nao podem divergir sem que o compilador se queixe.
    //
    // Escala: o PSQT entra como `(psqt(stm) - psqt(ntm)) * 0.5`, e a mesma dama
    // e' +v de um lado e -v do outro, logo a diferenca e' 2v e o 0.5 cancela-a.
    // O valor guardado e' portanto `cp / nnue2score` directo, sem correccao.
    // ameacasativas <fen> ... -- quantas ameacas estao activas numa posicao.
    //
    // Serve para decidir se o bloco de ameacas pode ser calculado de raiz so'
    // onde se avalia, em vez de mantido incremental. Mede-se o que ha' contra o
    // que muda: se as activas forem muitas mais que as ~7.5 que mudam por
    // lance, calcular de raiz e' pior e a pergunta fica respondida sem
    // escrever uma linha de codigo novo.
    // permameacas <saida.bin> -- correr o bench a contar e escrever a permutacao
    // que ordena as linhas de ameaca por frequencia de uso.
    if args.len() >= 3 && args[1] == "permameacas" {
        std::env::set_var("KESTREL_HISTO_AMEACAS", "1");
        bench(11);
        let p = nnue_sf::permutacao_por_uso();
        if p.is_empty() {
            eprintln!("sem histograma -- o bench nao tocou em ameacas nenhumas");
            return;
        }
        let mut out = Vec::with_capacity(p.len() * 4);
        for v in &p {
            out.extend_from_slice(&v.to_le_bytes());
        }
        match std::fs::write(&args[2], &out) {
            Ok(()) => println!("escrito: {} ({} entradas, {} bytes)", args[2], p.len(), out.len()),
            Err(e) => eprintln!("nao consegui escrever: {e}"),
        }
        return;
    }

    if args.len() >= 3 && args[1] == "ameacasativas" {
        let mut tot = 0usize;
        let mut n = 0usize;
        for fen in &args[2..] {
            let b = board::Board::from_fen(fen);
            let pos = nnue_sf::board_para_posbb_pub(&b);
            for pov in 0..2 {
                let mut v: Vec<usize> = Vec::new();
                nnue_sf::threats_pad_pub(&pos, pov, &mut v);
                tot += v.len();
                n += 1;
            }
        }
        if n > 0 {
            println!("{} perspectivas, {} ameacas activas, media {:.1}", n, tot, tot as f64 / n as f64);
        }
        return;
    }

    if args.len() >= 3 && args[1] == "psqtbase" {
        const FACT: usize = 704;
        const FEAT: usize = 86896;
        const DUST: usize = 1;
        const NIN: usize = FACT + FEAT + DUST;
        const NB: usize = 8;
        const NNUE2SCORE: f32 = 600.0;
        // `halfka_psqts` do nnue-pytorch, em centipeoes. O rei nao entra: esta'
        // sempre presente dos dois lados e o valor anular-se-ia.
        const CP: [f32; 6] = [126.0, 781.0, 825.0, 1276.0, 2538.0, 0.0];

        let mut w = vec![0f32; NB * NIN];
        let mut escritos = 0usize;
        for ksq in 0..64usize {
            for pov in 0..2usize {
                for sq in 0..64usize {
                    for peca in 0..6usize {
                        for cor in 0..2usize {
                            // peoes nao vivem nas filas 1 e 8
                            if peca == 0 && (sq < 8 || sq >= 56) { continue; }
                            let idx = crate::sf_features::indice_peca(ksq, pov, sq, peca, cor);
                            if idx >= FEAT { continue; }
                            // SINAL: `KESTREL_PSQT_SINAL=-1` inverte.
                            //
                            // A convencao obvia -- as nossas pecas positivas --
                            // produziu uma rede que avalia AO CONTRARIO: brancas
                            // a menos uma dama a +1385. O motor faz
                            // `(psqt_s - psqt_n) / 2` e o treinador o mesmo, por
                            // isso a formula nao explica a inversao; so' medindo
                            // se sabe qual das duas e' a certa.
                            let inv: f32 = std::env::var("KESTREL_PSQT_SINAL")
                                .ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
                            let sinal = inv * if cor == pov { 1.0 } else { -1.0 };
                            let v = sinal * CP[peca] / NNUE2SCORE;
                            for b in 0..NB {
                                w[b * NIN + FACT + idx] = v;
                            }
                            escritos += 1;
                        }
                    }
                }
            }
        }
        let mut saida: Vec<u8> = Vec::with_capacity(14 + w.len() * 4);
        saida.extend_from_slice(b"psqtw\n");
        saida.extend_from_slice(&(w.len() as u64).to_le_bytes());
        for v in &w { saida.extend_from_slice(&v.to_le_bytes()); }
        match std::fs::write(&args[2], &saida) {
            Ok(()) => {
                let nz = w.iter().filter(|x| **x != 0.0).count();
                println!("escrito: {} ({} bytes)", args[2], saida.len());
                println!("  {escritos} escritas, {nz} pesos nao-zero de {}", w.len());
                println!("  dama {:+.4}  torre {:+.4}  peao {:+.4}",
                         CP[4] / NNUE2SCORE, CP[3] / NNUE2SCORE, CP[0] / NNUE2SCORE);
            }
            Err(e) => eprintln!("nao consegui escrever {}: {e}", args[2]),
        }
        return;
    }

    if args.len() >= 5 && args[1] == "sfconvert" {
        // sfconvert <raw.bin do bullet> <molde.nnue> <saida.nnue>
        //
        // bullet's raw.bin is the source of truth: plain f32 tensors, written
        // in the store's alphabetical order (fc0b, fc0w, fc1b, fc1w, fc2b,
        // fc2w, l0b, l0w). The mould supplies the header fields so
        // Stockfish's architecture-hash check passes.
        const FACT: usize = 704;
        const DUST: usize = 1;
        const FEAT: usize = 86896;
        const NIN: usize = FACT + FEAT + DUST;
        const L1: usize = 1024;
        const L2: usize = 32;
        const L3: usize = 32;
        const NB: usize = 8;

        let raw = match std::fs::read(&args[2]) {
            Ok(b) => b,
            Err(e) => { eprintln!("nao consegui ler {}: {e}", args[2]); return; }
        };
        let f32s: Vec<f32> = raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        // Alphabetical, as the trainer's weight store emits them. The
        // per-layer factorisers (fc0f/fc1f/fc2f) are shared across buckets and
        // must be folded into every bucket here -- the .nnue format has no
        // notion of them.
        let tam = [
            ("fc0b", L2 * NB), ("fc0fb", L2), ("fc0fw", L2 * L1), ("fc0w", L2 * NB * L1),
            ("fc1b", L3 * NB), ("fc1fb", L3), ("fc1fw", L3 * 2 * L2), ("fc1w", L3 * NB * 2 * L2),
            ("fc2b", NB), ("fc2fb", 1), ("fc2fw", 2 * L2 + 2 * L3), ("fc2w", NB * (2 * L2 + 2 * L3)),
            ("l0b", L1), ("l0w", L1 * NIN),
            ("psqtb", NB), ("psqtw", NB * NIN),
        ];
        let total: usize = tam.iter().map(|(_, n)| n).sum();
        // Duas formas validas, e a diferenca entre elas sao 2209 valores.
        //
        // O treinador deixou de factorizar a fc_1 e a fc_2, para ficar igual ao
        // nnue-pytorch, que factoriza SO' a primeira camada -- depois de uma
        // nao-linearidade a componente partilhada limita-se a fazer a media dos
        // baldes e impede a saida de divergir por balde, que e' precisamente o
        // que os finais precisam. Os checkpoints novos vem sem `fc1f`/`fc2f`:
        //     fc1f = L3*2*L2 + L3 = 2080
        //     fc2f = (2*L2 + 2*L3) + 1 = 129
        // Aceitar as duas em vez de recusar a nova: sem factorizador, a
        // componente partilhada e' zero, que e' exactamente o que somar nada
        // significa.
        let sem_fc12f = L3 * 2 * L2 + L3 + (2 * L2 + 2 * L3) + 1;
        let tem_fc12f = f32s.len() == total;
        if !tem_fc12f && f32s.len() != total - sem_fc12f {
            eprintln!("raw.bin tem {} valores, esperava {total} (com fc1f/fc2f) ou {} (sem) -- arquitectura diferente?",
                      f32s.len(), total - sem_fc12f);
            return;
        }
        if !tem_fc12f {
            println!("checkpoint sem fc1f/fc2f (treinador alinhado com o nnue-pytorch): a usar zeros");
        }
        let mut o = 0usize;
        let mut get = |n: usize| -> &[f32] { let s = &f32s[o..o + n]; o += n; s };
        // A ORDEM E' A DA LISTA `SavedFormat`, nao alfabetica.
        //
        // `save_unquantised` percorre `for fmt in saved_format` e escreve os
        // tensores por essa ordem, tal como estao no optimizador -- sem aplicar
        // `.transpose()` nem `.round()`, que so' valem para o `quantised.bin`.
        //
        // Liamos isto por ordem alfabetica, portanto TODOS os tensores vinham
        // do sitio errado. Foi isso que obrigou a inventar uma negacao da saida
        // e que deixou a transposicao a parecer ambigua: nao havia layout
        // nenhum que salvasse a leitura, porque o problema era estarmos a ler
        // os pesos uns dos outros.
        let l0w = get(L1 * NIN).to_vec();
        let l0b = get(L1).to_vec();
        let fc0w = get(L2 * NB * L1).to_vec();
        let fc0b = get(L2 * NB).to_vec();
        let fc1w = get(L3 * NB * 2 * L2).to_vec();
        let fc1b = get(L3 * NB).to_vec();
        let fc2w = get(NB * (2 * L2 + 2 * L3)).to_vec();
        let fc2b = get(NB).to_vec();
        let fc0fw = get(L2 * L1).to_vec();
        let fc0fb = get(L2).to_vec();
        let (fc1fw, fc1fb, fc2fw, fc2fb) = if tem_fc12f {
            (get(L3 * 2 * L2).to_vec(), get(L3).to_vec(),
             get(2 * L2 + 2 * L3).to_vec(), get(1).to_vec())
        } else {
            (vec![0.0; L3 * 2 * L2], vec![0.0; L3],
             vec![0.0; 2 * L2 + 2 * L3], vec![0.0; 1])
        };
        let psqtw = get(NB * NIN).to_vec();
        let _psqtb = get(NB).to_vec();

        let molde_bytes = match std::fs::read(&args[3]) {
            Ok(b) => b,
            Err(e) => { eprintln!("nao consegui ler o molde {}: {e}", args[3]); return; }
        };
        let molde = match nnue_sf::carrega_pub(&molde_bytes) {
            Some(n) => n,
            None => { eprintln!("molde invalido"); return; }
        };

        let net = nnue_sf::de_bullet(&molde, &l0w, &l0b, &fc0w, &fc0b, &fc1w, &fc1b, &fc2w, &fc2b, &psqtw,
            &fc0fw, &fc0fb, &fc1fw, &fc1fb, &fc2fw, &fc2fb);
        let saida = nnue_sf::escreve(&net);
        match std::fs::write(&args[4], &saida) {
            Ok(()) => println!("escrito: {} ({} bytes)", args[4], saida.len()),
            Err(e) => eprintln!("nao consegui escrever: {e}"),
        }
        return;
    }
    if args.len() >= 2 && args[1] == "dustbin" {
        // Quanto e' que o dustbin pesa, em numeros e nao em opiniao.
        //
        // Para cada posicao do bench conta, por perspectiva, as features de
        // ameaca reais e as que caem no dustbin -- e sobretudo as ASSIMETRICAS,
        // que sao reais de um lado e dustbin do outro. E' a assimetria que
        // impede o dustbin de ser uma constante absorvivel.
        let mut t_reais = 0u64;
        let mut t_dust = 0u64;
        let mut t_assim = 0u64;
        let mut pos_com_assim = 0u64;
        let mut n = 0u64;
        let mut pior = 0usize;
        for fen in BENCH_FENS.iter() {
            let mut b = board::Board::from_fen(fen);
            let pb = nnue_sf::board_para_posbb_pub(&mut b);
            let mut f0 = Vec::new();
            let mut f1 = Vec::new();
            nnue_sf::threats_pad_pub(&pb, 0, &mut f0);
            nnue_sf::threats_pad_pub(&pb, 1, &mut f1);
            let dust = nnue_sf::threat_dim_pub();
            let d0 = f0.iter().filter(|&&x| x == dust).count();
            let d1 = f1.iter().filter(|&&x| x == dust).count();
            // As listas vem alinhadas por construcao (e' para isso que existe o
            // dustbin), logo o par i e' a mesma ameaca vista dos dois lados.
            let mut assim = 0usize;
            for i in 0..f0.len().min(f1.len()) {
                if (f0[i] == dust) != (f1[i] == dust) { assim += 1; }
            }
            t_reais += (f0.len() - d0 + f1.len() - d1) as u64;
            t_dust += (d0 + d1) as u64;
            t_assim += 2 * assim as u64;
            if assim > 0 { pos_com_assim += 1; }
            if assim > pior { pior = assim; }
            n += 1;
        }
        let tot = t_reais + t_dust;
        println!("{n} posicoes do bench");
        println!("features de ameaca (as duas perspectivas): {tot}");
        println!("  reais:      {t_reais} ({:.1}%)", 100.0 * t_reais as f64 / tot as f64);
        println!("  no dustbin: {t_dust} ({:.1}%)", 100.0 * t_dust as f64 / tot as f64);
        println!("  ASSIMETRICAS (real de um lado, dustbin do outro): {t_assim} ({:.2}%)",
            100.0 * t_assim as f64 / tot as f64);
        println!("posicoes com pelo menos uma assimetrica: {pos_com_assim} de {n}");
        println!("pior posicao: {pior} ameacas assimetricas");
        return;
    }
    if args.len() >= 4 && args[1] == "sfbulletrt" {
        // sfbulletrt <rede.nnue> <saida.nnue>
        //
        // Escreve a rede no layout do bullet e volta a le-la pelo `de_bullet`.
        // A resposta certa e' conhecida -- tem de sair a rede de partida.
        let bytes = match std::fs::read(&args[2]) {
            Ok(b) => b,
            Err(e) => { eprintln!("nao consegui ler {}: {e}", args[2]); return; }
        };
        let net = match nnue_sf::carrega_pub(&bytes) {
            Some(n) => n,
            None => { eprintln!("nao consegui interpretar {}", args[2]); return; }
        };
        let volta = nnue_sf::roundtrip_bullet(&net);
        let saida = nnue_sf::escreve(&volta);
        println!("entrada: {} bytes", bytes.len());
        println!("saida:   {} bytes", saida.len());
        if saida == bytes {
            println!("IDENTICO -- o de_bullet reproduz a rede exactamente");
        } else {
            let n = bytes.len().min(saida.len());
            let dif = (0..n).filter(|&i| bytes[i] != saida[i]).count();
            let prim = (0..n).find(|&i| bytes[i] != saida[i]).unwrap_or(n);
            println!("DIFERENTE: {} bytes diferentes de {}, o primeiro em {}", dif, n, prim);
        }
        let _ = std::fs::write(&args[3], &saida);
        println!("escrito: {}", args[3]);
        return;
    }
    if args.len() >= 3 && args[1] == "sfroundtrip" {
        // sfroundtrip <rede.nnue> [saida.nnue]
        //
        // Reads a Stockfish net with our own reader and writes it straight
        // back out. If the bytes come out identical to the input, the
        // serialiser reproduces SF's format exactly -- which is the
        // precondition for writing a net Stockfish will accept.
        let entrada = &args[2];
        let bytes = match std::fs::read(entrada) {
            Ok(b) => b,
            Err(e) => { eprintln!("nao consegui ler {entrada}: {e}"); return; }
        };
        let net = match nnue_sf::carrega_pub(&bytes) {
            Some(n) => n,
            None => { eprintln!("nao consegui interpretar {entrada}"); return; }
        };
        let saida_bytes = nnue_sf::escreve(&net);
        println!("entrada: {} bytes", bytes.len());
        println!("saida:   {} bytes", saida_bytes.len());
        if saida_bytes == bytes {
            println!("IDENTICO -- o serializador reproduz o formato do SF byte a byte");
        } else {
            println!("DIFERENTE");
            let n = bytes.len().min(saida_bytes.len());
            let primeiro = (0..n).find(|&i| bytes[i] != saida_bytes[i]);
            match primeiro {
                Some(i) => println!("  primeiro byte diferente no offset {i}"),
                None => println!("  prefixo igual, comprimentos diferentes"),
            }
        }
        if let Some(dest) = args.get(3) {
            if let Err(e) = std::fs::write(dest, &saida_bytes) {
                eprintln!("nao consegui escrever {dest}: {e}");
            } else {
                println!("escrito: {dest}");
            }
        }
        return;
    }
    if args.len() >= 4 && args[1] == "accdump" {
        // accdump <fens.txt> <out.bin>
        //
        // Dumps, per position, the 1024 post-SCReLU activations that feed the
        // output layer, plus the network's own score. For asking offline what
        // a different READOUT could do with the features this network already
        // computes -- fitting a per-king-zone weight vector against the single
        // global one, say -- which answers an output-bucketing question
        // without paying for a training run to find out.
        //
        // The score rides along as a checksum, not as a label: a fit that
        // reproduces `evaluate()` from these activations and the network's own
        // l1 weights proves the dumped matrix is the one the engine actually
        // reads. Without that check a transposed or mis-ordered dump would
        // still produce a plausible-looking regression against any target.
        let Some(net) = nnue::rede() else {
            eprintln!("accdump precisa de KESTREL_NNUE=<rede.bin>");
            return;
        };
        let texto = match std::fs::read_to_string(&args[2]) {
            Ok(t) => t,
            Err(e) => { eprintln!("nao consegui ler {}: {e}", args[2]); return; }
        };
        let mut bin: Vec<u8> = Vec::new();
        let mut fens_usadas = String::new();
        let mut n = 0usize;
        let mut saltadas = 0usize;
        for linha in texto.lines() {
            // EPD carries operations after a ';' and sometimes after the
            // move-number fields; take the board part and let from_fen default
            // the rest.
            let fen = linha.split(';').next().unwrap_or("").trim();
            if fen.is_empty() || fen.starts_with('#') {
                continue;
            }
            let board = Board::from_fen(fen);
            // A FEN that failed to parse leaves an empty board, which would
            // otherwise enter the fit as a row of zeros and quietly drag every
            // coefficient toward it.
            if board.occ_all.count_ones() < 2 {
                saltadas += 1;
                continue;
            }
            for v in nnue::activacoes_saida(net, &board) {
                bin.extend_from_slice(&v.to_le_bytes());
            }
            bin.extend_from_slice(&nnue::evaluate_board(net, &board).to_le_bytes());
            fens_usadas.push_str(fen);
            fens_usadas.push('\n');
            n += 1;
        }
        if let Err(e) = std::fs::write(&args[3], &bin) {
            eprintln!("nao consegui escrever {}: {e}", args[3]);
            return;
        }
        let fens_path = format!("{}.fens", args[3]);
        if let Err(e) = std::fs::write(&fens_path, &fens_usadas) {
            eprintln!("nao consegui escrever {fens_path}: {e}");
            return;
        }
        println!(
            "accdump: {} posicoes ({} saltadas), {} valores por posicao + score -> {} ({:.1} MB) e {}",
            n,
            saltadas,
            2 * nnue::HIDDEN,
            args[3],
            bin.len() as f64 / 1e6,
            fens_path
        );
        return;
    }
    let mut engine = uci::Engine::new();
    engine.run();
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
            thread_idx: 0,
            root_side: board.side,
            stop_flag: &crate::search::NO_STOP,
            asp_re: 0,
                asp_nos: 0,
                cut_nodes: 0,
            cut_first: 0,
            cut_idx: [0; 17],
            cut_noisy: 0,
            cut_etapa: [0; 7],
            tt_nos: 0,
            tt_com_lance: 0,
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
            atk,
            zob,
            tt: &tt,
            nodes: 0,
            limits: SearchLimits { deadline: None, max_depth: 64, max_nodes: Some(node_limit), soft_budget: None },
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
            ply_reducao: [0; crate::search::MAX_PLY],
            root_best: None,
                        root_scores: Vec::new(),
                        nmp_min_ply: 0,
            excluded_move: None,
            excluded_root_moves: vec![],
            style_book: None,
            root_move_nodes: Vec::new(),
            capture_history: [[[0; 6]; 6]; 2],
            dextensions: [0; MAX_PLY],
            cutoff_cnt: [0; MAX_PLY],
            ult_margem: [-1; MAX_PLY],
            ameacas_reduzidos: [0; 4],
            ameacas_bateram: [0; 4],
            cutcnt_reduzidos: [0; 4],
            cutcnt_bateram: [0; 4],
            subalfa_reduzidos: [0; 4],
            subalfa_bateram: [0; 4],
            margem_reduzidos: [0; 4],
            margem_bateram: [0; 4],
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
            thread_idx: 0,
            root_side: board.side,
            stop_flag: &crate::search::NO_STOP,
            asp_re: 0,
                asp_nos: 0,
                cut_nodes: 0,
            cut_first: 0,
            cut_idx: [0; 17],
            cut_noisy: 0,
            cut_etapa: [0; 7],
            tt_nos: 0,
            tt_com_lance: 0,
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
            atk,
            zob,
            tt: &tt,
            nodes: 0,
            limits: SearchLimits {
                deadline: Some(move_t0 + Duration::from_millis(budget_ms)),
                max_depth: 64,
                max_nodes: None,
                soft_budget: None,
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
            ply_reducao: [0; crate::search::MAX_PLY],
            root_best: None,
                        root_scores: Vec::new(),
                        nmp_min_ply: 0,
            excluded_move: None,
            excluded_root_moves: vec![],
            style_book: None,
            root_move_nodes: Vec::new(),
            capture_history: [[[0; 6]; 6]; 2],
            dextensions: [0; MAX_PLY],
            cutoff_cnt: [0; MAX_PLY],
            ult_margem: [-1; MAX_PLY],
            ameacas_reduzidos: [0; 4],
            ameacas_bateram: [0; 4],
            cutcnt_reduzidos: [0; 4],
            cutcnt_bateram: [0; 4],
            subalfa_reduzidos: [0; 4],
            subalfa_bateram: [0; 4],
            margem_reduzidos: [0; 4],
            margem_bateram: [0; 4],
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

/// Full-eval streaming logistic tuner: same streaming/sparse-cache/
/// thread::scope/sigmoid/fit-K mechanics as `tune_stream` above, but
/// (removido com o HCE)
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




/// Logistic eval tuning: coordinate descent on `Weights::to_vec()`'s flat
/// parameter vector, minimizing squared error between the sigmoid of
/// each position's static eval and the REAL game result it came from
/// (1.0/0.5/0.0 from White's perspective). Classic method (no autodiff
/// needed): for each parameter, try +step and
/// -step, keep whichever reduces total error over the whole dataset,
/// else leave it unchanged. Dataset format: one line per position,
/// "<FEN>\t<white_score>".

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

/// Etiquetar posicoes com o que a NOSSA busca ve a profundidade fixa.
///
/// Porque isto e' diferente de treinar contra a avaliacao de outro motor:
/// as margens de poda, o desprezo pelo empate e os limiares desta busca estao
/// todos calibrados para a escala da avaliacao que ela usa. Ensinar a
/// avaliacao a imitar os numeros de outro motor da' uma que ve melhor e joga
/// pior -- medido, -65.4 Elo em 629 jogos com a receita ja' corrigida.
///
/// A etiqueta daqui esta' na nossa escala por construcao. E o que a avaliacao
/// estatica aprende e' a antecipar o que a busca encontraria mais fundo, que
/// e' exactamente o que faz um motor jogar bem sem gastar relogio.
///
/// Iterativo por natureza: cada versao melhor produz etiquetas melhores.

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

/// Calibrate the king-safety weights, the one block the regression tuner
/// cannot reach.
///
/// Every king-safety field feeds a single danger curve, so a one-unit probe
/// of any of them measures a slope on that curve rather than the field's own
/// contribution -- which is why `tune_fast` holds them all fixed and why they
/// have only ever been set by hand. This optimiser does not linearise
/// anything: it calls the real evaluation with candidate weights and keeps
/// whatever lowers the error. Slower per step, and indifferent to the shape
/// of the function underneath.
///
/// Coordinate descent with a shrinking step. With ~46 parameters that is a
/// few thousand full passes over the sample, which is minutes rather than
/// hours at this size, and it cannot diverge the way a gradient step through
/// a curve of unknown slope can.
///
/// K is measured for THIS pair of data and model and then held. Fitting K
/// alongside the weights lets the optimiser lower the error by flattening the
/// sigmoid instead of improving the evaluation -- an error already made twice
/// on this project, once by fitting it during training and once by reusing a
/// K measured against different targets.


/// Positions spanning the phases the search behaves differently in: opening,
/// sharp middlegame, quiet middlegame, and endgames with and without pawns.
/// A benchmark drawn from one phase would miss a change that only affects
/// another, which for a signature is worse than useless.
/// Bench positions, drawn from the bot's own games by `build_bench_fens.py`
/// and spread evenly across opening, middlegame and endgame by piece count.
///
/// Twelve positions were too few for either job the bench does. As a stability
/// metric one unusual tree moved the total enough to read as a change; as a
/// signature -- this binary visits exactly this many nodes -- a small set makes
/// collisions between different builds likelier. Fifty-one, from real games,
/// gives a number that moves when the search moves and not otherwise.
const BENCH_FENS: [&str; 51] = [
    "8/1p5p/2nk1B2/p1nr4/4r3/PP3R1P/1R2P1K1/8 b - - 3 36",
    "r1bq1rk1/2p2ppp/2n1p3/P1bp4/4n3/4PN2/P2BNPPP/R2QKB1R w KQ - 6 12",
    "8/1p2k1K1/p1p5/P3P3/1P1P4/4B3/b7/8 b - - 50 71",
    "6k1/R7/2b5/7P/3B4/6P1/7q/4K3 w - - 2 47",
    "Bn2r3/p2Q2pp/5p1k/8/2p4N/Bn4P1/4PP1P/R4RK1 b - - 0 15",
    "r2q3r/pkpnb1p1/3pn1B1/1p2p1P1/QN2P3/2PPB3/PP4PP/R4RK1 w - - 2 19",
    "r2q3r/ppp2k1p/2nbb1p1/8/2Q5/2NP4/PPP2PPP/R1B1KB1R w KQ - 5 12",
    "r3rnk1/1b2qppp/p3p3/1p1pP3/2nP4/P1NB1NP1/1PQ2P1P/3RR1K1 w - - 1 19",
    "rnbqkb1r/2p2ppp/4pn2/P1Pp4/8/4PN2/P4PPP/RNBQKB1R b KQkq - 0 8",
    "rnb1qrk1/ppp1p1bn/3pP1p1/5p1p/3P3N/2PB3P/PP3PP1/RNBQR1K1 w - - 2 12",
    "1r1r2k1/pb3p2/1pp3pp/8/3PP3/2R2N2/P4PPP/4R1K1 b - - 3 22",
    "rnq1nrk1/1b2ppb1/p2p3B/1PpP2Q1/P3P1pp/2N2P2/1P3NPP/R3KB1R w KQ - 6 12",
    "r1bq1rk1/ppp1ppb1/2np1np1/7p/3PP3/2PB1N1P/PP3PP1/RNBQR1K1 b - - 0 8",
    "2r1r3/1pp2pkp/p2p2p1/3Pq3/nP1R3P/3Q2P1/P1P2PB1/1R4K1 b - - 2 22",
    "8/8/p6P/P7/1k3p2/2r5/1q6/3K4 b - - 3 64",
    "8/8/8/3pk3/8/3K1P2/8/8 b - - 1 64",
    "8/8/7R/8/8/1P5P/Pk3PP1/6K1 w - - 1 40",
    "5rk1/3b1ppp/2p1r3/1p1P4/1p6/5B2/1P1K1PPP/R6R b - - 0 22",
    "4k3/8/5Q1p/8/4N3/1pP4P/1qbK1PP1/8 w - - 7 54",
    "r1bq1rk1/pp2bppp/2p2n2/n2p4/3P4/P1N1PNB1/1PQ2PPP/R3KB1R w KQ - 0 12",
    "8/4Q3/7P/6PK/4n3/4q3/1k6/8 w - - 13 89",
    "rnb1kb1r/ppp1qppp/8/8/3pB3/8/PPP2PPP/RNBQR1K1 b kq - 1 8",
    "r3kb1r/pp1npppp/2P1bn2/1B4B1/4p3/2N5/PPP2PPP/R2K2NR b kq - 0 8",
    "6k1/1p4p1/8/2P2p2/1P3P2/2Q3K1/4p2P/3q4 b - - 1 36",
    "6k1/1p2Bppp/4p3/1p2P3/6P1/P4b1P/4rP2/6K1 w - - 2 40",
    "6n1/2p2k1p/pp3p2/3P2p1/2P5/2B1N3/PP3P2/7K b - - 3 22",
    "6b1/4p3/8/6p1/K1k2P2/6p1/1q6/8 b - - 0 43",
    "rnb1kb1r/2p1pppp/pq3n2/1p4N1/3P4/2N5/PPP1BPPP/R1BQ1RK1 b kq - 3 8",
    "r1bq1rk1/pp2ppbp/2n2np1/1BPp4/5B2/2N1PN2/PPP2PPP/R2Q1RK1 b - - 4 8",
    "r1b1r1k1/pp3pp1/2nb1q1p/8/3pn3/P4QP1/1PPP2KP/RNB2BNR b - - 3 15",
    "rq3rk1/1p2b1p1/pNnpb2p/4pp2/2P1P3/1P2BN1P/P3QPP1/R2R2K1 w - - 0 12",
    "6n1/5pk1/4p3/1b1pP1K1/3P2P1/7q/Q4P2/8 b - - 0 43",
    "rnbqk2r/pp2ppbp/3p2p1/2p5/3P4/2PBPNP1/PP3PP1/RN1QK2R b KQkq - 0 8",
    "8/8/4prp1/2Q4p/4RP2/3p3P/3k2PK/q7 w - - 4 54",
    "8/p6k/1p4qp/3P4/2P1n2P/b7/4Q1P1/4B2K b - - 0 36",
    "5rk1/p4ppp/3b4/8/Br2P3/4n3/P1P5/3R1R1K w - - 6 26",
    "2r3k1/1Q3pp1/p6p/1p2p3/4p1b1/P1P1PqP1/5P1P/1R4K1 b - - 3 36",
    "8/7p/5p1k/1N3P2/8/6PP/r6r/3RK3 w - - 11 47",
    "2r5/4k1pp/pB2pp2/1p1P4/1P3nbR/2PB1q2/P2K1P1P/R7 w - - 1 26",
    "3rn1k1/p3p1bp/P1R1p1p1/1P2p3/2P1P2P/3q1PP1/2Q2BK1/8 w - - 0 40",
    "r4r1k/p2P2p1/2Q4p/1P2Q3/2B2p2/4R2P/5PP1/7K b - - 1 43",
    "8/4K3/5pk1/5R2/6P1/5P2/8/8 w - - 9 82",
    "r2k1n2/ppp2p1p/5bpB/3b4/3P1N2/P7/1P1K1PPP/4R3 w - - 0 12",
    "8/4p2k/3p1p2/2pPP3/8/4n1PP/P2q4/6K1 b - - 0 36",
    "8/2R5/p7/Pk5P/1p3p2/2p2K2/8/7r b - - 1 57",
    "r1bqk2r/pppp1ppp/8/4p3/2PnP1Q1/P7/1P3PPP/RNB1KB1R b KQkq - 0 8",
    "8/1p6/pB3k2/2P5/4ppp1/1p1r4/1R6/4K3 b - - 1 57",
    "3r4/5k2/R4p2/1p6/2b1B1P1/1p2KP2/1P6/8 w - - 1 47",
    "r3r1k1/1p1bppbn/1q1pP2p/1NpP2pB/1nP2P2/P1B4P/1Q1N2P1/R4RK1 b - - 0 15",
    "2k4r/p4p2/3QpN2/3pP2p/1P1q4/8/P4PP1/2n2K2 w - - 4 40",
    "r1b1k2r/ppp2pp1/2np1q1p/2bNp3/2B1P3/3P1N2/PPP2PPP/R2QK2R b KQkq - 1 8",
];

fn bench(depth: i32) {
    let atk = Attacks::new();
    let zob = zobrist::Zobrist::new();
    let tt = tt::TranspositionTable::new(std::env::var("KESTREL_BENCH_HASH").ok().and_then(|v| v.parse().ok()).unwrap_or(16));
    evaluation::warmup();
    search::warmup();
    // Carregar a rede ANTES de arrancar o cronometro.
    //
    // Ela carrega preguicosamente, na primeira avaliacao -- que acontece dentro
    // do ciclo. Media: 1.02 G ciclos de 16.39, ou seja **6.2% do bench eram
    // descodificar 95 MB de LEB**, contados como se fossem busca. Todos os
    // numeros de nps deste motor sairam 6% abaixo do que a busca faz mesmo, e a
    // comparacao com outro motor saia pior ainda: o bench do Stockfish gasta 29%
    // do tempo a carregar a mesma rede do disco, portanto os dois numeros
    // estavam contaminados em proporcoes diferentes e o racio entre eles nao
    // media coisa nenhuma.
    let _ = crate::nnue_sf::rede();
    let start = std::time::Instant::now();
    let mut total: u64 = 0;
    let (mut c_rfp, mut c_razor, mut c_fut, mut c_nmp, mut c_q) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut c_cut, mut c_1st, mut c_noisy) = (0u64, 0u64, 0u64);
    let mut c_idx = [0u64; 17];
    let mut c_et = [0u64; 7];
    let (mut t_nos, mut t_lance) = (0u64, 0u64);
    let (mut n_tried, mut n_raw, mut n_vt, mut n_vf, mut n_low) = (0u64, 0u64, 0u64, 0u64, 0u64);
    for fen in BENCH_FENS.iter() {
        let mut board = Board::from_fen(fen);
        tt.clear();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let mut searcher = search::Searcher {
            thread_idx: 0,
            root_side: board.side,
            stop_flag: &stop,
            asp_re: 0,
                asp_nos: 0,
                cut_nodes: 0,
            cut_first: 0,
            cut_idx: [0; 17],
            cut_noisy: 0,
            cut_etapa: [0; 7],
            tt_nos: 0,
            tt_com_lance: 0,
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
            atk: &atk,
            zob: &zob,
            tt: &tt,
            nodes: 0,
            limits: search::SearchLimits { deadline: None, max_depth: depth, max_nodes: None, soft_budget: None },
            stop: false,
            history: Vec::new(),
            killers: [[None; 2]; search::MAX_PLY],
            history_scores: [[[0; 64]; 64]; 2],
            countermoves: [[None; 64]; 6],
            cont_hist: vec![0i32; search::CONT_HIST_SIZE].into_boxed_slice(),
            corr_hist: vec![0i32; search::CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_np_stm: vec![0i32; search::CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_np_nstm: vec![0i32; search::CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_minor: vec![0i32; search::CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_major: vec![0i32; search::CORR_HIST_SIZE * 2].into_boxed_slice(),
            corr_hist_threats: vec![0i32; search::CORR_HIST_SIZE * 2].into_boxed_slice(),
            ply_last_move: [None; search::MAX_PLY],
            static_evals: [0i32; search::MAX_PLY],
            ply_reducao: [0; crate::search::MAX_PLY],
            root_best: None,
                        root_scores: Vec::new(),
                        nmp_min_ply: 0,
            excluded_move: None,
            excluded_root_moves: vec![],
            style_book: None,
            root_move_nodes: Vec::new(),
            capture_history: [[[0; 6]; 6]; 2],
            dextensions: [0; search::MAX_PLY],
            cutoff_cnt: [0; search::MAX_PLY],
            ult_margem: [-1; search::MAX_PLY],
            ameacas_reduzidos: [0; 4],
            ameacas_bateram: [0; 4],
            cutcnt_reduzidos: [0; 4],
            cutcnt_bateram: [0; 4],
            subalfa_reduzidos: [0; 4],
            subalfa_bateram: [0; 4],
            margem_reduzidos: [0; 4],
            margem_bateram: [0; 4],
            report: false, // offline tools: no UCI narration
        };
        let (_, _, _, nodes) = searcher.iterative_deepening(&mut board);
        total += nodes;
        c_rfp += searcher.cut_rfp;
        c_razor += searcher.cut_razor;
        c_fut += searcher.cut_futility;
        c_nmp += searcher.nmp_cut_taken;
        n_tried += searcher.nmp_tried;
        n_raw += searcher.nmp_cutoff_raw;
        n_vt += searcher.nmp_verify_tried;
        n_vf += searcher.nmp_verify_failed;
        n_low += searcher.nmp_failed_low;
        c_q += searcher.qnodes;
        c_cut += searcher.cut_nodes;
        c_1st += searcher.cut_first;
        c_noisy += searcher.cut_noisy;
        for k in 0..7 { c_et[k] += searcher.cut_etapa[k]; }
        t_nos += searcher.tt_nos;
        t_lance += searcher.tt_com_lance;
        for k in 0..17 { c_idx[k] += searcher.cut_idx[k]; }
    }
    let ms = start.elapsed().as_millis().max(1) as u64;
    println!("{} nodes {} nps", total, total * 1000 / ms);
    if std::env::var("KESTREL_CONTA_FEATS").as_deref() == Ok("1") {
        println!("{}", crate::nnue_sf::relatorio_feats());
    }
    if std::env::var("KESTREL_HISTO_AMEACAS").as_deref() == Ok("1") {
        print!("{}", crate::nnue_sf::relatorio_histo());
    }
    if std::env::var_os("KESTREL_CORTES").is_some() {
        let p = |n: u64| 100.0 * n as f64 / total as f64;
        eprintln!(
            "cortes por 100 nos: rfp {:.2} razor {:.2} futility {:.2} nmp {:.2} | quiescencia {:.1}%",
            p(c_rfp), p(c_razor), p(c_fut), p(c_nmp), p(c_q)
        );
        eprintln!("brutos: rfp {c_rfp} razor {c_razor} futility {c_fut} nmp {c_nmp} qnodes {c_q}");
        eprintln!(
            "null move: {n_tried} tentados -> {n_raw} deram corte -> {n_vt} verificados \
             ({n_vf} falharam) -> {c_nmp} usados; {n_low} falharam baixo"
        );
        // A percentagem de cortes produzidos pelo PRIMEIRO lance tentado e' o
        // numero que decide a largura da arvore: um corte que so' chega ao
        // quinto lance ja' pagou quatro subarvores que ninguem queria. Uma
        // busca com boa ordenacao anda nos 90%; abaixo disso o problema nao e'
        // a poda, e' a ordem por que os lances sao tentados.
        if c_cut > 0 {
            eprintln!(
                "ordenacao: {c_1st} de {c_cut} cortes vieram do 1o lance -- {:.1}%; \
                 {:.1}% dos cortes foram capturas",
                100.0 * c_1st as f64 / c_cut as f64,
                100.0 * c_noisy as f64 / c_cut as f64
            );
            let acum: Vec<String> = (0..8)
                .map(|k| format!("{k}:{:.1}%", 100.0 * c_idx[k] as f64 / c_cut as f64))
                .collect();
            eprintln!(
                "indice do lance que cortou: {} ... 16+:{:.1}%",
                acum.join(" "),
                100.0 * c_idx[16] as f64 / c_cut as f64
            );
            let nomes = ["-", "tabela", "boa captura", "killer1", "killer2", "quieto", "ma captura"];
            let et: Vec<String> = (0..7)
                .filter(|k| c_et[*k] > 0)
                .map(|k| format!("{}:{:.1}%", nomes[k], 100.0 * c_et[k] as f64 / c_cut as f64))
                .collect();
            eprintln!("etapa que produziu o corte: {}", et.join("  "));
            if t_nos > 0 {
                eprintln!(
                    "tabela: {t_lance} de {t_nos} nos interiores trouxeram lance -- {:.1}%",
                    100.0 * t_lance as f64 / t_nos as f64
                );
            }
        }
    }
}

/// Diagnostic: is the king accumulator linear in the king weights?
/// `kinglin <fen>` probes each king field at +1 and +2 and prints the two
/// deltas. Linear means they are exactly x_i and 2*x_i.


/// `FEN<TAB>result` in, `FEN | score | result` out -- the text form the
/// network trainer converts from.
///
/// Both numbers are white-relative, which is the trainer's convention and NOT
/// the engine's: our search returns a score for the side to move, so it is
/// negated on black's turn. Getting that backwards trains the network to
/// evaluate half the positions upside down, and nothing downstream would say
/// so -- the loss simply stops falling.
fn bullet_data(in_path: &str, out_path: &str, depth: i32, threads: usize) {
    use crate::search::{MATE_SCORE, MAX_PLY};
    let texto = std::fs::read_to_string(in_path).expect("nao consegui ler o dataset");
    let linhas: Vec<&str> = texto.lines().filter(|l| !l.trim().is_empty()).collect();
    println!("bulletdata: {} posicoes, profundidade {}, {} threads", linhas.len(), depth, threads);

    let atk = Attacks::new();
    let zob = zobrist::Zobrist::new();
    let feito = std::sync::atomic::AtomicUsize::new(0);
    let t0 = std::time::Instant::now();
    let chunk = linhas.len().div_ceil(threads.max(1));

    let saidas: Vec<String> = std::thread::scope(|scope| {
        let hs: Vec<_> = linhas
            .chunks(chunk.max(1))
            .map(|parte| {
                let atk = &atk;
                let zob = &zob;
                let feito = &feito;
                scope.spawn(move || {
                    let tt = tt::TranspositionTable::new(std::env::var("KESTREL_BENCH_HASH").ok().and_then(|v| v.parse().ok()).unwrap_or(16));
                    let mut out = String::new();
                    for linha in parte {
                        let mut it = linha.split('\t');
                        let fen = match it.next() { Some(f) => f, None => continue };
                        // Only true game results. Our previous dataset had
                        // values like 0.826712 sitting in this column -- a
                        // search score run through a sigmoid, wearing a
                        // result's clothes. Anything that is not a result is
                        // dropped rather than trusted.
                        let res: f32 = match it.next().and_then(|r| r.trim().parse().ok()) {
                            Some(r) => r,
                            None => continue,
                        };
                        if res != 0.0 && res != 0.5 && res != 1.0 { continue; }

                        let mut board = Board::from_fen(fen);
                        let mut s = novo_searcher_raso(atk, zob, &tt, depth);
                        let (_mv, score, _d, _n) = s.iterative_deepening(&mut board);
                        // A position already decided tactically teaches a
                        // static evaluator nothing.
                        if score.abs() >= MATE_SCORE - MAX_PLY as i32 { continue; }
                        let branco = if board.side == crate::types::Color::White { score } else { -score };

                        out.push_str(fen);
                        out.push_str(" | ");
                        out.push_str(&branco.to_string());
                        out.push_str(" | ");
                        out.push_str(&format!("{:.1}", res));
                        out.push('\n');

                        let n = feito.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        if n % 100_000 == 0 {
                            println!("  {} posicoes, {:.0}s", n, t0.elapsed().as_secs_f64());
                        }
                    }
                    out
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });

    let junto: String = saidas.concat();
    std::fs::write(out_path, &junto).expect("nao consegui escrever");
    println!(
        "bulletdata: {} linhas em {:.0}s -> {}",
        junto.lines().count(),
        t0.elapsed().as_secs_f64(),
        out_path
    );
}

fn novo_searcher_raso<'a>(
    atk: &'a Attacks,
    zob: &'a zobrist::Zobrist,
    tt: &'a tt::TranspositionTable,
    depth: i32,
) -> search::Searcher<'a> {
    use crate::search::{SearchLimits, CONT_HIST_SIZE, CORR_HIST_SIZE, MAX_PLY};
    search::Searcher {
        thread_idx: 0,
        root_side: crate::types::Color::White,
        stop_flag: &crate::search::NO_STOP,
        asp_re: 0, asp_nos: 0,
        cut_nodes: 0, cut_first: 0, cut_idx: [0; 17], cut_noisy: 0, cut_etapa: [0; 7],
            tt_nos: 0,
            tt_com_lance: 0, nmp_tried: 0, nmp_tried_pv: 0,
        nmp_failed_pv: 0, nmp_cutoff_raw: 0, nmp_cut_taken: 0,
        nmp_verify_tried: 0, nmp_verify_ok: 0, nmp_verify_failed: 0,
        nmp_failed_low: 0, qnodes: 0, cut_rfp: 0, cut_razor: 0,
        cut_futility: 0, nodes_shallow: 0, lmr_quiet_total: 0,
        lmr_skip_check: 0, lmr_skip_depth: 0, lmr_skip_extend: 0,
        lmr_skip_early: 0, lmr_tried: 0, lmr_research: 0, lmr_sum: 0,
        atk, zob, tt, nodes: 0,
        limits: SearchLimits { deadline: None, max_depth: depth, max_nodes: None, soft_budget: None },
        stop: false, history: Vec::new(), killers: [[None; 2]; MAX_PLY],
        history_scores: [[[0; 64]; 64]; 2], countermoves: [[None; 64]; 6],
        cont_hist: vec![0i32; CONT_HIST_SIZE].into_boxed_slice(),
        corr_hist: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
        corr_hist_np_stm: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
        corr_hist_np_nstm: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
        corr_hist_minor: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
        corr_hist_major: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
        corr_hist_threats: vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice(),
        ply_last_move: [None; MAX_PLY], static_evals: [0i32; MAX_PLY],
        root_best: None, root_scores: Vec::new(), nmp_min_ply: 0,
        excluded_move: None, excluded_root_moves: vec![], style_book: None,
        root_move_nodes: Vec::new(), capture_history: [[[0; 6]; 6]; 2],
        dextensions: [0; MAX_PLY], cutoff_cnt: [0; MAX_PLY], ult_margem: [-1; MAX_PLY], ameacas_reduzidos: [0; 4], ameacas_bateram: [0; 4], cutcnt_reduzidos: [0; 4], cutcnt_bateram: [0; 4], subalfa_reduzidos: [0; 4], subalfa_bateram: [0; 4], margem_reduzidos: [0; 4], margem_bateram: [0; 4], report: false,
        ply_reducao: [0; crate::search::MAX_PLY],
    }
}
