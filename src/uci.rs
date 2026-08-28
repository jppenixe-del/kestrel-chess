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
const MOVE_OVERHEAD_DEFAULT_MS: i64 = 150;

/// What to hold back for everything that is not thinking, in milliseconds.
///
/// Settable, because 150 is right for one opponent and wrong for another and
/// there is no single value that is right for both. Against people and bots the
/// move POST costs ~50ms here and 150 covers it three times over. Against the
/// server's own engine the same POST is measured at **560ms** -- not our
/// network, which does a full TLS handshake and GET to the site in 46ms, but
/// the path on the far side. At 150 the engine believes it has half a second
/// per move that it does not have, and a bullet game is thirty of those: it
/// flags with the clock reading positive and the search never at fault.
///
/// The note this replaces recorded that a move-overhead change had been tried
/// and had not fixed the flags, "validated against an unrealistic 300ms
/// latency". The latency is 560ms. The premise was wrong, not the idea.
///
/// A larger reserve is not free -- 250 measurably lost non-flag games in
/// self-play by thinking too little -- so the client measures its own POSTs and
/// sets this, rather than anyone picking a number that covers the worst case
/// for every opponent.
static MOVE_OVERHEAD: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(MOVE_OVERHEAD_DEFAULT_MS);

fn move_overhead_ms() -> i64 {
    MOVE_OVERHEAD.load(std::sync::atomic::Ordering::Relaxed)
}
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

/// Multiplicador do orcamento por fase do jogo, em percentagem. Portado do
/// Pond (src/search/pond/search.cc), onde foi calibrado em jogos reais a
/// 60+0.
///
/// A nossa fatia era 1/55 do relogio a cada lance, o que e' geometricamente
/// PESADO no inicio -- medido nos nossos jogos de blitz: 4 segundos por lance
/// entre o lance 10 e o 25, cinquenta e seis segundos de cento e oitenta em
/// dezasseis lances, com o adversario a ir a um segundo. Ao lance 40
/// estavamos cinco segundos atras e a jogar a correr.
///
/// A abertura leva 30%: e' onde pensar rende menos, porque ha muitos lances
/// razoaveis e o livro cobre boa parte. O meio-jogo a serio leva 110%, que e'
/// onde se decidem os jogos. No Pond este valor esteve em 160% e foi BAIXADO
/// para 110% depois de um jogo real em que doze segundos de sessenta se foram
/// em tres lances desta fase.
const FASE_ABERTURA_PCT: i64 = 30;    // ply < 12
const FASE_MEIO_CEDO_PCT: i64 = 70;   // ply < 24
const FASE_MEIO_PCT: i64 = 110;       // ply < 45
const FASE_MEIO_TARDE_PCT: i64 = 100; // ply < 65
const FASE_SIMPLIFICADO_PCT: i64 = 60; // <= 10 pecas, ja' fora das fases acima

/// Pressao pelo relogio. Com folga confortavel investe-se mais; atras, menos.
/// Reage a' saude geral do relogio, ao contrario do ritmo do adversario, que
/// reage ao curto prazo.
const PRESSAO_FOLGADO_PCT: i64 = 115;  // acima de 1.5x o relogio dele
const PRESSAO_APERTADO_PCT: i64 = 85;  // abaixo de 0.7x

/// Modo predador: podemos pensar ate' esta percentagem do que ele pensa.
///
/// E' um PISO sobre o que ja' esta' orcamentado, nao um tecto. Nunca pensar
/// muito mais devagar do que um adversario que pensa muito -- mas por ser 95 e
/// nao 100, se ambos formos ao limite ganhamos relogio a cada lance. Como
/// tecto absoluto apagava o investimento do meio-jogo sempre que ele
/// respondesse depressa, mesmo com relogio de sobra do nosso lado.
const PREDADOR_PCT: i64 = 95;

/// O mesmo quando temos o relogio CONFORTAVELMENTE a nosso favor.
///
/// O 95 e' para recuperar: gastar menos do que ele para o relogio voltar a
/// nosso lado. Mas aplicado tambem com folga era conservador de mais -- com
/// vantagem de relogio o que se deve fazer e' usa-la, e nao ficar a poupar
/// tempo que ja' sobra. Aqui podemos pensar mais do que ele, com margem.
const PREDADOR_FOLGADO_PCT: i64 = 130;

/// A partir de que vantagem, em decimos do relogio dele, se considera folga.
/// 13 = 1.3x o relogio dele.
const PREDADOR_FOLGA_R10: i64 = 13;

/// O outro lado do ritmo: quanto podemos pensar ACIMA do ritmo dele quando
/// NAO temos folga de relogio, em percentagem do ritmo dele.
///
/// A regra do predador acima e' so' um PISO -- acompanha um adversario lento e
/// nao faz nada contra um rapido. Medido em quatro partidas de blitz reais:
/// gastamos 15-25% mais por lance do que o adversario e acabamos com muito
/// menos relogio (21s contra 53s num 180+0), que e' onde aparecem os erros e
/// as bandeiras.
///
/// Um tecto absoluto ja' foi tentado e era mau -- ver PREDADOR_PCT: apagava o
/// investimento do meio-jogo sempre que ele respondesse depressa. A diferenca
/// aqui e' o `folgado`: com o relogio a nosso favor nao se aplica nada e
/// investe-se a vontade; so' quando estamos a par ou atras e' que se recusa a
/// pensar varias vezes o que ele pensa.
///
/// 250 = podemos gastar 2.5x o ritmo dele. Nao corta o lance normal (andamos
/// nos 1.25x); corta os picos -- sete segundos num lance contra um adversario
/// que responde em 1.4s.
///
/// NAO E' UM VALOR AFINADO. Entra desligado (`RitmoTecto` a 0) precisamente
/// para poder ser medido por SPRT antes de contar para alguma coisa, um valor
/// de cada vez, como o HARD_CAP_BUDGET_MULT exige de si proprio.
const TECTO_RITMO_PCT_OMISSAO: i64 = 0;
static TECTO_RITMO: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(TECTO_RITMO_PCT_OMISSAO);

pub fn tecto_ritmo() -> i64 {
    TECTO_RITMO.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_tecto_ritmo(v: i64) {
    TECTO_RITMO.store(v.clamp(0, 1000), std::sync::atomic::Ordering::Relaxed);
}
/// The hard ceiling, as a percentage of the remaining clock. Guards the game
/// as a whole; on a healthy clock the ceiling below binds long before it.
/// Corrigido de 45 para 25: o tecto real nunca passava dos 25%.
///
/// A 45 este parametro era uma fraude analitica. O minimo do horizonte e' 12 e
/// o multiplicador de emergencia vai no maximo a 30 decimos, portanto o melhor
/// caso possivel era (safe/12)*3 = 25% -- e no meio-jogo, com o horizonte no
/// tecto de 42 e o multiplicador limitado pela fase, o real anda nos 3.6-4.8%.
/// Medido com a linha de debug do proprio motor.
///
/// Quem baixasse isto de 45 para 35 a tentar ser conservador nao mudava nada, e
/// nao teria como saber. Custou-me hoje uma alteracao inteira que ficou inerte.
///
/// Nao se sobe. Um tecto de 45% deixaria um unico lance comer vinte e cinco
/// lances normais, com o `soft` a 1/55 do relogio -- e' a receita para perder
/// por bandeira com a posicao ganha, que foi o defeito que custou uma manha a
/// corrigir. Se 4x o orcamento normal for apertado, mede-se em
/// HARD_CAP_BUDGET_MULT com SPRT, um valor de cada vez.
const HARD_CAP_PERCENT: i64 = 25;
/// The hard ceiling as a multiple of the base slice, in tenths. Sits just
/// above the largest scaling the search can award itself, so a move that has
/// earned every extension still gets cut before it can overshoot.
///
/// Raised from 25 (2.5x) once two real losses were traced to moves played
/// fast with the clock full: mated on move 37 with 68.6s of 180 unspent, and
/// on move 61 having spent 1.04s of an available 29.7s on the move that lost
/// the game -- a move three seconds of thought does not play. A ceiling this
/// close to the base allowance does not merely cut long searches, it stops
/// the search from ever asking for time, because there is none to ask for.
/// The percentage of the remaining clock below is what actually guards the
/// game, and this now sits above the search's own ceiling rather than under
/// it.
const HARD_CAP_BUDGET_MULT: i64 = 40;
/// Ceiling on how many ordinary moves a critical one may be worth, in tenths.
/// Reached on a comfortable clock and given up as the clock runs down: the
/// multiplier is the remaining clock in units of two seconds, floored at 1.0x
/// so a short clock buys no long thinks at all.
const EMERGENCY_MULT_MAX: i64 = 30;

const OPENING_PLIES: i64 = 16;
/// Ceiling on a single opening move once the book has no answer. Small on
/// purpose: the phase is theory, and the clock it saves is spent where the
/// position is actually unique.
const OPENING_MAX_MS: i64 = 70;


/// Gestao de tempo em 4 niveis -- a mesma arquitetura em camadas que
/// validamos esta sessao no Pond (jogos reais, derrotas por bandeira
/// investigadas e corrigidas uma a uma): formula elastica normal, corte
/// para relogio baixo sem vantagem clara, modo panico, zona da morte.
/// `last_score` e' o score (cp, da nossa perspetiva) do ultimo "go" --
/// None no 1o lance do jogo. Sem isto so' havia 2 niveis (normal +
/// panico), sem distinguir "estamos a perder" de "estamos a ganhar" --
/// o bug exato que causou uma derrota real por bandeira no Pond antes de
/// ser corrigido (relaxava o corte tambem quando estavamos a perder).
/// Play straight from the evaluation, no search at all. Set with
/// `KESTREL_HEATMAP_ONLY=1` or `setoption name HeatmapOnly value true`.
static HEATMAP_ONLY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What a position with no legal moves is worth, from the side to move.
///
/// The evaluation cannot answer this and does not try: it counts material, and
/// a stalemated position is usually one where we have plenty. Two of the first
/// seven games played from the evaluation alone ended in stalemate with a won
/// position, because every move was scored by material and the one that ended
/// the game scored best of all. Mate and stalemate are facts about the move
/// list, not about the pieces, and they have to be checked where the move list
/// is generated.
fn terminal_value(board: &Board, atk: &crate::attacks::Attacks) -> i32 {
    if board.in_check(board.side, atk) {
        // Mated: the worst thing that can happen to the side to move.
        -30000
    } else {
        // Stalemate is a draw however much material is on the board.
        0
    }
}

fn heatmap_only() -> bool {
    HEATMAP_ONLY.load(std::sync::atomic::Ordering::Relaxed)
}

/// 1 = only our move; 2 = his best reply too.
///
/// 3 exists and is MEASURABLY TERRIBLE -- kept only because the measurement is
/// worth more than the mode. See the note on threats below.
static HEATMAP_PLIES: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);

