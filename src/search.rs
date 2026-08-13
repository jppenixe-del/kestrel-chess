use crate::attacks::{bishop_attacks, rook_attacks, Attacks};
use crate::bitboard::{bb, Bitboard};
use crate::board::Board;
use crate::book::{encode_move, Book};
use crate::evaluation::evaluate;
use crate::movegen::{generate_legal, generate_legal_caps};
use crate::moves::{Move, MoveFlag};
use crate::tt::{Bound, TranspositionTable};
use crate::types::{file_of, rank_of, sq, Color, PieceType};
use crate::zobrist::Zobrist;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// LMR reduction table indexed by [depth][move_index], both clamped to
/// 63. Logarithmic formula (standard shape used by essentially every
/// modern alpha-beta engine): reduction grows with ln(depth)*ln(count),
/// smooth instead of the old fixed tiers (+1/+2/+3 at hard thresholds).
/// Computed once at first use, cheap (64*64 entries).
///
/// The divisor (2.1) is A/B-testable via `KESTREL_LMR_DIVISOR`, same
/// reversible opt-in pattern as `KESTREL_EVAL_MODE`/`KESTREL_TUNED_WEIGHTS`:
/// unset (every real deployment) reproduces the compiled-in default
/// bit-for-bit. Smaller divisor -> larger reduction -> more aggressive
/// pruning. A previous A/B attempt at tuning this used ad-hoc scratch
/// binaries whose provenance couldn't be reconstructed reliably -- this
/// env var replaces that with a reproducible single-binary comparison.
static LMR_TABLE: OnceLock<[[i32; 64]; 64]> = OnceLock::new();

/// A partir de que fila (contada do lado de quem joga) um lance de peao deixa
/// de ser reduzido pelo LMR. 6 = sexta e setima filas. 0 desliga a regra.
///
/// Porque existe: a avaliacao nao ve peoes passados. Nao ha feature nenhuma
/// nas 768 entradas que diga "este peao esta passado" ou "faltam-lhe duas
/// casas" -- ve peca e casa, e se o peao esta livre depende dos peoes do
/// adversario em tres colunas, que uma camada so' infere mal.
///
/// Medido nas nossas proprias posicoes, contra o Stockfish em 6000 posicoes
/// etiquetadas, o erro medio da avaliacao por distancia a promocao:
///
///     sem passados   94 cp        a 2 filas   128 cp
///     a 3 filas     118 cp        a 1 fila    158 cp
///
/// Uma posicao com um peao a uma casa de promover e' avaliada com 68% mais
/// erro do que uma sem passados -- e sao posicoes de resposta binaria, ou se
/// para o peao ou se perde. O mesmo padrao existe na rede anterior, portanto
/// nao e' regressao: e' a arquitectura.
///
/// Se a avaliacao nao sabe que a linha e' critica, o LMR reduz-a como reduz
/// qualquer lance quieto tardio, e a promocao cai para alem do horizonte. Nao
/// reduzir e' a correccao barata: nao toca na rede e devolve a essas linhas a
/// profundidade que a avaliacao nao sabe pedir.
///
/// LIGADO a 6 (sexta e setima filas) depois de medido: +9.9 +/- 13.3 Elo,
/// LOS 92.7%, em 2000 jogos a 5+0.05.
///
/// Nao decidiu formalmente -- o LLR parou nos 0.8 de 2.2 -- e adopta-se na
/// mesma porque o efeito tem um mecanismo medido por FORA do jogo: o erro da
/// avaliacao cresce 94 -> 158 cp conforme o peao se aproxima da promocao, nas
/// duas redes. Nao e' um numero bonito a procura de explicacao; e' uma
/// explicacao que produziu o numero previsto.
static PEAO_FILA_SEM_LMR: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(6);

pub fn peao_fila_sem_lmr() -> i32 {
    PEAO_FILA_SEM_LMR.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_peao_fila_sem_lmr(v: i32) {
    PEAO_FILA_SEM_LMR.store(v.clamp(0, 8), std::sync::atomic::Ordering::Relaxed);
}

/// Este lance leva um peao a uma fila avancada?
///
/// Chamado DEPOIS de `make_move`, portanto quem jogou e' `board.side.opp()` e
/// a peca ja esta em `mv.to` -- as duas coisas que o bug de 2026-07-25 nesta
/// mesma funcao de reducao apanhou da maneira dificil.
#[inline]
fn peao_avancado(board: &crate::board::Board, mv: &crate::moves::Move) -> bool {
    let fila_min = peao_fila_sem_lmr();
    if fila_min == 0 {
        return false;
    }
    let mover = board.side.opp();
    match board.piece_at(mv.to) {
        Some((crate::types::PieceType::Pawn, c)) if c == mover => {
            let r = (mv.to as i32) / 8;                  // 0..7, absoluta
            let rel = if mover == crate::types::Color::White { r } else { 7 - r };
            rel + 1 >= fila_min                          // rel 0 = 1a fila
        }
        _ => false,
    }
}

/// Same reasoning as `evaluation::warmup`: build the search-side globals before
/// the clock matters, not inside the first search.
pub fn warmup() {
    let _ = lmr_table();
    // Deliberately NOT `search_params()`. It is a OnceLock, so the first read
    // fixes it for the life of the process -- warming it here means every
    // `setoption` that arrives afterwards is folded into a value nobody will
    // ever read again. The engine accepts the option, reports nothing wrong,
    // and searches with the old number: a sweep of five values returns five
    // identical results and looks like a finding. Building it lazily costs one
    // branch on the first node.
}
fn lmr_table() -> &'static [[i32; 64]; 64] {
    LMR_TABLE.get_or_init(|| {
        let divisor = std::env::var("KESTREL_LMR_DIVISOR")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(2.1);
        if divisor != 2.1 {
            eprintln!("KESTREL_LMR_DIVISOR: using {} (default 2.1)", divisor);
        }
        let mut t = [[0i32; 64]; 64];
        for d in 1..64 {
            for m in 1..64 {
                let r = 0.5 + (d as f64).ln() * (m as f64).ln() / divisor;
                // Stored in MILLI-PLIES (see `LMR_ESCALA`), not whole plies.
                // Same curve as before, same divisor -- only the resolution
                // changes. Truncating this to an integer here was discarding
                // 42% of the reduction the formula asks for at depth 5,
                // move 5 (1.733 -> 1), and 21% at depth 14 (3.816 -> 3).
                t[d][m] = (r * LMR_ESCALA as f64).round() as i32;
            }
        }
        t
    })
}

/// Fixed-point scale for LMR reductions: they accumulate in 1/1024 of a ply
/// and are divided down to whole plies ONCE, at the end.
///
/// Every modulator used to be rounded to a whole ply on its own -- the base
/// truncated from its float, history integer-divided, the rest a flat +/-1 --
/// so each term could only say "nothing" or "a whole ply". Measured on our own
/// constants, that threw away up to 42% of the base reduction and silenced the
/// history term entirely below h=8846 (a move at 8845, over half of
/// HISTORY_MAX, asked for -1.000 ply and got 0).
///
/// The mechanism is common practice, arrived at independently by every engine
/// that carries several modulators: Stockfish (GPL-3.0) and Coda (GPL-3.0)
/// both accumulate this way, as does our own earlier napv10 engine -- the
/// first two in 1/1024, Coda in 1/100. The scale is arbitrary; 1024 is chosen
/// here because it makes the final division a shift.
///
/// The constants below are NOT taken from any of them. Every term keeps the
/// value this engine already had tuned (divisor 2.1, history divisor 8846,
/// the +/-1 ply caps), expressed exactly instead of rounded.
const LMR_ESCALA: i32 = 1024;

/// Reduction subtracted per move index, in milli-plies (`r -= c * i`).
///
/// Our base curve grows as `ln(depth) * ln(move)` and never flattens, so the
/// deeper into a move list we go the harder we cut -- by the 20th move we
/// were reducing ~0.6 ply more than a reference engine does at the same
/// point, which is where a late tactic stops being seen at all. A linear
/// term is what bends that tail back.
///
/// Fitted against the reference SHAPE, not copied from it: the coefficient
/// that maps OUR curve onto that shape is ~31 milli-plies per move, half the
/// value that engine applies to its own (differently-shaped) base curve.
/// Compile-time so each SPRT arm is its own binary; 0 = off.
const LMR_MOVE_LINEAR: i32 = match i32::from_str_radix(env!("KESTREL_LMR_MOVE_LINEAR_COMPILADO"), 10) {
    Ok(v) => v,
    Err(_) => 0,
};

/// Extra reduction at a cutnode, in milli-plies.
///
/// A cutnode is where a fail-high is expected, so it is the one place a
/// deeper cut costs least -- every engine that carries the signal reduces
/// harder there. Tried here once as a flat +2 whole plies and it was
/// catastrophic; the term only makes sense as a fraction, which the
/// fixed-point accumulator now allows. Compile-time; 0 = off.
const LMR_CUTNODE: i32 = match i32::from_str_radix(env!("KESTREL_LMR_CUTNODE_COMPILADO"), 10) {
    Ok(v) => v,
    Err(_) => 0,
};

static ROOT_TRACE: OnceLock<bool> = OnceLock::new();
/// Is the root trace switched on? Read once -- the check sits in the root
/// move loop, so it must not cost an environment lookup per move.
fn root_trace() -> bool {
    *ROOT_TRACE.get_or_init(|| std::env::var("KESTREL_ROOT_TRACE").is_ok())
}

pub const MATE_SCORE: i32 = 30000;
/// A root move that failed low this iteration has no usable score.
pub const NO_SCORE: i32 = -MATE_SCORE - 100;
pub const MAX_PLY: usize = 128;

/// Percent multiplier for the eval-margin pruning thresholds (RFP, NMP,
/// razoring, futility) -- 100 = unchanged. A foreign network read through
/// our own port has a different noise/volatility profile than the one
/// these margins were tuned against; the Nap2Siriux reference documents
/// exactly this (`search_params.h`: separate, ~1.5x wider `_NNUE` margins,
/// "the HCE margins fire too early" against their own bullet-WDL net).
/// We do not have a second network to split constants against the way
/// they do -- one pragmatic knob instead of relearning their whole
/// parameter set, widening every eval-margin site the same way while this
/// gets measured.
#[inline]
pub fn eval_margin_scale() -> i32 {
    // 2026-08-13: tried widening by 1.5x (the ratio dual_vision.h uses)
    // and measured WORSE (15% vs 27.5% vs the same SF1800 baseline
    // without this) -- their search is multi-threaded with heavy SIMD in
    // the dense forward pass and can afford to explore more per node;
    // ours, already slower per node on this network, only loses
    // effective depth by widening. Off until the cause is better
    // understood.
    let _ = crate::nnue_napv10_ffi::active();
    100
}

/// Every scalar pruning margin/threshold in the search, in one runtime-
/// swappable place -- same reversible pattern as `Weights`/
/// `KESTREL_TUNED_WEIGHTS` in eval.rs, but for the SEARCH side. Before
/// this, these were scattered `const`s and inline literals (RFP margin,
/// razoring, futility x2, qsearch delta pruning, qsearch LMP, TT
/// extended cutoff, history pruning) with no way to swap them without
/// editing and recompiling -- noticed while building an eval "profile"
/// that the search side had no equivalent, even though these margins
/// deserve tuning just as much (each carries a measurable Elo value
/// under SPSA, see NOTAS_PROXIMA_SESSAO.md). `KESTREL_SEARCH_PARAMS=
/// <path>` loads a `to_vec()`-shaped file the same way
/// `KESTREL_TUNED_WEIGHTS` does; unset reproduces every default exactly.
/// No coordinate-descent tuner for this yet (these interact with node
/// counts nonlinearly -- the static-position tuning method doesn't apply,
/// real tuning here needs SPSA over actual games, same self-play
/// infrastructure this session already uses for A/B validation) -- this
/// commit only makes the values swappable, doesn't add a tuner.
/// Margin shape `base + slope*depth`, generalizing the old pure-
/// multiplier form (`slope*depth`, base=0 -- what every Kestrel default
/// already was, so this changes zero behavior by default) to also
/// represent margins that have a flat component (e.g. a quiet futility
/// of the form `77 + lmrDepth*52`). The pure-multiplier form is just the
/// base=0 special case, so either shape can be expressed exactly.
#[derive(Clone, Copy)]
pub struct DepthMargin {
    pub base: i32,
    pub slope: i32,
}

/// Static Exchange Evaluation, as free functions.
///
/// Moved out of `Searcher` so the EVALUATION can use it too. The static
/// evaluation had no way to price a piece that is about to be captured: a
/// bishop left en prise scored a flat -58 penalty when the bishop is worth
/// 355, so a position that was really -215 evaluated as -73. That 142-point
/// error is larger than every pruning margin we have -- 35 per ply for RFP,
/// 265 for null move -- which means whole-node pruning was deciding on a
/// number that could be wrong by more than the margin it was compared against.
///
/// Nothing here needs searcher state; the only dependency was the attack
/// tables, which are a global.
pub mod see {
    use crate::attacks::{bishop_attacks, rook_attacks, Attacks};
    use crate::board::Board;
    use crate::bitboard::*;
    use crate::moves::{Move, MoveFlag};
    use crate::types::{file_of, rank_of, sq, Color, PieceType, Square};

    /// Static Exchange Evaluation: simula a sequencia completa de
    /// capturas/recapturas na casa `mv.to`, sempre com o atacante menos
    /// valioso de cada lado (a jogada optima para ambos), e devolve o
    /// ganho material líquido assumindo optimo jogo de ambos os lados
    /// (cada lado escolhe parar ou continuar a troca, o que for melhor
    /// para si -- minimax classico sobre a "swap list"). Nao verifica
    /// se a recaptura deixaria o proprio rei em xeque (limitacao
    /// standard/aceite de SEE simples, presente em praticamente todos
    /// os motores). So' chamar em lances de captura (incl. en passant).
    pub fn see(a: &Attacks, board: &Board, mv: &Move) -> i32 {
        let to = mv.to;
        let Some((attacker_pt0, attacker_color0)) = board.piece_at(mv.from) else {
            return 0;
        };
        let victim_val0 = if mv.flag == MoveFlag::EnPassant {
            PieceType::Pawn.value()
        } else {
            match board.piece_at(to) {
                Some((pt, _)) => pt.value(),
                // Quiet move: nothing is captured, so the exchange starts
                // at zero -- but the sequence below still runs, because the
                // piece we just moved can be taken on `to`. That makes this
                // a general "does this move lose material?" test rather
                // than a capture-only one (it used to bail out with 0 here,
                // which silently made any SEE-based test on a quiet move a
                // no-op). Every existing caller guards on is_capture(), so
                // their behaviour is unchanged.
                None => 0,
            }
        };

        let mut occ = board.occ_all;
        occ &= !bb(mv.from);
        if mv.flag == MoveFlag::EnPassant {
            let ep_captured = sq(file_of(to), rank_of(mv.from));
            occ &= !bb(ep_captured);
        }

        // Era um Vec com capacidade 1: uma alocacao no heap por CHAMADA, e
        // cada push a realocar (1->2->4->8). SEE e' chamado na ordenacao de
        // lances, na poda e na avaliacao de pecas penduradas -- milhoes de
        // vezes por segundo. A sequencia de trocas tem um tecto de 32, por
        // isso cabe na pilha e nunca precisou do heap.
        let mut gains = [0i32; 34];
        gains[0] = victim_val0;
        let mut n_gains = 1usize;
        let mut attacker_val = attacker_pt0.value();
        let mut side = attacker_color0.opp();

        // Os atacantes eram revarridos DO ZERO a cada troca -- ate' 32 vezes
        // por SEE, cada uma com duas consultas de peao, uma de cavalo, uma de
        // rei, duas magias e oito ORs para montar as mascaras. Mas ao tirar
        // uma peca do tabuleiro so' um deslizante pode passar a atacar a casa
        // (a bateria atras dele); cavalos, peoes e reis nunca aparecem de
        // novo. Entao calcula-se o conjunto uma vez e a seguir so' se
        // reexaminam os deslizantes, com as mascaras montadas ca' fora.
        let diag = board.pieces[Color::White.idx()][PieceType::Bishop.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Bishop.idx()]
            | board.pieces[Color::White.idx()][PieceType::Queen.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Queen.idx()];
        let orth = board.pieces[Color::White.idx()][PieceType::Rook.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Rook.idx()]
            | board.pieces[Color::White.idx()][PieceType::Queen.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Queen.idx()];
        let mut attackers = attackers_to(a, board, to, occ);
        loop {
            let side_attackers = attackers & board.occ_color[side.idx()];
            let Some((lva_sq, lva_pt)) = least_valuable_attacker(board, side_attackers, side) else {
                break;
            };
            gains[n_gains] = attacker_val - gains[n_gains - 1];
            n_gains += 1;
            attacker_val = lva_pt.value();
            occ &= !bb(lva_sq);
            // Tira o atacante usado e junta o que ele tapava.
            attackers |= (bishop_attacks(to, occ) & diag) | (rook_attacks(to, occ) & orth);
            attackers &= occ;
            side = side.opp();
            if n_gains > 32 {
                break;
            }
        }

        for i in (1..n_gains).rev() {
            gains[i - 1] = (-gains[i]).min(gains[i - 1]);
        }
        gains[0]
    }
    /// `see(mv) >= limiar`, mas sem calcular o valor exacto.
    ///
    /// O SEE completo constroi a sequencia de trocas toda e so' no fim, na
    /// passagem inversa, e' que sabe o resultado. Quando a pergunta e' apenas
    /// "isto passa a fasquia?" -- e e' o que a maioria dos sitios pergunta:
    /// `>= 0` na quiescencia, `< see_allowance` na poda -- a resposta costuma
    /// ficar decidida a' primeira ou segunda troca. Aqui leva-se um valor
    /// corrente com o truque negamax (`valor = peca - valor`) e sai-se assim
    /// que o sinal deixa de poder mudar.
    ///
    /// Tem de concordar com `see(..) >= limiar` em TODAS as posicoes; se
    /// discordar numa que seja, a contagem de nos do bench muda.
    pub fn see_ge(a: &Attacks, board: &Board, mv: &Move, limiar: i32) -> bool {
        let to = mv.to;
        let Some((attacker_pt0, attacker_color0)) = board.piece_at(mv.from) else {
            return 0 >= limiar;
        };
        let victim_val0 = if mv.flag == MoveFlag::EnPassant {
            PieceType::Pawn.value()
        } else {
            match board.piece_at(to) {
                Some((pt, _)) => pt.value(),
                None => 0,
            }
        };

        let mut valor = victim_val0 - limiar;
        if valor < 0 {
            return false;
        }
        valor = attacker_pt0.value() - valor;
        if valor <= 0 {
            return true;
        }

        let mut occ = board.occ_all;
        occ &= !bb(mv.from);
        if mv.flag == MoveFlag::EnPassant {
            let ep_captured = sq(file_of(to), rank_of(mv.from));
            occ &= !bb(ep_captured);
        }

        let diag = board.pieces[Color::White.idx()][PieceType::Bishop.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Bishop.idx()]
            | board.pieces[Color::White.idx()][PieceType::Queen.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Queen.idx()];
        let orth = board.pieces[Color::White.idx()][PieceType::Rook.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Rook.idx()]
            | board.pieces[Color::White.idx()][PieceType::Queen.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Queen.idx()];
        let mut attackers = attackers_to(a, board, to, occ);

        let mut side = attacker_color0.opp();
        let mut res = true;
        loop {
            attackers &= occ;
            let side_attackers = attackers & board.occ_color[side.idx()];
            let Some((lva_sq, lva_pt)) = least_valuable_attacker(board, side_attackers, side)
            else {
                break;
            };
            res = !res;
            valor = lva_pt.value() - valor;
            // `res` diz de quem e' a vez de ficar a ganhar: o limite muda de
            // 0 para 1 conforme o lado, tal como na formulacao classica.
            if valor < res as i32 {
                break;
            }
            occ &= !bb(lva_sq);
            attackers |= (bishop_attacks(to, occ) & diag) | (rook_attacks(to, occ) & orth);
            side = side.opp();
        }
        res
    }

    pub fn attackers_to(a: &Attacks, board: &Board, s: crate::types::Square, occ: crate::bitboard::Bitboard) -> crate::bitboard::Bitboard {
        let a = a;
        let mut att = 0u64;
        att |= a.pawn[Color::Black.idx()][s as usize] & board.pieces[Color::White.idx()][PieceType::Pawn.idx()];
        att |= a.pawn[Color::White.idx()][s as usize] & board.pieces[Color::Black.idx()][PieceType::Pawn.idx()];
        att |= a.knight[s as usize]
            & (board.pieces[Color::White.idx()][PieceType::Knight.idx()] | board.pieces[Color::Black.idx()][PieceType::Knight.idx()]);
        att |= a.king[s as usize]
            & (board.pieces[Color::White.idx()][PieceType::King.idx()] | board.pieces[Color::Black.idx()][PieceType::King.idx()]);
        let diag = board.pieces[Color::White.idx()][PieceType::Bishop.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Bishop.idx()]
            | board.pieces[Color::White.idx()][PieceType::Queen.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Queen.idx()];
        att |= bishop_attacks(s, occ) & diag;
        let orth = board.pieces[Color::White.idx()][PieceType::Rook.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Rook.idx()]
            | board.pieces[Color::White.idx()][PieceType::Queen.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Queen.idx()];
        att |= rook_attacks(s, occ) & orth;
        att & occ
    }
    pub fn least_valuable_attacker(
        board: &Board,
        attackers: crate::bitboard::Bitboard,
        side: Color,
    ) -> Option<(crate::types::Square, PieceType)> {
        for pt in [
            PieceType::Pawn,
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Rook,
            PieceType::Queen,
            PieceType::King,
        ] {
            let bbp = attackers & board.pieces[side.idx()][pt.idx()];
            if bbp != 0 {
                return Some((bbp.trailing_zeros() as crate::types::Square, pt));
            }
        }
        None
    }
}

impl DepthMargin {
    #[inline]
    pub fn at(&self, depth: i32) -> i32 {
        self.base + self.slope * depth
    }
}

#[derive(Clone, Copy)]
pub struct SearchParams {
    /// RFP margin, quadratic in depth: `step*d*d/2 - step*d/2 + base*d`.
    /// 2026-08-03: adopted from a reference engine's own RFP wholesale --
    /// not just the numbers, the SHAPE. That reference does not split on
    /// `improving` and has none of the three modulators below; one curve,
    /// unconditional. `rfp_base`/`rfp_step` are close to identical to its
    /// own constants (65 and 5). The three modulators and the old linear
    /// `rfp_improving`/`rfp_not_improving` pair stay in the struct --
    /// removing fields would shift every index after them in `to_vec` and
    /// break unrelated UCI tuning options -- but RFP itself no longer reads
    /// them.
    pub rfp_base: i32,
    pub rfp_step: i32,
    /// Extra margin per depth when the opponent has a capture that wins
    /// material outright. See the note at the RFP block.
    pub rfp_opp_easy_capture: i32,
    /// Margin removed when the opponent's position is deteriorating.
    pub rfp_opp_worsening: i32,
    /// Divides the previous move's continuation-history score into the margin.
    pub rfp_hist_divisor: i32,
    /// Slack above beta that promotes a cutoff to a deeper history bonus.
    pub hist_beta_margin: i32,
    /// Depth ceiling for history pruning. The reference this concept comes
    /// from prunes up to 7 and then BREAKS out of the move loop; we cannot
    /// break, because our ordering mixes countermove and book bonuses into
    /// the quiet score and places losing captures AFTER quiets, so leaving
    /// the loop on one bad quiet would also discard every sacrifice behind
    /// it. Skipping one move at a time is the safe half of the mechanism;
    /// this is the half that was left at 4 without ever being measured.
    pub hist_pruning_max_depth: i32,
    /// Extra slack below s_beta that turns a double extension into a triple.
    pub triple_ext_margin: i32,
    /// How far below zero a move's history must sit to be skipped in quiescence.
    pub qs_hist_prune_margin: i32,
    pub hist_bonus_quad: i32,
    pub hist_bonus_linear: i32,
    pub hist_bonus_offset: i32,
    pub hist_bonus_max: i32,
    pub hist_malus_quad: i32,
    pub hist_malus_linear: i32,
    pub hist_malus_offset: i32,
    pub hist_malus_max: i32,
    /// Divides continuation history into the LMR reduction step.
    pub lmr_hist_divisor: i32,
    pub rfp_improving: DepthMargin,
    pub rfp_not_improving: DepthMargin,
    pub razor_base: i32,
    pub razor_per_depth: i32,
    pub futility_improving: DepthMargin,
    pub futility_not_improving: DepthMargin,
    pub cap_futility_improving: DepthMargin,
    pub cap_futility_not_improving: DepthMargin,
    /// Quiescence delta pruning margin (both the negamax entry point and
    /// the tuning-dataset quiescence_leaf path use the same value).
    pub delta_margin: i32,
    pub qs_lmp_limit: i32,
    pub tt_extended_cutoff_margin: i32,
    /// History pruning threshold multiplier: a quiet move is skipped
    /// outright (not even reduced-searched) when its history score is
    /// below `-history_prune_mult * depth`.
    pub history_prune_mult: i32,
    /// Null-move pruning gating/reduction, driven by an eval-adaptive
    /// formula -- see NMP block in negamax(). Previously a flat
    /// depth>6?3:2 reduction with no eval awareness at all; this is a
    /// genuinely different (more capable) mechanism, not just
    /// recalibrated constants.
    pub nmp_min_depth: i32,
    pub nmp_eval_margin: i32,
    pub nmp_static_eval_base_margin: i32,
    pub nmp_static_eval_depth_margin: i32,
    pub nmp_base_reduction: i32,
    pub nmp_depth_reduction_scale: i32,
    pub nmp_eval_reduction_scale: i32,
    pub nmp_max_eval_reduction: i32,
    /// ProbCut margin above beta for the cheap verification search
    /// (was a hardcoded `beta + 150`).
    pub probcut_beta_margin: i32,
    /// Aspiration window fields. Reverted once in 2026-07 after a single
    /// A/B of one integration measured 39%, then adopted again in 2026-07-27
    /// as part of the whole mechanism rather than as a swap of the widening
    /// formula alone -- the starting width, the response to each kind of
    /// failure and the growth rate are one thing, and testing them apart
    /// tests none of them.
    pub asp_init_delta: i32,
    pub asp_widening_factor: i32,
    pub min_asp_depth: i32,
    /// doDeeper/doShallower margins -- 2026-07-23: the MECHANISM is a
    /// real technique (adjust the LMR re-search depth by +/-1 based on
    /// how far the reduced search beat alpha, relative to this node's
    /// best score so far). First attempt used raw margins (36/141/8)
    /// carried over from an eval whose centipawn scale is ~1.92x smaller
    /// than ours (pawn 65 vs Kestrel's 125), and bisection localized it
    /// as the day's single biggest regression (-6.2%): the margins
    /// compare against SCORES in KESTREL's eval units, so at that scale
    /// they fired "go deeper" ~2x too eagerly -> unsound extra depth.
    /// This is a CALIBRATION error, not a wrong mechanism (user's point
    /// 2026-07-23: "não são as funções que estão mal, mas a calibração
    /// dos valores"). Fixed by rescaling the raw margins (36/141/8) to
    /// Kestrel's eval scale. Factor picked EMPIRICALLY, not assumed: swept 1.7/1.8/1.9
    /// (user's request) -- all clustered 57-59% vs 0c1b388, 1.8 best
    /// (59.0%, reproduced in two independent 200-game runs). Final:
    /// 36*1.8=65, 141*1.8=254, 8*1.8=14. NOTE: rescaling was applied
    /// ONLY to doDeeper -- a parallel test rescaling ALL eval-unit
    /// search margins (RFP/razor/NMP/ProbCut) by 1.8 scored WORSE
    /// (54.2% vs doDeeper-only 59.0%), so the calibration bug was
    /// specific to doDeeper (whose effect ADDS depth -- dangerous when
    /// it over-fires); the pruning margins were already ~fine at their
    /// raw values (matching their individually-neutral A/Bs). Lesson:
    /// calibration is per-parameter and empirical, not one blanket
    /// rescale factor.
    pub do_deeper_margin_base: i32,
    pub do_deeper_margin_depth: i32,
    pub do_shallower_margin: i32,
}

