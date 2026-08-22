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
use crate::features::{map_features_pairs_fase, Pos, TOTAL_INPUTS_V10};
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
fn soma_col(dst: &mut [i16], col: &[i16]) {
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
unsafe fn produto_screlu(us: &[i16], them: &[i16], w: &[i16]) -> i32 {
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
    // `_fase`, nao `_mode`: as ameacas nao sao emitidas nos finais (ver
    // `features::AMEACAS_MIN_PECAS`). A regra vive no ficheiro que o treinador
    // partilha, porque tem de ser a MESMA dos dois lados -- aplicada so' aqui
    // faria a rede ler entradas que nunca viu.
    map_features_pairs_fase(&pos, stm, 2, &mut |a: usize, b: usize| {
        let ia = a * HL;
        let ib = b * HL;
        soma_col(&mut acc_w, &net.l0w[ia..ia + HL]);
        soma_col(&mut acc_b, &net.l0w[ib..ib + HL]);
    });

    // `acc_w` holds white and `acc_b` black; the network wants the side to
    // move first. Swapping here is free and keeps the accumulator a function
    // of the position rather than of the turn.
    let (us, them) = if board.side == Color::White { (&acc_w, &acc_b) } else { (&acc_b, &acc_w) };

    cabeca(net, us, them, output_bucket(board))
}

/// As tres camadas depois do acumulador: `fc1 -> fc2 -> fc3[bucket]`.
///
/// Separada do `evaluate` porque os dois caminhos precisam dela -- a
/// recomputacao total e o acumulador incremental diferem apenas em COMO
/// chegam aos dois acumuladores, nunca no que se faz a seguir. Duas copias
/// disto e' a maneira classica de as duas deixarem de concordar sem ninguem
/// dar por isso.
///
/// As escalas, porque sao a parte que produz disparates plausiveis se
/// estiver errada. O acumulador esta' em QA. `screlu` eleva ao quadrado,
/// logo a saida esta' em QA^2. Os pesos estao em QB, portanto a soma esta'
/// em QA^2*QB; dividir por QA cai em QA*QB, que e' a escala a que o treinador
/// quantizou os vieses. Para alimentar o `screlu` seguinte, que espera QA,
/// divide-se por QB outra vez.
fn cabeca(net: &RedeV3, us: &[i16], them: &[i16], ob: usize) -> i32 {
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
    if SAUDE.load(std::sync::atomic::Ordering::Relaxed) {
        regista_saude(&h1, &h2);
    }
    let w = &net.fc3w[ob * FC2..(ob + 1) * FC2];
    let mut s = 0i32;
    for (i, &wi) in w.iter().enumerate() {
        s += screlu(h2[i]) * wi as i32;
    }
    (s / QA + net.fc3b[ob] as i32) * escala() / (QA * QB)
}

/// Diagnostico de saude das camadas escondidas.
///
/// Sob SCReLU um neuronio que nunca sai de zero contribui exactamente zero
/// para tudo o que vem a seguir -- "morto" nao e' uma opiniao, conta-se. O
/// Coda documenta que sem aquecimento do LR os neuronios da primeira camada
/// escondida morrem por volta do superbatch 40, e o nosso treinador nao tem
/// aquecimento nenhum; isto foi escrito para verificar a suspeita em vez de
/// a aceitar. (Verificou-se: 3 em 32. A suspeita estava errada.)
pub static SAUDE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static ACTIVOU_H1: [std::sync::atomic::AtomicU64; FC1] =
    [const { std::sync::atomic::AtomicU64::new(0) }; FC1];
pub static ACTIVOU_H2: [std::sync::atomic::AtomicU64; FC2] =
    [const { std::sync::atomic::AtomicU64::new(0) }; FC2];
pub static VISITAS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn regista_saude(h1: &[i32; FC1], h2: &[i32; FC2]) {
    use std::sync::atomic::Ordering::Relaxed;
    VISITAS.fetch_add(1, Relaxed);
    for i in 0..FC1 {
        if h1[i] > 0 { ACTIVOU_H1[i].fetch_add(1, Relaxed); }
    }
    for i in 0..FC2 {
        if h2[i] > 0 { ACTIVOU_H2[i].fetch_add(1, Relaxed); }
    }
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


/// `dst -= col`, vectorizado. Gemeo de `soma_col`.
#[inline]
fn sub_col(dst: &mut [i16], col: &[i16]) {
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
                        _mm256_sub_epi16(a, b),
                    );
                    i += 16;
                }
                while i < HL { dst[i] -= col[i]; i += 1; }
            }
            return;
        }
    }
    for i in 0..HL { dst[i] -= col[i]; }
}