fn heatmap_plies() -> i32 {
    HEATMAP_PLIES.load(std::sync::atomic::Ordering::Relaxed)
}

/// What the increment pays for, which is time that costs nothing to spend.
///
/// The increment is income: whatever is spent up to it comes back, so the clock
/// ends the move where it started. But it does not all come back, because the
/// move has to REACH the server -- and that leg is billed to us. Measured from
/// here: ~50ms against people and bots, and 560ms against the server's own
/// engine, which is why the reserve is the measured one and not a guess.
///
/// Subtracting it first is what makes this safe for SMALL increments. Four
/// fifths of a 100ms increment is 80ms, and 80 spent plus 40 of lag against 100
/// received is a clock that drains a move at a time while the arithmetic looks
/// like it balances. With the reserve taken off the top first, a increment too
/// small to fund a think funds nothing and the burst tier takes over, which is
/// the right answer.
///
/// Capped at a third of what is on the clock as well: the increment arrives
/// AFTER the move, so spending it beforehand is spending money not yet paid.
fn increment_funded(my_inc: i64, safe_time: i64) -> i64 {
    if my_inc <= 0 {
        return 0;
    }
    let net = (my_inc - move_overhead_ms()).max(0);
    (net * 4 / 5).min(safe_time / 3).max(0)
}

fn compute_time_budget(
    my_time: i64,
    my_inc: i64,
    opp_time: i64,
    movestogo: Option<i64>,
    last_score: Option<i32>,
    pieces_left: i64,
    game_ply: i64,
    opp_pace: Option<i64>,
    relogio_inicial: Option<i64>,
) -> (i64, i64) {
    let safe_time = (my_time - move_overhead_ms()).max(1);

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
    let sp_t = crate::search::search_params();
    // `.max(1)`: a banda anunciada ao afinador chega a zero, e zero aqui e'
    // divisao por zero -- um panico que so' aparecia a meio de um jogo.
    let moves_left = movestogo.unwrap_or(sp_t.tempo_divisor as i64).max(1);
    let mut base = safe_time / moves_left + my_inc * 3 / 4;

    // Fase do jogo. A fatia constante do relogio e' geometricamente pesada no
    // inicio, que e' precisamente onde pensar rende menos. Ver as constantes.
    let fase = if game_ply < 12 {
        sp_t.tempo_fase_abertura as i64
    } else if game_ply < 24 {
        sp_t.tempo_fase_meio_cedo as i64
    } else if game_ply < 45 {
        sp_t.tempo_fase_meio as i64
    } else if game_ply < 65 {
        sp_t.tempo_fase_meio_tarde as i64
    } else if pieces_left <= 10 {
        sp_t.tempo_fase_simplificado as i64
    } else {
        100
    };
    base = base * fase / 100;

    // Pressao pelo relogio: com folga confortavel investe-se mais, atras
    // investe-se menos. Saude geral do relogio, nao ritmo do momento.
    if opp_time > 0 {
        let r10 = my_time * 10 / opp_time;
        let pressao = if r10 > 15 {
            PRESSAO_FOLGADO_PCT
        } else if r10 < 7 {
            PRESSAO_APERTADO_PCT
        } else {
            100
        };
        base = base * pressao / 100;
    }
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
    // How many of our moves the game still has in it, read off the material
    // rather than the move number. A position with eight pieces left has few
    // moves to go -- to mate, to a decided endgame, or to the point where the
    // tablebases answer outright -- while a full board has the whole game
    // ahead of it. The move number cannot tell those apart: move 40 of a
    // queenless rook ending and move 40 of a full-board middlegame have very
    // different amounts of chess remaining.
    //
    // This governs the EMERGENCY ceiling only. The base allowance keeps its
    // constant fraction, which decays with the clock instead of accelerating
    // against it -- an earlier attempt to drive the base allowance from an
    // estimate of moves remaining had the engine spending 8% of its clock per
    // move at move 34 and playing the last twenty at nothing. The ceiling is
    // a different question: it is not what a move costs, it is what a move is
    // ALLOWED to cost when the search says this one matters, and that should
    // grow as the remaining game shrinks.
    let horizon = (12 + (pieces_left - 2) * 7 / 5).clamp(12, 42);
    // What a critical move is allowed to be worth, in ordinary moves, and that
    // is a question about the clock as much as about the position. With a
    // healthy clock a move that decides the game can be worth three; with the
    // clock nearly out there is no such thing as an affordable long think, and
    // the same reasoning that justifies spending would spend the game away.
    // In tenths, so it moves continuously instead of in steps.
    // ...and about WHEN in the game it is being asked. A first real loss with
    // the raised ceiling was spent at the wrong end of the game: 73 seconds
    // went on moves 9 to 20, one of them 12s, and the phase that actually
    // decided the game -- moves 32 to 48, where an advantage of +1.6 eroded
    // into a lost position -- was played at 1.4 to 3s a move. Nothing had gone
    // wrong yet on move 11; the position was still theory-adjacent and the
    // alternatives still roughly equal. The permission to think long belongs
    // later, when the position is genuinely unique.
    let by_clock = (safe_time / 2000).clamp(10, EMERGENCY_MULT_MAX);
    // Grows slowly on purpose. At ply/2 this reached 2.5 of a possible 3.0 by
    // move 13, which is not "later in the game" by any reading -- a real 3+0
    // loss spent 72% of its clock on the first 40 moves, twelve of them at 4s
    // or more and two above 10s, and then had 51 seconds left for 109 moves.
    // A game that runs long is not rare and cannot be predicted, so the
    // permission to think long has to arrive late enough that a long game can
    // still afford it.
    let by_phase = (10 + game_ply / 4).clamp(10, EMERGENCY_MULT_MAX);
    let emergency_mult = by_clock.min(by_phase);
    // Com incremento o tecto pode ser mais largo, sem ele nao.
    //
    // Gastar tempo a mais e' recuperavel quando ha incremento -- o relogio
    // volta a encher a cada lance -- e irrecuperavel em morte subita, onde
    // cada segundo gasto e' um segundo que nunca mais existe. Toda a
    // calibracao acima (fase, pressao, predador) foi pensada para morte
    // subita, que e' o que o bot joga no Lichess (60+0 e 180+0). Aplicar a
    // mesma aperto a um controlo com incremento seria deixar em cima da mesa
    // tempo que ia ser devolvido de qualquer maneira.
    //
    // O incremento entra em proporcao do que ele vale por lance face a' fatia
    // base: um incremento que iguala a fatia duplica o tecto; um incremento
    // insignificante nao muda nada. Limitado a' dobra para nao transformar um
    // 1+1 num convite a gastar o relogio todo num lance.
    let fatia = (safe_time / moves_left).max(1);
    let folga_inc = (100 + (my_inc * 100 / fatia)).clamp(100, 200);
    let mut hard_cap = (safe_time * sp_t.tempo_hard_cap_pct as i64 / 100)
        .min((safe_time / horizon) * emergency_mult / 10)
        .min(soft * HARD_CAP_BUDGET_MULT / 10)
        .max(soft);
    // Aplicado ao tecto JA' decidido, e nao a um dos tres termos.
    //
    // A primeira tentativa alargou so' a percentagem do relogio -- e ficou
    // inerte, porque quem manda nestes controlos e' o termo do horizonte. Um
    // tecto que e' o minimo de tres coisas so' se alarga alargando a que
    // aperta, e qual delas e' depende do relogio e da fase. Multiplicar o
    // resultado resolve isso sem ter de adivinhar qual esta' a morder.
    hard_cap = hard_cap * folga_inc / 100;

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
    // How far ahead or behind we are on the clock, not just how much clock we
    // have. A raised ceiling is only safe if something watches the gap: the
    // move that deserves six seconds deserves them when we are ahead on time
    // and cannot afford them when the opponent has three times our clock and
    // will simply outlast us. Continuous rather than two steps, because the
    // step version treated 1.49x and 1.51x as different worlds.
    if opp_time > 0 && my_time > 0 {
        // ratio in tenths: 10 = level, 20 = double their clock, 5 = half.
        let ratio = (my_time * 10 / opp_time).clamp(3, 25);
        // Ahead: up to +50% of ceiling. Behind: down to -50%. Level: unchanged.
        let adj = (10 + (ratio - 10) / 2).clamp(5, 15);
        hard_cap = hard_cap * adj / 10;
        // Being far behind on the clock also caps the base allowance, not just
        // the emergency ceiling -- thinking well and flagging is still a loss.
    }

    // Modo predador: acompanhar o ritmo dele, 5% mais depressa.
    //
    // PISO sobre o que ja' esta' orcamentado, nao tecto. Se ele pensa dez
    // segundos, podemos pensar nove e meio -- nao se deixa um adversario
    // lento pensar o dobro de nos. E por ser 95 e nao 100, sempre que ambos
    // vamos ao limite ganhamos meio segundo de relogio por lance.
    //
    // O tecto duro por percentagem do NOSSO relogio continua a aplicar-se por
    // cima disto, portanto nunca estoura o orcamento quando somos nos a estar
    // apertados.
    if let Some(pace) = opp_pace {
        if pace > 0 {
            // Com folga de relogio podemos pensar MAIS do que ele; atras ou a
            // par, menos, que e' o que faz o relogio voltar a nosso favor.
            let folgado = opp_time > 0 && my_time * 10 >= opp_time * PREDADOR_FOLGA_R10;
            let fac = if folgado { PREDADOR_FOLGADO_PCT } else { PREDADOR_PCT };
            let predador = pace * fac / 100;
            soft = soft.max(predador.min(hard_cap));

            // E o tecto, so' sem folga de relogio. Ver TECTO_RITMO_PCT_OMISSAO.
            //
            // Aplicado ao `soft` e nao ao `hard_cap`: uma posicao critica
            // continua a poder pedir a extensao que merece, e o que se recusa
            // e' gastar por rotina varias vezes o que ele gasta. Nunca abaixo
            // do que a formula normal ja' daria sem ritmo nenhum, para um
            // adversario que joga de livro instantaneamente nao nos deixar
            // sem tempo para pensar.
            let tecto = tecto_ritmo();
            if tecto > 0 && !folgado {
                soft = soft.min((pace * tecto / 100).max(base));
            }
        }
    }


    let clearly_winning = last_score.map(|s| s >= 400).unwrap_or(false);
    let clearly_losing = last_score.map(|s| s <= -400).unwrap_or(false);

    // Os patamares, escalados ao ritmo da partida.
    //
    // Eram 20s e 10s fixos. Um nono do relogio inicial da' exactamente esses
    // vinte segundos num 180+0 -- o blitz nao muda nada -- e desce-os para
    // oito num 60+0, onde vinte eram um terco da partida a ser jogada em modo
    // de corte. O tecto de 20s mantem o comportamento de tudo o que seja mais
    // lento do que blitz, onde a formula ja estava calibrada.
    //
    // Sem relogio inicial conhecido (analise, `go` sem tempos) fica-se pelos
    // valores de sempre.
    let limiar_corte = relogio_inicial
        .map(|ini| (ini / 9).clamp(8_000, 20_000))
        .unwrap_or(20_000);
    let limiar_rajada = limiar_corte / 2;

    // Nivel 2: relogio baixo e SEM vantagem clara -- corta mais
    // fundo do que a formula normal permitiria. So' se relaxa quando a
    // vantagem e' NOSSA (clearly_winning); nunca quando e' do adversario.
    if safe_time < limiar_corte && !clearly_winning {
        let cut = (safe_time / 25).clamp(20, 800);
        soft = soft.min(cut);
        hard_cap = hard_cap.min(cut);
    }

    // Nivel 2.5: under ten seconds, play like a machine gun.
    //
    // Below this the game is no longer decided by the quality of any single
    // move -- it is decided by whether we still have a clock. Thinking is
    // what loses here: a good move played in 400ms and a slightly better one
    // played in 900ms are the same move if the second costs the game on time.
    // So the allowance stops being a share of the clock scaled by difficulty
    // and becomes a flat, small number of milliseconds, with the emergency
    // ceiling switched off entirely -- there is no move important enough to
    // be worth a long think when there is no long think left to have.
    //
    // Ten seconds buys about eighty moves at this rate, which is more chess
    // than most positions have remaining. Kept clear of the panic tier below,
    // which is a different thing: this is playing fast, that is surviving.
    if safe_time < limiar_rajada {
        let burst = (safe_time / 80).clamp(15, 150);
        soft = soft.min(burst);
        // An increment is income, not savings. Whatever is spent up to it comes
        // back, so the clock ends the move where it started -- and playing
        // 112ms a move on a five-second increment gives away 4.9 seconds of
        // free thinking, every move, to arrive at exactly the same clock.
        //
        // Capped at a third of what is left as well, because the increment
        // only arrives AFTER the move: spending it before it lands is spending
        // money that has not been paid.
        soft = soft.max(increment_funded(my_inc, safe_time));
        hard_cap = soft;
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
        // Panic must never be SLOWER than the burst tier above it. It was:
        // the clamp's floor of 150ms meant three seconds bought 142ms a move
        // while five seconds bought 60ms, so the engine slowed down as the
        // clock ran out. Taking the minimum keeps the whole ladder
        // monotonic -- less clock is never more time per move.
        soft = soft.min(panic);
        soft = soft.max(increment_funded(my_inc, safe_time));
        hard_cap = soft;
    }

    // Nivel 4: zona da morte (< 1200ms) -- praticamente so' vive do
    // incremento, chao absoluto independente de tudo o resto.
    if safe_time < 1200 {
        // The 40ms ceiling here was written for sudden death, where there is
        // no income and every millisecond spent is gone. With an increment it
        // is the wrong instrument: it caps a five-second allowance at forty
        // milliseconds and plays the rest of the game blind while the clock
        // climbs.
        let floor = increment_funded(my_inc, safe_time).max((my_inc * 4 / 5).clamp(2, 40));
        soft = floor;
        hard_cap = floor;
    }

    (soft, hard_cap)
}


