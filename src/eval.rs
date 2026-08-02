use crate::attacks::*;
use crate::bitboard::*;
use crate::board::Board;
use crate::types::*;
use std::sync::OnceLock;

static ATTACKS: OnceLock<Attacks> = OnceLock::new();

/// Force every lazily-built global into existence.
///
/// These are built on first use, and first use happens inside the first
/// search -- with the clock already running. The attack tables in
/// particular are expensive to build (magic bitboards), and measuring the
/// per-iteration `info` output showed the first search losing ~600ms
/// before even completing depth 1, every time a fresh engine process
/// played its first move. Called once at start-up so the cost is paid
/// before any clock is ticking.
pub fn warmup() {
    let _ = atk();
    let _ = default_weights();
    // Touching the globals is not enough: measurement showed the first
    // real evaluation still costing ~700ms (the first search ran at
    // ~2.8k nps instead of ~550k until it was paid). Run a full
    // evaluation of a real position so whatever that first pass sets up
    // is set up here, off the clock.
    let mut b = crate::board::Board::startpos();
    let _ = evaluate(&b);
    let _ = positional_terms(&b, default_weights());
    b.side = b.side.opp();
    let _ = evaluate(&b);
}
fn atk() -> &'static Attacks {
    ATTACKS.get_or_init(Attacks::new)
}

// Tapered piece-square tables (perspetiva das brancas, a1=indice 0 ..
// h8=indice 63 -- peca preta usa espelho vertical). PSQT publicas de
// ponto de partida educacional classico -- valores para afinar via o
// nosso tuner a seguir, nao um estado final. Convertidas de rank8-first
// para rank1-first (convencao deste codigo).
#[rustfmt::skip]
const MG_PAWN: [i32; 64] = [
       0,    0,    0,    0,    0,    0,    0,    0,
     -35,   -1,  -20,  -23,  -15,   24,   38,  -22,
     -26,   -4,   -4,  -10,    3,    3,   33,  -12,
     -27,   -2,   -5,   12,   17,    6,   10,  -25,
     -14,   13,    6,   21,   23,   12,   17,  -23,
      -6,    7,   26,   31,   65,   56,   25,  -20,
      98,  134,   61,   95,   68,  126,   34,  -11,
       0,    0,    0,    0,    0,    0,    0,    0,
];
#[rustfmt::skip]
const EG_PAWN: [i32; 64] = [
       0,    0,    0,    0,    0,    0,    0,    0,
      13,    8,    8,   10,   13,    0,    2,   -7,
       4,    7,   -6,    1,    0,   -5,   -1,   -8,
      13,    9,   -3,   -7,   -7,   -8,    3,   -1,
      32,   24,   13,    5,   -2,    4,   17,   17,
      94,  100,   85,   67,   56,   53,   82,   84,
     178,  173,  158,  134,  147,  132,  165,  187,
       0,    0,    0,    0,    0,    0,    0,    0,
];
#[rustfmt::skip]
const MG_KNIGHT: [i32; 64] = [
    -105,  -21,  -58,  -33,  -17,  -28,  -19,  -23,
     -29,  -53,  -12,   -3,   -1,   18,  -14,  -19,
     -23,   -9,   12,   10,   19,   17,   25,  -16,
     -13,    4,   16,   13,   28,   19,   21,   -8,
      -9,   17,   19,   53,   37,   69,   18,   22,
     -47,   60,   37,   65,   84,  129,   73,   44,
     -73,  -41,   72,   36,   23,   62,    7,  -17,
    -167,  -89,  -34,  -49,   61,  -97,  -15, -107,
];
#[rustfmt::skip]
const EG_KNIGHT: [i32; 64] = [
     -29,  -51,  -23,  -15,  -22,  -18,  -50,  -64,
     -42,  -20,  -10,   -5,   -2,  -20,  -23,  -44,
     -23,   -3,   -1,   15,   10,   -3,  -20,  -22,
     -18,   -6,   16,   25,   16,   17,    4,  -18,
     -17,    3,   22,   22,   22,   11,    8,  -18,
     -24,  -20,   10,    9,   -1,   -9,  -19,  -41,
     -25,   -8,  -25,   -2,   -9,  -25,  -24,  -52,
     -58,  -38,  -13,  -28,  -31,  -27,  -63,  -99,
];
#[rustfmt::skip]
const MG_BISHOP: [i32; 64] = [
     -33,   -3,  -14,  -21,  -13,  -12,  -39,  -21,
       4,   15,   16,    0,    7,   21,   33,    1,
       0,   15,   15,   15,   14,   27,   18,   10,
      -6,   13,   13,   26,   34,   12,   10,    4,
      -4,    5,   19,   50,   37,   37,    7,   -2,
     -16,   37,   43,   40,   35,   50,   37,   -2,
     -26,   16,  -18,  -13,   30,   59,   18,  -47,
     -29,    4,  -82,  -37,  -25,  -42,    7,   -8,
];
#[rustfmt::skip]
const EG_BISHOP: [i32; 64] = [
     -23,   -9,  -23,   -5,   -9,  -16,   -5,  -17,
     -14,  -18,   -7,   -1,    4,   -9,  -15,  -27,
     -12,   -3,    8,   10,   13,    3,   -7,  -15,
      -6,    3,   13,   19,    7,   10,   -3,   -9,
      -3,    9,   12,    9,   14,   10,    3,    2,
       2,   -8,    0,   -1,   -2,    6,    0,    4,
      -8,   -4,    7,  -12,   -3,  -13,   -4,  -14,
     -14,  -21,  -11,   -8,   -7,   -9,  -17,  -24,
];
#[rustfmt::skip]
const MG_ROOK: [i32; 64] = [
     -19,  -13,    1,   17,   16,    7,  -37,  -26,
     -44,  -16,  -20,   -9,   -1,   11,   -6,  -71,
     -45,  -25,  -16,  -17,    3,    0,   -5,  -33,
     -36,  -26,  -12,   -1,    9,   -7,    6,  -23,
     -24,  -11,    7,   26,   24,   35,   -8,  -20,
      -5,   19,   26,   36,   17,   45,   61,   16,
      27,   32,   58,   62,   80,   67,   26,   44,
      32,   42,   32,   51,   63,    9,   31,   43,
];
#[rustfmt::skip]
const EG_ROOK: [i32; 64] = [
      -9,    2,    3,   -1,   -5,  -13,    4,  -20,
      -6,   -6,    0,    2,   -9,   -9,  -11,   -3,
      -4,    0,   -5,   -1,   -7,  -12,   -8,  -16,
       3,    5,    8,    4,   -5,   -6,   -8,  -11,
       4,    3,   13,    1,    2,    1,   -1,    2,
       7,    7,    7,    5,    4,   -3,   -5,   -3,
      11,   13,   13,   11,   -3,    3,    8,    3,
      13,   10,   18,   15,   12,   12,    8,    5,
];
#[rustfmt::skip]
const MG_QUEEN: [i32; 64] = [
      -1,  -18,   -9,   10,  -15,  -25,  -31,  -50,
     -35,   -8,   11,    2,    8,   15,   -3,    1,
     -14,    2,  -11,   -2,   -5,    2,   14,    5,
      -9,  -26,   -9,  -10,   -2,   -4,    3,   -3,
     -27,  -27,  -16,  -16,   -1,   17,   -2,    1,
     -13,  -17,    7,    8,   29,   56,   47,   57,
     -24,  -39,   -5,    1,  -16,   57,   28,   54,
     -28,    0,   29,   12,   59,   44,   43,   45,
];
#[rustfmt::skip]
const EG_QUEEN: [i32; 64] = [
     -33,  -28,  -22,  -43,   -5,  -32,  -20,  -41,
     -22,  -23,  -30,  -16,  -16,  -23,  -36,  -32,
     -16,  -27,   15,    6,    9,   17,   10,    5,
     -18,   28,   19,   47,   31,   34,   39,   23,
       3,   22,   24,   45,   57,   40,   57,   36,
     -20,    6,    9,   49,   47,   35,   19,    9,
     -17,   20,   32,   41,   58,   25,   30,    0,
      -9,   22,   22,   27,   27,   19,   10,   20,
];
#[rustfmt::skip]
const MG_KING: [i32; 64] = [
     -15,   36,   12,  -54,    8,  -28,   24,   14,
       1,    7,   -8,  -64,  -43,  -16,    9,    8,
     -14,  -14,  -22,  -46,  -44,  -30,  -15,  -27,
     -49,   -1,  -27,  -39,  -46,  -44,  -33,  -51,
     -17,  -20,  -12,  -27,  -30,  -25,  -14,  -36,
      -9,   24,    2,  -16,  -20,    6,   22,  -22,
      29,   -1,  -20,   -7,   -8,   -4,  -38,  -29,
     -65,   23,   16,  -15,  -56,  -34,    2,   13,
];
#[rustfmt::skip]
const EG_KING: [i32; 64] = [
     -53,  -34,  -21,  -11,  -28,  -14,  -24,  -43,
     -27,  -11,    4,   13,   14,    4,   -5,  -17,
     -19,   -3,   11,   21,   23,   16,    7,   -9,
     -18,   -4,   21,   24,   27,   23,    9,  -11,
      -8,   22,   24,   27,   26,   33,   26,    3,
      10,   17,   23,   15,   20,   45,   44,   13,
     -12,   17,   14,   17,   17,   38,   23,   11,
     -74,  -35,  -18,  -18,  -11,   15,    4,  -17,
];

/// Material tapered. Raciocinio:
///  - Peao 100 mg / 115 eg: valor classico "1 pawn = 100cp" no mg;
///    no eg o peao vale mais (proximidade da promocao, menos pecas
///    para o parar).
///  - Cavalo 320 mg / 285 eg: cavalo perde valor sem outras pecas por
///    perto para saltar (a mobilidade eficaz baixa no eg).
///  - Bispo 335 mg / 335 eg: bispo mantem valor no eg (diagonais
///    abertas com menos pecas).
///  - Torre 500 mg / 550 eg: torre ganha no eg (colunas abertas, 7a
///    fileira).
///  - Dama 950 mg / 960 eg: dama mantem-se (ambas fases).
///  - Rei 0: nao conta na soma material.
/// Distintos de PieceType::value() (usado por SEE/MVV-LVA sem fase).
// Com `fittedpsqtb8` entram os valores do ajuste: peao pregado como padrao de
// medida, tudo o resto encontrado em unidades de peao sobre 9M de posicoes com
// rotulos de um motor forte. A hierarquia sai do ajuste, nao da mao: cavalo
// 4,47 peoes, bispo 4,36, torre 6,69, dama 13,20. E a escala aterra em K=402,
// contra os 967 do conjunto anterior.
#[cfg(feature = "fittedpsqtb8")]
const MG_VALUE: [i32; 6] = [125, 559, 545, 836, 1651, 0];
#[cfg(feature = "fittedpsqtb8")]
const EG_VALUE: [i32; 6] = [140, 478, 538, 817, 1664, 0];
#[cfg(not(feature = "fittedpsqtb8"))]
const MG_VALUE: [i32; 6] = [125, 340, 355, 520, 990, 0];
#[cfg(not(feature = "fittedpsqtb8"))]
const EG_VALUE: [i32; 6] = [140, 300, 350, 570, 1050, 0];

/// Material values, overridable at runtime so they can be swept without a
/// rebuild.
///
/// These are the only evaluation numbers on the same footing as the search
/// margins, and they turned out to be the ones most worth questioning: in the
/// endgame our pieces are worth about three quarters of what they should be
/// relative to a pawn -- a rook prices at 3.8 pawns instead of 5 -- which is
/// the difference between finding the trade that wins an endgame and shuffling
/// in a position that is already won. Two real games against a 2367 bot showed
/// the same signature both times: middlegame accuracy at or above the
/// opponent, endgame accuracy below it.
///
/// Atomics rather than a OnceLock, and that is not a style choice. `warmup()`
/// evaluates a real position to pay the first-call cost off the clock, which
/// would seal a OnceLock before any `setoption` arrived -- the engine would
/// accept the option, report nothing wrong, and evaluate with the old number.
/// That exact failure cost a whole parameter sweep earlier today. A relaxed
/// atomic load costs the same as a plain load here and cannot be sealed.
static MG_ATOMIC: [std::sync::atomic::AtomicI32; 6] = [
    std::sync::atomic::AtomicI32::new(125),
    std::sync::atomic::AtomicI32::new(340),
    std::sync::atomic::AtomicI32::new(355),
    std::sync::atomic::AtomicI32::new(520),
    std::sync::atomic::AtomicI32::new(990),
    std::sync::atomic::AtomicI32::new(0),
];
static EG_ATOMIC: [std::sync::atomic::AtomicI32; 6] = [
    std::sync::atomic::AtomicI32::new(140),
    std::sync::atomic::AtomicI32::new(300),
    std::sync::atomic::AtomicI32::new(350),
    std::sync::atomic::AtomicI32::new(570),
    std::sync::atomic::AtomicI32::new(1050),
    std::sync::atomic::AtomicI32::new(0),
];

#[inline]
fn mg_value(i: usize) -> i32 {
    MG_ATOMIC[i].load(std::sync::atomic::Ordering::Relaxed)
}

#[inline]
fn eg_value(i: usize) -> i32 {
    EG_ATOMIC[i].load(std::sync::atomic::Ordering::Relaxed)
}

/// Set one material value by name, e.g. `eg_rook`. Returns false for an
/// unknown name so a typo is reported rather than silently ignored.
pub fn set_material(name: &str, value: i32) -> bool {
    invalida_cache_peoes();
    const PIECES: [&str; 5] = ["pawn", "knight", "bishop", "rook", "queen"];
    let (phase, piece) = match name.split_once('_') {
        Some(x) => x,
        None => return false,
    };
    let idx = match PIECES.iter().position(|&p| p == piece) {
        Some(i) => i,
        None => return false,
    };
    match phase {
        "mg" => MG_ATOMIC[idx].store(value, std::sync::atomic::Ordering::Relaxed),
        "eg" => EG_ATOMIC[idx].store(value, std::sync::atomic::Ordering::Relaxed),
        _ => return false,
    }
    true
}

/// Incremento de fase por peca -- 4 cavalos+4 bispos+4 torres+2 damas =
/// 4*1+4*1+4*2+2*4 = 24 = fase maxima (abertura). Fase 0 = so' reis e
/// peoes (final puro). Peao nao conta (fase so' mede pecas maiores).
const PHASE_INC: [i32; 6] = [0, 1, 1, 2, 4, 0];
const MAX_PHASE: i32 = 24;


/// Per-piece scaling of the piece-square tables, in per-mille.
///
/// The PSQTs were adopted as "generic public educational tables: a starting
/// point, not a finished state" and never calibrated against this engine. The
/// amplitudes say as much. Compared with a strong reference, ours are roughly
/// twice as loud for rooks and pawns in the middlegame -- and HALF as loud for
/// the king, in both phases.
///
/// That last one matters most: the king's table is what says "shelter now,
/// centralise later", and ours barely has an opinion. The error profile puts
/// our exposed king at 1.19 times its expected share of mistakes, which is the
/// same fact seen from the board.
/// PSQT amplitude, per phase bucket: `[bucket * 6 + piece]`.
///
/// The incremental accumulator in `Board` is built with the LAST bucket's
/// factors -- it has no board to ask, and rebuilding it whenever a pawn leaves
/// the board would cost more than the term is worth. Everything else is
/// corrected in the evaluation, and only for pieces whose factor actually
/// differs between buckets, which in the V4 profile is the queen alone.
static PSQT_SCALE: [std::sync::atomic::AtomicI32; 6 * NUM_BUCKETS] = {
    use std::sync::atomic::AtomicI32;
    [
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1350),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1350),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1350),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1350),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1350),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1350),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1350),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1350),
    ]
};

/// The bucket whose factors the incremental accumulator is built with.
const PSQT_BASE_BUCKET: usize = NUM_BUCKETS - 1;

const PSQT_NAMES: [&str; 6] = ["pawn", "knight", "bishop", "rook", "queen", "king"];

/// Set one piece's PSQT factor, e.g. `psqt_king`. False on an unknown name.
pub fn set_psqt_scale(name: &str, bucket: Option<usize>, per_mille: i32) -> bool {
    invalida_cache_peoes();
    let i = match PSQT_NAMES.iter().position(|&p| p == name) {
        Some(i) => i,
        None => return false,
    };
    match bucket {
        None => {
            for b in 0..NUM_BUCKETS {
                PSQT_SCALE[b * 6 + i].store(per_mille, std::sync::atomic::Ordering::Relaxed);
            }
        }
        Some(b) if b < NUM_BUCKETS => {
            PSQT_SCALE[b * 6 + i].store(per_mille, std::sync::atomic::Ordering::Relaxed);
        }
        Some(_) => return false,
    }
    PSQT_BUCKETS_DIFFER.store(
        (0..NUM_BUCKETS).any(|b| {
            (0..6).any(|k| {
                PSQT_SCALE[b * 6 + k].load(std::sync::atomic::Ordering::Relaxed)
                    != PSQT_SCALE[PSQT_BASE_BUCKET * 6 + k].load(std::sync::atomic::Ordering::Relaxed)
            })
        }),
        std::sync::atomic::Ordering::Relaxed,
    );
    true
}

/// Whether any bucket wants different PSQT amplitudes from the base one. When
/// false -- the common case -- the correction below is skipped entirely.
static PSQT_BUCKETS_DIFFER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether the family scales differ BETWEEN buckets -- that is, whether a
/// per-phase profile is driving them.
///
/// Deliberately not the same question as `family_scaling_active`, which has
/// been true since the V3 profile set king to 1100 and threats to 1150 for the
/// whole game. Asking that one instead silently disabled the material-bucket
/// correction with no per-bucket profile loaded at all, and cost 4 positions
/// on the suite before a single number had been changed -- a measurement of
/// nothing, presented as a measurement of the profile.
static FAMILY_BUCKETS_DIFFER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn family_buckets_differ() -> bool {
    FAMILY_BUCKETS_DIFFER.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the per-piece-count volume correction still applies.
///
/// It was switched off automatically whenever any bucket differed, on the
/// assumption that a bucket profile would always be flattening the same slope
/// and the two would multiply. That assumption is wrong for a profile that
/// uses buckets surgically -- moving weight from mobility to pawn structure in
/// locked positions, say -- while leaving the overall scale alone. Such a
/// profile replaces nothing, and turning this off costs the 4 suite positions
/// it is worth for no reason at all.
///
/// So it is explicit: `scale.material_buckets 0` in a profile turns it off,
/// and a profile that flattens the slope itself should say so.
static MATERIAL_BUCKETS_ON: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

pub fn set_material_buckets(on: bool) {
    invalida_cache_peoes();
    MATERIAL_BUCKETS_ON.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// What the accumulator got wrong for this position's bucket.
///
/// `Board` keeps material and piece-square scores incrementally, built with
/// PSQT_BASE_BUCKET's factors because a piece knows nothing about how many
/// pawns are on the board. Where a bucket asks for a different amplitude, the
/// difference is added here -- over the pieces that differ, which is normally
/// just the queens.
pub fn psqt_bucket_correction(board: &Board) -> i32 {
    if !PSQT_BUCKETS_DIFFER.load(std::sync::atomic::Ordering::Relaxed) {
        return 0;
    }
    let b = bucket_of(board);
    if b == PSQT_BASE_BUCKET {
        return 0;
    }
    let (mut mg, mut eg) = (0i32, 0i32);
    for pt in ALL_PIECES {
        let i = pt.idx();
        let f = PSQT_SCALE[b * 6 + i].load(std::sync::atomic::Ordering::Relaxed);
        let base = PSQT_SCALE[PSQT_BASE_BUCKET * 6 + i].load(std::sync::atomic::Ordering::Relaxed);
        if f == base {
            continue;
        }
        for c in [Color::White, Color::Black] {
            let sign = if c == Color::White { 1 } else { -1 };
            let mut bb = board.pieces[c.idx()][i];
            while bb != 0 {
                let sq = bb.trailing_zeros() as Square;
                bb &= bb - 1;
                mg += sign * (pst_mg(pt, c, sq) * f / 1000 - pst_mg(pt, c, sq) * base / 1000);
                eg += sign * (pst_eg(pt, c, sq) * f / 1000 - pst_eg(pt, c, sq) * base / 1000);
            }
        }
    }
    let phase = board.phase.min(MAX_PHASE);
    (mg * phase + eg * (MAX_PHASE - phase)) / MAX_PHASE
}

#[inline]
fn psqt_factor(idx: usize) -> i32 {
    PSQT_SCALE[PSQT_BASE_BUCKET * 6 + idx].load(std::sync::atomic::Ordering::Relaxed)
}

/// Piece-square tables loaded at runtime, replacing the compiled-in ones.
///
/// The tables in this file were adopted as generic public tables -- "a
/// starting point, not a finished state" -- and never calibrated here.
/// Measured against a strong reference, ours are about twice as loud for rooks
/// and pawns in the middlegame and HALF as loud for the king in both phases,
/// and the king's table is the one that says "shelter now, centralise later".
///
/// Replacing a whole table rather than scaling it, so generated tables can be
/// tried against the real suite and real games. Format: one line per table,
/// `MG_KING 12,-4,...` with 64 values, White's point of view, a1 first.
///
/// An env var rather than setoption, because 768 numbers do not belong on a
/// UCI line. Read once at startup, before any search.
static PSQT_OVERRIDE: OnceLock<Option<([Option<[i32; 64]>; 6], [Option<[i32; 64]>; 6])>> =
    OnceLock::new();

fn build_psqt_tables(
    m: std::collections::HashMap<String, [i32; 64]>,
) -> ([Option<[i32; 64]>; 6], [Option<[i32; 64]>; 6]) {
    const NAMES: [&str; 6] = ["PAWN", "KNIGHT", "BISHOP", "ROOK", "QUEEN", "KING"];
    let mut mg: [Option<[i32; 64]>; 6] = Default::default();
    let mut eg: [Option<[i32; 64]>; 6] = Default::default();
    for (i, n) in NAMES.iter().enumerate() {
        mg[i] = m.get(&format!("MG_{}", n)).copied();
        eg[i] = m.get(&format!("EG_{}", n)).copied();
    }
    (mg, eg)
}

fn psqt_override() -> &'static Option<([Option<[i32; 64]>; 6], [Option<[i32; 64]>; 6])> {
    PSQT_OVERRIDE.get_or_init(|| {
        let path = std::env::var("KESTREL_PSQT").ok()?;
        let text = std::fs::read_to_string(&path).ok()?;
        let mut m = std::collections::HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (name, rest) = match line.split_once(char::is_whitespace) {
                Some(x) => x,
                None => continue,
            };
            let v: Vec<i32> = rest
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse().ok())
                .collect();
            if v.len() == 64 {
                let mut a = [0i32; 64];
                a.copy_from_slice(&v);
                m.insert(name.to_string(), a);
            }
        }
        eprintln!("KESTREL_PSQT: {} tabelas carregadas de {}", m.len(), path);
        PSQT_OVERRIDE_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
        Some(build_psqt_tables(m))
    })
}

/// An overridden square, or None to use the compiled table.
///
/// Flat arrays indexed by piece, NOT a name lookup. The first version built a
/// String with `format!` and hit a HashMap on every square of every
/// evaluation -- the hottest path there is. Same tree, same node count, and
/// the engine ran at 58% speed: 563k nps down to 325k, which cost about 180
/// Elo and made every profile measured this afternoon look catastrophic when
/// the values were fine. The bench never showed it, because a bench counts
/// nodes and this only costs time.
/// Set once, read on every square. Checking a relaxed bool is free; going
/// through the OnceLock and an Option per square is not, and the normal case
/// -- no override at all, which is how the bot runs -- must cost nothing.
static PSQT_OVERRIDE_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// PSQT com dimensao de BUCKET: [bucket][peca][casa], MG e EG.
///
/// Porque isto teve de existir: o afinador, ao separar por contagem de peoes,
/// mostrou que a torre quer coisas OPOSTAS conforme a fase. Na setima fila
/// vale +56 quando ha' peoes para atacar e -32 quando nao ha' -- com o
/// tabuleiro quase vazio a setima nao serve para nada e o que conta e' a torre
/// atras do peao passado, na propria retaguarda.
///
/// Uma tabela global nao consegue exprimir isso. Qualquer numero que la' se
/// ponha e' a media de duas verdades contrarias, e a media de duas verdades
/// contrarias nao serve nenhuma delas. Foi exactamente assim que uma tabela
/// escrita a mao, com +50 na setima para todos os casos, perdeu 23.8 Elo em
/// 1171 partidas num livro de finais de torres feito para lhe ser favoravel.
pub static PSQT_BUCKETS_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// [bucket][peca][casa] para MG e EG. So' consultada quando ACTIVE.
pub static PSQT_BUCKETS: std::sync::OnceLock<Box<([[[i32; 64]; 6]; NUM_BUCKETS],
                                                  [[[i32; 64]; 6]; NUM_BUCKETS])>> =
    std::sync::OnceLock::new();

/// A contribuicao JA' CALCULADA de cada peca, por bucket, cor e casa:
/// material + PSQT do bucket + factor de amplitude, com o sinal da cor.
///
/// Sem isto, manter oito acumuladores custava 25% do debito: cada peca que
/// entrava ou saia do tabuleiro fazia oito vezes o calculo inteiro --
/// mirror_idx, leitura do factor, leitura da tabela -- em vez de oito somas.
/// Medido em maquina livre: 499k nos por segundo contra 375k.
///
/// Com a tabela, `make_move` passa a ler inteiros de memoria contigua e a
/// soma-los. Sao 8 x 2 x 6 x 64 pares de i32 = 48 KB, que cabe em L2 com
/// folga -- e o ciclo de oito e' desenrolado pelo compilador.
pub static PSQT_COMBINADO: std::sync::OnceLock<
    Box<[[[[(i32, i32); 64]; 6]; 2]; NUM_BUCKETS]>,
> = std::sync::OnceLock::new();

/// Carregar as PSQT de um vector afinado, bucket a bucket.
///
/// `v` tem NUM_BUCKETS * stride valores, no layout do extractor:
/// 705 posicionais | 12 de material | 768 de PSQT | 1 de bias.
/// As PSQT sao 6 pecas x 64 casas em MG e depois o mesmo em EG.
///
/// Os buckets que o dataset nao cobriu ficam com a tabela COMPILADA. Um
/// conjunto de treino so' de finais nao toca nos buckets de 12 peoes ou mais,
/// e escrever la' zeros ou valores extrapolados seria estragar o meio-jogo
/// para arranjar o final -- que e' precisamente o erro que este trabalho todo
/// existe para evitar. `cobertos` diz quais confiar.
pub fn psqt_buckets_de_vector(v: &[i32], stride: usize, cobertos: &[bool]) -> bool {
    if v.len() < NUM_BUCKETS * stride || cobertos.len() < NUM_BUCKETS {
        return false;
    }
    let mut mg = [[[0i32; 64]; 6]; NUM_BUCKETS];
    let mut eg = [[[0i32; 64]; 6]; NUM_BUCKETS];
    let tmg = [&MG_PAWN, &MG_KNIGHT, &MG_BISHOP, &MG_ROOK, &MG_QUEEN, &MG_KING];
    let teg = [&EG_PAWN, &EG_KNIGHT, &EG_BISHOP, &EG_ROOK, &EG_QUEEN, &EG_KING];
    const PSQT_OFF: usize = 705 + 12;
    for b in 0..NUM_BUCKETS {
        for p in 0..6 {
            for i in 0..64 {
                if cobertos[b] {
                    mg[b][p][i] = v[b * stride + PSQT_OFF + p * 64 + i];
                    eg[b][p][i] = v[b * stride + PSQT_OFF + 384 + p * 64 + i];
                } else {
                    mg[b][p][i] = tmg[p][i];
                    eg[b][p][i] = teg[p][i];
                }
            }
        }
    }
    if PSQT_BUCKETS.set(Box::new((mg, eg))).is_err() {
        return false;
    }
    PSQT_BUCKETS_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    psqt_combinado_reconstroi();
    true
}

/// Reconstroi a tabela combinada a partir das tabelas por bucket em vigor.
/// Chamada uma vez ao arranque e sempre que os buckets forem recarregados.
pub fn psqt_combinado_reconstroi() {
    let _ = PSQT_COMBINADO.set(constroi_combinado());
}

fn constroi_combinado() -> Box<[[[[(i32, i32); 64]; 6]; 2]; NUM_BUCKETS]> {
    let mut t = Box::new([[[[(0i32, 0i32); 64]; 6]; 2]; NUM_BUCKETS]);
    for b in 0..NUM_BUCKETS {
        for (ci, color) in [Color::White, Color::Black].iter().enumerate() {
            for pi in 0..6 {
                let kind = ALL_PIECES[pi];
                for sq in 0..64u8 {
                    let (mg, eg, _) = piece_contribution_bucket_lento(kind, *color, sq, b);
                    t[b][ci][pi][sq as usize] = (mg, eg);
                }
            }
        }
    }
    t
}

/// A tabela de um bucket. Se os buckets nao estiverem activos, devolve None e
/// tudo segue pelo caminho antigo.
#[inline]
fn psqt_do_bucket(bucket: usize, kind: PieceType, idx: usize, eg: bool) -> Option<i32> {
    if !cfg!(feature = "psqtbuckets") {
        return None;
    }
    let t = PSQT_BUCKETS.get_or_init(|| tabelas_do_ambiente().unwrap_or_else(tabelas_compiladas));
    let tab = if eg { &t.1 } else { &t.0 };
    Some(tab[bucket][kind.idx()][idx])
}

/// Encher os oito buckets com a MESMA tabela compilada.
///
/// E' o estado em que o invariante da identidade tem de valer: com as oito
/// iguais, o motor com buckets tem de dar avaliacoes identicas ao motor sem
/// eles, em qualquer posicao. Se diferirem num centipeao, ha' erro na
/// refactorizacao e sabe-se antes de gastar um jogo.
/// As tabelas por bucket, do ficheiro se houver um, senao as compiladas.
///
/// KESTREL_PSQT_BUCKETS aponta para um ficheiro de pesos no layout do
/// extractor; KESTREL_PSQT_COBERTOS diz quais os buckets a confiar
/// (ex. "0,1,2,3,4"). Os restantes ficam com as compiladas -- um treino so'
/// de finais nao produz sinal nenhum para os buckets de muitos peoes.
fn tabelas_do_ambiente() -> Option<Box<([[[i32; 64]; 6]; NUM_BUCKETS], [[[i32; 64]; 6]; NUM_BUCKETS])>> {
    let caminho = std::env::var("KESTREL_PSQT_BUCKETS").ok()?;
    let txt = std::fs::read_to_string(&caminho).ok()?;
    let v: Vec<i32> = txt.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty()).filter_map(|t| t.parse().ok()).collect();
    if v.len() < NUM_BUCKETS { return None; }
    let stride = v.len() / NUM_BUCKETS;
    let mut cobertos = [true; NUM_BUCKETS];
    if let Ok(lista) = std::env::var("KESTREL_PSQT_COBERTOS") {
        cobertos = [false; NUM_BUCKETS];
        for t in lista.split(',') {
            if let Ok(b) = t.trim().parse::<usize>() { if b < NUM_BUCKETS { cobertos[b] = true; } }
        }
    }
    let mut mg = [[[0i32; 64]; 6]; NUM_BUCKETS];
    let mut eg = [[[0i32; 64]; 6]; NUM_BUCKETS];
    let tmg = [&MG_PAWN, &MG_KNIGHT, &MG_BISHOP, &MG_ROOK, &MG_QUEEN, &MG_KING];
    let teg = [&EG_PAWN, &EG_KNIGHT, &EG_BISHOP, &EG_ROOK, &EG_QUEEN, &EG_KING];
    const PSQT_OFF: usize = 705 + 12;
    for b in 0..NUM_BUCKETS {
        for p in 0..6 {
            for i in 0..64 {
                if cobertos[b] && b * stride + PSQT_OFF + 384 + p * 64 + i < v.len() {
                    mg[b][p][i] = v[b * stride + PSQT_OFF + p * 64 + i];
                    eg[b][p][i] = v[b * stride + PSQT_OFF + 384 + p * 64 + i];
                } else {
                    mg[b][p][i] = tmg[p][i];
                    eg[b][p][i] = teg[p][i];
                }
            }
        }
    }
    eprintln!("info string psqt buckets de {} (cobertos {:?})", caminho, cobertos);
    Some(Box::new((mg, eg)))
}