/// Soma a coluna da feature `a`.
#[inline]
fn aplica_feature(net: &RedeV3, a: usize, dst: &mut [i16]) {
    let i = a * HL;
    soma_col(dst, &net.l0w[i..i + HL]);
}

/// Subtrai a coluna da feature `a`.
#[inline]
fn remove_feature_v3(net: &RedeV3, a: usize, dst: &mut [i16]) {
    let i = a * HL;
    sub_col(dst, &net.l0w[i..i + HL]);
}

/// Aplica um lote de (indice, somar?) de uma vez.
fn aplica_lista_sinal(net: &RedeV3, ops: &[(u32, bool)], dst: &mut [i16]) {
    for &(a, add) in ops {
        if add { aplica_feature(net, a as usize, dst) } else { remove_feature_v3(net, a as usize, dst) }
    }
}

// ---------------------------------------------------------------------------
// Acumulador incremental
//
// Portado do ramo `threats-cancel`, que o construiu e validou para EXACTAMENTE
// o mesmo conjunto de features (31744) -- so' que a 512 de largura, onde
// mediu tres vezes mais lento que recomputar. Aqui a largura e' 256: cada
// delta toca metade das colunas, que era precisamente o que nao amortizava.
//
// Se voltar a medir pior, a resposta e' a mesma de la': fica documentado e a
// recomputacao mantem-se. O que nao se faz e' assumir sem medir -- a v3 gasta
// 61% do tempo a avaliar (perf), contra ~16% da rede simples, e o tecto se a
// avaliacao fosse gratis sao 1197k nps contra os 467k de agora.
// ---------------------------------------------------------------------------

use crate::bitboard::Bitboard;
use crate::types::{PieceType, Square};

/// The threat feature space, indexed the CANONICAL way: `make_threat_full`
/// called with `side` = the victim's actual colour (0=white, 1=black) and the
/// victim square UNMIRRORED. That is exactly the "white perspective" (`us`)
/// index that `map_features_pairs` produces when called with `stm=0` (see its
/// header comment on why `stm` is always fixed at 0) -- so this doubles as
/// the physical key: one slot per `(attacker_type, victim_type, victim_sq)`,
/// regardless of which or how many actual pieces currently supply it.
#[inline]
fn other_perspective(idx: usize) -> usize {
    let sq = idx & 63;
    let rest = idx >> 6;
    let vt = rest % 6;
    let rest = rest / 6;
    let at = rest % 6;
    let rest = rest / 6;
    let rel = rest & 1;
    let side = rest >> 1;
    crate::features::make_threat_full(1 - side, rel, at, vt, sq ^ 56)
}

/// Twin of `aplica_feature`: subtract instead of add.
#[inline]
fn remove_feature_portado(net: &RedeV3, a: usize, dst: &mut [i16]) {
    let corte = 1 + crate::features::PIECE_FEATURES;
    if a < corte {
        let i = a * HL;
        sub_col(dst, &net.l0w[i..i + HL]);
    } else {
        let i = (a - corte) * HL;
        sub_col(dst, &net.l0w[i..i + HL]);
    }
}

/// Apply (or undo) one physical threat's contribution to BOTH perspectives at
/// once -- they always move together, since one physical attacker/victim pair
/// is a feature in each perspective's own index space.
#[inline]
fn aplica_par_threat(net: &RedeV3, idx: usize, add: bool, us: &mut [i16], them: &mut [i16]) {
    let a_us = crate::features::PIECE_FEATURES + idx;
    let a_them = crate::features::PIECE_FEATURES + other_perspective(idx);
    if add {
        aplica_feature(net, a_us, us);
        aplica_feature(net, a_them, them);
    } else {
        remove_feature_v3(net, a_us, us);
        remove_feature_v3(net, a_them, them);
    }
}

/// Bump the reference count for one physical threat feature, touching the
/// network only on the 0->1 / 1->0 transition. `add=true` means one more
/// physical attacker/victim pair now supplies this slot; `add=false` means
/// one fewer does.
#[inline]
fn bump(cnt: &mut [u8], net: &RedeV3, us: &mut [i16], them: &mut [i16], idx: usize, add: bool) {
    if add {
        cnt[idx] += 1;
        if cnt[idx] == 1 {
            aplica_par_threat(net, idx, true, us, them);
        }
    } else {
        debug_assert!(cnt[idx] > 0, "removendo uma ameaca que o contador diz que nao existe");
        cnt[idx] -= 1;
        if cnt[idx] == 0 {
            aplica_par_threat(net, idx, false, us, them);
        }
    }
}