impl Default for SearchParams {
    fn default() -> Self {
        // 2026-07-22: rfp_improving/rfp_not_improving, razor_base/
        // razor_per_depth, cap_futility_improving/not_improving and
        // history_prune_mult are SPSA-tuned margin values adopted where
        // their formula genuinely matches this shape. A/B (300 games,
        // 30000 nodes/move) came back exactly neutral -- 50.0%/50.0%,
        // W138-L138-D24 -- against our own previously hand-set values,
        // so a calibrated set replaces a guess rather than being
        // discarded for lack of a positive delta ("os testes sao so'
        // para verificar, e' sempre para implementar").
        SearchParams {
            // Close to identical to a reference engine's own constants (65
            // and 5) -- see the field doc comment for why the shape, not
            // just the numbers, was adopted.
            rfp_base: 75,
            rfp_step: 4,
            // Reasoned from this engine's own history scale (see the note
            // at the RFP block), not copied. Starting points, not tuned
            // values -- exposed by name so they can be swept without a
            // rebuild.
            rfp_opp_easy_capture: 15,
            rfp_opp_worsening: 12,
            rfp_hist_divisor: 150,
            hist_beta_margin: 46,
            hist_pruning_max_depth: 4,
            triple_ext_margin: 155,
            qs_hist_prune_margin: 6144,
            hist_bonus_quad: 439,
            hist_bonus_linear: 196,
            hist_bonus_offset: 100,
            hist_bonus_max: 2121,
            hist_malus_quad: 235,
            hist_malus_linear: 277,
            hist_malus_offset: -44,
            hist_malus_max: 992,
            lmr_hist_divisor: 36000,
            // 2026-08-03: tried lowering these to a reference's raw base
            // slope (26/85) and measured a real loss (-102 Elo, LOS 0.6%).
            // Reading the reference's own formula afterward explained why:
            // its base slope is modulated by three more terms (an easy-
            // capture bonus, an opponent-worsening discount, a history
            // term) that this engine already has too -- `rfp_opp_easy_
            // capture`, `rfp_opp_worsening`, `rfp_hist_divisor` above, added
            // in an earlier session by reading the same reference. Those
            // three were tuned TOGETHER with these slopes at 50/159. Taking
            // just the base slope from the reference and leaving the three
            // modulating terms at values tuned for a different base broke
            // the coherence of an already-adapted formula. These two fields
            // are dead code now regardless -- the RFP block below reads
            // `rfp_base`/`rfp_step` (a different reference's simpler,
            // unconditional curve, which measured a real win, +46 Elo).
            // Left at the values the modulated formula was last tuned
            // around, in case a future measurement wants that shape back.
            rfp_improving: DepthMargin { base: 0, slope: 50 },
            rfp_not_improving: DepthMargin { base: 0, slope: 159 },
            razor_base: 629,
            razor_per_depth: 629,
            // SPSA do OpenBench (teste #3), leitura aos 531 018 jogos.
            //
            // Os valores anteriores eram uma leitura INTERMEDIA da mesma
            // corrida; ela continuou a andar e estes sao os ultimos
            // registados. Nao sao finais -- a corrida nunca convergiu porque
            // foi parada -- mas sao estritamente mais informados do que os
            // que substituem, que e' o criterio que a casa usa para adoptar
            // ("os testes sao so' para verificar, e' sempre para
            // implementar").
            //
            // AFINADOS PARA A rede_bot v1. A rede 512 nova e' a MESMA
            // arquitectura com mais dados, portanto herda-os razoavelmente.
            // A arquitectura de ameacas nao: enumera outras features e le'
            // noutra escala, e estas margens nao lhe dizem respeito -- e' uma
            // afinacao por fazer, nao uma que se aproveite.
            futility_improving: DepthMargin { base: 2, slope: 114 },
            futility_not_improving: DepthMargin { base: 1, slope: 114 },
            cap_futility_improving: DepthMargin { base: 1, slope: 186 },
            cap_futility_not_improving: DepthMargin { base: 2, slope: 97 },
            delta_margin: 275,
            qs_lmp_limit: 8,
            tt_extended_cutoff_margin: 162,
            history_prune_mult: 2472,
            // Same adoption rationale as above -- eval-adaptive NMP is a
            // strictly more informed mechanism than the old flat
            // depth>6?3:2 reduction, and there was no Kestrel-tuned
            // value to compare against for these fields at all.
            nmp_min_depth: 2,
            nmp_eval_margin: 40,
            nmp_static_eval_base_margin: 265,
            nmp_static_eval_depth_margin: 22,
            nmp_base_reduction: 1343,
            nmp_depth_reduction_scale: 78,
            nmp_eval_reduction_scale: 208,
            nmp_max_eval_reduction: 4,
            probcut_beta_margin: 251,
            asp_init_delta: 12,
            asp_widening_factor: 46,
            min_asp_depth: 6,
            do_deeper_margin_base: 81,
            do_deeper_margin_depth: 318,
            do_shallower_margin: 18,
        }
    }
}

impl SearchParams {
    pub fn to_vec(&self) -> Vec<i32> {
        vec![
            self.rfp_improving.base,
            self.rfp_improving.slope,
            self.rfp_not_improving.base,
            self.rfp_not_improving.slope,
            self.razor_base,
            self.razor_per_depth,
            self.futility_improving.base,
            self.futility_improving.slope,
            self.futility_not_improving.base,
            self.futility_not_improving.slope,
            self.cap_futility_improving.base,
            self.cap_futility_improving.slope,
            self.cap_futility_not_improving.base,
            self.cap_futility_not_improving.slope,
            self.delta_margin,
            self.qs_lmp_limit,
            self.tt_extended_cutoff_margin,
            self.history_prune_mult,
            self.nmp_min_depth,
            self.nmp_eval_margin,
            self.nmp_static_eval_base_margin,
            self.nmp_static_eval_depth_margin,
            self.nmp_base_reduction,
            self.nmp_depth_reduction_scale,
            self.nmp_eval_reduction_scale,
            self.nmp_max_eval_reduction,
            self.probcut_beta_margin,
            self.asp_init_delta,
            self.asp_widening_factor,
            self.min_asp_depth,
            self.do_deeper_margin_base,
            self.do_deeper_margin_depth,
            self.do_shallower_margin,
            // Appended, never inserted: `from_vec` reads this vector by
            // position, so putting a new parameter anywhere but the end
            // silently shifts every one after it. It compiles, and every
            // margin in the search quietly becomes a different margin.
            self.rfp_opp_easy_capture,
            self.rfp_opp_worsening,
            self.rfp_hist_divisor,
            self.hist_beta_margin,
            self.hist_pruning_max_depth,
            self.triple_ext_margin,
            self.qs_hist_prune_margin,
            self.hist_bonus_quad,
            self.hist_bonus_linear,
            self.hist_bonus_offset,
            self.hist_bonus_max,
            self.hist_malus_quad,
            self.hist_malus_linear,
            self.hist_malus_offset,
            self.hist_malus_max,
            self.lmr_hist_divisor,
            self.rfp_base,
            self.rfp_step,
        ]
    }
    pub fn from_vec(v: &[i32]) -> Self {
        SearchParams {
            rfp_improving: DepthMargin { base: v[0], slope: v[1] },
            rfp_not_improving: DepthMargin { base: v[2], slope: v[3] },
            razor_base: v[4],
            razor_per_depth: v[5],
            futility_improving: DepthMargin { base: v[6], slope: v[7] },
            futility_not_improving: DepthMargin { base: v[8], slope: v[9] },
            cap_futility_improving: DepthMargin { base: v[10], slope: v[11] },
            cap_futility_not_improving: DepthMargin { base: v[12], slope: v[13] },
            delta_margin: v[14],
            qs_lmp_limit: v[15],
            tt_extended_cutoff_margin: v[16],
            history_prune_mult: v[17],
            nmp_min_depth: v[18],
            nmp_eval_margin: v[19],
            nmp_static_eval_base_margin: v[20],
            nmp_static_eval_depth_margin: v[21],
            nmp_base_reduction: v[22],
            nmp_depth_reduction_scale: v[23],
            nmp_eval_reduction_scale: v[24],
            nmp_max_eval_reduction: v[25],
            probcut_beta_margin: v[26],
            asp_init_delta: v[27],
            asp_widening_factor: v[28],
            min_asp_depth: v[29],
            do_deeper_margin_base: v[30],
            do_deeper_margin_depth: v[31],
            do_shallower_margin: v[32],
            rfp_opp_easy_capture: v[33],
            rfp_opp_worsening: v[34],
            rfp_hist_divisor: v[35],
            hist_beta_margin: v[36],
            hist_pruning_max_depth: v[37],
            triple_ext_margin: v[38],
            qs_hist_prune_margin: v[39],
            hist_bonus_quad: v[40],
            hist_bonus_linear: v[41],
            hist_bonus_offset: v[42],
            hist_bonus_max: v[43],
            hist_malus_quad: v[44],
            hist_malus_linear: v[45],
            hist_malus_offset: v[46],
            hist_malus_max: v[47],
            lmr_hist_divisor: v[48],
            rfp_base: v[49],
            rfp_step: v[50],
        }
    }
}

/// Names of the search parameters, in the exact order `to_vec` emits them.
/// Exposed so they can be set over UCI (`setoption name <n> value <v>`) and
/// swept without a rebuild -- the difference between an experiment costing
/// minutes and one costing a compile each.
///
/// Generated from `to_vec`, never hand-written. A list that drifts out of
/// order does not fail: it quietly sets the wrong parameter, and the sweep
/// reports whatever that other parameter happens to do.
pub const PARAM_NAMES: [&str; 51] = [
    "rfp_improving_base",
    "rfp_improving_slope",
    "rfp_not_improving_base",
    "rfp_not_improving_slope",
    "razor_base",
    "razor_per_depth",
    "futility_improving_base",
    "futility_improving_slope",
    "futility_not_improving_base",
    "futility_not_improving_slope",
    "cap_futility_improving_base",
    "cap_futility_improving_slope",
    "cap_futility_not_improving_base",
    "cap_futility_not_improving_slope",
    "delta_margin",
    "qs_lmp_limit",
    "tt_extended_cutoff_margin",
    "history_prune_mult",
    "nmp_min_depth",
    "nmp_eval_margin",
    "nmp_static_eval_base_margin",
    "nmp_static_eval_depth_margin",
    "nmp_base_reduction",
    "nmp_depth_reduction_scale",
    "nmp_eval_reduction_scale",
    "nmp_max_eval_reduction",
    "probcut_beta_margin",
    "asp_init_delta",
    "asp_widening_factor",
    "min_asp_depth",
    "do_deeper_margin_base",
    "do_deeper_margin_depth",
    "do_shallower_margin",
    "rfp_opp_easy_capture",
    "rfp_opp_worsening",
    "rfp_hist_divisor",
    "hist_beta_margin",
    "hist_pruning_max_depth",
    "triple_ext_margin",
    "qs_hist_prune_margin",
    "hist_bonus_quad",
    "hist_bonus_linear",
    "hist_bonus_offset",
    "hist_bonus_max",
    "hist_malus_quad",
    "hist_malus_linear",
    "hist_malus_offset",
    "hist_malus_max",
    "lmr_hist_divisor",
    "rfp_base",
    "rfp_step",
];

/// Overrides applied on top of the defaults, set over UCI before the first
/// search. `SEARCH_PARAMS` is a `OnceLock` and cannot be changed after it is
/// read, so these are held separately and folded in when it is first built.
pub static PARAM_OVERRIDES: std::sync::Mutex<Vec<(usize, i32)>> = std::sync::Mutex::new(Vec::new());

/// Set one parameter by name. Returns false for an unknown name so the caller
/// can say so out loud -- an ignored typo is indistinguishable from "this
/// parameter has no effect", and that mistake costs a whole experiment.
/// Whether a parameter is a quantity in EVALUATION units, i.e. compared
/// against a score rather than counting plies, moves or history points.
///
/// It matters because those are the only ones that stop meaning what they were
/// calibrated to mean when the evaluation's scale changes -- and it changed by
/// 1.45x when the fitted weight set arrived. A margin of 629 against an
/// evaluation that got half again as loud is a margin of 434 in the old money.
///
/// Classified by reading each use, not by the name: `nmp_eval_reduction_scale`
/// divides `static_eval - beta`, so it is an eval quantity even though it
/// yields plies; `qs_hist_prune_margin`, `rfp_hist_divisor` and the whole
/// `hist_*` family are history points and are NOT, however much the word
/// "margin" suggests otherwise. Guessing this from the names is exactly the
/// mistake that once applied an afternoon of parameter work to the wrong
/// fields.
pub fn param_in_eval_units(name: &str) -> bool {
    matches!(
        name,
        "rfp_improving_base"
            | "rfp_improving_slope"
            | "rfp_not_improving_base"
            | "rfp_not_improving_slope"
            | "razor_base"
            | "razor_per_depth"
            | "futility_improving_base"
            | "futility_improving_slope"
            | "futility_not_improving_base"
            | "futility_not_improving_slope"
            | "cap_futility_improving_base"
            | "cap_futility_improving_slope"
            | "cap_futility_not_improving_base"
            | "cap_futility_not_improving_slope"
            | "delta_margin"
            | "tt_extended_cutoff_margin"
            | "nmp_eval_margin"
            | "nmp_static_eval_base_margin"
            | "nmp_static_eval_depth_margin"
            | "nmp_eval_reduction_scale"
            | "probcut_beta_margin"
            | "asp_init_delta"
            | "do_deeper_margin_base"
            | "do_deeper_margin_depth"
            | "do_shallower_margin"
            | "rfp_opp_easy_capture"
            | "rfp_opp_worsening"
            | "hist_beta_margin"
            | "triple_ext_margin"
    )
}

pub fn set_param(name: &str, value: i32) -> bool {
    match PARAM_NAMES.iter().position(|&n| n == name) {
        Some(i) => {
            PARAM_OVERRIDES.lock().unwrap().push((i, value));
            true
        }
        None => false,
    }
}

static SEARCH_PARAMS: OnceLock<SearchParams> = OnceLock::new();
pub fn search_params() -> &'static SearchParams {
    SEARCH_PARAMS.get_or_init(|| {
        let overrides = PARAM_OVERRIDES.lock().unwrap().clone();
        if !overrides.is_empty() {
            let mut v = SearchParams::default().to_vec();
            for (i, val) in &overrides {
                if *i < v.len() {
                    v[*i] = *val;
                }
            }
            eprintln!("setoption: {} search parameter(s) overridden", overrides.len());
            return SearchParams::from_vec(&v);
        }
        if let Ok(path) = std::env::var("KESTREL_SEARCH_PARAMS") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let parsed: Vec<i32> = text.trim().split(',').filter_map(|s| s.parse().ok()).collect();
                let default = SearchParams::default();
                if parsed.len() == default.to_vec().len() {
                    eprintln!("KESTREL_SEARCH_PARAMS: loaded {} scalars from {}", parsed.len(), path);
                    return SearchParams::from_vec(&parsed);
                } else {
                    eprintln!(
                        "KESTREL_SEARCH_PARAMS: length mismatch ({} vs expected {}), ignoring",
                        parsed.len(),
                        default.to_vec().len()
                    );
                }
            }
        }
        SearchParams::default()
    })
}

/// Limite de saturacao da history heuristic (bonus/malus acumulados por
/// [cor][from][to]) -- evita que um par from/to muito bem sucedido
/// domine a ordenacao para sempre, sem precisar de "aging"/decay mais
/// complexo.
const HISTORY_MAX: i32 = 16000;

#[derive(Copy, Clone)]
pub struct SearchLimits {
    pub deadline: Option<Instant>,
    pub max_depth: i32,
    pub max_nodes: Option<u64>,
    /// The normal per-move allowance, as a duration from the start of the
    /// search. `deadline` above is the hard ceiling and exists to stop a
    /// disaster; THIS is the number the engine actually aims at, and
    /// iterative_deepening() scales it up or down between iterations
    /// according to how the search is going (see `time_scale`). A hard move
    /// can be given several times this; an obvious one gives most of it back.
    pub soft_budget: Option<Duration>,
}

/// How much of the soft budget this search has earned, judged between
/// iterations. Two independent readings of "does this position still need
/// thinking", multiplied together.
///
/// `effort` is the share of nodes that went into the move we intend to play.
/// A search that has poured almost everything into one move has found its
/// answer and is re-confirming it; one still splitting nodes across rivals
/// has not decided yet. This is the sturdier of the two signals, because it
/// is a ratio over millions of nodes rather than a verdict that can flip on
/// one.
///
/// `settle` counts consecutive iterations that kept the same root move, and
/// decays fast: a move that just changed is worth far more time than one
/// that has held for five iterations. It is deliberately the weaker term
/// here. Lazy SMP makes root-move stability partly a matter of which thread
/// got where first -- an earlier attempt at elastic time management keyed on
/// stability ALONE and had to be reverted the same day, because it read
/// thread timing as position difficulty and burned 10-16s on ordinary moves.
/// The lesson kept: stability may lengthen a search, never on its own, and
/// never without the hard ceiling standing behind it.
fn time_scale(effort_frac: f64, settle: u32, score_drop: i32, changes: u32) -> f64 {
    let effort = (TM_EFFORT_BASE - effort_frac) * TM_EFFORT_SCALE;
    let settle = (TM_SETTLE_BASE + TM_SETTLE_SCALE * (settle as f64 + TM_SETTLE_OFFSET).powf(TM_SETTLE_POWER))
        .max(TM_SETTLE_MIN);
    // A score that is FALLING between iterations is the clearest sign that a
    // position deserves more thought: the search is discovering a problem and
    // has not yet found the way out. Neither of the other two signals sees
    // this -- effort can stay high while the position collapses under it, and
    // stability measures whether the MOVE changed, not whether it got worse.
    //
    // Only falls count. A score climbing means the news is good and there is
    // nothing to solve, and paying extra for good news is how a clock is
    // wasted on won positions.
    let falling = if score_drop > 0 {
        (1.0 + score_drop as f64 * TM_FALLING_SCALE).min(TM_FALLING_MAX)
    } else {
        1.0
    };
    // A move the search keeps changing its mind about is worth paying for.
    // This is the factor the engine did not have, and it is the one that
    // separates a hard position from a slow one: effort and settle both read
    // "several moves are equally good" as difficulty, so they fire on quiet
    // positions with many reasonable moves. A best move that keeps being
    // overturned means something concrete was found late, repeatedly.
    let instability = (1.0 + changes as f64 * TM_INSTABILITY_SCALE).min(TM_INSTABILITY_MAX);
    (effort * settle * falling * instability).clamp(TM_SCALE_MIN, TM_SCALE_MAX)
}

/// Extra time per centipawn lost since the previous iteration, and its cap.
/// Small per unit and capped low: this multiplies two other factors that can
/// each already stretch the budget, and the hard ceiling still stands behind
/// all three. An earlier elastic-time attempt keyed on one signal alone burned
/// 10-16 seconds on ordinary moves and had to be reverted the same day.
const TM_FALLING_SCALE: f64 = 0.004;
const TM_FALLING_MAX: f64 = 1.5;

// Effort carries most of the decision. Measured across easy and hard
// positions the fraction runs from about 0.13 (nothing decided yet) to about
// 0.80 (everything behind one move): 0.80 -> 0.93x, 0.50 -> 1.40x,
// 0.30 -> 1.71x. A steeper version that cut to 0.77x at the easy end was
// tried and pulled back -- it halved the median move time, and cutting that
// hard is only worth doing on a signal that deserves the confidence.
const TM_EFFORT_BASE: f64 = 1.40;
const TM_EFFORT_SCALE: f64 = 1.55;
// Settle is deliberately the gentler term -- 1.48x when the move has just
// changed, decaying to 1.0x, never below. A much steeper curve was tried
// first, and tracing showed why it cannot be trusted here: on a FORCED
// RECAPTURE, where there is nothing to decide, the root move still changed
// at three separate depths and each change threw the multiplier back to its
// maximum. That is Lazy SMP thread timing, not the position being hard, and
// it is the same signal that made the 2026-07-21 attempt burn 10-16s on
// ordinary moves. It stays in because a genuinely changing move IS worth
// more time; it stays small because here it lies often.
const TM_SETTLE_BASE: f64 = 0.95;
const TM_SETTLE_SCALE: f64 = 2.2;
const TM_SETTLE_OFFSET: f64 = 2.6;
const TM_SETTLE_POWER: f64 = -1.5;
const TM_SETTLE_MIN: f64 = 1.0;
// The envelope, on top of which the hard deadline is a second and
// independent bound.
/// Um lance obvio custa isto do orcamento. Ver o bloco em
/// iterative_deepening: o melhor a' frente do segundo por TM_OBVIO_CP,
/// estavel ha' TM_OBVIO_ITERS profundidades e com o score parado.
const TM_OBVIO_SCALE: f64 = 0.35;
/// Quanto o melhor tem de estar a' frente do segundo para o lance ser obvio.
/// Meia peca menor: abaixo disto ha' escolha a fazer.
const TM_OBVIO_CP: i32 = 150;
/// E ha' quantas profundidades tem de ser o mesmo lance.
const TM_OBVIO_ITERS: u32 = 4;

const TM_SCALE_MIN: f64 = 0.65;
/// The most the search may award itself over the base allowance.
///
/// Was 2.2, which is what a healthy position needs and what a critical one
/// cannot use. Two real losses were traced to it: a move that turned an equal
/// game into mate, played in 1.04s with 29.7s on the clock, where three
/// seconds of thought picks a different move. The ceiling was not reached in
/// either case -- it was low enough that the signals never bothered to argue
/// for more. The hard cap in the time budget bounds this from above, and a
/// percentage of the remaining clock bounds THAT, so a raised ceiling here
/// buys thinking time on contested moves without putting the game at risk.
const TM_SCALE_MAX: f64 = 3.4;
/// Growth per change of heart about the best move.
const TM_INSTABILITY_SCALE: f64 = 0.22;
/// ...and its ceiling, so a position that never settles cannot spend the game.
const TM_INSTABILITY_MAX: f64 = 2.4;
/// How much the evaluation must move for a change of best move to count as
/// the search finding something, rather than picking between equals.
const TM_INSTABILITY_MIN_CP: i32 = 20;

const TM_QUIET_CP: i32 = 10;
const TM_QUIET_ITERS: u32 = 3;
/// Ceiling on the allowance while the position is still in the opening book.
/// Enough to confirm the prepared move and notice if it is refuted, not enough
/// to spend the opening's share of the clock on a decision already made.
const TM_BOOK_SCALE: f64 = 0.35;
/// Ceiling on the allowance during the opening once out of book. Not a cut --
/// the full slice is still available, it simply cannot be multiplied by
/// signals that measurement showed to be noise this early.
const TM_OPENING_SCALE: f64 = 1.0;
/// How many full moves count as "the opening" for the ceiling above.
const TM_OPENING_MOVES: u32 = 10;
/// A score swing this large between iterations is a real change of mind, not
/// search noise, and lifts the opening ceiling.
const TM_ALERT_CP: i32 = 50;


/// For single-threaded callers that have no search to coordinate with -- the
/// command-line tools, which stop on node counts and nothing else. Never set.
pub static NO_STOP: AtomicBool = AtomicBool::new(false);

/// Quanto custa um empate, em centipeoes. Ver `valor_empate`.
pub static CONTEMPT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(20);