fn tabelas_compiladas() -> Box<([[[i32; 64]; 6]; NUM_BUCKETS], [[[i32; 64]; 6]; NUM_BUCKETS])> {
    // Com `fittedpsqtb8` as tabelas afinadas entram compiladas. Sem ela, os
    // oito buckets recebem a MESMA tabela de sempre -- que e' a condicao do
    // invariante da identidade e o que mantem o bench em 3377667.
    if cfg!(feature = "fittedpsqtb8") {
        return Box::new((crate::fitted_psqt_b8::PSQT_MG, crate::fitted_psqt_b8::PSQT_EG));
    }
    let mut mg = [[[0i32; 64]; 6]; NUM_BUCKETS];
    let mut eg = [[[0i32; 64]; 6]; NUM_BUCKETS];
    let tmg = [&MG_PAWN, &MG_KNIGHT, &MG_BISHOP, &MG_ROOK, &MG_QUEEN, &MG_KING];
    let teg = [&EG_PAWN, &EG_KNIGHT, &EG_BISHOP, &EG_ROOK, &EG_QUEEN, &EG_KING];
    for b in 0..NUM_BUCKETS {
        for p in 0..6 {
            for i in 0..64 { mg[b][p][i] = tmg[p][i]; eg[b][p][i] = teg[p][i]; }
        }
    }
    Box::new((mg, eg))
}

pub fn psqt_buckets_iguais() {
    let mut mg = [[[0i32; 64]; 6]; NUM_BUCKETS];
    let mut eg = [[[0i32; 64]; 6]; NUM_BUCKETS];
    let tmg = [&MG_PAWN, &MG_KNIGHT, &MG_BISHOP, &MG_ROOK, &MG_QUEEN, &MG_KING];
    let teg = [&EG_PAWN, &EG_KNIGHT, &EG_BISHOP, &EG_ROOK, &EG_QUEEN, &EG_KING];
    for b in 0..NUM_BUCKETS {
        for p in 0..6 {
            for i in 0..64 {
                mg[b][p][i] = tmg[p][i];
                eg[b][p][i] = teg[p][i];
            }
        }
    }
    let _ = PSQT_BUCKETS.set(Box::new((mg, eg)));
    PSQT_BUCKETS_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[inline]
fn psqt_from_override(kind: PieceType, idx: usize, eg: bool) -> Option<i32> {
    if !PSQT_OVERRIDE_ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let tabs = psqt_override().as_ref()?;
    let t = if eg { &tabs.1 } else { &tabs.0 };
    t[kind.idx()].as_ref().map(|a| a[idx])
}

/// Load one file that holds every tunable number in the engine.
///
/// The motivation is coherence, not convenience. The values arrived from
/// different places and were never designed together: piece-square tables
/// adopted generic and never calibrated, search margins half inherited from
/// another engine and half our own, material somewhere in between. Anyone
/// designing a consistent set has to be able to see and set all of it at once,
/// and the blocks are coupled -- pruning margins are compared against
/// evaluation scores, the PSQT is a bonus added to material, and four
/// constants read the history tables against their scale.
///
/// Format: `<section>.<name> <value(s)>`, lists comma-separated, `#` comments.
/// Anything absent keeps its compiled-in value, so a profile can be partial.
/// Say so when a profile writes 1000 over a value that was not 1000.
///
/// In a system of multipliers 1000 reads as the identity, and it is not one
/// here: the loader SETS, it does not multiply. A profile that writes
/// `scale.king 1000` is not leaving the king alone, it is discarding the
/// 1100 the V3 profile measured. Omitting the key is what leaves it alone.
/// This has already cost one afternoon -- a V4 draft rescaled the king from
/// 1100 to a slope-flattening curve and erased a tuned setting while
/// appearing only to rescale.
fn warn_if_undoing_v3(section: &str, name: &str, default: Option<i32>, rest: &str) {
    let Some(d) = default else { return };
    if d == 1000 {
        return;
    }
    if rest.trim().split_whitespace().next().and_then(|v| v.parse::<i32>().ok()) == Some(1000) {
        eprintln!(
            "perfil: AVISO -- {}.{} = 1000 sobrepoe o default {} (afinado, vem do V3). \
Omitir a chave preserva-o; declarar 1000 apaga-o.",
            section, name, d
        );
    }
}

/// `king` -> ("king", None); `king.3` -> ("king", Some(3)).
fn split_bucket(name: &str) -> (&str, Option<usize>) {
    match name.rsplit_once('.') {
        Some((head, tail)) => match tail.parse::<usize>() {
            Ok(b) => (head, Some(b)),
            Err(_) => (name, None),
        },
        None => (name, None),
    }
}

pub fn load_profile(path: &str) -> Result<usize, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut n = 0usize;
    let mut psqt: std::collections::HashMap<String, [i32; 64]> = std::collections::HashMap::new();
    for (lineno, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, rest) = match line.split_once(char::is_whitespace) {
            Some(x) => x,
            None => continue,
        };
        let rest = rest.trim();
        let (section, name) = match key.split_once('.') {
            Some(x) => x,
            None => continue,
        };
        let one = || -> Option<i32> { rest.parse().ok() };
        let ok = match section {
            "material" => one().map(|v| set_material(name, v)).unwrap_or(false),
            // `<family>` means the whole game; `<family>.<bucket>` means one
            // phase. Both forms are accepted so a profile written before
            // buckets existed still loads and still means what it meant.
            "scale" if name == "material_buckets" => {
                one().map(|v| { set_material_buckets(v != 0); true }).unwrap_or(false)
            }
            "scale" => {
                let (fam, bucket) = split_bucket(name);
                warn_if_undoing_v3("scale", fam, family_scale_default(fam), &rest);
                one().map(|v| set_family_scale(fam, bucket, v)).unwrap_or(false)
            }
            "psqt_scale" => {
                let (piece, bucket) = split_bucket(name);
                warn_if_undoing_v3("psqt_scale", piece, psqt_scale_default(piece), &rest);
                one().map(|v| set_psqt_scale(piece, bucket, v)).unwrap_or(false)
            }
            "search" => one()
                .map(|v| crate::search::set_param(name, v))
                .unwrap_or(false),
            "psqt" => {
                let v: Vec<i32> = rest
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .filter(|s| !s.is_empty())
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if v.len() == 64 {
                    let mut a = [0i32; 64];
                    a.copy_from_slice(&v);
                    psqt.insert(name.to_string(), a);
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if ok {
            n += 1;
        } else {
            // Loud, not silent. A profile that half-applies is worse than one
            // that fails: the engine would run with a set nobody designed.
            eprintln!("perfil: linha {} ignorada -- '{}'", lineno + 1, key);
        }
    }
    if !psqt.is_empty() {
        let count = psqt.len();
        PSQT_OVERRIDE_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
        if PSQT_OVERRIDE.set(Some(build_psqt_tables(psqt))).is_err() {
            eprintln!("perfil: tabelas ja fixadas, {} ignoradas", count);
        }
    }
    Ok(n)
}

fn mirror_idx(color: Color, s: Square) -> usize {
    if color == Color::White {
        s as usize
    } else {
        let f = file_of(s);
        let r = 7 - rank_of(s);
        (r * 8 + f) as usize
    }
}

/// A mesma casa, mas lida do ponto de vista de "o meu rei esta' no flanco da
/// dama". Se estiver no do rei, espelha-se a COLUNA.
///
/// Duas tabelas por peca, a segunda e' a primeira com a coluna trocada. Nao
/// custa um unico peso novo: e' a MESMA tabela lida de duas maneiras.
///
/// O que resolve: hoje damos o mesmo valor a um cavalo em f5 quer o nosso rei
/// esteja em g1 quer esteja em c1, e a um peao na coluna h quer ele seja
/// abrigo do nosso roque quer seja um passado remoto do outro lado do
/// tabuleiro. Sao coisas diferentes e a tabela nao as distinguia.
///
/// Medido: a nossa cauda de erros caros e' 7% dos lances tranquilos em finais
/// e 0% na abertura -- e o final e' precisamente onde a posicao do rei decide
/// o valor de cada casa, enquanto na abertura os dois reis estao quase sempre
/// no mesmo sitio.
#[inline]
fn psqt_idx(color: Color, s: Square, rei_no_flanco_do_rei: bool) -> usize {
    let i = mirror_idx(color, s);
    if rei_no_flanco_do_rei { i ^ 0b111 } else { i }
}

/// O flanco do rei de uma cor: verdadeiro se estiver nas colunas e-h.
#[inline]
pub fn flanco_do_rei(board: &Board, c: Color) -> bool {
    file_of(board.king_sq(c)) >= 4
}

fn pst_mg(kind: PieceType, color: Color, s: Square) -> i32 {
    pst_mg_esp(kind, color, s, false)
}

fn pst_mg_esp(kind: PieceType, color: Color, s: Square, esp: bool) -> i32 {
    let idx = psqt_idx(color, s, esp);
    if let Some(v) = psqt_from_override(kind, idx, false) {
        return v;
    }
    match kind {
        PieceType::Pawn => MG_PAWN[idx],
        PieceType::Knight => MG_KNIGHT[idx],
        PieceType::Bishop => MG_BISHOP[idx],
        PieceType::Rook => MG_ROOK[idx],
        PieceType::Queen => MG_QUEEN[idx],
        PieceType::King => MG_KING[idx],
    }
}

fn pst_eg(kind: PieceType, color: Color, s: Square) -> i32 {
    pst_eg_esp(kind, color, s, false)
}

fn pst_eg_esp(kind: PieceType, color: Color, s: Square, esp: bool) -> i32 {
    let idx = psqt_idx(color, s, esp);
    if let Some(v) = psqt_from_override(kind, idx, true) {
        return v;
    }
    match kind {
        PieceType::Pawn => EG_PAWN[idx],
        PieceType::Knight => EG_KNIGHT[idx],
        PieceType::Bishop => EG_BISHOP[idx],
        PieceType::Rook => EG_ROOK[idx],
        PieceType::Queen => EG_QUEEN[idx],
        PieceType::King => EG_KING[idx],
    }
}

/// Contribuicao (mg, eg, incremento de fase) de UMA peca numa casa, do
/// ponto de vista das BRANCAS (ja' com o sinal aplicado: positivo para
/// brancas, negativo para pretas) -- usado por board.rs para manter
/// `mg_score`/`eg_score`/`phase` actualizados incrementalmente em
/// add_piece()/remove_piece(), em vez de recalcular material_pst() do
/// zero em cada no' da busca (era o maior custo por no' que faltava
/// tornar incremental).
/// Ligar a leitura das PSQT pelo flanco do rei. Interruptor de compilacao para
/// as duas versoes poderem ser medidas uma contra a outra como dois binarios,
/// que e' a unica comparacao em que confiamos.
pub const PSQT_ESPELHO_REI: bool = cfg!(feature = "psqtmirror");

/// A contribuicao de cada peca ja' calculada: material + PSQT + factor de
/// amplitude, com o sinal da cor. [cor][peca][casa], 12 KB.
///
/// Nao tem nada que ver com buckets -- e' a mesma ideia aplicada ao motor
/// normal. Cada peca que entra ou sai do tabuleiro fazia mirror_idx, lia o
/// factor, indexava a tabela, multiplicava por mil e dividia, somava o
/// material e aplicava o sinal. Tudo isso e' deterministico e cabe numa
/// tabela; `make_move` passa a ler dois inteiros.
static CONTRIB: std::sync::OnceLock<Box<[[[(i32, i32); 64]; 6]; 2]>> =
    std::sync::OnceLock::new();

fn constroi_contrib() -> Box<[[[(i32, i32); 64]; 6]; 2]> {
    let mut t = Box::new([[[(0i32, 0i32); 64]; 6]; 2]);
    for (ci, color) in [Color::White, Color::Black].iter().enumerate() {
        for pi in 0..6 {
            let kind = ALL_PIECES[pi];
            for sq in 0..64u8 {
                let (mg, eg, _) = piece_contribution_lenta(kind, *color, sq);
                t[ci][pi][sq as usize] = (mg, eg);
            }
        }
    }
    t
}

#[inline(always)]
pub fn piece_contribution(kind: PieceType, color: Color, s: Square) -> (i32, i32, i32) {
    let t = CONTRIB.get_or_init(constroi_contrib);
    let (mg, eg) = t[color.idx()][kind.idx()][s as usize];
    (mg, eg, PHASE_INC[kind.idx()])
}

fn piece_contribution_lenta(kind: PieceType, color: Color, s: Square) -> (i32, i32, i32) {
    piece_contribution_flanco(kind, color, s, false)
}

/// A contribuicao de uma peca lida pela tabela de um bucket especifico.
/// Lookup na tabela combinada. E' esta que o `make_move` usa.
#[inline(always)]
pub fn piece_contribution_bucket(
    kind: PieceType, color: Color, s: Square, bucket: usize,
) -> (i32, i32, i32) {
    // get_or_init em vez de get: assim quem carrega tabelas afinadas pode
    // faze-lo ANTES da primeira leitura, sem lutar com um OnceLock ja' cheio.
    // Encher no arranque obrigava o carregador a chegar primeiro que o main, o
    // que nao e' possivel.
    let t = PSQT_COMBINADO.get_or_init(constroi_combinado);
    let (mg, eg) = t[bucket][color.idx()][kind.idx()][s as usize];
    (mg, eg, PHASE_INC[kind.idx()])
}

/// O calculo por extenso. So' usado para CONSTRUIR a tabela acima.
fn piece_contribution_bucket_lento(
    kind: PieceType, color: Color, s: Square, bucket: usize,
) -> (i32, i32, i32) {
    let sign = if color == Color::White { 1 } else { -1 };
    let f = psqt_factor(kind.idx());
    let idx = mirror_idx(color, s);
    let pmg = psqt_do_bucket(bucket, kind, idx, false).unwrap_or_else(|| pst_mg(kind, color, s));
    let peg = psqt_do_bucket(bucket, kind, idx, true).unwrap_or_else(|| pst_eg(kind, color, s));
    let (pmg, peg) = if f == 1000 { (pmg, peg) } else { (pmg * f / 1000, peg * f / 1000) };
    (sign * (mg_value(kind.idx()) + pmg), sign * (eg_value(kind.idx()) + peg), PHASE_INC[kind.idx()])
}

/// A contribuicao de uma peca, lida do ponto de vista de um dado flanco do rei
/// DA SUA PROPRIA COR.
pub fn piece_contribution_flanco(
    kind: PieceType,
    color: Color,
    s: Square,
    flanco_rei: bool,
) -> (i32, i32, i32) {
    let sign = if color == Color::White { 1 } else { -1 };
    let f = psqt_factor(kind.idx());
    let esp = PSQT_ESPELHO_REI && flanco_rei;
    let (pmg, peg) = if f == 1000 {
        (pst_mg_esp(kind, color, s, esp), pst_eg_esp(kind, color, s, esp))
    } else {
        (pst_mg_esp(kind, color, s, esp) * f / 1000,
         pst_eg_esp(kind, color, s, esp) * f / 1000)
    };
    let mg = sign * (mg_value(kind.idx()) + pmg);
    let eg = sign * (eg_value(kind.idx()) + peg);
    (mg, eg, PHASE_INC[kind.idx()])
}

// === Tuning de material/PST (comando `tunepst`) =========================
// As tabelas de material (MG_VALUE/EG_VALUE) e PST (MG_PAWN..EG_KING) sao
// tabelas educacionais publicas genericas ("ponto de partida, nao estado final").
// Calibra-las sobre os dados do Kestrel e' o maior bloco de valores por
// afinar. Como o material/PST e' LINEAR nos valores (o material_pst_white
// e' uma soma ponderada por fase das contribuicoes por peca), pode-se
// tunar com a mesma matematica de regressao logistica do `tune_fast`: bias = positional
// (fixo), features = as contagens de material/PST por casa/tipo, pesos
// tunaveis = os 780 valores. Estas funcoes SO' sao usadas pelo tuner --
// o eval de producao continua a usar as consts via o board incremental;
// os valores tunados sao escritos de volta nas consts no fim.

/// Dimensao do vector de material/PST: 6+6 material + 6 pecas x 64 x 2
/// (mg/eg) = 12 + 768 = 780.
pub const MAT_PST_DIM: usize = 12 + 6 * 64 * 2;

/// Vector actual de material/PST (mesma ordem que `material_pst_features`
/// abaixo), para servir de ponto de partida ao tuner.
pub fn material_pst_current_vec() -> Vec<i32> {
    let mut v = Vec::with_capacity(MAT_PST_DIM);
    for pt in 0..6 { v.push(MG_VALUE[pt]); }
    for pt in 0..6 { v.push(EG_VALUE[pt]); }
    let tables_mg = [&MG_PAWN, &MG_KNIGHT, &MG_BISHOP, &MG_ROOK, &MG_QUEEN, &MG_KING];
    let tables_eg = [&EG_PAWN, &EG_KNIGHT, &EG_BISHOP, &EG_ROOK, &EG_QUEEN, &EG_KING];
    // Through the same scale the evaluation applies. `piece_contribution`
    // multiplies each table by `psqt_factor`, so returning the raw tables
    // described a set of weights the engine does not use. It cost 5cp per
    // position in the feature extractor's self-check -- reported as a
    // positional non-linearity, and hunted as one, when it was material all
    // along and entirely linear.
    //
    // The king's factor is 1350, and that is deliberate, not an artefact:
    // measured in the project's own PSQT simulator, our king table has half
    // the amplitude of a strong reference's, and 1350 was chosen there as the
    // compromise between a fixed-time reading (115) and one blind to castling
    // (122). It is recorded in perfil_v3.txt with that reasoning. Nobody
    // should "restore" it to 1000 on the assumption that a round number was
    // meant -- if it changes, it changes because the simulator says so.
    for (i, t) in tables_mg.iter().enumerate() {
        let f = psqt_factor(i);
        for &x in t.iter() { v.push(if f == 1000 { x } else { x * f / 1000 }); }
    }
    for (i, t) in tables_eg.iter().enumerate() {
        let f = psqt_factor(i);
        for &x in t.iter() { v.push(if f == 1000 { x } else { x * f / 1000 }); }
    }
    v
}

/// Preenche `feats[0..MAT_PST_DIM]` com a contribuicao marginal de cada
/// valor de material/PST ao `material_pst_white(board)`, JA' ponderada
/// pela fase (porque o eval final e' tapered). Garantia (testada em
/// `checkmatpst`): `sum(feats[i] * material_pst_current_vec()[i]) ~=
/// material_pst_white(board)` a menos do arredondamento inteiro do taper.
/// Ordem: [MG_VALUE 0..6][EG_VALUE 6..12][MG_PST pecas 12..396][EG_PST
/// pecas 396..780], PST por peca na ordem P,N,B,R,Q,K, 64 casas cada.
pub fn material_pst_features(board: &Board, feats: &mut [f32]) {
    for f in feats.iter_mut().take(MAT_PST_DIM) { *f = 0.0; }
    let phase = board.phase.min(MAX_PHASE);
    let mg_w = phase as f32 / MAX_PHASE as f32;
    let eg_w = (MAX_PHASE - phase) as f32 / MAX_PHASE as f32;
    const MG_VAL_OFF: usize = 0;
    const EG_VAL_OFF: usize = 6;
    const MG_PST_OFF: usize = 12;
    const EG_PST_OFF: usize = 12 + 6 * 64;
    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1.0 } else { -1.0 };
        for pt in ALL_PIECES {
            let mut bb = board.pieces[c.idx()][pt.idx()];
            while bb != 0 {
                let s = bb.trailing_zeros() as Square;
                bb &= bb - 1;
                let idx = mirror_idx(c, s);
                let pt_i = pt.idx();
                feats[MG_VAL_OFF + pt_i] += sign * mg_w;
                feats[EG_VAL_OFF + pt_i] += sign * eg_w;
                feats[MG_PST_OFF + pt_i * 64 + idx] += sign * mg_w;
                feats[EG_PST_OFF + pt_i * 64 + idx] += sign * eg_w;
            }
        }
    }
}

/// Zona do rei: a propria casa + as 8 vizinhas (igual ao king_attacks).
fn king_zone(king_sq: Square) -> Bitboard {
    let near = atk().king[king_sq as usize] | bb(king_sq);
    #[cfg(not(feature = "widekingzone"))]
    {
        near
    }
    // The eight neighbours plus the rank in front of them, and a file wider
    // when the king sits on a rook file.
    //
    // The narrow zone was measured blind, not merely miscalibrated. After
    // 18.g3 f4 in a real loss, with a knight on h5, a knight on g6, a bishop
    // on d6 and the queen on e7 all pointed at our king, our king-safety term
    // read ZERO where a strong reference read -65. Removing each attacker in
    // turn and re-reading the term: no knight, 0; no other knight, 1; no
    // BISHOP, 0; no QUEEN, 9. Taking the queen out of a mating attack moved
    // king safety by nine centipawns. That is not a weight that needs
    // adjusting, it is a term that cannot see.
    //
    // The cause: with the king on g1 the zone was f1,g1,h1,f2,g2,h2. An attack
    // on a castled king is built on the third rank -- f3, g3, h3 -- and none of
    // them were in it. A knight landing on g3 and a pawn arriving on f4 were
    // invisible.
    //
    // Widening was tried on 27/07 and scored 35 of 59 against 37, so it was
    // shelved. That margin is two positions, and this project has since
    // measured what two positions are worth: a four-position gap came out of
    // fifty disagreements at 26-24, p = 0.89. Two is noise, and a mechanism
    // this clearly demonstrated should not have been dismissed by it.
    //
    // Behind a feature because it changes what the extractor sees, so the
    // king-safety weights have to be refitted with it before it means
    // anything -- which is exactly what the 27/07 note said to do and what
    // there was no working tuner to do with.
    #[cfg(feature = "widekingzone")]
    {
        let forward = if rank_of(king_sq) < 4 { north(near) } else { south(near) };
        let sideways = if (bb(king_sq) & FILE_A) != 0 {
            east(near)
        } else if (bb(king_sq) & FILE_H) != 0 {
            west(near)
        } else {
            0
        };
        near | forward | sideways
    }
}

// Pesos de eval -- valores proprios sensatos como ponto de partida,
// para afinar via o nosso tuner de regressao logistica.
// Formato ScorePair (mg, eg), interpolados em positional_terms() pela
// fase actual. Estrutura (mobility por peca e count, threats indexadas
// por defended, king safety com attackers+attack_count, etc.) e' a
// organizacao classica de um HCE forte. Os valores abaixo sao os
// nossos -- monotonos e razoaveis, afinaveis pelo nosso tuner.

// === Structural bonuses (raciocinio explicito) ===
// BISHOP_PAIR: dois bispos cobrem todas as cores; vantagem cresce no
// eg (diagonais abertas com menos pecas). Valor classico 30-50.
const BISHOP_PAIR: (i32, i32) = (30, 55);
// Bispo na longa diagonal central (a1-h8 ou h1-a8) que ataca >=2 casas
// do centro (d4/e4/d5/e5) -- peca activa, small bonus.
const LONG_DIAG_BISHOP: (i32, i32) = (10, 12);
// Cavalo/bispo com peao proprio directamente a frente -- abrigo,
// pequeno bonus por seguranca.
const MINOR_BEHIND_PAWN: (i32, i32) = (5, 6);
// Cavalo em outpost (casa avancada, defendida por peao, sem peao
// inimigo nas colunas adjacentes que possa capturar) -- peca dominante.
const KNIGHT_OUTPOST: (i32, i32) = (25, 20);
// Torre em coluna aberta / semi-aberta. Aberta = mais mg (linhas de
// ataque), eg mantem-se. Semi-open = metade.
const ROOK_OPEN: [(i32, i32); 2] = [(30, 12), (18, 8)];
// ROOK_ON_SEVENTH: 2026-07-23, new -- dedicated bonus for a rook on
// the 7th rank (relative), independent of open-file status; before this
// we folded 7th-rank activity into other terms. Value expressed on
// Kestrel's pawn anchor (mg=125,eg=140), same scaling discipline as
// PASSED_PAWN above.
const ROOK_ON_SEVENTH: (i32, i32) = (-2, 41);
// Tempo -- lado que joga tem pequena vantagem estrutural (iniciativa).
// Valor classico 15-25.
const TEMPO: (i32, i32) = (22, 15);

/// Avanco TRANCADO: o peao dele que vem contra o nosso rei tem um peao nosso
/// imediatamente a' frente, cabeca com cabeca, e nao passa dali.
///
/// Um peao que avanca contra o rei so' e' perigoso porque pode ABRIR a coluna.
/// Trancado nao abre nada -- nem avanca, nem troca, nem cria linha. O termo
/// normal de avanco assume que ele pode progredir, e por isso pune uma ameaca
/// que nao existe.
///
/// Arranca a zero: a distincao entra agora, os valores vem do ajuste.
const TORNADO_LOCKED: [(i32, i32); 4] = [(0, 0); 4];

/// Peao passado EXTERIOR -- quanto vale estar longe do centro.
///
/// Aplicado por unidade de centralidade (0 nas colunas a/h, 3 nas d/e). O
/// passado exterior e' conhecimento classico de finais: arrasta o rei
/// defensor para um lado do tabuleiro e o atacante decide no outro. Com pecas
/// no tabuleiro a logica inverte-se, porque um passado central e' mais activo
/// e apoia mais depressa.
///
/// A nossa tabela de passado e' [bloqueado][controlado][fila] e nao tinha
/// dimensao de COLUNA nenhuma -- um passado na coluna a valia o mesmo que na
/// coluna d. Arranca a zero; os valores vem do ajuste.
const PASSER_OUTSIDE: (i32, i32) = (0, 0);

/// Desequilibrio material: quanto vale cada peca NOSSA contra cada peca DELE.
///
/// [nossa][dele], indices 0..4 = peao cavalo bispo torre dama. Dois cavalos
/// contra dois bispos nao vale o mesmo que dois cavalos contra cavalo e bispo,
/// e uma soma de valores fixos por peca nao pode dizer isso -- so' o produto
/// cruzado pode. Ambos os motores de referencia tem esta tabela e nos nao
/// tinhamos nada.
///
/// Arranca a ZERO: a estrutura entra agora, os valores vem do ajuste. Comecar
/// com numeros inventados era pior do que nao ter o termo.
const IMBALANCE: [[(i32, i32); 5]; 5] = [[(0, 0); 5]; 5];

// === Mobility ===
// Ideia geral: 0 lances legais = peca presa, penalidade forte. Curva
// concava crescente ate' plateau (mobilidade extra alem de "activa" da'
// diminishing returns). Piece-specific: dama tem 27 slots mas o valor
// da mobilidade e' menor em cada slot (dama ja' e' potente sem precisar
// de mobility). Cavalo tem so' 8 slots mas cada casa vale mais (cavalo
// preso em canto vale muito pouco). eg = ligeiramente mais baixo que
// mg em geral (mobility conta menos com menos pecas para interagir).
const MOBILITY_KNIGHT: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    // 0..=8 lances
    let mg = [-40, -15, -5, 5, 12, 18, 25, 30, 35];
    let eg = [-32, -14, -5, 3, 9, 14, 18, 22, 25];
    let mut i = 0; while i < 9 { t[i] = (mg[i], eg[i]); i += 1; }
    t
};
const MOBILITY_BISHOP: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    // 0..=13 lances
    let mg = [-40, -20, -8, 0, 7, 13, 18, 22, 25, 28, 30, 32, 34, 36];
    let eg = [-30, -18, -8, -2, 5, 10, 14, 18, 20, 22, 24, 25, 26, 27];
    let mut i = 0; while i < 14 { t[i] = (mg[i], eg[i]); i += 1; }
    t
};
const MOBILITY_ROOK: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    // 0..=14 lances -- torre ganha mais no eg (colunas abertas)
    let mg = [-45, -25, -12, -4, 2, 7, 12, 16, 20, 23, 25, 27, 28, 28, 28];
    let eg = [-35, -22, -12, -4, 3, 8, 13, 18, 23, 28, 30, 32, 33, 34, 34];
    let mut i = 0; while i < 15 { t[i] = (mg[i], eg[i]); i += 1; }
    t
};
const MOBILITY_QUEEN: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    // 0..=27 lances. Cada slot vale menos (dama ja' e' potente).
    // Plateau depois de ~20 lances.
    let mg = [-30, -25, -15, -8, -3, 2, 6, 10, 13, 16, 18, 20, 22, 23, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24];
    let eg = [-25, -20, -15, -8, -3, 2, 5, 8, 11, 14, 16, 18, 20, 21, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22];
    let mut i = 0; while i < 28 { t[i] = (mg[i], eg[i]); i += 1; }
    t
};

// === King safety ===
// Peso por peca a atacar a zona do rei inimigo. Dama pesa MUITO (peca
// suprema no ataque), torre pesa ~2x menor, menores pesam menos.
// eg = negativo pequeno -- ataques ao rei importam pouco quando ja
// nao ha muitas pecas para atacar. Baseado no padrao classico de
// "attack units" da avaliacao de seguranca do rei.
// Moderado ~25% em 2026-07-22: dois lotes reais consecutivos contra
// oponentes fortes e precisos mostraram
// um padrao recorrente de sacrificios/trocas especulativas do kestrel
// sem compensacao suficiente, consistente com estes pesos a empurrar
// para ataques ao rei que a busca pratica nao consegue validar em
// profundidade suficiente. Pedido do utilizador: "isso nao e'
// compativel com o jogo entre motores" -- moderado, nao eliminado (o
// estilo agressivo continua a existir, so' menos extremo). Valores
// antigos preservados em comentario para referencia/reversao.
const KING_ATTACKER_WEIGHT: [(i32, i32); 4] = [
    (15, -2),   // Cavalo (era 20,-3)
    (13, -2),   // Bispo (era 18,-3)
    (26, -4),   // Torre (era 35,-5)
    (48, -4),   // Dama (era 65,-5)
];
// Extra por casa da king zone atacada, alem do bonus por atacante.
const KING_ATTACKS: (i32, i32) = (5, 0);
// SAFE_CHECK: unidade de perigo por peca que poderia dar um xeque numa
// casa sem qualquer defensor inimigo (ver a segunda passagem em
// positional_terms(), depois do loop principal). Valor deliberadamente
// pequeno e SEPARADO de king_attacks -- a primeira versao reutilizava
// king_attacks directamente e o A/B (2026-07-22, 300 jogos self-play)
// deu 46.8%, negativo e persistente ao longo de todo o lote. Campo
// proprio para poder ser recalibrado sem afectar o resto do king
// safety, e para o proximo A/B isolar se o problema era mesmo a
// magnitude (hipotese principal) ou o novo threshold de 1-atacante-
// com-dama (ver endgame_scale_factor -- king_attackers[]/threshold
// continuam iguais, so' o peso mudou aqui).
// 2026-07-23: per-piece-type split, replacing the single flat
// SAFE_CHECK that used to be here. A counter-intuitive ordering holds
// here -- rook and knight checks are weighted HIGHER than queen checks
// (queen danger is captured elsewhere in the king-safety model; a lone
// queen check without support is less "mating" than it looks). The
// per-piece weights are anchored to what Kestrel's own king-danger
// curve (KING_DANGER_TABLE) is already calibrated for, preserving the
// relative ordering/proportions between pieces. The old ad-hoc "queen
// counts double" multiplier is dropped now that queen has its own
// (lower) dedicated weight -- keeping both would double-correct in
// opposite directions.
const SAFE_KNIGHT_CHECK: (i32, i32) = (2, 0);
const SAFE_BISHOP_CHECK: (i32, i32) = (2, 2);
const SAFE_ROOK_CHECK: (i32, i32) = (3, 1);
const SAFE_QUEEN_CHECK: (i32, i32) = (1, 2);
// WEAK_KING_RING: 2026-07-23, new -- per king-ring square that's
// "weak" (undefended by the enemy, or only defended by their own
// king). Anchored to Kestrel's pawn ratio, like the passed-pawn terms.
// UNSAFE_*_CHECK (2026-07-26): a check that the defender CAN meet still
// costs him something -- it forces the reply, denies him the move he wanted,
// and the checking square stays a square he must keep watching. Our model
// counted only checks landing on squares he cannot touch, which throws that
// away entirely. Worth clearly less than the safe version: roughly a third,
// at this model's unit scale (safe checks are 2-3 units, KING_ATTACKS is 5).
// Queen last, for the same reason her safe check is: a queen check that can
// be answered is often just an invitation to trade her off.
const UNSAFE_KNIGHT_CHECK: (i32, i32) = (1, 0);
const UNSAFE_BISHOP_CHECK: (i32, i32) = (1, 0);
const UNSAFE_ROOK_CHECK: (i32, i32) = (1, 0);
const UNSAFE_QUEEN_CHECK: (i32, i32) = (1, 1);