/// Twin of `bump`, for `materialize`'s batched path: instead of touching the
/// network the moment a physical threat feature's count crosses 0<->1, queue
/// the (index, add) pair for the caller's prefetching batch pass. `idx` is
/// the canonical (white-view) index; both perspectives' engine-space column
/// indices are queued together, exactly as `aplica_par_threat` would apply
/// them, since they always move in lockstep.
#[inline]
fn bump_collect(cnt: &mut [u8], idx: usize, add: bool, us_ops: &mut Vec<(u32, bool)>, them_ops: &mut Vec<(u32, bool)>) {
    let cruzou = if add {
        cnt[idx] += 1;
        cnt[idx] == 1
    } else {
        debug_assert!(cnt[idx] > 0, "removendo uma ameaca que o contador diz que nao existe");
        cnt[idx] -= 1;
        cnt[idx] == 0
    };
    if cruzou {
        us_ops.push(((crate::features::PIECE_FEATURES + idx) as u32, add));
        them_ops.push(((crate::features::PIECE_FEATURES + other_perspective(idx)) as u32, add));
    }
}

/// Piece-feature index for the threats net's own HalfK2 layout (704 planes:
/// no own-king plane, since the king square already picked the bucket).
/// Mirrors `features::map_pieces_pairs`'s inline closure exactly -- this is
/// the one other file allowed to know that layout, so the two must be read
/// side by side if either ever changes.
#[inline]
fn piece_feat(bucket: usize, persp: usize, c: usize, t: usize, sq_raw: usize) -> Option<usize> {
    let sq = if persp == 0 { sq_raw } else { sq_raw ^ 56 };
    if t == 5 {
        if c != persp { Some(bucket * 704 + sq) } else { None }
    } else {
        Some(bucket * 704 + (crate::features::piece_idx(t) + if c == persp { 0 } else { 5 }) * 64 + sq)
    }
}

/// For one piece change at square `s` (either just placed there, or just
/// removed from there -- `occ_with_s`/`occ_without_s` and `mailbox` already
/// reflect whichever is real), call `f(canonical_idx, add)` for every
/// physical threat feature it touches: what it attacks (outgoing), what
/// attacks it (incoming), and what any slider among those attackers reveals
/// or hides beyond `s` (discovered).
///
/// This is the whole reason the piece-square path's per-move branching
/// (quiet/capture/castle/en-passant/promotion) does not need reproducing
/// here: `Board::make_move` already breaks every one of those into a
/// sequence of `remove_piece`/`add_piece` calls (see `board.rs`), and this
/// function only needs to be correct for ONE such call. Castling's rook and
/// a promotion's piece-type change are just two more calls to it.
fn threat_deltas<F: FnMut(usize, bool)>(
    occ_with_s: Bitboard,
    occ_without_s: Bitboard,
    pieces: &[[Bitboard; 6]; 2],
    mailbox: &[Option<(PieceType, crate::types::Color)>; 64],
    pt: PieceType,
    c: crate::types::Color,
    s: Square,
    adding: bool,
    f: &mut F,
) {
    use crate::attacks::{bishop_attacks, rook_attacks};
    use crate::bitboard::bb;
    let atk = crate::evaluation::atk();
    let discovered_add = !adding;

    // Outgoing: what this piece attacks from `s`.
    let attacks = match pt {
        PieceType::Pawn => atk.pawn[c.idx()][s as usize],
        PieceType::Knight => atk.knight[s as usize],
        PieceType::King => atk.king[s as usize],
        PieceType::Bishop => bishop_attacks(s, occ_with_s),
        PieceType::Rook => rook_attacks(s, occ_with_s),
        PieceType::Queen => bishop_attacks(s, occ_with_s) | rook_attacks(s, occ_with_s),
    };
    let mut targets = attacks & occ_with_s & !bb(s);
    while targets != 0 {
        let t = targets.trailing_zeros() as Square;
        targets &= targets - 1;
        if let Some((vt, vc)) = mailbox[t as usize] {
            let rel = (vc == c) as usize;
            f(crate::features::make_threat_full(vc.idx(), rel, pt.idx(), vt.idx(), t as usize), adding);
        }
    }

    // Incoming: who attacks `s` -- this piece as victim -- plus, for every
    // slider found this way, whatever its ray reveals or hides past `s`.
    let bishops_queens = pieces[0][2] | pieces[0][4] | pieces[1][2] | pieces[1][4];
    let rooks_queens = pieces[0][3] | pieces[0][4] | pieces[1][3] | pieces[1][4];
    let mut attackers = 0u64;
    for by in [crate::types::Color::White, crate::types::Color::Black] {
        attackers |= atk.pawn[by.opp().idx()][s as usize] & pieces[by.idx()][0];
    }
    attackers |= atk.knight[s as usize] & (pieces[0][1] | pieces[1][1]);
    attackers |= atk.king[s as usize] & (pieces[0][5] | pieces[1][5]);
    attackers |= bishop_attacks(s, occ_with_s) & bishops_queens;
    attackers |= rook_attacks(s, occ_with_s) & rooks_queens;

    let mut aset = attackers;
    while aset != 0 {
        let a = aset.trailing_zeros() as Square;
        aset &= aset - 1;
        let (at, ac) = match mailbox[a as usize] {
            Some(p) => p,
            None => continue, // the piece at `s` itself, for a king/knight/pawn pattern that is symmetric
        };
        let rel = (ac == c) as usize;
        f(crate::features::make_threat_full(c.idx(), rel, at.idx(), pt.idx(), s as usize), adding);

        if matches!(at, PieceType::Bishop | PieceType::Rook | PieceType::Queen) {
            // One table lookup instead of two magic-bitboard slider calls
            // (attacks with `s` occupied vs without, then diff): `s` blocks
            // this slider's ray past it whenever `s` is occupied at all, so
            // the piece revealed/hidden is simply the first real piece in
            // `ray_extension[a][s]`, found with one AND + one bit-scan. See
            // `Attacks::ray_extension`.
            let candidates = atk.ray_extension[a as usize][s as usize] & occ_without_s;
            if candidates != 0 {
                let rs = if a < s {
                    candidates.trailing_zeros() as Square
                } else {
                    (63 - candidates.leading_zeros()) as Square
                };
                if let Some((rt, rc)) = mailbox[rs as usize] {
                    let rrel = (rc == ac) as usize;
                    f(crate::features::make_threat_full(rc.idx(), rrel, at.idx(), rt.idx(), rs as usize), discovered_add);
                }
            }
        }
    }
}

