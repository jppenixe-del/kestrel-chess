//! Reader for the `sf256` architecture: 256-wide accumulator, full threats
//! (mode 2, 704-shared factoriser + 31744 piece/threat features), and a
//! three-layer head (32 -> 32 -> 1 x8 buckets) with a linear skip channel
//! bypassing the non-linear funnel (the 33rd output of the first dense
//! layer, never activated, added straight into the final sum -- same shape
//! as the FC0 skip already used in the li11 reader).
//!
//! Gated behind `KESTREL_NNUE_SF256`; unset, this module does nothing.

use crate::board::Board;
use crate::features::{self, PIECE_FEATURES, TOTAL_INPUTS_V10};

const HL: usize = 256;
const NB: usize = 8;
const D1_REAL: usize = 32;
const D1_TOTAL: usize = D1_REAL + 1; // +1 = the skip channel
const D2: usize = 32;
const QA: i32 = 255;
const QB: i32 = 64;
const ESCALA_TREINO: i32 = 410;
const FACTOR: usize = 704;
// mode 2 (full threats): pieces (22528) + threats (9216) = 31744, not the
// plain-pieces-only 22528 the q900 reader used. Confirmed the hard way: the
// first version of this file reused q900's PIECE_FEATURES constant, and the
// loader silently accepted it (le_i16 never fails on a short file, it just
// stops early) -- 4.7MB of the network sat unread and every feature past
// piece 22528 pointed at the wrong row.
const N_IN: usize = FACTOR + 1 + TOTAL_INPUTS_V10;
const OFF_MAIN: usize = FACTOR;

pub struct RedeSf256 {
    l0w: Vec<i16>, // [N_IN * HL]
    l0b: Vec<i16>, // [HL]
    d1w: Vec<i16>, // [(2*HL) * (D1_TOTAL*NB)]
    d1b: Vec<i16>, // [D1_TOTAL*NB]
    d2w: Vec<i16>, // [D1_REAL * (D2*NB)]
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

fn carrega(b: &[u8]) -> Option<RedeSf256> {
    let mut o = 0usize;
    let l0w = le_i16(b, &mut o, N_IN * HL)?;
    let l0b = le_i16(b, &mut o, HL)?;
    let d1w = le_i16(b, &mut o, 2 * HL * D1_TOTAL * NB)?;
    let d1b = le_i16(b, &mut o, D1_TOTAL * NB)?;
    let d2w = le_i16(b, &mut o, D1_REAL * D2 * NB)?;
    let d2b = le_i16(b, &mut o, D2 * NB)?;
    let d3w = le_i16(b, &mut o, D2 * NB)?;
    let d3b = le_i16(b, &mut o, NB)?;
    let sobra = b.len() - o;
    eprintln!("nnue-sf256: rede carregada (HL={HL}, cabeca {D1_REAL}(+1 salto)->{D2}->1, {} bytes, {sobra} de sobra)", b.len());
    Some(RedeSf256 { l0w, l0b, d1w, d1b, d2w, d2b, d3w, d3b })
}

#[inline]
fn screlu(x: i32) -> i32 {
    let c = x.clamp(0, QA);
    c * c
}

pub fn evaluate(net: &RedeSf256, board: &Board) -> i32 {
    let pos = features::Pos { pieces: board.pieces };
    let attacked_by = features::compute_attack_maps_by_type(&pos);
    let mut acc = [[0i32; HL]; 2];
    for persp in 0..2 {
        for k in 0..HL {
            acc[persp][k] = net.l0b[k] as i32;
        }
        let mut feats: Vec<usize> = Vec::with_capacity(32);
        features::gather_pieces(&pos, persp, &mut feats);
        // Threats sit past the piece block (PIECE_FEATURES..TOTAL_INPUTS_V10)
        // in the combined 31744-wide input space; gather_threats_full itself
        // returns indices in its own 0..THREAT_FEATURES_FULL range.
        let mut threat_feats: Vec<usize> = Vec::with_capacity(64);
        features::gather_threats_full(&pos, persp, &attacked_by, &mut threat_feats);
        for f in feats {
            // Piece features (0..PIECE_FEATURES) are factorised: the same
            // piece-square without the king bucket, shared across all 32
            // buckets, same as q900.
            let base = OFF_MAIN + f;
            let fact = f % FACTOR;
            for k in 0..HL {
                acc[persp][k] += net.l0w[base * HL + k] as i32;
                acc[persp][k] += net.l0w[fact * HL + k] as i32;
            }
        }
        for f in threat_feats {
            let base = OFF_MAIN + PIECE_FEATURES + f;
            for k in 0..HL {
                acc[persp][k] += net.l0w[base * HL + k] as i32;
            }
        }
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

    // Layer 1: (2*HL) -> D1_TOTAL, this bucket's slice. Column D1_REAL is
    // the skip -- computed the same way as every other column, just never
    // activated or carried into layer 2.
    let mut v = [0i32; D1_TOTAL];
    for o in 0..D1_TOTAL {
        let col = bucket * D1_TOTAL + o;
        let mut s: i64 = 0;
        for i in 0..2 * HL {
            s += x[i] as i64 * net.d1w[i * (D1_TOTAL * NB) + col] as i64;
        }
        v[o] = (s / (QA as i64)) as i32 + net.d1b[col] as i32;
    }
    let skip_raw = v[D1_REAL];
    let mut a1 = [0i32; D1_REAL];
    for o in 0..D1_REAL {
        a1[o] = screlu(v[o] / QB);
    }

    // Layer 2: D1_REAL -> D2.
    let mut a2 = [0i32; D2];
    for o in 0..D2 {
        let col = bucket * D2 + o;
        let mut s: i64 = 0;
        for i in 0..D1_REAL {
            s += a1[i] as i64 * net.d2w[i * (D2 * NB) + col] as i64;
        }
        let v2 = (s / (QA as i64)) as i32 + net.d2b[col] as i32;
        a2[o] = screlu(v2 / QB);
    }

    // Layer 3: D2 -> 1, raw (no activation) -- same units as skip_raw
    // (both are "one affine layer's output before any external scale"),
    // so they sum before the final descale, matching how the training
    // graph adds `dense_eval + skip` before the sigmoid.
    let mut s3: i64 = 0;
    for i in 0..D2 {
        s3 += a2[i] as i64 * net.d3w[i * NB + bucket] as i64;
    }
    let out_raw = (s3 / (QA as i64)) as i32 + net.d3b[bucket] as i32;

    if std::env::var_os("KESTREL_SF256_DEBUG").is_some() {
        eprintln!(
            "DBG bucket={bucket} skip_raw={skip_raw} out_raw={out_raw} skip_cp={} out_cp={} combined_cp={}",
            (skip_raw as i64 * ESCALA_TREINO as i64 / (QA as i64 * QB as i64)),
            (out_raw as i64 * ESCALA_TREINO as i64 / (QA as i64 * QB as i64)),
            ((out_raw + skip_raw) as i64 * ESCALA_TREINO as i64 / (QA as i64 * QB as i64)),
        );
    }

    let combined = out_raw + skip_raw;
    (combined as i64 * ESCALA_TREINO as i64 / (QA as i64 * QB as i64)) as i32
}

static REDE: std::sync::OnceLock<Option<RedeSf256>> = std::sync::OnceLock::new();

pub fn rede() -> Option<&'static RedeSf256> {
    REDE.get_or_init(|| {
        let path = std::env::var("KESTREL_NNUE_SF256").ok()?;
        let bytes = std::fs::read(&path).ok()?;
        carrega(&bytes)
    })
    .as_ref()
}

pub fn active() -> bool {
    rede().is_some()
}