// QUEENLESS_ATTACK (2026-07-26): attacking without a queen is a different
// proposition -- most mating nets need her. This used to be expressed as a
// GATE (require two attackers instead of one when the defender has no
// queen), which is a cliff: one attacker short and the entire danger term
// vanished. Expressed as units removed instead, it says the same thing
// smoothly, and says it about the ATTACKER's queen, which is what actually
// determines whether the attack can finish.
/// Starting point of the king-danger accumulator, mg/eg. Larger in the
/// endgame, where a bare king is in danger from much less.
const KING_SAFETY_OFFSET: (i32, i32) = (60, 120);
const QUEENLESS_ATTACK: (i32, i32) = (-10, 0);

// SAFETY_PINNED / SAFETY_DISCOVERED (2026-07-26): a defender pinned against
// his own king cannot do its job, and one of our pieces sitting on a line to
// his king is a discovered check waiting to happen. Neither was represented
// at all. Indexed by [piece type involved][sniper: bishop, rook, queen].
//
// Shape reasoned, not transcribed: a pin matters more the more valuable the
// piece stuck (it cannot move at all) and the heavier the sniper behind it.
// A discovered check is worth more than a pin of the same shape -- it is a
// tempo we can take at a moment of our choosing, with check.
const SAFETY_PINNED: [[(i32, i32); 3]; 5] = [
    [(3, 0), (3, 0), (4, 0)],   // pawn pinned
    [(5, 0), (5, 0), (7, 0)],   // knight
    [(5, 0), (5, 0), (7, 0)],   // bishop
    [(6, 0), (7, 0), (9, 0)],   // rook
    [(8, 0), (9, 0), (11, 0)],  // queen
];
const SAFETY_DISCOVERED: [[(i32, i32); 3]; 5] = [
    [(5, 0), (5, 0), (6, 0)],
    [(8, 0), (8, 0), (10, 0)],
    [(8, 0), (8, 0), (10, 0)],
    [(9, 0), (10, 0), (12, 0)],
    [(11, 0), (12, 0), (14, 0)],
];

// STONEWALL (2026-07-26): a named pawn structure the evaluation had no way
// of seeing. White holds c3-d4-e3-f4, Black c6-d5-e6-f5, and the whole point
// of the formation is that all four pawns sit on ONE colour -- dark for
// White, light for Black. Three consequences follow, and they are what these
// weights price:
//
//  * the square in front of the chain (e5 for White, e4 for Black) cannot be
//    challenged by a pawn ever again, so a knight there is permanent;
//  * the bishop travelling on the pawns' own colour is shut in behind them,
//    and unlike the generic "bishop blocked by own pawns" term this one is
//    not a matter of degree -- the structure is a commitment, not a
//    tendency;
//  * the formation itself buys a kingside attack and costs long-term
//    flexibility, which is why it is worth something in the middlegame and
//    rather less once the pieces come off.
//
// The generic terms we already have -- knight outposts, bishop-blocked-by-
// pawns -- catch fragments of this by accident. None of them can recognise
// the structure as a thing with a plan attached.
/// Starting points, not tuned values. Small on purpose: a new term that is
/// wrong is worse than one that is absent, and these can be swept by profile
/// (`scale.pawns` covers blocked_pawns, `scale.pieces` covers space).
/// Peao, cavalo, bispo, torre, dama. Fraccao do valor da peca: nao o valor
/// inteiro, porque nem toda a peca atacada se perde -- ha contra-jogo, xeques
/// intermedios, e a busca resolve o resto. Ponto de partida conservador.
/// What a hanging piece costs, as a fraction of what SEE says the exchange
/// wins, in thousandths and per piece type.
///
/// Was a flat 3/4 written into the code, which is what these defaults are: the
/// change is that the number can now be seen by the tuner and can differ by
/// piece. The old values here were absolute centipawn penalties from the
/// version before SEE priced this term, and were dead.
const HANGING: [(i32, i32); 5] = [(750, 750), (750, 750), (750, 750), (750, 750), (750, 750)];

/// Cavalo, bispo, torre, dama -- por unidade de proximidade (7 - distancia).
/// Ponto de partida pequeno: um termo novo errado e' pior que ausente.
const KING_TROPISM: [(i32, i32); 4] = [(3, 1), (2, 1), (2, 0), (1, 0)];

const SPACE: (i32, i32) = (3, 1);
/// A slider aimed at the enemy king through [1, 2, 3+] blockers.
///
/// Steep on purpose. One piece from opening the line is a live threat the
/// defender must keep answering; three is a shape that may never come to
/// anything. Middlegame only in any real sense -- with the queens off, a
/// bishop pointing at a king is just a bishop, which is what the endgame
/// values say.
const KING_AIM: [(i32, i32); 3] = [(14, 4), (7, 2), (3, 1)];
/// A second slider on the same line. The pattern is worth more than its
/// parts: one blocker removed exposes the king to both at once.
const KING_BATTERY: (i32, i32) = (12, 3);
const BLOCKED_PAWNS: (i32, i32) = (-6, -3);

const STONEWALL: (i32, i32) = (14, -6);
const STONEWALL_OUTPOST: (i32, i32) = (22, 10);
const STONEWALL_BAD_BISHOP: (i32, i32) = (-20, -12);

const WEAK_KING_RING: (i32, i32) = (10, 0);
// KING_FLANK_ATTACKS/DEFENSES: 2026-07-23, new -- "wide flank"
// attack/defense counting, a broader zone than the immediate 3x3 king
// ring (a 4-file band on the king's side of the board x a 5-rank band
// on that king's own half). [0]=squares touched once, [1]=touched by
// 2+ attackers/defenders. Same pawn-anchored scaling as the terms above.
const KING_FLANK_ATTACKS: [(i32, i32); 2] = [(27, -3), (8, 0)];
const KING_FLANK_DEFENSES: [(i32, i32); 2] = [(-17, 0), (-13, 2)];
// UNCASTLED_KING: 2026-07-23, new -- an explicit castling-rights term.
// A rich king-safety system can make a king on its home square score
// badly implicitly (once attackers/open files threaten it), but that is
// reactive. Added because real games showed a genuine, measurable
// pattern: Kestrel was castling late, and outright FAILED to castle at
// all in 3 of 14 games in a sample -- the existing king-safety terms
// (shelter/storm, safe-check, weak-king-ring, king-flank) react to
// actual threats, they are not proactive about the structural risk of
// staying uncastled. These are modest, hand-set values explicitly meant
// to be self-play-validated (or eventually tuner-derived).
// [no_rights_left]: king still on its home square AND has already
// lost ALL castling rights for that side (missed the window
// entirely, the worse case, matches games where Kestrel never
// castled at all). [rights_still_available]: king still on its home
// square but at least one castling right remains (milder -- still
// time to fix it, just a nudge).
const UNCASTLED_KING_NO_RIGHTS: (i32, i32) = (-20, 0);
const UNCASTLED_KING_HAS_RIGHTS: (i32, i32) = (-8, 0);

// King danger units (mg channel of the accumulation above) go through
// this saturating, roughly-quadratic lookup before being added to the
// score, instead of straight in. This is the classical king-safety
// approach: several attackers combining is much more than additively
// dangerous, because
// they can cover each other's escape squares/overload defenders in a
// way a lone piece can't. A flat linear sum lets a single lurking
// queen (65 units) already outweigh real pawn-shelter damage
// regardless of backup. Table is self-derived (identity below the
// ~100-unit mark that one or two ordinary attackers land in -- keeps
// today's already-validated single/double-attacker behaviour
// unchanged -- then grows superlinearly once several attackers
// combine past that, capped so it can never swamp material). Not
// copied from any specific engine's tuned safety table.
/// The king-danger curve, as a function rather than a 128-entry lookup.
///
/// Same shape the table encoded -- one-for-one below the level one or two
/// ordinary attackers reach, then growing quadratically once several
/// threats combine -- but it now takes the WHOLE king-safety total,
/// shelter damage included, so it can no longer be bounded by a table
/// index. Several attackers against an intact shelter and the same
/// attackers against a stripped one are not the same position, and only a
/// curve they both pass through can say so.
///
/// Still one-sided: a total at or below zero means nothing is pointed at
/// that king, and contributes nothing. The ceiling is high rather than
/// absent -- king danger should be able to outweigh a piece, but never run
/// away far enough to make the rest of the evaluation irrelevant.
fn king_danger_curve(v: i32) -> i32 {
    // DIAGNOSTICO (KESTREL_SEM_REI=1): anula a curva, para isolar o que vem daqui.
    static SEM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *SEM.get_or_init(|| std::env::var_os("KESTREL_SEM_REI").is_some()) {
        return 0;
    }
    // A FORMA e' a que todos os motores fortes usam, e nao e' opiniao: uma
    // parte linear mais uma parte quadratica de UM SO' LADO, ambas divididas
    // por constantes grandes.
    //
    //     perigo = v/lin + max(v,0)*v/quad
    //
    // A nossa anterior tinha outra forma inteiramente -- passava v INTEIRO
    // abaixo de um joelho em 100, depois (v-100)^2/40, com tecto rigido em
    // 1200. Isso significa que ate' ao joelho nao havia amortecimento nenhum e
    // a partir dele havia demais, e que o termo podia sozinho valer mais do
    // que uma dama.
    //
    // Os VALORES sao nossos e sao para calibrar (KESTREL_KING_CURVE=lin,quad);
    // o que se adopta e' a forma. Continua de um so' lado por construcao: com
    // v negativo -- rei confortavel -- sobra a parte linear e nada acelera.
    let (lin, quad) = king_curve_params();
    v / lin + v.max(0) * v / quad
}

/// Where the curve stops being one-for-one, and how fast it climbs after.
///
/// These were set when the accumulator held attack units alone. It now also
/// carries the shelter damage, which roughly doubles what a given position
/// feeds in -- so the old knee sits far too low: shelter by itself can reach
/// it, and the attack that should be amplified gets compressed into the
/// straight part instead. Adjustable so the right values can be measured
/// rather than guessed: KESTREL_KING_CURVE=knee,div.
static KING_CURVE: OnceLock<(i32, i32)> = OnceLock::new();
fn king_curve_params() -> (i32, i32) {
    *KING_CURVE.get_or_init(|| {
        std::env::var("KESTREL_KING_CURVE")
            .ok()
            .and_then(|v| {
                let mut it = v.split(',');
                let k = it.next()?.trim().parse().ok()?;
                let d: i32 = it.next()?.trim().parse().ok()?;
                if d > 0 { Some((k, d)) } else { None }
            })
            .unwrap_or((8, 1024))
    })
}

const KING_DANGER_TABLE: [i32; 128] = {
    let mut t = [0i32; 128];
    let mut i = 0;
    while i < 128 {
        let d = i as i32;
        let v = if d <= 100 { d } else { 100 + (d - 100) * (d - 100) / 40 };
        t[i] = if v > 500 { 500 } else { v };
        i += 1;
    }
    t
};

// Pawn shelter/storm: indexado por "offset" (distancia em ranks entre o
// rei e o peao relevante, offset=1 e' o peao imediatamente a frente).
// Shelter (peao proprio): offset 1 intacto = zero custo; cada rank extra
// de avanco e' abrigo perdido sem ganho nenhum em troca. Storm (peao
// inimigo): o inverso -- quanto mais perto do rei, mais perigoso.
const PAWN_SHELTER: [(i32, i32); 4] = [(0, 0), (-10, -2), (-24, -4), (-34, -6)];
const SHELTER_OPEN: (i32, i32) = (-30, -6);
const PAWN_TORNADO: [(i32, i32); 4] = [(-38, -8), (-22, -5), (-10, -2), (0, 0)];

/// Rank offset (sempre positivo, "para a frente" do rei) do peao mais
/// perto do rei nesta bitboard (ja filtrada a uma unica coluna). `None`
/// se nao houver nenhum peao dessa cor "a frente" do rei nessa coluna.
fn shield_pawn_offset(pawns_on_file: Bitboard, king_rank: i32, white: bool) -> Option<i32> {
    let mut bbp = pawns_on_file;
    let mut best: Option<i32> = None;
    while bbp != 0 {
        let s = bbp.trailing_zeros() as Square;
        bbp &= bbp - 1;
        let r = rank_of(s) as i32;
        let off = if white { r - king_rank } else { king_rank - r };
        if off > 0 {
            best = Some(best.map_or(off, |b| b.min(off)));
        }
    }
    best
}

// === Threats ===
// Estrutura standard (indexed por tipo da peca atacada e por
// "defendida pelo inimigo?"). Raciocinio para os valores:
//
// UNDEFENDED = ganho de material em quase todos os casos. O bonus
// aproxima o valor da peca ganha, com desconto por: possivel fuga
// do alvo, tempo consumido, contra-ameaca. Tipico ~50-70% do valor
// nominal.
//
// DEFENDED = recaptura, quase sempre equal ou pequeno ganho. Peao
// defendido a peao defendido vale zero (recaptura pura); dama
// defendida atacada por menor vale eg mais no eg (troca de dama por
// menor + peao passado por baixo pressao).
//
// Ordem interna: [Pawn, Knight, Bishop, Rook, Queen, King].

// THREAT_BY_PAWN: um peao vale 100mg; ganhar 1 peao com um peao vale
// ~70mg (peao inicial pode ser recapturado se defendido depois; se
// pendurado tira ~1 peao inteiro).
const THREAT_BY_PAWN: [[(i32, i32); 6]; 2] = [
    // undefended (peao inimigo pendurado) - ganho quase full material
    [(70, 60), (85, 55), (85, 55), (95, 55), (85, 40), (0, 0)],
    // defended - trocamos peao por peao (equal); vs peca maior, ainda
    // ganho porque a peca tem de sair. Especialmente eg.
    [(0, 5), (25, 15), (25, 15), (30, 20), (25, 10), (0, 0)],
];

// THREAT_BY_KNIGHT: cavalo pode forkar 2 pecas (bonus grande vs torre/
// dama undefended). Cavalo x cavalo = 0 (troca), cavalo x bispo pequena
// pressao. Rook fork por cavalo e' patente 200+cp mas so' considera
// aqui uma ameaca simples.
const THREAT_BY_KNIGHT: [[(i32, i32); 6]; 2] = [
    // undefended
    [(5, 20), (0, 0), (30, 25), (65, 25), (50, 25), (0, 0)],
    // defended
    [(0, 5), (0, 0), (15, 15), (35, 20), (35, 25), (0, 0)],
];

// THREAT_BY_BISHOP: bispo x torre e' padrao "attack on rank" tipico.
// Bispo x bispo = 0 (troca), bispo x dama vale mais eg (dama nao pode
// facilmente sair da diagonal).
const THREAT_BY_BISHOP: [[(i32, i32); 6]; 2] = [
    // undefended
    [(5, 20), (30, 25), (0, 0), (60, 25), (45, 45), (0, 0)],
    // defended
    [(0, 5), (12, 15), (0, 0), (35, 25), (40, 50), (0, 0)],
];

// THREAT_BY_ROOK: torre x cavalo/bispo = pressao clara (torre vale
// mais). Torre x torre = troca equal. Torre x dama = grande bonus.
const THREAT_BY_ROOK: [[(i32, i32); 6]; 2] = [
    // undefended
    [(0, 20), (30, 30), (30, 30), (0, 0), (55, 25), (0, 0)],
    // defended
    [(-5, 5), (5, 12), (10, 8), (0, 0), (40, 55), (0, 0)],
];

// THREAT_BY_QUEEN: dama e' o topo, atacar peca inimiga menor com dama
// e' pressao mas nao tanto (dama nao quer trocar por peca menor).
// Bonus modesto. Se defendida, e' quase mau para nos (dama presa).
const THREAT_BY_QUEEN: [[(i32, i32); 6]; 2] = [
    // undefended
    [(5, 15), (18, 20), (18, 22), (12, 10), (0, 0), (0, 0)],
    // defended
    [(0, 5), (0, 5), (-5, 15), (-5, 5), (0, 0), (0, 0)],
];

// THREAT_BY_KING: rei so' ataca coisas se nao defendidas (senao morre).
// Padrao end-game (rei activo).
const THREAT_BY_KING: [(i32, i32); 6] = [(30, 20), (35, 30), (65, 25), (55, 10), (0, 0), (0, 0)];

// Hit-queen: peca menor a UM movimento de atacar a dama inimiga (a
// partir de casa segura). Valores baixos (a ameaca ainda nao aconteceu).
// Moderado ~25% em 2026-07-22, mesmo motivo do KING_ATTACKER_WEIGHT
// acima (padrao de sacrificios especulativos vs oponentes fortes).
const KNIGHT_HIT_QUEEN: (i32, i32) = (6, 4);   // era (8, 5)
const BISHOP_HIT_QUEEN: (i32, i32) = (10, 9);  // era (14, 12)
const ROOK_HIT_QUEEN: (i32, i32) = (10, 4);    // era (14, 5)
// Peao a UM push de atacar peca inimiga nao-peao (a partir de casa
// safe). Padrao "pawn storm creates threat".
const PUSH_THREAT: (i32, i32) = (12, 15);
// Casas restritas ao adversario (nos double-attackamos, eles nao
// double-defendem, mas eles ainda atacam). Reflecte "controlo do
// espaco". Valor pequeno.
const RESTRICTED_SQUARES: (i32, i32) = (2, 3);

// === Pawn structure ===
// Todas as tabelas indexadas por RANK RELATIVO (rank 0 = nossa 1a
// fileira; rank 7 = 8a fileira / promocao). Slots 0/7 sao 0 porque
// peoes nao existem la.
//
// PAWN_PHALANX: peao adjacente na mesma fileira -- estrutura forte,
// especialmente perto de promocao (peoes avancados juntos suportam
// promocao). Cresce quase quadraticamente com rank.
const PAWN_PHALANX: [(i32, i32); 8] = [
    (0, 0), (5, 0), (10, 5), (18, 12), (35, 30), (65, 100), (110, 175), (0, 0),
];
// DEFENDED_PAWN: peao suportado por outro peao proprio -- estabilidade.
// Menos importante que phalanx (que promove junto), mas relevante.
const DEFENDED_PAWN: [(i32, i32); 8] = [
    (0, 0), (0, 0), (12, 10), (10, 12), (18, 22), (35, 55), (70, 110), (0, 0),
];
// ISOLATED_PAWN/DOUBLED_PAWN: 2026-07-23, upgraded from flat scalars
// to file-edge-distance-indexed `[4]` tables (index 0 = a/h file,
// index 3 = d/e file, `min(file, file^7)`), plus the `_EXPOSED` extra
// penalty (applies when NO enemy pawn anywhere ahead on this exact file
// could ever contest/capture it -- a stronger, more permanently-fixed
// weakness than the base isolated/backward penalty alone). Same
// pawn-anchored scaling as the passed-pawn terms above. Note the
// doubled-pawn eg penalty is much steeper toward the edge files (-53)
// than center (-7) -- counter to naive intuition, but that is what the
// tuning produced, kept as-is rather than re-derived by hand.
const ISOLATED_PAWN: [(i32, i32); 4] = [(-12, 6), (-6, -10), (-15, -6), (-19, -11)];
const DOUBLED_PAWN: [(i32, i32); 4] = [(2, -53), (10, -43), (-10, -25), (-23, -7)];
const ISOLATED_EXPOSED: (i32, i32) = (-12, -6);
const BACKWARD_EXPOSED: (i32, i32) = (-27, -5);
// PASSED_PAWN: 2026-07-23, upgraded from a flat rank-only table to a
// [blocked][controlled][rank] shape -- `blocked` = the push square is
// occupied by any piece, `controlled` = the push square is attacked by
// the enemy. Values expressed on Kestrel's pawn anchor (mg=125,eg=140),
// the same anchoring discipline used for history_prune_mult/
// SCORE_KNOWN_WIN earlier this session. Indexed by rank (0/1/2/7 always
// zero -- only rank 4-7 relative is evaluated, see the gating
// `rel_rank >= 3` check at the call site).
const PASSED_PAWN: [[[(i32, i32); 8]; 2]; 2] = [
    [
        // not blocked, not controlled (best case)
        [(0, 0), (0, 0), (0, 0), (-60, -30), (-21, 26), (42, 142), (221, 232), (0, 0)],
        // not blocked, controlled
        [(0, 0), (0, 0), (0, 0), (-44, -44), (-6, -12), (44, 62), (113, 83), (0, 0)],
    ],
    [
        // blocked, not controlled
        [(0, 0), (0, 0), (0, 0), (-48, -44), (-13, -9), (40, 59), (62, 45), (0, 0)],
        // blocked, controlled (worst case)
        [(0, 0), (0, 0), (0, 0), (-50, -55), (-12, -25), (15, 37), (6, 7), (0, 0)],
    ],
];
// OUR/THEIR_PASSER_PROXIMITY: 2026-07-23, new -- Kestrel previously had
// no king-to-passer distance term at all (a textbook HCE feature: own
// king should shepherd a passer home, enemy king should be kept away).
// Indexed by Chebyshev distance (0-7) from the respective king to the
// passer's PUSH square. Same pawn-anchored scaling as PASSED_PAWN above.
// Only applied where PASSED_PAWN itself applies (rel_rank >= 3).
const OUR_PASSER_PROXIMITY: [(i32, i32); 8] = [
    (127, 116), (165, 83), (62, 76), (-13, 62), (-4, 34), (6, 21), (35, 9), (6, 18),
];
const THEIR_PASSER_PROXIMITY: [(i32, i32); 8] = [
    (-125, 18), (2, 1), (50, 1), (40, 26), (21, 62), (27, 78), (37, 82), (35, 68),
];
// PASSER_DEFENDED_PUSH: bonus when the passer's push square is
// defended by one of our own pieces. PASSER_SLIDER_BEHIND: penalty
// when an enemy rook/queen sits behind the passer on its file (the
// classic "rook behind the passed pawn" idea, applied to the
// opponent). Both new this session, same pawn-anchored scaling.
const PASSER_DEFENDED_PUSH: [(i32, i32); 8] = [
    (0, 0), (0, 0), (0, 0), (17, 5), (19, 18), (67, 32), (115, 101), (0, 0),
];
const PASSER_SLIDER_BEHIND: [(i32, i32); 8] = [
    (0, 0), (0, 0), (0, 0), (-52, -13), (-48, -28), (-50, -45), (15, -96), (0, 0),
];
// BACKWARD_PAWN: no pawn on an adjacent file can ever support it (none
// sit level with or behind it) AND its advance square is controlled by
// an enemy pawn -- stuck, can't safely push, can't be defended by a
// pawn. Mild penalty (structural, not material): a classic term, a
// smaller effect than isolation.
const BACKWARD_PAWN: (i32, i32) = (-6, -10);
// CANDIDATE_PASSER: 2026-07-23, upgraded from a flat scalar to a
// [defended][rank] shape -- `defended` = this pawn has at least as many
// own-pawn defenders as enemy-pawn attackers on its square. Same
// pawn-anchored scaling as PASSED_PAWN.
const CANDIDATE_PASSER: [[(i32, i32); 8]; 2] = [
    // not defended
    [(0, 0), (-44, -12), (-15, -14), (-6, 1), (29, 22), (65, 87), (0, 0), (0, 0)],
    // defended
    [(0, 0), (-31, -11), (-21, 8), (-12, 25), (13, 38), (73, 103), (0, 0), (0, 0)],
];
// BAD_BISHOP -> BISHOP_PAWNS: 2026-07-23, upgraded from a flat
// per-pawn linear multiplier to a graduated table, indexed by
// `min(own-color-pawn-count, 6)`. Richer than the old version: a bishop
// with ZERO same-color pawns gets an actual BONUS (a genuinely good
// bishop, not just "no penalty"), not something a flat per-pawn
// multiplier could ever express. Same pawn-anchored scaling as above.
const BISHOP_PAWNS: [(i32, i32); 7] = [
    (25, 0), (12, 11), (6, 8), (0, 3), (-8, -2), (-13, -9), (-21, -17),
];
// Endgame scale factors (see scale_endgame/endgame_scale_factor):
// out of SCALE_NORMAL=128. Were plain hardcoded numbers in
// endgame_scale_factor's return statements until this commit -- pulled
// into Weights (single scalars, not ScorePair -- they scale a whole
// already-tapered eval, not an mg/eg component) so they're swappable
// the same way every other eval constant is.
const SCALE_OCB_BISHOPS_ONLY: i32 = 64;
const SCALE_OCB_ONE_ROOK: i32 = 96;
const SCALE_OCB_ONE_KNIGHT: i32 = 106;
const SCALE_FALLBACK_BASE: i32 = 96;
const SCALE_FALLBACK_PER_PAWN: i32 = 8;
// Complexity adjustment (see complexity_adjustment()/evaluate()).
// Shrinks the eval toward zero
// in positions that are "simple" by these signals -- a negative bias
// means most positions get a small penalty by default, only genuinely
// complex ones (many pawns, both flanks open, pure pawn endgame) offset
// it. Clamped so it can never flip which side is better, only reduce
// the margin (see complexity_adjustment's sign-preserving clamp).
const COMPLEXITY_TOTAL_PAWNS: i32 = 8;
const COMPLEXITY_PAWN_FLANKS: i32 = 82;
const COMPLEXITY_PAWN_ENDGAME: i32 = 76;
const COMPLEXITY_ADJUSTMENT: i32 = -157;

