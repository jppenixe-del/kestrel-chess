//! Position evaluation.
//!
//! Every score the search sees comes from the network. There is no
//! hand-written term left: no piece-square tables, no mobility counts, no king
//! safety curve, no tuned weight vector. That is a deliberate deletion, not an
//! omission -- a day of measurement showed the remaining error in the
//! hand-written evaluation was not any one mispriced term but a hundred terms
//! each slightly wrong in the same direction, which is precisely the shape of
//! error a network fixes from data and hand tuning does not.
//!
//! What stayed behind, and why it is not evaluation:
//!
//! - `PieceType::value()` in `types.rs` -- static exchange evaluation and move
//!   ordering need to know a rook outranks a knight. That is a comparison of
//!   pieces, not a judgement about a position, and it is used before any score
//!   exists to judge.
//! - `Board::phase` -- the search uses it for time and reduction decisions,
//!   not to interpolate anything.
//!
//! The network is loaded from `KESTREL_NNUE`. With no network the engine
//! cannot evaluate at all and says so loudly rather than quietly playing at
//! random: there is no fallback evaluation to hide behind any more, and a
//! silent zero would look like a drawn position from every square on the
//! board.

use crate::board::Board;

/// Score for the side to move. Halfmove-independent -- see
/// `amortece_rule50` for why that has to be true and where the scaling
/// happens instead.
///
/// Takes `&mut Board` because the piece-square accumulator is lazy: the values
/// it holds are only brought up to date here, at the one moment a score is
/// actually wanted. See `nnue::Accumulator`.
pub fn evaluate(board: &mut Board) -> i32 {
    // A li11, quando carregada, tem prioridade sobre tudo o resto -- a
    // rede-teste da noite de 2026-08-12/13, leitor validado byte a byte
    // contra o motor (acumulador, FC0, FC1 conferidos), correlacao ainda
    // fraca (0.42 sem ameacas contra 0.91 da 512) e NUNCA jogada. Atras de
    // KESTREL_NNUE_LI11 vazio por omissao -- sem a variavel, este bloco nao
    // faz nada e o caminho de sempre continua a decidir.
    // A napv10 (Nap2Siriux, rede NOSSA) tem prioridade sobre a li11 quando
    // ambas as variaveis estiverem definidas -- nao deviam estar as duas ao
    // mesmo tempo em uso a serio, mas a ordem tem de ser alguma.
    if crate::nnue_napv10::ligada() {
        if let Some(net) = crate::nnue_napv10::rede() {
            return crate::nnue_napv10::evaluate(net, board);
        }
    }
    if crate::nnue_li11::li11_ligada() {
        if let Some(net) = crate::nnue_li11::rede() {
            return crate::nnue_li11::evaluate(net, board);
        }
    }
    // The threats network takes precedence when one is loaded. Chosen by which
    // file the caller supplied rather than by a build flag, so comparing the
    // two architectures compares two networks and not two binaries.
    // v3 first when one is loaded: same rule as everywhere else here, the
    // architecture is chosen by which file was given, never by a build flag,
    // so comparing two of them compares networks and not binaries.
    if let Some(net) = crate::nnue_v3::rede() {
        // Pelo acumulador quando existe, como na rede simples: recomputar as
        // ~190 features a cada avaliacao custava 61% do tempo total (perf),
        // contra ~16% da simples.
        return match board.acc_v3.as_ref() {
            Some(acc) => acc.valor(net, board),
            None => crate::nnue_v3::evaluate(net, board),
        };
    }
    if let Some(net) = crate::nnue_threats::rede() {
        return crate::nnue_threats::evaluate(net, board);
    }
    let net = match crate::nnue::rede() {
        Some(n) => n,
        None => {
            sem_rede();
            return 0;
        }
    };
    let ob = crate::nnue::output_bucket(net, board);
    let side = board.side;
    match board.acc.as_mut() {
        // The accumulator the position carries, fed one piece at a time by
        // make_move and folded in here. Rebuilding it from scratch at this
        // point would make every node pay for all thirty-two pieces --
        // measured, that is slower than the hand-written evaluation was by
        // more than an order of magnitude.
        Some(acc) => {
            acc.materialise(net);
            crate::nnue::evaluate(net, acc, side, ob)
        }
        None => crate::nnue::evaluate_board(net, board),
    }
}

/// The same score. Kept as a separate name because the search calls it from
/// the paths where it used to matter that the evaluation was the cheap one --
/// quiescence stand-pat and the pruning margins. With a network there is only
/// one evaluation and it is already cheap, so the distinction is now a
/// courtesy to the call sites rather than two different functions.
#[inline]
pub fn evaluate_fast(board: &mut Board) -> i32 {
    evaluate(board)
}