/// One piece change, recorded but not yet turned into threat-feature deltas.
///
/// Cheap to create -- a handful of `Copy` fields, no `attacks_bb` calls -- by
/// design: this is exactly the work `add_piece`/`remove_piece` used to do
/// immediately, deferred until something actually reads the accumulator.
/// Occupancy and the piece/mailbox snapshot are captured NOW because they
/// describe the board at the moment of this specific change; by the time
/// `materialize` runs, later moves (or later pieces within the SAME move --
/// a capture is three of these) have moved on, and `threat_deltas` needs the
/// occupancy that was actually true when this one piece came or went.
#[derive(Clone, Copy)]
struct PendingEvent {
    pt: PieceType,
    c: Color,
    s: Square,
    adding: bool,
    occ_with_s: Bitboard,
    occ_without_s: Bitboard,
    pieces: [[Bitboard; 6]; 2],
    mailbox: [Option<(PieceType, Color)>; 64],
    /// Piece-feature index (engine space, `< PIECE_FEATURES`), one per
    /// perspective, computed at push time -- cheap, a formula, no
    /// `attacks_bb`. Precomputed here (rather than left for `materialize`
    /// to redo) purely so it can ride in the SAME prefetching batch pass as
    /// the threat deltas instead of being applied on its own; see the
    /// struct note on why applying one column at a time is the thing to
    /// avoid. `None` when this perspective has no feature for it (the
    /// perspective's own king -- see `piece_feat`).
    piece_us: Option<u32>,
    piece_them: Option<u32>,
}

/// The board's threat accumulator: both perspectives' running weight sums.
///
/// `add_piece`/`remove_piece` only ever record a `PendingEvent` -- no
/// `attacks_bb`, no weight-column touch, not even for the piece half.
/// Everything that costs anything happens in `materialize`, called from
/// `valor`, i.e. only for positions something actually reads the score of.
///
/// Measured why the piece half is deferred too, not just threats: profiling
/// an earlier version that applied piece columns immediately (one
/// `aplica_feature` call per changed perspective, right there in
/// `add_piece`/`remove_piece`) showed `aplica_feature`/`remove_feature`
/// alone at 58% of total search time, with the actual `threat_deltas`
/// geometry not even appearing in the top ten. The cost was never the
/// `attacks_bb` calls -- it was touching a 512-wide column in a table that
/// does not fit cache, one at a time, with nothing to prefetch ahead of it.
/// `evaluate()`'s own `aplica_lista` already solved exactly this for a full
/// recompute (gather indices first, prefetch the next column while summing
/// the current one); `materialize` does the same thing for however many
/// piece and threat touches accumulated since the last read.
///
/// This needs no special handling for undo. `unmake_move` calls the same
/// `add_piece`/`remove_piece` as any other change, in reverse, which records
/// the exact geometric inverse of the original event (same occupancy, since
/// nothing else has touched the board in between -- any moves made and
/// unmade inside this one's subtree already cancelled by the time we get
/// here). Materializing a change and then its inverse nets to no-ops on
/// every feature it touched, in any processing order (reference counts only
/// care about the final tally, not the order two increments and two
/// decrements happen in) -- so a long run of moves and undos between two
/// `valor` calls is exactly as correct as materializing after every single
/// one, just cheaper.
#[derive(Clone)]
struct AccV3Inner {
    us: [i16; HL],
    them: [i16; HL],
    /// Reference count per physical threat feature (canonical/white-view
    /// index). See the struct note on why this can't be a bitset.
    cnt: Vec<u8>,
    /// Which king bucket each perspective's PIECE half is currently written
    /// for. The threat half has no king bucket at all (see `features.rs`),
    /// so a bucket crossing only ever touches pieces.
    piece_bucket: [usize; 2],
    /// Com que regime de fase este acumulador foi construido: `true` = com
    /// ameacas. Ver `features::AMEACAS_MIN_PECAS`.
    ///
    /// Sem isto o incremental diverge assim que uma captura cruza o limiar:
    /// a recomputacao deixa de emitir ameacas e o acumulador continua a
    /// carregar as que ja' la' tinha. Medido antes de existir este campo:
    /// 2850 divergencias em 3906 lances.
    com_ameacas: bool,
    /// Pecas no tabuleiro depois do ultimo evento registado.
    n_pecas: u32,
    /// Os bitboards depois do ultimo evento -- o que `reconstroi` precisa.
    pecas: [[Bitboard; 6]; 2],
    pending: Vec<PendingEvent>,
    /// Scratch space for `materialize`'s batch pass, kept here and `clear`ed
    /// rather than allocated fresh each call -- `materialize` runs on
    /// essentially every node (see `valor`), so a `Vec::with_capacity` there
    /// would trade the very allocations this design exists to avoid for a
    /// new one just as frequent.
    us_ops: Vec<(u32, bool)>,
    them_ops: Vec<(u32, bool)>,
}