/// Runtime-adjustable copy of every constant `positional_terms()` uses
/// (mobility/king-safety/threats/pawn-structure -- NOT material/PST,
/// those stay compile-time consts read via the incremental board
/// accumulators in board.rs, a performance-critical path this struct
/// deliberately doesn't touch). `Default` just copies the existing
/// consts field-by-field -- never retyped by hand -- so building this
/// struct cannot introduce a transcription error: `default_weights()`
/// is byte-for-byte what `positional_terms()` already computed before
/// this struct existed. This is the prerequisite for our logistic
/// tuner: the tuner builds its own `Weights`, nudges
/// fields, and calls `positional_terms(board, &candidate)` to score
/// datasets -- the live search keeps using `default_weights()`
/// unchanged until a tuning run's result is deliberately copied back
/// into the consts above.
#[derive(Clone)]
pub struct Weights {
    pub bishop_pair: (i32, i32),
    pub long_diag_bishop: (i32, i32),
    pub minor_behind_pawn: (i32, i32),
    pub knight_outpost: (i32, i32),
    pub rook_open: [(i32, i32); 2],
    pub rook_on_seventh: (i32, i32),
    pub tempo: (i32, i32),
    pub mobility_knight: [(i32, i32); 28],
    pub mobility_bishop: [(i32, i32); 28],
    pub mobility_rook: [(i32, i32); 28],
    pub mobility_queen: [(i32, i32); 28],
    pub king_attacker_weight: [(i32, i32); 4],
    pub king_attacks: (i32, i32),
    pub safe_knight_check: (i32, i32),
    pub safe_bishop_check: (i32, i32),
    pub safe_rook_check: (i32, i32),
    pub safe_queen_check: (i32, i32),
    pub unsafe_knight_check: (i32, i32),
    pub unsafe_bishop_check: (i32, i32),
    pub unsafe_rook_check: (i32, i32),
    pub unsafe_queen_check: (i32, i32),
    pub queenless_attack: (i32, i32),
    /// Added to the king-danger accumulator before the curve, always.
    ///
    /// Not decoration. The curve is linear below its knee and quadratic
    /// above, so where the accumulator SITS decides whether an attack gets
    /// amplified or merely added. With no offset an ordinary attack never
    /// reaches the knee, the term stays in its linear stretch, and king
    /// safety ends up driving this evaluation less than half as much as it
    /// drives a stronger one -- measured over 400 positions, a share of 0.19
    /// against 0.48. The constant moves the whole term into the regime where
    /// the curve does its work.
    pub king_safety_offset: (i32, i32),
    pub safety_pinned: [[(i32, i32); 3]; 5],
    pub safety_discovered: [[(i32, i32); 3]; 5],
    pub king_danger_table: [i32; 128],
    pub pawn_shelter: [(i32, i32); 4],
    pub shelter_open: (i32, i32),
    pub pawn_tornado: [(i32, i32); 4],
    /// Ver TORNADO_LOCKED.
    pub tornado_locked: [(i32, i32); 4],
    /// Ver PASSER_OUTSIDE.
    pub passer_outside: (i32, i32),
    pub threat_by_pawn: [[(i32, i32); 6]; 2],
    pub threat_by_knight: [[(i32, i32); 6]; 2],
    pub threat_by_bishop: [[(i32, i32); 6]; 2],
    pub threat_by_rook: [[(i32, i32); 6]; 2],
    pub threat_by_queen: [[(i32, i32); 6]; 2],
    pub threat_by_king: [(i32, i32); 6],
    pub knight_hit_queen: (i32, i32),
    pub bishop_hit_queen: (i32, i32),
    pub rook_hit_queen: (i32, i32),
    pub push_threat: (i32, i32),
    pub restricted_squares: (i32, i32),
    pub pawn_phalanx: [(i32, i32); 8],
    pub defended_pawn: [(i32, i32); 8],
    pub isolated_pawn: [(i32, i32); 4],
    pub doubled_pawn: [(i32, i32); 4],
    pub isolated_exposed: (i32, i32),
    pub backward_exposed: (i32, i32),
    pub passed_pawn: [[[(i32, i32); 8]; 2]; 2],
    pub our_passer_proximity: [(i32, i32); 8],
    pub their_passer_proximity: [(i32, i32); 8],
    pub passer_defended_push: [(i32, i32); 8],
    pub passer_slider_behind: [(i32, i32); 8],
    pub backward_pawn: (i32, i32),
    pub candidate_passer: [[(i32, i32); 8]; 2],
    pub bishop_pawns: [(i32, i32); 7],
    /// [nossa][dele] -- ver IMBALANCE.
    pub imbalance: [[(i32, i32); 5]; 5],
    pub weak_king_ring: (i32, i32),
    pub king_flank_attacks: [(i32, i32); 2],
    pub king_flank_defenses: [(i32, i32); 2],
    pub uncastled_king_no_rights: (i32, i32),
    pub uncastled_king_has_rights: (i32, i32),
    pub scale_ocb_bishops_only: i32,
    pub scale_ocb_one_rook: i32,
    pub scale_ocb_one_knight: i32,
    pub scale_fallback_base: i32,
    pub scale_fallback_per_pawn: i32,
    pub complexity_total_pawns: i32,
    pub complexity_pawn_flanks: i32,
    pub complexity_pawn_endgame: i32,
    pub complexity_adjustment: i32,
    /// Safe squares in our own half, behind our pawns, that the opponent does
    /// not attack. Room to manoeuvre.
    ///
    /// This is the term that decides closed positions, and neither we nor the
    /// reference had it. Measured on 214 real mistakes, we err TWICE as often
    /// in locked structures as anywhere else -- and a locked board is exactly
    /// where there is no tactic to find and the game is decided by who has
    /// somewhere to put their pieces. Material and mobility have almost
    /// nothing to say there: mobility counts squares a piece can reach NOW,
    /// which in a blocked position is a small number for both sides and says
    /// nothing about who is better placed to improve.
    /// Per piece, scaled by how close it stands to the enemy king.
    ///
    /// Every other king term counts squares ATTACKED. None of them notices a
    /// piece that is two squares from the enemy king and attacking nothing
    /// yet -- which is exactly the piece about to decide the game. Mobility
    /// makes this worse, not better: a rook with eighteen squares on a dead
    /// flank outscores a rook with six pointed at the king.
    ///
    /// This is the term that says WHERE THE GAME IS. A hand-written
    /// evaluation has to state that explicitly; it is one of the things a
    /// neural network learns on its own, and one of the reasons this kind of
    /// evaluation reads as "static" even when most of its terms are dynamic.
    ///
    /// Knights weigh most: they are short-range, so distance to the king is
    /// nearly all of their attacking value. Queens weigh least per unit of
    /// distance because they already reach far -- their closeness matters, but
    /// the other king terms already price it.
    /// Penalty for OUR pieces that are attacked and cannot be saved, when it
    /// is the opponent's turn. Indexed by piece type.
    ///
    /// Every other threat term prices ATTACKING. None of them prices being
    /// about to die. Found with the heatmap tool: dropping a bishop on d6 --
    /// where an enemy bishop takes it next move -- scored +23 and came out as
    /// the single BEST square on the board, because the bishop attacks a rook
    /// and a bishop from there and nothing charges it for the fact that it
    /// will not survive to collect.
    ///
    /// This matters far beyond one square. The static evaluation feeds every
    /// whole-node pruning decision -- RFP, null move, razoring, futility --
    /// and each of them asks "is my static score already good enough to cut
    /// without searching?". An evaluation that is optimistic exactly when a
    /// piece hangs makes those cuts on a lie, which is a candidate explanation
    /// for null move firing 4.9x more often here than in a reference engine
    /// and failing 64% of the time against its 28%.
    pub hanging: [(i32, i32); 5],
    pub king_tropism: [(i32, i32); 4],
    pub space: (i32, i32),
    /// A slider sharing a line with the enemy king, by how many blockers still
    /// stand in the way: [1, 2, 3+]. One square from opening is a threat.
    pub king_aim: [(i32, i32); 3],
    /// A second slider on the same line -- the battery, worth more than the
    /// two pieces counted apart.
    pub king_battery: (i32, i32),
    /// Per own pawn standing directly in front of an enemy pawn.
    ///
    /// A board with six blocked pawns plays nothing like an open one, and
    /// without this the evaluation cannot tell them apart. Counting them lets
    /// the space bonus matter more precisely when the position is actually
    /// closed.
    pub blocked_pawns: (i32, i32),
    pub stonewall: (i32, i32),
    pub stonewall_outpost: (i32, i32),
    pub stonewall_bad_bishop: (i32, i32),
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            bishop_pair: BISHOP_PAIR,
            long_diag_bishop: LONG_DIAG_BISHOP,
            minor_behind_pawn: MINOR_BEHIND_PAWN,
            knight_outpost: KNIGHT_OUTPOST,
            rook_open: ROOK_OPEN,
            rook_on_seventh: ROOK_ON_SEVENTH,
            tempo: TEMPO,
            mobility_knight: MOBILITY_KNIGHT,
            mobility_bishop: MOBILITY_BISHOP,
            mobility_rook: MOBILITY_ROOK,
            mobility_queen: MOBILITY_QUEEN,
            king_attacker_weight: KING_ATTACKER_WEIGHT,
            king_attacks: KING_ATTACKS,
            safe_knight_check: SAFE_KNIGHT_CHECK,
            safe_bishop_check: SAFE_BISHOP_CHECK,
            safe_rook_check: SAFE_ROOK_CHECK,
            safe_queen_check: SAFE_QUEEN_CHECK,
            unsafe_knight_check: UNSAFE_KNIGHT_CHECK,
            unsafe_bishop_check: UNSAFE_BISHOP_CHECK,
            unsafe_rook_check: UNSAFE_ROOK_CHECK,
            unsafe_queen_check: UNSAFE_QUEEN_CHECK,
            queenless_attack: QUEENLESS_ATTACK,
            king_safety_offset: KING_SAFETY_OFFSET,
            safety_pinned: SAFETY_PINNED,
            safety_discovered: SAFETY_DISCOVERED,
            king_danger_table: KING_DANGER_TABLE,
            pawn_shelter: PAWN_SHELTER,
            shelter_open: SHELTER_OPEN,
            pawn_tornado: PAWN_TORNADO,
            tornado_locked: TORNADO_LOCKED,
            passer_outside: PASSER_OUTSIDE,
            threat_by_pawn: THREAT_BY_PAWN,
            threat_by_knight: THREAT_BY_KNIGHT,
            threat_by_bishop: THREAT_BY_BISHOP,
            threat_by_rook: THREAT_BY_ROOK,
            threat_by_queen: THREAT_BY_QUEEN,
            threat_by_king: THREAT_BY_KING,
            knight_hit_queen: KNIGHT_HIT_QUEEN,
            bishop_hit_queen: BISHOP_HIT_QUEEN,
            rook_hit_queen: ROOK_HIT_QUEEN,
            push_threat: PUSH_THREAT,
            restricted_squares: RESTRICTED_SQUARES,
            pawn_phalanx: PAWN_PHALANX,
            defended_pawn: DEFENDED_PAWN,
            isolated_pawn: ISOLATED_PAWN,
            doubled_pawn: DOUBLED_PAWN,
            isolated_exposed: ISOLATED_EXPOSED,
            backward_exposed: BACKWARD_EXPOSED,
            passed_pawn: PASSED_PAWN,
            our_passer_proximity: OUR_PASSER_PROXIMITY,
            their_passer_proximity: THEIR_PASSER_PROXIMITY,
            passer_defended_push: PASSER_DEFENDED_PUSH,
            passer_slider_behind: PASSER_SLIDER_BEHIND,
            backward_pawn: BACKWARD_PAWN,
            candidate_passer: CANDIDATE_PASSER,
            bishop_pawns: BISHOP_PAWNS,
            imbalance: IMBALANCE,
            weak_king_ring: WEAK_KING_RING,
            king_flank_attacks: KING_FLANK_ATTACKS,
            king_flank_defenses: KING_FLANK_DEFENSES,
            uncastled_king_no_rights: UNCASTLED_KING_NO_RIGHTS,
            uncastled_king_has_rights: UNCASTLED_KING_HAS_RIGHTS,
            scale_ocb_bishops_only: SCALE_OCB_BISHOPS_ONLY,
            scale_ocb_one_rook: SCALE_OCB_ONE_ROOK,
            scale_ocb_one_knight: SCALE_OCB_ONE_KNIGHT,
            scale_fallback_base: SCALE_FALLBACK_BASE,
            scale_fallback_per_pawn: SCALE_FALLBACK_PER_PAWN,
            complexity_total_pawns: COMPLEXITY_TOTAL_PAWNS,
            complexity_pawn_flanks: COMPLEXITY_PAWN_FLANKS,
            complexity_pawn_endgame: COMPLEXITY_PAWN_ENDGAME,
            complexity_adjustment: COMPLEXITY_ADJUSTMENT,
            hanging: HANGING,
            king_tropism: KING_TROPISM,
            space: SPACE,
            king_aim: KING_AIM,
            king_battery: KING_BATTERY,
            blocked_pawns: BLOCKED_PAWNS,
            stonewall: STONEWALL,
            stonewall_outpost: STONEWALL_OUTPOST,
            stonewall_bad_bishop: STONEWALL_BAD_BISHOP,
        }
    }
}

/// Pesos de eval calibrados pelo tuner PRÓPRIO do Kestrel (regressao
/// logistica, gradient descent) sobre 566098 posições de self-play
/// geradas pelo binário +46.6 desta sessão (dataset_v46), lr=1000, 3000
/// iterações, erro 0.075604 -> 0.074356. Validado por SPRT: +36 Elo
/// (55.2% em 1243 jogos, LOS ~100%) sobre o tuning anterior. Substituem os defaults.
/// Validado por A/B: +2.3% (52.3%) vs os defaults a nós
/// fixos, 300 jogos -- ganho real, e sem custo de NPS (só muda
/// valores). É a resposta à direcção do utilizador: calibrar os
/// valores para a arquitectura do Kestrel nos nossos próprios dados.
/// Aplicado via `from_vec` (669 escalares na ordem exacta do
/// `to_vec`); material/PST (consts separadas) e king_danger_table
/// (derivada) não fazem parte deste vector e ficam como estavam.
#[rustfmt::skip]
const TUNED_V46: [i32; 669] = [28,41,11,2,0,-4,21,14,15,-6,15,-5,-19,17,20,10,-40,-32,-20,-10,-4,-7,3,3,11,4,12,9,14,4,18,9,24,9,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,-36,-32,-20,-14,-4,-8,1,-6,9,0,5,4,11,5,12,6,10,8,12,9,26,19,28,17,32,24,34,24,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,-40,-31,-14,-19,-11,-10,-5,-4,-6,0,0,0,3,3,2,6,5,10,11,14,8,15,12,20,11,19,15,22,15,19,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,-25,-25,-22,-20,-14,-14,-3,-7,-1,-1,1,2,0,5,7,6,8,10,10,9,14,13,14,11,14,14,21,20,19,18,22,18,23,20,22,20,22,21,23,22,23,22,23,23,24,22,24,22,24,22,24,22,24,22,24,22,15,-2,13,-2,26,-4,48,-4,5,0,2,0,2,2,3,1,1,2,-5,-14,-19,-15,-25,-20,-40,-16,-34,2,-33,4,-19,-5,-2,-2,11,-7,50,50,86,54,86,54,95,55,84,41,0,0,8,3,32,15,30,15,30,20,26,10,0,0,10,20,0,-1,30,23,66,25,50,24,0,0,-13,10,-1,0,23,14,36,20,35,25,0,0,7,18,33,25,2,0,60,25,45,45,0,0,-1,0,19,8,-3,-1,34,25,38,49,0,0,1,25,29,28,28,28,1,0,54,24,0,0,-10,5,-8,6,7,4,-3,-1,41,55,0,0,13,15,20,18,21,20,15,10,-1,-1,0,0,-9,3,-10,0,-11,9,-13,1,0,-1,0,0,30,27,32,26,64,24,53,9,0,0,0,0,12,-2,22,8,7,-8,10,5,4,4,0,0,0,3,6,-2,18,6,33,29,65,99,110,175,0,0,0,0,0,0,1,0,3,-1,6,7,29,48,70,109,0,0,-1,-12,-24,-18,-11,-14,-20,-13,-10,-55,3,-43,-16,-27,-34,-6,-11,-7,-17,-4,0,0,0,0,0,0,-61,-34,-26,21,28,124,217,225,0,0,0,0,0,0,0,0,-43,-47,-11,-17,29,43,107,76,0,0,0,0,0,0,0,0,-50,-47,-19,-15,34,52,57,39,0,0,0,0,0,0,0,0,-49,-55,-15,-25,14,34,4,5,0,0,125,112,155,73,49,61,-16,50,-10,20,-5,6,17,-12,-2,9,-125,12,-3,-7,29,-11,20,8,11,42,12,59,30,68,34,66,0,0,0,0,0,0,15,-1,13,14,57,18,113,97,0,0,0,0,0,0,0,0,-52,-13,-48,-29,-54,-51,12,-99,0,0,7,2,0,0,-44,-12,-15,-14,-7,1,26,20,63,85,0,0,0,0,0,0,-17,-10,-20,-4,-11,15,1,25,73,103,0,0,0,0,12,-15,7,-6,0,3,-4,-9,-8,-6,-8,-10,-16,-15,-5,-3,0,2,3,2,-8,-2,0,-7,-26,-2,-8,6,64,96,106,96,8,8,82,76,-157];

static DEFAULT_WEIGHTS: OnceLock<Weights> = OnceLock::new();
/// A/B testing hook for a tuning run's output, same reversible pattern
/// as `KESTREL_EVAL_MODE` above: with the env var unset (every
/// deployment, including the live bot, unless someone deliberately
/// sets it) this is byte-for-byte `Weights::default()`. Lets a
/// candidate weight set from `kestrel tune` be exercised by the real
/// position suite / a real game before ever touching the compiled-in
/// consts -- nothing is "deployed" by running the tuner, only by a
/// deliberate later commit copying values back into the consts.

/// Per-family scaling of the evaluation weights.
///
/// A single global factor cannot answer the question that matters. It says
/// "better or worse" and nothing else -- and when the parameter set is a
/// mixture of inherited and locally tuned values, as ours is, it does not even
/// say that reliably: scaling a value that was already on our scale is noise
/// injected on purpose.
///
/// A factor per family asks a sharper question: not "does the engine play
/// better" but "WHERE is it mispriced". If mobility wants +20% and king safety
/// wants nothing, that is a finding about the evaluation, not a number to
/// tune. Factors are in per-mille so they can arrive over UCI as integers:
/// 1000 is unchanged, 1200 is +20%.
/// Family scale, per phase bucket: `[bucket * 6 + family]`.
///
/// One row per bucket because the correction the profile asks for is not a
/// single number -- our evaluation runs at 0.78 of a strong reference's with a
/// full board and 1.57 of it once the pieces are gone, so the error changes
/// SIGN in the middle and no global factor can flatten it.
///
/// Every bucket starts at the values that were in force before buckets
/// existed (king 1100, threats 1150 from the V3 profile, the rest neutral), so
/// turning the mechanism on changes nothing until a profile says otherwise.
static FAMILY_SCALE: [std::sync::atomic::AtomicI32; 6 * NUM_BUCKETS] = {
    use std::sync::atomic::AtomicI32;
    [
        AtomicI32::new(1000), AtomicI32::new(1100), AtomicI32::new(1150), AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1100), AtomicI32::new(1150), AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1100), AtomicI32::new(1150), AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1100), AtomicI32::new(1150), AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1100), AtomicI32::new(1150), AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1100), AtomicI32::new(1150), AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1100), AtomicI32::new(1150), AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
        AtomicI32::new(1000), AtomicI32::new(1100), AtomicI32::new(1150), AtomicI32::new(1000), AtomicI32::new(1000), AtomicI32::new(1000),
    ]
};

const FAMILIES: [&str; 6] = ["mobility", "king", "threats", "pawns", "pieces", "tempo"];

/// Which family a weight field belongs to, or None if it is not scaled
/// (scaling factors, complexity terms and anything else whose units are not
/// centipawns).

fn family_of(name: &str) -> &'static str {
    field_family(name).unwrap_or("")
}

fn field_family(name: &str) -> Option<&'static str> {
    match name {
            "backward_exposed" => Some("pawns"),
            "backward_pawn" => Some("pawns"),
            "bishop_hit_queen" => Some("threats"),
            "bishop_pair" => Some("pieces"),
            "bishop_pawns" => Some("pawns"),
            "imbalance" => Some("pieces"),
            "candidate_passer" => Some("pawns"),
            "defended_pawn" => Some("pawns"),
            "doubled_pawn" => Some("pawns"),
            "isolated_exposed" => Some("pawns"),
            "isolated_pawn" => Some("pawns"),
            "king_attacker_weight" => Some("king"),
            "king_attacks" => Some("king"),
            "king_flank_attacks" => Some("king"),
            "king_flank_defenses" => Some("king"),
            "knight_hit_queen" => Some("threats"),
            "knight_outpost" => Some("pieces"),
            "long_diag_bishop" => Some("pieces"),
            "minor_behind_pawn" => Some("pieces"),
            "mobility_bishop" => Some("mobility"),
            "mobility_knight" => Some("mobility"),
            "mobility_queen" => Some("mobility"),
            "mobility_rook" => Some("mobility"),
            "our_passer_proximity" => Some("pawns"),
            "passed_pawn" => Some("pawns"),
            "passer_defended_push" => Some("pawns"),
            "passer_slider_behind" => Some("pawns"),
            "pawn_phalanx" => Some("pawns"),
            "pawn_shelter" => Some("king"),
            "pawn_tornado" => Some("king"),
            "tornado_locked" => Some("king"),
            "passer_outside" => Some("pawns"),
            "push_threat" => Some("threats"),
            "restricted_squares" => Some("threats"),
            "rook_hit_queen" => Some("threats"),
            "rook_on_seventh" => Some("pieces"),
            "rook_open" => Some("pieces"),
            "safe_bishop_check" => Some("king"),
            "safe_knight_check" => Some("king"),
            "safe_queen_check" => Some("king"),
            "safe_rook_check" => Some("king"),
            "shelter_open" => Some("king"),
            "tempo" => Some("tempo"),
            "their_passer_proximity" => Some("pawns"),
            "threat_by_bishop" => Some("threats"),
            "threat_by_king" => Some("threats"),
            "threat_by_knight" => Some("threats"),
            "threat_by_pawn" => Some("threats"),
            "threat_by_queen" => Some("threats"),
            "threat_by_rook" => Some("threats"),
            "uncastled_king_has_rights" => Some("king"),
            "uncastled_king_no_rights" => Some("king"),
            "weak_king_ring" => Some("king"),
            _ => None,
    }
}

/// Set one family's factor, in per-mille. Returns false for an unknown family.
/// What a family scale is BEFORE any profile touches it.
///
/// Not 1000. The V3 profile is compiled into these defaults, so writing 1000
/// into a profile does not neutralise a family -- it overwrites a calibrated
/// value with a generic one. Omitting the key is what preserves V3.
pub fn family_scale_default(name: &str) -> Option<i32> {
    // Index 0..6 is bucket 0, which every bucket starts equal to.
    FAMILIES.iter().position(|&f| f == name).map(|i| FAMILY_DEFAULTS[i])
}

/// The compiled-in family scales, in FAMILIES order. Kept as plain data so
/// tooling can print them without reading the atomics, which a profile may
/// already have changed.
pub const FAMILY_DEFAULTS: [i32; 6] = [1000, 1100, 1150, 1000, 1000, 1000];

/// Same, for PSQT amplitudes, in PSQT_NAMES order.
pub const PSQT_DEFAULTS: [i32; 6] = [1000, 1000, 1000, 1000, 1000, 1350];

pub fn psqt_scale_default(name: &str) -> Option<i32> {
    PSQT_NAMES.iter().position(|&p| p == name).map(|i| PSQT_DEFAULTS[i])
}

pub fn set_family_scale(name: &str, bucket: Option<usize>, per_mille: i32) -> bool {
    invalida_cache_peoes();
    let i = match FAMILIES.iter().position(|&f| f == name) {
        Some(i) => i,
        None => return false,
    };
    match bucket {
        // A profile written before buckets existed means the whole game.
        None => {
            for b in 0..NUM_BUCKETS {
                FAMILY_SCALE[b * 6 + i].store(per_mille, std::sync::atomic::Ordering::Relaxed);
            }
        }
        Some(b) if b < NUM_BUCKETS => {
            FAMILY_SCALE[b * 6 + i].store(per_mille, std::sync::atomic::Ordering::Relaxed);
        }
        Some(_) => return false,
    }
    FAMILY_BUCKETS_DIFFER.store(
        (0..NUM_BUCKETS).any(|b| {
            (0..6).any(|k| {
                FAMILY_SCALE[b * 6 + k].load(std::sync::atomic::Ordering::Relaxed)
                    != FAMILY_SCALE[k].load(std::sync::atomic::Ordering::Relaxed)
            })
        }),
        std::sync::atomic::Ordering::Relaxed,
    );
    true
}

/// True when any family has been rescaled, so the common path can skip the
/// work entirely.
/// Whether any family is scaled at all. Compared against 1000, not against
/// the defaults: king and threats now ship at 1100 and 1150, so the scaled
/// path IS the normal path. Building the scaled weights happens once behind a
/// OnceLock; what this guards is only whether to take that path at all.
pub fn family_scaling_active() -> bool {
    FAMILY_SCALE
        .iter()
        .any(|a| a.load(std::sync::atomic::Ordering::Relaxed) != 1000)
}

fn family_factor(name: &str, bucket: usize) -> i32 {
    match field_family(name).and_then(|f| FAMILIES.iter().position(|&x| x == f)) {
        Some(i) => FAMILY_SCALE[bucket * 6 + i].load(std::sync::atomic::Ordering::Relaxed),
        None => 1000,
    }
}

impl Weights {
    /// A copy with every king-safety input silenced -- the tunable block in
    /// `to_vec` AND the fields deliberately kept out of it.
    ///
    /// This exists so the `eval` command can report the king's contribution
    /// exactly, by difference. That method is only honest if zeroing really
    /// removes the whole block: leave one input behind and the leftover feeds
    /// the danger curve, the difference stops being the king's contribution,
    /// and the number quietly lies. It did exactly that the moment king
    /// safety grew fields outside `to_vec`.
    ///
    /// Note the curve itself needs no special handling: with every input at
    /// zero the accumulated total is zero, and the curve maps zero to zero.
    /// Non-linearity is not what makes decomposition-by-difference fragile --
    /// incomplete zeroing is.
    pub fn with_king_silenced(&self) -> Weights {
        let mut v = self.to_vec();
        let hi = KING_RANGE.1.min(v.len());
        for x in v[KING_RANGE.0..hi].iter_mut() {
            *x = 0;
        }
        let mut w = self.from_vec(&v);
        w.unsafe_knight_check = (0, 0);
        w.unsafe_bishop_check = (0, 0);
        w.unsafe_rook_check = (0, 0);
        w.unsafe_queen_check = (0, 0);
        w.queenless_attack = (0, 0);
        w.king_safety_offset = (0, 0);
        w.safety_pinned = [[(0, 0); 3]; 5];
        w.safety_discovered = [[(0, 0); 3]; 5];
        w
    }
}

/// How many phase regimes the evaluation keeps separate weights for.
///
/// The evaluation used to interpolate every term linearly between a midgame
/// and an endgame value. That is two points and a straight line, and it
/// assumes everything worth knowing about a position changes at a constant
/// rate as pieces come off. Measurement says otherwise: scored against a
/// stronger hand-crafted evaluation over real games, our bias swings 54cp
/// depending on how much material is left, non-monotonically, at six sigma.
/// A straight line cannot follow a swing, whatever its slope.
///
/// Eight regimes, chosen by the same phase count the taper uses, each free
/// to hold its own opinion. Bucket 0 is the opening, bucket 7 the bare
/// endgame. The mg/eg pair stays inside each bucket -- the taper is still
/// doing useful work within a regime, it just no longer has to stretch
/// across the whole game.
/// Four, not eight, and indexed by PAWN COUNT rather than material phase.
///
/// Two separate corrections to what was here.
///
/// The index: every term in this evaluation is already a (midgame, endgame)
/// pair interpolated by material phase, so bucketing by material phase again
/// splits the data along an axis the evaluation has already accounted for --
/// the same information, with a fraction of the samples behind each copy of
/// it. That is a fair explanation for the earlier eight-bucket attempt
/// measuring worse rather than better. Pawn count is close to orthogonal to
/// it: a middlegame with fourteen pawns is a closed position where knights
/// beat bishops and a king is safe behind a wall, and one with six is an open
/// one where neither is true, at identical material phase.
///
/// The count: measured on 400k positions from this engine's own games, the
/// pawn-count distribution is far from uniform -- 16 pawns is 2.1% of
/// positions and 12 is 9.8%. Equal-width slices would leave the end buckets
/// with a few thousand positions and dozens of weights, which is not
/// calibration but noise-fitting, and a search this wide finds and exploits
/// exactly that kind of mirage. These boundaries are set so each bucket holds
/// a comparable share: 23% / 28% / 29% / 19%.
pub const NUM_BUCKETS: usize = 8;

/// Particao alternativa POR PECAS (KESTREL_BUCKET_PECAS=1), a que os
/// motores com NNUE usam: (npecas-1)/4, oito baldes. Existe para a comparacao ser feita
/// com o MESMO binario e os MESMOS dados, mudando so' a particao.
fn bucket_por_pecas(board: &Board) -> usize {
    let mut n = 0u32;
    for c in 0..2 {
        for pt in 0..6 {
            n += board.pieces[c][pt].count_ones();
        }
    }
    (((n.max(1) - 1) / 4) as usize).min(NUM_BUCKETS - 1)
}

fn particao_por_pecas() -> bool {
    static P: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *P.get_or_init(|| std::env::var_os("KESTREL_BUCKET_PECAS").is_some())
}

pub fn bucket_of(board: &Board) -> usize {
    if particao_por_pecas() {
        return bucket_por_pecas(board);
    }
    let pawns = crate::bitboard::count(
        board.pieces[Color::White.idx()][PieceType::Pawn.idx()]
            | board.pieces[Color::Black.idx()][PieceType::Pawn.idx()],
    );
    // Eight buckets, boundaries from the V4 profile. Measured on the pawn
    // distribution of our own games so no bucket is left fitting noise: the
    // slices come out 14/16/14/8/19/10/8/12%, uneven because pawn count is
    // discrete, and with 12.5M positions available the smallest still holds
    // over a thousand positions per weight.
    match pawns {
        0..=3 => 0,
        4..=6 => 1,
        7..=8 => 2,
        9 => 3,
        10..=11 => 4,
        12 => 5,
        13 => 6,
        _ => 7,
    }
}

static BUCKET_WEIGHTS: OnceLock<Vec<Weights>> = OnceLock::new();

/// The weight set for this position's phase.
///
/// Every bucket starts as a copy of the single set the engine already used,
/// so switching this on changes nothing at all -- same moves, same
/// evaluations, same node counts -- until trained weights are supplied.
/// That is deliberate: a structural change and a strength change should
/// never arrive together, or there is no telling which one did what.
///
/// KESTREL_BUCKET_WEIGHTS points at a file of NUM_BUCKETS x to_vec() values,
/// comma separated, which is what a bucketed tuner produces.
/// Buckets with per-family factors applied. Built lazily and only when some
/// factor was actually set, so `warmup()` -- which evaluates a position and
/// would otherwise seal this before any setoption arrived -- never reaches it.
static SCALED_BUCKETS: OnceLock<Vec<Weights>> = OnceLock::new();

/// Os multiplicadores, para quem os quiser ver em vez de os adivinhar.
pub fn escala_familia(nome: &str, bucket: usize) -> i32 {
    match FAMILIES.iter().position(|&f| f == nome) {
        Some(k) => FAMILY_SCALE[bucket * 6 + k].load(std::sync::atomic::Ordering::Relaxed),
        None => 1000,
    }
}

pub fn escala_psqt(peca: usize, bucket: usize) -> i32 {
    PSQT_SCALE[bucket * 6 + peca].load(std::sync::atomic::Ordering::Relaxed)
}

pub fn weights_for(board: &Board) -> &'static Weights {
    // A fitted file wins outright, and is checked BEFORE the family factors.
    //
    // It used to be checked after, which made it dead code: the V3 profile is
    // compiled into the defaults, so `family_scaling_active()` is true in
    // every build that has ever run, and the function returned before it ever
    // looked at the file. A tuning run could be measured only by a binary that
    // ignored its output.
    //
    // Winning outright is also the right meaning, not just the convenient one.
    // The file holds a complete set of weights, fitted against features that
    // already contain whatever the factors were doing -- scaling them again
    // would apply the same correction twice, exactly the trap `psqt_scale`
    // sets for anyone who tunes the tables themselves.
    let all = BUCKET_WEIGHTS.get_or_init(|| {
        let base = default_weights().clone();
        let dim = base.to_vec().len();
        if let Ok(path) = std::env::var("KESTREL_BUCKET_WEIGHTS") {
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    let v: Vec<i32> = text.trim().split(',').filter_map(|s| s.trim().parse().ok()).collect();
                    // Two accepted widths, because the fitter emits more per
                    // bucket than this can install.
                    //
                    // `gpuextract` lays each bucket out as dim positional
                    // weights, then MAT_PST_DIM material and piece-square
                    // values, then one bias. Only the positional block has a
                    // from_vec to be loaded through: the piece-square tables
                    // feed the incremental accumulator, which knows nothing
                    // about how many pawns are on the board and cannot hold a
                    // set per bucket.
                    //
                    // So a full-width file is read for its positional block
                    // and the rest is dropped, out loud. Dropping it is not a
                    // detail: the positional weights were fitted against the
                    // material values in the same vector, and those come out
                    // of the fit disagreeing sharply with the engine's --
                    // Material and PSQT are collinear and the fit splits their
                    // shared explanation wherever it likes. Adopting a fitted
                    // material block whole has already failed twice, measured.
                    let wide = dim + MAT_PST_DIM + 1;
                    let stride = if v.len() == dim * NUM_BUCKETS {
                        dim
                    } else if v.len() == wide * NUM_BUCKETS {
                        eprintln!(
                            "KESTREL_BUCKET_WEIGHTS: full-width file -- using the {} positional \
                             weights per bucket, IGNORING {} material/PST values and the bias",
                            dim, MAT_PST_DIM + 1
                        );
                        wide
                    } else {
                        eprintln!(
                            "KESTREL_BUCKET_WEIGHTS: expected {} or {} values ({} buckets x {} or x {}), \
                             found {} -- ignoring",
                            dim * NUM_BUCKETS, wide * NUM_BUCKETS, NUM_BUCKETS, dim, wide, v.len()
                        );
                        return Vec::new();
                    };
                    eprintln!(
                        "KESTREL_BUCKET_WEIGHTS: {} buckets x {} weights from {} \
                         (family scaling not applied on top)",
                        NUM_BUCKETS, dim, path
                    );
                    return (0..NUM_BUCKETS)
                        .map(|b| base.from_vec(&v[b * stride..b * stride + dim]))
                        .collect();
                }
                Err(e) => eprintln!("KESTREL_BUCKET_WEIGHTS: cannot read {}: {} -- ignoring", path, e),
            }
        }
        Vec::new()
    });
    if !all.is_empty() {
        return &all[bucket_of(board)];
    }
    // The fitted set, compiled in.
    //
    // Same precedence and same rule as a file: it is a complete set of
    // weights, so the family factors are NOT applied on top -- they are
    // already inside these numbers, having been in force when the features
    // were extracted, and applying them again would apply them twice.
    #[cfg(feature = "fitted")]
    {
        static FIT: OnceLock<Vec<Weights>> = OnceLock::new();
        return &FIT.get_or_init(|| {
            let base = default_weights().clone();
            let dim = base.to_vec().len();
            assert_eq!(
                crate::fitted::FITTED.len(),
                dim * NUM_BUCKETS,
                "fitted table is {} values, evaluation wants {} ({} buckets x {})",
                crate::fitted::FITTED.len(), dim * NUM_BUCKETS, NUM_BUCKETS, dim
            );
            (0..NUM_BUCKETS)
                .map(|b| base.from_vec(&crate::fitted::FITTED[b * dim..(b + 1) * dim]))
                .collect()
        })[bucket_of(board)];
    }
    #[cfg(not(feature = "fitted"))]
    {
    if family_scaling_active() {
        // One set per bucket, each built with ITS OWN family factors. This
        // used to clone a single scaled set into every bucket, which made the
        // mechanism structurally present and practically inert: whatever the
        // profile said about bucket 7 was applied to bucket 0 as well.
        let scaled = SCALED_BUCKETS
            .get_or_init(|| (0..NUM_BUCKETS).map(scaled_weights_for).collect());
        return &scaled[bucket_of(board)];
    }
    static PLAIN: OnceLock<Vec<Weights>> = OnceLock::new();
    &PLAIN.get_or_init(|| vec![default_weights().clone(); NUM_BUCKETS])[bucket_of(board)]
    }
}

