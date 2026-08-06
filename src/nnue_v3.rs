//! The v3 network: `(1 + 31744 -> 256)x2 -> 32 -> 32 -> 8`.
//!
//! The first architecture in this engine with anything between the
//! accumulator and the answer. Every network before it went straight from
//! the summed columns to an inner product -- a linear model over features,
//! however many features there were. Here the two perspectives are
//! concatenated and pushed through two small layers first.
//!
//! Why the accumulator SHRANK to 256 while the network grew: the accumulator
//! is what every node pays. It is updated on every move; the small layers run
//! only when something asks for a score, which is 0.65 times per node
//! (measured). Halving it also halves the weights in the layer that eats
//! training data, and that was the binding constraint -- the 512-wide,
//! 12-bucket v2 had 4.7M first-layer weights and 64 training positions per
//! weight where the production network had 254. It lost 25-81-25 for that
//! reason and not for any other: its reading was verified independently
//! (correlation 0.93 against a known-good net, mirror-invariance 10/10).
//!
//! Inputs are `features.rs` unchanged -- 22528 piece features plus the 9216
//! threats. Threats are in because they were measured worth +135 Elo inside
//! their own network (same weights, `setoption Threats` on and off, 259
//! games); what sank them was the reader around them, and at 256 wide they
//! cost half of what they did.
//!
//! The trainer's leading slot is reproduced rather than trimmed, for the same
//! reason as everywhere else in this file's neighbours: the weights are laid
//! out against it and shifting them by one row reads every feature off by
//! one.

use crate::board::Board;
use crate::features::{map_features_pairs_mode, Pos, TOTAL_INPUTS_V10};
use crate::types::Color;

pub const INPUTS: usize = TOTAL_INPUTS_V10; // 31744
pub const HL: usize = 256;
pub const FC1: usize = 32;
pub const FC2: usize = 32;
pub const OUT_BUCKETS: usize = 8;

const QA: i32 = 255;
const QB: i32 = 64;

use crate::nnue::escala;

/// The trainer writes `1 + INPUTS` rows: one leading slot it reserves.
const TRAINER_INPUTS: usize = 1 + TOTAL_INPUTS_V10;

pub struct RedeV3 {
    /// First layer, i16. 8.1M values and by far the largest thing here.
    pub l0w: Vec<i16>,
    pub l0b: Vec<i16>,
    /// `fc1` takes both perspectives concatenated, so its input is `2 * HL`.
    /// Stored transposed by the trainer (`SavedFormat::transpose`), i.e.
    /// output-major: row `o` is the `2*HL` weights feeding output `o`.
    pub fc1w: Vec<i16>,
    pub fc1b: Vec<i16>,
    pub fc2w: Vec<i16>,
    pub fc2b: Vec<i16>,
    /// One row of `FC2` weights per output bucket.
    pub fc3w: Vec<i16>,
    pub fc3b: Vec<i16>,
}

pub fn load(bytes: &[u8]) -> Option<RedeV3> {
    let n_l0w = TRAINER_INPUTS * HL;
    let precisa = n_l0w + HL + 2 * HL * FC1 + FC1 + FC1 * FC2 + FC2 + FC2 * OUT_BUCKETS + OUT_BUCKETS;
    let total = bytes.len() / 2;
    if total < precisa {
        eprintln!("nnue-v3: ficheiro tem {} valores, precisa de {}", total, precisa);
        return None;
    }
    // Trailing values beyond what the shape needs are the trainer's own
    // padding and are ignored, not an error -- the v2 files carry 24 of them
    // and load correctly.
    let mut it = bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]));
    let mut pega = |n: usize| -> Vec<i16> { (&mut it).take(n).collect() };
    let l0w = pega(n_l0w);
    let l0b = pega(HL);
    let fc1w = pega(2 * HL * FC1);
    let fc1b = pega(FC1);
    let fc2w = pega(FC1 * FC2);
    let fc2b = pega(FC2);
    let fc3w = pega(FC2 * OUT_BUCKETS);
    let fc3b = pega(OUT_BUCKETS);
    eprintln!(
        "nnue-v3: rede carregada ({} entradas, HL={}, {}->{}->{} buckets)",
        INPUTS, HL, FC1, FC2, OUT_BUCKETS
    );
    Some(RedeV3 { l0w, l0b, fc1w, fc1b, fc2w, fc2b, fc3w, fc3b })
}