/// `valor` is called through `&Board` (the search treats evaluation as
/// read-only everywhere), but materializing needs to mutate `us`/`them`/
/// `cnt`/`pending`. A `RefCell` gives that without widening `evaluate`'s
/// signature to `&mut Board` across every call site in `search.rs`.
/// `add_piece`/`remove_piece` already hold a real `&mut Board` and use
/// `get_mut` instead, which skips the (otherwise harmless, just not free)
/// runtime borrow check.
pub struct AccV3(std::cell::RefCell<AccV3Inner>);

impl Clone for AccV3 {
    fn clone(&self) -> Self {
        AccV3(std::cell::RefCell::new(self.0.borrow().clone()))
    }
}

pub static MEXIDAS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static CHAMADAS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl AccV3Inner {
    /// Reconstroi tudo a partir dos bitboards guardados.
    ///
    /// Usada quando a regra de fase liga ou desliga as ameacas: nesse
    /// instante o conjunto de features muda em bloco e nenhum delta o
    /// descreve.
    fn reconstroi(&mut self, net: &RedeV3, pecas: [[Bitboard; 6]; 2]) {
        // Os bitboards vem de fora, do TABULEIRO, e nao de `self.pecas` --
        // esse guarda o ultimo evento, que a meio de uma captura descreve uma
        // posicao que nunca existiu. Mesma razao pela qual a fase e' lida do
        // tabuleiro; corrigir uma sem a outra nao corrige nada, e foi o que
        // aconteceu a primeira vez.
        let pos = Pos { pieces: pecas };
        for i in 0..HL {
            self.us[i] = net.l0b[i];
            self.them[i] = net.l0b[i];
        }
        for c in self.cnt.iter_mut() { *c = 0; }
        let (us, them, cnt) = (&mut self.us, &mut self.them, &mut self.cnt);
        map_features_pairs_fase(&pos, 0, 2u8, &mut |a: usize, b: usize| {
            if a < crate::features::PIECE_FEATURES {
                aplica_feature(net, a, us);
                aplica_feature(net, b, them);
            } else {
                let idx = a - crate::features::PIECE_FEATURES;
                bump(cnt, net, us, them, idx, true);
            }
        });
        self.n_pecas = pos.occ().count_ones();
        self.pecas = pecas;
        self.com_ameacas = self.n_pecas >= crate::features::AMEACAS_MIN_PECAS;
        // O bucket cacheado tambem faz parte do estado, e reconstruir sem ele
        // deixa o acumulador certo e a etiqueta errada. `map_features_pairs`
        // calcula o bucket a partir dos reis e nao o diz a ninguem; o
        // `fix_bucket` acredita neste campo. Ficando obsoleto, o lance de rei
        // seguinte tira colunas de um bucket que ja' nao esta' la' dentro.
        //
        // Sintoma: divergencias so' abaixo do limiar das ameacas -- nao porque
        // as ameacas tivessem que ver com isto, mas porque so' se reconstroi
        // ao cruzar o limiar, e era a reconstrucao que sujava a etiqueta.
        for persp in 0..2usize {
            let ks_raw = pecas[persp][5].trailing_zeros() as usize;
            let ks = if persp == 0 { ks_raw } else { ks_raw ^ 56 };
            self.piece_bucket[persp] = crate::features::BUCKET_MAP[ks];
        }
    }

    /// Record a change, or cancel it against its own inverse.
    ///
    /// This is the piece the three earlier attempts at an incremental threat
    /// accumulator were missing, and it is why they all measured slower than
    /// full recompute. `generate_legal` settles legality by making and
    /// unmaking each candidate move, and the search itself makes and unmakes
    /// around every node -- so most piece changes arrive in pairs that undo
    /// each other with nothing evaluated in between. Deferring them was not
    /// enough; they still had to be processed eventually. Cancelling means
    /// they are never processed at all.
    ///
    /// Only the tail is examined, which is all that is needed: make/unmake is
    /// perfectly nested, so an inverse always arrives adjacent to what it
    /// undoes. The geometry snapshot each event carries is irrelevant here --
    /// a cancelled pair is never materialised, so the occupancy it would have
    /// been evaluated against never matters.
    /// Returns true if this change cancelled the last recorded one, in which
    /// case nothing needs recording at all.
    ///
    /// Deliberately takes the four identifying fields rather than a built
    /// `PendingEvent`: the event carries a snapshot of the piece bitboards
    /// and the mailbox -- about 250 bytes, against six for the piece-square
    /// accumulator's record -- because `threat_deltas` has to be evaluated
    /// against the occupancy that was true when the change happened, not
    /// whatever the board looks like later. Building that snapshot and then
    /// throwing it away is most of the cost of a pair that cancels, and most
    /// pairs cancel. So the question is asked first, and the snapshot is only
    /// taken by callers when the answer is no.
    #[inline]
    fn cancela(&mut self, pt: PieceType, c: Color, s: Square, adding: bool) -> bool {
        if let Some(ultimo) = self.pending.last() {
            if ultimo.s == s && ultimo.pt == pt && ultimo.c == c && ultimo.adding != adding {
                self.pending.pop();
                return true;
            }
        }
        false
    }
}