/// Book path relative to the executable itself, never absolute: an absolute
/// path only exists on the machine it was written on, and the engine moves
/// between machines. Expects the book file next to the binary.
/// Quantas vezes um lance tem de aparecer no livro para ser jogado sem busca.
/// Medido neste livro: 23% das posicoes tem o seu melhor lance com contagem 1
/// e 43% com 2 ou menos. Abaixo deste numero o livro cala-se e a posicao vai a
/// busca. Valor a medir em A/B -- por isso e' uma constante e nao um palpite
/// espalhado pelo codigo.
const BOOK_MIN_COUNT: u32 = 1;   // MEDIDO e rejeitado: com 3, 391-430-233 em 1054 jogos (-13 Elo). Fica inerte.

/// Escolher entre os lances que o livro da' para esta posicao, com peso.
///
/// Escolhiamos sempre o mais frequente. Determinista, e por isso TODOS os
/// jogos com a mesma cor seguiam a mesma linha, lance por lance -- coisa que
/// qualquer adversario que jogue connosco duas vezes repara, e que qualquer
/// livro do outro lado explora de graca.
///
/// Sorteio proporcional a' contagem: a linha principal continua a sair na
/// maioria das vezes, mas nao sempre. Um lance jogado metade das vezes da
/// linha principal sai metade das vezes.
///
/// Xorshift proprio e nao uma biblioteca: sao tres linhas, nao ha dependencias
/// no projecto, e a semente vem do relogio uma vez por processo -- o que basta
/// para dois jogos seguidos nao serem iguais.
/// Repertorio no PRIMEIRO lance de brancas: d4, e depois e4.
///
/// Medido nos nossos 300 ultimos jogos, de brancas:
///   30+0   d4  13-0-2  em 15 jogos = 87%      e4  4-0-1 =  80%
///          c4   4-0-0  em  4 jogos = 100%     Nf3 2-0-1 =  67%
///   60+0   d4  50-9-20 em 79 jogos = 69%      e4  4-2-2 =  62%
///
/// O c4 esta' a 100% mas com quatro jogos, o que nao prova nada; o d4 e' a
/// unica linha com amostra que se aguente e e' a melhor. O sorteio entre
/// quatro estava a tirar jogos ao d4 para os dar ao Nf3, que e' o pior.
/// Variedade vale contra quem nos estude, e bots nao estudam.
const ABERTURA_BRANCAS: [&str; 2] = ["d2d4", "e2e4"];

/// Resposta ao primeiro lance dele. Medido nos mesmos 300 jogos, de pretas,
/// em 30+0 e 60+0:
///   contra e4  ->  c5   8-3-0 em 11 jogos = 86%, SEM UMA DERROTA
///   contra d4  ->  Nf6  6-4-3 em 13 jogos = 62%
///   contra Nf3 ->  Nf6  2-1-3 em  6 jogos = 42%   <- a nossa pior entrada
///   contra c4  ->  varias, 3-0-3 em 6     = 50%
///
/// A Siciliana e' o melhor terreno de todo o repertorio. Contra d4 o Nf6 e' o
/// que ja' jogavamos e aguenta-se. Nas outras duas a amostra e' pequena de
/// mais para escolher, e o livro decide como sempre.
fn resposta_de_pretas(board: &Board) -> Option<&'static str> {
    use crate::types::{Color, PieceType};
    let pw = board.pieces[Color::White.idx()][PieceType::Pawn.idx()];
    const E4: u64 = 1u64 << 28;
    const D4: u64 = 1u64 << 27;
    if pw & E4 != 0 {
        Some("c7c5")
    } else if pw & D4 != 0 {
        Some("g8f6")
    } else {
        None
    }
}