/// The weights with per-family factors applied.
///
/// A separate OnceLock from `default_weights()`, and deliberately not touched
/// by `warmup()`. That function evaluates a real position to pay the
/// first-call cost off the clock, which seals whatever it reads -- so anything
/// configurable over UCI must be built lazily, on a lock the warm-up never
/// looks at. The same mistake in search.rs silently invalidated a whole
/// parameter sweep.
static SCALED_WEIGHTS: OnceLock<Weights> = OnceLock::new();

/// The weights this bucket evaluates with right now, family factors included.
///
/// This is the starting point a fit has to be given, and it is not
/// `default_weights()`: with a profile in force -- and V3 is compiled in, so
/// one always is -- the engine evaluates with the scaled set. A fit started
/// from the unscaled one begins by rediscovering the profile, and a fit
/// started from zero begins by rediscovering chess.
pub fn effective_weights_for_bucket(bucket: usize) -> Weights {
    // Ask the evaluation, do not reconstruct what it probably does.
    //
    // This used to rebuild the answer from the family factors, and it was wrong
    // the moment a fitted table was compiled in: it returned V3 while the engine
    // played something else, so a fit told to start from "the engine" started
    // 380 centipawns away from it on its very first weight, and nothing said so.
    //
    // Same failure as the one that made KESTREL_BUCKET_WEIGHTS dead code --
    // a second path that describes the first instead of calling it. There is
    // one function that decides which weights a position gets, and this is it.
    let board = Board::from_fen(bucket_probe_fen(bucket));
    debug_assert_eq!(bucket_of(&board), bucket);
    weights_for(&board).clone()
}

/// A position with the pawn count that lands in this bucket.
///
/// `weights_for` takes a board because the bucket is a property of the
/// position; asking it for a particular bucket means handing it one.
fn bucket_probe_fen(bucket: usize) -> &'static str {
    // Pawn counts 0, 5, 7, 9, 10, 12, 13, 16 -- one inside each bucket of the
    // V4 boundaries (0-3, 4-6, 7-8, 9, 10-11, 12, 13, 14+).
    const FENS: [&str; 8] = [
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
        "4k3/ppp5/8/8/8/8/PP6/4K3 w - - 0 1",
        "4k3/pppp4/8/8/8/8/PPP5/4K3 w - - 0 1",
        "4k3/ppppp3/8/8/8/8/PPPP4/4K3 w - - 0 1",
        "4k3/ppppp3/8/8/8/8/PPPPP3/4K3 w - - 0 1",
        "4k3/pppppp2/8/8/8/8/PPPPPP2/4K3 w - - 0 1",
        "4k3/ppppppp1/8/8/8/8/PPPPPP2/4K3 w - - 0 1",
        "4k3/pppppppp/8/8/8/8/PPPPPPPP/4K3 w - - 0 1",
    ];
    FENS[bucket.min(FENS.len() - 1)]
}

fn scaled_weights_for(bucket: usize) -> Weights {
    {
        let base = default_weights();
        let mut v = base.to_vec();
        let fams = base.field_families();
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for (i, fam) in fams.iter().enumerate() {
            if i >= v.len() || fam.is_empty() {
                continue;
            }
            let f = match FAMILIES.iter().position(|&x| x == *fam) {
                Some(k) => FAMILY_SCALE[bucket * 6 + k].load(std::sync::atomic::Ordering::Relaxed),
                None => 1000,
            };
            if f != 1000 {
                v[i] = (v[i] as i64 * f as i64 / 1000) as i32;
                *counts.entry(fam).or_insert(0) += 1;
            }
        }
        if bucket == 0 {
            for (fam, n) in &counts {
                eprintln!("family scale: {} -> {} pesos alterados (por bucket)", fam, n);
            }
        }
        base.from_vec(&v)
    }
}

/// The weights the evaluation should use: scaled when any family factor was
/// set, the plain defaults otherwise. One branch on a value that is almost
/// always false.
pub fn active_weights() -> &'static Weights {
    if family_scaling_active() {
        // No board here, so the fullest bucket: this is used by tooling that
        // wants "the weights", not by the search, which always has a position.
        SCALED_BUCKETS
            .get_or_init(|| (0..NUM_BUCKETS).map(scaled_weights_for).collect())
            .last()
            .unwrap()
    } else {
        default_weights()
    }
}

pub fn default_weights() -> &'static Weights {
    DEFAULT_WEIGHTS.get_or_init(|| {
        if let Ok(path) = std::env::var("KESTREL_TUNED_WEIGHTS") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let parsed: Vec<i32> = text.trim().split(',').filter_map(|s| s.parse().ok()).collect();
                let base = Weights::default();
                if parsed.len() == base.to_vec().len() {
                    eprintln!("KESTREL_TUNED_WEIGHTS: loaded {} scalars from {}", parsed.len(), path);
                    return base.from_vec(&parsed);
                } else {
                    eprintln!("KESTREL_TUNED_WEIGHTS: length mismatch ({} vs expected {}), ignoring", parsed.len(), base.to_vec().len());
                }
            }
        }
        // Default agora = pesos calibrados pelo tuner do Kestrel (TUNED_V46,
        // ver doc acima). O env var KESTREL_TUNED_WEIGHTS continua a
        // permitir testar OUTRO conjunto por cima destes. from_vec só
        // toca nos campos do Weights presentes no to_vec (mobilidade/
        // threats/pawn-structure/king-safety weights) -- material/PST e
        // king_danger_table ficam das consts.
        let w = Weights::default().from_vec(&TUNED_V46);
        // King-safety scale, for measuring how much this engine should lean
        // on king safety at all. Comparing our evaluation against a stronger
        // hand-crafted one term by term (2026-07-26) showed every category
        // matching its share of the total except this one: king safety moves
        // their evaluation more than twice as much as it moves ours. Rather
        // than guess at thirty-six individual weights, scale the whole block
        // by one number and find out whether leaning harder helps at all.
        // Unset reproduces the previous behaviour exactly.
        // The range is the same one the `eval` command reports as "king":
        // attackers, safe checks, shelter and storm.
        if let Ok(v) = std::env::var("KESTREL_KING_SCALE") {
            if let Ok(f) = v.parse::<f64>() {
                let mut vec = w.to_vec();
                let hi = KING_RANGE.1.min(vec.len());
                for x in vec[KING_RANGE.0..hi].iter_mut() {
                    *x = (*x as f64 * f).round() as i32;
                }
                eprintln!("KESTREL_KING_SCALE: king-safety weights scaled by {}", f);
                return w.from_vec(&vec);
            }
        }
        w
    })
}

/// Index range of the king-safety block inside `Weights::to_vec()` -- the
/// same slice the `eval` command sums to report "king". Kept next to the
/// scale above so the two cannot drift apart silently.
pub const KING_RANGE: (usize, usize) = (240, 276);

impl Weights {
    /// Flattens every tunable scalar into one Vec<i32>, fixed order,
    /// matching `apply_vec()` exactly -- lets the tuner (src/tuning.rs)
    /// treat this as one flat parameter vector for coordinate descent
    /// instead of hand-writing a perturbation loop per field.
    /// `king_danger_table` is deliberately excluded: it's a derived
    /// smooth curve (see its own comment above), not something to let
    /// 128 independent coordinate-descent steps chew on with a small
    /// dataset -- tune the pieces that feed it instead.
    pub fn to_vec(&self) -> Vec<i32> {
        let mut v = Vec::with_capacity(512);
        macro_rules! pair { ($p:expr) => { v.push($p.0); v.push($p.1); } }
        macro_rules! pairs { ($arr:expr) => { for p in $arr.iter() { pair!(p); } } }
        pair!(self.bishop_pair);
        pair!(self.long_diag_bishop);
        pair!(self.minor_behind_pawn);
        pair!(self.knight_outpost);
        pairs!(self.rook_open);
        pair!(self.rook_on_seventh);
        pair!(self.tempo);
        pairs!(self.mobility_knight);
        pairs!(self.mobility_bishop);
        pairs!(self.mobility_rook);
        pairs!(self.mobility_queen);
        pairs!(self.king_attacker_weight);
        pair!(self.king_attacks);
        pair!(self.safe_knight_check);
        pair!(self.safe_bishop_check);
        pair!(self.safe_rook_check);
        pair!(self.safe_queen_check);
        pairs!(self.pawn_shelter);
        pair!(self.shelter_open);
        pairs!(self.pawn_tornado);
        for row in self.threat_by_pawn.iter() { pairs!(row); }
        for row in self.threat_by_knight.iter() { pairs!(row); }
        for row in self.threat_by_bishop.iter() { pairs!(row); }
        for row in self.threat_by_rook.iter() { pairs!(row); }
        for row in self.threat_by_queen.iter() { pairs!(row); }
        pairs!(self.threat_by_king);
        pair!(self.knight_hit_queen);
        pair!(self.bishop_hit_queen);
        pair!(self.rook_hit_queen);
        pair!(self.push_threat);
        pair!(self.restricted_squares);
        pairs!(self.pawn_phalanx);
        pairs!(self.defended_pawn);
        pairs!(self.isolated_pawn);
        pairs!(self.doubled_pawn);
        pair!(self.isolated_exposed);
        pair!(self.backward_exposed);
        for blocked_row in self.passed_pawn.iter() {
            for controlled_row in blocked_row.iter() {
                pairs!(controlled_row);
            }
        }
        pairs!(self.our_passer_proximity);
        pairs!(self.their_passer_proximity);
        pairs!(self.passer_defended_push);
        pairs!(self.passer_slider_behind);
        pair!(self.backward_pawn);
        for row in self.candidate_passer.iter() { pairs!(row); }
        pairs!(self.bishop_pawns);
        pair!(self.weak_king_ring);
        pairs!(self.king_flank_attacks);
        pairs!(self.king_flank_defenses);
        pair!(self.uncastled_king_no_rights);
        pair!(self.uncastled_king_has_rights);
        v.push(self.scale_ocb_bishops_only);
        v.push(self.scale_ocb_one_rook);
        v.push(self.scale_ocb_one_knight);
        v.push(self.scale_fallback_base);
        v.push(self.scale_fallback_per_pawn);
        v.push(self.complexity_total_pawns);
        v.push(self.complexity_pawn_flanks);
        v.push(self.complexity_pawn_endgame);
        v.push(self.complexity_adjustment);
        // Appended at the end on purpose: every index before this keeps its
        // meaning, so weight files written before these existed stay valid
        // simply by being shorter.
        pair!(self.stonewall);
        pair!(self.stonewall_outpost);
        pair!(self.stonewall_bad_bishop);
        // Added 2026-07-29. These terms were being evaluated but not emitted,
        // so the feature extractor could not see them: probing a weight did
        // not move them, and its self-check reported a 14.8cp gap between the
        // decomposition and the real evaluation -- a gap that did not shrink
        // when the probe scale was raised a hundredfold, which is the test
        // that separates rounding from "the model is not the function". A
        // tuner fed features that do not add up produces weights happily, and
        // they are wrong.
        pairs!(self.hanging);
        pairs!(self.king_tropism);
        pair!(self.space);
        pair!(self.blocked_pawns);
        pairs!(self.king_aim);
        pair!(self.king_battery);
        // NO FIM de proposito: acrescentar um termo a meio desloca tudo o que
        // vem depois, e as tabelas ja' ajustadas passam a ser lidas nos sitios
        // errados sem que nada avise. No fim, expandir e' acrescentar zeros.
        for linha in self.imbalance.iter() {
            for x in linha.iter() { v.push(x.0); v.push(x.1); }
        }
        pairs!(self.tornado_locked);
        pair!(self.passer_outside);
        v
    }
    /// The family of every scalar `to_vec` emits, in the same order.
    ///
    /// Generated from the body of `to_vec` by substitution rather than written
    /// out, so it cannot drift from the vector it describes -- a families list
    /// that is one entry out of step does not fail, it scales the wrong terms
    /// and reports whatever those happen to do.
    pub fn field_families(&self) -> Vec<&'static str> {
        let mut f: Vec<&'static str> = Vec::with_capacity(512);
        // Macros, not closures: three closures capturing the same Vec mutably
        // cannot coexist, and splitting them into separate scopes would make
        // the generated body harder to keep in step with `to_vec`.
        macro_rules! two { ($n:expr) => {{ let g = family_of($n); f.push(g); f.push(g); }} }
        macro_rules! one { ($n:expr) => {{ f.push(family_of($n)); }} }
        macro_rules! many {
            ($n:expr, $len:expr) => {{
                let g = family_of($n);
                for _ in 0..$len { f.push(g); f.push(g); }
            }}
        }
        two!("bishop_pair");
        two!("long_diag_bishop");
        two!("minor_behind_pawn");
        two!("knight_outpost");
        many!("rook_open", self.rook_open.len());
        two!("rook_on_seventh");
        two!("tempo");
        many!("mobility_knight", self.mobility_knight.len());
        many!("mobility_bishop", self.mobility_bishop.len());
        many!("mobility_rook", self.mobility_rook.len());
        many!("mobility_queen", self.mobility_queen.len());
        many!("king_attacker_weight", self.king_attacker_weight.len());
        two!("king_attacks");
        two!("safe_knight_check");
        two!("safe_bishop_check");
        two!("safe_rook_check");
        two!("safe_queen_check");
        many!("pawn_shelter", self.pawn_shelter.len());
        two!("shelter_open");
        many!("pawn_tornado", self.pawn_tornado.len());
        many!("threat_by_king", self.threat_by_king.len());
        two!("knight_hit_queen");
        two!("bishop_hit_queen");
        two!("rook_hit_queen");
        two!("push_threat");
        two!("restricted_squares");
        many!("pawn_phalanx", self.pawn_phalanx.len());
        many!("defended_pawn", self.defended_pawn.len());
        many!("isolated_pawn", self.isolated_pawn.len());
        many!("doubled_pawn", self.doubled_pawn.len());
        two!("isolated_exposed");
        two!("backward_exposed");
        many!("our_passer_proximity", self.our_passer_proximity.len());
        many!("their_passer_proximity", self.their_passer_proximity.len());
        many!("passer_defended_push", self.passer_defended_push.len());
        many!("passer_slider_behind", self.passer_slider_behind.len());
        two!("backward_pawn");
        many!("bishop_pawns", self.bishop_pawns.len());
        two!("weak_king_ring");
        many!("king_flank_attacks", self.king_flank_attacks.len());
        many!("king_flank_defenses", self.king_flank_defenses.len());
        two!("uncastled_king_no_rights");
        two!("uncastled_king_has_rights");
        one!("scale_ocb_bishops_only");
        one!("scale_ocb_one_rook");
        one!("scale_ocb_one_knight");
        one!("scale_fallback_base");
        one!("scale_fallback_per_pawn");
        one!("complexity_total_pawns");
        one!("complexity_pawn_flanks");
        one!("complexity_pawn_endgame");
        one!("complexity_adjustment");
        two!("stonewall");
        two!("stonewall_outpost");
        two!("stonewall_bad_bishop");
        many!("imbalance", 25);
        many!("tornado_locked", 4);
        two!("passer_outside");
        f
    }

    pub fn field_names(&self) -> Vec<&'static str> {
        let mut f: Vec<&'static str> = Vec::with_capacity(512);
        // Macros, not closures: three closures capturing the same Vec mutably
        // cannot coexist, and splitting them into separate scopes would make
        // the generated body harder to keep in step with `to_vec`.
        macro_rules! two { ($n:expr) => {{ f.push($n); f.push($n); }} }
        macro_rules! one { ($n:expr) => {{ f.push($n); }} }
        macro_rules! many {
            ($n:expr, $len:expr) => {{
                let g = family_of($n);
                for _ in 0..$len { f.push(g); f.push(g); }
            }}
        }
        two!("bishop_pair");
        two!("long_diag_bishop");
        two!("minor_behind_pawn");
        two!("knight_outpost");
        many!("rook_open", self.rook_open.len());
        two!("rook_on_seventh");
        two!("tempo");
        many!("mobility_knight", self.mobility_knight.len());
        many!("mobility_bishop", self.mobility_bishop.len());
        many!("mobility_rook", self.mobility_rook.len());
        many!("mobility_queen", self.mobility_queen.len());
        many!("king_attacker_weight", self.king_attacker_weight.len());
        two!("king_attacks");
        two!("safe_knight_check");
        two!("safe_bishop_check");
        two!("safe_rook_check");
        two!("safe_queen_check");
        many!("pawn_shelter", self.pawn_shelter.len());
        two!("shelter_open");
        many!("pawn_tornado", self.pawn_tornado.len());
        many!("threat_by_king", self.threat_by_king.len());
        two!("knight_hit_queen");
        two!("bishop_hit_queen");
        two!("rook_hit_queen");
        two!("push_threat");
        two!("restricted_squares");
        many!("pawn_phalanx", self.pawn_phalanx.len());
        many!("defended_pawn", self.defended_pawn.len());
        many!("isolated_pawn", self.isolated_pawn.len());
        many!("doubled_pawn", self.doubled_pawn.len());
        two!("isolated_exposed");
        two!("backward_exposed");
        many!("our_passer_proximity", self.our_passer_proximity.len());
        many!("their_passer_proximity", self.their_passer_proximity.len());
        many!("passer_defended_push", self.passer_defended_push.len());
        many!("passer_slider_behind", self.passer_slider_behind.len());
        two!("backward_pawn");
        many!("bishop_pawns", self.bishop_pawns.len());
        two!("weak_king_ring");
        many!("king_flank_attacks", self.king_flank_attacks.len());
        many!("king_flank_defenses", self.king_flank_defenses.len());
        two!("uncastled_king_no_rights");
        two!("uncastled_king_has_rights");
        one!("scale_ocb_bishops_only");
        one!("scale_ocb_one_rook");
        one!("scale_ocb_one_knight");
        one!("scale_fallback_base");
        one!("scale_fallback_per_pawn");
        one!("complexity_total_pawns");
        one!("complexity_pawn_flanks");
        one!("complexity_pawn_endgame");
        one!("complexity_adjustment");
        two!("stonewall");
        two!("stonewall_outpost");
        two!("stonewall_bad_bishop");
        many!("imbalance", 25);
        many!("tornado_locked", 4);
        two!("passer_outside");
        f
    }


    /// Inverse of `to_vec()` -- rebuilds a full `Weights` from a flat
    /// vector in the exact same field order. `king_danger_table` is
    /// copied from `self` unchanged (see `to_vec` doc).
    pub fn from_vec(&self, v: &[i32]) -> Weights {
        let mut i = 0;
        // A vector shorter than to_vec() is not an error: it is a weight
        // file written before some field existed. Those fields keep the
        // value they have here rather than making the whole file unusable,
        // which is what lets new tunable terms be added without invalidating
        // every set of weights already measured.
        let base_vec = self.to_vec();
        macro_rules! next { () => { { let x = if i < v.len() { v[i] } else { base_vec[i] }; i += 1; x } } }
        macro_rules! pair { () => { (next!(), next!()) } }
        macro_rules! pairs { ($n:expr) => { { let mut a = [(0i32,0i32); $n]; for j in 0..$n { a[j] = pair!(); } a } } }
        let bishop_pair = pair!();
        let long_diag_bishop = pair!();
        let minor_behind_pawn = pair!();
        let knight_outpost = pair!();
        let rook_open = pairs!(2);
        let rook_on_seventh = pair!();
        let tempo = pair!();
        let mobility_knight = pairs!(28);
        let mobility_bishop = pairs!(28);
        let mobility_rook = pairs!(28);
        let mobility_queen = pairs!(28);
        let king_attacker_weight = pairs!(4);
        let king_attacks = pair!();
        let safe_knight_check = pair!();
        let safe_bishop_check = pair!();
        let safe_rook_check = pair!();
        let safe_queen_check = pair!();
        let pawn_shelter = pairs!(4);
        let shelter_open = pair!();
        let pawn_tornado = pairs!(4);
        let threat_by_pawn = [pairs!(6), pairs!(6)];
        let threat_by_knight = [pairs!(6), pairs!(6)];
        let threat_by_bishop = [pairs!(6), pairs!(6)];
        let threat_by_rook = [pairs!(6), pairs!(6)];
        let threat_by_queen = [pairs!(6), pairs!(6)];
        let threat_by_king = pairs!(6);
        let knight_hit_queen = pair!();
        let bishop_hit_queen = pair!();
        let rook_hit_queen = pair!();
        let push_threat = pair!();
        let restricted_squares = pair!();
        let pawn_phalanx = pairs!(8);
        let defended_pawn = pairs!(8);
        let isolated_pawn = pairs!(4);
        let doubled_pawn = pairs!(4);
        let isolated_exposed = pair!();
        let backward_exposed = pair!();
        let passed_pawn = [[pairs!(8), pairs!(8)], [pairs!(8), pairs!(8)]];
        let our_passer_proximity = pairs!(8);
        let their_passer_proximity = pairs!(8);
        let passer_defended_push = pairs!(8);
        let passer_slider_behind = pairs!(8);
        let backward_pawn = pair!();
        let candidate_passer = [pairs!(8), pairs!(8)];
        let bishop_pawns = pairs!(7);
        let weak_king_ring = pair!();
        let king_flank_attacks = pairs!(2);
        let king_flank_defenses = pairs!(2);
        let uncastled_king_no_rights = pair!();
        let uncastled_king_has_rights = pair!();
        let scale_ocb_bishops_only = next!();
        let scale_ocb_one_rook = next!();
        let scale_ocb_one_knight = next!();
        let scale_fallback_base = next!();
        let scale_fallback_per_pawn = next!();
        let complexity_total_pawns = next!();
        let complexity_pawn_flanks = next!();
        let complexity_pawn_endgame = next!();
        let complexity_adjustment = next!();
        let stonewall = pair!();
        let stonewall_outpost = pair!();
        let stonewall_bad_bishop = pair!();
        // Same order as `to_vec`. Files written before these existed are
        // shorter and simply keep the defaults -- see the note on `next!`.
        let hanging_v = pairs!(5);
        let king_tropism_v = pairs!(4);
        let space_v = pair!();
        let blocked_pawns_v = pair!();
        let king_aim_v = pairs!(3);
        let king_battery_v = pair!();
        debug_assert!(i >= v.len(), "from_vec: read fewer values than supplied");
        let imbalance = {
            let mut t = [[(0i32, 0i32); 5]; 5];
            for i in 0..5 { for j in 0..5 { t[i][j] = (next!(), next!()); } }
            t
        };
        let tornado_locked = pairs!(4);
        let passer_outside = pair!();
        Weights {
            bishop_pair, long_diag_bishop, minor_behind_pawn, knight_outpost, rook_open, rook_on_seventh, tempo,
            mobility_knight, mobility_bishop, mobility_rook, mobility_queen,
            king_attacker_weight, king_attacks,
            safe_knight_check, safe_bishop_check, safe_rook_check, safe_queen_check,
            // Carried over from `self`, like king_danger_table: these are
            // deliberately NOT in to_vec/from_vec. Adding fields to that
            // vector would change its length and invalidate the tuned weight
            // file already in production. They are reasoned constants for
            // now; they join the tuned vector once the shape is settled.
            // Fora do vector pela mesma razao que os de baixo: mudar o
            // comprimento invalidaria o ficheiro de pesos ja' em producao.
            hanging: hanging_v,
            king_tropism: king_tropism_v,
            space: space_v,
            king_aim: king_aim_v,
            king_battery: king_battery_v,
            blocked_pawns: blocked_pawns_v,
            unsafe_knight_check: self.unsafe_knight_check,
            unsafe_bishop_check: self.unsafe_bishop_check,
            unsafe_rook_check: self.unsafe_rook_check,
            unsafe_queen_check: self.unsafe_queen_check,
            queenless_attack: self.queenless_attack,
            king_safety_offset: self.king_safety_offset,
            safety_pinned: self.safety_pinned,
            safety_discovered: self.safety_discovered,
            king_danger_table: self.king_danger_table,
            pawn_shelter, shelter_open, pawn_tornado,
            threat_by_pawn, threat_by_knight, threat_by_bishop, threat_by_rook, threat_by_queen, threat_by_king,
            knight_hit_queen, bishop_hit_queen, rook_hit_queen, push_threat, restricted_squares,
            pawn_phalanx, defended_pawn, isolated_pawn, doubled_pawn, isolated_exposed, backward_exposed, passed_pawn,
            our_passer_proximity, their_passer_proximity, passer_defended_push, passer_slider_behind,
            backward_pawn, candidate_passer, bishop_pawns, imbalance, tornado_locked, passer_outside, weak_king_ring,
            king_flank_attacks, king_flank_defenses,
            uncastled_king_no_rights, uncastled_king_has_rights,
            scale_ocb_bishops_only, scale_ocb_one_rook, scale_ocb_one_knight,
            scale_fallback_base, scale_fallback_per_pawn,
            complexity_total_pawns, complexity_pawn_flanks, complexity_pawn_endgame, complexity_adjustment,
            stonewall, stonewall_outpost, stonewall_bad_bishop,
        }
    }
}

/// Full evaluate(), but with a caller-supplied `Weights` instead of
/// `default_weights()` -- what the tuner calls to score a position
/// under a candidate parameter vector. Mirrors `evaluate()`'s material
/// + positional composition exactly.
pub fn evaluate_with_weights(board: &Board, w: &Weights) -> i32 {
    let p = positional_terms(board, w);
    let p_signed = if board.side == Color::White { p } else { -p };
    material_pst(board) + p_signed
}

/// Avalia mobilidade, pressao sobre o rei inimigo, par de bispos, torres
/// em colunas abertas/semi-abertas e estrutura de peoes usando os nossos
/// pesos calibrados (ver constantes acima) -- consistente com as
/// tabelas PSQT/MATERIAL desta seccao. Acumula (mg, eg) e interpola pela
/// fase do jogo (mesma logica de material_pst). Devolve da perspetiva
/// das BRANCAS (score_white - score_black), interpolado; o chamador
/// (evaluate) converte para a convencao negamax.
/// Bitboard de todas as casas atacadas por peoes de `by`. Combinacao
/// classica: shifts diagonais dos peoes ao invez de um loop.
fn pawn_attacks_by(board: &Board, by: Color) -> Bitboard {
    let pawns = board.pieces[by.idx()][PieceType::Pawn.idx()];
    if by == Color::White {
        // brancas atacam para NW e NE (rank+1, file-1 / file+1)
        (pawns & !FILE_A) << 7 | (pawns & !FILE_H) << 9
    } else {
        // pretas atacam para SW e SE
        (pawns & !FILE_A) >> 9 | (pawns & !FILE_H) >> 7
    }
}

/// Chebyshev (king-move) distance between two squares -- max of the
/// file and rank deltas. Used by OUR/THEIR_PASSER_PROXIMITY.
#[inline]
fn chebyshev_distance(a: Square, b: Square) -> usize {
    let fd = (file_of(a) as i32 - file_of(b) as i32).unsigned_abs();
    let rd = (rank_of(a) as i32 - rank_of(b) as i32).unsigned_abs();
    fd.max(rd) as usize
}

/// "King flank": a 4-file band on the king's side of the board
/// (a-d/c-f/e-h depending which third the king's file falls in)
/// intersected with a 5-rank band on that king's own half (see the
/// KING_FLANK_ATTACKS/DEFENSES doc comment) -- wider than the immediate
/// king-ring/zone, used for the "space near my king is compromised"
/// evaluation.
#[inline]
fn king_flank(king_sq: Square, color: Color) -> Bitboard {
    let kf = file_of(king_sq) as i32;
    let file_band: Bitboard = if kf <= 2 {
        (FILE_A << 0) | (FILE_A << 1) | (FILE_A << 2) | (FILE_A << 3)
    } else if kf <= 4 {
        (FILE_A << 2) | (FILE_A << 3) | (FILE_A << 4) | (FILE_A << 5)
    } else {
        (FILE_A << 4) | (FILE_A << 5) | (FILE_A << 6) | (FILE_A << 7)
    };
    let rank_band: Bitboard = if color == Color::White {
        RANK_1 | RANK_2 | RANK_3 | RANK_4 | RANK_5
    } else {
        RANK_8 | RANK_7 | RANK_6 | RANK_5 | RANK_4
    };
    file_band & rank_band
}

// === Mascaras pre-computadas de peao ===
//
// Estas mascaras eram construidas bit a bit, com ciclos aninhados sobre
// tres colunas e ate' sete filas -- ~21 iteracoes por peao so' para
// decidir se ele esta' passado. Sao funcao apenas da casa e da cor,
// portanto pertencem a uma tabela, nao a um ciclo em tempo de busca.
//
//   PASSADO_MASK   colunas f-1,f,f+1, filas estritamente a frente
//   FRENTE_MASK    mesma coluna, filas estritamente a frente
//   TRAS_MASK      mesma coluna, filas estritamente atras
//   ADJ_FRENTE     colunas f-1,f+1, filas estritamente a frente
//   ADJ_TRAS_INCL  colunas f-1,f+1, mesma fila ou atras
//   ADJ_MESMA_FILA colunas f-1,f+1, mesma fila (falange)
//   ADJ_COLUNAS    colunas f-1,f+1 inteiras (isolado)
struct MascarasPeao {
    passado: [[Bitboard; 64]; 2],
    frente: [[Bitboard; 64]; 2],
    tras: [[Bitboard; 64]; 2],
    adj_frente: [[Bitboard; 64]; 2],
    adj_tras_incl: [[Bitboard; 64]; 2],
    adj_mesma_fila: [Bitboard; 64],
    adj_colunas: [Bitboard; 64],
}

static MASCARAS_PEAO: std::sync::OnceLock<Box<MascarasPeao>> = std::sync::OnceLock::new();

fn constroi_mascaras_peao() -> Box<MascarasPeao> {
    let mut m = Box::new(MascarasPeao {
        passado: [[0; 64]; 2],
        frente: [[0; 64]; 2],
        tras: [[0; 64]; 2],
        adj_frente: [[0; 64]; 2],
        adj_tras_incl: [[0; 64]; 2],
        adj_mesma_fila: [0; 64],
        adj_colunas: [0; 64],
    });
    for s in 0..64u8 {
        let f = file_of(s as Square) as i32;
        let r = rank_of(s as Square) as i32;
        for adj in [f - 1, f + 1] {
            if !(0..8).contains(&adj) { continue; }
            m.adj_colunas[s as usize] |= FILE_A << adj;
            m.adj_mesma_fila[s as usize] |= bb(sq(adj as u8, r as u8));
        }
        for &c in &[Color::White, Color::Black] {
            let i = c.idx();
            // Filas a frente / atras do ponto de vista desta cor.
            let (a_frente, a_tras): (Vec<i32>, Vec<i32>) = if c == Color::White {
                (((r + 1)..8).collect(), (0..r).collect())
            } else {
                ((0..r).collect(), ((r + 1)..8).collect())
            };
            for &rr in &a_frente {
                m.frente[i][s as usize] |= bb(sq(f as u8, rr as u8));
            }
            for &rr in &a_tras {
                m.tras[i][s as usize] |= bb(sq(f as u8, rr as u8));
            }
            for adj in (f - 1)..=(f + 1) {
                if !(0..8).contains(&adj) { continue; }
                for &rr in &a_frente {
                    m.passado[i][s as usize] |= bb(sq(adj as u8, rr as u8));
                    if adj != f {
                        m.adj_frente[i][s as usize] |= bb(sq(adj as u8, rr as u8));
                    }
                }
                if adj != f {
                    // Mesma fila ou atras.
                    for &rr in &a_tras {
                        m.adj_tras_incl[i][s as usize] |= bb(sq(adj as u8, rr as u8));
                    }
                    m.adj_tras_incl[i][s as usize] |= bb(sq(adj as u8, r as u8));
                }
            }
        }
    }
    m
}