/// Squared clipped ReLU, the activation the network was trained with.
///
/// Input is on the QA scale, output on QA squared -- which is why every layer
/// below divides by QA once more than the quantisation alone would suggest.
#[inline]
fn screlu(x: i32) -> i32 {
    let v = x.clamp(0, QA);
    v * v
}

/// Which output bucket a position falls in.
///
/// `(pieces - 2) / ceil(32 / N)`, which is what the trainer uses
/// (`MaterialCount<N>` in bullet). Written here rather than reused from
/// `nnue.rs` only because that one takes a `Network`; the formula is the
/// same one, and it is the same one that was wrong engine-wide until
/// 2026-08-06.
#[inline]
fn output_bucket(board: &Board) -> usize {
    let n = board.occ_all.count_ones() as usize;
    (n.saturating_sub(2) / 32usize.div_ceil(OUT_BUCKETS)).min(OUT_BUCKETS - 1)
}

/// Add one feature's column into an accumulator.
///
/// i16 accumulator, not i32, and vectorised. This runs ~190 times per
/// evaluation (one per active feature) and it is where the time goes: the
/// first layer is 4.2 MB, so every column applied is a kilobyte fetched from
/// memory. The scalar i32 version this replaced measured 250k nps against
/// 465k for the wider threats network -- half the accumulator running at half
/// the speed, which is the shape of a missing SIMD path and not of an
/// architecture.
///
/// i16 is safe at this width for the same reason it is in the other two
/// networks: the trainer clips the first layer so no combination of active
/// features can leave the range.
#[inline]
fn soma_col(dst: &mut [i16; HL], col: &[i16]) {
    #[cfg(target_arch = "x86_64")]
    {
        if tem_avx2() {
            unsafe {
                use std::arch::x86_64::*;
                let mut i = 0;
                while i + 16 <= HL {
                    let a = _mm256_loadu_si256(dst.as_ptr().add(i) as *const __m256i);
                    let b = _mm256_loadu_si256(col.as_ptr().add(i) as *const __m256i);
                    _mm256_storeu_si256(
                        dst.as_mut_ptr().add(i) as *mut __m256i,
                        _mm256_add_epi16(a, b),
                    );
                    i += 16;
                }
                while i < HL {
                    dst[i] += col[i];
                    i += 1;
                }
            }
            return;
        }
    }
    for i in 0..HL {
        dst[i] += col[i];
    }
}

/// The output sum over both perspectives for ONE `fc1` neuron, vectorised.
///
/// Same kernel shape as the other networks': clamp in i16, multiply the
/// clamped value by the weight, then `madd` against the clamped value again
/// so the i32 lanes accumulate `x * x * w`. Squaring first would leave i16.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn produto_screlu(us: &[i16; HL], them: &[i16; HL], w: &[i16]) -> i32 {
    use std::arch::x86_64::*;
    let zero = _mm256_setzero_si256();
    let topo = _mm256_set1_epi16(QA as i16);
    let mut acc = _mm256_setzero_si256();
    let mut i = 0;
    while i + 16 <= HL {
        for (v, off) in [(us, 0usize), (them, HL)] {
            let x = _mm256_loadu_si256(v.as_ptr().add(i) as *const __m256i);
            let wv = _mm256_loadu_si256(w.as_ptr().add(off + i) as *const __m256i);
            let c = _mm256_min_epi16(_mm256_max_epi16(x, zero), topo);
            let cw = _mm256_mullo_epi16(c, wv);
            acc = _mm256_add_epi32(acc, _mm256_madd_epi16(c, cw));
        }
        i += 16;
    }
    let baixo = _mm256_castsi256_si128(acc);
    let alto = _mm256_extracti128_si256(acc, 1);
    let mut sx = _mm_add_epi32(baixo, alto);
    sx = _mm_add_epi32(sx, _mm_shuffle_epi32(sx, 0b01_00_11_10));
    sx = _mm_add_epi32(sx, _mm_shuffle_epi32(sx, 0b10_11_00_01));
    let mut total = _mm_cvtsi128_si32(sx);
    while i < HL {
        total += screlu(us[i] as i32) * w[i] as i32;
        total += screlu(them[i] as i32) * w[HL + i] as i32;
        i += 1;
    }
    total
}