/// `v -= v * rule50 / 199`, ported literally from `Eval::evaluate` in
/// Stockfish's `evaluate.cpp`. The network knows nothing about the 50-move
/// clock -- it is not an input feature -- so without this the search always
/// trusts the full advantage, even ten moves from bleeding out into a draw.
/// The shrink is what gives the search a reason to prefer, when ahead, a
/// move that zeroes the counter (the score snaps back to full) and to avoid
/// zeroing when behind (that would hand the opponent's own score back to
/// full too).
///
/// Deliberately NOT folded into `evaluate()` itself. `evaluate()`'s raw
/// output is what the search caches -- TT's `static_eval` field and the
/// improving heuristic's `static_evals[ply]` -- and reused across nodes that
/// share a position but not a halfmove count. Baking the scale in there
/// would freeze a value computed for one node's halfmove into a cache read
/// back by a different node's, applying the wrong shrink. Coda hit this
/// exact class of bug (their comment: "apply this at the point of use,
/// never before storing to TT") and their own conversion-failure study
/// traced won-position draws to an earlier, more aggressive version of this
/// same formula -- worth remembering before touching the constant here.
/// Callers apply this to a raw eval (freshly computed OR read back from a
/// cache) immediately before using it for a decision, with THIS node's
/// `board.halfmove`, never before storing it.
#[inline]
pub fn amortece_rule50(v: i32, halfmove: u32) -> i32 {
    v - v * halfmove as i32 / 199
}

/// Said once, on the first evaluation with no network loaded.
///
/// Loudly, and every path that could evaluate goes through here: an engine
/// that returns zero for every position looks like it thinks the game is drawn
/// rather than like it is broken, and that is the kind of failure that costs a
/// session before anyone works out what happened.
fn sem_rede() {
    static AVISADO: std::sync::Once = std::sync::Once::new();
    AVISADO.call_once(|| {
        eprintln!(
            "ERRO: nenhuma rede carregada. Define KESTREL_NNUE=<ficheiro.bin>. \
             Sem rede o motor nao sabe avaliar nada."
        );
    });
}

/// Win/draw/loss estimate from a score, for the `wdl` field the UCI protocol
/// reports.
///
/// A logistic on the score. The scale is the same one the network was trained
/// against, which is what makes this meaningful rather than decorative: the
/// training target was a win probability put through a sigmoid at this scale,
/// so inverting it recovers the probability the network was actually fitted
/// to.
pub fn win_draw_loss(score: i32) -> (i32, i32, i32) {
    // The same scale the score itself is on, read rather than written down.
    //
    // It was the constant 400, which was right for exactly as long as the
    // evaluation scale was also 400. Halving that scale halved every score
    // without touching this, so the curve was being fed numbers half the size
    // it expected and answered that every position was nearly drawn.
    //
    // Tying the two together is not tidiness. This curve is what fixes the
    // meaning of a score: a pawn is worth whatever it moves the probability of
    // winning by, and every pruning margin in the search is denominated in the
    // same units. Let the two drift apart and the engine reports a confidence
    // it does not act on.
    let escala = crate::nnue::escala() as f64;
    let w = 1.0 / (1.0 + (-(score as f64) / escala).exp());
    let l = 1.0 / (1.0 + ((score as f64) / escala).exp());
    // Draws are what is left. Modelling them separately needs a second fitted
    // curve and the number is reported, not searched on.
    let d = (1.0 - w - l).max(0.0);
    let total = w + d + l;
    (
        (1000.0 * w / total).round() as i32,
        (1000.0 * d / total).round() as i32,
        (1000.0 * l / total).round() as i32,
    )
}

/// The engine's internal score is already in centipawns, so this is the
/// identity. Kept as a named function because the UCI layer calls it wherever
/// a score crosses the protocol boundary: when the network's output scale is
/// next changed, this is the one place that has to know.
#[inline]
pub fn score_normalizado(interno: i32) -> i32 {
    interno
}

/// Build the globals before the first search rather than during it.
///
/// The attack tables and the network are both built lazily, and paying for
/// them inside the first `go` shows up as a move that took most of a second to
/// think about nothing. This is called once when the engine starts.
pub fn warmup() {
    let _ = atk();
    // EVERY architecture, not just the piece-square one. Each `rede()` is a
    // separate OnceLock, so loading one leaves the others to be decoded
    // inside the first search that asks for a score -- and the threats
    // network is 16.7 MB of LEB128, measured at ~1.6s to decode. A bullet
    // game cannot pay that on its first move, which is the same failure that
    // once cost six losses a day when the warmup did not run at all.
    //
    // Cheap when a network is absent: `rede()` returns None without reading
    // anything when its variable is unset and nothing is embedded.
    let _ = crate::nnue::rede();
    let _ = crate::nnue_threats::rede();
    let _ = crate::nnue_v3::rede();
}

/// The attack tables, built once.
///
/// They live here rather than in the network module because move generation
/// and the search need them too -- they are geometry, not evaluation, and they
/// outlast every scoring scheme the engine has had.
static ATTACKS: std::sync::OnceLock<crate::attacks::Attacks> = std::sync::OnceLock::new();

pub fn atk() -> &'static crate::attacks::Attacks {
    ATTACKS.get_or_init(crate::attacks::Attacks::new)
}
