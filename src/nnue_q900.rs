//! Reader for the `q900` architecture: 128-wide accumulator, no threat
//! inputs, and a THREE-LAYER dense head (16 -> 32 -> 1) bucketed eight ways
//! by material.
//!
//! Two things make it different from the 512 in production.
//!
//! **A deeper head.** The 512 goes `2*HL -> 1` per bucket: material and
//! position are summed into a single dot product and the network has nowhere
//! to keep them apart. Two intermediate layers give it that room. The same
//! shape our earlier napk9 engine already used, so this is not new ground for
//! the project.
//!
//! **Trained at eval_scale 900 instead of 408.** Measured inside the engine
//! (`KESTREL_VALORES_PECAS`, 150 real positions, pieces removed on the real
//! `Board`), the 512 values a queen at 626cp against a true ~900, and the
//! error grows with the size of the piece: pawn 1.34x, knight 1.03x, rook
//! 0.92x, queen 0.70x. That shape cannot come from the data -- it is
//! Stockfish at depth 30+ -- it comes from the loss. With `sigmoid(cp/408)`
//! the gradient `s*(1-s)` is 0.246 at a pawn and 0.086 at a queen: the
//! network gets a third of the learning pressure exactly where it is wrong,
//! and the measured curve follows the predicted one almost point for point.
//! At 900 -- the queen's own value, not a number picked by eye -- the queen
//! rises to 79% of the pawn's gradient while knight and rook barely lose
//! (97% and 93%).
//!
//! What that costs: the real win curve fitted to 185k of our own archived
//! moves has width b~76cp, so a queen up is already a certain win and the
//! exact number does not change the probability. Widening the scale trades a
//! little WDL accuracy for correct MATERIAL ORDER, which is what the search
//! needs to decide exchanges -- and that is where the defect showed: the
//! engine preferred rook+bishop (856) over a queen (626).
//!
//! Gated behind `KESTREL_NNUE_Q900`; unset, this module does nothing.

use crate::board::Board;
use crate::features::{self, PIECE_FEATURES};

const HL: usize = 128;
const NB: usize = 8;
const D1: usize = 16;
const D2: usize = 32;
const FACTOR: usize = 704;
const QA: i32 = 255;
const QB: i32 = 64;
/// The `eval_scale` this network was trained with. Part of the weights.
const ESCALA_TREINO: i32 = 900;
/// The trainer declares `new_affine("l0", 704 + 1 + 22528, HL)`, so the layer
/// is one row wider than the features need. That spare row is NOT part of the
/// layout: `Factorised::from_parts` puts the factoriser first and sets
/// `offset = factoriser.num_inputs()`, which is exactly 704, so the piece
/// features start at 704 and the extra row sits unused at the end.
///
/// Reading `offset` as 705 was the first version of this file, and it valued a
/// pawn at 399 and a knight at 937 -- every weight row shifted by one, which
/// scrambles the piece values without producing anything obviously broken.
const N_IN: usize = FACTOR + 1 + PIECE_FEATURES;
const OFF_MAIN: usize = FACTOR;

pub struct RedeQ900 {
    l0w: Vec<i16>, // [N_IN * HL], input-major
    l0b: Vec<i16>, // [HL]
    d1w: Vec<i16>, // [(2*HL) * (D1*NB)], input-major
    d1b: Vec<i16>, // [D1*NB]
    d2w: Vec<i16>, // [D1 * (D2*NB)]
    d2b: Vec<i16>, // [D2*NB]
    d3w: Vec<i16>, // [D2 * NB]
    d3b: Vec<i16>, // [NB]
}

fn le_i16(b: &[u8], off: &mut usize, n: usize) -> Option<Vec<i16>> {
    if *off + n * 2 > b.len() {
        return None;
    }
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        v.push(i16::from_le_bytes([b[*off + i * 2], b[*off + i * 2 + 1]]));
    }
    *off += n * 2;
    Some(v)
}

fn carrega(b: &[u8]) -> Option<RedeQ900> {
    let mut o = 0usize;
    let l0w = le_i16(b, &mut o, N_IN * HL)?;
    let l0b = le_i16(b, &mut o, HL)?;
    let d1w = le_i16(b, &mut o, 2 * HL * D1 * NB)?;
    let d1b = le_i16(b, &mut o, D1 * NB)?;
    let d2w = le_i16(b, &mut o, D1 * D2 * NB)?;
    let d2b = le_i16(b, &mut o, D2 * NB)?;
    let d3w = le_i16(b, &mut o, D2 * NB)?;
    let d3b = le_i16(b, &mut o, NB)?;
    // The file ends with the trainer's own 48-byte signature. Anything else
    // left over means the geometry above does not match the file, which is
    // worth failing loudly for rather than evaluating garbage.
    let sobra = b.len() - o;
    if sobra != 48 {
        eprintln!("nnue-q900: {sobra} bytes a mais no fim (esperava 48 de assinatura)");
        return None;
    }
    eprintln!("nnue-q900: rede carregada (HL={HL}, cabeca {D1}->{D2}->1, {} bytes)", b.len());
    Some(RedeQ900 { l0w, l0b, d1w, d1b, d2w, d2b, d3w, d3b })
}