pub struct Searcher<'a> {
    /// Quem manda na raiz. Um empate e' mau para ESTE lado, e um no' qualquer
    /// da arvore pode ser de qualquer um dos dois.
    pub root_side: crate::types::Color,
    pub atk: &'a Attacks,
    pub zob: &'a Zobrist,
    pub tt: &'a TranspositionTable,
    pub nodes: u64,
    pub limits: SearchLimits,
    pub stop: bool,
    /// Shared across every thread of one search, so that whoever decides the
    /// move is settled ends the search rather than only its own thread.
    /// Without it the per-thread stop was near useless: the move takes as
    /// long as the SLOWEST thread, so one thread giving the clock back saved
    /// nothing while the others kept going. Set by the reporting thread when
    /// the soft budget is spent, and by any thread that hits the hard
    /// deadline (there is no reason for the rest to carry on past that).
    pub stop_flag: &'a AtomicBool,
    /// Nodes where a beta cutoff happened, and how many of those took only
    /// the first move. See the note at the increment.
    /// Indice desta thread na busca paralela. Zero e a principal.
    pub thread_idx: usize,
    pub asp_re: u64,
    pub asp_nos: u64,
    pub cut_nodes: u64,
    pub cut_first: u64,
    /// Nodes spent in quiescence. It obeys neither the depth limit nor
    /// LMR nor LMP, so it is the one part of the tree that can grow without
    /// showing up in any of the other telemetry.
    /// Null-move telemetry. Both of the last two attempts at this gate were
    /// built on the assumption that a sharp position explodes because the
    /// null move FIRES and then has to be verified. Measurement inverted it:
    /// blocking the null move there doubled the tree, so it was firing and
    /// paying for itself. These counters replace the assumption with numbers.
    pub nmp_tried: u64,
    pub nmp_tried_pv: u64,
    pub nmp_failed_pv: u64,
    pub nmp_cutoff_raw: u64,
    pub nmp_cut_taken: u64,
    pub nmp_verify_tried: u64,
    pub nmp_verify_ok: u64,
    pub nmp_verify_failed: u64,
    pub nmp_failed_low: u64,
    pub qnodes: u64,
    /// How often each shallow, eval-based pruning actually fires. The tree is
    /// wide and shallow, which points at whatever decides how many low-depth
    /// nodes get to exist at all -- and constants that look aggressive on the
    /// page have already fooled me twice tonight.
    pub cut_rfp: u64,
    pub cut_razor: u64,
    pub cut_futility: u64,
    pub nodes_shallow: u64,
    pub lmr_quiet_total: u64,
    pub lmr_skip_check: u64,
    pub lmr_skip_depth: u64,
    pub lmr_skip_extend: u64,
    pub lmr_skip_early: u64,
    pub lmr_tried: u64,
    pub lmr_research: u64,
    pub lmr_sum: u64,
    pub history: Vec<u64>, // hashes da partida real ate' agora (para repeticao)
    pub killers: [[Option<Move>; 2]; MAX_PLY],
    /// History heuristic ("butterfly boards" classicos): [cor][from][to],
    /// bonus quando um lance tranquilo causa um corte beta, malus nos
    /// lances tranquilos experimentados antes dele no MESMO no' que nao
    /// cortaram -- peca canonica que faltava por completo (so' havia
    /// TT-move/MVV-LVA/killers/livro; todos os outros lances tranquilos
    /// ficavam sem NENHUM sinal de ordenacao). 2026-07-20, ver
    /// project_kestrel_achados_2026-07-20.md. Zerada uma vez por `go`
    /// (o Searcher e' reconstruido a cada `go` em uci.rs), nunca a meio
    /// da busca -- a mesma licao do bug de killers corrigido antes.
    pub history_scores: [[[i32; 64]; 64]; 2],
    /// Countermove heuristic: indexed by [piece type][to square] of the
    /// move that led INTO this node (the opponent's last move) -> a quiet
    /// move that previously caused a beta cutoff in reply to that exact
    /// context. Kept for the picker's tier scoring; overshadowed by the
    /// finer-grained `cont_hist` below (which gives a numeric weight per
    /// (prev_piece,prev_to)->(curr_piece,curr_to) pair, at 1 AND 2 plies
    /// back -- our multi-lag continuation history).
    pub countermoves: [[Option<Move>; 64]; 6],
    /// Capture history: indexed by [side][moving piece][captured piece]
    /// -- a coarser, dedicated signal complementing SEE in noisy move
    /// ordering. SEE gives the true material outcome of an exchange but
    /// says nothing about which of several SEE-EQUAL captures tends to
    /// actually work out (e.g. two captures that both win a pawn cleanly
    /// -- history says which capture pattern has paid off more at this
    /// kind of node before). Deliberately used ONLY as a tie-break when
    /// SEE values are exactly equal (see MovePicker::pick_best_noisy) --
    /// never mixed into the SEE score itself, which would shift the
    /// good/bad-noisy partition boundary that many other pruning
    /// decisions rely on being pure SEE.
    pub capture_history: [[[i32; 6]; 6]; 2],
    /// Continuation history: dense i32 table indexed by (prev_piece,
    /// prev_to, curr_piece, curr_to) -- gives quiet move ordering a
    /// numeric bonus/malus based on how the SAME curr_move performed in
    /// the past following the SAME prev_move (piece type + to-square).
    /// Used at both 1-ply back (opponent's last move) and 2-ply back
    /// (our own last move) -- multi-lag at plies -1 and -2 (a -4 lag
    /// could be added later if it proves worth it).
    /// Heap-allocated (~576KB, 6*64*6*64 * 4 bytes) since it doesn't fit
    /// on the stack. Zeroed once per `go` (Searcher is rebuilt each go).
    pub cont_hist: Box<[i32]>,
    /// Correction history: keyed by a cheap pawn-structure hash, learns
    /// how far off the raw static eval tends to be for THIS pawn
    /// structure once real search has settled on a score. The rationale:
    /// static eval is fast but systematically biased for certain structures
    /// (e.g. closed positions, specific pawn chains); this nudges it
    /// toward what search has actually been finding there. Only affects
    /// pruning-margin decisions (RFP/futility/LMP/razoring), never the
    /// real leaf/quiescence evaluation.
    pub corr_hist: Box<[i32]>,
    /// 2026-07-22: four more correction-history dimensions (a multi-term
    /// weighted correction; Kestrel previously had only the pawn term
    /// below). Same table shape/update rule as `corr_hist`, different
    /// hash input. Continuation-history correction (further terms at
    /// lags 2-7) deferred (needs a shared 4D per-ply-lag table, real
    /// scope, own follow-up).
    pub corr_hist_np_stm: Box<[i32]>,
    pub corr_hist_np_nstm: Box<[i32]>,
    pub corr_hist_minor: Box<[i32]>,
    pub corr_hist_major: Box<[i32]>,
    /// 2026-07-23: the `threats` term, added once `all_attacks()`
    /// (a standalone "all squares attacked by side X" helper, not
    /// dependent on eval.rs's internal loop state) made it possible
    /// without a real eval.rs refactor -- see `threats_hash()`.
    pub corr_hist_threats: Box<[i32]>,
    /// For each ply, the (piece type, to-square) of the move that was
    /// played to reach that ply (i.e. the opponent's last move as seen
    /// from this node) -- set by the parent right before recursing, read
    /// by the picker to look up `cont_hist`.
    pub ply_last_move: [Option<(PieceType, crate::types::Square)>; MAX_PLY],
    /// Static eval saved at each ply -- used by the `improving`
    /// heuristic: at a node, compare the current side's static eval
    /// against the one from 2 plies back (same side to move). If it
    /// went up, we're "improving" -- position getting better, so we
    /// spend less time (tighter futility, more aggressive pruning).
    pub static_evals: [i32; MAX_PLY],
    pub root_best: Option<Move>,
    /// Per root move: the score from the current iteration, and the score
    /// from the previous one. `NO_SCORE` means "not measured this iteration".
    pub root_scores: Vec<(Move, i32, i32)>,
    /// Ply below which the null move is not allowed, used to stop the null
    /// move recursing inside its own verification search.
    pub nmp_min_ply: i32,
    /// Singular extensions: quando estamos a verificar se o tt_move e'
    /// "singular" (nenhum outro lance bate uma janela restrita), fazemos
    /// uma re-pesquisa no MESMO no' excluindo o tt_move. Este campo diz
    /// ao picker para saltar esse lance e ao proprio negamax para NAO
    /// devolver cedo por TT nem armazenar na TT durante a re-pesquisa
    /// (a busca a janela restrita nao deve poluir a TT). Restaurado
    /// para None imediatamente depois da re-pesquisa.
    pub excluded_move: Option<Move>,
    /// MultiPV via the "exclusion" method: root moves listed here are
    /// dropped from the root's legal-move list, so a repeated search at
    /// the same position finds the next-best line instead of the same
    /// one. Empty during normal single-PV search (no behavior change).
    pub excluded_root_moves: Vec<Move>,
    // Livro de "assinatura" da Judit Polgar (ver book.rs) -- so' influencia
    // a ORDEM em que a busca experimenta os lances, nunca substitui a
    // avaliacao real. None se o livro nao carregou (o motor continua a
    // funcionar normalmente sem ele).
    pub style_book: Option<&'a Book>,
    /// Node-count time management: total nodes spent on each
    /// ROOT move across the whole `go`, accumulated over every
    /// iterative-deepening iteration (not cleared between depths --
    /// only `iterative_deepening()` clears it, once per `go`). A small
    /// Vec, not a HashMap: the root move list is at most a few dozen
    /// moves, so a linear scan per update is cheaper than hashing.
    pub root_move_nodes: Vec<(Move, u64)>,
    /// Double-extension counter, propagated down a search LINE (indexed
    /// by ply): how many times this exact line has already used a
    /// double extension. Read from the PARENT ply before deciding to
    /// grant another one, a guard (`dextensions<=6`) that stops a run
    /// of double extensions from exploding the tree --
    /// each one costs an extra full ply, and they can chain if several
    /// nodes in a row are singular by a wide margin.
    pub dextensions: [i32; MAX_PLY],
    /// Report each completed iteration on stdout as a UCI `info` line.
    /// Set on ONE searcher only (the rest of the Lazy-SMP threads stay
    /// silent, or every depth would be reported several times over).
    ///
    /// Without this the engine only ever announced its final answer, which
    /// hides how its opinion developed -- the thing you actually need when
    /// asking why a move was chosen, and what every GUI expects to display.
    pub report: bool,
}

/// The learned tables that should OUTLIVE a single `go`.
///
/// Until this existed, `uci.rs` built a fresh `Searcher` for every search,
/// which zeroed every one of these -- so each move in a game started from
/// nothing. That throws away most of what they are for: correction history
/// in particular is a slow-learning signal (it accumulates how far the
/// static eval tends to sit from what search finds for a given structure),
/// and it can only pay off if it survives across the moves of a game. The
/// same holds, less dramatically, for history/countermoves/capture history,
/// where consecutive positions in a game are closely related and last
/// move's statistics are immediately useful for ordering this one.
///
/// The UCI protocol assumes exactly this lifetime: `ucinewgame` exists to
/// tell an engine to forget, which only means something if it otherwise
/// remembers between searches.
///
/// Killers are deliberately NOT carried over: they are indexed by ply, and
/// ply N means a different point in the game after each move is played, so
/// keeping them would just mis-attribute cutoffs.
///
/// One instance per search thread (Lazy SMP threads keep independent
/// statistics), held by the UCI layer and handed back and forth.
pub struct HistoryTables {
    pub history_scores: [[[i32; 64]; 64]; 2],
    pub countermoves: [[Option<Move>; 64]; 6],
    pub capture_history: [[[i32; 6]; 6]; 2],
    pub cont_hist: Box<[i32]>,
    pub corr_hist: Box<[i32]>,
    pub corr_hist_np_stm: Box<[i32]>,
    pub corr_hist_np_nstm: Box<[i32]>,
    pub corr_hist_minor: Box<[i32]>,
    pub corr_hist_major: Box<[i32]>,
    pub corr_hist_threats: Box<[i32]>,
}

impl HistoryTables {
    /// Fade the move-ordering statistics before reusing them in the next
    /// search of the same game. CURRENTLY UNUSED: fading at 3/4 measured
    /// worse than carrying the tables over intact on the blunder replay
    /// (45/60 blunders avoided vs 47/60, 74 improving deviations vs 83),
    /// so `uci.rs` does not call it. Kept for a future retry with a
    /// gentler factor, or applied to only some of the tables.
    /// Kept at 3/4 per move: strong enough that
    /// evidence from several moves ago stops dominating (after ~5 moves an
    /// old score is down to a quarter), gentle enough that the ordering
    /// carried over is still worth having on the first iterations, which
    /// is the whole point of keeping the tables at all.
    ///
    /// Only the move-ordering tables fade. Correction history is left
    /// intact on purpose: it is not an ordering preference but a measured
    /// bias of our own static eval for a given structure, which does not
    /// go stale as the game advances -- fading it would just slow down the
    /// one signal that needs the longest to become reliable.
    pub fn fade(&mut self) {
        for side in self.history_scores.iter_mut() {
            for from in side.iter_mut() {
                for v in from.iter_mut() {
                    *v = *v * 3 / 4;
                }
            }
        }
        for side in self.capture_history.iter_mut() {
            for moved in side.iter_mut() {
                for v in moved.iter_mut() {
                    *v = *v * 3 / 4;
                }
            }
        }
        for v in self.cont_hist.iter_mut() {
            *v = *v * 3 / 4;
        }
    }
}

impl Default for HistoryTables {
    fn default() -> Self {
        let corr = || vec![0i32; CORR_HIST_SIZE * 2].into_boxed_slice();
        HistoryTables {
            history_scores: [[[0; 64]; 64]; 2],
            countermoves: [[None; 64]; 6],
            capture_history: [[[0; 6]; 6]; 2],
            cont_hist: vec![0i32; CONT_HIST_SIZE].into_boxed_slice(),
            corr_hist: corr(),
            corr_hist_np_stm: corr(),
            corr_hist_np_nstm: corr(),
            corr_hist_minor: corr(),
            corr_hist_major: corr(),
            corr_hist_threats: corr(),
        }
    }
}

/// Limiar a partir do qual um score e' considerado "de mate" (nao so'
/// avaliacao normal) -- MATE_SCORE menos a profundidade maxima possivel,
/// para nao confundir avaliacoes normais muito altas com mates reais.
const MATE_THRESHOLD: i32 = MATE_SCORE - MAX_PLY as i32;
/// How much MORE singular (below s_beta) a move has to be, on top of
/// just passing the ordinary singular-extension check, before it earns
/// a double (+2 ply) extension instead of the normal +1.
const DOUBLE_EXT_MARGIN: i32 = 16;
/// Cap on chained double extensions along one search line (read from
/// the parent ply's count).
const DOUBLE_EXT_MAX: i32 = 6;

/// Tamanho da tabela cont_hist -- 6 tipos de peca * 64 casas destino
/// para o prev-move, vezes o mesmo para o curr-move. Ver campo
/// `cont_hist` do Searcher.
pub const CONT_HIST_SIZE: usize = 6 * 64 * 6 * 64;
const CONT_HIST_MAX: i32 = 16000;

/// History update with gravity: the closer an entry already sits to the
/// ceiling, the less a new observation moves it.
///
/// A plain clamped sum does not do this. An entry that reaches the ceiling
/// stays pinned there, and every later cutoff for that move is discarded --
/// so does every later failure, which is worse, because a move that stopped
/// working keeps its maximum score until something drags it all the way back
/// down one bonus at a time. Gravity makes the table saturate smoothly and
/// stay responsive: near zero an update lands in full, near the ceiling it is
/// almost entirely cancelled.
#[inline]
fn apply_gravity(value: i32, delta: i32, max: i32) -> i32 {
    (value + delta - value * delta.abs() / max).clamp(-max, max)
}

/// Bonus for the move that caused a beta cutoff.
///
/// The old formula was `depth * depth`, which at depth 1 is 1 -- against a
/// ceiling of 16000. Shallow nodes are the overwhelming majority of the tree,
/// and there the table was effectively frozen: it took thousands of identical
/// cutoffs to move an entry enough to change any ordering decision. The
/// linear term is what makes a shallow cutoff worth recording at all; the
/// quadratic term still makes a deep one worth more.
fn history_bonus(depth: i32) -> i32 {
    let sp = search_params();
    let b = sp.hist_bonus_quad * depth * depth / 64 + sp.hist_bonus_linear * depth
        - sp.hist_bonus_offset;
    b.clamp(0, sp.hist_bonus_max)
}

/// Penalty for the quiet moves tried before the one that cut off.
///
/// Separate from the bonus, and deliberately so: "this move refuted the node"
/// and "this move was tried first and did not" are not equally strong claims.
/// The second is far weaker -- a move can fail simply for being ordered ahead
/// of a better one -- so punishing it as hard as the cutoff is rewarded
/// teaches the table noise. Using one number for both is the version we had.
fn history_malus(depth: i32) -> i32 {
    let sp = search_params();
    let m = sp.hist_malus_quad * depth * depth / 64 + sp.hist_malus_linear * depth
        - sp.hist_malus_offset;
    m.clamp(0, sp.hist_malus_max)
}

#[inline(always)]
fn cont_hist_idx(prev_pt: PieceType, prev_to: crate::types::Square, curr_pt: PieceType, curr_to: crate::types::Square) -> usize {
    let prev = prev_pt.idx() * 64 + prev_to as usize;
    let curr = curr_pt.idx() * 64 + curr_to as usize;
    prev * (6 * 64) + curr
}

/// Correction history table size (per color) and clamp. 16384 slots is
/// plenty for a hash-modulo table at this scale; collisions just blend
/// two structures' corrections together, self-correcting over time.
pub const CORR_HIST_SIZE: usize = 16384;
const CORR_HIST_MAX: i32 = 1200; // clamp on the stored correction itself
const CORR_HIST_GRAIN: i32 = 256; // internal fixed-point scale

/// SPSA-tuned weights for combining the 5 correction-history dimensions
/// (pawn, non-pawn side-to-move, non-pawn other side, minor, major).
/// `CORR_WEIGHT_SCALE`=256 means a weight of 256 is "full 1.0 effect"
/// (matches the old pawn-only formula's implicit weight).
const CORR_WEIGHT_SCALE: i32 = 256;
// 2026-07-22 CORRECTED (see NOTAS): a set of raw reference weights
// (384/406/280/274/418) had been SPSA-tuned against a DIFFERENT
// maxCorrHist clamp/grain, not Kestrel's independently-chosen
// CORR_HIST_MAX=1200 -- applying the raw numbers directly caused a
// severe regression (300-game A/B: 6.7% vs the pre-change baseline).
// Rescaled here to preserve the REAL relative proportions between the
// 5 terms (which term matters more than which) while capping the
// worst-case total (all 5 tables simultaneously maxed the same
// direction) close to the same bound the old single-pawn-term system
// safely operated at: weights sum to 257 (independent per-term
// rounding of those proportions, not tuned to hit exactly 256) --
// 0.4% over the old implicit pawn-only weight's bound, instead of the
// raw set's much larger sum of 1762.
const CORR_WEIGHT_PAWN: i32 = 56;
const CORR_WEIGHT_NP_STM: i32 = 59;
const CORR_WEIGHT_NP_NSTM: i32 = 41;
const CORR_WEIGHT_MINOR: i32 = 40;
const CORR_WEIGHT_MAJOR: i32 = 61;
// 2026-07-23: threats term, added separately from the 5 above rather
// than folded into the same rescale (which would mean touching the
// already-validated 5 weights again, bundling a rescale with an
// addition -- kept isolated instead, same discipline used all
// session). Same conversion rate as the original 5-term rescale
// (256/1762 ~= 0.1453) applied individually to a raw threats weight
// of 252: 252*0.1453 ~= 37.
const CORR_WEIGHT_THREATS: i32 = 37;

/// Cheap, non-incremental pawn-structure hash -- just the two pawn
/// bitboards mixed together. Not the real Zobrist key (which would
/// need incremental maintenance in make/unmake_move); recomputed on
/// demand, which is fine since it's only touched once or twice per
/// node, not in the hot per-move loop.
#[inline]
fn pawn_structure_hash(board: &Board) -> u64 {
    let wp = board.pieces[Color::White.idx()][PieceType::Pawn.idx()];
    let bp = board.pieces[Color::Black.idx()][PieceType::Pawn.idx()];
    wp.wrapping_mul(0x9E3779B97F4A7C15) ^ bp.wrapping_mul(0xC2B2AE3D27D4EB4F)
}

/// Same non-incremental, on-demand approach as `pawn_structure_hash`,
/// applied to the other correction-history dimensions. Non-pawn
/// material (knights/bishops/rooks/queens) of a SINGLE side -- called
/// once for the side to move and once for the other side (the two
/// non-pawn terms are independent, not one table read from both angles).
#[inline]
fn non_pawn_hash(board: &Board, color: Color) -> u64 {
    let n = board.pieces[color.idx()][PieceType::Knight.idx()];
    let b = board.pieces[color.idx()][PieceType::Bishop.idx()];
    let r = board.pieces[color.idx()][PieceType::Rook.idx()];
    let q = board.pieces[color.idx()][PieceType::Queen.idx()];
    n.wrapping_mul(0x165667B19E3779F9)
        ^ b.wrapping_mul(0x27D4EB2F165667C5)
        ^ r.wrapping_mul(0x9E3779B185EBCA87)
        ^ q.wrapping_mul(0xC2B2AE3D27D4EB4F)
}

/// Minor pieces (knights+bishops), both sides mixed together -- same
/// both-colors-combined shape as `pawn_structure_hash`.
#[inline]
fn minor_piece_hash(board: &Board) -> u64 {
    let wn = board.pieces[Color::White.idx()][PieceType::Knight.idx()];
    let wb = board.pieces[Color::White.idx()][PieceType::Bishop.idx()];
    let bn = board.pieces[Color::Black.idx()][PieceType::Knight.idx()];
    let bb = board.pieces[Color::Black.idx()][PieceType::Bishop.idx()];
    wn.wrapping_mul(0x9E3779B97F4A7C15)
        ^ wb.wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ bn.wrapping_mul(0x165667B19E3779F9)
        ^ bb.wrapping_mul(0x27D4EB2F165667C5)
}

/// Major pieces (rooks+queens), both sides mixed together.
#[inline]
fn major_piece_hash(board: &Board) -> u64 {
    let wr = board.pieces[Color::White.idx()][PieceType::Rook.idx()];
    let wq = board.pieces[Color::White.idx()][PieceType::Queen.idx()];
    let br = board.pieces[Color::Black.idx()][PieceType::Rook.idx()];
    let bq = board.pieces[Color::Black.idx()][PieceType::Queen.idx()];
    wr.wrapping_mul(0x9E3779B185EBCA87)
        ^ wq.wrapping_mul(0xFF51AFD7ED558CCD)
        ^ br.wrapping_mul(0xC4CEB9FE1A85EC53)
        ^ bq.wrapping_mul(0x2545F4914F6CDD1D)
}

/// All squares attacked by every piece of `color` (pawns, knights,
/// bishops/queens via magic sliding attacks, rooks/queens likewise,
/// king) -- used only by the threats correction-history term below.
/// Not incremental (recomputed on demand like the other corr-hist
/// hashes), acceptable since it's touched once or twice per node, not
/// in the hot per-move loop.
fn all_attacks(board: &Board, atk: &Attacks, color: Color) -> Bitboard {
    let us = color.idx();
    let occ = board.occ_all;
    let mut att: Bitboard = 0;
    let mut pawns = board.pieces[us][PieceType::Pawn.idx()];
    while pawns != 0 {
        let s = pawns.trailing_zeros() as usize;
        pawns &= pawns - 1;
        att |= atk.pawn[us][s];
    }
    let mut knights = board.pieces[us][PieceType::Knight.idx()];
    while knights != 0 {
        let s = knights.trailing_zeros() as usize;
        knights &= knights - 1;
        att |= atk.knight[s];
    }
    let mut bishops = board.pieces[us][PieceType::Bishop.idx()] | board.pieces[us][PieceType::Queen.idx()];
    while bishops != 0 {
        let s = bishops.trailing_zeros() as u8;
        bishops &= bishops - 1;
        att |= bishop_attacks(s, occ);
    }
    let mut rooks = board.pieces[us][PieceType::Rook.idx()] | board.pieces[us][PieceType::Queen.idx()];
    while rooks != 0 {
        let s = rooks.trailing_zeros() as u8;
        rooks &= rooks - 1;
        att |= rook_attacks(s, occ);
    }
    let king_sq = board.pieces[us][PieceType::King.idx()].trailing_zeros() as usize;
    att |= atk.king[king_sq];
    att
}

/// Threats correction hash: which of OUR pieces are currently attacked
/// by the enemy -- hash of (opponent-attacked squares & our pieces).
fn threats_hash(board: &Board, atk: &Attacks) -> u64 {
    let enemy_attacks = all_attacks(board, atk, board.side.opp());
    let own_pieces = board.occ_color[board.side.idx()];
    let threatened = enemy_attacks & own_pieces;
    threatened.wrapping_mul(0x2545F4914F6CDD1D) ^ threatened.wrapping_mul(0x9E3779B97F4A7C15).rotate_left(17)
}

/// 2026-07-20 (BUG REAL encontrado por auditoria -- investigacao da
/// queda de resultados, ver NOTAS_PROXIMA_SESSAO.md): a TT guardava e
/// lia scores de mate em BRUTO, sem ajustar pela distancia (ply) entre
/// o no' onde a entrada foi escrita e o no' onde e' reaproveitada --
/// bug classico de "corrupcao de mate score" em qualquer motor
/// alfa-beta com TT. Um "mate em N" escrito a um ply e' relativo a ESSE
/// ply; reaproveitado sem ajuste noutro ply, o motor pode "ver" mates
/// que nao existem dali, ou avaliar mal posicoes decisivas perto de
/// mate -- exatamente onde um estilo agressivo (Polgar) mais precisa de
/// avaliacoes corretas. Converte para "distancia ao no' ATUAL" antes de
/// guardar, converte de volta para "distancia a partir da raiz real"
/// (ou seja, para a escala que negamax() usa) ao ler.
fn score_to_tt(score: i32, ply: i32) -> i32 {
    if score >= MATE_THRESHOLD {
        score + ply
    } else if score <= -MATE_THRESHOLD {
        score - ply
    } else {
        score
    }
}
fn score_from_tt(score: i32, ply: i32) -> i32 {
    if score >= MATE_THRESHOLD {
        score - ply
    } else if score <= -MATE_THRESHOLD {
        score + ply
    } else {
        score
    }
}