impl AccV3 {
    /// Build from scratch. Used once per game (`Board::from_fen`) and again
    /// whenever a king bucket crossing needs the piece half rebuilt outright
    /// rather than delta-swapped (not currently -- see `fix_bucket`, which
    /// deltas instead -- but kept for whoever adds a Finny-table cache later
    /// and wants a known-correct fallback to check it against).
    pub fn fresh(net: &RedeV3, board: &Board) -> Self {
        let mut pos = Pos::default();
        for c in 0..2 {
            for t in 0..6 {
                pos.pieces[c][t] = board.pieces[c][t];
            }
        }
        let mut inner = AccV3Inner {
            us: [0i16; HL],
            them: [0i16; HL],
            cnt: vec![0u8; crate::features::THREAT_FEATURES_FULL],
            piece_bucket: [
                crate::features::BUCKET_MAP[board.king_sq(Color::White) as usize],
                crate::features::BUCKET_MAP[(board.king_sq(Color::Black) as usize) ^ 56],
            ],
            com_ameacas: board.occ_all.count_ones() >= crate::features::AMEACAS_MIN_PECAS,
            n_pecas: board.occ_all.count_ones(),
            pecas: board.pieces,
            pending: Vec::new(),
            us_ops: Vec::new(),
            them_ops: Vec::new(),
        };
        inner.us.copy_from_slice(&net.l0b);
        inner.them.copy_from_slice(&net.l0b);
                map_features_pairs_fase(&pos, 0, 2u8, &mut |a: usize, b: usize| {
            if a < crate::features::PIECE_FEATURES {
                aplica_feature(net, a, &mut inner.us);
                aplica_feature(net, b, &mut inner.them);
            } else {
                let idx = a - crate::features::PIECE_FEATURES;
                debug_assert_eq!(b - crate::features::PIECE_FEATURES, other_perspective(idx));
                bump(&mut inner.cnt, net, &mut inner.us, &mut inner.them, idx, true);
            }
        });
        AccV3(std::cell::RefCell::new(inner))
    }