fn escolhe_do_livro(
    cands: &[(u32, crate::moves::Move)],
    rng: &mut u64,
) -> Option<crate::moves::Move> {
    if cands.is_empty() {
        return None;
    }
    if cands.len() == 1 {
        return Some(cands[0].1);
    }
    let total: u64 = cands.iter().map(|(c, _)| *c as u64).sum();
    if total == 0 {
        return Some(cands[0].1);
    }
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    let mut alvo = *rng % total;
    for (c, mv) in cands {
        let c = *c as u64;
        if alvo < c {
            return Some(*mv);
        }
        alvo -= c;
    }
    Some(cands[cands.len() - 1].1)
}

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
    // The filename here was the one the book shipped with when it was built
    // from another engine's analysis. Renaming the FILE to drop that reference
    // without renaming it HERE meant the engine looked for something that no
    // longer existed, found nothing, and quietly played every opening from
    // scratch -- for days. It cost 2.16 seconds on move one of a 60-second
    // bullet game and an improvised opening, in a game that ended 7
    // inaccuracies and 2 mistakes.
    //
    // A missing book is silent by design: the engine is supposed to work
    // without one. That is exactly what made this invisible.
    let filename =
        std::env::var("KESTREL_BOOK_FILE").unwrap_or_else(|_| "kestrel_book.bin".to_string());
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
    // Relogio do adversario no `go` anterior, e a media corrida do que ele
    // gasta por lance. Serve para nao o deixar fugir no relogio -- ver
    // TETO_RITMO em compute_time_budget.
    /// Semente para escolher entre lances de livro. Ver escolhe_do_livro.
    book_rng: u64,
    opp_time_anterior: Option<i64>,
    /// O relogio no inicio DESTA partida, estimado pelo maior valor ja visto.
    ///
    /// Existe porque os patamares de relogio baixo eram absolutos -- cortar
    /// aos 20s e disparar aos 10s -- e vinte segundos nao querem dizer o mesmo
    /// em todos os ritmos: num 180+0 sao o ultimo nono do relogio, num 60+0
    /// sao um terco dele. O bullet entrava em modo de corte com um terco da
    /// partida por jogar, que e' a origem do "primeiro lento de mais, depois
    /// rapido de mais".
    ///
    /// Estimado e nao recebido: o UCI nao diz qual foi o relogio inicial, so'
    /// o actual. O maior ja visto e' exacto desde o primeiro `go` da partida,
    /// e se a ponte so' apanhar o jogo a meio erra para MENOS, o que torna os
    /// patamares mais conservadores em vez de mais arriscados.
    relogio_inicial: Option<i64>,
    opp_pace: Option<i64>,
    last_score: Option<i32>, // score (cp, nossa perspetiva) do ultimo "go" -- para os niveis 2/3 de compute_time_budget
    style_book: Option<crate::book::Book>, // "assinatura" da Judit Polgar -- ver book.rs
    threads: usize, // Lazy SMP -- ver search_mt(). 1 = sem paralelismo (comportamento antigo).
    /// Whether the threads vote on the move, or the main thread simply decides.
    ///
    /// An option because the two models have never been separated by enough
    /// games to tell them apart here. The comment defending the vote cites 50
    /// games; fifty games separate nothing. Measured in the position that lost
    /// a rated game, the vote overrode a CORRECT main thread and played the
    /// losing move in 2 runs of 20, while the main thread picked it in 0 of 20
    /// at either thread count.
    lazy_vote: bool,
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
        // DESLIGAR O LIVRO, e dizê-lo em voz alta.
        //
        // Já era possível ficar sem livro -- bastava o ficheiro não existir --
        // mas isso acontece EM SILÊNCIO, e foi assim que o motor jogou dias
        // inteiros a improvisar aberturas sem ninguém dar por isso, depois de
        // o ficheiro ser renomeado. Uma ausência que se escolhe tem de se
        // distinguir de uma ausência por acidente.
        let sem_livro = std::env::var("KESTREL_SEM_LIVRO")
            .map(|v| v != "0")
            .unwrap_or(false);
        let style_book = if sem_livro {
            eprintln!("livro: DESLIGADO por KESTREL_SEM_LIVRO -- aberturas jogadas pela busca");
            None
        } else {
            crate::book::Book::load(&default_style_book_path()).ok()
        };
        // Build every lazily-initialised global now, while no clock is
        // running -- see evaluation::warmup().
        // Profile before warm-up: warm-up evaluates a position, which seals
        // the lazily-built tables, so anything the profile sets has to be in
        // place first.
        if std::env::var("KESTREL_HEATMAP_ONLY").map(|v| v == "1").unwrap_or(false) {
            HEATMAP_ONLY.store(true, std::sync::atomic::Ordering::Relaxed);
            let n: i32 = std::env::var("KESTREL_HEATMAP_PLIES").ok()
                .and_then(|v| v.parse().ok()).unwrap_or(1).clamp(1, 3);
            HEATMAP_PLIES.store(n, std::sync::atomic::Ordering::Relaxed);
            eprintln!("modo heatmap: sem busca, {} ply(s) de avaliacao estatica", n);
        }
        crate::evaluation::warmup();
        crate::search::warmup();
        Engine {
            board: Board::startpos(),
            atk,
            zob,
            tt: TranspositionTable::new(64),
            history: Vec::new(),
            book_rng: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E3779B97F4A7C15)
                | 1,
            opp_time_anterior: None,
            opp_pace: None,
            relogio_inicial: None,
            last_score: None,
            style_book,
            threads: 1,
            lazy_vote: true,
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
        self.history.push(self.board.hash);
        if tokens.get(i) == Some(&"moves") {
            i += 1;
            while i < tokens.len() {
                if let Some(mv) = self.find_move(tokens[i]) {
                    self.board.make_move(&mv);
                    self.history.push(self.board.hash);
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

    /// The book's answer for the position on the board, if it has one and it
    /// is legal. Highest recorded count wins; ties keep the first, so the
    /// choice is deterministic and a game can be reproduced from its moves.
    ///
    /// Legality is re-checked rather than trusted. A 64-bit position key can
    /// collide, and a book built by another tool can disagree about castling
    /// or en-passant rights in ways the key does not capture. Playing an
    /// illegal move loses the game outright, which is far worse than the cost
    /// of generating the move list once.
    fn book_move(&mut self) -> Option<crate::moves::Move> {
        let book = self.style_book.as_ref()?;
        let entries = book.lookup(self.board.hash);
        if entries.is_empty() {
            return None;
        }
        let mut b = self.board.clone();
        let legal = crate::movegen::generate_legal(&mut b, &self.atk);
        let cands: Vec<(u32, crate::moves::Move)> = entries
            .iter()
            .filter_map(|(m16, count)| {
                let (from, to, promo) = crate::book::decode_move16(*m16);
                legal
                    .iter()
                    .find(|l| l.from == from && l.to == to && l.promotion == promo)
                    .map(|l| (*count, *l))
            })
            // O livro nao pode repetir uma posicao que ja' aconteceu no jogo.
            //
            // A consulta e' por posicao e nao sabe nada do que ja' se jogou,
            // portanto duas posicoes que se respondem uma a' outra mandam o
            // motor de uma para a outra para sempre. Jogo gevDcupD: 6...Nh5
            // 7.Bd2 Nhf6 8.Bf4 Nh5 9.Bd2 Nhf6 10.Bf4 -- empate ao lance 9,
            // todos os lances a 0.00s e o relogio parado nos 59.9s. O motor
            // nunca pensou; o livro repetiu-se ate' a' triplice.
            //
            // Nao chega o desprezo pelo empate na busca: quando o livro
            // responde, busca nenhuma corre.
            //
            // Se todos os lances de livro repetirem, devolve-se None e a
            // posicao vai a busca -- que ja' sabe pontuar um empate com
            // desprezo e escolhe outra coisa se houver melhor.
            .filter(|(_, mv)| {
                let mut ap = self.board.clone();
                ap.make_move(mv);
                !self.history.contains(&ap.hash)
            })
            .collect::<Vec<_>>()
            .into_iter()
            // Um lance visto num unico jogo nao e' teoria, e' a escolha de
            // alguem naquele dia. Contado no proprio livro: em 23% das
            // posicoes o lance MAIS jogado aparece uma unica vez, e em 43%
            // duas ou menos. Nessas, jogar de livro e' jogar as cegas com
            // zero nos de busca -- e a busca, com dois segundos, quase de
            // certeza sabe mais do que uma amostra de um.
            //
            // As linhas principais nao sao afectadas: a contagem maxima do
            // livro e' 17445, e sao essas as posicoes que aparecem quase
            // sempre. O que se recusa e' a cauda rara e profunda.
            .filter(|(count, _)| *count >= BOOK_MIN_COUNT)
            .collect::<Vec<_>>();
        // Primeiro lance de brancas: so' o repertorio. Em qualquer outra
        // posicao o livro responde como sempre.
        // Primeiro lance de pretas: a resposta do repertorio, se houver.
        if self.history.len() == 2 && self.board.side == crate::types::Color::Black {
            if let Some(alvo) = resposta_de_pretas(&self.board) {
                if let Some((_, mv)) = cands.iter().find(|(_, m)| m.to_uci() == alvo) {
                    return Some(*mv);
                }
            }
        }
        if self.history.len() == 1 && self.board.side == crate::types::Color::White {
            let rep: Vec<(u32, crate::moves::Move)> = cands
                .iter()
                .filter(|(_, mv)| {
                    let u = mv.to_uci();
                    ABERTURA_BRANCAS.iter().any(|a| *a == u)
                })
                .cloned()
                .collect();
            if !rep.is_empty() {
                return escolhe_do_livro(&rep, &mut self.book_rng);
            }
        }
        escolhe_do_livro(&cands, &mut self.book_rng)
    }

    fn cmd_go(&mut self, tokens: &[&str], out: &mut impl Write) {
        // Once per real move, not once per node and not once per Lazy SMP
        // thread (which would be several times per move) -- this is the one
        // place in the whole engine a real "go" from a real game always
        // passes through, book and tablebase answers included.
        self.tt.increase_gen();
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

        // Ritmo do adversario: quanto gastou desde o nosso `go` anterior.
        //
        // Tem de ficar AQUI, antes do livro e das tablebases. Estava a seguir
        // a eles e nunca arrancava: as primeiras dez ou quinze jogadas saem do
        // livro, que devolve sem passar pelo calculo do tempo, portanto
        // `opp_time_anterior` ficava por preencher e o ritmo dele era None
        // exactamente nos lances em que ele comeca a contar.
        //
        // O incremento dele volta ao relogio depois de jogar, logo entra na
        // conta: gasto = anterior + inc - agora.
        if let Some(ant) = self.opp_time_anterior {
            let opp_inc = if side_white { binc } else { winc };
            let gasto = ant + opp_inc - opp_time;
            if gasto >= 0 && gasto < 600_000 {
                // Media corrida curta -- o que ele fez ha vinte lances nao diz
                // nada sobre o que vai fazer agora, e uma media longa demora
                // demasiado a reagir a uma mudanca de ritmo, que e'
                // precisamente quando isto importa.
                self.opp_pace = Some(match self.opp_pace {
                    Some(p) => (p * 2 + gasto) / 3,
                    None => gasto,
                });
            }
        }
        if wtime > 0 || btime > 0 {
            self.opp_time_anterior = Some(opp_time);
            // O maior relogio ja visto nesta partida e a melhor estimativa do
            // inicial que o UCI permite. Ver `relogio_inicial`.
            self.relogio_inicial = Some(match self.relogio_inicial {
                Some(m) => m.max(my_time),
                None => my_time,
            });
        }

        let instant_book_ok = restrict_root.is_empty()
            && !infinite
            && depth.is_none()
            && nodes.is_none()
            && std::env::var_os("KESTREL_NO_BOOK_INSTANT").is_none();

        // UM so' lance legal: joga-se, nao se pensa.
        //
        // Nao havia atalho nenhum para isto -- uma recaptura obrigatoria ou uma
        // saida de xeque unica pagavam a busca inteira, e num relogio de bullet
        // isso e' orcamento gasto onde nao ha nada para decidir. Reportado com
        // `time 0` e `nodes 0` porque e' a verdade: nao houve busca.
        //
        // A montante do livro e das tablebases de proposito: se so' ha um
        // lance, nem vale a pena perguntar-lhes. Gerar os legais custa
        // microsegundos contra os centenas de milissegundos que se poupam.
        //
        // Sem pontuacao inventada: a busca e' que sabe quanto vale a posicao e
        // aqui nao correu, portanto anuncia-se cp 0. Quem le' o score para
        // decidir empates (ver `lichess_bridge.py`) ve' um valor neutro, que e'
        // preferivel a um numero com ar de opiniao que ninguem formou.
        if instant_book_ok {
            let legais = crate::movegen::generate_legal(&mut self.board, &self.atk);
            if legais.len() == 1 {
                let mv = legais[0];
                let _ = writeln!(
                    out,
                    "info depth 1 multipv 1 score cp 0 nodes 0 nps 0 time 0 pv {}",
                    mv.to_uci()
                );
                let _ = writeln!(out, "bestmove {}", mv.to_uci());
                let _ = out.flush();
                return;
            }
        }
        // Solved before it is searched.
        //
        // Same place as the book and for the same reason: if the answer already
        // exists there is nothing to decide, and thinking about it only spends
        // clock. Reported with time 0 and no nodes, because that is the truth --
        // the engine did not search, it asked.
        // A consulta a tabela corre no NOSSO relogio, antes de o motor pensar.
        // Cem milissegundos e' o que ela tem; se nao responder, joga-se. Ver o
        // tempo limite em tablebase.rs.
        if instant_book_ok && crate::tablebase::enabled() {
            if let Some(hit) = crate::tablebase::probe(&self.board) {
                if let Some(mv) = self.find_move(&hit.best) {
                    let sc = match hit.wdl {
                        1 => 10_000 - hit.dtz.abs(),
                        -1 => -10_000 + hit.dtz.abs(),
                        _ => 0,
                    };
                    let (w, d, l) = match hit.wdl {
                        1 => (1000, 0, 0),
                        -1 => (0, 0, 1000),
                        _ => (0, 1000, 0),
                    };
                    let _ = writeln!(
                        out,
                        "info depth 0 multipv 1 score cp {} wdl {} {} {} nodes 0 nps 0 time 0 pv {}",
                        sc, w, d, l, mv.to_uci()
                    );
                    let _ = writeln!(out, "info string tablebase: dtz {}", hit.dtz);
                    let _ = writeln!(out, "bestmove {}", mv.to_uci());
                    let _ = out.flush();
                    return;
                }
            }
        }
        if instant_book_ok {
            if let Some(mv) = self.book_move() {
                let _ = writeln!(out, "info depth 0 multipv 1 score cp 0 nodes 0 nps 0 time 0 pv {}", mv.to_uci());
                let _ = writeln!(out, "bestmove {}", mv.to_uci());
                let _ = out.flush();
                return;
            }
        }

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
        // Heatmap mode: play what the evaluation alone says, with no search.
        //
        // Every legal move is played, the resulting position evaluated once,
        // and the best one returned -- which is exactly what the heatmap page
        // paints. It is not a way to play well; it is a way to see the
        // evaluation with nothing in front of it. Anything the search normally
        // hides -- a term with the wrong sign, an amplitude that swamps the
        // rest -- shows up in the move immediately.
        //
        // Note the sign: `evaluate` answers from the side to move's point of
        // view, and after our move that is the opponent. The best move for us
        // is the one that leaves THEM the worst position.
        if heatmap_only() {
            let plies = heatmap_plies();
            let legal = crate::movegen::generate_legal(&mut self.board, &self.atk);

            // The threat: what he would play if we passed.
            //
            // MEASURED, and the result is a lesson rather than a feature.
            // Naming the threat works perfectly -- on a knight attacked twice
            // it reports the capture outright -- but spending the extra ply on
            // the moves that answer it takes the suite from 42/214 to 23, and
            // giving every move that third ply takes it to 5.
            //
            // The cause is not the mixture of depths, which was the obvious
            // suspect. It is parity. An odd depth ends on OUR move, so the
            // evaluation sees the piece we just took and not the recapture --
            // the horizon effect in its purest form. At two plies he has the
            // last word and the recapture shows up. Without a quiescence
            // search, only even depths mean anything, and this is why the
            // engine's own `go depth 1` holds up at 41/214: it HAS one.
            //
            // The null move stays because naming the threat is worth having on
            // its own, as an `info string`. What it must not do is buy depth.
            //
            // A null move answers a question nothing else here can. Two plies
            // show one reply to each of OUR moves, but never what he was
            // trying to do beforehand -- so they cannot tell "this move is
            // good" from "this move saves me". Passing the turn and reading
            // his heatmap names the threat outright.
            //
            // Note what this is NOT for. The threat's value is the same for
            // every move we might play, so it cannot reorder anything by
            // itself; adding a constant changes no ranking. What it buys is
            // knowing WHERE to look further: the moves that answer the threat
            // -- taking the attacker, moving the target away -- get a third
            // ply, and the rest do not. Depth where it is earned, on a budget
            // of one extra square's worth of work.
            let threat: Option<(crate::moves::Move, i32)> = if plies >= 3 {
                let u = self.board.make_null_move();
                let his = crate::movegen::generate_legal(&mut self.board, &self.atk);
                let mut best_r: Option<(crate::moves::Move, i32)> = None;
                for r in &his {
                    let u2 = self.board.make_move(r);
                    let v = -crate::evaluation::evaluate(&mut self.board);
                    self.board.unmake_move(r, &u2);
                    if best_r.map_or(true, |(_, b)| v > b) {
                        best_r = Some((*r, v));
                    }
                }
                self.board.unmake_null_move(&u);
                best_r
            } else {
                None
            };
            // What the position is worth if we do nothing: the baseline the
            // threat is measured against.
            let idle = threat.map(|(_, v)| -v);
            if let Some((t, v)) = threat {
                let _ = writeln!(out, "info string ameaca {} vale {}", t.to_uci(), -v);
            }
            let mut best: Option<(crate::moves::Move, i32)> = None;
            for mv in &legal {
                let undo = self.board.make_move(mv);
                // One ply asks what the evaluation thinks of OUR move. Two
                // plies asks whether it saw what he answers -- his own heatmap,
                // from his side, and he plays its best square. The difference
                // between the two is how much of our error is blindness to the
                // reply rather than a wrong reading of the position.
                // A move that answers the threat is looked at one ply deeper:
                // it either takes the piece that was going to move, or gets
                // the target out of the way.
                // Does his threat survive our move?
                //
                // No extra ply for this: the question is answered by looking,
                // not by searching. If the piece he was going to take is still
                // there, and the piece that was going to take it is still
                // there, the threat stands and the move has not addressed it.
                // Costs two board reads, and the point of this mode is to see
                // what the evaluation sees rather than to out-search it.
                let threat_survives = threat.map_or(false, |(t, _)| {
                    self.board.piece_at(t.from).is_some() && self.board.piece_at(t.to).is_some()
                });
                let after = if plies >= 3 {
                    // Threat-aware, at one ply of cost. The static value of
                    // our move, minus what he still threatens if we did not
                    // deal with it.
                    let base = -crate::evaluation::evaluate(&mut self.board);
                    if threat_survives {
                        base.saturating_sub(idle.map_or(0, |i| (base - i).max(0)))
                    } else {
                        base
                    }
                } else if plies <= 1 {
                    let his = crate::movegen::generate_legal(&mut self.board, &self.atk);
                    if his.is_empty() {
                        // He has no move: mate in our favour, or a stalemate
                        // that throws the whole game away.
                        -terminal_value(&self.board, &self.atk)
                    } else {
                        -crate::evaluation::evaluate(&mut self.board)
                    }
                } else {
                    let replies = crate::movegen::generate_legal(&mut self.board, &self.atk);
                    if replies.is_empty() {
                        // Mate for him is the best move on the board; stalemate
                        // is a draw and must never look like the material says.
                        -terminal_value(&self.board, &self.atk)
                    } else {
                        let mut his_best = i32::MIN;
                        for r in &replies {
                            let u2 = self.board.make_move(r);
                            let ours = crate::movegen::generate_legal(&mut self.board, &self.atk);
                            let v = if ours.is_empty() {
                                // Now WE have no move: his reply mates us, or
                                // stalemates us out of a won game.
                                -terminal_value(&self.board, &self.atk)
                            } else {
                                -crate::evaluation::evaluate(&mut self.board)
                            };
                            self.board.unmake_move(r, &u2);
                            if v > his_best {
                                his_best = v;
                            }
                        }
                        // His best is our worst.
                        -his_best
                    }
                };
                self.board.unmake_move(mv, &undo);
                if best.map_or(true, |(_, b)| after > b) {
                    best = Some((*mv, after));
                }
            }
            match best {
                Some((mv, sc)) => {
                    let _ = writeln!(
                        out,
                        "info depth {} multipv 1 score cp {} nodes {} nps 0 time 0 pv {}",
                        plies, sc, legal.len(), mv.to_uci()
                    );
                    let _ = writeln!(out, "bestmove {}", mv.to_uci());
                }
                None => {
                    let _ = writeln!(out, "bestmove 0000");
                }
            }
            let _ = out.flush();
            return;
        }

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
            let pieces_left = self
                .board
                .pieces
                .iter()
                .flat_map(|side| side.iter())
                .map(|bb| bb.count_ones() as i64)
                .sum::<i64>();
            let (mut soft, mut hard_cap) =
                compute_time_budget(my_time, my_inc, opp_time, movestogo, self.last_score,
                                    pieces_left, (self.board.fullmove as i64 - 1) * 2
                                        + if side_white { 0 } else { 1 },
                                    self.opp_pace, self.relogio_inicial);
            // The opening is played, not calculated -- including the parts of
            // it the book does not reach.
            //
            // Book positions already answer instantly. The moves in between
            // did not: leaving book at ply 2 and searching it normally cost
            // 1594ms, and out-of-book opening moves at 1.5s each are how a
            // quarter of the clock disappears before the game starts. What
            // makes that affordable in the middlegame -- the position is
            // unique and the clock is there to be spent on it -- is exactly
            // what is not true here.
            //
            // So the whole opening is capped, book or not. The engine still
            // searches, and at this speed still reaches a sensible depth; it
            // simply cannot spend middlegame money on a phase where the answer
            // is either already known or is one of several equally playable
            // moves. Tunable without a rebuild, because the right number
            // depends on the book underneath it.
            let game_ply = (self.board.fullmove as i64 - 1) * 2 + if side_white { 0 } else { 1 };
            if game_ply < OPENING_PLIES {
                let cap = std::env::var("KESTREL_OPENING_MS")
                    .ok()
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(OPENING_MAX_MS);
                soft = soft.min(cap);
                hard_cap = hard_cap.min(cap);
            }
            // Publish the decision, not just its effect. The clock is the one
            // part of the engine whose reasoning is invisible in the output --
            // a move that took 1.04s tells you nothing about whether the
            // engine considered spending more and declined, or was never
            // allowed to. This line is what the heatmap reads.
            // See the note in search.rs: println! panics on a closed pipe,
            // and the arbiter closes ours at the end of every game.
            let _ = writeln!(
                std::io::stdout(),
                "info string tm soft {} hard {} horizon {} pieces {} myclock {} oppclock {} pace {:?}",
                soft,
                hard_cap,
                (12 + (pieces_left - 2) * 7 / 5).clamp(12, 42),
                pieces_left,
                my_time,
                opp_time,
                self.opp_pace,
            );
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
                (soft - ADVISOR_RESERVE_MS - move_overhead_ms()).max(think_floor)
            } else {
                (soft - move_overhead_ms()).max(think_floor)
            };
            let hard_ms = (hard_cap - move_overhead_ms()).max(search_ms);
            soft_budget = Some(Duration::from_millis(search_ms.max(1) as u64));
            Some(Instant::now() + Duration::from_millis(hard_ms.max(1) as u64))
        };

        let max_depth = depth.unwrap_or(64);
        let limits = SearchLimits { deadline, max_depth, max_nodes: nodes, soft_budget };
        let board_now = self.board.clone();
        // Per-move piece values, when asked for. Printed BEFORE the search
        // so the numbers describe the position the engine is about to think
        // about, not the one it already chose a move in.
        if std::env::var_os("KESTREL_VALORES_PECAS").is_some() {
            let mut b = self.board.clone();
            let n = b.occ_all.count_ones() as usize;
            let balde = (n.saturating_sub(2) / 4).min(7);
            let vals = crate::evaluation::valores_das_pecas(&mut b);
            let mut linha = format!(
                "info string pecas balde={} n={} escala={}",
                balde,
                n,
                crate::nnue::escala_pos(&b)
            );
            for (pt, v, cnt) in vals {
                linha.push_str(&format!(" {:?}={}({})", pt, v, cnt));
            }
            let _ = writeln!(out, "{}", linha);
            let _ = out.flush();
        }
        // Raw static eval, no search at all -- for cross-checking a reader
        // against an external oracle position by position, where even a
        // depth-1 search already mixes in a best-move choice.
        if std::env::var_os("KESTREL_EVAL_ONLY").is_some() {
            let mut b = self.board.clone();
            let v = crate::evaluation::evaluate(&mut b);
            let _ = writeln!(out, "info string eval_only {}", v);
            let _ = out.flush();
            return;
        }
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
                format!("cp {}", crate::evaluation::score_normalizado(score))
            };
            match best {
                Some(mv) => {
                    collected.push((((b'A' + (pv_index - 1) as u8)) as char, mv, score));
                    // Com MultiPV a 1 e UMA thread a busca ja' narrou esta
                    // profundidade enquanto aprofundava, e esta seria uma SEGUNDA
                    // linha para a mesma -- sem o campo wdl e com o nps recalculado
                    // sobre o tempo total, portanto com numeros diferentes dos da
                    // primeira. Uma interface que leia info linha a linha via duas
                    // actualizacoes incoerentes para a mesma profundidade, e um
                    // `tail -1` apanhava a errada -- foi o que aconteceu a varias
                    // medicoes deste projecto.
                    //
                    // Com VARIAS threads essa supressao estava errada, e de uma
                    // forma que se ve de fora: quem narra durante a busca e' a
                    // thread 0, mas quem decide e' a votacao entre threads. Quando
                    // a votacao elege outro lance, a ultima linha impressa nomeia
                    // um lance e o `bestmove` seguinte nomeia outro. Medido nesta
                    // posicao, tres vezes em doze a 300ms -- um quarto dos lances.
                    // O PV corrigido ja' era calculado aqui a partir da thread
                    // vencedora e depois deitado fora.
                    //
                    // Um motor que anuncia uma variante e joga outra contradiz-se
                    // no proprio protocolo: corrompe qualquer ferramenta que leia
                    // a linha, e esconde exactamente o caso que interessa
                    // investigar, que e' quando a votacao muda a decisao.
                    //
                    // Com MultiPV > 1 continua a ser precisa por outra razao: as
                    // linhas B, C e seguintes nao passam pelo narrador da busca.
                    if pv_index <= multipv && (multipv > 1 || pv_index > 1 || self.threads > 1) {
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
        let vote_on = self.lazy_vote;
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
                    root_side: board.side,
                        atk: atk_ref,
                        zob: zob_ref,
                        tt: tt_ref,
                        nodes: 0,
                        limits,
                        stop: false,
                        stop_flag: stop_ref,
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
                        cutoff_cnt: [0; crate::search::MAX_PLY],
                        ult_margem: [-1; crate::search::MAX_PLY],
                        ameacas_reduzidos: [0; 4],
                        ameacas_bateram: [0; 4],
                        cutcnt_reduzidos: [0; 4],
                        cutcnt_bateram: [0; 4],
                        subalfa_reduzidos: [0; 4],
                        subalfa_bateram: [0; 4],
                        margem_reduzidos: [0; 4],
                        margem_bateram: [0; 4],
                        // only thread 0 narrates, or each depth would be
                        // announced once per Lazy-SMP thread
                        report: ti == 0,
                        thread_idx: ti,
                    };
                    // The default stack (2MB) is enough for the Rust search
                    // alone, but an evaluation that allocates its working
                    // buffers on the stack multiplies them by the real
                    // negamax recursion depth, and 2MB does not survive that.
                    // 16MB is generous headroom and costs nothing but virtual
                    // address space on a 64-bit system.
                    std::thread::Builder::new()
                        .stack_size(16 * 1024 * 1024)
                        .spawn_scoped(scope, move || {
                            let mut searcher = searcher;
                            let (best, score, depth_reached, nodes) =
                                searcher.iterative_deepening(&mut b);
                            (best, score, depth_reached, nodes, searcher)
                        })
                        .expect("failed to spawn search thread")
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
            // Thread 0 is index 0 and is the one that narrated the search.
            // With the vote off it simply decides, which is the other model in
            // common use: helpers exist only to fill the shared table.
            let best_idx = if results.len() < 2 || !vote_on {
                0
            } else {
                // The score votes, but only when it can be trusted.
                //
                // First attempt at fixing this removed the score from the
                // weight entirely, on the theory that picking the maximum of
                // N noisy samples is biased upward and the bias grows with N.
                // The theory is right and the fix was wrong: a strong
                // reference uses score * depth exactly as this did, and
                // guards the bias at its source instead. What it refuses are
                // scores that are not measurements -- a thread whose search
                // was cut, and a thread whose principal variation is too
                // short to be a line at all. Those are where an inflated
                // number comes from, and the difference between four threads
                // and ten is how often one of them shows up.
                //
                // Traced from a real loss: 22.Nxb5 cost 244cp, and the
                // engine's own evaluation ranks it DEAD LAST of seven
                // candidates -- -285 against -31 for the move it plays at one
                // thread. Search did not fail to see it. A thread came back
                // convinced, and being convinced was the whole of the weight.
                let min_score = results.iter().map(|r| r.1).min().unwrap_or(0);
                let pv_len: Vec<usize> = results
                    .iter()
                    .map(|r| r.4.extract_pv(board, 4).len())
                    .collect();
                let weight = |i: usize, r: &(Option<crate::moves::Move>, i32, i32, u64, Searcher)| {
                    // A line of two moves is not a line. Such a thread still
                    // plays -- it simply does not get to outvote one that
                    // searched something through to an end.
                    let trusted = if pv_len[i] > 2 { 1i64 } else { 0i64 };
                    // SEM o factor de profundidade: pesa-se por
                    // `score - minScore + 14` e mais nada. O `* depth` vinha
                    // do tempo do esquema de saltos que o acompanhava. Com as threads todas a percorrer as
                    // mesmas profundidades (ver search.rs), multiplicar pela
                    // profundidade so' amplifica ruido de quem chegou la'
                    // primeiro.
                    ((r.1 - min_score + 14) as i64) * trusted
                };
                let mut return_idx: Option<usize> = None;
                let mut votes: Vec<(Option<crate::moves::Move>, i64)> = Vec::new();
                for (i, r) in results.iter().enumerate() {
                    match votes.iter_mut().find(|(m, _)| *m == r.0) {
                        Some((_, v)) => *v += weight(i, r),
                        None => votes.push((r.0, weight(i, r))),
                    }
                }
                // Ties go to thread 0. It is the one that reports during the
                // search and the one whose clock decisions end it, so when the
                // pool is genuinely split, the move already being announced is
                // the one to play.
                // DUAS coisas que a votacao nunca pode fazer, e que ate' aqui
                // podia. Ambas vem do `get_best_thread` do Stockfish, que as
                // trata antes de contar votos (GPL-3.0, ver NOTICES.md):
                //
                //   1. Eleger um lance cuja PROPRIA pontuacao e' derrota
                //      provada. Uma thread convencida de que esta perdida nao
                //      deve poder arrastar as outras para la'.
                //   2. Sobrepor-se a uma thread que ja' provou vitoria. Se ha'
                //      mate visto, a decisao passa a ser "qual o mate mais
                //      curto" e a votacao deixa de mandar.
                //
                // A primeira e' exactamente a avaria registada no comentario
                // acima -- "a votacao sobrepos-se a um thread principal
                // CORRECTO e jogou o lance perdedor em 2 de 20". O peso podia
                // ser grande porque `score - min_score` e' grande justamente
                // quando as OUTRAS threads estao ainda pior.
                let limiar_decisivo = crate::search::MATE_SCORE - crate::search::MAX_PLY as i32;
                let vitoria = |sc: i32| sc >= limiar_decisivo;
                let derrota = |sc: i32| sc <= -limiar_decisivo;

                // Ha' vitoria provada? Entao escolhe-se o mate mais curto e
                // acabou -- pontuacao maior significa mate em menos lances.
                if let Some((i, _)) = results
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| vitoria(r.1))
                    .max_by_key(|(_, r)| r.1)
                {
                    return_idx = Some(i);
                }

                // Lances que sao derrota provada saem da contagem. Se TODOS o
                // forem, nao ha nada a salvar e a votacao segue como antes --
                // recusar tudo deixaria a escolha sem candidatos.
                let algum_salvavel = results.iter().any(|r| !derrota(r.1));
                if algum_salvavel {
                    let perdidos: Vec<Option<crate::moves::Move>> = results
                        .iter()
                        .filter(|r| derrota(r.1))
                        .map(|r| r.0)
                        .collect();
                    votes.retain(|(m, _)| !perdidos.contains(m) || results[0].0 == *m);
                }

                let top = votes.iter().map(|(_, v)| *v).max().unwrap_or(0);
                let winner = if votes.iter().any(|(m, v)| *v == top && *m == results[0].0) {
                    results[0].0
                } else {
                    votes
                        .iter()
                        .max_by_key(|(_, v)| *v)
                        .map(|(m, _)| *m)
                        .unwrap_or(results[0].0)
                };
                // Among the threads that agree on the winning move, take the
                // one with the strongest individual claim: its PV is the one
                // worth reporting.
                match return_idx {
                    // Vitoria provada: ja' esta decidido, e a votacao nao
                    // opina sobre mates.
                    Some(i) => i,
                    None => (0..results.len())
                        .filter(|&i| results[i].0 == winner)
                        .max_by_key(|&i| weight(i, &results[i]))
                        .unwrap_or(0),
                }
            };
            let nodes_total: u64 = results.iter().map(|r| r.3).sum();
            let (best, score, depth_reached, _, winner) = results.remove(best_idx);
            // Anchored on the move actually returned. extract_pv rebuilds the
            // line from the transposition table, whose root entry is
            // always-replace and so need not be the move this search settled
            // on: the engine printed "pv a2b4 ..." and then "bestmove h4f5".
            // The decision was right and the announcement was wrong, which
            // misleads anyone reading the output and corrupts any tool that
            // walks the line -- the line analysed was not the line played.
            let pv_line = match best {
                Some(mv) => winner.extract_pv_from(board, mv, depth_reached.max(1) as usize + 4),
                None => winner.extract_pv(board, depth_reached.max(1) as usize + 4),
            };
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
                    let _ = writeln!(out, "option name Move Overhead type spin default {} min 0 max 5000", MOVE_OVERHEAD_DEFAULT_MS);
                    #[cfg(feature = "bot")]
                    let _ = writeln!(out, "option name OnlineTablebase type check default false");
                    let _ = writeln!(out, "option name Contempt type spin default 20 min -200 max 200");
                    // Escala da rede, em centesimos. Uma rede que avalia material
                    // ao dobro faz TODAS as margens de poda dispararem ao dobro
                    // da velocidade, porque elas sao centipeoes fixos. Medido:
                    // uma dama vale 2578 na rede oficial contra 1161 na
                    // nossa antiga, para a qual as margens foram afinadas -- dai'
                    // 222. Por opcao em vez de por compilacao, para o bot poder
                    // trocar de rede sem recompilar.
                    let _ = writeln!(out, "option name EvalFactor type spin default 100 min 20 max 500");
                    let _ = writeln!(out, "option name LazyVote type check default true");
                    let _ = writeln!(out, "option name Threats type check default true");
                    let _ = writeln!(
                        out,
                        "option name EscalaPorBalde type check default false"
                    );
                    let _ = writeln!(
                        out,
                        "option name LmrCaptures type check default false"
                    );
                    // A rede, pelo nome que toda a gente usa. Sem isto so' se
                    // carrega por variavel de ambiente, que nenhuma interface
                    // grafica sabe pôr.
                    let _ = writeln!(
                        out,
                        "option name EvalFile type string default {}",
                        crate::nnue_sf::caminho_evalfile()
                    );
                    let _ = writeln!(
                        out,
                        "option name EvalScale type spin default {} min 100 max 2000",
                        crate::nnue::escala()
                    );
                    // Tecto de tempo pelo ritmo do adversario. 0 = desligado,
                    // que e' a omissao ate' um SPRT dizer o contrario.
                    let _ = writeln!(
                        out,
                        "option name RitmoTecto type spin default {} min 0 max 1000",
                        tecto_ritmo()
                    );
                    // A partir de que fila um peao deixa de ser reduzido pelo
                    // LMR. 0 = desligado. Ver PEAO_FILA_SEM_LMR.
                    let _ = writeln!(
                        out,
                        "option name PeaoSemLmr type spin default {} min 0 max 8",
                        crate::search::peao_fila_sem_lmr()
                    );
                    // Every search parameter, announced as a spin so an SPSA
                    // harness can read the default and the band straight from
                    // `uci` and drive them with `setoption`. They were already
                    // settable -- `set_param` and `PARAM_OVERRIDES` have been
                    // there all along -- but a tuner cannot use what it cannot
                    // see, so the whole calibration had to be done by hand or
                    // through a file, and the file silently ignores a vector
                    // of the wrong length.
                    //
                    // The band is the default plus or minus its own size, with
                    // a floor of one unit so a parameter that defaults to zero
                    // still has somewhere to move. Wide enough for the scale of
                    // the evaluation to have changed underneath a margin,
                    // narrow enough that a run cannot wander into nonsense.
                    for (n, d) in crate::search::PARAM_NAMES
                        .iter()
                        .zip(crate::search::SearchParams::default().to_vec())
                    {
                        // The band is normally proportional to the default, but
                        // a parameter that defaults to zero would get a range of
                        // 0..10 -- and these two are measured in MILLI-plies
                        // (1/1024 of a ply), where the interesting values are
                        // ~31 and ~1500. A tuner handed 0..10 would sweep a
                        // range in which nothing it can set changes anything,
                        // and report that the parameter does not matter.
                        let band = match *n {
                            "lmr_move_linear" => 120,
                            "lmr_cutnode" => 3072,
                            "lmr_capture_base" => 3072,
                            "lmr_capture_hist_divisor" => 15000,
                            // Mesma armadilha: `rfp_return_beta` e' uma fraccao
                            // em 1024-avos com default 0, e a banda proporcional
                            // dava-lhe 0..10 -- uma faixa onde nada do que o
                            // afinador puser muda seja o que for.
                            "rfp_return_beta" => 1024,
                            // Tempo: a banda proporcional daria 0..110 ao
                            // divisor, e um divisor perto de zero gasta o
                            // relogio inteiro num lance -- perde-se por tempo
                            // antes de o afinador aprender que foi mau.
                            "tempo_divisor" => 35,
                            "tempo_fase_abertura" => 25,
                            "tempo_fase_meio_cedo" => 40,
                            "tempo_fase_meio" => 50,
                            "tempo_fase_meio_tarde" => 50,
                            "tempo_fase_simplificado" => 40,
                            "tempo_hard_cap_pct" => 15,
                            _ => (d.abs()).max(10),
                        };
                        // A default below zero exists (`hist_malus_offset`),
                        // and clamping the floor to zero puts the default
                        // OUTSIDE its own declared range. A strict UCI client
                        // rejects the option outright -- and a tuning harness
                        // that swallows that error scores every game as a draw
                        // and reports perfect convergence while learning
                        // nothing at all.
                        let lo = if d < 0 { d - band } else { (d - band).max(0) };
                        let hi = d + band;
                        let _ = writeln!(
                            out,
                            "option name {} type spin default {} min {} max {}",
                            n, d, lo, hi
                        );
                    }
                    let _ = writeln!(out, "uciok");
                    let _ = out.flush();
                }
                "isready" => {
                    let _ = writeln!(out, "readyok");
                    let _ = out.flush();
                }
                "setoption" => {
                    if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "HeatmapPlies" && tokens[3] == "value" {
                        let n: i32 = tokens[4].parse().unwrap_or(1);
                        HEATMAP_PLIES.store(n.clamp(1, 3), std::sync::atomic::Ordering::Relaxed);
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "HeatmapOnly" && tokens[3] == "value" {
                        let on = tokens[4].eq_ignore_ascii_case("true") || tokens[4] == "1";
                        HEATMAP_ONLY.store(on, std::sync::atomic::Ordering::Relaxed);
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "LazyVote"
                        && tokens[3] == "value" {
                        self.lazy_vote = tokens[4].eq_ignore_ascii_case("true") || tokens[4] == "1";
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "PeaoSemLmr"
                        && tokens[3] == "value" {
                        if let Ok(v) = tokens[4].parse::<i32>() {
                            crate::search::set_peao_fila_sem_lmr(v);
                        }
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "RitmoTecto"
                        && tokens[3] == "value" {
                        // Percentagem do ritmo do adversario que podemos gastar
                        // por lance quando NAO temos folga de relogio. Ver
                        // TECTO_RITMO_PCT_OMISSAO.
                        if let Ok(v) = tokens[4].parse::<i64>() {
                            set_tecto_ritmo(v);
                        }
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "EvalScale"
                        && tokens[3] == "value" {
                        // Exposed so it can be fitted the way every other search
                        // parameter is, by SPSA over real games, instead of by
                        // rebuilding the engine for each candidate. It is worth
                        // about two plies and nobody has ever fitted it.
                        if let Ok(v) = tokens[4].parse::<i32>() {
                            crate::nnue::set_escala(v);
                        }
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "EvalFactor"
                        && tokens[3] == "value"
                    {
                        if let Ok(v) = tokens[4].parse::<i32>() {
                            crate::nnue_sf::set_eval_factor(v);
                        }
                                        } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "Hash" && tokens[3] == "value" {
                        if let Ok(mb) = tokens[4].parse::<usize>() {
                            self.tt = TranspositionTable::new(mb.max(1));
                        }
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "Contempt"
                        && tokens[3] == "value" {
                        if let Ok(v) = tokens[4].parse::<i32>() {
                            crate::search::CONTEMPT.store(v.clamp(-200, 200),
                                std::sync::atomic::Ordering::Relaxed);
                        }
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "EscalaPorBalde"
                        && tokens[3] == "value" {
                        let on = tokens[4].eq_ignore_ascii_case("true") || tokens[4] == "1";
                        crate::nnue::set_escala_por_balde(on);
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "LmrCaptures"
                        && tokens[3] == "value" {
                        let on = tokens[4].eq_ignore_ascii_case("true") || tokens[4] == "1";
                        crate::search::set_lmr_captures(on);
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "EvalFile"
                        && tokens[3] == "value" {
                        // Junta-se tudo o que vem depois de `value`: um caminho
                        // com espacos e' vulgar em Windows ("Program Files") e
                        // ler so' `tokens[4]` truncava-o silenciosamente -- a
                        // rede nao carregava e o motor jogava sem ela.
                        let caminho = tokens[4..].join(" ");
                        if crate::nnue_sf::define_evalfile(&caminho) {
                            let _ = writeln!(out, "info string EvalFile = {}", caminho);
                        } else {
                            let _ = writeln!(
                                out,
                                "info string EvalFile ignorado: a rede ja' esta' carregada"
                            );
                        }
                        let _ = out.flush();
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "OnlineTablebase"
                        && tokens[3] == "value" {
                        let on = tokens[4].eq_ignore_ascii_case("true") || tokens[4] == "1";
                        crate::tablebase::set_enabled(on);
                    } else if tokens.len() >= 6 && tokens[1] == "name" && tokens[2] == "Move"
                        && tokens[3] == "Overhead" && tokens[4] == "value" {
                        if let Ok(ms) = tokens[5].parse::<i64>() {
                            MOVE_OVERHEAD.store(ms.clamp(0, 5000), std::sync::atomic::Ordering::Relaxed);
                        }
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[2] == "Threads" && tokens[3] == "value" {
                        if let Ok(n) = tokens[4].parse::<usize>() {
                            self.threads = n.max(1);
                        }
                    } else if tokens.len() >= 5 && tokens[1] == "name" && tokens[3] == "value"
                        && tokens[2].starts_with("ev_") {
                        // Evaluation weights by flat index, encoded in the name
                        // as `ev_<field>_<index>`. The index is what carries
                        // the meaning -- the field name is there so a human
                        // reading a tuner's output can tell what moved.
                        let idx = tokens[2].rsplit('_').next().and_then(|x| x.parse::<usize>().ok());
                        let val = tokens[4].parse::<i32>().ok();
                        match (idx, val) {
                            _ => {
                                let _ = writeln!(out, "info string unknown eval weight {}", tokens[2]);
                                let _ = out.flush();
                            }
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
                    self.opp_time_anterior = None;
                    self.opp_pace = None;
                    self.relogio_inicial = None;
                }
                "position" => {
                    self.set_position(&tokens[1..]);
                }
                "go" => {
                    self.cmd_go(&tokens[1..], &mut out);
                }
                "stop" => {}
                "fen" => {
                    // O FEN da posicao actual. Serve para gerar livros de
                    // aberturas levando posicoes mais fundo com `position ...
                    // moves ...` e lendo onde ficaram.
                    let _ = writeln!(out, "{}", self.board.to_fen());
                }
                "evalraw" => {
                    // O numero pelado, na perspectiva do LADO A JOGAR --
                    // a mesma convencao usada por outros motores UCI, para
                    // comparadores automaticos poderem ler uma linha sem
                    // parsear tabela nenhuma.
                    //
                    // `evaluate()` JA devolve o lado a jogar (e' o que a
                    // busca precisa). O comando "eval"/"evalbreak" acima
                    // fez de proposito o flip inverso para mostrar sempre
                    // do lado das brancas -- daqui NAO se copia esse flip,
                    // ou fica-se com o dobro dele, que e' voltar ao ponto
                    // de partida disfarcado de resposta.
                    let seen = crate::evaluation::evaluate(&mut self.board);
                    let _ = writeln!(out, "{}", crate::evaluation::score_normalizado(seen));
                }
                "quit" => break,
                _ => {}
            }
        }
    }
}