impl<'a> Searcher<'a> {
    /// Reconstructs the full principal variation by walking the TT's
    /// best-move chain from `board` forward. Not a dedicated PV table --
    /// cheap and good enough for UCI `info ... pv` output and for
    /// verifying deep/forced lines (e.g. long mates) actually hold up
    /// move by move, not just at the root. Defensive against a stale or
    /// hash-collided entry pointing at an illegal move (stops the line
    /// there instead of applying it) and against cycles (a repetition
    /// loop in a corrupted chain would otherwise iterate forever).
    pub fn extract_pv(&self, board: &Board, max_len: usize) -> Vec<Move> {
        let mut b = board.clone();
        let first = match self.tt.probe(b.hash).and_then(|e| e.best) {
            Some(m) => m,
            None => return Vec::new(),
        };
        self.extract_pv_from(board, first, max_len)
    }

    /// Same reconstruction, but starting from a move the caller names --
    /// used to make the reported line begin with the move actually chosen.
    pub fn extract_pv_from(&self, board: &Board, first: Move, max_len: usize) -> Vec<Move> {
        let mut pv = Vec::new();
        let mut b = board.clone();
        {
            let legal = generate_legal(&mut b, self.atk);
            if !legal.contains(&first) {
                return pv;
            }
            b.make_move(&first);
            pv.push(first);
        }
        let mut seen = std::collections::HashSet::new();
        for _ in 0..max_len {
            let hash = b.hash;
            if !seen.insert(hash) {
                break;
            }
            let mv = match self.tt.probe(hash).and_then(|e| e.best) {
                Some(m) => m,
                None => break,
            };
            let legal = generate_legal(&mut b, self.atk);
            if !legal.contains(&mv) {
                break;
            }
            b.make_move(&mv);
            pv.push(mv);
        }
        pv
    }

    fn time_up(&mut self) -> bool {
        if self.stop {
            return true;
        }
        if self.nodes % 2048 == 0 {
            if self.stop_flag.load(Ordering::Relaxed) {
                self.stop = true;
                return true;
            }
            if let Some(d) = self.limits.deadline {
                if Instant::now() >= d {
                    self.stop = true;
                    self.stop_flag.store(true, Ordering::Relaxed);
                }
            }
            if let Some(mx) = self.limits.max_nodes {
                if self.nodes >= mx {
                    self.stop = true;
                    self.stop_flag.store(true, Ordering::Relaxed);
                }
            }
        }
        self.stop
    }

    /// Quanto vale um empate -- e nao e' zero.
    ///
    /// Valia. E com zero, um empate e' um resultado aceitavel sempre que as
    /// alternativas nao parecem muito melhores: o adversario repete, a busca
    /// ve zero, compara com uma linha que a nossa avaliacao pontua em +40, e
    /// quarenta centipeoes de vantagem incerta nem sempre ganham a um zero
    /// garantido. O resultado e' meio ponto entregue em posicoes que estavamos
    /// a ganhar.
    ///
    /// Um empate custa-nos alguma coisa, e o numero diz isso: e' pontuado como
    /// ligeiramente MAU para quem esta a decidir a raiz. Assim, entre repetir
    /// e continuar a jogar, a busca prefere jogar -- e so' aceita o empate
    /// quando a alternativa e' mesmo pior, que e' quando um empate e' de facto
    /// o melhor que ha.
    ///
    /// Pequeno de proposito. Grande demais e o motor recusa empates em
    /// posicoes perdidas, que e' deitar fora meio ponto pela razao oposta.
    /// Vinte centipeoes e' menos de um quinto de peao: chega para desempatar
    /// entre repetir e jogar, e nao chega para inventar vantagem nenhuma.
    fn valor_empate(&self, board: &Board) -> i32 {
        let c = CONTEMPT.load(std::sync::atomic::Ordering::Relaxed);
        if c == 0 {
            return 0;
        }
        // Escalado com a avaliacao, nao fixo em centipeoes.
        //
        // O valor foi escolhido contra uma escala em que a dama valia 1980, e
        // a escala passou para metade disso hoje. Sem esta correccao os mesmos
        // vinte centipeoes valeriam o dobro do que foram afinados para valer:
        // o comentario acima diz "menos de um quinto de peao" e passariam a
        // ser mais de um terco.
        //
        // E' o mesmo erro que a curva de vitoria/derrota tinha: um numero em
        // centipeoes so' significa o mesmo enquanto o centipeao significar o
        // mesmo. Ancorar na escala e' o que faz o contempt querer dizer sempre
        // a mesma fraccao de peao.
        let c = c * crate::nnue::escala() / 400;
        if c == 0 {
            return 0;
        }
        // Do ponto de vista de quem joga NESTE no', e negativo para o lado que
        // manda na raiz: e' a nos que um empate custa.
        if board.side == self.root_side { -c } else { c }
    }

    fn is_repetition_or_fifty(&self, board: &Board, hash: u64) -> bool {
        // DIAGNOSTICO (KESTREL_SEM_REPETICAO=1): desliga a deteccao de
        // repeticao para testar se e' ela a origem da explosao a varias
        // threads. O score de um no que repete depende do CAMINHO, e ainda
        // assim vai parar a uma TT partilhada -- outra thread le-o num
        // caminho onde nao havia repeticao. Nao e' para producao: sem isto o
        // motor nao evita linhas de empate.
        static SEM_REP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *SEM_REP.get_or_init(|| std::env::var_os("KESTREL_SEM_REPETICAO").is_some()) {
            return false;
        }
        if board.halfmove >= 100 {
            return true;
        }
        // conta ocorrencias da mesma posicao no historico real + no
        // caminho de busca ja percorrido (self.history acumula ambos)
        let mut cnt = 0;
        for &h in self.history.iter().rev().take(board.halfmove as usize + 1) {
            if h == hash {
                cnt += 1;
                if cnt >= 1 {
                    return true; // repeticao simples ja chega para evitar linhas de empate a repetir
                }
            }
        }
        false
    }

    /// O lado a jogar tem alguma peca alem de peoes e rei?
    /// (Condicao anti-zugzwang para o null-move pruning.)
    fn has_non_pawn_material(&self, board: &Board) -> bool {
        let us = board.side.idx();
        board.pieces[us][PieceType::Knight.idx()]
            | board.pieces[us][PieceType::Bishop.idx()]
            | board.pieces[us][PieceType::Rook.idx()]
            | board.pieces[us][PieceType::Queen.idx()]
            != 0
    }

    /// Does the opponent have a threat that wins material outright?
    ///
    /// Not "is anything attacked" -- specifically a piece of ours attacked by
    /// a CHEAPER enemy piece, which wins material on any exchange and does not
    /// need a search to confirm. Pure bitboard intersections against the
    /// attack tables, no move generation: this runs before the move list
    /// exists, and paying for the generator inside a test meant to avoid
    /// searching would defeat the purpose.
    ///
    /// Used to make reverse futility pruning careful. A margin that says "we
    /// are far enough ahead to skip this node" is a statement about the
    /// evaluation, and the evaluation does not know that a rook is hanging to
    /// a bishop. Where it is, the node deserves to be searched.
    fn opponent_has_winning_threat(&self, board: &Board) -> bool {
        let a = self.atk;
        let us = board.side.idx();
        let them = board.side.opp().idx();
        let occ = board.occ_all;

        let our_q = board.pieces[us][PieceType::Queen.idx()];
        let our_r = board.pieces[us][PieceType::Rook.idx()];
        let our_minor =
            board.pieces[us][PieceType::Knight.idx()] | board.pieces[us][PieceType::Bishop.idx()];

        // Pawns threaten anything above a pawn.
        let mut pawns = board.pieces[them][PieceType::Pawn.idx()];
        let valuable = our_q | our_r | our_minor;
        while pawns != 0 {
            let sq = pawns.trailing_zeros() as crate::types::Square;
            pawns &= pawns - 1;
            if a.pawn[them][sq as usize] & valuable != 0 {
                return true;
            }
        }
        // Minors threaten rooks and queens.
        let mut knights = board.pieces[them][PieceType::Knight.idx()];
        while knights != 0 {
            let sq = knights.trailing_zeros() as crate::types::Square;
            knights &= knights - 1;
            if a.knight[sq as usize] & (our_q | our_r) != 0 {
                return true;
            }
        }
        let mut bishops = board.pieces[them][PieceType::Bishop.idx()];
        while bishops != 0 {
            let sq = bishops.trailing_zeros() as crate::types::Square;
            bishops &= bishops - 1;
            if crate::attacks::bishop_attacks(sq, occ) & (our_q | our_r) != 0 {
                return true;
            }
        }
        // Rooks threaten queens.
        let mut rooks = board.pieces[them][PieceType::Rook.idx()];
        while rooks != 0 {
            let sq = rooks.trailing_zeros() as crate::types::Square;
            rooks &= rooks - 1;
            if crate::attacks::rook_attacks(sq, occ) & our_q != 0 {
                return true;
            }
        }
        false
    }

    fn mvv_lva(&self, board: &Board, mv: &Move) -> i32 {
        if !mv.is_capture() {
            return 0;
        }
        let victim = board.piece_at(mv.to).map(|(pt, _)| pt.value()).unwrap_or(100); // en passant = peao
        let attacker = board.piece_at(mv.from).map(|(pt, _)| pt.value()).unwrap_or(0);
        victim * 16 - attacker
    }

    /// Todas as pecas (ambas as cores) que atacam `sq` dada uma
    /// ocupacao HIPOTETICA `occ` (nao necessariamente `board.occ_all`
    /// -- usado pelo SEE para simular a troca a medida que remove
    /// pecas). Ataques de peao usam a tabela do lado CONTRARIO (truque
    /// classico: "que casas atacaria um peao preto aqui" = "que peoes
    /// brancos atacam aqui", por simetria do padrao diagonal).



    /// Bonus de ordenacao para lances que a Judit Polgar realmente jogou
    /// nesta posicao exata (1825 jogos reais, ver book.rs) -- cresce com
    /// a frequencia mas satura, para nunca competir com uma captura
    /// claramente boa (MVV-LVA fica sempre a frente). So' um empurrao de
    /// preferencia entre lances tranquilos que a busca ja consideraria
    /// razoaveis de qualquer forma. Recebe o hash JA CALCULADO (nunca o
    /// recalcula por lance -- bug de desempenho real corrigido: chegou a
    /// custar 3x o NPS por recalcular o zobrist inteiro por CANDIDATO em
    /// vez de uma vez por posicao).
    fn book_bonus(&self, book_entries: &[(u16, u32)], mv: &Move) -> i32 {
        if book_entries.is_empty() {
            return 0;
        }
        let target = encode_move(mv);
        for &(m16, cnt) in book_entries {
            if m16 == target {
                return 550 + (cnt as i32 * 10).min(200);
            }
        }
        0
    }

    /// Aplica bonus/malus de history heuristic -- ver campo `history_scores`.
    /// `depth*depth` e' a formula classica (peso maior quanto mais fundo o
    /// corte, um corte a profundidade alta diz muito mais sobre a
    /// qualidade real do lance do que um corte raso).
    fn update_history(&mut self, side: usize, mv: &Move, delta: i32) {
        let v = &mut self.history_scores[side][mv.from as usize][mv.to as usize];
        *v = apply_gravity(*v, delta, HISTORY_MAX);
    }

    /// Same bonus/malus shape as update_history, for captures -- keyed
    /// by (moving piece, captured piece) instead of (from, to), since
    /// what matters for "does this TYPE of capture tend to work out" is
    /// which pieces are involved, not the exact squares.
    fn update_capture_history(&mut self, side: usize, moving: PieceType, captured: PieceType, delta: i32) {
        let v = &mut self.capture_history[side][moving.idx()][captured.idx()];
        *v = apply_gravity(*v, delta, HISTORY_MAX);
    }

    /// Actualiza cont_hist para o par (prev_move, curr_move) -- +bonus
    /// se `curr_move` acabou de cortar beta em resposta a `prev_move`,
    /// -bonus para os quiets tentados antes que nao cortaram.
    /// `curr_pt` e a peca que fez `curr_mv` (piece_at(mv.from) no board
    /// ANTES do make_move). Ver campo cont_hist no Searcher.
    fn update_cont_hist(&mut self, prev_pt: PieceType, prev_to: crate::types::Square, curr_pt: PieceType, curr_to: crate::types::Square, delta: i32) {
        let idx = cont_hist_idx(prev_pt, prev_to, curr_pt, curr_to);
        let v = &mut self.cont_hist[idx];
        *v = apply_gravity(*v, delta, CONT_HIST_MAX);
    }

    /// Continuation-history score for moving `curr_pt` to `to` at `ply`:
    /// how this move has performed before in reply to the SAME preceding
    /// context, summed over the 1- and 2-ply lags (the same pair the move
    /// picker scores with). Read where a pruning/reduction decision needs
    /// the "is this move good IN THIS CONTEXT" signal -- plain history
    /// alone is context-free and rates a move identically no matter what
    /// was just played, which is exactly the blind spot continuation
    /// history fixes.
    ///
    /// Takes the piece type explicitly rather than looking it up: the two
    /// call sites read it from opposite ends (the pruning site runs BEFORE
    /// make_move, so the piece is still on `mv.from`; the LMR site runs
    /// AFTER, when it already sits on `mv.to`).
    #[inline]
    fn cont_hist_score(&self, curr_pt: PieceType, to: crate::types::Square, ply: usize) -> i32 {
        let mut ch = 0i32;
        if ply >= 1 {
            if let Some((p_pt, p_to)) = self.ply_last_move.get(ply).and_then(|x| *x) {
                ch += self.cont_hist[cont_hist_idx(p_pt, p_to, curr_pt, to)];
            }
        }
        if ply >= 2 {
            if let Some((p_pt, p_to)) = self.ply_last_move.get(ply - 1).and_then(|x| *x) {
                ch += self.cont_hist[cont_hist_idx(p_pt, p_to, curr_pt, to)];
            }
        }
        // Four plies back as well, not just one and two. One and two capture
        // the immediate exchange -- what the opponent just did and what we did
        // before that. Four reaches past it, to the move that set up the
        // structure the current one is working within, and a plan that takes
        // several moves to pay off is invisible at the shorter lags.
        if ply >= 4 {
            if let Some((p_pt, p_to)) = self.ply_last_move.get(ply - 3).and_then(|x| *x) {
                ch += self.cont_hist[cont_hist_idx(p_pt, p_to, curr_pt, to)];
            }
        }
        ch
    }

    #[inline]
    fn corr_idx(&self, board: &Board, hash: u64) -> usize {
        board.side.idx() * CORR_HIST_SIZE + (hash as usize % CORR_HIST_SIZE)
    }

    /// Static eval adjusted by the learned correction (see `corr_hist`
    /// and friends). Used only where the raw static eval feeds a
    /// PRUNING margin decision, never for the real leaf value.
    ///
    /// 2026-07-22/23: weighted sum of 6 correction-history dimensions
    /// (pawn structure, non-pawn material of each side, minor pieces,
    /// major pieces, threats), with SPSA-tuned per-term weights.
    /// Previously just the pawn term alone with an implicit weight of
    /// `CORR_HIST_GRAIN` (i.e. "full effect", no partial trust) -- the
    /// recalibrated pawn weight is 384/256 = 1.5x that, so this is a
    /// real recalibration of the existing term too, not just new
    /// additions. `threats`/continuation-history terms deliberately
    /// not included here, see the field doc comment on
    /// `corr_hist_np_stm` for why.
    fn corrected_static_eval(&self, board: &Board, raw: i32) -> i32 {
        let pawn_idx = self.corr_idx(board, pawn_structure_hash(board));
        let np_stm_idx = self.corr_idx(board, non_pawn_hash(board, board.side));
        let np_nstm_idx = self.corr_idx(board, non_pawn_hash(board, board.side.opp()));
        let minor_idx = self.corr_idx(board, minor_piece_hash(board));
        let major_idx = self.corr_idx(board, major_piece_hash(board));
        let threats_idx = self.corr_idx(board, threats_hash(board, self.atk));
        let sum = self.corr_hist[pawn_idx] * CORR_WEIGHT_PAWN
            + self.corr_hist_np_stm[np_stm_idx] * CORR_WEIGHT_NP_STM
            + self.corr_hist_np_nstm[np_nstm_idx] * CORR_WEIGHT_NP_NSTM
            + self.corr_hist_minor[minor_idx] * CORR_WEIGHT_MINOR
            + self.corr_hist_major[major_idx] * CORR_WEIGHT_MAJOR
            + self.corr_hist_threats[threats_idx] * CORR_WEIGHT_THREATS;
        raw + sum / (CORR_HIST_GRAIN * CORR_WEIGHT_SCALE)
    }

    /// Called once a node's real search has settled on `best_score`
    /// (not a stopped/aborted search, not a mate score, not near the
    /// static-eval-unreliable zone): nudge each of the 5 correction
    /// tables toward the gap between what the fast static eval guessed
    /// and what real search found. Small learning-rate style update so
    /// a single unusual position doesn't dominate any one table.
    /// Learning-rate cap raised from 16 to 32 (2026-07-22, following the
    /// form `weight = 2*min(1+depth,16)`) -- was under-weighting
    /// high-depth updates before.
    fn update_corr_hist(&mut self, board: &Board, static_eval: i32, best_score: i32, depth: i32) {
        if best_score.abs() >= MATE_THRESHOLD {
            return;
        }
        let diff = (best_score - static_eval) * CORR_HIST_GRAIN;
        let weight = 2 * (depth + 1).min(16);
        let pawn_idx = self.corr_idx(board, pawn_structure_hash(board));
        let np_stm_idx = self.corr_idx(board, non_pawn_hash(board, board.side));
        let np_nstm_idx = self.corr_idx(board, non_pawn_hash(board, board.side.opp()));
        let minor_idx = self.corr_idx(board, minor_piece_hash(board));
        let major_idx = self.corr_idx(board, major_piece_hash(board));
        let threats_idx = self.corr_idx(board, threats_hash(board, self.atk));
        for (table, idx) in [
            (&mut self.corr_hist, pawn_idx),
            (&mut self.corr_hist_np_stm, np_stm_idx),
            (&mut self.corr_hist_np_nstm, np_nstm_idx),
            (&mut self.corr_hist_minor, minor_idx),
            (&mut self.corr_hist_major, major_idx),
            (&mut self.corr_hist_threats, threats_idx),
        ] {
            let v = &mut table[idx];
            *v += (diff - *v) * weight / 256;
            *v = (*v).clamp(-CORR_HIST_MAX * CORR_HIST_GRAIN, CORR_HIST_MAX * CORR_HIST_GRAIN);
        }
    }

    fn order_moves(&self, board: &Board, mut moves: Vec<Move>, tt_move: Option<Move>, ply: usize, hash: Option<u64>) -> Vec<Move> {
        let killers = self.killers[ply];
        let side = board.side.idx();
        let book_entries: Vec<(u16, u32)> = match (self.style_book, hash) {
            (Some(b), Some(h)) => b.lookup(h),
            _ => Vec::new(),
        };
        // Countermove heuristic: look up whether there's a recorded reply
        // for the exact context that led into this node (the opponent's
        // last move, piece type + destination square).
        let countermove = self
            .ply_last_move
            .get(ply)
            .and_then(|x| *x)
            .and_then(|(pt, to)| self.countermoves[pt.idx()][to as usize]);
        // sort_by_cached_key (found in review, 2026-07-21), not
        // sort_by_key: the key closure calls see::see(self.atk, ) for every
        // capture, a full exchange simulation -- sort_by_key doesn't
        // guarantee calling the key function exactly once per element,
        // so that SEE could be recomputed more than once per move
        // during the sort. This runs in quiescence, visited far more
        // often than main-search nodes (every horizon leaf resolves
        // through it). Pure perf fix, same ordering/behavior either
        // way -- caching a key is never observable, only its cost is.
        moves.sort_by_cached_key(|m| {
            if Some(*m) == tt_move {
                -1_000_000
            } else if m.is_capture() {
                // SEE replaces plain MVV-LVA for ordering: good/neutral
                // captures (SEE>=0) go to the top, ranked by the real
                // exchange value (not just "bigger piece first"); bad
                // captures (SEE<0, lose material in the full exchange)
                // sink below quiet moves -- MVV-LVA couldn't tell "Bxf7"
                // against a defended bishop (loses the piece) apart from
                // a genuinely good capture.
                let see = see::see(self.atk, board, m);
                if see >= 0 {
                    -200_000 - see
                } else {
                    100_000 - see
                }
            } else if Some(*m) == killers[0] {
                -700 - self.book_bonus(&book_entries, m)
            } else if Some(*m) == killers[1] {
                -600 - self.book_bonus(&book_entries, m)
            } else {
                // Countermove folded in as an ADDITIVE bonus on top of
                // history, not a hard priority slot -- a single recorded
                // reply can be wrong; letting it outrank every other
                // quiet move unconditionally (as a fixed slot did) can
                // force a bad move to the front. Better to treat this
                // (continuation history) as a weighted signal blended
                // into the ordinary history score, not a rigid tier --
                // here simplified to a single ply-lag rather than the
                // full multi-lag sum.
                let h = self.history_scores[side][m.from as usize][m.to as usize];
                let cm_bonus = if Some(*m) == countermove { 2000 } else { 0 };
                -h - cm_bonus - self.book_bonus(&book_entries, m)
            }
        });
        moves
    }

    fn quiescence(&mut self, board: &mut Board, alpha: i32, beta: i32, ply: usize) -> i32 {
        // MEASURED AND REJECTED (2026-08-06), and worth knowing before
        // trying it: reusing a stored static eval here instead of
        // recomputing costs ~3.2% NPS rather than saving anything
        // (1123k -> 1087k, five interleaved runs, node count identical).
        //
        // 54% of all evaluations do come from this line, so the premise was
        // right -- but with the lazy accumulator in place, evaluating the
        // piece-square network here is reading an accumulator that is
        // usually already current plus one 2x512 output pass, while a table
        // probe is a cache miss into several megabytes. The cheap thing was
        // already the eval.
        //
        // This is NOT a general verdict. It should pay for the THREATS
        // architecture, whose evaluate() re-enumerates ~200 features from
        // scratch every call with no accumulator behind it -- there the
        // probe is far cheaper than what it replaces. Reference engines that
        // do probe here (Coda: ~990 probes per 1000 nodes, 44% static-eval
        // hits) have expensive evaluations, which is exactly the condition
        // that makes it worthwhile.
        let stand_pat = crate::evaluation::amortece_rule50(
            crate::evaluation::evaluate_fast(board),
            board.halfmove,
        );
        self.quiescence_from(board, alpha, beta, ply, stand_pat)
    }

    /// Same search as quiescence(), but also returns the board reached
    /// at the leaf of the best line found -- for tuning dataset prep
    /// (resolve a training position to a tactically quiet successor
    /// before scoring it with the candidate eval weights, instead of
    /// scoring a position mid-exchange). Not used by real gameplay
    /// search (negamax calls quiescence()/quiescence_from(), unchanged)
    /// -- purely additive, zero behavior change for the live engine.
    pub fn quiescence_leaf(&mut self, board: &mut Board, alpha: i32, beta: i32, ply: usize) -> (i32, Board) {
        let stand_pat = crate::evaluation::evaluate_fast(board);
        self.quiescence_leaf_from(board, alpha, beta, ply, stand_pat)
    }

    fn quiescence_leaf_from(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: usize, stand_pat: i32) -> (i32, Board) {
        self.nodes += 1;
        self.qnodes += 1;
        let leaf_here = board.clone();
        if self.time_up() || ply >= MAX_PLY - 1 {
            return (stand_pat, leaf_here);
        }
        let in_check = board.in_check(board.side, self.atk);
        if !in_check {
            if stand_pat >= beta {
                return (beta, leaf_here);
            }
            if stand_pat > alpha {
                alpha = stand_pat;
            }
        }
        let mut moves = generate_legal(board, self.atk);
        if in_check {
            if moves.is_empty() {
                return (-MATE_SCORE + ply as i32, leaf_here);
            }
        } else {
            moves.retain(|m| m.is_capture() || m.promotion == Some(PieceType::Queen));
            moves.retain(|m| !m.is_capture() || see::see_ge(self.atk, board, m, 0));
            if alpha.abs() < MATE_SCORE - MAX_PLY as i32 {
                let delta_margin = search_params().delta_margin;
                moves.retain(|m| {
                    if m.promotion.is_some() {
                        return true;
                    }
                    let captured_value = if m.flag == MoveFlag::EnPassant {
                        PieceType::Pawn.value()
                    } else {
                        board.piece_at(m.to).map(|(pt, _)| pt.value()).unwrap_or(0)
                    };
                    stand_pat + captured_value + delta_margin >= alpha
                });
            }
        }
        let moves = self.order_moves(board, moves, None, ply.min(MAX_PLY - 1), None);

        let mut best = if in_check { -MATE_SCORE - 1 } else { alpha };
        let mut best_leaf = leaf_here;
        for mv in moves {
            let undo = board.make_move(&mv);
            let (child_score, child_leaf) = self.quiescence_leaf(board, -beta, -alpha, ply + 1);
            let score = -child_score;
            board.unmake_move(&mv, &undo);
            if self.stop {
                return (if in_check { best.max(alpha) } else { alpha }, best_leaf);
            }
            let beats_beta = score >= beta;
            if score > best {
                best = score;
                if beats_beta {
                    return (beta, child_leaf);
                }
                best_leaf = child_leaf;
            } else if beats_beta {
                return (beta, child_leaf);
            }
            if score > alpha {
                alpha = score;
            }
        }
        if in_check { (best, best_leaf) } else { (alpha, best_leaf) }
    }