    /// Called after `board.rs` has already applied a piece to the board
    /// (bitboards, occupancy, mailbox all reflect the new state). Just
    /// records the event -- see the struct note on why even the piece half
    /// waits for `materialize` now.
    pub fn add_piece(
        &mut self,
        _net: &RedeV3,
        pt: PieceType,
        c: Color,
        s: Square,
        occ_all: Bitboard,
        pieces: [[Bitboard; 6]; 2],
        mailbox: [Option<(PieceType, Color)>; 64],
    ) {
        let inner = self.0.get_mut();
        // Guardado a cada evento, e nao lido do tabuleiro em `materialize`:
        // quando a materializacao acontece o tabuleiro ja' andou para a
        // frente, e o que interessa e' o estado deste evento.
        inner.n_pecas = occ_all.count_ones();
        inner.pecas = pieces;
        if inner.cancela(pt, c, s, true) {
            return;
        }
        let (piece_us, piece_them) = (
            piece_feat(inner.piece_bucket[0], 0, c.idx(), pt.idx(), s as usize).map(|i| i as u32),
            piece_feat(inner.piece_bucket[1], 1, c.idx(), pt.idx(), s as usize).map(|i| i as u32),
        );
        use crate::bitboard::bb;
        inner.pending.push(PendingEvent {
            pt, c, s, adding: true,
            occ_with_s: occ_all,
            occ_without_s: occ_all & !bb(s),
            pieces, mailbox,
            piece_us, piece_them,
        });
    }

    /// Twin of `add_piece`, called after `board.rs` has already removed the
    /// piece (bitboards, occupancy, mailbox all reflect its absence).
    pub fn remove_piece(
        &mut self,
        _net: &RedeV3,
        pt: PieceType,
        c: Color,
        s: Square,
        occ_all: Bitboard,
        pieces: [[Bitboard; 6]; 2],
        mailbox: [Option<(PieceType, Color)>; 64],
    ) {
        let inner = self.0.get_mut();
        inner.n_pecas = occ_all.count_ones();
        inner.pecas = pieces;
        if inner.cancela(pt, c, s, false) {
            return;
        }
        let (piece_us, piece_them) = (
            piece_feat(inner.piece_bucket[0], 0, c.idx(), pt.idx(), s as usize).map(|i| i as u32),
            piece_feat(inner.piece_bucket[1], 1, c.idx(), pt.idx(), s as usize).map(|i| i as u32),
        );
        use crate::bitboard::bb;
        inner.pending.push(PendingEvent {
            pt, c, s, adding: false,
            occ_with_s: occ_all | bb(s),
            occ_without_s: occ_all,
            pieces, mailbox,
            piece_us, piece_them,
        });
    }