#[cfg(target_arch = "x86_64")]
fn tem_avx2() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        if std::env::var_os("KESTREL_SEM_SIMD").is_some() {
            return false;
        }
        std::is_x86_feature_detected!("avx2")
    })
}

#[cfg(not(target_arch = "x86_64"))]
fn tem_avx2() -> bool {
    false
}

pub fn evaluate(net: &RedeV3, board: &Board) -> i32 {
    let mut pos = Pos::default();
    for c in 0..2 {
        for t in 0..6 {
            pos.pieces[c][t] = board.pieces[c][t];
        }
    }
    // Enumerated from white's side always, never from the side to move.
    //
    // The pair callback returns (side-to-move view, other view). Asking it
    // for that directly indexes the accumulator by WHOSE TURN IT IS, and then
    // simply passing the move renumbers every feature -- measured on the
    // threats network, 108 of 200 changed on every evaluation, half of them
    // only because the turn had passed. Fixing white first makes the pair
    // (white, black) and the turn stops moving anything; which one is "ours"
    // is decided when they are read, below.
    let stm = 0usize;

    let mut acc_w = [0i16; HL];
    let mut acc_b = [0i16; HL];
    acc_w.copy_from_slice(&net.l0b);
    acc_b.copy_from_slice(&net.l0b);
    map_features_pairs_mode(&pos, stm, 2, &mut |a: usize, b: usize| {
        let ia = a * HL;
        let ib = b * HL;
        soma_col(&mut acc_w, &net.l0w[ia..ia + HL]);
        soma_col(&mut acc_b, &net.l0w[ib..ib + HL]);
    });

    // `acc_w` holds white and `acc_b` black; the network wants the side to
    // move first. Swapping here is free and keeps the accumulator a function
    // of the position rather than of the turn.
    let (us, them) = if board.side == Color::White { (&acc_w, &acc_b) } else { (&acc_b, &acc_w) };

    // fc1: both perspectives concatenated.
    //
    // The scales, because they are the part that silently produces plausible
    // nonsense if got wrong. The accumulator is on QA. `screlu` squares it,
    // so its output is on QA^2. Weights are on QB, so the sum is on QA^2*QB;
    // dividing by QA lands on QA*QB, which is the scale the trainer quantised
    // the biases to. To feed the next `screlu`, which expects QA, divide by QB
    // again.
    let mut h1 = [0i32; FC1];
    for o in 0..FC1 {
        let w = &net.fc1w[o * 2 * HL..(o + 1) * 2 * HL];
        #[cfg(target_arch = "x86_64")]
        let s = if tem_avx2() {
            unsafe { produto_screlu(us, them, w) }
        } else {
            (0..HL).map(|i| screlu(us[i] as i32) * w[i] as i32
                          + screlu(them[i] as i32) * w[HL + i] as i32).sum()
        };
        #[cfg(not(target_arch = "x86_64"))]
        let s: i32 = (0..HL).map(|i| screlu(us[i] as i32) * w[i] as i32
                                   + screlu(them[i] as i32) * w[HL + i] as i32).sum();
        h1[o] = (s / QA + net.fc1b[o] as i32) / QB;
    }

    let mut h2 = [0i32; FC2];
    for o in 0..FC2 {
        let w = &net.fc2w[o * FC1..(o + 1) * FC1];
        let mut s = 0i32;
        for (i, &wi) in w.iter().enumerate() {
            s += screlu(h1[i]) * wi as i32;
        }
        h2[o] = (s / QA + net.fc2b[o] as i32) / QB;
    }

    let ob = output_bucket(board);
    let w = &net.fc3w[ob * FC2..(ob + 1) * FC2];
    let mut s = 0i32;
    for (i, &wi) in w.iter().enumerate() {
        s += screlu(h2[i]) * wi as i32;
    }
    (s / QA + net.fc3b[ob] as i32) * escala() / (QA * QB)
}

static REDE: std::sync::OnceLock<Option<RedeV3>> = std::sync::OnceLock::new();

pub fn rede() -> Option<&'static RedeV3> {
    REDE.get_or_init(|| {
        let path = std::env::var("KESTREL_NNUE_V3").ok()?;
        match std::fs::read(&path) {
            Ok(b) => load(&b),
            Err(e) => {
                eprintln!("nnue-v3: nao consegui ler {}: {}", path, e);
                None
            }
        }
    })
    .as_ref()
}