    /// Nucleo da quiescence, recebendo o stand-pat ja' calculado (completo
    /// na 1a chamada vinda do negamax, rapido nas recursoes seguintes --
    /// ver negamax()).
    fn quiescence_from(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: usize, stand_pat: i32) -> i32 {
        self.nodes += 1;
        self.qnodes += 1;
        if self.time_up() {
            return stand_pat;
        }
        // "Standing pat" (declining to search further, taking the
        // static eval as-is) is only a legal option when NOT in check
        // -- a side in check has no "do nothing" move, it MUST respond.
        // Applying the stand-pat cutoff/floor while in check would
        // silently accept an illegal null move and could miss forced
        // mates or misjudge a check sequence entirely. When in check,
        // every legal reply must be searched (not just captures), same
        // as the main search's check-evasion handling.
        let in_check = board.in_check(board.side, self.atk);
        if !in_check {
            if stand_pat >= beta {
                return beta;
            }
            if stand_pat > alpha {
                alpha = stand_pat;
            }
        }
        if ply >= MAX_PLY - 1 {
            return stand_pat;
        }

        // Out of check, generate only what quiescence searches instead of
        // generating everything and discarding the quiet moves -- see
        // `generate_legal_caps`. In check, every legal evasion counts, so the
        // full generator stays.
        let mut moves = if in_check {
            generate_legal(board, self.atk)
        } else {
            generate_legal_caps(board, self.atk)
        };
        if in_check {
            if moves.is_empty() {
                return -MATE_SCORE + ply as i32;
            }
            // No capture-only filter here: any legal evasion may be the
            // only way out (blocking, king move, or capturing the
            // checker). SEE pruning also doesn't apply -- a losing
            // capture can still be the only legal escape from check.
        } else {
            debug_assert!(moves.iter().all(|m| m.is_capture() || m.promotion.is_some()));
            moves.retain(|m| m.is_capture() || m.promotion == Some(PieceType::Queen));
            // Poda por SEE: uma captura que perde material na troca completa
            // (SEE negativo) quase nunca vale a pena dentro da quiescence --
            // e' exactamente o tipo de "captura mal calculada" que antes
            // era sempre pesquisada (MVV-LVA nao filtra nada, so' ordena).
            // Promocoes de dama ficam sempre (mv.to nao e' captura nesse
            // caso, is_capture()==false, so' entram aqui por causa do
            // OR acima -- SEE nao se aplica, `is_capture()` protege isso).
            moves.retain(|m| !m.is_capture() || see::see_ge(self.atk, board, m, 0));
            // Delta pruning: a capture that CANNOT reach alpha even in
            // the best case (winning the captured piece outright, a
            // looser bound than the full SEE exchange) isn't worth
            // trying at all -- cheaper pre-filter than SEE, applied
            // after it since SEE already thinned the list. Skipped
            // near mate scores (a fixed material margin isn't
            // meaningful there) and for promotions (potential gain is
            // much larger than a simple capture value suggests).
            if alpha.abs() < MATE_SCORE - MAX_PLY as i32 {
                let delta_margin = search_params().delta_margin;
                moves.retain(|m| {
                    if m.promotion.is_some() {
                        return true;
                    }
                    let captured_value = if m.flag == MoveFlag::EnPassant {
                        PieceType::Pawn.value()
                    } else {
                        board.piece_at(m.to).map(|(pt, _)| pt.value()).unwrap_or(0)
                    };
                    stand_pat + captured_value + delta_margin >= alpha
                });
            }
        }
        let moves = self.order_moves(board, moves, None, ply.min(MAX_PLY - 1), None);

        let mut best = if in_check { -MATE_SCORE - 1 } else { alpha };
        let mut tried = 0;
        for mv in moves {
            // Quiescence late move pruning: captures are already ordered
            // best-SEE-first and filtered to SEE>=0 above, so anything
            // past the first handful is very unlikely to be the one that
            // matters -- cap it, same spirit as LMP in the main search.
            // Never while in check (every legal reply must be tried
            // there, not just captures) and never near mate scores
            // (a fixed count isn't meaningful when the game is decided).
            if !in_check && tried >= search_params().qs_lmp_limit as usize && alpha.abs() < MATE_SCORE - MAX_PLY as i32 {
                break;
            }
            // History pruning inside quiescence. The move list here is
            // already filtered to captures that do not lose material, but
            // "does not lose material on this square" and "is worth
            // searching" are different questions, and the history tables
            // have an answer to the second one that SEE cannot give. A move
            // the tables have watched fail everywhere it has been tried is
            // not made promising by winning an exchange.
            //
            // Not while in check, where every reply must be tried, and not
            // once the score is already in mate territory, where a fixed
            // history threshold means nothing.
            if !in_check
                && best > -MATE_SCORE + MAX_PLY as i32
                && alpha.abs() < MATE_SCORE - MAX_PLY as i32
            {
                let h = if mv.is_capture() {
                    match (board.piece_at(mv.from), board.piece_at(mv.to)) {
                        (Some((moving, _)), Some((captured, _))) => {
                            self.capture_history[board.side.idx()][moving.idx()][captured.idx()]
                        }
                        _ => 0,
                    }
                } else {
                    let ch = match board.piece_at(mv.from) {
                        Some((pt, _)) => self.cont_hist_score(pt, mv.to, ply),
                        None => 0,
                    };
                    self.history_scores[board.side.idx()][mv.from as usize][mv.to as usize] + ch
                };
                if h < -search_params().qs_hist_prune_margin {
                    continue;
                }
            }
            tried += 1;
            let undo = board.make_move(&mv);
            let score = -self.quiescence(board, -beta, -alpha, ply + 1);
            board.unmake_move(&mv, &undo);
            if self.stop {
                return if in_check { best.max(alpha) } else { alpha };
            }
            if score > best {
                best = score;
            }
            if score >= beta {
                return beta;
            }
            if score > alpha {
                alpha = score;
            }
        }
        if in_check {
            best
        } else {
            alpha
        }
    }

