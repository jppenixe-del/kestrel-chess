//! Reader for the "juntas" 3-head checkpoint family
//! (`napk9_train_juntas.rs`): shared accumulator + PSQT, bullet/small/big
//! heads trained jointly against the same target. Only the "big" (g_*) head
//! is read here -- it is the value `(out_big, loss)` actually returns from
//! the training graph, so it is the intended evaluation, not an auxiliary.
//!
//! Inputs are full threats (piece 22528 + threat 9216, no factoriser
//! wrapper) -- confirmed by the checkpoint's own byte count, not by
//! `napk9_train_juntas.rs`'s current `NapkInput23168` declaration, which
//! this specific checkpoint does not match (see the size check below).
//! `map_features_pairs_mode(pos, stm, mode=2, ..)` is the single shared
//! function between trainer and engine for this feature set (see its own
//! doc comment in features.rs), used here directly instead of a second
//! hand-written mirror.
//!
//! Gated behind `KESTREL_NNUE_JUNTAS`; unset, this module does nothing.

use crate::board::Board;
use crate::features::{self, TOTAL_INPUTS_V10};

// Byte-count check against the checkpoint ruled out the 640-threat scheme
// napk9_train_juntas.rs declares (n_in=23169): this exact file is 65,858,496
// bytes, which only matches n_in=31745 (full 9216 threats) -- 16 bytes off
// after accounting for every field below. Whatever produced this checkpoint
// used full threats, not the classic-640 the current script hardcodes.
const N_ACTIVE: usize = TOTAL_INPUTS_V10; // 31744, features actually used
const NB: usize = 8;
const G_L1: usize = 32; // BIG_L2
const G_L2: usize = 32; // BIG_L3
const QA: i32 = 255;
const QB: i32 = 64;
const ESCALA_TREINO: i32 = 410;