#[inline(always)]
fn masc() -> &'static MascarasPeao {
    MASCARAS_PEAO.get_or_init(constroi_mascaras_peao)
}

// === Cache de estrutura de peoes ===
//
// A estrutura de peoes muda raramente: numa partida de 40 lances ha' talvez
// 15 lances de peao, e todos os milhoes de nos entre eles partilham
// exactamente a mesma estrutura. Recalcula-la em cada no' e' copiar a
// mesma pagina do livro mil vezes.
//
// O que entra aqui e' SO' o que depende exclusivamente dos dois bitboards
// de peoes: atrasado, candidato, isolado, defendido, falange, dobrado.
// O peao PASSADO nao entra -- vale conforme a casa de avanco esta'
// bloqueada, conforme quem a controla, conforme a distancia dos reis e
// conforme ha' torre/dama atras. Isso muda sem nenhum peao se mexer, e
// cachea-lo pela chave dos peoes devolveria um valor errado em silencio.
//
// Mas a parte CARA do teste de passado -- varrer as tres colunas a frente
// de cada peao, fila a fila -- essa e' pura, e por isso o resultado dela
// (o bitboard dos passados por cor) viaja na cache. Quem chama so' percorre
// esses poucos peoes para aplicar os termos que dependem das pecas.
#[derive(Clone, Copy)]
struct EntradaPeoes {
    chave: u64,
    geracao: u64,
    mg: i32,
    eg: i32,
    passados: [Bitboard; 2],
}

/// Os pesos NAO sao imutaveis: `set_family_scale` (ha' uma familia chamada
/// exactamente "pawns"), `set_psqt_scale`, `set_material` e
/// `set_material_buckets` alteram-nos em runtime por setoption -- e' assim
/// que o afinador trabalha, e uma cache de VALORES que nao reparasse nisso
/// devolveria a calibracao anterior sem dar sinal nenhum.
///
/// Medido: hoje isso nao chega a acontecer, porque `uci.rs` corre cada
/// busca em `std::thread::scope` e as tabelas thread_local morrem com as
/// threads no fim de cada `go`. Confirmado com uma versao sem este guarda:
/// o score muda na mesma quando `scale_pawns` muda. Fica na mesma porque
/// custa uma leitura atomica relaxed por avaliacao e e' o que nos protege
/// no dia em que as threads passarem a ser reaproveitadas -- altura em que
/// a falha seria silenciosa e daria numeros de afinacao errados.
static GERACAO_PESOS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Chamada por quem altera pesos em runtime.
pub fn invalida_cache_peoes() {
    GERACAO_PESOS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

const CACHE_PEOES_BITS: usize = 15; // 32768 entradas, ~0.5 MB por thread

// Uma por thread: a busca corre Lazy SMP e uma tabela partilhada exigiria
// sincronizacao para poupar memoria que nao falta.
thread_local! {
    static CACHE_PEOES: std::cell::RefCell<Vec<EntradaPeoes>> =
        std::cell::RefCell::new(vec![
            EntradaPeoes { chave: 0, geracao: u64::MAX, mg: 0, eg: 0, passados: [0; 2] };
            1 << CACHE_PEOES_BITS
        ]);
}

/// Chave Zobrist-equivalente sobre SO' os peoes. Multiplicacao por
/// constante impar e' bijectiva em u64; a entrada guarda a chave inteira e
/// comparamo-la, portanto uma colisao exige coincidencia nos 64 bits.
#[inline(always)]
fn chave_peoes(board: &Board) -> u64 {
    let wp = board.pieces[Color::White.idx()][PieceType::Pawn.idx()];
    let bp = board.pieces[Color::Black.idx()][PieceType::Pawn.idx()];
    wp.wrapping_mul(0x9E3779B97F4A7C15) ^ bp.wrapping_mul(0xC2B2AE3D27D4EB4F)
}

/// Percorre a estrutura de peoes de raiz. Devolve (mg, eg) ja' com sinal
/// (brancas positivo, pretas negativo) e o bitboard dos passados por cor.
/// Nao le' NADA do tabuleiro alem dos dois bitboards de peoes -- e' isso
/// que torna o resultado cacheavel.
fn estrutura_de_peoes(board: &Board, w: &Weights) -> (i32, i32, [Bitboard; 2]) {
    let a = atk();
    let mk = masc();
    let mut mg = 0i32;
    let mut eg = 0i32;
    let mut passados = [0 as Bitboard; 2];

    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let own_pawns = board.pieces[c.idx()][PieceType::Pawn.idx()];
        let enemy_pawns = board.pieces[c.opp().idx()][PieceType::Pawn.idx()];
        let mut pawns = own_pawns;
        while pawns != 0 {
            let s = pawns.trailing_zeros() as Square;
            pawns &= pawns - 1;
            let f = file_of(s) as i32;
            let r = rank_of(s) as i32;
            let rel_rank = if c == Color::White { r as usize } else { (7 - r) as usize };

            // Teste de passado -- caro, e puro. O resultado sai daqui pelo
            // bitboard `passados`; os termos que dependem das pecas sao
            // aplicados por quem chama.
            let blocked = enemy_pawns & mk.passado[c.idx()][s as usize] != 0;
            if !blocked {
                passados[c.idx()] |= bb(s);
            } else {
                // Peao atrasado.
                let front_r = if c == Color::White { r + 1 } else { r - 1 };
                let supported_ever = own_pawns & mk.adj_tras_incl[c.idx()][s as usize] != 0;
                // Atrasado e' ALTERNATIVA a isolado, nao acumula com ele. Nas
                // duas referencias e' um `else if`: um peao sem vizinhos
                // nenhuns e' isolado, e chamar-lhe atrasado por cima disso e'
                // penalizar a mesma falta de apoio duas vezes. Nos tinhamos os
                // dois blocos independentes.
                let tem_vizinho = own_pawns & mk.adj_colunas[s as usize] != 0;
                if !supported_ever && tem_vizinho && (0..8).contains(&front_r) {
                    let front_sq = sq(f as u8, front_r as u8);
                    if a.pawn[c.idx()][front_sq as usize] & enemy_pawns != 0 {
                        mg += sign * w.backward_pawn.0;
                        eg += sign * w.backward_pawn.1;

                        if enemy_pawns & mk.frente[c.idx()][s as usize] == 0 {
                            mg += sign * w.backward_exposed.0;
                            eg += sign * w.backward_exposed.1;
                        }
                    }
                }

                // Passado candidato.
                if enemy_pawns & (FILE_A << f) == 0 {
                    let enemy_ahead = count(enemy_pawns & mk.adj_frente[c.idx()][s as usize]);
                    let own_support = count(own_pawns & mk.adj_tras_incl[c.idx()][s as usize]);
                    if enemy_ahead >= 1 && enemy_ahead <= own_support {
                        let defenders = count(a.pawn[c.opp().idx()][s as usize] & own_pawns);
                        let threats = count(a.pawn[c.idx()][s as usize] & enemy_pawns);
                        let defended = defenders >= threats;
                        let cp = w.candidate_passer[defended as usize][rel_rank];
                        mg += sign * cp.0;
                        eg += sign * cp.1;
                    }
                }
            }

            // Peao isolado.
            //
            // Nao penalizar a fraqueza quando o peao ja' esta' sob ataque de
            // peao: o problema dele ja' esta' contado pelo termo de ameacas.
            //
            // CUIDADO -- isto e' de UMA referencia so'. A outra penaliza sempre,
            // sem condicao. Pela nossa propria regra (o que ambas fazem e'
            // estrutura, o que so' uma faz e' escolha) isto NAO se adopta as
            // cegas: fica atras de interruptor ate' ser medido.
            // KESTREL_FRAQUEZA_SO_SE_LIVRE=1 liga-o.
            let so_se_livre = {
                static F: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
                *F.get_or_init(|| std::env::var_os("KESTREL_FRAQUEZA_SO_SE_LIVRE").is_some())
            };
            let sob_ataque_de_peao = so_se_livre
                && a.pawn[c.idx()][s as usize] & enemy_pawns != 0;
            let has_neighbor = own_pawns & mk.adj_colunas[s as usize] != 0;
            if !has_neighbor && !sob_ataque_de_peao {
                let edge_idx = (f.min(7 - f)) as usize;
                mg += sign * w.isolated_pawn[edge_idx].0;
                eg += sign * w.isolated_pawn[edge_idx].1;

                if enemy_pawns & mk.frente[c.idx()][s as usize] == 0 {
                    mg += sign * w.isolated_exposed.0;
                    eg += sign * w.isolated_exposed.1;
                }
            }

            // Peao defendido por outro peao proprio.
            if a.pawn[c.opp().idx()][s as usize] & own_pawns != 0 {
                mg += sign * w.defended_pawn[rel_rank].0;
                eg += sign * w.defended_pawn[rel_rank].1;
            }

            // Falange.
            if own_pawns & mk.adj_mesma_fila[s as usize] != 0 {
                mg += sign * w.pawn_phalanx[rel_rank].0;
                eg += sign * w.pawn_phalanx[rel_rank].1;
            }
        }

        // Peoes dobrados.
        for file in 0..8u32 {
            let n = count(own_pawns & (FILE_A << file)) as i32;
            if n > 1 {
                let edge_idx = (file.min(7 - file)) as usize;
                mg += sign * w.doubled_pawn[edge_idx].0 * (n - 1);
                eg += sign * w.doubled_pawn[edge_idx].1 * (n - 1);
            }
        }
    }

    (mg, eg, passados)
}

/// Consulta a cache; em falha calcula e guarda. Substituicao sempre (a
/// entrada nova e' a mais provavel de voltar a ser pedida).
///
/// Os pesos `w` sao carregados uma vez no arranque do processo e nunca
/// mudam depois -- e' o que permite guardar o VALOR e nao so' a forma.
#[inline]
fn estrutura_de_peoes_cache(board: &Board, w: &Weights) -> (i32, i32, [Bitboard; 2]) {
    let chave = chave_peoes(board);
    let geracao = GERACAO_PESOS.load(std::sync::atomic::Ordering::Relaxed);
    let idx = (chave as usize) & ((1 << CACHE_PEOES_BITS) - 1);
    CACHE_PEOES.with(|t| {
        let mut t = t.borrow_mut();
        let e = t[idx];
        if e.chave == chave && e.geracao == geracao {
            return (e.mg, e.eg, e.passados);
        }
        let (mg, eg, passados) = estrutura_de_peoes(board, w);
        t[idx] = EntradaPeoes { chave, geracao, mg, eg, passados };
        (mg, eg, passados)
    })
}