    /// `reached_by_null`: was the move that led to THIS node a null
    /// move (see NMP block below)? 2026-07-22: needed for the
    /// double-null-move guard (`plies_from_null > 0`) -- consecutive
    /// null moves in the same line are unsound (can "prove" a fail-high
    /// via two passes that wouldn't survive a single one) and the
    /// aggressive eval-adaptive NMP reduction genuinely relies on this
    /// guard for safety; using the reduction formula without it caused a
    /// severe regression (A/B: 6.7% vs a pre-change baseline) -- ~93% of
    /// games lost, not a small/noisy signal, a real missing safety net.
    fn negamax(
        &mut self,
        board: &mut Board,
        depth: i32,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        reached_by_null: bool,
        cutnode: bool,
    ) -> i32 {
        self.nodes += 1;
        if depth <= 6 {
            self.nodes_shallow += 1;
        }
        if self.time_up() {
            return 0;
        }
        // Ply upper bound (found in review, 2026-07-21): check
        // extensions don't decrease depth (`depth - 1 + 1` when
        // in_check), so an unbroken chain of in-check plies never
        // shrinks `depth` to <=0 on its own -- ply keeps climbing on
        // every recursive call with nothing else to stop it. Extremely
        // unlikely in a real game (needs ~126+ consecutive checks
        // unbroken), but static_evals[ply-2] a few lines below is an
        // unguarded array read once ply passes MAX_PLY, so this would
        // panic (an instant loss mid-game) rather than fail safely.
        // quiescence_from() already has the equivalent guard; negamax
        // didn't.
        if ply >= MAX_PLY - 1 {
            return crate::evaluation::amortece_rule50(
                crate::evaluation::evaluate_fast(board),
                board.halfmove,
            );
        }

        let mut beta = beta;

        let hash = board.hash;
        if ply > 0 && self.is_repetition_or_fifty(board, hash) {
            return self.valor_empate(board);
        }

        // Mate distance pruning: se um mate mais curto do que o melhor
        // possivel a este ply ja' esta' garantido/impossivel de bater,
        // aperta a janela -- corte trivial e sempre correcto (nao
        // interfere com scores normais, so' com scores de mate).
        let mating_value = MATE_SCORE - ply as i32;
        if mating_value < beta {
            beta = mating_value;
            if alpha >= mating_value {
                return mating_value;
            }
        }
        let mated_value = -MATE_SCORE + ply as i32;
        if mated_value > alpha {
            alpha = mated_value;
            if beta <= mated_value {
                return mated_value;
            }
        }

        // Singular extensions: se estamos numa re-pesquisa singular
        // (excluded_move definido), ignorar TT probe/store por completo
        // -- a busca a janela restrita nao deve devolver cedo por TT
        // nem poluir a TT com scores enviesados por excluir um lance.
        let excluded = self.excluded_move;

        let orig_alpha = alpha;
        let mut tt_move = None;
        // Standard PVS convention: a null/scout window (beta == alpha+1)
        // means this is not a PV node. Used below for the extended TT
        // cutoff.
        let is_pv = beta - alpha > 1;
        // Propagate the double-extension count from the PARENT ply
        // unconditionally, every time this ply is visited -- not just
        // when a double extension is actually granted (bug found by
        // review 2026-07-22: `dextensions[ply]` was only ever WRITTEN
        // inside the double-extension branch below, so on every other
        // path through this ply -- normal/negative extension, multicut,
        // depth<8, no tt_move -- the slot kept whatever a PREVIOUS,
        // unrelated visit to this same ply left there during the DFS
        // -- sibling/cousin branches, not this line's real ancestor.
        // `ply` alone doesn't identify a line, only depth-in-tree, so
        // without this unconditional propagation the counter is a
        // shared watermark across unrelated branches instead of a
        // per-line counter, defeating the whole point of the cap).
        if ply > 0 {
            self.dextensions[ply] = self.dextensions[ply - 1];
        }
        let mut tt_entry_captured: Option<crate::tt::TtEntry> = None;
        if excluded.is_none() { if let Some(e) = self.tt.probe(hash) {
            tt_entry_captured = Some(e);
            tt_move = e.best;
            // score_from_tt(): converte o score guardado (relativo ao
            // no' onde foi escrito) para a escala deste no' -- ver nota
            // grande junto de score_to_tt/score_from_tt.
            let tt_score = score_from_tt(e.score, ply as i32);
            // MultiPV: a stored root entry can point at (or bound around)
            // a move we're deliberately excluding for this line -- skip
            // every TT-based shortcut/adjustment at the root while an
            // exclusion list is active, so the real move loop below
            // (which already filters excluded_root_moves) is always
            // reached instead of returning a cached result that ignores
            // the exclusion.
            let multipv_guard = ply == 0 && !self.excluded_root_moves.is_empty();
            if e.depth >= depth && !multipv_guard {
                match e.bound {
                    Bound::Exact => {
                        // 2026-07-20 (BUG REAL corrigido -- achado por
                        // instrumentacao directa num jogo real onde o
                        // motor jogou o "primeiro lance legal gerado" em
                        // vez do lance realmente escolhido pela busca,
                        // numa posicao completamente ganha): quando a TT
                        // ja tem um bound Exact suficiente para a raiz
                        // (ply==0), esta funcao retorna aqui SEM NUNCA
                        // passar pelo loop de lances mais abaixo -- que e'
                        // o unico sitio onde `self.root_best` era
                        // definido. Em jogos longos (TT acumulada ao
                        // longo de muitos `go`), isto podia fazer VARIAS
                        // iteracoes da iterative deepening (todas com
                        // `e.depth` >= profundidade pedida) devolverem
                        // sem NUNCA definir root_best, deixando toda a
                        // decisao do lance final refem da ULTIMA
                        // iteracao -- e se essa tambem fosse interrompida
                        // a meio (ver bug irmao em iterative_deepening()),
                        // `root_best` ficava None e o motor caia no
                        // fallback "primeiro lance legal", ignorando
                        // completamente o que a busca sabia.
                        if ply == 0 {
                            if let Some(tm) = tt_move {
                                self.root_best = Some(tm);
                            }
                        }
                        return tt_score;
                    }
                    Bound::Lower => {
                        if tt_score > alpha {
                            alpha = tt_score;
                        }
                    }
                    // 2026-07-20 (BUG REAL corrigido -- ver nota grande
                    // acima do ScoreFromTT): faltava apertar "beta" aqui
                    // -- o ramo "Upper" real de um alfa-beta com TT
                    // sempre aperta o limite CONTRARIO ao que "Lower"
                    // aperta (Lower sobe alpha, Upper desce beta), para
                    // o corte combinado "alpha>=beta" logo a seguir
                    // conseguir mesmo cortar quando aplicavel. O corpo
                    // vazio anterior fazia este ramo nunca contribuir
                    // para nenhum corte.
                    Bound::Upper => {
                        if tt_score < beta {
                            beta = tt_score;
                        }
                    }
                }
                if alpha >= beta {
                    if ply == 0 {
                        if let Some(tm) = tt_move {
                            self.root_best = Some(tm);
                        }
                    }
                    return tt_score;
                }
            } else if !is_pv
                && ply > 0
                && e.depth == depth - 1
                && e.bound == Bound::Upper
                && tt_score + search_params().tt_extended_cutoff_margin <= alpha
            {
                // Extended TT cutoff: a same-position entry exactly ONE
                // depth short of what's needed still short-circuits the
                // search if it already looked like a clear fail-low --
                // the entry says "this position tops out at tt_score or
                // below" one ply shallower, and tt_score is already well
                // under alpha even with a safety margin. A full re-search
                // at the requested depth would almost certainly just
                // confirm the same fail-low, so accept it now instead of
                // paying for the confirmation. Never at PV nodes (those
                // need the real answer, not a probable one).
                return alpha;
            }
        }}

        if depth <= 0 {
            // Ponto de entrada na quiescence: usa a avaliacao COMPLETA
            // (com os termos "Polgar") uma unica vez aqui, como stand-pat
            // inicial -- e' aqui que a riqueza posicional realmente
            // influencia a busca. Dentro da propria quiescence (resolucao
            // de capturas, que pode ter varios nos), usa-se a versao
            // rapida (ver quiescence()) para nao pagar o custo repetido.
            // Mesma reutilizacao que o caminho de profundidade > 0 faz dez
            // linhas abaixo: se a TT ja' tem a avaliacao completa desta
            // posicao, calcula-la outra vez e' repetir trabalho identico.
            // Aqui pesa mais do que la', porque a entrada da quiescencia e'
            // onde esta' a maioria dos nos.
            let raw_full_stand_pat = match tt_entry_captured
                .filter(|e| e.static_eval != crate::tt::TT_EVAL_NONE)
            {
                Some(e) => e.static_eval as i32,
                None => evaluate(board),
            };
            // Scaled at THIS node's halfmove, whether the raw value came
            // fresh or from a TT entry stored at some other node's halfmove
            // -- see `evaluation::amortece_rule50`.
            let full_stand_pat = crate::evaluation::amortece_rule50(raw_full_stand_pat, board.halfmove);
            return self.quiescence_from(board, alpha, beta, ply, full_stand_pat);
        }

        let in_check = board.in_check(board.side, self.atk);

        // Static eval computed once at each node (except while in check,
        // where it is meaningless); cached in `static_evals[ply]` so
        // the `improving` heuristic below can compare against 2 plies
        // back. Slight cost but pays off multiple times per node.
        //
        // Cached in the TT too: a TT hit on this position already has
        // the full eval computed by whichever earlier visit stored it,
        // so reuse it instead of paying for
        // `evaluate()` again -- this is what makes switching static
        // eval from evaluate_fast to the full evaluate() affordable.
        // Only the raw (uncorrected) eval is cached; corr-hist is
        // applied fresh below every time since it can change between
        // visits even for the same board.
        let raw_static_eval = if in_check {
            0
        } else if let Some(e) = tt_entry_captured.filter(|e| e.static_eval != crate::tt::TT_EVAL_NONE) {
            e.static_eval as i32
        } else {
            crate::evaluation::evaluate(board)
        };
        // Halfmove-scaled before correction (see `evaluation::amortece_rule50`
        // for why this has to happen here and not inside `evaluate()`
        // itself): `raw_static_eval` above may have come from a TT entry
        // stored at a different node's halfmove, so the scale is applied
        // fresh against THIS node's clock rather than baked into the cached
        // value.
        //
        // Corrected version (see corr_hist) used for pruning-margin
        // decisions below; the raw (unscaled, uncorrected) value is what
        // improving/static_evals track, since correction is a slow-moving
        // average and mixing it -- or the halfmove shrink -- into the
        // improving comparison would blur a signal that's meant to be about
        // THIS node's fast eval trend, not the learned bias or the clock.
        let static_eval = if in_check {
            0
        } else {
            let amortecido = crate::evaluation::amortece_rule50(raw_static_eval, board.halfmove);
            self.corrected_static_eval(board, amortecido)
        };
        if ply < MAX_PLY {
            self.static_evals[ply] = raw_static_eval;
        }
        // `improving`: at a same-side ply, are we better than 2 plies
        // ago (last time we moved)? If so, position is trending our
        // way -- afford tighter pruning; if not, we're stagnant/worse,
        // be more careful. Standard heuristic in every strong engine.
        let improving = !in_check
            && ply >= 2
            && raw_static_eval > self.static_evals[ply - 2];

        // Reverse futility pruning -- the quadratic curve that measured a
        // real win (+46 Elo, see the note this replaced), now with three
        // contextual nudges added back on top of THAT curve rather than a
        // different one.
        //
        // 2026-08-03: many strong engines modulate this margin by roughly
        // this trio -- an opponent capture in the air, an opponent trend,
        // continuation history -- and that is real evidence the idea earns
        // its keep, not just that one engine happened to like it. The
        // earlier attempt to test it here was not a fair test of the idea:
        // it mixed a reference's base slope with THIS engine's old
        // modulator constants (34/26/615), values nobody had tuned for that
        // base, and lost. This time the modulators are reasoned from what
        // this engine's own history actually produces (`hist_bonus_max` =
        // 2121, `hist_malus_max` = -992), not copied from any reference:
        // divisor 150 caps the history nudge around +/-14 at the extremes,
        // small next to a depth-1 base of 65; the easy-capture bonus (15)
        // and the worsening discount (12) are each roughly a fifth of that
        // same base -- present, not dominant. A hypothesis with its own
        // reasoning behind the numbers, to be measured on its own result.
        if !is_pv
            && !in_check
            && ply > 0
            && depth <= 6
            && beta.abs() < MATE_SCORE - MAX_PLY as i32
        {
            let sp = search_params();
            let mut margin = sp.rfp_step * depth * depth / 2 - sp.rfp_step * depth / 2 + sp.rfp_base * depth;

            // The opponent has a piece of ours attacked by a cheaper piece.
            // Material is about to change hands and the static evaluation
            // says nothing about it, so raise the bar for skipping.
            if self.opponent_has_winning_threat(board) {
                margin += sp.rfp_opp_easy_capture * depth;
            }

            // The opponent's position got worse over the last ply. Someone
            // losing ground is less likely to have a refutation waiting.
            if ply > 0 && self.static_evals[ply - 1] != 0
                && raw_static_eval > -self.static_evals[ply - 1] + 1
            {
                margin -= sp.rfp_opp_worsening;
            }

            // The move that led here had a good history score.
            if let Some((pt, to)) = self.ply_last_move[ply] {
                let prev_hist = self.cont_hist_score(pt, to, ply);
                margin += prev_hist / sp.rfp_hist_divisor.max(1);
            }

            let margin = (margin.max(20) * eval_margin_scale()) / 100;
            if static_eval - margin >= beta {
                self.cut_rfp += 1;
                return static_eval - margin;
            }
        }
        // Null-move pruning: se mesmo passando a vez ao adversario ainda
        // ficamos >= beta numa busca reduzida, a posicao e' tao boa que
        // podemos cortar ja'. Condicoes de seguranca:
        //  - nao em xeque (passar a vez em xeque e' ilegal/absurdo)
        //  - profundidade suficiente para a busca reduzida ter significado
        //  - lado a jogar tem pelo menos uma peca maior que peao (evita
        //    zugzwang, tipico de finais de peoes)
        //  - beta longe de scores de mate (nao mascarar mates)
        //  - nunca na raiz (ply > 0), para root_best ser sempre definido
        //
        // 2026-07-22: reducao "R" agora e' eval-adaptive, nao o antigo
        // `depth>6?3:2` fixo que ignorava completamente a avaliacao
        // estatica -- mecanismo genuinamente mais informado (quanto
        // mais a posicao excede beta, mais funda a reducao), nao so'
        // constantes recalibradas. A reducao usa `static_eval`, o valor
        // corrigido; os dois gates usam valores diferentes, e a razao
        // esta' explicada onde eles estao.
        // 2026-07-23: tentei uma busca de verificacao completa (R sem
        // cap + `nmp_min_ply` + re-busca real quando depth>15 e beta e'
        // quase decisivo) -- A/B isolado (300 jogos) deu 41.5%,
        // negativo e claro. Revertido para esta versao (formula do R,
        // `.max(1)` simples, sem cap artificial nem busca de
        // verificacao) -- e' a versao que já
        // tinha validado 50/50 (neutro) contra o estado anterior
        // (R fixo=4 por bug), que por sua vez já era +57.5% sobre o
        // baseline pre-NMP. Ver NOTAS_PROXIMA_SESSAO para o historico
        // completo.
        let sp_nmp = search_params();
        // Whole-node pruning belongs OUTSIDE the principal variation.
        // RFP, razoring, ProbCut and the null move all decide a node without
        // searching it properly, which is a trade the principal variation
        // cannot make: a wrong cut there does not lose a side branch, it
        // corrupts the line the engine is going to play and sends the search
        // back to redo it. Measured before this guard existed, the null move
        // was attempted in PV nodes and failed 100% of the time.
        if !is_pv
            && depth >= sp_nmp.nmp_min_depth
            && !in_check
            && ply > 0
            && (ply as i32) >= self.nmp_min_ply
            && !reached_by_null
            && excluded.is_none()
            && beta.abs() < MATE_SCORE - MAX_PLY as i32
            && self.has_non_pawn_material(board)
            // The two gates deliberately read different evaluations. The
            // narrow one (margin ~29) is the fine judgement of whether this
            // node is comfortably above beta, and it wants the CORRECTED
            // eval, which is what the search actually believes. The wide one
            // (margin ~193, relaxing with depth) is a floor: it exists to stop
            // us handing away a move in a position that only looks good
            // because correction history says so, and a floor built on the
            // corrected value cannot do that job. A 2026-07-23 review saw the
            // raw value here, read it as a copy-paste slip, and made both
            // gates corrected. It was not a slip.
            && static_eval >= beta + (sp_nmp.nmp_eval_margin * eval_margin_scale()) / 100
            && raw_static_eval
                >= beta
                    + (sp_nmp.nmp_static_eval_base_margin * eval_margin_scale()) / 100
                    - sp_nmp.nmp_static_eval_depth_margin * depth
        {
            let r = ((sp_nmp.nmp_base_reduction + depth * sp_nmp.nmp_depth_reduction_scale) / 256
                + ((static_eval - beta) / sp_nmp.nmp_eval_reduction_scale).min(sp_nmp.nmp_max_eval_reduction))
                .max(1);
            self.nmp_tried += 1;
            if is_pv {
                self.nmp_tried_pv += 1;
            }
            let undo = board.make_null_move();
            let score = -self.negamax(board, depth - r, -beta, -beta + 1, ply + 1, true, !cutnode);
            board.unmake_null_move(&undo);
            if self.stop {
                return 0;
            }
            if score < beta {
                self.nmp_failed_low += 1;
                if is_pv {
                    self.nmp_failed_pv += 1;
                }
            }
            if score >= beta {
                self.nmp_cutoff_raw += 1;
                // Verification, where skipping a move is not safe to trust.
                //
                // The null move assumes there is always something useful to
                // do. In zugzwang there is not, and the deeper the search and
                // the closer beta is to decisive, the more a wrong cut costs.
                // Below that, and inside a verification already running, the
                // cut is taken as before.
                //
                // The verification is a real search of the same reduced
                // depth, in a null window at beta, with the null move
                // disabled beneath it -- otherwise it would verify itself by
                // skipping a move again, which is what it exists to check.
                //
                // This engine had this, measured 41.5% in one 300-game A/B,
                // and removed it. That is a mechanism every strong engine
                // carries, deleted on a single measurement of one integration
                // of it.
                if (depth <= 15 && beta.abs() < MATE_SCORE - MAX_PLY as i32) || self.nmp_min_ply > 0 {
                    self.nmp_cut_taken += 1;
                    return beta;
                }
                self.nmp_verify_tried += 1;
                let saved = self.nmp_min_ply;
                self.nmp_min_ply = ply as i32 + (depth - r) * 3 / 4;
                let verify = self.negamax(board, depth - r, beta - 1, beta, ply, false, true);
                self.nmp_min_ply = saved;
                if self.stop {
                    return 0;
                }
                if verify >= beta {
                    self.nmp_verify_ok += 1;
                    return verify;
                }
                self.nmp_verify_failed += 1;
            }
        }

        // Razoring: a profundidade muito baixa, se a avaliacao estatica
        // mais uma margem generosa ainda fica abaixo de alfa, e' muito
        // improvavel que exista um lance tranquilo que recupere a
        // diferenca -- verifica-se com uma chamada real a quiescence
        // (nao um corte cego) e so' se aceita o resultado se confirmar
        // o fail-low, para nunca perder uma tactica real.
        if !is_pv && !in_check && ply > 0 && depth <= 3 {
            let sp = search_params();
            let margin = ((sp.razor_base + sp.razor_per_depth * (depth - 1)) * eval_margin_scale()) / 100;
            if static_eval + margin <= alpha {
                // `raw_static_eval` already IS `evaluate(board)` here (we are
                // under `!in_check`, so it was computed as the full eval on
                // entry, or reused from the TT's cached full eval of THIS same
                // position). Recomputing it would just repeat the now-expensive
                // full eval for an identical value -- reuse it instead. Exact:
                // node counts are unchanged, only the redundant eval is saved.
                let full_stand_pat = raw_static_eval;
                let q = self.quiescence_from(board, alpha, beta, ply, full_stand_pat);
                if q <= alpha {
                    self.cut_razor += 1;
                    return q;
                }
            }
        }

        // Internal Iterative Reduction (IIR): if there is no TT move at
        // a node that would otherwise search deep, drop the depth by 1
        // instead of running a full nested IID search (which costs a
        // sub-tree). The idea: without a TT hint the move ordering is
        // weaker, so an extra ply won't help much anyway -- better to
        // spend the time on the fully-ordered later iterations. Cheap
        // to implement, well-tested pattern.
        let mut depth = depth;
        if tt_move.is_none() && depth >= 4 && !in_check {
            depth -= 1;
        }

        let mut moves = generate_legal(board, self.atk);
        if moves.is_empty() {
            return if in_check { -MATE_SCORE + ply as i32 } else { 0 };
        }
        // MultiPV support (simple exclusion method): at the root only,
        // drop moves already reported by a previous MultiPV line so the
        // next call finds the next-best line instead of repeating the
        // same move. No effect on normal single-PV search (the list is
        // empty then).
        if ply == 0 && !self.excluded_root_moves.is_empty() {
            moves.retain(|m| !self.excluded_root_moves.contains(m));
            if moves.is_empty() {
                return if in_check { -MATE_SCORE + ply as i32 } else { 0 };
            }
        }

        // ProbCut: at reasonable depth, a capture that already beats a
        // margin ABOVE the real beta in a cheap verification search is
        // very likely to also beat the real beta with a full search --
        // cut immediately instead of paying for it. Guards: not in
        // check, not root (ply > 0, keeps root_best always defined),
        // not during a singular re-search (keeps TT semantics simple),
        // far from mate scores (never risk masking a real mate). The
        // `depth >= 5` floor and the margin below (was a hardcoded 150)
        // are the tuned parameters for this check.
        if !is_pv
            && depth >= 5
            && ply > 0
            && !in_check
            && excluded.is_none()
            && beta.abs() < MATE_SCORE - MAX_PLY as i32
        {
            let prob_beta = beta + search_params().probcut_beta_margin;
            if prob_beta < MATE_SCORE - MAX_PLY as i32 {
                for mv in &moves {
                    if !mv.is_capture() && mv.promotion.is_none() {
                        continue;
                    }
                    // SEE pre-filter: skip captures whose max plausible
                    // gain can't reach prob_beta from here anyway.
                    if !see::see_ge(self.atk, board, mv, prob_beta - static_eval) {
                        continue;
                    }
                    let undo = board.make_move(mv);
                    if ply + 1 < MAX_PLY {
                        if let Some((moved_pt, _)) = board.piece_at(mv.to) {
                            self.ply_last_move[ply + 1] = Some((moved_pt, mv.to));
                        }
                    }
                    // Cheap verification at depth 1, then a real (but
                    // reduced) search only if the quick probe holds up.
                    let mut score = -self.negamax(board, 1, -prob_beta, -prob_beta + 1, ply + 1, false, !cutnode);
                    if score >= prob_beta && !self.stop {
                        score = -self.negamax(board, depth - 4, -prob_beta, -prob_beta + 1, ply + 1, false, !cutnode);
                    }
                    board.unmake_move(mv, &undo);
                    if self.stop {
                        return 0;
                    }
                    if score >= prob_beta {
                        return score;
                    }
                }
            }
        }

        // Singular extensions: se o tt_move parece dominante (a TT diz "este
        // e' bom o suficiente" com bound Lower ou Exact e depth similar
        // a esta), testar se e' MESMO singular fazendo uma re-pesquisa
        // reduzida a excluir esse lance, numa janela restrita a volta
        // de `tt.score - m*depth`. Se nenhum outro lance chega la, e'
        // singular: estende +1 quando for a vez dele no picker.
        // Multi-cut: se um lance de reserva bate `beta` ate' na janela
        // restrita, corte seguro imediato.
        //
        // Aplicado a depth >= 8, fora da raiz, fora de re-pesquisa
        // singular, TT entry suficientemente fiavel.
        //
        // Nota: revertido uma vez a meio da sessao 2026-07-20 por causa
        // de A/B self-play de 30 jogos ter dado 50% -- decisao errada,
        // essa amostra nao tem resolucao para +Elo real e o padrao vem
        // do #1 em HCE puro. Restaurado. Ver
        // feedback_kestrel_nao_reverter_por_self_play_pequeno.
        let mut se_candidate: Option<Move> = None;
        let mut se_extension: i32 = 0;
        if excluded.is_none() && ply > 0 && depth >= 8 {
            if let (Some(tm), Some(te)) = (tt_move, tt_entry_captured) {
                let tt_score = score_from_tt(te.score, ply as i32);
                if te.depth >= depth - 3
                    && te.bound != Bound::Upper
                    && tt_score.abs() < MATE_THRESHOLD
                {
                    let s_beta = (tt_score - 2 * depth).max(-MATE_SCORE + 1);
                    let s_depth = (depth - 1) / 2;
                    self.excluded_move = Some(tm);
                    let s_score = self.negamax(board, s_depth, s_beta - 1, s_beta, ply, reached_by_null, cutnode);
                    self.excluded_move = None;
                    if self.stop {
                        return 0;
                    }
                    // Double extension: not just singular but
                    // singular by a WIDE margin (DOUBLE_EXT_MARGIN extra)
                    // -- extend 2 plies instead of 1. Capped via
                    // `dextensions` (read from the PARENT ply) so a run
                    // of double extensions along one line can't explode
                    // the tree; falls back to a normal +1 singular
                    // extension once the cap is hit.
                    let parent_dext = self.dextensions[ply.saturating_sub(1)];
                    if s_score < s_beta - DOUBLE_EXT_MARGIN && !is_pv && parent_dext <= DOUBLE_EXT_MAX {
                        se_candidate = Some(tm);
                        // Third ply for a QUIET move that is singular by an
                        // even wider margin. The restriction to quiets is the
                        // whole point: a capture can be singular simply
                        // because it is the only way to recapture, which says
                        // nothing about the line being forced. A quiet move
                        // that no alternative comes close to matching is one
                        // the position genuinely compels, and those are worth
                        // following further than anything else on the board.
                        se_extension =
                            if !tm.is_capture() && s_score < s_beta - search_params().triple_ext_margin {
                                3
                            } else {
                                2
                            };
                        self.dextensions[ply] = parent_dext + 1;
                    } else if s_score < s_beta {
                        se_candidate = Some(tm);
                        se_extension = 1;
                    } else if s_beta >= beta {
                        return s_beta;
                    } else if tt_score >= beta {
                        // Negative extension: the tt_move already looked
                        // like it beats beta at the CURRENT depth (not
                        // just the reduced verification depth) without
                        // triggering the multicut condition above -- a
                        // signal that a full-depth search here
                        // would likely just re-confirm the same cutoff,
                        // so shrink depth by 1 instead of granting an
                        // extension.
                        se_candidate = Some(tm);
                        se_extension = -1;
                    }
                }
            }
        }
        // Staged move picker: substitui o `order_moves` + `for mv in
        // moves` que pontuava TUDO upfront antes de sequer tentar o
        // primeiro lance. Ver `MovePicker` no fim deste ficheiro para as
        // fases e a motivacao.
        let killers = self.killers[ply.min(MAX_PLY - 1)];
        let mut picker = MovePicker::new(moves, tt_move, killers);

        let mut best_score = -MATE_SCORE - 1;
        let mut best_move = None;
        // Lances tranquilos experimentados neste no' ate' agora, para
        // aplicar malus de history heuristic se um lance POSTERIOR causar
        // o corte beta (ver update_history/history_scores).
        let mut quiets_tried: Vec<Move> = Vec::new();
        let mut captures_tried: Vec<(Move, PieceType, PieceType)> = Vec::new();
        let mut futility_eval: Option<i32> = None;
        self.history.push(hash);
        let mut i: usize = 0;
        while let Some(mv) = picker.next_move(self, board, ply.min(MAX_PLY - 1), hash) {
            // Late Move Pruning (LMP): at low depth, after already
            // trying enough quiet moves, skip the rest entirely
            // (unlike LMR which only reduces depth). Threshold grows
            // quadratically with depth; tighter when not improving.
            // Never in check, never on capture/promotion, never near
            // mate scores.
            if !is_pv
                && !in_check
                && depth <= 5
                && !mv.is_capture()
                && mv.promotion.is_none()
                && alpha.abs() < MATE_SCORE - MAX_PLY as i32
            {
                let lmp_threshold = if improving {
                    3 + depth * depth
                } else {
                    2 + depth * depth / 2
                };
                if (quiets_tried.len() as i32) >= lmp_threshold {
                    i += 1;
                    continue;
                }
            }

            // Futility pruning: quiet moves at low depth that cannot
            // beat alpha even with a generous margin over static eval
            // are usually not worth searching. Improving-aware: tighter
            // margin when position is trending well (afford more prune).
            if i > 0
                && !is_pv
                && !in_check
                && depth <= 6
                && !mv.is_capture()
                && mv.promotion.is_none()
                && alpha.abs() < MATE_SCORE - MAX_PLY as i32
            {
                let sp = search_params();
                let margin = (if improving { sp.futility_improving.at(depth) } else { sp.futility_not_improving.at(depth) }
                    * eval_margin_scale())
                    / 100;
                let fe = *futility_eval.get_or_insert(static_eval);
                if fe + margin <= alpha {
                    i += 1;
                    self.cut_futility += 1;
                    continue;
                }
            }

            // Noisy (capture) futility pruning: same idea as the quiet
            // version above, but for captures -- uses SEE (the real net
            // material swing of the full exchange, not just the target
            // piece's face value) as the realistic best case instead of
            // a flat piece-value guess. If even that can't reach alpha
            // with the margin, this capture isn't worth searching either.
            // Wider margin than the quiet case: a capture at least wins
            // back some material even when it's not tactically decisive,
            // so it needs more slack before being confidently dismissed.
            if i > 0
                && !is_pv
                && !in_check
                && depth <= 6
                && mv.is_capture()
                && mv.promotion.is_none()
                && alpha.abs() < MATE_SCORE - MAX_PLY as i32
            {
                let sp = search_params();
                let margin = (if improving { sp.cap_futility_improving.at(depth) } else { sp.cap_futility_not_improving.at(depth) }
                    * eval_margin_scale())
                    / 100;
                let fe = *futility_eval.get_or_insert(static_eval);
                let see_val = see::see(self.atk, board, &mv);
                if fe + see_val + margin <= alpha {
                    i += 1;
                    continue;
                }
            }

            // History pruning: late quiet moves at low depth whose
            // history score is strongly negative have consistently
            // failed to cause a cutoff in similar contexts before --
            // skip them outright instead of even a reduced search.
            // Separate signal from LMP (which is pure move-count) and
            // from LMR (which still searches, just shallower).
            if i >= 3
                && !is_pv
                && !in_check
                && depth <= search_params().hist_pruning_max_depth
                && !mv.is_capture()
                && mv.promotion.is_none()
                && alpha.abs() < MATE_SCORE - MAX_PLY as i32
            {
                // Main history PLUS continuation history: a move that looks
                // bad on average can still be the right reply to what was
                // just played (and vice versa). Pruning on the context-free
                // signal alone throws those away; summing both means a move
                // is only skipped when it is bad generally AND bad here.
                let ch = match board.piece_at(mv.from) {
                    Some((pt, _)) => self.cont_hist_score(pt, mv.to, ply),
                    None => 0,
                };
                let h = self.history_scores[board.side.idx()][mv.from as usize][mv.to as usize] + ch;
                if h < -search_params().history_prune_mult * depth {
                    i += 1;
                    continue;
                }
            }

            // SEE pruning: skip moves that lose material beyond a
            // depth-scaled allowance, judged purely by Static Exchange
            // Evaluation. Complements capture futility (which is
            // eval-relative): this fires on the raw material swing alone,
            // so it also catches quiets that hang a piece.
            //
            // ONLY at non-PV nodes. A first attempt without that gate
            // measured NEGATIVE (490 games, 48.9%): pruning on material
            // alone inside the principal variation throws away exactly the
            // speculative sacrifices this engine is built to find, on the
            // one line it will actually play. Off the PV, a scout search
            // is only trying to refute a move cheaply, so a material-losing
            // continuation is a fair thing to dismiss.
            //
            // Allowance shape: quadratic in depth for captures, linear for
            // quiets -- a losing capture at least resolves a tension and
            // deserves more slack deeper in the tree, while a quiet that
            // simply drops material rarely justifies itself. Margins are in
            // OUR eval units (pawn = 125, not the 100 most published
            // numbers assume), so they are scaled accordingly rather than
            // copied.
            if !is_pv
                && i > 0
                && depth <= 8
                && !in_check
                && alpha.abs() < MATE_SCORE - MAX_PLY as i32
            {
                let see_allowance = if mv.is_capture() {
                    -40 * depth * depth
                } else {
                    -100 * depth
                };
                if !see::see_ge(self.atk, board, &mv, see_allowance) {
                    i += 1;
                    continue;
                }
            }

            let root_nodes_before = if ply == 0 { self.nodes } else { 0 };
            let undo = board.make_move(&mv);
            if ply + 1 < MAX_PLY {
                if let Some((moved_pt, _)) = board.piece_at(mv.to) {
                    self.ply_last_move[ply + 1] = Some((moved_pt, mv.to));
                }
            }
            // Check extension + singular extension: se este e' o
            // tt_move provado singular acima, estende +1.
            let extend = if in_check {
                1
            } else if Some(mv) == se_candidate {
                se_extension
            } else {
                0
            };
            let score = if i == 0 {
                -self.negamax(board, depth - 1 + extend, -beta, -alpha, ply + 1, false, if is_pv { false } else { !cutnode })
            } else {
                // LMR: late quiet moves are usually not the best -- search
                // them at a reduced depth first, verify with full depth
                // only if promising. Logarithmic reduction (standard
                // shape) instead of hard tiers: smooth growth with depth
                // and move index. Never reduce captures/promotions/
                // checks/while escaping check. History-adjusted: a move
                // with strongly positive history gets less reduction
                // (it's usually been good here before), strongly
                // negative gets more.
                let gives_check = board.in_check(board.side, self.atk);
                // 2026-07-22: min-move-count gate now uses a per-node-type
                // split (PV min 4 moves, non-PV min 3, min depth 3)
                // instead of a single `i>=2, depth>=2`
                // threshold for every node type -- PV nodes get one
                // extra move of "trust" before LMR kicks in, since a PV
                // node's move ordering has already earned more
                // confidence than a non-PV scout node's. Pure integer
                // threshold, no fixed-point/scale conversion involved
                // (unlike the NMP/corr-hist calibrations above), so
                // applied directly without the caution those needed.
                let min_moves = if is_pv { 4 } else { 3 };
                // Which condition is granting immunity to quiet moves. The
                // reductions we DO apply almost never need a re-search (1.5%),
                // which is not a healthy sign -- it says we only reduce what
                // was obviously bad already. So the question is not how hard
                // we reduce, it is how much never reaches the reduction at
                // all, and which test is letting it past.
                if !mv.is_capture() && mv.promotion.is_none() {
                    self.lmr_quiet_total += 1;
                    if gives_check {
                        self.lmr_skip_check += 1;
                    } else if depth < 3 {
                        self.lmr_skip_depth += 1;
                    } else if extend != 0 {
                        self.lmr_skip_extend += 1;
                    } else if i < min_moves {
                        self.lmr_skip_early += 1;
                    }
                }
                let r = if i >= min_moves
                    && depth >= 3
                    && extend == 0
                    && !mv.is_capture()
                    && mv.promotion.is_none()
                    && !gives_check
                    && !peao_avancado(board, &mv)
                {
                    let base = lmr_table()[(depth as usize).min(63)][(i + 1).min(63)];
                    // BUG FIX (2026-07-25): this runs AFTER make_move, so
                    // `board.side` is already the OPPONENT -- indexing the
                    // history table with it read the wrong side's stats
                    // entirely (history is written with the mover's index,
                    // see update_history at the cutoff below). Use the side
                    // that actually played `mv`.
                    let mover = board.side.opp().idx();
                    let h = self.history_scores[mover][mv.from as usize][mv.to as usize];
                    // 2026-07-23: divisor was a hand-set guess (4000);
                    // retuned to 8846, our HISTORY_MAX being 16000. Same
                    // divisor as before -- now applied in milli-plies, so
                    // it stops being all-or-nothing: h=8845 used to ask for
                    // -1.000 ply and get 0.
                    let hist_adj = -(h * LMR_ESCALA / 8846);
                    // TTPV: this position was reached by a real PV search
                    // before (full window, not a scout probe) -- reduce
                    // one ply less here. A position that earned
                    // full-window search once is less likely to be a
                    // safe-to-skip wasteland.
                    let ttpv_adj = if tt_entry_captured.map(|e| e.pv).unwrap_or(false) { -LMR_ESCALA } else { 0 };
                    // Continuation history as its OWN reduction term, with
                    // its own divisor -- deliberately NOT folded into `h`
                    // above. It sums two lags, so its range is ~2x the main
                    // history's; adding it into `h` and reusing the 8846
                    // divisor silently tripled the reduction swing and
                    // measured neutral (1247 games, 50.4%). Capped at ~1 ply
                    // on its own, as before -- the cap is unchanged, only
                    // the values inside it are now continuous instead of
                    // snapping to -1/0/+1. The piece already sits on
                    // `mv.to` at this point (post-make_move).
                    let cont_adj = match board.piece_at(mv.to) {
                        Some((pt, _)) => {
                            let ch = self.cont_hist_score(pt, mv.to, ply);
                            (-(ch * LMR_ESCALA / search_params().lmr_hist_divisor.max(1)))
                                .clamp(-LMR_ESCALA, LMR_ESCALA)
                        }
                        None => 0,
                    };
                    // Corrplexity: reduce ~one ply less when
                    // |eval-staticEval| > 89 --
                    // i.e. when the correction-history signal
                    // says this position's static eval is trending far
                    // from what raw material/PST said (a "complex"
                    // position where blind reduction is riskier).
                    // A threshold term: either the position is complex or it
                    // is not, so this one is genuinely a whole ply -- it just
                    // says so in milli-plies now, like the rest.
                    let corrplexity = (static_eval - raw_static_eval).abs();
                    let corrplexity_adj = if corrplexity > 89 { -LMR_ESCALA } else { 0 };
                    // Reduce MORE (one whole ply) when !improving -- the same
                    // `improving` signal RFP/futility already use. Also a
                    // threshold, also exactly one ply.
                    let non_imp_adj = if !improving { LMR_ESCALA } else { 0 };
                    // The two compile-time terms. Both 0 unless their build
                    // variable was set, so the default binary is unchanged.
                    let cutnode_adj = if cutnode { LMR_CUTNODE } else { 0 };
                    let move_linear_adj = -LMR_MOVE_LINEAR * (i as i32 + 1);
                    let r_milli = base
                        + hist_adj
                        + cont_adj
                        + ttpv_adj
                        + corrplexity_adj
                        + non_imp_adj
                        + cutnode_adj
                        + move_linear_adj;
                    // ONE division, at the end. Clamped in milli-plies first so
                    // the ceiling means the same thing it did before.
                    r_milli.clamp(0, (depth - 1) * LMR_ESCALA) / LMR_ESCALA
                } else {
                    0
                };
                // PVS: janela nula primeiro (reduzida se LMR), re-pesquisa se prometedor.
                // doDeeper/doShallower: depois de a re-pesquisa reduzida
                // bater alpha, ajusta a profundidade da re-pesquisa
                // +/-1 conforme bateu alpha por muito (1 ply mais fundo,
                // lance invulgarmente forte) ou por pouco (1 ply mais
                // raso, poupa tempo). 2026-07-23: a PRIMEIRA versão usou
                // margens RAW (36/141/8) vindas de uma escala de eval
                // ~1.92x mais pequena que a nossa (peão 65 vs 125) e a
                // bisecção localizou-a como o maior culpado da regressão
                // do dia (-6.2%) -- as margens comparam com SCORES na
                // escala de eval do Kestrel, por isso os valores raw
                // disparavam "mais fundo" ~2x mais depressa do que
                // deviam. Recalibradas pela razão do peão (69/271/15) --
                // mecanismo mantido, valores corrigidos (ponto do
                // utilizador: "não são as funções que estão mal, mas a
                // calibração dos valores").
                let new_depth = depth - 1 + extend;
                let mut research_depth = new_depth;
                // Floor the reduced DEPTH at 1 (never 0 = plain quiescence),
                // exactly as the reference does: max(newDepth - reduction, 1).
                let reduced_depth = (new_depth - r).max(1);
                let probe_cutnode = if r > 0 { true } else { !cutnode };
                let mut s = -self.negamax(board, reduced_depth, -alpha - 1, -alpha, ply + 1, false, probe_cutnode);
                if r > 0 {
                    self.lmr_tried += 1;
                    self.lmr_sum += r as u64;
                }
                if r > 0 && s > alpha && !self.stop {
                    // A reduced search that beats alpha has to be redone at
                    // full depth, so this branch cost MORE than not reducing
                    // it. The share of reductions that end up here is what
                    // decides whether LMR is saving nodes or buying them --
                    // a healthy engine re-searches a small minority.
                    self.lmr_research += 1;
                    let sp = search_params();
                    let do_deeper = (s > best_score + sp.do_deeper_margin_base + sp.do_deeper_margin_depth * new_depth / 64) as i32;
                    let do_shallower = (s < best_score + sp.do_shallower_margin) as i32;
                    research_depth = (new_depth + do_deeper - do_shallower).max(1);
                    s = -self.negamax(board, research_depth, -alpha - 1, -alpha, ply + 1, false, !cutnode);
                }
                if s > alpha && s < beta && !self.stop {
                    s = -self.negamax(board, research_depth, -beta, -alpha, ply + 1, false, false)
                }
                s
            };
            board.unmake_move(&mv, &undo);
            if !mv.is_capture() {
                quiets_tried.push(mv);
            } else if let Some((moving_pt, _)) = board.piece_at(mv.from) {
                // Post-unmake board has the captured piece restored at
                // mv.to (except en passant, where it's a pawn beside
                // mv.to, not on it -- handled by the flag check instead
                // of relying on board state for that one case).
                let captured_pt = if mv.flag == MoveFlag::EnPassant {
                    PieceType::Pawn
                } else {
                    board.piece_at(mv.to).map(|(pt, _)| pt).unwrap_or(PieceType::Pawn)
                };
                captures_tried.push((mv, moving_pt, captured_pt));
            }
            if ply == 0 {
                let delta = self.nodes.saturating_sub(root_nodes_before);
                if let Some(entry) = self.root_move_nodes.iter_mut().find(|(m, _)| *m == mv) {
                    entry.1 += delta;
                } else {
                    self.root_move_nodes.push((mv, delta));
                }
            }

            // BUG corrigido (2026-07-20, achado num jogo real na Arena --
            // "bestmove 0000" a meio de uma posicao completamente ganha):
            // a busca do 1o lance-filho pode terminar e devolver um
            // resultado valido no EXATO momento em que o relogio esgota
            // (self.stop passa a true dentro da recursao). O codigo antigo
            // verificava self.stop ANTES de guardar o resultado, deitando
            // fora um lance perfeitamente valido -- se isto acontecesse em
            // TODAS as profundidades (incl. profundidade 1), root_best
            // nunca chegava a ser definido e o motor devolvia lance nulo.
            // Agora guarda-se sempre o resultado do lance que JA terminou;
            // so' se para de explorar MAIS lances depois disso.
            // Root trace: what every root move actually scored, in what
            // window, at what depth. Set KESTREL_ROOT_TRACE to switch on.
            //
            // This exists because "the engine played the wrong move" is not
            // a debuggable statement -- the interesting question is what the
            // search believed about each alternative at the moment it chose,
            // and no other output shows that. It was this trace that showed
            // the losing pattern: the first move fixes alpha, every later
            // move is then searched with a null window, and a quiet move
            // whose value only appears deeper fails low there and is never
            // re-searched wide enough to reveal it.
            if ply == 0 && root_trace() {
                eprintln!(
                    "ROOT d={} i={} mv={} score={} alpha={} beta={} best={}",
                    depth, i, mv.to_uci(), score, alpha, beta, best_score
                );
            }
            if ply == 0 {
                // Bookkeeping per root move, adapted from how a stronger
                // engine does it -- the details matter and each one was
                // learned by getting it wrong first.
                //
                // The previous score is saved on EVERY visit, before anything
                // else: it is the tiebreak that stops a score which merely
                // wobbles from changing the decision.
                //
                // The current score is written only for the first move, whose
                // full window makes it a value, and for moves that raise
                // alpha, which get the full-window re-search. Anything else
                // is INVALIDATED rather than left alone. That last part is
                // the one that matters: leaving a stale score behind lets a
                // move measured two iterations ago at a shallower depth
                // compete against one measured now, and it made this engine
                // open 1.d3 instead of 1.d4.
                let idx = match self.root_scores.iter().position(|(m, _, _)| *m == mv) {
                    Some(i) => i,
                    None => {
                        self.root_scores.push((mv, NO_SCORE, NO_SCORE));
                        self.root_scores.len() - 1
                    }
                };
                self.root_scores[idx].2 = self.root_scores[idx].1;
                // A move whose own search was cut by the clock has no score to
                // record. The return value of an aborted search is whatever
                // the partial window held -- in practice 0 -- and because the
                // first move is stored unconditionally, that 0 went in as if
                // it were measured. It then became the reported evaluation:
                // a position worth -12 published as 0.00, and 0.00 fed to the
                // next move's aspiration window. Unmeasured is the truth here,
                // and the previous iteration's value in .2 is what answers.
                self.root_scores[idx].1 = if self.stop {
                    NO_SCORE
                } else if i == 0 || score > alpha {
                    score
                } else {
                    NO_SCORE
                };
            }
            // Um lance cuja propria busca o relogio cortou nao tem score, e
            // isso ja' foi dito dez linhas acima ao gravar NO_SCORE em
            // root_scores: "Unmeasured is the truth here". Deixa-lo competir
            // aqui era usar exactamente o numero que acabamos de declarar
            // invalido -- o valor de uma busca abortada e' o que a janela
            // parcial tinha, na pratica 0.
            //
            // Num posicao perdida TODOS os lances reais pontuam negativo,
            // portanto esse 0 ganha-lhes a todos e o lance jogado passa a ser
            // aquele em que o relogio calhou de cortar. Medido: a mesma
            // posicao, 1 thread, mesmo relogio, cinco corridas -- tres lances
            // diferentes (518k-604k nos, todos a -95cp). Um motor de
            // referencia nas mesmas condicoes devolveu o mesmo lance e o mesmo
            // numero de nos as cinco vezes. A instabilidade era nossa, e era
            // aqui.
            //
            // O jogo IZt573pD perdeu-se assim: 27.Rh3 num posicao que a busca
            // completa avalia a -705.
            let medido = !(ply == 0 && self.stop);
            if medido && score > best_score {
                best_score = score;
                best_move = Some(mv);
                if ply == 0 {
                    self.root_best = Some(mv);
                }
            }
            if self.stop {
                self.history.pop();
                return best_score;
            }
            if score > alpha {
                alpha = score;
            }
            if alpha >= beta {
                // Move-ordering telemetry. The share of beta cutoffs produced
                // by the FIRST move tried is the number that decides how wide
                // this tree is: every cutoff that takes until the fifth move
                // has paid for four subtrees nobody needed. Measured against
                // a reference at the same depth we visit 2.76 nodes per node
                // where it visits 2.48, and that gap is where it lives.
                self.cut_nodes += 1;
                if i == 0 {
                    self.cut_first += 1;
                }
                // How much a cutoff is worth to the history tables is not the
                // same as how deep the search was. A move that beats beta by a
                // wide margin refuted the node outright; one that scrapes past
                // it by a centipawn may not survive one more ply. Crediting
                // both identically teaches the ordering that a marginal move
                // is as trustworthy as a decisive one. A comfortable cutoff is
                // scored as if the search had been a ply deeper.
                let hist_depth =
                    depth + (best_score > beta + search_params().hist_beta_margin) as i32;
                if !mv.is_capture() && ply < MAX_PLY {
                    let k = &mut self.killers[ply];
                    if k[0] != Some(mv) {
                        k[1] = k[0];
                        k[0] = Some(mv);
                    }
                    // History heuristic: bonus para o lance que cortou,
                    // malus para os lances tranquilos anteriores neste
                    // no' que NAO cortaram (quiets_tried inclui `mv` como
                    // ultimo elemento, ja' que foi empurrado logo acima --
                    // excluido do malus).
                    let bonus = history_bonus(hist_depth);
                    let malus = history_malus(hist_depth);
                    let side = board.side.idx();
                    self.update_history(side, &mv, bonus);
                    let n = quiets_tried.len().saturating_sub(1);
                    for qm in &quiets_tried[..n] {
                        self.update_history(side, qm, -malus);
                    }
                    // Countermove heuristic (binario) mantido para
                    // compatibilidade; cont_hist e' o sinal principal.
                    if let Some((ctx_pt, ctx_to)) = self.ply_last_move[ply] {
                        self.countermoves[ctx_pt.idx()][ctx_to as usize] = Some(mv);
                    }
                    // Continuation history: actualiza (prev_move -> mv)
                    // com +bonus para mv que cortou, -bonus para os
                    // quiets tentados antes. Feito a 1-ply e 2-ply back
                    // (multi-lag, ver `cont_hist`). Precisamos da peca
                    // que fez mv -- board ja' fez unmake, portanto o
                    // piece_at do mailbox devolve o estado ANTES do mv,
                    // que e' exactamente o que queremos.
                    if let Some((curr_pt, _)) = board.piece_at(mv.from) {
                        let prev1 = if ply >= 1 { self.ply_last_move.get(ply).and_then(|x| *x) } else { None };
                        let prev2 = if ply >= 2 { self.ply_last_move.get(ply - 1).and_then(|x| *x) } else { None };
                        // The lag-4 entry has to be written as well as read,
                        // or the accessor sums a table nothing ever fills.
                        let prev4 = if ply >= 4 { self.ply_last_move.get(ply - 3).and_then(|x| *x) } else { None };
                        if let Some((p4_pt, p4_to)) = prev4 {
                            self.update_cont_hist(p4_pt, p4_to, curr_pt, mv.to, bonus);
                        }
                        if let Some((p1_pt, p1_to)) = prev1 {
                            self.update_cont_hist(p1_pt, p1_to, curr_pt, mv.to, bonus);
                        }
                        if let Some((p2_pt, p2_to)) = prev2 {
                            self.update_cont_hist(p2_pt, p2_to, curr_pt, mv.to, bonus);
                        }
                        for qm in &quiets_tried[..n] {
                            if let Some((q_pt, _)) = board.piece_at(qm.from) {
                                if let Some((p4_pt, p4_to)) = prev4 {
                                    self.update_cont_hist(p4_pt, p4_to, q_pt, qm.to, -malus);
                                }
                                if let Some((p1_pt, p1_to)) = prev1 {
                                    self.update_cont_hist(p1_pt, p1_to, q_pt, qm.to, -malus);
                                }
                                if let Some((p2_pt, p2_to)) = prev2 {
                                    self.update_cont_hist(p2_pt, p2_to, q_pt, qm.to, -malus);
                                }
                            }
                        }
                    }
                } else if mv.is_capture() {
                    // Capture history: same bonus/malus shape as the
                    // quiet-move history above, keyed by (moving,
                    // captured) piece type instead of (from, to).
                    // Complements SEE in ordering (see MovePicker) --
                    // never touches SEE itself.
                    let bonus = history_bonus(hist_depth);
                    let malus = history_malus(hist_depth);
                    let side = board.side.idx();
                    let n = captures_tried.len().saturating_sub(1);
                    if let Some(&(_, moving_pt, captured_pt)) = captures_tried.last() {
                        self.update_capture_history(side, moving_pt, captured_pt, bonus);
                    }
                    for &(_, moving_pt, captured_pt) in &captures_tried[..n] {
                        self.update_capture_history(side, moving_pt, captured_pt, -malus);
                    }
                }
                break;
            }
            i += 1;
        }
        self.history.pop();

        let bound = if best_score <= orig_alpha {
            Bound::Upper
        } else if best_score >= beta {
            Bound::Lower
        } else {
            Bound::Exact
        };
        // score_to_tt(): guarda relativo a ESTE no' (nao a raiz) -- ver
        // nota grande junto de score_to_tt/score_from_tt. Nao guardar
        // durante uma re-pesquisa singular -- score enviesado por
        // janela restrita e excluded_move.
        if excluded.is_none() {
            // TTPV: OR with whatever the entry already had, not just
            // this node's own window -- fix from code review
            // (2026-07-22): with always-replace storage, a PV-written
            // entry gets overwritten by the next (far more common)
            // scout-window visit to the same position, erasing the
            // flag almost immediately. Once true, stays true across
            // subsequent non-PV writes to the same slot
            // (`store_pv = is_pv || (hit && old.pv)`).
            let store_pv = is_pv || tt_entry_captured.map(|e| e.pv).unwrap_or(false);
            let tt_static_eval = if in_check {
                crate::tt::TT_EVAL_NONE
            } else {
                raw_static_eval.clamp(i16::MIN as i32, i16::MAX as i32) as i16
            };
            self.tt.store(hash, depth, score_to_tt(best_score, ply as i32), bound, best_move, store_pv, tt_static_eval);
            // Correction history update: only on a genuine Exact result
            // (fail-high/fail-low bounds are one-sided, not a real
            // estimate of the true value) and never in check (tactics
            // dominate there, not the slow eval-bias signal we want).
            if !self.stop && !in_check && bound == Bound::Exact {
                self.update_corr_hist(board, raw_static_eval, best_score, depth);
            }
        }

        best_score
    }