    /// Turn every pending event into feature-column touches, gathered into
    /// two flat lists (one per perspective) and applied in ONE prefetching
    /// pass each -- see the struct note on why this is not done inline as
    /// each pending event is processed. Threat features still go through
    /// the 0<->1 reference-count gate (`bump_collect`); piece features have
    /// no such gate, they are simply queued as-is (`ev.adding`). Drains
    /// `pending` completely: once folded into `cnt`/`us`/`them`, an event's
    /// own snapshot is no longer needed for anything.
    fn materialize(net: &RedeV3, inner: &mut AccV3Inner, pecas_tabuleiro: [[Bitboard; 6]; 2]) {
        // A fase e' verificada ANTES do teste de "nada pendente", e nao
        // depois: o cancelamento de pares inversos esvazia a lista, portanto
        // uma posicao pode chegar aqui com zero eventos e ainda assim estar
        // do outro lado do limiar. Verificada depois, a travessia passava
        // despercebida -- restavam 10 divergencias em 3906 lances, todas em
        // posicoes de onze pecas, mesmo em cima do corte de doze.
        // A fase e' lida do TABULEIRO, nao do ultimo evento.
        //
        // Um lance nao e' atomico aqui: uma captura chega como tres eventos
        // (tirar a peca capturada, levantar a que move, poussa-la), e a meio
        // disso a contagem de pecas passa por valores que nunca existem numa
        // posicao real -- desce a dez e volta a onze. Reconstruir a partir
        // desse estado intermedio produz um acumulador para uma posicao que
        // nao existe. Eram as dez divergencias que restavam, todas a onze
        // pecas, mesmo em cima do corte.
        //
        // O tabuleiro passado ao `valor` esta' sempre num estado valido, por
        // definicao: e' o que a busca vai avaliar.
        let n_pecas_tabuleiro: u32 = pecas_tabuleiro.iter().flatten().map(|b| b.count_ones()).sum();
        let agora = n_pecas_tabuleiro >= crate::features::AMEACAS_MIN_PECAS;
        if agora != inner.com_ameacas {
            inner.pending.clear();
            inner.reconstroi(net, pecas_tabuleiro);
            return;
        }
        if inner.pending.is_empty() {
            return;
        }
        // A regra de fase muda o CONJUNTO de features, nao os seus valores:
        // ao cruzar `AMEACAS_MIN_PECAS` as 9216 colunas de ameaca deixam de
        // existir de uma vez. Nenhum delta exprime isso -- os deltas dizem "o
        // que este lance mudou", nao "metade das features desapareceu". A
        // unica resposta correcta e' reconstruir.
        //
        // Custa uma reconstrucao por travessia do limiar, que num jogo
        // acontece meia duzia de vezes. Sem isto o incremental divergia em
        // 2850 de 3906 lances.
        inner.us_ops.clear();
        inner.them_ops.clear();
        let mut mexidas = 0u64;
        let ameacas = true;
        let inner_com_ameacas = inner.com_ameacas;
        let (pending, cnt, us_ops, them_ops) =
            (&mut inner.pending, &mut inner.cnt, &mut inner.us_ops, &mut inner.them_ops);
        for ev in pending.drain(..) {
            if let Some(a) = ev.piece_us {
                us_ops.push((a, ev.adding));
            }
            if let Some(a) = ev.piece_them {
                them_ops.push((a, ev.adding));
            }
            // A regra de fase tem de valer AQUI tambem, e nao so' na
            // recomputacao: `threat_deltas` calcula ameacas sempre, portanto
            // abaixo do limiar o incremental somava colunas que o outro
            // caminho ja' nao emite. Divergia em posicoes de onze pecas sem
            // sequer haver travessia -- bastava um lance tranquilo.
            if !ameacas || !inner_com_ameacas {
                continue;
            }
            threat_deltas(
                ev.occ_with_s, ev.occ_without_s, &ev.pieces, &ev.mailbox,
                ev.pt, ev.c, ev.s, ev.adding,
                &mut |idx, add| {
                    bump_collect(cnt, idx, add, us_ops, them_ops);
                    mexidas += 1;
                },
            );
        }
        aplica_lista_sinal(net, &inner.us_ops, &mut inner.us);
        aplica_lista_sinal(net, &inner.them_ops, &mut inner.them);
        MEXIDAS.fetch_add(mexidas, std::sync::atomic::Ordering::Relaxed);
        CHAMADAS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// A king crossing a bucket boundary invalidates every PIECE feature for
    /// its own perspective (same input count, different meaning). Threats
    /// are untouched -- they carry no king bucket -- so this deltas the
    /// piece half in place (old column out, new column in for all 32
    /// pieces) instead of resetting to bias, which would also wipe out
    /// whatever threat contribution is already summed into `us`/`them`.
    pub fn fix_bucket(&mut self, net: &RedeV3, pieces: [[Bitboard; 6]; 2]) {
        let inner = self.0.get_mut();
        // Applied directly below (old column out, new in), not queued --
        // which means `us`/`them` must already reflect every piece pending
        // hasn't caught up on yet, or the swap deltas against the wrong
        // baseline. Materializing first is cheap when there is nothing
        // pending (the empty check at the top of `materialize` returns
        // immediately) and correct when there is.
        Self::materialize(net, inner, pieces);
        for persp in 0..2usize {
            let ks_raw = pieces[persp][5].trailing_zeros() as usize;
            let ks = if persp == 0 { ks_raw } else { ks_raw ^ 56 };
            let quer = crate::features::BUCKET_MAP[ks];
            if inner.piece_bucket[persp] == quer {
                continue;
            }
            let velho = inner.piece_bucket[persp];
            inner.piece_bucket[persp] = quer;
            let dst = if persp == 0 { &mut inner.us } else { &mut inner.them };
            for c in 0..2 {
                for t in 0..6 {
                    let mut bb_ = pieces[c][t];
                    while bb_ != 0 {
                        let sq_raw = bb_.trailing_zeros() as usize;
                        bb_ &= bb_ - 1;
                        if let Some(old_idx) = piece_feat(velho, persp, c, t, sq_raw) {
                            remove_feature_v3(net, old_idx, dst);
                        }
                        if let Some(new_idx) = piece_feat(quer, persp, c, t, sq_raw) {
                            aplica_feature(net, new_idx, dst);
                        }
                    }
                }
            }
        }
    }

    /// Bring the accumulator up to date (processing whatever moves happened
    /// since the last read) and read the score. `&self` because the search
    /// treats evaluation as read-only -- see the `AccV3` struct note on
    /// why materializing through a `RefCell` is what makes that possible.
    /// Com que regime de fase o acumulador esta' construido, e sobre quantas
    /// pecas. Diagnostico: as divergencias apareciam todas em cima do corte
    /// das doze pecas, e supor de que lado estava o acumulador nao substitui
    /// perguntar-lhe.
    pub fn debug_fase(&self) -> (bool, u32) {
        let i = self.0.borrow();
        (i.com_ameacas, i.n_pecas)
    }

    pub fn valor(&self, net: &RedeV3, board: &Board) -> i32 {
        let mut inner = self.0.borrow_mut();
        Self::materialize(net, &mut inner, board.pieces);
        let ob = output_bucket(board);
        let (a, b) = if board.side == Color::White {
            (&inner.us, &inner.them)
        } else {
            (&inner.them, &inner.us)
        };
        cabeca(net, a, b, ob)
    }
}