pub fn positional_terms(board: &Board, w: &Weights) -> i32 {
    let a = atk();
    let occ = board.occ_all;
    let mut mg = 0i32;
    let mut eg = 0i32;

    // === EvalData: agregados de ataque por cor ===
    // Bitboards acumulados por cor para: casas atacadas por cada tipo
    // de peca (attacked_by_pt), casas atacadas total (attacked), casas
    // atacadas por 2 ou mais pecas (attacked_by_2). Precisamos destes
    // agregados para calcular ameacas correctamente: "peca inimiga
    // defendida" = casa atacada 2x por eles OU atacada 1x por peao
    // deles OU atacada por qualquer peca deles e nao atacada 2x por nos.
    let mut attacked_by_pt: [[Bitboard; 6]; 2] = [[0; 6]; 2];
    let mut attacked: [Bitboard; 2] = [0; 2];
    let mut attacked_by_2: [Bitboard; 2] = [0; 2];

    // Peoes: ataques diagonais em massa via shifts.
    for c in [Color::White, Color::Black] {
        let pa = pawn_attacks_by(board, c);
        attacked_by_2[c.idx()] |= attacked[c.idx()] & pa;
        attacked[c.idx()] |= pa;
        attacked_by_pt[c.idx()][PieceType::Pawn.idx()] |= pa;

        // Rei.
        let ks = board.king_sq(c);
        let ka = a.king[ks as usize];
        attacked_by_2[c.idx()] |= attacked[c.idx()] & ka;
        attacked[c.idx()] |= ka;
        attacked_by_pt[c.idx()][PieceType::King.idx()] |= ka;
    }

    // Attack-span dos peoes: todas as casas que os peoes de uma cor podem VIR
    // a atacar, propagadas para a frente. As duas referencias usam isto para
    // decidir o posto avancado; nos olhavamos apenas se EXISTE peao inimigo na
    // coluna adjacente, que e' diferente -- um peao que ja' passou o posto
    // nunca mais o pode atacar, e para nos continuava a invalida-lo.
    let span = {
        let wp = board.pieces[0][PieceType::Pawn.idx()];
        let bp = board.pieces[1][PieceType::Pawn.idx()];
        let mut w_at = ((wp & !FILE_A) << 7) | ((wp & !(FILE_A << 7)) << 9);
        let mut b_at = ((bp & !FILE_A) >> 9) | ((bp & !(FILE_A << 7)) >> 7);
        w_at |= w_at << 8; w_at |= w_at << 16; w_at |= w_at << 32;
        b_at |= b_at >> 8; b_at |= b_at >> 16; b_at |= b_at >> 32;
        [w_at, b_at]
    };

    // Peoes proprios travados: tem algo imediatamente a' frente e nao se
    // podem mexer. Sao casas mortas para efeito de mobilidade.
    let peoes_travados = [
        board.pieces[0][PieceType::Pawn.idx()] & (board.occ_all >> 8),
        board.pieces[1][PieceType::Pawn.idx()] & (board.occ_all << 8),
    ];

    // Pecas cravadas de cada cor, uma vez por avaliacao. A geracao de lances
    // ja' sabe calcula-las; aqui servem para nao dar mobilidade a quem nao a
    // tem.
    let pinned_de = [
        crate::movegen::compute_pinned(board, a, Color::White, board.king_sq(Color::White)),
        crate::movegen::compute_pinned(board, a, Color::Black, board.king_sq(Color::Black)),
    ];

    // === Tropismo: onde esta o jogo ===========================================
    //
    // Todos os outros termos de rei contam casas ATACADAS. Nenhum repara numa
    // peca a duas casas do rei inimigo que ainda nao ataca nada -- que e'
    // precisamente a peca prestes a decidir o jogo. A mobilidade ate agrava
    // isto: uma torre com dezoito casas num flanco morto pontua melhor que uma
    // com seis apontada ao rei.
    //
    // So' com dama no tabuleiro: sem ela nao ha ataque de mate a preparar e
    // aproximar pecas do rei inimigo e' so' descentralizacao.
    for c in [Color::White, Color::Black] {
        let us = c.idx();
        let them = c.opp().idx();
        if board.pieces[us][PieceType::Queen.idx()] == 0 {
            continue;
        }
        let sign = if c == Color::White { 1 } else { -1 };
        let ksq = board.king_sq(c.opp());
        let kf = (ksq as i32) & 7;
        let kr = (ksq as i32) >> 3;
        for (wi, pt) in [
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Rook,
            PieceType::Queen,
        ]
        .iter()
        .enumerate()
        .map(|(i, p)| (i, *p))
        {
            let mut bb = board.pieces[us][pt.idx()];
            while bb != 0 {
                let sq = bb.trailing_zeros() as i32;
                bb &= bb - 1;
                let df = ((sq & 7) - kf).abs();
                let dr = ((sq >> 3) - kr).abs();
                // Distancia de rei (Chebyshev): 7 e' o canto oposto, 1 e'
                // encostado. O bonus cresce com a proximidade.
                let close = 7 - df.max(dr);
                mg += sign * w.king_tropism[wi].0 * close;
                eg += sign * w.king_tropism[wi].1 * close;
            }
        }
        let _ = them;
    }



    let white_king_zone = king_zone(board.king_sq(Color::White));
    let black_king_zone = king_zone(board.king_sq(Color::Black));

    // Indexed by the ATTACKING color (not the color whose king is in
    // danger) -- king_attack_units[White] is how much White's pieces
    // threaten Black's king, contributed with White's (+1) sign below,
    // same convention the code already used locally before this was
    // hoisted out of the loop.
    let mut king_attackers = [0i32; 2];
    let mut king_attack_units = [(0i32, 0i32); 2];
    // How damaged each king's own pawn cover is (negative). Collected in the
    // shelter pass below and then handed to the OPPONENT as danger, so that
    // a broken shelter and pieces bearing down on it go through the same
    // curve together instead of being added up side by side. A king whose
    // cover is gone and who is also under attack is in far more trouble than
    // the sum of those two facts, and only a shared non-linearity says so.
    let mut shelter_penalty = [(0i32, 0i32); 2];

    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let own = board.occ_color[c.idx()];
        let enemy_king_zone = if c == Color::White { black_king_zone } else { white_king_zone };
        let ci = c.idx();

        // Desequilibrio material: produto cruzado das nossas pecas pelas dele.
        // Uma soma de valores fixos por peca nao consegue dizer que dois
        // cavalos valem coisas diferentes conforme o que esta' do outro lado.
        for i in 0..5 {
            let nossas = count(board.pieces[c.idx()][i]) as i32;
            if nossas == 0 { continue; }
            for j in 0..5 {
                let delas = count(board.pieces[c.opp().idx()][j]) as i32;
                if delas == 0 { continue; }
                let t = w.imbalance[i][j];
                mg += sign * t.0 * nossas * delas;
                eg += sign * t.1 * nossas * delas;
            }
        }

        if count(board.pieces[c.idx()][PieceType::Bishop.idx()]) >= 2 {
            mg += sign * w.bishop_pair.0;
            eg += sign * w.bishop_pair.1;
        }

        for pt in [PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen] {
            let mut bbp = board.pieces[c.idx()][pt.idx()];
            while bbp != 0 {
                let s = bbp.trailing_zeros() as Square;
                bbp &= bbp - 1;
                // Ocupacao com RAIOS-X entre pecas proprias da mesma linha.
                //
                // Dois bispos na mesma diagonal, ou torre e dama na mesma
                // coluna, veem atraves um do outro: a de tras exerce pressao
                // atraves da da frente, e sai para la' assim que a primeira se
                // mexa. Contar a da frente como bloqueio e' contar uma bateria
                // como se fosse uma peca so'. E' o que os motores fortes fazem
                // e nos usavamos a ocupacao inteira.
                let occ_xray = match pt {
                    PieceType::Bishop => occ & !(board.pieces[ci][PieceType::Bishop.idx()]
                        | board.pieces[ci][PieceType::Queen.idx()]),
                    PieceType::Rook => occ & !(board.pieces[ci][PieceType::Rook.idx()]
                        | board.pieces[ci][PieceType::Queen.idx()]),
                    PieceType::Queen => occ & !(board.pieces[ci][PieceType::Bishop.idx()]
                        | board.pieces[ci][PieceType::Rook.idx()]),
                    _ => occ,
                };
                let mut attacks = match pt {
                    PieceType::Knight => a.knight[s as usize],
                    PieceType::Bishop => bishop_attacks(s, occ_xray),
                    PieceType::Rook => rook_attacks(s, occ_xray),
                    PieceType::Queen => queen_attacks(s, occ_xray),
                    _ => 0,
                };
                // Uma peca CRAVADA so' se pode mexer ao longo da linha da
                // cravada. Contar-lhe a mobilidade toda e' dar-lhe casas onde
                // ela nao pode por os pes sem perder o rei.
                if bb(s) & pinned_de[ci] != 0 {
                    attacks &= a.between[s as usize][board.king_sq(c) as usize] | bb(board.king_sq(c));
                }
                // Registar em EvalData para a fase de threats abaixo.
                attacked_by_2[c.idx()] |= attacked[c.idx()] & attacks;
                attacked[c.idx()] |= attacks;
                attacked_by_pt[c.idx()][pt.idx()] |= attacks;

                // Mobility area excludes squares attacked by enemy
                // pawns (moving there just hangs the piece for a pawn,
                // not real mobility) as well as own-occupied squares.
                // Standard refinement: the "mobility area".
                // Area de mobilidade, na forma estabelecida: fora as casas
                // atacadas por peoes inimigos, fora a casa do proprio rei, e
                // fora os PEOES PROPRIOS TRAVADOS -- peoes com algo em frente
                // sao casas mortas e uma peca que os "ataca" nao ganha
                // mobilidade nenhuma com isso.
                //
                // Nos excluiamos TODAS as pecas proprias, o que e' diferente:
                // uma torre que cobre a casa do proprio bispo tem essa casa
                // disponivel assim que o bispo saia, e a forma estabelecida
                // conta-a. Excluiamos casas a mais e casas a menos ao mesmo
                // tempo.
                let enemy_pawn_attacks = attacked_by_pt[c.opp().idx()][PieceType::Pawn.idx()];
                let area = !enemy_pawn_attacks & !bb(board.king_sq(c)) & !peoes_travados[ci];
                let mobility = count(attacks & area) as usize;
                let mob_table = match pt {
                    PieceType::Knight => &w.mobility_knight,
                    PieceType::Bishop => &w.mobility_bishop,
                    PieceType::Rook => &w.mobility_rook,
                    PieceType::Queen => &w.mobility_queen,
                    _ => &w.mobility_knight,
                };
                let m = mob_table[mobility.min(27)];
                mg += sign * m.0;
                eg += sign * m.1;

                let hits = count(attacks & enemy_king_zone) as i32;
                if hits > 0 {
                    king_attackers[ci] += 1;
                    let widx = match pt {
                        PieceType::Knight => 0,
                        PieceType::Bishop => 1,
                        PieceType::Rook => 2,
                        PieceType::Queen => 3,
                        _ => 0,
                    };
                    let aw = w.king_attacker_weight[widx];
                    king_attack_units[ci].0 += aw.0 + hits * w.king_attacks.0;
                    king_attack_units[ci].1 += aw.1 + hits * w.king_attacks.1;
                }

                if pt == PieceType::Rook {
                    let file_mask = FILE_A << file_of(s);
                    let own_pawns_on_file = board.pieces[c.idx()][PieceType::Pawn.idx()] & file_mask;
                    let enemy_pawns_on_file = board.pieces[c.opp().idx()][PieceType::Pawn.idx()] & file_mask;
                    if own_pawns_on_file == 0 {
                        let idx = if enemy_pawns_on_file == 0 { 0 } else { 1 };
                        mg += sign * w.rook_open[idx].0;
                        eg += sign * w.rook_open[idx].1;
                    }
                    // ROOK_ON_SEVENTH: rank 7 relative (a standalone
                    // term, independent of open-file status
                    // -- see const doc comment).
                    let rel_rank = if c == Color::White { rank_of(s) } else { 7 - rank_of(s) };
                    if rel_rank == 6 {
                        mg += sign * w.rook_on_seventh.0;
                        eg += sign * w.rook_on_seventh.1;
                    }
                }

                if pt == PieceType::Knight || pt == PieceType::Bishop {
                    let f = file_of(s) as i32;
                    let r = rank_of(s) as i32;
                    let front_r = if c == Color::White { r + 1 } else { r - 1 };
                    if (0..8).contains(&front_r)
                        // Peao de QUALQUER cor. As duas referencias usam o
                        // conjunto de todos os peoes: uma peca menor abrigada
                        // atras de um peao inimigo esta' igualmente protegida
                        // de ser expulsa por peao, e nos exigiamos que fosse
                        // nosso.
                        && (board.pieces[0][PieceType::Pawn.idx()] | board.pieces[1][PieceType::Pawn.idx()])
                            & bb(sq(f as u8, front_r as u8)) != 0
                    {
                        mg += sign * w.minor_behind_pawn.0;
                        eg += sign * w.minor_behind_pawn.1;
                    }

                    if pt == PieceType::Knight {
                        let own_side_rank = if c == Color::White { r } else { 7 - r };
                        if (3..=5).contains(&own_side_rank) {
                            let defended = a.pawn[c.opp().idx()][s as usize] & board.pieces[c.idx()][PieceType::Pawn.idx()] != 0;
                            let ever_attackable = span[c.opp().idx()] & bb(s) != 0;
                            if defended && !ever_attackable {
                                mg += sign * w.knight_outpost.0;
                                eg += sign * w.knight_outpost.1;
                            }
                        }
                    }
                }

                if pt == PieceType::Bishop {
                    let center: Bitboard = bb(sq(3, 3)) | bb(sq(4, 3)) | bb(sq(3, 4)) | bb(sq(4, 4));
                    if count(attacks & center) >= 2 {
                        mg += sign * w.long_diag_bishop.0;
                        eg += sign * w.long_diag_bishop.1;
                    }

                    // Bispo mau: penalidade por cada peao proprio na
                    // mesma cor de casa que o bispo -- casas que essa
                    // peca nunca pode influenciar, muitas vezes tem de
                    // ser defendidas por outra peca em vez dele.
                    let bishop_light = (rank_of(s) + file_of(s)) % 2 == 1;
                    let own_pawns_same_color = if bishop_light { LIGHT_SQUARES } else { !LIGHT_SQUARES };
                    let n = (count(board.pieces[c.idx()][PieceType::Pawn.idx()] & own_pawns_same_color) as usize).min(6);
                    mg += sign * w.bishop_pawns[n].0;
                    eg += sign * w.bishop_pawns[n].1;
                }
            }
        }

        // === Pawn shelter / storm around own king ===
        // Missing piece found after a real bullet loss (2026-07-21,
        // GLUlNq1Q): White played g4/g5 in front of its own castled
        // king with no concrete follow-up, and lost material to ...Bxh3
        // a few moves later. At bullet depth the search never calculated
        // that far -- what should have stopped the push is the STATIC
        // eval already pricing in "my own king shield just moved 2
        // squares forward", the same way a human's intuition flags a
        // self-weakening pawn storm before calculating anything concrete.
        // This is a universal HCE component (shelter strength / unblocked
        // storm, encoded in some form by every strong classical eval);
        // values below are mine, reasoned from scratch:
        // an intact shield pawn one square ahead of the king costs
        // nothing, each extra square it advances trades king safety for
        // nothing in return, and an enemy pawn closing in on the king's
        // file is progressively more dangerous the closer it gets.
        let kf = file_of(board.king_sq(c)) as i32;
        let kr = rank_of(board.king_sq(c)) as i32;
        let white = c == Color::White;
        for f in (kf - 1).max(0)..=(kf + 1).min(7) {
            let file_mask = FILE_A << f;
            let own_pawns = board.pieces[c.idx()][PieceType::Pawn.idx()] & file_mask;
            let enemy_pawns = board.pieces[c.opp().idx()][PieceType::Pawn.idx()] & file_mask;
            match shield_pawn_offset(own_pawns, kr, white) {
                None => {
                    shelter_penalty[c.idx()].0 += w.shelter_open.0;
                    shelter_penalty[c.idx()].1 += w.shelter_open.1;
                }
                Some(off) => {
                    let idx = (off - 1).clamp(0, 3) as usize;
                    shelter_penalty[c.idx()].0 += w.pawn_shelter[idx].0;
                    shelter_penalty[c.idx()].1 += w.pawn_shelter[idx].1;
                }
            }
            if let Some(off) = shield_pawn_offset(enemy_pawns, kr, white) {
                let idx = (off - 1).clamp(0, 3) as usize;
                // Avanco TRANCADO: o nosso peao esta' imediatamente a' frente
                // do dele nesta coluna. Um peao que vem contra o rei so' e'
                // perigoso porque pode abrir a coluna; cabeca com cabeca nao
                // abre nada. Usar a tabela normal aqui pune uma ameaca que nao
                // existe -- e' a distincao que um dos motores de referencia faz
                // e nos nao faziamos.
                let trancado = match shield_pawn_offset(own_pawns, kr, white) {
                    Some(nosso_off) => nosso_off == off - 1,
                    None => false,
                };
                let t = if trancado { w.tornado_locked[idx] } else { w.pawn_tornado[idx] };
                shelter_penalty[c.idx()].0 += t.0;
                shelter_penalty[c.idx()].1 += t.1;
            }
        }
    }

    // === Safe checks + queen-gated king danger ===
    // Deferred to its own pass after both colors' attacked[]/
    // attacked_by_pt[] are fully known -- a "safe" square (no enemy
    // defender at all, conservative but simple) can only be judged once
    // the DEFENDING side's full attack set exists, which isn't true yet
    // mid-loop above when processing the attacking side first.
    // Stonewall.
    //
    // White holds c3-d4-e3-f4, Black c6-d5-e6-f5. What makes it a structure
    // rather than four pawns is that all four stand on ONE colour -- dark
    // for White, light for Black -- and that is a permanent commitment, not
    // a tendency. Three things follow, and the evaluation had no way of
    // seeing any of them as connected:
    //
    //   the square ahead of the chain (e5 / e4) can never be challenged by a
    //   pawn again, so a knight there is not an outpost that might be
    //   evicted, it is a fixture;
    //
    //   the bishop that travels on the pawns' own colour is behind them for
    //   the rest of the game;
    //
    //   the formation buys a kingside attack and sells long-term
    //   flexibility, which is why it is worth something while the pieces are
    //   on and rather less afterwards.
    //
    // Squares are named from White's side and mirrored for Black.
    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let us = c.idx();
        let pawns = board.pieces[us][PieceType::Pawn.idx()];
        let wall: [u8; 4] = if c == Color::White {
            [sq(2, 2), sq(3, 3), sq(4, 2), sq(5, 3)]   // c3 d4 e3 f4
        } else {
            [sq(2, 5), sq(3, 4), sq(4, 5), sq(5, 4)]   // c6 d5 e6 f5
        };
        if !wall.iter().all(|&x| pawns & bb(x) != 0) {
            continue;
        }
        mg += sign * w.stonewall.0;
        eg += sign * w.stonewall.1;

        let outpost = if c == Color::White { sq(4, 4) } else { sq(4, 3) };  // e5 / e4
        if board.pieces[us][PieceType::Knight.idx()] & bb(outpost) != 0 {
            mg += sign * w.stonewall_outpost.0;
            eg += sign * w.stonewall_outpost.1;
        }

        // The bishop sharing the pawns' colour. d4 is dark and d5 is light,
        // so testing one wall square tells us which one is shut in.
        let wall_is_dark = (file_of(wall[1]) + rank_of(wall[1])) % 2 == 0;
        let mut b = board.pieces[us][PieceType::Bishop.idx()];
        while b != 0 {
            let s = b.trailing_zeros() as Square;
            b &= b - 1;
            if ((file_of(s) + rank_of(s)) % 2 == 0) == wall_is_dark {
                mg += sign * w.stonewall_bad_bishop.0;
                eg += sign * w.stonewall_bad_bishop.1;
            }
        }
    }

    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let us = c.idx();
        let them = c.opp().idx();
        let enemy_king_sq = board.king_sq(c.opp());
        let own_occ = board.occ_color[us];
        // Conservative "safe": zero enemy defenders on the square at
        // all (not even the king). Undercounts some genuinely-safe
        // checks where we'd have enough backup to win the exchange,
        // but is cheap and never wrongly counts an unsafe one.
        // A square defended ONLY by the enemy king counts as weak: he cannot
        // both hold it and step off it, so a check landing there is not
        // really answerable. Squares we attack twice over such a defender are
        // ours too. The old rule (nothing of theirs touches it at all) is
        // strictly more conservative and threw away real checks -- its own
        // comment admitted as much.
        let weak = !attacked[them]
            | (!attacked_by_2[them] & attacked_by_pt[them][PieceType::King.idx()]);
        let safe = !own_occ & (!attacked[them] | (weak & attacked_by_2[us]));

        let knight_checks = a.knight[enemy_king_sq as usize];
        let bishop_checks = bishop_attacks(enemy_king_sq, occ);
        let rook_checks = rook_attacks(enemy_king_sq, occ);
        let queen_checks = bishop_checks | rook_checks;

        // Every square from which each piece type would give check, whether
        // or not we could survive there.
        let all_knight = knight_checks & attacked_by_pt[us][PieceType::Knight.idx()] & !own_occ;
        let all_bishop = bishop_checks & attacked_by_pt[us][PieceType::Bishop.idx()] & !own_occ;
        let all_rook = rook_checks & attacked_by_pt[us][PieceType::Rook.idx()] & !own_occ;
        let all_queen = queen_checks & attacked_by_pt[us][PieceType::Queen.idx()] & !own_occ;

        let n_knight = count(all_knight & safe) as i32;
        let n_bishop = count(all_bishop & safe) as i32;
        let n_rook = count(all_rook & safe) as i32;
        let n_queen = count(all_queen & safe) as i32;

        let u_knight = count(all_knight & !safe) as i32;
        let u_bishop = count(all_bishop & !safe) as i32;
        let u_rook = count(all_rook & !safe) as i32;
        let u_queen = count(all_queen & !safe) as i32;

        // Per-piece-type dedicated weights (2026-07-23, counter-intuitive
        // relative ordering -- see SAFE_KNIGHT_CHECK doc comment: rook/
        // knight weighted HIGHER than queen, the old flat weight with
        // an ad-hoc "queen counts double" multiplier is gone, replaced
        // by queen's own naturally-lower dedicated weight). First
        // version (single flat weight, reusing `king_attacks` directly)
        // gave a negative, persistent 46.8% self-play A/B on 2026-07-22
        // -- this field exists to be recalibrated independently of the
        // rest of king safety, same reasoning as before, now per piece
        // type instead of just one flat number.
        if n_knight + n_bishop + n_rook + n_queen > 0 {
            king_attackers[us] += 1;
            king_attack_units[us].0 += n_knight * w.safe_knight_check.0
                + n_bishop * w.safe_bishop_check.0
                + n_rook * w.safe_rook_check.0
                + n_queen * w.safe_queen_check.0;
            king_attack_units[us].1 += n_knight * w.safe_knight_check.1
                + n_bishop * w.safe_bishop_check.1
                + n_rook * w.safe_rook_check.1
                + n_queen * w.safe_queen_check.1;
        }
        // Checks he CAN answer, counted separately and for less: they still
        // force the reply and still tie him to watching the square.
        king_attack_units[us].0 += u_knight * w.unsafe_knight_check.0
            + u_bishop * w.unsafe_bishop_check.0
            + u_rook * w.unsafe_rook_check.0
            + u_queen * w.unsafe_queen_check.0;
        king_attack_units[us].1 += u_knight * w.unsafe_knight_check.1
            + u_bishop * w.unsafe_bishop_check.1
            + u_rook * w.unsafe_rook_check.1
            + u_queen * w.unsafe_queen_check.1;

        // Pieces frozen on a line to his king, and our own pieces sitting on
        // one. A sniper is any of our sliders that would reach his king on an
        // empty board; if exactly one man stands in the way, he is either his
        // (pinned, and unable to defend) or ours (a discovered check we can
        // spring when we choose).
        let our_bishops_queens = board.pieces[us][PieceType::Bishop.idx()]
            | board.pieces[us][PieceType::Queen.idx()];
        let our_rooks_queens = board.pieces[us][PieceType::Rook.idx()]
            | board.pieces[us][PieceType::Queen.idx()];
        let mut snipers = (bishop_attacks(enemy_king_sq, 0) & our_bishops_queens)
            | (rook_attacks(enemy_king_sq, 0) & our_rooks_queens);
        while snipers != 0 {
            let sniper_sq = snipers.trailing_zeros() as Square;
            snipers &= snipers - 1;
            let blockers = a.between[enemy_king_sq as usize][sniper_sq as usize] & occ;
            if blockers == 0 || (blockers & (blockers - 1)) != 0 {
                continue; // clear line, or more than one man in the way
            }
            let blocker_sq = blockers.trailing_zeros() as Square;
            let (sniper_pt, _) = match board.piece_at(sniper_sq) { Some(x) => x, None => continue };
            let (blocker_pt, blocker_color) = match board.piece_at(blocker_sq) { Some(x) => x, None => continue };
            // Sniper index: bishop, rook, queen.
            let si = match sniper_pt {
                PieceType::Bishop => 0,
                PieceType::Rook => 1,
                PieceType::Queen => 2,
                _ => continue,
            };
            let bi = blocker_pt.idx();
            if bi >= 5 {
                continue; // a king is never the man in the middle
            }
            let entry = if blocker_color.idx() == them {
                w.safety_pinned[bi][si]
            } else {
                w.safety_discovered[bi][si]
            };
            king_attack_units[us].0 += entry.0;
            king_attack_units[us].1 += entry.1;
        }

        // Attacking without a queen: said as units removed rather than as a
        // threshold, so it shades instead of switching.
        if board.pieces[us][PieceType::Queen.idx()] == 0 {
            king_attack_units[us].0 += w.queenless_attack.0;
            king_attack_units[us].1 += w.queenless_attack.1;
        }

        // WEAK_KING_RING: 2026-07-23, new -- count of the enemy
        // king-ring squares that are "weak" (not attacked by them at
        // all, or only defended by their own king with no second
        // defender) and applied directly, unconditionally (not gated by
        // the attacker-count threshold below).
        let enemy_king_zone = if c == Color::White { black_king_zone } else { white_king_zone };
        // `weak` is the same set computed above for the safe-check rule.
        let weak_king_ring = count(enemy_king_zone & weak) as i32;
        king_attack_units[us].0 += w.weak_king_ring.0 * weak_king_ring;
        king_attack_units[us].1 += w.weak_king_ring.1 * weak_king_ring;

        // KING_FLANK_ATTACKS/DEFENSES: 2026-07-23, new -- wide-zone
        // version of the same "attacked"/"defended" counting, over the
        // enemy king's whole flank, not just its immediate ring. Also
        // applied unconditionally.
        let their_flank = king_flank(enemy_king_sq, c.opp());
        let flank_attacks = count(their_flank & attacked[us]) as i32;
        let flank_attacks_2 = count(their_flank & attacked_by_2[us]) as i32;
        let flank_defenses = count(their_flank & attacked[them]) as i32;
        let flank_defenses_2 = count(their_flank & attacked_by_2[them]) as i32;
        king_attack_units[us].0 += flank_attacks * w.king_flank_attacks[0].0
            + flank_attacks_2 * w.king_flank_attacks[1].0
            + flank_defenses * w.king_flank_defenses[0].0
            + flank_defenses_2 * w.king_flank_defenses[1].0;
        king_attack_units[us].1 += flank_attacks * w.king_flank_attacks[0].1
            + flank_attacks_2 * w.king_flank_attacks[1].1
            + flank_defenses * w.king_flank_defenses[0].1
            + flank_defenses_2 * w.king_flank_defenses[1].1;

        // The attacker-count threshold that used to gate this block is gone.
        // It was a cliff: one attacker short of it and the whole danger term
        // contributed exactly zero, leaving the evaluation blind to a king
        // that is merely uncomfortable -- and it fired on a quarter of the
        // positions in our own games. What it was really expressing (an
        // attack without a queen rarely finishes) is now a continuous term,
        // QUEENLESS_ATTACK, applied in units above.
        //
        // king_attackers is still counted: the endgame scale factor reads it.
        // KESTREL_KING_NOGATE: drop the attacker-count threshold, so the
        // danger curve applies always instead of switching on at one or two
        // attackers.
        //
        // The threshold is a cliff: one attacker short of it, this whole
        // block contributes exactly zero, and the evaluation is blind to a
        // king that is merely uncomfortable. Measuring our evaluation
        // against a stronger hand-crafted one showed king safety accounting
        // for less than half the share of the total that it does for them,
        // and scaling the block's weight failed in both directions (-58 Elo
        // at 2.0, -60 at 1.4, and 0.7 no worse than 1.0) -- which points at
        // the term's shape rather than its size. This cliff is the most
        // obvious piece of shape we have and they do not.
        // The danger curve now applies always. Negative unit totals (a king
        // with nothing pointed at him, after the queenless deduction) map to
        // index 0 and contribute nothing, so the curve stays one-sided
        // without needing a branch to say so.
        // His shelter damage is our danger.
        king_attack_units[us].0 -= shelter_penalty[them].0;
        king_attack_units[us].1 -= shelter_penalty[them].1;

        // The offset goes in HERE, before the curve, not after it. Added
        // afterwards it would be a constant that cancels between the two
        // sides and changes nothing; added before, it decides which part of
        // the curve the attack is measured on, which is the entire point.
        mg += sign * king_danger_curve(king_attack_units[us].0 + w.king_safety_offset.0);
        eg += sign * king_danger_curve(king_attack_units[us].1 + w.king_safety_offset.1);

        // UNCASTLED_KING: see const doc comment -- added after real
        // games showed Kestrel castling late and outright failing to
        // castle in 3/14 recent games.
        let home_sq = if c == Color::White { sq(4, 0) } else { sq(4, 7) };
        if board.king_sq(c) == home_sq {
            let (king_flag, queen_flag) = if c == Color::White {
                (crate::board::CASTLE_WK, crate::board::CASTLE_WQ)
            } else {
                (crate::board::CASTLE_BK, crate::board::CASTLE_BQ)
            };
            if board.castling & (king_flag | queen_flag) == 0 {
                mg += sign * w.uncastled_king_no_rights.0;
                eg += sign * w.uncastled_king_no_rights.1;
            } else {
                mg += sign * w.uncastled_king_has_rights.0;
                eg += sign * w.uncastled_king_has_rights.1;
            }
        }
    }

    // === Ameacas ===
    // Aplica por cor: bonus para cada peca inimiga que a nossa peca de
    // tipo X ATACA, indexada pelo tipo alvo e por "defended".
    // defended = attackedBy2[them] | attackedBy[them][PAWN] |
    //            (attacked[them] & ~attackedBy2[us])
    // (a intuicao: peca inimiga esta "defendida" se qq peca ou peao
    // deles defende, EXCEPTO quando nos temos MAIS atacantes que eles
    // defensores.)
    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let us = c.idx();
        let them = c.opp().idx();
        let their_pieces = board.occ_color[them];
        let their_queen = board.pieces[them][PieceType::Queen.idx()];
        let their_king = board.pieces[them][PieceType::King.idx()];

    // === Miras: pecas de longo alcance apontadas ao rei ATRAVES de bloqueios ==
    //
    // What a game is lost to is rarely a move; it is a plan, played one move at
    // a time while nothing looks wrong. Traced from a real loss: over sixteen
    // moves the opponent played Qc2, Qb3, Qa7, b5, Qa5, Bc6, Bb7, Qa8, lining
    // a queen and a bishop up on the long diagonal against our king, and then
    // Bxd5+ turned +1.6 into a lost position. The evaluation never saw any of
    // it coming. A bishop on b7 whose diagonal is blocked at d5 does not
    // attack the king zone, so it counted as nothing at all, and the queen
    // waiting behind it counted as nothing twice.
    //
    // The aiming is the fact worth pricing, not the arrival. A slider that
    // shares a line with the enemy king is committed to that king: every
    // blocker on the line is a piece that can be traded, deflected, or
    // captured with check, and the defender has to keep answering that
    // question for the rest of the game. So the score is by how much still
    // stands in the way, and the last blocker is worth far more than the
    // first -- one square from opening is a threat, three is a shape.
    //
    // A second slider on the SAME line is the pattern itself: a battery is
    // worth more than the two pieces apart, because removing one blocker
    // exposes the king to both.
    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let us = c.idx();
        let them = c.opp().idx();
        let their_king_bb = board.pieces[them][PieceType::King.idx()];
        if their_king_bb == 0 {
            continue;
        }
        let ksq = their_king_bb.trailing_zeros() as u8;
        let occ_all = board.occ_color[0] | board.occ_color[1];
        // Lines already counted once, so a battery is scored as a battery
        // rather than as two independent sliders aiming at the same king.
        let mut seen_lines: u64 = 0;
        for pt in [PieceType::Bishop, PieceType::Rook, PieceType::Queen] {
            let mut bb = board.pieces[us][pt.idx()];
            while bb != 0 {
                let sq = bb.trailing_zeros() as u8;
                bb &= bb - 1;
                // Does this piece's MOVEMENT share a line with the king --
                // ignoring everything in between? That is the question the
                // direct-attack terms cannot ask.
                //
                // Asked with arithmetic, not with magic lookups. The first
                // version generated each slider's attacks on an empty board
                // and intersected with the king; correct, and it cost 6% of
                // the engine's speed for a question that is two comparisons.
                // `between` is empty for squares that do not share a line, so
                // it doubles as the alignment test.
                let line = a.between[sq as usize][ksq as usize];
                if line == 0 {
                    continue;
                }
                let (sf, sr) = (file_of(sq) as i32, rank_of(sq) as i32);
                let (kf, kr) = (file_of(ksq) as i32, rank_of(ksq) as i32);
                let diagonal = (sf - kf).abs() == (sr - kr).abs();
                let straight = sf == kf || sr == kr;
                let can_travel = match pt {
                    PieceType::Bishop => diagonal,
                    PieceType::Rook => straight,
                    _ => diagonal || straight,
                };
                if !can_travel {
                    continue;
                }
                let blockers = (line & occ_all).count_ones() as usize;
                if blockers == 0 {
                    // Already a check or a direct attack: the existing king
                    // safety terms own this case, and paying twice for it
                    // would double-count the one thing we do see.
                    continue;
                }
                let idx = (blockers - 1).min(2);
                let (mut m, mut e) = w.king_aim[idx];
                // A line a second slider already claims. Scored once more, as
                // the battery it is.
                let line_key = line | (1u64 << ksq);
                if seen_lines & line_key == line_key && line != 0 {
                    let (bm, be) = w.king_battery;
                    m += bm;
                    e += be;
                }
                seen_lines |= line_key;
                mg += sign * m;
                eg += sign * e;
            }
        }
    }

    // === Pecas penduradas, avaliadas por SEE ==================================
    //
    // So' para o lado que NAO joga a seguir: quem tem o lance foge com a peca,
    // quem nao tem vai perde-la.
    //
    // A primeira versao disto usava uma constante por tipo de peca: -58 para um
    // bispo que vale 355. Numa posicao real (Bxf7+ na italiana) a avaliacao dava
    // -73 quando a verdade, depois da recaptura, era -215. Um erro de 142
    // centipeoes -- maior do que QUALQUER margem de poda que temos (35 por ply
    // no RFP, 265 no null move). Toda a poda de no inteiro estava a decidir
    // sobre um numero que podia estar errado por mais do que a margem contra a
    // qual era comparado.
    //
    // SEE responde exactamente a pergunta certa: jogando a melhor captura sobre
    // esta peca, e trocando de forma optima ate ao fim, quanto se perde? Uma
    // peca defendida por igual da zero; uma pendurada da o seu valor inteiro.
    //
    // Uma rede neuronal aprende isto sozinha a partir de resultados; uma
    // avaliacao escrita a mao tem de o dizer. E' a diferenca de metodo, nao de
    // conhecimento.
    {
        // Both sides, when asked for.
        //
        // The reasoning for one side only -- "whoever has the move runs away
        // with the piece" -- is search reasoning: it assumes something will
        // look one ply ahead and see the escape. With no search there is
        // nothing to see it, and the evaluation goes blind to half the board.
        // The side to move still counts for less, because it CAN run: a piece
        // it can save is not a piece it has lost.
        for (victim, share) in see_both_sides(board.side) {
            if share == 0 {
                continue;
            }
        let vi = victim.idx();
        let ai = victim.opp().idx();
        let sign = if victim == Color::White { 1 } else { -1 };
        let mut worst = 0i32;
        let mut worst_pt = PieceType::Pawn;
        for pt in [
            PieceType::Pawn,
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Rook,
            PieceType::Queen,
        ] {
            let mut bb = board.pieces[vi][pt.idx()] & attacked[ai];
            while bb != 0 {
                let sq = bb.trailing_zeros() as u8;
                bb &= bb - 1;
                // A captura mais barata sobre esta casa e' a que o adversario
                // vai escolher, e e' a que SEE avalia a partir da lista de
                // trocas completa.
                let atk_bb = crate::search::see::attackers_to(a, board, sq, occ)
                    & board.occ_color[ai];
                if atk_bb == 0 {
                    continue;
                }
                // A cor tem de ser a de QUEM ATACA. Era `board.side`, o que
                // e' o mesmo enquanto so' se avalia o lado que nao joga (o
                // caminho por omissao), mas com `KESTREL_SEE_BOTH=1` a
                // interseccao dava vazia para o segundo lado e a peca era
                // saltada em silencio -- a feature nao fazia nada.
                if let Some((from, _)) =
                    crate::search::see::least_valuable_attacker(board, atk_bb, victim.opp())
                {
                    let mv = crate::moves::Move {
                        from,
                        to: sq,
                        promotion: None,
                        flag: crate::moves::MoveFlag::Capture,
                    };
                    let gain = crate::search::see::see(a, board, &mv);
                    // So' a PIOR ameaca conta. Somar todas contaria varias
                    // perdas que nunca acontecem juntas: so' se perde uma peca
                    // por lance.
                    if gain > worst {
                        worst = gain;
                        worst_pt = pt;
                    }
                }
            }
        }
        if worst > 0 && debug_see() {
            eprintln!("debug: worst SEE = {worst}");
        }
        if worst > 0 {
            // Fraccao, nao o valor inteiro: ha xeques intermedios, contra-jogo
            // e fugas que o SEE estatico nao ve. A busca resolve o resto -- isto
            // so' tem de deixar a avaliacao estatica honesta o suficiente para
            // as margens de poda fazerem sentido.
            // Through the weight, not through a constant. The fraction was
            // hardcoded at 3/4, which made this term invisible to the feature
            // extractor -- probing a weight did not move it -- and untunable by
            // anything. In thousandths, so 750 is exactly what the constant
            // was: the defaults reproduce the old behaviour to the unit.
            let (hm, he) = w.hanging[worst_pt.idx().min(4)];
            mg += sign * -(worst * hm / 1000) * share / 1000;
            eg += sign * -(worst * he / 1000) * share / 1000;
        }
        }
    }

    // === Espaco e peoes travados ===============================================
    //
    // O termo que decide posicoes fechadas, e que nenhum dos dois motores tinha.
    // Medido nos 214 erros reais: erramos o DOBRO em estruturas travadas. Num
    // tabuleiro fechado nao ha tactica para encontrar -- ganha quem tem onde por
    // as pecas, e a mobilidade nao mede isso (conta casas alcancaveis AGORA, que
    // num bloqueio sao poucas para ambos).
    //
    // Espaco = casas na nossa metade, nas colunas centrais, atras dos nossos
    // peoes, que o adversario nao ataca. Vale mais quando ha muitas pecas para
    // manobrar e quando o tabuleiro esta de facto travado, portanto o bonus e'
    // multiplicado pelo numero de peoes bloqueados.
    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let us = c.idx();
        let them = c.opp().idx();
        let our_pawns = board.pieces[us][PieceType::Pawn.idx()];
        let their_pawns = board.pieces[them][PieceType::Pawn.idx()];

        // Peoes nossos travados de frente contra um peao deles.
        let blocked = if c == Color::White {
            our_pawns & (their_pawns >> 8)
        } else {
            our_pawns & (their_pawns << 8)
        };
        let n_blocked = blocked.count_ones() as i32;
        mg += sign * w.blocked_pawns.0 * n_blocked;
        eg += sign * w.blocked_pawns.1 * n_blocked;

        // Colunas c a f, fileiras 2-4 do nosso lado: onde as pecas manobram.
        const CENTRE_FILES: Bitboard = (FILE_A << 2) | (FILE_A << 3) | (FILE_A << 4) | (FILE_A << 5);
        let home = if c == Color::White {
            RANK_2 | RANK_3 | RANK_4
        } else {
            RANK_7 | RANK_6 | RANK_5
        };
        // Atras dos nossos peoes: a casa esta vazia e ha um peao nosso a
        // frente dela na mesma coluna.
        let behind = if c == Color::White {
            (our_pawns >> 8) | (our_pawns >> 16)
        } else {
            (our_pawns << 8) | (our_pawns << 16)
        };
        let space = home & CENTRE_FILES & behind & !occ & !attacked[them];
        // Escalado pelo bloqueio: espaco vale pouco num tabuleiro aberto, onde
        // as pecas chegam a qualquer lado de qualquer maneira.
        let weight = 1 + n_blocked;
        mg += sign * w.space.0 * space.count_ones() as i32 * weight / 2;
        eg += sign * w.space.1 * space.count_ones() as i32 * weight / 2;
    }

        let defended_bb: Bitboard = attacked_by_2[them]
            | attacked_by_pt[them][PieceType::Pawn.idx()]
            | (attacked[them] & !attacked_by_2[us]);

        // Threats por peao -- agora indexed por defended tambem.
        let mut t = attacked_by_pt[us][PieceType::Pawn.idx()] & their_pieces;
        while t != 0 {
            let s = t.trailing_zeros() as Square;
            t &= t - 1;
            let defended = (defended_bb & bb(s)) != 0;
            if let Some((pt, _)) = board.piece_at(s) {
                let entry = w.threat_by_pawn[defended as usize][pt.idx()];
                mg += sign * entry.0;
                eg += sign * entry.1;
            }
        }
        // Threats por cavalo/bispo/torre/dama.
        for (pt_us, table) in [
            (PieceType::Knight, &w.threat_by_knight),
            (PieceType::Bishop, &w.threat_by_bishop),
            (PieceType::Rook, &w.threat_by_rook),
            (PieceType::Queen, &w.threat_by_queen),
        ] {
            let mut t = attacked_by_pt[us][pt_us.idx()] & their_pieces;
            // Dama nao conta ameacas ao rei (o mate cobre isso).
            if pt_us == PieceType::Queen {
                t &= !their_king;
            }
            while t != 0 {
                let s = t.trailing_zeros() as Square;
                t &= t - 1;
                let defended = (defended_bb & bb(s)) != 0;
                if let Some((tgt, _)) = board.piece_at(s) {
                    let entry = table[defended as usize][tgt.idx()];
                    mg += sign * entry.0;
                    eg += sign * entry.1;
                }
            }
        }
        // Threats por rei -- so' contra pecas nao-defendidas.
        let mut t = attacked_by_pt[us][PieceType::King.idx()] & their_pieces & !defended_bb;
        while t != 0 {
            let s = t.trailing_zeros() as Square;
            t &= t - 1;
            if let Some((pt, _)) = board.piece_at(s) {
                mg += sign * w.threat_by_king[pt.idx()].0;
                eg += sign * w.threat_by_king[pt.idx()].1;
            }
        }

        // Restricted squares: casas onde nos temos 2+ atacantes, eles
        // nao tem 2+, mas eles atacam pelo menos 1 vez:
        // attackedBy2[us] & ~attackedBy2[them] & attacked[them].
        let restricted = attacked_by_2[us] & !attacked_by_2[them] & attacked[them];
        let n_restr = count(restricted) as i32;
        mg += sign * w.restricted_squares.0 * n_restr;
        eg += sign * w.restricted_squares.1 * n_restr;

        // Push threats: um peao nosso pode avancar 1 casa (ou 2 se
        // ainda esta' na fileira inicial) para uma casa "segura" e
        // ATACAR uma peca nao-peao inimiga a partir dai. `safe` =
        // casas nao defendidas OU casas onde nos temos mais atacantes.
        let empty = !occ;
        let own_pawns = board.pieces[us][PieceType::Pawn.idx()];
        let one_push = if c == Color::White {
            (own_pawns << 8) & empty
        } else {
            (own_pawns >> 8) & empty
        };
        // Second push (para peoes na fileira inicial, o "empurrao
        // duplo"): sobre o subconjunto do one_push que caiu na 3a
        // fileira relativa.
        let rank3_bb: Bitboard = if c == Color::White { RANK_3 } else { RANK_6 };
        let two_push = if c == Color::White {
            ((one_push & rank3_bb) << 8) & empty
        } else {
            ((one_push & rank3_bb) >> 8) & empty
        };
        let pushes = one_push | two_push;
        let safe = !defended_bb
            | (attacked[us] & !attacked_by_pt[them][PieceType::Pawn.idx()] & !attacked_by_2[them]);
        let safe_pushes = pushes & safe;
        // Casas atacadas por peoes-nossos-simulados-nas-safe_pushes:
        let push_attacks_on_enemy = if c == Color::White {
            ((safe_pushes & !FILE_A) << 7) | ((safe_pushes & !FILE_H) << 9)
        } else {
            ((safe_pushes & !FILE_A) >> 9) | ((safe_pushes & !FILE_H) >> 7)
        };
        let non_pawn_enemies = their_pieces & !board.pieces[them][PieceType::Pawn.idx()];
        let n_push_threats = count(push_attacks_on_enemy & non_pawn_enemies) as i32;
        mg += sign * w.push_threat.0 * n_push_threats;
        eg += sign * w.push_threat.1 * n_push_threats;

        // Hit-queen: peca menor/torre nossa esta' a UMA-JOGADA de
        // atacar a dama inimiga a partir de casa segura.
        if count(their_queen) == 1 {
            let qs = their_queen.trailing_zeros() as Square;
            let targets_base = safe & !own_pawns;
            let knight_hits = a.knight[qs as usize];
            let bishop_hits = bishop_attacks(qs, occ);
            let rook_hits = rook_attacks(qs, occ);
            // Knight hits nao precisam de attackedBy2[us], mas
            // bishop/rook precisam (targets &= attackedBy2[us]).
            let n_knight_hit = count(targets_base & knight_hits & attacked_by_pt[us][PieceType::Knight.idx()]) as i32;
            mg += sign * w.knight_hit_queen.0 * n_knight_hit;
            eg += sign * w.knight_hit_queen.1 * n_knight_hit;
            let targets_double = targets_base & attacked_by_2[us];
            let n_bishop_hit = count(targets_double & bishop_hits & attacked_by_pt[us][PieceType::Bishop.idx()]) as i32;
            mg += sign * w.bishop_hit_queen.0 * n_bishop_hit;
            eg += sign * w.bishop_hit_queen.1 * n_bishop_hit;
            let n_rook_hit = count(targets_double & rook_hits & attacked_by_pt[us][PieceType::Rook.idx()]) as i32;
            mg += sign * w.rook_hit_queen.0 * n_rook_hit;
            eg += sign * w.rook_hit_queen.1 * n_rook_hit;
        }
    }

    // === Estrutura de peoes (via cache dedicada) ===
    //
    // Tudo o que depende so' dos peoes vem da cache de uma vez. O que sai
    // de la' em `passados` e' o bitboard dos peoes passados por cor -- o
    // teste caro de os identificar ja' foi feito (ou reaproveitado).
    let (mg_peoes, eg_peoes, passados) = estrutura_de_peoes_cache(board, w);
    mg += mg_peoes;
    eg += eg_peoes;

    // Termos do peao passado que NAO sao cacheaveis: dependem de onde
    // estao as pecas, e mudam sem nenhum peao se mexer.
    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let mut pp_bb = passados[c.idx()];
        while pp_bb != 0 {
            let s = pp_bb.trailing_zeros() as Square;
            pp_bb &= pp_bb - 1;
            let f = file_of(s) as i32;
            let r = rank_of(s) as i32;
            let rel_rank = if c == Color::White { r as usize } else { (7 - r) as usize };
            if rel_rank < 3 { continue; }
            let push_r = if c == Color::White { r + 1 } else { r - 1 };
            if !(0..8).contains(&push_r) { continue; }
            let push_sq = sq(f as u8, push_r as u8);
            let push_bb = bb(push_sq);
            let push_blocked = board.occ_all & push_bb != 0;
            let push_controlled = attacked[c.opp().idx()] & push_bb != 0;
            let pp = w.passed_pawn[push_blocked as usize][push_controlled as usize][rel_rank];
            mg += sign * pp.0;
            eg += sign * pp.1;

            // Passado exterior: por unidade de CENTRALIDADE (0 nas colunas a/h,
            // 3 nas d/e). A nossa tabela de passado nao tinha dimensao de
            // coluna -- um passado na coluna a valia o mesmo que na d, quando
            // o exterior e' precisamente o que decide finais de rei e peoes.
            let centralidade = f.min(7 - f);
            mg += sign * w.passer_outside.0 * centralidade;
            eg += sign * w.passer_outside.1 * centralidade;

            let own_king = board.king_sq(c);
            let enemy_king = board.king_sq(c.opp());
            let our_dist = chebyshev_distance(own_king, push_sq);
            let their_dist = chebyshev_distance(enemy_king, push_sq);
            mg += sign * w.our_passer_proximity[our_dist].0;
            eg += sign * w.our_passer_proximity[our_dist].1;
            mg += sign * w.their_passer_proximity[their_dist].0;
            eg += sign * w.their_passer_proximity[their_dist].1;

            if attacked[c.idx()] & push_bb != 0 {
                mg += sign * w.passer_defended_push[rel_rank].0;
                eg += sign * w.passer_defended_push[rel_rank].1;
            }

            // Torre/dama inimiga atras do peao passado, na mesma coluna.
            let behind = masc().tras[c.idx()][s as usize];
            let enemy_rq = board.pieces[c.opp().idx()][PieceType::Rook.idx()]
                | board.pieces[c.opp().idx()][PieceType::Queen.idx()];
            if behind & enemy_rq != 0 {
                mg += sign * w.passer_slider_behind[rel_rank].0;
                eg += sign * w.passer_slider_behind[rel_rank].1;
            }
        }
    }

    // Tempo -- bonus para quem tem a jogar. Aplicado como (mg,eg) do
    // ponto de vista das brancas: se e' a vez das brancas, +w.tempo; se
    // e' a vez das pretas, -w.tempo.
    let tempo_sign = if board.side == Color::White { 1 } else { -1 };
    // Ter o lance nunca e' desvantagem em media, e num final de peoes e'
    // frequentemente decisivo. Um tempo negativo e' xadrez errado, e o ajuste
    // produz um: no bucket dos muitos peoes so' 0,10% das posicoes estao em
    // fase de final, portanto o lado `eg` deste peso treina com uma fraccao
    // dos dados e deriva. Onde os dados sao mudos, decide o xadrez.
    //
    // Custo de nao o travar: numa posicao perfeitamente simetrica com dezasseis
    // peoes e nenhuma peca, um eg de -5 passava a -108 depois da complexidade,
    // que le' o SINAL da avaliacao e amplifica o que la' estiver.
    mg += tempo_sign * w.tempo.0.max(0);
    eg += tempo_sign * w.tempo.1.max(0);

    // Interpolacao final pela fase actual do board (mesma logica de
    // material_pst; fase mantida incrementalmente em add_piece/
    // remove_piece).
    let phase = board.phase.min(MAX_PHASE);
    // Flat mode: the bucket does the phase work, so nothing is interpolated.
    //
    // Tapering between a midgame and an endgame value IS a phase model, and a
    // rather good one. Putting eight phase buckets on top of it asks the same
    // question twice: measured on a stronger engine, giving its
    // already-tuned terms a per-bucket multiplier cost 40 Elo, because the
    // taper had already said what the multiplier was trying to say.
    //
    // So when buckets carry the weights, the taper steps aside. Each bucket
    // holds one value per term instead of two, and eight free values beat two
    // values joined by a straight line -- that is the whole point of having
    // buckets, and it only holds if they are not competing with the line.
    //
    // The cost is a discontinuity where buckets meet: the evaluation jumps
    // when a capture moves the position into the next bucket. Buckets change
    // only when a knight, bishop, rook or queen leaves the board, and the
    // evaluation was going to jump at that moment anyway.
    // Flat mode: the phase is quantised to the bucket, not read continuously.
    //
    // Within a bucket the taper stops varying, so the bucket's own weights
    // decide everything and there is no second phase model competing with
    // them. Across buckets the weights are free to disagree, which is what
    // buckets are for. The pair is kept rather than collapsed because the
    // weight vector is not uniformly pairs -- several fields are lone
    // scalars -- and quantising the phase needs no assumption about which is
    // which.
    let phase = if flat_buckets() {
        let b = ((MAX_PHASE - phase) * NUM_BUCKETS as i32) / (MAX_PHASE + 1);
        let b = b.clamp(0, NUM_BUCKETS as i32 - 1);
        // Middle of that bucket's range, in phase units.
        MAX_PHASE - (2 * b + 1) * MAX_PHASE / (2 * NUM_BUCKETS as i32)
    } else {
        phase
    };
    (mg * phase + eg * (MAX_PHASE - phase)) / MAX_PHASE
}