    /// Busca na raiz com aspiration windows: profundidade 1 usa sempre
    /// janela total (referencia inicial). Profundidades seguintes tentam
    /// primeiro uma janela estreita centrada no score da iteracao
    /// anterior -- corta muito mais no resto da arvore -- e alarga
    /// (dobra o delta) e repete se falhar por baixo ou por cima, ate'
    /// obter um score dentro da janela ou o tempo esgotar.
    ///
    /// 2026-07-23: tentei substituir por uma formula mais elaborada
    /// (delta escalado por prev_score^2, janela total abaixo de
    /// profundidade 6, alargamento ~1.18x em vez de 2x, fail-high com
    /// profundidade reduzida) -- A/B isolado (300 jogos)
    /// deu 39% negativo, claro e fora do ruido. Combinado com o
    /// terceiro dado negativo ja' existente para esta area (versao
    /// antiga isolada: 33%), tres sinais independentes na mesma
    /// direccao -- revertido para esta versao (delta fixo=25, janela
    /// estreita desde profundidade 2, dobra sempre), que E' a versao
    /// que os testes em lote (futility/RFP/razoring/mate-distance)
    /// validaram como positiva em conjunto. Ver NOTAS_PROXIMA_SESSAO
    /// para o historico completo.
    fn search_root(
        &mut self,
        board: &mut Board,
        depth: i32,
        prev_score: i32,
        _root_average: &mut Option<i32>,
    ) -> i32 {
        if depth <= 1 {
            return self.negamax(board, depth, -MATE_SCORE - 1, MATE_SCORE + 1, 0, false, false);
        }
        // 2026-08-13: tried a napv10-gated sliding-average variant here
        // (Nap2Siriux's aspWindowsNNUE(), centers on a running average
        // instead of prev_score, asymmetric 52/256 vs 120/256 widening).
        // Measured WORSE against the same SF1800 baseline than the plain
        // version below (21.7% vs the un-gated 35% baseline), on top of an
        // already-documented history of three independent negative signals
        // for touching this function at all. Reverted; not worth the added
        // surface for an unproven gain. `_root_average` kept as a parameter
        // so the call site does not have to change again if this is
        // revisited with better evidence.
        // Adopted whole, rather than assembled a piece at a time.
        //
        // Every part of this had a reason behind it in the engine it comes
        // from, and the parts work together: the starting width, what happens
        // on each kind of failure, and how the width grows are one mechanism,
        // not four options. Taking them singly and vetoing each against our
        // own baseline is how a package that works ends up rejected in
        // fragments -- which is what happened here before.
        //
        // Four things this does that a plain "widen the failing side and
        // double" does not:
        //
        //  * the starting width scales with the previous score, because a
        //    position already far from equal moves in bigger steps than one
        //    near it;
        //  * a full window below a minimum depth, where the previous score is
        //    too unreliable to aim at;
        //  * on a fail-low, beta comes DOWN to the midpoint as alpha goes
        //    out. The score is below the window, so the top of it was never
        //    the question, and leaving it high keeps searching a range we
        //    already know is empty;
        //  * on a fail-high, the re-search goes one ply SHALLOWER, floored
        //    five below. A move that beats the window will beat it at less
        //    depth too, and paying full depth to confirm it is what makes
        //    fail-highs expensive.
        let sp = search_params();
        // A janela e' o que separa as threads umas das outras agora. Cada uma
        // parte de uma largura propria, portanto falha alto/baixo em pontos
        // diferentes e explora ordens diferentes -- mesma ideia do
        // `5 + threadIdx % 8` do Stockfish, escrita nas nossas unidades.
        let mut delta: i32 = sp.asp_init_delta
            + (self.thread_idx as i32 % 8)
            + prev_score * prev_score / 16384;
        let (mut alpha, mut beta) = if depth >= sp.min_asp_depth {
            (
                (prev_score - delta).max(-MATE_SCORE - 1),
                (prev_score + delta).min(MATE_SCORE + 1),
            )
        } else {
            (-MATE_SCORE - 1, MATE_SCORE + 1)
        };
        let mut asp_depth = depth;
        let nos_antes = self.nodes;
        loop {
            let score = self.negamax(board, asp_depth.max(1), alpha, beta, 0, false, false);
            if self.stop {
                return score;
            }
            self.asp_re += 1;
            self.asp_nos = self.asp_nos.saturating_add(self.nodes - nos_antes);
            if score <= alpha {
                beta = (alpha + beta) / 2;
                alpha = (alpha - delta).max(-MATE_SCORE - 1);
                asp_depth = depth;
            } else if score >= beta {
                beta = (beta + delta).min(MATE_SCORE + 1);
                asp_depth = (asp_depth - 1).max(depth - 5);
            } else {
                return score;
            }
            delta += delta * sp.asp_widening_factor / 256;
        }
    }