/// screlu in quantised units: clamp to [0,QA] then square, matching
/// `.screlu()` in the trainer. Kept as i32 -- the square of QA is 65025,
/// which fits, and going through floats here would cost more than it buys.
#[inline]
fn screlu(x: i32) -> i32 {
    let c = x.clamp(0, QA);
    c * c
}

pub fn evaluate(net: &RedeQ900, board: &Board) -> i32 {
    // Accumulator, both perspectives. Rebuilt each call: at 128 wide this is
    // ~30 rows of 128 i16, and correctness first -- an incremental version
    // belongs after the network has earned it.
    let pos = features::Pos { pieces: board.pieces };
    let mut acc = [[0i32; HL]; 2];
    for persp in 0..2 {
        for k in 0..HL {
            acc[persp][k] = net.l0b[k] as i32;
        }
        let mut feats: Vec<usize> = Vec::with_capacity(32);
        features::gather_pieces(&pos, persp, &mut feats);
        for f in feats {
            // Every piece feature also fires its factorised twin: the same
            // piece-square without the king bucket, shared across all 32
            // buckets. Without it each bucket would learn from a
            // thirty-second of the data.
            let base = OFF_MAIN + f;
            let fact = f % FACTOR;
            let so_principal = std::env::var_os("Q900_SEM_FACTOR").is_some();
            for k in 0..HL {
                let (ib, ifa) = if std::env::var_os("Q900_TRANSP").is_some() {
                    (k * N_IN + base, k * N_IN + fact)
                } else {
                    (base * HL + k, fact * HL + k)
                };
                acc[persp][k] += net.l0w[ib] as i32;
                if !so_principal {
                    acc[persp][k] += net.l0w[ifa] as i32;
                }
            }
        }
    }

    if std::env::var_os("Q900_DEBUG").is_some() {
        let mn = acc[0].iter().min().unwrap();
        let mx = acc[0].iter().max().unwrap();
        eprintln!("acc[0] min={mn} max={mx} (QA={QA}, screlu satura acima de {QA})");
    }
    let stm = board.side.idx();
    let (us, them) = (stm, stm ^ 1);
    let mut x = [0i32; 2 * HL];
    for k in 0..HL {
        x[k] = screlu(acc[us][k]);
        x[HL + k] = screlu(acc[them][k]);
    }

    let n = board.occ_all.count_ones() as usize;
    let bucket = (n.saturating_sub(2) / 4).min(NB - 1);

    // Layer 1: (2*HL) -> D1, this bucket's slice only.
    let mut a1 = [0i32; D1];
    for o in 0..D1 {
        let col = bucket * D1 + o;
        let mut s: i64 = 0;
        for i in 0..2 * HL {
            s += x[i] as i64 * net.d1w[i * (D1 * NB) + col] as i64;
        }
        // x carries QA^2 from screlu and the weights carry QB, so the product
        // is QA^2*QB; the bias is stored at QA*QB. Divide by QA to land both
        // on QA*QB, then bring back to QA units for the next screlu.
        let v = (s / (QA as i64)) as i32 + net.d1b[col] as i32;
        a1[o] = screlu(v / QB);
    }

    // Layer 2: D1 -> D2.
    let mut a2 = [0i32; D2];
    for o in 0..D2 {
        let col = bucket * D2 + o;
        let mut s: i64 = 0;
        for i in 0..D1 {
            s += a1[i] as i64 * net.d2w[i * (D2 * NB) + col] as i64;
        }
        let v = (s / (QA as i64)) as i32 + net.d2b[col] as i32;
        a2[o] = screlu(v / QB);
    }

    // Layer 3: D2 -> 1.
    let mut s: i64 = 0;
    for i in 0..D2 {
        s += a2[i] as i64 * net.d3w[i * NB + bucket] as i64;
    }
    let out = (s / (QA as i64)) as i32 + net.d3b[bucket] as i32;

    // Back to centipawns, the same way the 512's reader does it:
    // `(sum/QA + bias) * scale / (QA*QB)`. `out` is in QA*QB units here, so
    // dividing by QA*QB gives the raw value the loss saw and multiplying by
    // the TRAINING scale puts it in centipawns.
    //
    // 900 and not the tunable `EvalScale`: this network was trained against
    // Stockfish scores through `sigmoid(cp/900)`, so 900 is a property of the
    // weights, not a preference of whoever launches the engine. Reading it
    // through any other number reports a different position than the one the
    // network learned.
    (out as i64 * ESCALA_TREINO as i64 / (QA as i64 * QB as i64)) as i32
}

static REDE: std::sync::OnceLock<Option<RedeQ900>> = std::sync::OnceLock::new();

pub fn rede() -> Option<&'static RedeQ900> {
    REDE.get_or_init(|| {
        let path = std::env::var("KESTREL_NNUE_Q900").ok()?;
        let bytes = std::fs::read(&path).ok()?;
        carrega(&bytes)
    })
    .as_ref()
}

pub fn active() -> bool {
    rede().is_some()
}