pub struct RedeJuntas {
    l1: usize,
    accw: Vec<i16>,  // [N_ACTIVE * l1] -- the trailing +1 dummy row is never indexed
    accb: Vec<i16>,  // [l1]
    psqtw: Vec<i16>, // [N_ACTIVE * NB]
    // g head only. Only the OUTPUT layer (g_l3) is bucketed -- g_l1/g_l2 are
    // ONE shared weight set for all 8 material buckets (matches the
    // training graph: `new_affine("g_l1", l1*2, BIG_L2)`, no `*NB` on
    // either hidden layer, only on g_l3's `1*8`).
    g_l1w: Vec<i16>, // [2*l1 * G_L1]
    g_l1b: Vec<i16>, // [G_L1]
    g_l2w: Vec<i16>, // [G_L1 * G_L2]
    g_l2b: Vec<i16>, // [G_L2]
    g_l3w: Vec<i16>, // [G_L2 * NB]
    g_l3b: Vec<i16>, // [NB]
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

fn skip(off: &mut usize, n_bytes: usize) {
    *off += n_bytes;
}

fn carrega(b: &[u8], l1: usize) -> Option<RedeJuntas> {
    let n_in = N_ACTIVE + 1; // the unused dummy row
    let mut o = 0usize;
    let accw_full = le_i16(b, &mut o, n_in * l1)?;
    let accb = le_i16(b, &mut o, l1)?;
    let psqtw_full = le_i16(b, &mut o, n_in * NB)?;
    skip(&mut o, NB * 2); // psqtb, unused (cancels stm-ntm, matches project convention)

    // bullet head: read past it, never used for evaluation.
    skip(&mut o, (2 * l1 * 16) * 2); // bullet_l1w
    skip(&mut o, 16 * 2); // bullet_l1b
    skip(&mut o, (16 * 32) * 2); // bullet_l2w
    skip(&mut o, 32 * 2); // bullet_l2b
    skip(&mut o, (32 * NB) * 2); // bullet_l3w
    skip(&mut o, NB * 2); // bullet_l3b

    // small head: read past it too.
    skip(&mut o, (2 * l1 * 32) * 2); // small_l1w
    skip(&mut o, 32 * 2); // small_l1b
    skip(&mut o, (32 * 32) * 2); // small_l2w
    skip(&mut o, 32 * 2); // small_l2b
    skip(&mut o, (32 * NB) * 2); // small_l3w
    skip(&mut o, NB * 2); // small_l3b

    let g_l1w = le_i16(b, &mut o, 2 * l1 * G_L1)?;
    let g_l1b = le_i16(b, &mut o, G_L1)?;
    let g_l2w = le_i16(b, &mut o, G_L1 * G_L2)?;
    let g_l2b = le_i16(b, &mut o, G_L2)?;
    let g_l3w = le_i16(b, &mut o, G_L2 * NB)?;
    let g_l3b = le_i16(b, &mut o, NB)?;

    // Only the N_ACTIVE rows of accw/psqtw are ever indexed by real
    // features; drop the trailing dummy row here so the evaluator doesn't
    // have to know about it.
    let accw = accw_full[..N_ACTIVE * l1].to_vec();
    let psqtw = psqtw_full[..N_ACTIVE * NB].to_vec();

    eprintln!(
        "nnue-juntas: rede carregada (l1={l1}, cabeca 'g' {G_L1}->{G_L2}->1, {} bytes, {} de sobra)",
        b.len(),
        b.len() as i64 - o as i64
    );
    Some(RedeJuntas { l1, accw, accb, psqtw, g_l1w, g_l1b, g_l2w, g_l2b, g_l3w, g_l3b })
}

#[inline]
fn screlu(x: i32) -> i32 {
    let c = x.clamp(0, QA);
    c * c
}

pub fn evaluate(net: &RedeJuntas, board: &Board) -> i32 {
    let l1 = net.l1;
    let pos = features::Pos { pieces: board.pieces };
    let stm = board.side.idx();

    let mut acc = vec![0i32; 2 * l1];
    let (acc_stm, acc_ntm) = acc.split_at_mut(l1);
    for k in 0..l1 {
        acc_stm[k] = net.accb[k] as i32;
        acc_ntm[k] = net.accb[k] as i32;
    }
    let mut psqt_raw: i64 = 0;
    features::map_features_pairs_mode(&pos, stm, 2, &mut |a, b| {
        for k in 0..l1 {
            acc_stm[k] += net.accw[a * l1 + k] as i32;
            acc_ntm[k] += net.accw[b * l1 + k] as i32;
        }
    });
    // Second pass for psqt (kept separate from the closure above so the
    // borrow checker doesn't need `psqt_raw` captured mutably alongside
    // `acc`; correctness doesn't depend on doing it in one pass).
    let n = board.occ_all.count_ones() as usize;
    let bucket = (n.saturating_sub(2) / 4).min(NB - 1);
    features::map_features_pairs_mode(&pos, stm, 2, &mut |a, b| {
        psqt_raw += net.psqtw[a * NB + bucket] as i64 - net.psqtw[b * NB + bucket] as i64;
    });

    let mut x = vec![0i32; 2 * l1];
    for k in 0..l1 {
        x[k] = screlu(acc_stm[k]);
        x[l1 + k] = screlu(acc_ntm[k]);
    }

    // g_l1/g_l2 are shared across all 8 buckets (only g_l3 is bucketed).
    let mut a1 = [0i32; G_L1];
    for o in 0..G_L1 {
        let mut s: i64 = 0;
        for i in 0..2 * l1 {
            s += x[i] as i64 * net.g_l1w[i * G_L1 + o] as i64;
        }
        let v = (s / (QA as i64)) as i32 + net.g_l1b[o] as i32;
        a1[o] = screlu(v / QB);
    }
    let mut a2 = [0i32; G_L2];
    for o in 0..G_L2 {
        let mut s: i64 = 0;
        for i in 0..G_L1 {
            s += a1[i] as i64 * net.g_l2w[i * G_L2 + o] as i64;
        }
        let v = (s / (QA as i64)) as i32 + net.g_l2b[o] as i32;
        a2[o] = screlu(v / QB);
    }
    let mut s3: i64 = 0;
    for i in 0..G_L2 {
        s3 += a2[i] as i64 * net.g_l3w[i * NB + bucket] as i64;
    }
    let g3_raw = (s3 / (QA as i64)) as i32 + net.g_l3b[bucket] as i32;

    // g3_raw is in QA*QB units (same derivation as q900's final layer).
    // psqt_raw is only quantised by QA (a raw per-feature weight sum, no
    // matmul chain), so it needs one extra factor of QB to land on the same
    // units before they can be summed and descaled together -- both are
    // added inside the SAME sigmoid in training (`g_l3_out + psqt_g`), so
    // they have to be on the same scale before the one division that turns
    // either of them into real centipawns.
    let combined = g3_raw as i64 + psqt_raw * QB as i64;
    (combined * ESCALA_TREINO as i64 / (QA as i64 * QB as i64)) as i32
}

static REDE: std::sync::OnceLock<Option<RedeJuntas>> = std::sync::OnceLock::new();

pub fn rede() -> Option<&'static RedeJuntas> {
    REDE.get_or_init(|| {
        let path = std::env::var("KESTREL_NNUE_JUNTAS").ok()?;
        let l1: usize = std::env::var("KESTREL_NNUE_JUNTAS_L1").ok()?.parse().ok()?;
        let bytes = std::fs::read(&path).ok()?;
        carrega(&bytes, l1)
    })
    .as_ref()
}

pub fn active() -> bool {
    rede().is_some()
}