/// Avaliacao: material + PST + termos posicionais/taticos ("estilo
/// Polgar" -- pressao sobre o rei inimigo, mobilidade pesada, iniciativa
/// via peoes passados e torres ativas). Devolve da perspetiva de quem
/// tem a jogar (convencao negamax).
///
/// 2026-07-20 (teste A/B -- investigacao da queda de resultados, ver
/// NOTAS_PROXIMA_SESSAO.md "proximos passos" #1): a variavel de ambiente
/// KESTREL_EVAL_MODE=material desliga positional_terms_signed por
/// completo, isolando se os termos "Polgar" ajudam ou atrapalham face a
/// so' material+PST. Por omissao (variavel ausente ou qualquer outro
/// valor) o comportamento fica EXATAMENTE como antes -- nada muda para o
/// motor "normal" que a arena ja usa. Ler o env UMA vez (OnceLock),
/// nao a cada chamada de evaluate() (custaria NPS real).
static EVAL_MODE_MATERIAL_ONLY: OnceLock<bool> = OnceLock::new();
/// Read once. `env::var_os` on every evaluation that finds a hanging piece is
/// a syscall-shaped cost in the hottest loop the engine has.
static DEBUG_SEE: OnceLock<bool> = OnceLock::new();
/// Which sides the hanging-piece term looks at, and how much each counts.
///
/// By default only the side that does NOT have the move, at full weight --
/// what the search wants, since it will see the escape itself. With
/// `KESTREL_SEE_BOTH=1` the side to move is priced too, at a fraction, for
/// running without a search at all (see HeatmapOnly).
static SEE_BOTH: OnceLock<bool> = OnceLock::new();
const SEE_TO_MOVE_SHARE: i32 = 350;

fn see_both_sides(to_move: Color) -> [(Color, i32); 2] {
    let both = *SEE_BOTH.get_or_init(|| {
        std::env::var("KESTREL_SEE_BOTH").map(|v| v == "1").unwrap_or(false)
    });
    // The side without the move always counts in full. The side to move counts
    // at a fraction when asked, and at zero otherwise -- a zero share skips the
    // work, so the default path costs exactly what it did before.
    if debug_see() {
        eprintln!("debug: see_both={} to_move={:?}", both, to_move);
    }
    [
        (to_move.opp(), 1000),
        (to_move, if both { SEE_TO_MOVE_SHARE } else { 0 }),
    ]
}

fn debug_see() -> bool {
    *DEBUG_SEE.get_or_init(|| std::env::var_os("KESTREL_DEBUG_SEE").is_some())
}

fn eval_mode_material_only() -> bool {
    *EVAL_MODE_MATERIAL_ONLY.get_or_init(|| {
        std::env::var("KESTREL_EVAL_MODE").map(|v| v == "material").unwrap_or(false)
    })
}
// === Cache de avaliacao por posicao ===
//
// Isto nao veio do motor de referencia -- veio de medir onde ELE nao pode ir.
// Por no', ele falha a cache 14,45 vezes e nos 8,34; o IPC dele e' 1,10 e o
// nosso 2,36. Ou seja: ele esta' preso a' memoria e nos estamos presos a
// trabalho. Quem tem folga de memoria e falta de ciclos deve trocar uma pela
// outra, e essa troca so' esta' disponivel para o nosso lado.
//
// A TT ja' guarda a avaliacao, mas e' o instrumento errado para isto: e'
// substituida por buscas mais profundas e as entradas competem por espaco com
// scores e lances. Uma cache dedicada guarda so' o que interessa e mantem
// posicoes que a TT ja' deitou fora.
//
// A chave cobre tudo o que a avaliacao le': os doze bitboards, o lado a jogar
// e os direitos de roque (ha' um termo de rei por rocar). Se faltasse alguma
// coisa, o bench mudava de contagem.
#[derive(Clone, Copy)]
struct EntradaEval {
    chave: u64,
    geracao: u64,
    valor: i32,
}

const CACHE_EVAL_BITS: usize = 17; // 131072 entradas, ~2 MB por thread

thread_local! {
    static CACHE_EVAL: std::cell::RefCell<Vec<EntradaEval>> =
        std::cell::RefCell::new(vec![
            EntradaEval { chave: 0, geracao: u64::MAX, valor: 0 };
            1 << CACHE_EVAL_BITS
        ]);
}

#[inline(always)]
fn chave_posicao(board: &Board) -> u64 {
    const K: [u64; 12] = [
        0x9E3779B97F4A7C15, 0xC2B2AE3D27D4EB4F, 0x165667B19E3779F9, 0x27D4EB2F165667C5,
        0x9E3779B185EBCA87, 0xFF51AFD7ED558CCD, 0xC4CEB9FE1A85EC53, 0xD6E8FEB86659FD93,
        0xA24BAED4963EE407, 0x9FB21C651E98DF25, 0xEB44ACCAB455D165, 0x2545F4914F6CDD1D,
    ];
    let mut h = 0u64;
    let mut i = 0;
    for c in 0..2 {
        for pt in 0..6 {
            // Misturador completo, com deslocamento ANTES da primeira
            // multiplicacao. Sem ele, uma peca no bit 63 e' invisivel: a
            // multiplicacao so' propaga bits para cima e 2^63 vezes qualquer
            // impar da' 2^63, portanto trocar h8 entre dois bitboards mexe
            // nos dois termos pelo mesmo delta e o XOR cancela. Torre, bispo
            // e cavalo em h8 davam a mesma chave -- foi assim que esta cache
            // falhou, duas vezes, antes de eu perceber que o problema era o
            // sentido em que a multiplicacao mistura.
            let mut v = board.pieces[c][pt] ^ K[i];
            v ^= v >> 30;
            v = v.wrapping_mul(0xBF58476D1CE4E5B9);
            v ^= v >> 27;
            v = v.wrapping_mul(0x94D049BB133111EB);
            v ^= v >> 31;
            h ^= v;
            i += 1;
        }
    }
    h ^= (board.castling as u64).wrapping_mul(0x8EBC6AF09C88C6E3);
    if board.side == Color::White {
        h ^= 0x589965CC75374CC3;
    }
    h
}

pub fn evaluate(board: &Board) -> i32 {
    let chave = chave_posicao(board);
    let geracao = GERACAO_PESOS.load(std::sync::atomic::Ordering::Relaxed);
    let idx = (chave as usize) & ((1 << CACHE_EVAL_BITS) - 1);
    if let Some(v) = CACHE_EVAL.with(|t| {
        let e = t.borrow()[idx];
        if e.chave == chave && e.geracao == geracao { Some(e.valor) } else { None }
    }) {
        static VERIFICA: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *VERIFICA.get_or_init(|| std::env::var_os("KESTREL_VERIFICA_EVAL").is_some()) {
            let real = evaluate_sem_cache(board);
            if real != v {
                eprintln!("EVAL-CACHE DISCORDA: cache={} real={} fen={}", v, real, board.to_fen());
            }
        }
        return v;
    }
    let valor = evaluate_sem_cache(board);
    CACHE_EVAL.with(|t| {
        t.borrow_mut()[idx] = EntradaEval { chave, geracao, valor };
    });
    valor
}

fn evaluate_sem_cache(board: &Board) -> i32 {
    // Known endgames answer for themselves, before anything is counted.
    // Material is the wrong instrument here: it scored two knights against a
    // bare king at +4.20, a position with no mate in it at all, higher than
    // rook against king which is a forced win. See endgame.rs.
    match crate::endgame::probe(board, eg_material_white(board), eg_queen_minus_pawn()) {
        Some((strong, crate::endgame::Verdict::Exact(v))) => {
            let white_pov = if strong == Color::White { v } else { -v };
            return if board.side == Color::White { white_pov } else { -white_pov };
        }
        Some((_, crate::endgame::Verdict::Scale(f))) if f != crate::endgame::SCALE_NORMAL => {
            let raw = if eval_mode_material_only() {
                material_pst(board)
            } else {
                material_pst(board) + positional_terms_signed(board)
            };
            let w = weights_for(board);
            let raw = raw + complexity_adjustment(board, raw, w);
            return scale_endgame(board, raw, w) * f / crate::endgame::SCALE_NORMAL;
        }
        _ => {}
    }
    let raw = if eval_mode_material_only() {
        material_pst(board)
    } else {
        material_pst(board) + positional_terms_signed(board)
    };
    let w = weights_for(board);
    let raw = raw + complexity_adjustment(board, raw, w);
    // The per-bucket family scales in `weights_for` correct the same slope
    // this did, and they do it where it was measured -- the positional terms,
    // which carry 83-100% of the evaluation in balanced positions. Applying
    // both multiplied the correction: 0.65 x 0.82 is half the evaluation.
    // Kept, and inert, while the profile drives the buckets.
    let v = scale_endgame(board, raw, w) + psqt_bucket_correction(board);
    let v = if MATERIAL_BUCKETS_ON.load(std::sync::atomic::Ordering::Relaxed) {
        material_bucket_scale(board, v)
    } else {
        v
    };
    // Added last, so the scaling that corrects how loud the evaluation is does
    // not also scale a tie-break that is not part of the position's worth --
    // and turned to the side to move first, which is what `evaluate` returns
    // and what everything above it already is.
    //
    // Getting that backwards is not a small error: a drive term with the wrong
    // sign tells the winning side to walk AWAY from the king it is trying to
    // trap. It showed up as the evaluation dropping 14cp in a position White
    // was winning, which is the opposite of what the term exists to do.
    #[cfg(feature = "goldeneye")]
    {
        let drive = crate::endgame::conversion_drive(board, eg_material_white(board));
        v + if board.side == Color::White { drive } else { -drive }
    }
    #[cfg(not(feature = "goldeneye"))]
    {
        v
    }
}

/// How loud this evaluation is, measured against a strong reference and
/// corrected per material bucket.
///
/// Two phases blended linearly is one model of how a position changes as it
/// empties, and it is not the model the board follows. Measured on 220 quiet
/// positions, the slope of our evaluation against a reference's runs from
/// 0.78 with a full board to 1.57 once the queens and most pieces are gone:
/// we are quiet where there is most material and shrill where there is least.
/// A single global factor cannot fix that, because the error changes sign.
///
/// Eight buckets by piece count, each with the factor that brings its slope
/// back to one. These are corrections to the LOUDNESS, not to what the terms
/// believe -- the ordering of moves within a bucket is untouched. What changes
/// is that a pawn's worth of advantage now means the same thing to the pruning
/// margins in an endgame as it does in a middlegame, which it did not before.
/// The chance of winning, drawing and losing this position, in per mille.
///
/// The search needs one number and gets one; this is for everything that has to
/// DECIDE something -- offer a draw, accept one, resign, spend clock -- and
/// those decisions are about outcomes, not centipawns.
///
/// It exists because centipawns are not a stable unit here and that is measured,
/// not suspected. Fitting `1/(1+10^(-eval/D))` to the actual results of 220,000
/// positions, the D the data asks for is not 400 and is not one number:
///
///   pawns   0-3   4-6   7-8    9   10-11   12    13   14+
///   D       433   548   631   694   782   987  1042  1564
///
/// Three and a half times, end to end. A centipawn in the opening is worth
/// under a third of a centipawn in an endgame in actual winning chances, and
/// the evaluation reports them as the same quantity. At the single 400 the
/// engine assumed, a position it calls +210 was being read as a 77% score where
/// the truth is 64%.
///
/// Buckets by PAWN count, the same partition `bucket_of` uses. Measuring on one
/// partition and applying on another is a mistake this project has already made
/// once, in the feature extractor.
pub fn win_draw_loss(board: &Board, eval_cp: i32) -> (i32, i32, i32) {
    const DIVISOR: [i32; NUM_BUCKETS] = [433, 548, 631, 694, 782, 987, 1042, 1564];
    // Draw rate at level, per bucket, falling as the position leaves level.
    // Endgames with few pawns draw far more often than a full board does, which
    // no single number can express either.
    const DRAW_AT_LEVEL: [i32; NUM_BUCKETS] = [620, 480, 430, 400, 370, 330, 310, 250];
    let b = bucket_of(board);
    let d = DIVISOR[b] as f64;
    let x = eval_cp as f64 / d;
    let score = 1.0 / (1.0 + 10f64.powf(-x));
    // The draw share shrinks as the evaluation leaves zero, on the same scale
    // the score is measured in, so both curves stretch together per bucket.
    let draw = DRAW_AT_LEVEL[b] as f64 / 1000.0 / (1.0 + x * x);
    let draw = draw.min(2.0 * score.min(1.0 - score));
    let w = ((score - draw / 2.0) * 1000.0).round() as i32;
    let dr = (draw * 1000.0).round() as i32;
    (w.clamp(0, 1000), dr.clamp(0, 1000), (1000 - w - dr).clamp(0, 1000))
}

pub fn material_bucket_scale(board: &Board, v: i32) -> i32 {
    // Per mille, indexed by (piece count - 1) / 4, so bucket 7 is a full board.
    // Derived from measured slope: factor = 1 / slope, softened toward 1.0
    // because the sample is a few dozen positions per bucket, not a tuning run.
    const SCALE: [i32; 8] = [850, 850, 820, 830, 830, 975, 1015, 1140];
    let n = (board.occ_color[0] | board.occ_color[1]).count_ones() as usize;
    let b = n.saturating_sub(1) / 4;
    let f = SCALE[b.min(7)];
    if f == 1000 {
        v
    } else {
        v * f / 1000
    }
}

/// Endgame-phase material only, from White's point of view -- the baseline the
/// known-endgame handlers build their progress terms on top of.
fn eg_material_white(board: &Board) -> i32 {
    let mut v = 0;
    for pt in [PieceType::Pawn, PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen] {
        v += EG_VALUE[pt.idx()] * count(board.pieces[Color::White.idx()][pt.idx()]) as i32;
        v -= EG_VALUE[pt.idx()] * count(board.pieces[Color::Black.idx()][pt.idx()]) as i32;
    }
    v
}

fn eg_queen_minus_pawn() -> i32 {
    EG_VALUE[PieceType::Queen.idx()] - EG_VALUE[PieceType::Pawn.idx()]
}

/// Complexity adjustment.
/// Correction from code review (2026-07-22): this does NOT generally
/// shrink the eval toward zero, despite an earlier version of this
/// comment claiming that -- with the real ported constants, a typical
/// two-flank middlegame with plenty of pawns scores complexity=+53 and
/// that gets ADDED to the eval (confirmed by direct testing: a normal
/// opening-ish position went from raw=19 to evaluate()=72). What it
/// actually does: reward positions with more pawns spread across both
/// flanks (more winning chances/harder for the weaker side to hold)
/// and penalize/neutralize pawnless or single-flank endgames (easier
/// to hold or drawn outright) via the negative bias term. "Complexity"
/// names the SIGNAL (many pawns, both flanks, pure pawn endgame),
/// not the DIRECTION of the adjustment -- that depends on the sign of
/// each constant.
/// Sign-preserving clamp (`raw.signum() * complexity.max(-raw.abs())`)
/// guarantees the adjustment can only move the eval toward or away
/// from zero without ever flipping who's better -- an easy mistake to
/// make without it.
pub fn complexity_adjustment(board: &Board, raw: i32, w: &Weights) -> i32 {
    // DIAGNOSTICO (KESTREL_SEM_COMPLEXITY=1): este termo SOMA um bloco
    // proporcional ao numero de peoes, com o sinal do que a avaliacao ja
    // dizia, sem olhar a magnitude. Numa posicao perfeitamente simetrica com
    // 16 peoes transforma -2 em -108. Serve para medir quanto da nossa escala
    // (K=967 contra os 400 do NNUE) vem daqui.
    static SEM_CX: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *SEM_CX.get_or_init(|| std::env::var_os("KESTREL_SEM_COMPLEXITY").is_some()) {
        return 0;
    }
    if raw == 0 {
        return 0;
    }
    let w_idx = Color::White.idx();
    let b_idx = Color::Black.idx();
    let all_pawns = board.pieces[w_idx][PieceType::Pawn.idx()] | board.pieces[b_idx][PieceType::Pawn.idx()];
    let total_pawns = count(all_pawns) as i32;

    const QUEENSIDE: Bitboard = FILE_A | (FILE_A << 1) | (FILE_A << 2) | (FILE_A << 3);
    let both_flanks = (all_pawns & QUEENSIDE != 0) && (all_pawns & !QUEENSIDE != 0);

    let no_pieces = [PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen]
        .iter()
        .all(|&pt| board.pieces[w_idx][pt.idx()] == 0 && board.pieces[b_idx][pt.idx()] == 0);

    let complexity = w.complexity_total_pawns * total_pawns
        + if both_flanks { w.complexity_pawn_flanks } else { 0 }
        + if no_pieces { w.complexity_pawn_endgame } else { 0 }
        + w.complexity_adjustment;

    // A forma estabelecida define este termo com a componente de meio-jogo a
    // ZERO -- existe so' no final. Faz sentido: ele diz "este final e' dificil
    // de converter", e numa abertura com dezasseis peoes e todas as pecas isso
    // nao quer dizer nada.
    //
    // A nossa aplicava-se ao valor JA' interpolado, cega a' fase, e por isso
    // entrava na abertura: medido, somava 63 centipeoes a uma posicao inicial
    // perfeitamente simetrica, e numa posicao simetrica de dezasseis peoes
    // transformava -2 em -108, com o sinal decidido por um arredondamento.
    //
    // Ponderar pela fase de final e' o equivalente exacto de a por no slot eg
    // de um par (mg=0, eg=x) antes da interpolacao.
    let cheio = raw.signum() * complexity.max(-raw.abs());
    let fase = board.phase.min(MAX_PHASE);
    cheio * (MAX_PHASE - fase) / MAX_PHASE
}

/// Endgame scale factor (own thresholds below): known drawish/hard-to-convert
/// material patterns get their eval shrunk toward zero, in proportion
/// to how "scaled down" that material pattern actually plays in
/// practice. Applied to the WHOLE already-tapered eval rather than
/// splitting mg/eg and rescaling only eg separately -- by the time any
/// of these patterns fire, so little material is left that `phase` is
/// already deep in the endgame anyway, so the approximation costs
/// little accuracy for a lot less invasive a change (keeps
/// `positional_terms()` exactly linear in its weights, which
/// `tune_fast` in main.rs relies on -- see the comment there).
/// Symmetric (doesn't care whose turn it is), so it's safe to apply
/// after `material_pst`/`positional_terms_signed` have already flipped
/// sign for side-to-move.
const SCALE_NORMAL: i32 = 128;

pub fn scale_endgame(board: &Board, raw: i32, weights: &Weights) -> i32 {
    if raw == 0 {
        return 0;
    }
    let scale = endgame_scale_factor(board, raw, weights);
    if scale == SCALE_NORMAL {
        return raw;
    }
    raw * scale / SCALE_NORMAL
}

fn endgame_scale_factor(board: &Board, raw: i32, weights: &Weights) -> i32 {
    let w = Color::White.idx();
    let b = Color::Black.idx();
    let wp = board.pieces[w][PieceType::Pawn.idx()];
    let bp = board.pieces[b][PieceType::Pawn.idx()];
    let wn = board.pieces[w][PieceType::Knight.idx()];
    let bn = board.pieces[b][PieceType::Knight.idx()];
    let wb = board.pieces[w][PieceType::Bishop.idx()];
    let bb_ = board.pieces[b][PieceType::Bishop.idx()];
    let wr = board.pieces[w][PieceType::Rook.idx()];
    let br = board.pieces[b][PieceType::Rook.idx()];
    let wq = board.pieces[w][PieceType::Queen.idx()];
    let bq = board.pieces[b][PieceType::Queen.idx()];

    let n_wp = count(wp) as i32;
    let n_bp = count(bp) as i32;
    let n_wn = count(wn);
    let n_bn = count(bn);
    let n_wb = count(wb);
    let n_bb = count(bb_);
    let n_wr = count(wr);
    let n_br = count(br);
    let n_wq = count(wq);
    let n_bq = count(bq);

    // Opposite-colored bishops: exactly one bishop each, on different
    // square colors. Classic drawing fortress even a pawn or two up.
    // Scales down further, the fewer other pieces are left to help
    // convert. Values: bishops-only=64
    // < one-rook-each=96 < one-knight-each=106 -- corrected 2026-07-22,
    // an earlier version of this comment had the last two swapped.
    if n_wb == 1 && n_bb == 1 {
        let wb_sq = wb.trailing_zeros();
        let bb_sq = bb_.trailing_zeros();
        let wb_light = (rank_of(wb_sq as Square) + file_of(wb_sq as Square)) % 2 == 1;
        let bb_light = (rank_of(bb_sq as Square) + file_of(bb_sq as Square)) % 2 == 1;
        if wb_light != bb_light {
            if n_wn == 0 && n_bn == 0 && n_wr == 0 && n_br == 0 && n_wq == 0 && n_bq == 0 {
                return weights.scale_ocb_bishops_only;
            }
            if n_wr == 1 && n_br == 1 && n_wn == 0 && n_bn == 0 && n_wq == 0 && n_bq == 0 {
                return weights.scale_ocb_one_rook;
            }
            if n_wn == 1 && n_bn == 1 && n_wr == 0 && n_br == 0 && n_wq == 0 && n_bq == 0 {
                return weights.scale_ocb_one_knight;
            }
        }
    }

    // True insufficient material: at most a lone minor (knight or
    // bishop) and NO PAWNS AT ALL against a completely bare king --
    // K+N vs K / K+B vs K, the only cases actually impossible to force
    // a win in. Bug found by review (2026-07-22): the original version
    // of this check never looked at the STRONG side's own pawn count
    // (only the weak side's), so it fired for K+P vs K and similar
    // trivially-won endgames -- `evaluate()` returned exactly 0 for a
    // real K+P vs K position, confirmed by direct testing. A minor
    // piece PLUS pawns is not a fortress in general (the pawn just
    // queens); only the zero-pawn case is a genuine forced draw.
    let w_minors_only = n_wr == 0 && n_wq == 0 && n_wp == 0 && (n_wn + n_wb) <= 1;
    let b_minors_only = n_br == 0 && n_bq == 0 && n_bp == 0 && (n_bn + n_bb) <= 1;
    if w_minors_only && n_br == 0 && n_bq == 0 && n_bn == 0 && n_bb == 0 && n_bp == 0 {
        return 0;
    }
    if b_minors_only && n_wr == 0 && n_wq == 0 && n_wn == 0 && n_wb == 0 && n_wp == 0 {
        return 0;
    }

    // Fallback: scale down with how few pawns the stronger side has
    // left -- fewer pawns left to shelter a passer/create a second
    // weakness makes converting a material edge progressively harder.
    // Gated to queenless positions only: this function scales the
    // WHOLE already-tapered eval (not just the eg component a mg/eg-split
    // version would), so applying it unconditionally
    // would also shrink ordinary middlegame evals whenever pawn counts
    // differ -- wrong, since in the midgame this pattern says nothing
    // about convertibility. No queens is a cheap, real proxy for "this
    // is actually an endgame" that keeps the approximation safe.
    if n_wq == 0 && n_bq == 0 {
        // `raw` vem na perspectiva de QUEM JOGA, nao das brancas -- e' o que
        // diz o comentario de scale_endgame logo acima, e e' o que
        // material_pst/positional_terms_signed fazem. Portanto "raw > 0" quer
        // dizer "quem joga esta melhor", NAO "as brancas estao melhor".
        //
        // Com as pretas a jogar e melhores, a versao anterior contava os peoes
        // das BRANCAS como sendo do lado forte, e a escala saia diferente da
        // mesma posicao vista do outro lado.
        //
        // Medido, espelhando 900 posicoes (trocar as cores e virar o tabuleiro
        // tem de dar exactamente a mesma avaliacao): 30% assimetricas com 3-6
        // pecas, 33% com 7-10, 0% com o tabuleiro cheio -- ate 324cp de
        // diferenca. Um peao a mais bastava; um cavalo, bispo ou torre a mais
        // nao, porque so' os peoes entram nesta conta. Com material simetrico
        // dava zero, que e' o disfarce: escolher o lado errado nao custa nada
        // quando os dois lados tem os mesmos peoes.
        let brancas_fortes = if board.side == Color::White { raw > 0 } else { raw < 0 };
        let strong_pawns = if brancas_fortes { n_wp } else { n_bp };
        return (weights.scale_fallback_base + weights.scale_fallback_per_pawn * strong_pawns).min(SCALE_NORMAL);
    }
    SCALE_NORMAL
}

/// So' material + PST, sem os termos posicionais caros (mobilidade/
/// varrimento de ataques). Usada na quiescence, onde a busca ja' passa
/// por MUITOS nos so' para resolver capturas -- pedido explicito: "ela
/// tem de poder jogar bullet com as suas tecnicas". A riqueza posicional
/// fica reservada aos nos reais do negamax, onde influencia a escolha de
/// lances; na quiescence so' precisamos de um "stand pat" rapido e
/// decente. Reduz bastante o custo por no sem perder a personalidade nas
/// decisoes que realmente importam.
pub fn evaluate_fast(board: &Board) -> i32 {
    // The bucket correction belongs here too. Without it the quiescence search
    // and the pruning margins read the accumulator raw -- the queen at the
    // base bucket's amplitude in every position -- while the real evaluation
    // corrected it. Search and evaluation disagreeing about the same position
    // is worth more than the term itself: measured, it cost 12 suite positions
    // out of 214, and it looked exactly like the profile being wrong.
    let c = psqt_bucket_correction(board);
    material_pst(board) + if board.side == Color::White { c } else { -c }
}

/// Le' os acumuladores incrementais mantidos por add_piece()/remove_piece()
/// (ver board.rs) em vez de percorrer todas as pecas -- era a soma mais
/// cara paga em TODOS os nos (evaluate_fast() e' chamada em RFP/razoring/
/// futility/IID, e evaluate() chama-a tambem via este material_pst()).
/// A soma completa (loop por todas as pecas) so' acontece uma vez, na
/// construcao do board (ver Board::recompute_eval_accumulators).
fn material_pst(board: &Board) -> i32 {
    if board.side == Color::White {
        material_pst_white(board)
    } else {
        -material_pst_white(board)
    }
}

/// Same as `material_pst()` but always from White's perspective
/// (no side-to-move flip) -- what the fast linear-feature tuner
/// (src/main.rs `tune_fast`) needs, since it builds its per-position
/// bias directly in White's POV to match `positional_terms()`'s own
/// convention, rather than negamax's STM-relative one.
/// How far into the game a position is, as 1.0 at the opening down to 0.0
/// in a bare endgame -- the same quantity the evaluation tapers with, made
/// available so tooling can bucket positions by phase the way the
/// evaluation itself weights them.
/// The taper denominator, exposed so tooling can probe at a value that
/// divides exactly and avoid the truncation a unit probe would suffer.
pub const MAX_PHASE_PUB: i32 = MAX_PHASE;

static FLAT_BUCKETS: OnceLock<bool> = OnceLock::new();
/// Is the phase taper switched off in favour of the buckets? Only sensible
/// with KESTREL_BUCKET_WEIGHTS supplying per-bucket values.
fn flat_buckets() -> bool {
    *FLAT_BUCKETS.get_or_init(|| std::env::var("KESTREL_FLAT_BUCKETS").is_ok())
}

pub fn phase_fraction(board: &Board) -> f32 {
    board.phase.min(MAX_PHASE) as f32 / MAX_PHASE as f32
}

pub fn material_pst_white(board: &Board) -> i32 {
    let phase = board.phase.min(MAX_PHASE);
    // Com o espelho ligado le-se o acumulador do flanco em que cada rei
    // esta'; sem ele, o de sempre. Sao os dois mantidos em paralelo, portanto
    // isto e' so' escolher qual ler -- um lance de rei nao custa nada.
    // Buckets activos: o lookup e' directo, sem ciclo -- o trabalho ja' foi
    // feito incrementalmente nos oito acumuladores.
    // Decidido em COMPILACAO, nao com uma carga atomica por no'. A feature e'
    // de compilacao; ler um atomico em cada evaluate para responder a uma
    // pergunta que o compilador ja' sabe e' pagar um acesso a memoria e um
    // branch milhoes de vezes por segundo para nada.
    if cfg!(feature = "psqtbuckets") {
        let b = bucket_of(board);
        let (mg, eg) = board.psqt_bucket[b];
        return (mg * phase + eg * (MAX_PHASE - phase)) / MAX_PHASE;
    }
    let (mg, eg) = if PSQT_ESPELHO_REI {
        let fw = flanco_do_rei(board, Color::White) as usize;
        let fb = flanco_do_rei(board, Color::Black) as usize;
        let w = board.psqt_por_flanco[Color::White.idx()][fw];
        let b = board.psqt_por_flanco[Color::Black.idx()][fb];
        (w.0 + b.0, w.1 + b.1)
    } else {
        (board.mg_score, board.eg_score)
    };
    (mg * phase + eg * (MAX_PHASE - phase)) / MAX_PHASE
}

pub fn positional_terms_signed(board: &Board) -> i32 {
    let p = positional_terms(board, weights_for(board));
    if board.side == Color::White {
        p
    } else {
        -p
    }
}

/// Converte a avaliacao INTERNA para centipeoes reportaveis.
///
/// As unidades internas nao tem de significar centipeoes -- sao o que o
/// afinador produziu, e as margens de poda foram calibradas contra elas, por
/// isso mexer nelas partia a coerencia da busca. O que tem de estar calibrado
/// e' o numero que SAI. E' exactamente o que o motor de referencia faz:
/// mantem o peao interno a 65 em meio-jogo e divide o score reportado por uma
/// constante (102, no caso dele) afinada contra resultados reais.
///
/// A nossa constante vem da relacao `normalizado = k * bruto`, com o k medido
/// por `medek` contra 200 mil posicoes com rotulos de um motor forte:
/// k=0.4136,
/// logo N = 100/k = 242. Antes disto a posicao inicial reportava 83 quando o
/// motor de referencia reporta 28, e a startpos depois de e4 dava 103 -- tres
/// vezes o que vale.
// 242 dos dois lados ate' a escala do binario COMPLETO ser medida. O K=402
// que o afinador reporta e' do modelo linear dele -- nao inclui a complexidade
// nem a curva do rei, que o motor soma por cima e o ajuste nunca viu.
pub const NORMALIZA_PARA_PEAO: i32 = 242;

#[inline]
pub fn score_normalizado(interno: i32) -> i32 {
    interno * 100 / NORMALIZA_PARA_PEAO
}