    pub fn iterative_deepening(&mut self, board: &mut Board) -> (Option<Move>, i32, i32, u64) {
        let search_start = std::time::Instant::now();
        let mut best_move = None;
        let mut best_score = 0;
        let mut last_depth = 0;
        let mut prev_score = 0;
        // Sliding average of the root score across iterations, only read
        // when napv10 is active -- see `search_root`'s use of it.
        let mut root_average: Option<i32> = None;
        let mut stable_count: u32 = 0;
        // How often the search has changed its mind about the best move.
        // Stability says "nothing has moved lately"; this says "this position
        // has been argued over", and the two are not the same signal -- a move
        // that flipped four times and then held for two iterations reads as
        // settled to one and as contested to the other. Measured on a real
        // loss: the move that lost the game was played in 1.04s with 29.7s
        // still on the clock, and 3s of thought finds a different move.
        let mut move_changes: u32 = 0;
        // Score of the previous completed iteration, for the falling-eval
        // signal: a position whose score is dropping is one the search has not
        // finished solving.
        let mut prev_iter_score: Option<i32> = None;
        let mut quiet_iters: u32 = 0;
        // Are we still inside our own opening book? The book already holds a
        // curated answer for this position, and until now it was used only to
        // ORDER moves -- so the engine spent a full slice of clock rederiving
        // a choice it had been handed. Measured from the starting position at
        // 60+0, the first ten moves cost 24-30% of the clock, which is the
        // part of the game where that time buys least.
        //
        // Not played instantly: a book this size (a few thousand positions)
        // can end anywhere, and walking out of it without having searched is
        // how an engine gets a lost position for free. It searches, at a
        // fraction of the allowance, and the fraction stops applying the
        // moment the position is no longer in the book.
        let in_book = self
            .style_book
            .map(|b| !b.lookup(board.hash).is_empty())
            .unwrap_or(false);
        let opening_move = board.fullmove <= TM_OPENING_MOVES;

        self.killers = [[None; 2]; MAX_PLY];
        self.root_move_nodes.clear();
        self.root_scores.clear();
        // Diversificacao das threads ajudantes.
        //
        // Antes disto a UNICA diferenca entre as N threads era qual delas
        // narrava: mesmo tabuleiro, mesma busca, mesmos parametros. No
        // meio-jogo safava-se, porque com trinta lances legais os historicos
        // separam-se e cada thread acaba noutro ramo. Num final de rei e
        // peoes ha seis lances: todas percorriam a MESMA arvore, escreviam
        // nas mesmas linhas de cache da TT e atropelavam-se. Medido em
        // Lasker-Reichhelm a profundidade 29: uma thread 825 mil nos em
        // 376ms, quatro threads 11,2 MILHOES em 7527ms -- vinte vezes mais
        // lento, com o ritmo a cair para um quinto ao mesmo tempo que os nos
        // explodiam. As duas coisas juntas sao a assinatura de threads a
        // lutar pelo mesmo trabalho, nao de sobrecarga de paralelismo.
        //
        // Cada ajudante salta um padrao proprio de profundidades, portanto
        // chega a cada uma com a TT noutro estado e explora outra ordem. A
        // thread 0 nunca salta nada: e ela que decide o lance.
        // SEM SALTOS. As threads passam a divergir pela LARGURA DA JANELA DE
        // ASPIRACAO (ver `asp_init_delta` em search_root), que e' como o
        // Stockfish actual o faz -- `delta = 5 + threadIdx % 8 + ...`.
        //
        // Os saltos de profundidade vinham de uma versao antiga do Stockfish
        // que entretanto os abandonou, e o efeito medido aqui foi patologico:
        // na MESMA posicao e com o MESMO tempo, um fio escolhia sempre o mesmo
        // lance e seis fios espalhavam-se por cinco lances diferentes, sem
        // nunca escolher o do fio unico --
        //
        //     abertura, 10 corridas:  1 fio -> 10x a2a3
        //                             6 fios -> 3x b1c3, 2x g1f3, 2x f1d3,
        //                                       2x b2b4, 1x c1e3
        //     meio-jogo, 8 corridas:  1 fio -> 8x f3e5
        //                             6 fios -> 6x h2h3, 1x h2h4, 1x f3e5
        //
        // Divergir e' o proposito do Lazy SMP -- tirar a votacao custa 37,6%,
        // medido. O que nao e' proposito e' as ajudantes estarem em
        // profundidades DIFERENTES e a votacao pesar por profundidade: uma
        // thread que saltou para um numero alto por um caminho raso ganha peso
        // exactamente por isso. Com janelas em vez de saltos, todas percorrem
        // as mesmas profundidades e divergem no caminho, que e' o que se quer.
        for depth in 1..=self.limits.max_depth {
            let score = self.search_root(board, depth, prev_score, &mut root_average);
            // 2026-07-20 (BUG REAL corrigido -- irmao do bug ja' corrigido
            // dentro do loop de lances de negamax(), "nunca descartar o
            // resultado de um lance-filho ja' terminado so' porque o
            // relogio esgotou a seguir"): aqui a mesma logica falhava um
            // nivel acima -- `if self.stop && depth > 1 { break; }`
            // acontecia ANTES de ler `self.root_best` para `best_move`,
            // descartando uma iteracao que TINHA encontrado um lance
            // valido (root_best ja' actualizado dentro de negamax()) so'
            // porque o relogio esgotou a meio de um lance POSTERIOR dessa
            // mesma iteracao. Reproduzido num jogo real: motor acabou a
            // jogar o "primeiro lance legal gerado" (fallback de
            // uci.rs::cmd_go) em vez do lance vencedor que a busca ja'
            // tinha encontrado e guardado em root_best.
            // Play the move that measured best, ties going to whichever
            // looked better a ply ago.
            if !self.stop {
                if let Some(best) = self
                    .root_scores
                    .iter()
                    .filter(|e| e.1 != NO_SCORE)
                    .reduce(|a, b| if b.1 > a.1 || (b.1 == a.1 && b.2 > a.2) { b } else { a })
                {
                    self.root_best = Some(best.0);
                }
            }
            if let Some(rb) = self.root_best {
                let interrupted = self.stop && Some(rb) != best_move;
                if Some(rb) == best_move {
                    stable_count += 1;
                } else {
                    stable_count = 0;
                    // Only a change that MOVED THE EVALUATION counts.
                    //
                    // A root move that swaps between alternatives worth the
                    // same is not a hard position, it is a position with
                    // several playable moves -- the opening, most of the
                    // time. Counting those cost a real game: 73 seconds went
                    // on moves 9 to 20, one of them 12s, and the phase that
                    // decided the game was played at under 3s a move. This is
                    // the exact trap `effort` and `settle` were already
                    // documented as falling into, and the first version of
                    // this counter walked straight into it.
                    let real = prev_iter_score
                        .map(|p: i32| (p - score).abs() >= TM_INSTABILITY_MIN_CP)
                        .unwrap_or(false);
                    if real {
                        move_changes += 1;
                    }
                }
                best_move = Some(rb);
                // The MOVE from an interrupted iteration is kept -- it was
                // computed and is often better than the previous depth's.
                // The SCORE is not: an iteration cut mid-way returns whatever
                // the partial window happened to hold, which in practice is
                // frequently 0. Traced from a real bullet game: searches cut
                // at 30ms and 250ms both reported score 0 on a position the
                // full search values at -1225.
                //
                // Two things break when that score is kept. The engine reports
                // a evaluation it does not believe, which is what the bridge
                // logs and what anyone watching reads. And `prev_score` seeds
                // the next move's aspiration window, so the following search
                // starts centred on a number that came from nowhere -- and
                // pays for it in fail-highs and fail-lows on a clock that, in
                // bullet, it does not have.
                // The score comes from the root move itself, not from the
                // return value of an interrupted search.
                //
                // A search cut mid-iteration returns whatever its partial
                // window held -- in practice often 0, which is how this engine
                // reported "0.00" on a position it valued at -12. The root
                // move carries a score that was actually computed for it, so
                // it survives the interruption intact.
                // .1 is this iteration's score, .2 the previous one's. An
                // iteration cut before the move finished leaves .1 unmeasured,
                // and the fallback then has to be the last depth that DID
                // measure it -- otherwise the report falls back to the
                // initial 0, which is exactly the "0.00" this is fixing.
                let rb_score = self.root_scores.iter().find(|e| e.0 == rb).and_then(|e| {
                    if e.1 != NO_SCORE {
                        Some(e.1)
                    } else if e.2 != NO_SCORE {
                        Some(e.2)
                    } else {
                        None
                    }
                });
                match rb_score {
                    // A LOSING score from an interrupted iteration is not
                    // trusted, and this is not caution for its own sake: with
                    // the clock gone mid-search, the moves that would refute
                    // the loss may simply not have been looked at yet. Keeping
                    // it would announce a lost position that the next depth
                    // routinely overturns, and would seed the following
                    // aspiration window far below where the search actually
                    // sits.
                    Some(v) if self.stop && v < -MATE_SCORE + MAX_PLY as i32 => {}
                    Some(v) => {
                        best_score = v;
                        prev_score = v;
                    }
                    None => {
                        if !self.stop {
                            best_score = score;
                            prev_score = score;
                        }
                    }
                }
                last_depth = depth;
                // An interrupted iteration is still reported when it
                // changed the move.
                //
                // The engine keeps a better move found by an iteration the
                // clock cut short -- deliberately, and correctly: a result
                // already computed should not be thrown away because time ran
                // out afterwards. But the narration skipped incomplete
                // iterations, so the last line printed belonged to the
                // previous depth and named a different move than the one
                // played. Reproducibly: "pv f7e6 ..." then "bestmove e8d8".
                // The decision was right and the announcement was wrong,
                // which is the worse of the two failures to leave in place --
                // it misleads anyone reading the output and silently
                // corrupts tools that follow the reported line.
                if self.report && (!self.stop || interrupted) {
                    // Anchored on the move actually chosen.
                    //
                    // extract_pv rebuilds the line from the transposition
                    // table, and the table's entry for the root is not
                    // necessarily the move root_best settled on -- it is
                    // always-replace, so a later sibling can have overwritten
                    // it. The engine then announced one move and played
                    // another: reproducibly, "pv f7e6 ..." followed by
                    // "bestmove e8d8". That misleads anyone reading the
                    // output, and it quietly corrupts any tool that walks the
                    // principal variation, since the line analysed is not the
                    // line played.
                    let pv = self.extract_pv_from(board, rb, depth.max(1) as usize + 4);
                    let pv_str: Vec<String> = pv.iter().map(|m| m.to_uci()).collect();
                    let ms = search_start.elapsed().as_millis().max(1) as u64;
                    let nps = self.nodes.saturating_mul(1000) / ms;
                    // Report best_score, not the raw return value: an
                    // interrupted iteration returns whatever its partial
                    // window held, and announcing that as the evaluation is
                    // how a position worth -12 was published as 0.00.
                    let rs = best_score;
                    let score_str = if rs.abs() >= MATE_THRESHOLD {
                        let mate_in = (MATE_SCORE - rs.abs() + 1) / 2;
                        format!("mate {}", if rs > 0 { mate_in } else { -mate_in })
                    } else {
                        format!("cp {}", crate::evaluation::score_normalizado(rs))
                    };
                    // The same number as chances of winning, drawing, losing.
                    //
                    // A centipawn is not a stable unit across the game -- fitted
                    // on 220k real results, the scale that turns evaluation into
                    // a score runs from 433 with three pawns on the board to
                    // 1564 with a full one. So "+2.10" means a 77% score in one
                    // ending and 60% in an opening, and a client deciding
                    // whether to offer a draw on centipawns is reading a ruler
                    // whose marks move. This is what it should read instead.
                    let (w, d, l) = crate::evaluation::win_draw_loss(rs);
                    println!(
                        "info depth {} multipv 1 score {} wdl {} {} {} nodes {} nps {} time {} pv {}",
                        depth, score_str, w, d, l, self.nodes, nps, ms, pv_str.join(" ")
                    );
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
            }
            if self.stop {
                break;
            }
            // Spend the clock where the position asks for it.
            //
            // What stood here judged the two signals below as a pair of
            // yes/no gates that could only ever END the search early: stable
            // for 3 iterations AND 70% of nodes on the best move AND past the
            // checkpoint -> stop, otherwise run out the full budget. Every
            // move therefore cost the same, whether it was a forced recapture
            // or the one move the game turned on. Measured over 25 real games
            // at 180+0: median 3.0s a move, mean 2.84s, with a budget of
            // ~3.5s -- an almost flat line where there should be a range.
            //
            // The same two signals, read as continuous quantities instead of
            // gates, give a multiplier that moves in BOTH directions. A move
            // that just changed with its effort still spread across rivals
            // earns several times the budget; one that has held for
            // iterations with nearly every node behind it gives most of it
            // back. Only the reporting thread decides -- the others would
            // each reach their own verdict from their own noisy statistics,
            // and the move costs as long as the slowest of them.
            if self.report && depth >= 5 {
                if let Some(budget) = self.limits.soft_budget {
                    let effort_frac = best_move
                        .map(|bm| {
                            let on_best = self
                                .root_move_nodes
                                .iter()
                                .find(|(m, _)| *m == bm)
                                .map(|(_, n)| *n)
                                .unwrap_or(0);
                            on_best as f64 / self.nodes.max(1) as f64
                        })
                        .unwrap_or(0.0);
                    // Has the evaluation stopped moving? Both signals above
                    // read "several moves are equally good" as "this position
                    // is hard", because the root move swaps between equals
                    // and the nodes spread across them. Traced from the
                    // opening position: effort down at 0.08-0.13, settle
                    // knocked back to 0 at half the depths, and the
                    // multiplier pinned at its 2.20 ceiling -- the maximum
                    // allowance, spent on the one position in chess that
                    // needs it least. Measured cost in real games: 24% of the
                    // clock gone in ten moves at 60+0, 31% at 180+0.
                    //
                    // The score is the signal neither of them is. When it
                    // holds still across iterations the search is not
                    // changing its mind, whichever of several equal moves it
                    // happens to be naming this time -- and a value cannot be
                    // an artefact of which thread arrived first, which is why
                    // stability of the MOVE could never be trusted here.
                    // Extensions stay available the moment the score does
                    // move, which is when they are worth having.
                    let score_delta = prev_iter_score.map(|p| (score - p).abs());
                    // Falling eval: the DROP with its sign, not the absolute
                    // change. It stretches the budget when the position is
                    // getting worse -- the search has found a problem and not
                    // yet the answer. Complementary to `quiet` below, which
                    // shortens when the score is not moving at all: one asks
                    // "is there still something to solve", the other "is it
                    // already solved".
                    let score_drop = prev_iter_score
                        .map(|p: i32| (p - score).clamp(0, 400))
                        .unwrap_or(0);
                    let quiet = score_delta.map(|d| d <= TM_QUIET_CP).unwrap_or(false);
                    quiet_iters = if quiet { quiet_iters + 1 } else { 0 };
                    prev_iter_score = Some(score);
                    // The opening ceiling below is there because the search's
                    // signals are noise that early -- but a score that has
                    // genuinely lurched is not noise, and refusing to think
                    // about a real tactic because it happened on move eight
                    // would be the same mistake in the other direction.
                    let alarmed = score_delta.map(|d| d > TM_ALERT_CP).unwrap_or(false);
                    let mut scale = time_scale(effort_frac, stable_count, score_drop, move_changes);
                    if quiet_iters >= TM_QUIET_ITERS {
                        scale = scale.min(1.0);
                    }
                    // Lance obvio: nao ha nada para decidir.
                    //
                    // `quiet_iters` ja' reconhece "o score parou", mas so'
                    // trava a escala em 1.0 -- o orcamento inteiro. Numa
                    // recaptura forcada, ou numa troca em que qualquer outra
                    // coisa perde peca, gastar o orcamento inteiro e' deitar
                    // relogio fora: o lance ja' esta' escolhido a' quarta
                    // profundidade e nao volta a mudar.
                    //
                    // Tres condicoes ao mesmo tempo, porque cada uma sozinha
                    // engana. O score parado sozinho acontece em posicoes
                    // quietas com dez lances equivalentes -- e essas nao sao
                    // obvias, sao indiferentes. O melhor lance estavel sozinho
                    // acontece por acaso. O que nao engana e' o melhor estar
                    // muito a' frente do SEGUNDO: aí ha' um lance e os outros
                    // sao piores, que e' a definicao de obvio.
                    let margem = {
                        let mut sc: Vec<i32> = self
                            .root_scores
                            .iter()
                            .filter(|e| e.1 != NO_SCORE)
                            .map(|e| e.1)
                            .collect();
                        sc.sort_unstable_by(|a, b| b.cmp(a));
                        if sc.len() >= 2 { sc[0] - sc[1] } else { i32::MAX }
                    };
                    if quiet_iters >= TM_QUIET_ITERS
                        && stable_count >= TM_OBVIO_ITERS
                        && margem >= TM_OBVIO_CP
                    {
                        scale = scale.min(TM_OBVIO_SCALE);
                    }
                    if in_book {
                        scale = scale.min(TM_BOOK_SCALE);
                    } else if opening_move && !alarmed {
                        // Out of book but still in the opening. Neither signal
                        // the multiplier is built on carries information here:
                        // traced from the starting position, effort sits at
                        // 0.08-0.13 and settle is knocked back to zero every
                        // other depth, because a dozen moves are genuinely
                        // near-equal and the root swaps between them. That
                        // reads as "hard" and buys the ceiling, when what it
                        // means is "it hardly matters which". A signal that
                        // cannot inform should not be able to ask for more
                        // time; the allowance still applies in full, it just
                        // cannot be extended beyond it.
                        scale = scale.min(TM_OPENING_SCALE);
                    }
                    let allowed = budget.mul_f64(scale);
                    if std::env::var_os("KESTREL_TM_TRACE").is_some() {
                        eprintln!(
                            "tm d={:<3} elapsed={:>6}ms effort={:.2} settle={} scale={:.2} allowed={:>6}ms",
                            depth,
                            search_start.elapsed().as_millis(),
                            effort_frac,
                            stable_count,
                            scale,
                            allowed.as_millis()
                        );
                    }
                    if search_start.elapsed() >= allowed {
                        self.stop_flag.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }
        }
        if std::env::var_os("KESTREL_CUT_STATS").is_some() && self.report {
            let pct = if self.cut_nodes > 0 {
                100.0 * self.cut_first as f64 / self.cut_nodes as f64
            } else {
                0.0
            };
            eprintln!(
                "asp: re-pesquisas={} nos-gastos-em-re-pesquisa={} ({:.0}% do total)",
                self.asp_re, self.asp_nos,
                100.0 * self.asp_nos as f64 / self.nodes.max(1) as f64
            );
            eprintln!(
                "cut-stats: cortes={} ao-primeiro-lance={} ({:.1}%)",
                self.cut_nodes, self.cut_first, pct
            );
            let rr = if self.lmr_tried > 0 {
                100.0 * self.lmr_research as f64 / self.lmr_tried as f64
            } else {
                0.0
            };
            let avg = if self.lmr_tried > 0 {
                self.lmr_sum as f64 / self.lmr_tried as f64
            } else {
                0.0
            };
            eprintln!(
                "lmr-stats: reduzidos={} repesquisados={} ({:.1}%) reducao-media={:.2}",
                self.lmr_tried, self.lmr_research, rr, avg
            );
            // QUEM esta a impedir a reducao, e nao so' quantas houve.
            //
            // Uma taxa de re-pesquisa de 1% nao diz se reduzimos de menos ou
            // de mais -- diz que quase nada do que reduzimos volta acima de
            // alpha. As duas explicacoes possiveis sao opostas: ou reduzimos
            // tao fundo que a busca reduzida nunca acha nada (poda cega), ou
            // so' chegam a' reducao lances que ja eram maus. Os contadores
            // separam-nas, e existiam sem nunca terem sido impressos, o que
            // e' o mesmo que nao existirem.
            let qt = self.lmr_quiet_total.max(1);
            eprintln!(
                "lmr-porque: quiets={} | xeque={} ({:.1}%) profundidade={} ({:.1}%)                  extensao={} ({:.1}%) cedo-demais={} ({:.1}%) | reduzidos={} ({:.1}%)",
                self.lmr_quiet_total,
                self.lmr_skip_check, 100.0 * self.lmr_skip_check as f64 / qt as f64,
                self.lmr_skip_depth, 100.0 * self.lmr_skip_depth as f64 / qt as f64,
                self.lmr_skip_extend, 100.0 * self.lmr_skip_extend as f64 / qt as f64,
                self.lmr_skip_early, 100.0 * self.lmr_skip_early as f64 / qt as f64,
                self.lmr_tried, 100.0 * self.lmr_tried as f64 / qt as f64
            );
            // What the tables actually hold. Every consumer of history --
            // history pruning, the LMR step, the RFP shift, the quiescence
            // cut -- compares a raw table value against a constant, so those
            // constants are only meaningful relative to the distribution
            // here. Changing how the bonus is computed changes this
            // distribution, and every one of those constants silently means
            // something different afterwards. Printing it turns "pick a
            // scaling factor" into a measurement.
            {
                let mut vals: Vec<i32> = self
                    .history_scores
                    .iter()
                    .flat_map(|side| side.iter())
                    .flat_map(|from| from.iter())
                    .copied()
                    .filter(|&v| v != 0)
                    .collect();
                if !vals.is_empty() {
                    vals.sort_unstable();
                    let at = |q: f64| vals[((vals.len() - 1) as f64 * q) as usize];
                    eprintln!(
                        "hist-dist: n={} p05={} p25={} mediana={} p75={} p95={} |max|={}",
                        vals.len(),
                        at(0.05),
                        at(0.25),
                        at(0.50),
                        at(0.75),
                        at(0.95),
                        vals[0].abs().max(vals[vals.len() - 1].abs())
                    );
                }
                let mut cv: Vec<i32> = self
                    .cont_hist
                    .iter()
                    .copied()
                    .filter(|&v| v != 0)
                    .collect();
                if !cv.is_empty() {
                    cv.sort_unstable();
                    let at = |q: f64| cv[((cv.len() - 1) as f64 * q) as usize];
                    eprintln!(
                        "cont-dist: n={} p05={} p25={} mediana={} p75={} p95={} |max|={}",
                        cv.len(),
                        at(0.05),
                        at(0.25),
                        at(0.50),
                        at(0.75),
                        at(0.95),
                        cv[0].abs().max(cv[cv.len() - 1].abs())
                    );
                }
            }
            let t = self.nmp_tried.max(1) as f64;
            eprintln!(
                "nmp: tentado={} corte-cru={} ({:.0}%) aceite-sem-verificar={} verificado={} ok={} falhou={} fail-low={} ({:.0}%)",
                self.nmp_tried,
                self.nmp_cutoff_raw, 100.0 * self.nmp_cutoff_raw as f64 / t,
                self.nmp_cut_taken,
                self.nmp_verify_tried, self.nmp_verify_ok, self.nmp_verify_failed,
                self.nmp_failed_low, 100.0 * self.nmp_failed_low as f64 / t
            );
            let pv = self.nmp_tried_pv.max(1) as f64;
            let nonpv = (self.nmp_tried - self.nmp_tried_pv).max(1) as f64;
            let nonpv_fail = self.nmp_failed_low - self.nmp_failed_pv;
            eprintln!(
                "nmp-pv: em-PV={} ({:.0}% do total) falha-em-PV={:.0}% | fora-de-PV={} falha={:.0}%",
                self.nmp_tried_pv,
                100.0 * self.nmp_tried_pv as f64 / t,
                100.0 * self.nmp_failed_pv as f64 / pv,
                self.nmp_tried - self.nmp_tried_pv,
                100.0 * nonpv_fail as f64 / nonpv
            );
            let sh = self.nodes_shallow.max(1) as f64;
            eprintln!(
                "poda-rasa: nos-com-prof<=6={} | RFP={} ({:.1}%) razor={} ({:.1}%) futility={} ({:.1}%)",
                self.nodes_shallow,
                self.cut_rfp, 100.0 * self.cut_rfp as f64 / sh,
                self.cut_razor, 100.0 * self.cut_razor as f64 / sh,
                self.cut_futility, 100.0 * self.cut_futility as f64 / sh
            );
            eprintln!(
                "qsearch: total={} quiescencia={} ({:.1}%) principal={} ({:.1}%)",
                self.nodes,
                self.qnodes,
                100.0 * self.qnodes as f64 / self.nodes.max(1) as f64,
                self.nodes - self.qnodes,
                100.0 * (self.nodes - self.qnodes) as f64 / self.nodes.max(1) as f64
            );
            let q = self.lmr_quiet_total.max(1) as f64;
            eprintln!(
                "lmr-skip: quietos={} | xeque={} ({:.0}%) prof<3={} ({:.0}%) extensao={} ({:.0}%) i<min={} ({:.0}%) | reduzidos={} ({:.0}%)",
                self.lmr_quiet_total,
                self.lmr_skip_check, 100.0 * self.lmr_skip_check as f64 / q,
                self.lmr_skip_depth, 100.0 * self.lmr_skip_depth as f64 / q,
                self.lmr_skip_extend, 100.0 * self.lmr_skip_extend as f64 / q,
                self.lmr_skip_early, 100.0 * self.lmr_skip_early as f64 / q,
                self.lmr_tried, 100.0 * self.lmr_tried as f64 / q
            );
        }
        (best_move, best_score, last_depth, self.nodes)
    }
}

/// Staged move picker. Ideia: em vez de pontuar TODOS os lances legais upfront
/// (SEE em todas as capturas, history+livro+countermove em todos os
/// quietos) antes de sequer tentar o primeiro, devolver os lances por
/// fases e pontuar SO' o subconjunto que a fase actual precisa. Se um
/// corte beta acontece no TT-move (muito comum quando a TT tem info),
/// nao pagamos NENHUM SEE nem lookup de history. Se um good-noisy corta,
/// nao pagamos NENHUM history nem lookup de livro.
///
/// Correccao preservada: gera todos os lances LEGAIS uma vez a
/// construcao (mesmo `generate_legal` de antes), so' muda a ordem/
/// timing de pontuacao. `MovePicker::next` devolve `None` quando todos
/// os lances foram devolvidos -- o chamador so' precisa de saber quantos
/// devolveu para distinguir mate/stalemate de fim de loop normal.
#[derive(Copy, Clone, PartialEq, Eq)]
enum PickerStage {
    TtMove,
    ScoreNoisy,
    GoodNoisy,
    Killer1,
    Killer2,
    ScoreQuiet,
    Quiet,
    BadNoisy,
    Done,
}

pub struct MovePicker {
    stage: PickerStage,
    tt_move: Option<Move>,
    killer1: Option<Move>,
    killer2: Option<Move>,
    /// noisy = capturas + promocoes. Pontuado com SEE (ver `score_noisy`)
    /// so' quando entramos em `ScoreNoisy`. Cada entrada guarda o lance
    /// e o SEE score correspondente; SEE>=0 vao primeiro (GoodNoisy),
    /// SEE<0 vao no fim (BadNoisy).
    noisy: Vec<(Move, i32, i32)>,
    noisy_idx: usize,
    /// Marca onde acabam os good noisy (SEE>=0) e comecam os bad noisy
    /// (SEE<0). Definido quando `ScoreNoisy` termina.
    good_noisy_end: usize,
    /// quiet = tudo o que nao e' captura nem promocao. Pontuado com
    /// history + livro + countermove (ver `score_quiet`) so' quando
    /// entramos em `ScoreQuiet`.
    quiet: Vec<(Move, i32)>,
    quiet_idx: usize,
}

impl MovePicker {
    /// `excluded` (usado no MultiPV, ver `excluded_root_moves`) tem de ser
    /// filtrado ANTES da construcao do picker -- basta o chamador passar
    /// `moves` ja' filtrado; o picker nao conhece MultiPV.
    pub fn new(moves: Vec<Move>, tt_move: Option<Move>, killers: [Option<Move>; 2]) -> Self {
        // Separa capturas/promocoes de quietos numa unica passagem.
        // MoveFlag::EnPassant e captura; promocoes contam sempre como
        // noisy (mesmo sem captura -- a promocao propria e "material").
        let mut noisy: Vec<(Move, i32, i32)> = Vec::with_capacity(moves.len() / 4);
        let mut quiet: Vec<(Move, i32)> = Vec::with_capacity(moves.len());
        for m in moves {
            if m.is_capture() || m.promotion.is_some() {
                noisy.push((m, 0, 0));
            } else {
                quiet.push((m, 0));
            }
        }
        MovePicker {
            stage: PickerStage::TtMove,
            tt_move,
            killer1: killers[0],
            killer2: killers[1],
            noisy,
            noisy_idx: 0,
            good_noisy_end: 0,
            quiet,
            quiet_idx: 0,
        }
    }

    /// Devolve o proximo lance ou `None` quando nao ha mais nada.
    /// `searcher` e usado para SEE (na fase ScoreNoisy) e para
    /// history/livro/countermove (na fase ScoreQuiet). `ply`/`hash` sao
    /// os do no' actual (para lookup de livro por posicao, igual ao
    /// order_moves antigo).
    /// Wrapper que salta o `excluded_move` do Searcher (usado por
    /// singular extensions). Delega para `next_move_raw` e re-chama-se
    /// se o lance devolvido for o excluido.
    pub fn next_move(
        &mut self,
        searcher: &Searcher,
        board: &Board,
        ply: usize,
        hash: u64,
    ) -> Option<Move> {
        loop {
            let mv = self.next_move_raw(searcher, board, ply, hash)?;
            if searcher.excluded_move == Some(mv) {
                continue;
            }
            return Some(mv);
        }
    }

    fn next_move_raw(
        &mut self,
        searcher: &Searcher,
        board: &Board,
        ply: usize,
        hash: u64,
    ) -> Option<Move> {
        loop {
            match self.stage {
                PickerStage::TtMove => {
                    self.stage = PickerStage::ScoreNoisy;
                    if let Some(tm) = self.tt_move {
                        // TT-move so' e valido se estiver na lista real
                        // de lances legais (a TT pode conter lixo por
                        // colisao de hash). Procura em noisy+quiet.
                        if self.contains_move(tm) {
                            return Some(tm);
                        }
                    }
                }
                PickerStage::ScoreNoisy => {
                    // Pontua SEE de cada captura, uma unica vez. Nao ha
                    // MVV-LVA em separado -- SEE ja engloba a ideia de
                    // "captura de peca grande com peca pequena", e ainda
                    // rejeita capturas que aparentam ganhar mas perdem no
                    // full exchange (Bxf7 defendido).
                    for i in 0..self.noisy.len() {
                        let m = self.noisy[i].0;
                        if Some(m) == self.tt_move {
                            self.noisy[i].1 = i32::MIN; // marca para saltar depois
                            continue;
                        }
                        self.noisy[i].1 = see::see(searcher.atk, board, &m);
                        // Capture history: tie-break only (see field doc
                        // on Searcher::capture_history) -- non-capture
                        // promotions have no "captured piece", left at 0.
                        self.noisy[i].2 = if m.is_capture() {
                            let moving_pt = board.piece_at(m.from).map(|(pt, _)| pt);
                            let captured_pt = if m.flag == MoveFlag::EnPassant {
                                Some(PieceType::Pawn)
                            } else {
                                board.piece_at(m.to).map(|(pt, _)| pt)
                            };
                            match (moving_pt, captured_pt) {
                                (Some(mp), Some(cp)) => searcher.capture_history[board.side.idx()][mp.idx()][cp.idx()],
                                _ => 0,
                            }
                        } else {
                            0
                        };
                    }
                    // Nao ordenamos o vector agora -- selection-sort
                    // in-place em `GoodNoisy` e `BadNoisy` extrai o
                    // maior de cada vez, evitando O(n log n) upfront
                    // quando muitas vezes so' precisamos do primeiro.
                    self.stage = PickerStage::GoodNoisy;
                }
                PickerStage::GoodNoisy => {
                    if let Some(m) = self.pick_best_noisy(true) {
                        return Some(m);
                    }
                    // Terminou os good noisy; a partir daqui o
                    // `noisy_idx` marca o inicio dos bad noisy (que
                    // ficam para o fim).
                    self.good_noisy_end = self.noisy_idx;
                    self.stage = PickerStage::Killer1;
                }
                PickerStage::Killer1 => {
                    self.stage = PickerStage::Killer2;
                    if let Some(k) = self.killer1 {
                        if Some(k) != self.tt_move && self.quiet_contains(k) {
                            self.mark_quiet_used(k);
                            return Some(k);
                        }
                    }
                }
                PickerStage::Killer2 => {
                    self.stage = PickerStage::ScoreQuiet;
                    if let Some(k) = self.killer2 {
                        if Some(k) != self.tt_move
                            && Some(k) != self.killer1
                            && self.quiet_contains(k)
                        {
                            self.mark_quiet_used(k);
                            return Some(k);
                        }
                    }
                }
                PickerStage::ScoreQuiet => {
                    // Livro e' pesquisado por posicao (nao por lance),
                    // por isso uma unica vez aqui em vez de N vezes
                    // no loop.
                    let side = board.side.idx();
                    let book_entries: Vec<(u16, u32)> = match searcher.style_book {
                        Some(b) => b.lookup(hash),
                        None => Vec::new(),
                    };
                    // Countermove ainda usado como fallback binario (bonus
                    // fixo se bater) para preservar continuidade das
                    // iteracoes anteriores; cont_hist adiciona o sinal
                    // numerico multi-lag por cima (somando contHist a -1
                    // e -2 plies por agora; um lag -4 poderia entrar mais
                    // tarde).
                    let countermove = searcher
                        .ply_last_move
                        .get(ply)
                        .and_then(|x| *x)
                        .and_then(|(pt, to)| searcher.countermoves[pt.idx()][to as usize]);
                    let prev1 = if ply >= 1 { searcher.ply_last_move.get(ply).and_then(|x| *x) } else { None };
                    let prev2 = if ply >= 2 { searcher.ply_last_move.get(ply - 1).and_then(|x| *x) } else { None };
                    for i in 0..self.quiet.len() {
                        let m = self.quiet[i].0;
                        if m.from == m.to {
                            // marcador "ja usado" (killer, ver mark_quiet_used)
                            self.quiet[i].1 = i32::MIN;
                            continue;
                        }
                        if Some(m) == self.tt_move {
                            self.quiet[i].1 = i32::MIN;
                            continue;
                        }
                        let h = searcher.history_scores[side][m.from as usize][m.to as usize];
                        let cm_bonus = if Some(m) == countermove { 2000 } else { 0 };
                        let book = searcher.book_bonus(&book_entries, &m);
                        // Continuation history: precisa da peca que faz o
                        // lance actual, obtida do mailbox O(1) do board
                        // -- ~2ns por lookup, e so' aqui, fora do hot path
                        // de make_move.
                        let mut ch = 0i32;
                        if let Some((curr_pt, _)) = board.piece_at(m.from) {
                            if let Some((p1_pt, p1_to)) = prev1 {
                                ch += searcher.cont_hist[cont_hist_idx(p1_pt, p1_to, curr_pt, m.to)];
                            }
                            if let Some((p2_pt, p2_to)) = prev2 {
                                ch += searcher.cont_hist[cont_hist_idx(p2_pt, p2_to, curr_pt, m.to)];
                            }
                        }
                        self.quiet[i].1 = h + cm_bonus + book + ch;
                    }
                    self.stage = PickerStage::Quiet;
                }
                PickerStage::Quiet => {
                    if let Some(m) = self.pick_best_quiet() {
                        return Some(m);
                    }
                    self.stage = PickerStage::BadNoisy;
                }
                PickerStage::BadNoisy => {
                    if let Some(m) = self.pick_best_noisy(false) {
                        return Some(m);
                    }
                    self.stage = PickerStage::Done;
                }
                PickerStage::Done => return None,
            }
        }
    }

    /// Se `m` (o lance sugerido) ainda estiver nas listas geradas,
    /// devolve true. Serve para validar TT-move e killers antes de os
    /// emitir -- ambos podem ser lixo (TT colisao ou killer stale que
    /// ja nao aplica a esta posicao).
    fn contains_move(&self, m: Move) -> bool {
        self.noisy.iter().any(|(x, _, _)| *x == m) || self.quiet.iter().any(|(x, _)| *x == m)
    }
    fn quiet_contains(&self, m: Move) -> bool {
        self.quiet.iter().any(|(x, _)| *x == m)
    }

    /// Marca um quiet como "ja usado" (usei-o como killer, nao repetir
    /// mais tarde no stage Quiet). Truque: guardar `from == to`, que
    /// nunca acontece num lance real; o loop de ScoreQuiet trata como
    /// score = MIN e o pick_best_quiet salta-o.
    fn mark_quiet_used(&mut self, m: Move) {
        for entry in self.quiet.iter_mut() {
            if entry.0 == m {
                entry.0 = Move {
                    from: 0,
                    to: 0,
                    promotion: None,
                    flag: MoveFlag::Quiet,
                };
                return;
            }
        }
    }

    /// Selection-sort in-place: encontra o de maior score a partir de
    /// `noisy_idx`, faz swap para essa posicao, avanca. Devolve o lance;
    /// respeita a fase (good_only=true so' devolve SEE>=0, senao so'
    /// devolve SEE<0). Se nao ha mais na fase actual, devolve None.
    fn pick_best_noisy(&mut self, good_only: bool) -> Option<Move> {
        while self.noisy_idx < self.noisy.len() {
            // Primary key SEE, tie-broken by capture history ONLY when
            // SEE is exactly equal -- the good/bad-noisy boundary check
            // below still looks purely at SEE, completely unaffected by
            // the tie-break (many other pruning decisions assume that
            // boundary is pure SEE, see search_params()-driven captures
            // futility/SEE pruning).
            let mut best_i = self.noisy_idx;
            for i in (self.noisy_idx + 1)..self.noisy.len() {
                let (_, s, h) = self.noisy[i];
                let (_, bs, bh) = self.noisy[best_i];
                if s > bs || (s == bs && h > bh) {
                    best_i = i;
                }
            }
            self.noisy.swap(self.noisy_idx, best_i);
            let (m, score, _) = self.noisy[self.noisy_idx];
            // score == i32::MIN significa "e' o TT-move, salta"
            if score == i32::MIN {
                self.noisy_idx += 1;
                continue;
            }
            // Boundary entre good e bad: good tem SEE>=0.
            if good_only {
                if score < 0 {
                    return None;
                }
            } else if score >= 0 {
                // Nao devia acontecer (todos os good_noisy ja foram
                // devolvidos), mas por defesa, salta -- os good ja
                // foram devolvidos por definicao.
                self.noisy_idx += 1;
                continue;
            }
            self.noisy_idx += 1;
            return Some(m);
        }
        None
    }

    fn pick_best_quiet(&mut self) -> Option<Move> {
        while self.quiet_idx < self.quiet.len() {
            let mut best_i = self.quiet_idx;
            let mut best_score = self.quiet[best_i].1;
            for i in (self.quiet_idx + 1)..self.quiet.len() {
                if self.quiet[i].1 > best_score {
                    best_score = self.quiet[i].1;
                    best_i = i;
                }
            }
            self.quiet.swap(self.quiet_idx, best_i);
            let (m, score) = self.quiet[self.quiet_idx];
            if score == i32::MIN {
                self.quiet_idx += 1;
                continue;
            }
            self.quiet_idx += 1;
            return Some(m);
        }
        None
    }
}
