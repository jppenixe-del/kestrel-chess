//! The threats network: `(31744 -> HL)x2 -> 8`.
//!
//! Same shape as the plain one, different inputs -- and the inputs are the
//! whole point. Piece-square features say where the pieces are; threat
//! features say what is pointing at what. Measured on our own training runs,
//! that difference took the loss from 0.0251 after two hundred and forty
//! superbatches to 0.0153 after thirty-one.
//!
//! Recomputed per position rather than updated per move, for now. That is
//! slow and it is deliberate: threats change on almost every move -- moving
//! any piece rewrites its attack map -- so the incremental version needs a
//! delta machinery of its own, and building that before knowing whether the
//! network is worth it would be the wrong order. Correct first, then fast.

use crate::board::Board;
use crate::features::{map_features_pairs_mode, Pos, TOTAL_INPUTS_V10};
use crate::types::Color;

pub const INPUTS: usize = TOTAL_INPUTS_V10; // 31744
pub const HIDDEN: usize = 512;
const QA: i32 = 255;
const QB: i32 = 64;
const SCALE: i32 = 400;
const OUT_BUCKETS: usize = 8;

pub struct RedeThreats {
    /// Piece-feature weights, i16. There are 22528 of these columns and they
    /// are the ones whose magnitude matters.
    pub l0w: Vec<i16>,
    /// Threat-feature weights, i8.
    ///
    /// Halved on purpose, and it is the one optimisation that targets what
    /// actually costs here. The full first layer is 32.5 MB against the plain
    /// network's 786 kB -- one fits in cache and the other does not, so every
    /// feature applied is a kilobyte fetched from RAM and the accumulator is
    /// bandwidth-bound, not compute-bound. That is why magic bitboards changed
    /// nothing and why reordering the enumeration made it slower: the work was
    /// never in the arithmetic.
    ///
    /// Threat weights are small -- measured on our own network, 1.9% fall
    /// outside i8 and clamping them moves a column's magnitude by 0.9% at the
    /// median. Halving the bytes moved is worth more than that.
    pub l0w_threats: Vec<i8>,
    pub l0b: Vec<i16>,
    pub l1w: Vec<i16>,
    pub l1b: Vec<i16>,
}

/// The trainer's input count is `1 + 31744` -- one leading slot it reserves.
/// Reproduced here rather than trimmed, because the weights are laid out
/// against it and shifting them by one row reads every feature off by one.
const TRAINER_INPUTS: usize = 1 + TOTAL_INPUTS_V10;

pub fn load(bytes: &[u8]) -> Option<RedeThreats> {
    let cauda = HIDDEN + 2 * HIDDEN * OUT_BUCKETS + OUT_BUCKETS;
    let precisa = TRAINER_INPUTS * HIDDEN + cauda;
    let total = bytes.len() / 2;
    if total < precisa {
        eprintln!(
            "nnue-threats: ficheiro tem {} valores, precisa de {} (HL={})",
            total, precisa, HIDDEN
        );
        return None;
    }
    let mut it = bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]));
    let todos: Vec<i16> = (&mut it).take(TRAINER_INPUTS * HIDDEN).collect();
    // Split at the threat boundary. The leading slot the trainer reserves
    // stays with the piece block, which keeps the piece indices unchanged.
    let corte = (1 + crate::features::PIECE_FEATURES) * HIDDEN;
    let l0w: Vec<i16> = todos[..corte].to_vec();
    let mut fora = 0usize;
    let l0w_threats: Vec<i8> = todos[corte..]
        .iter()
        .map(|&x| {
            if x < -128 || x > 127 {
                fora += 1;
            }
            x.clamp(-128, 127) as i8
        })
        .collect();
    if fora > 0 {
        eprintln!(
            "nnue-threats: {} pesos ({:.2}%) cortados para i8",
            fora,
            100.0 * fora as f64 / l0w_threats.len() as f64
        );
    }
    let l0b: Vec<i16> = (&mut it).take(HIDDEN).collect();
    let l1w: Vec<i16> = (&mut it).take(2 * HIDDEN * OUT_BUCKETS).collect();
    let l1b: Vec<i16> = (&mut it).take(OUT_BUCKETS).collect();
    eprintln!("nnue-threats: rede carregada ({} entradas, HL={})", INPUTS, HIDDEN);
    Some(RedeThreats { l0w, l0w_threats, l0b, l1w, l1b })
}

#[inline]
fn screlu(x: i16) -> i32 {
    let v = (x as i32).clamp(0, QA);
    v * v
}

pub fn evaluate(net: &RedeThreats, board: &Board) -> i32 {
    let mut pos = Pos::default();
    for c in 0..2 {
        for t in 0..6 {
            pos.pieces[c][t] = board.pieces[c][t];
        }
    }
    // Enumerado SEMPRE da perspetiva das brancas primeiro.
    //
    // A funcao devolve o par (perspetiva de quem joga, perspetiva do outro).
    // Pedir-lhe isso directamente indexa o acumulador por QUEM JOGA, e entao
    // uma simples troca de lado renumera todas as features -- medido, cento e
    // oito de duzentas mudavam a cada avaliacao, metade delas so' porque o
    // turno passou. Fixando as brancas em primeiro, o par passa a ser
    // (brancas, pretas) e o turno deixa de mexer em nada; quem escolhe qual e'
    // "nossa" e' a leitura, la' em baixo.
    let stm = 0usize;

    // i16, not i32.
    //
    // Halving the accumulator halves the memory traffic of the inner loop,
    // and the inner loop is where this function spends its time: measured,
    // enumerating the features costs 1.8us per node and adding the weight
    // columns costs 4.0us. Overflow is not a risk at this width -- the
    // trainer clips the first layer so that no combination of active features
    // can leave i16, which is the same guarantee the piece-square network
    // relies on.
    let mut us = vec![0i16; HIDDEN];
    let mut them = vec![0i16; HIDDEN];
    us.copy_from_slice(&net.l0b);
    them.copy_from_slice(&net.l0b);
    // The pair callback gives both perspectives of the same physical thing at
    // once, which is what keeps the two accumulators in step by construction.
    // DIAGNOSTICO: KESTREL_SO_ENUM salta as somas, para separar o custo de
    // enumerar do custo de aplicar.
    static SO_ENUM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let so_enum = *SO_ENUM.get_or_init(|| std::env::var_os("KESTREL_SO_ENUM").is_some());
    let mut n_feat = 0usize;
    map_features_pairs_mode(&pos, stm, 2, &mut |a: usize, b: usize| {
        n_feat += 1;
        if so_enum { return; }
        aplica_feature(net, a, &mut us);
        aplica_feature(net, b, &mut them);
    });
    let _ = n_feat;

    let n = board.occ_all.count_ones() as usize;
    let ob = ((n.max(1) - 1) / 4).min(OUT_BUCKETS - 1);
    let base = ob * 2 * HIDDEN;
    let w = &net.l1w[base..base + 2 * HIDDEN];

    // `us` guarda as brancas e `them` as pretas; a rede quer quem joga
    // primeiro. Trocar aqui e' de graca e mantem o acumulador estavel.
    let (a, b) = if board.side == Color::White { (&us, &them) } else { (&them, &us) };
    let soma = saida_par(a, b, w);
    (soma / QA + net.l1b[ob] as i32) * SCALE / (QA * QB)
}

/// Add one feature's column, from whichever block holds it.
///
/// `a` is an index into the trainer's layout: the leading slot plus the piece
/// features, then the threats. Threat columns live in the i8 block and are
/// widened as they are added.
#[inline]
fn aplica_feature(net: &RedeThreats, a: usize, dst: &mut [i16]) {
    let corte = 1 + crate::features::PIECE_FEATURES;
    if a < corte {
        let i = a * HIDDEN;
        soma_col(dst, &net.l0w[i..i + HIDDEN]);
    } else {
        let i = (a - corte) * HIDDEN;
        soma_col8(dst, &net.l0w_threats[i..i + HIDDEN]);
    }
}

/// `dst += col` where the column is i8, widened on the fly.
#[inline]
fn soma_col8(dst: &mut [i16], col: &[i8]) {
    #[cfg(target_arch = "x86_64")]
    {
        if tem_avx2() {
            unsafe {
                use std::arch::x86_64::*;
                let mut i = 0;
                while i + 16 <= HIDDEN {
                    // Sixteen i8 loaded as one 128-bit half, sign-extended to
                    // sixteen i16 -- the widening is a single instruction, so
                    // the halved memory traffic costs nothing to undo.
                    let b8 = _mm_loadu_si128(col.as_ptr().add(i) as *const __m128i);
                    let b16 = _mm256_cvtepi8_epi16(b8);
                    let a16 = _mm256_loadu_si256(dst.as_ptr().add(i) as *const __m256i);
                    _mm256_storeu_si256(
                        dst.as_mut_ptr().add(i) as *mut __m256i,
                        _mm256_add_epi16(a16, b16),
                    );
                    i += 16;
                }
                while i < HIDDEN {
                    dst[i] += col[i] as i16;
                    i += 1;
                }
            }
            return;
        }
    }
    for i in 0..HIDDEN {
        dst[i] += col[i] as i16;
    }
}

/// `dst += col`, vectorised when the machine allows it.
///
/// This is the hot loop: it runs once per active feature per node, which is
/// around a hundred times. The scalar version was costing more than the
/// feature enumeration and the search around it put together.
#[inline]
fn soma_col(dst: &mut [i16], col: &[i16]) {
    #[cfg(target_arch = "x86_64")]
    {
        if tem_avx2() {
            unsafe {
                use std::arch::x86_64::*;
                let mut i = 0;
                while i + 16 <= HIDDEN {
                    let a = _mm256_loadu_si256(dst.as_ptr().add(i) as *const __m256i);
                    let b = _mm256_loadu_si256(col.as_ptr().add(i) as *const __m256i);
                    _mm256_storeu_si256(
                        dst.as_mut_ptr().add(i) as *mut __m256i,
                        _mm256_add_epi16(a, b),
                    );
                    i += 16;
                }
                while i < HIDDEN {
                    dst[i] += col[i];
                    i += 1;
                }
            }
            return;
        }
    }
    for i in 0..HIDDEN {
        dst[i] += col[i];
    }
}

/// `dst -= col` where the column is i8. Twin of `soma_col8`.
#[inline]
fn sub_col8(dst: &mut [i16], col: &[i8]) {
    #[cfg(target_arch = "x86_64")]
    {
        if tem_avx2() {
            unsafe {
                use std::arch::x86_64::*;
                let mut i = 0;
                while i + 16 <= HIDDEN {
                    let b8 = _mm_loadu_si128(col.as_ptr().add(i) as *const __m128i);
                    let b16 = _mm256_cvtepi8_epi16(b8);
                    let a16 = _mm256_loadu_si256(dst.as_ptr().add(i) as *const __m256i);
                    _mm256_storeu_si256(
                        dst.as_mut_ptr().add(i) as *mut __m256i,
                        _mm256_sub_epi16(a16, b16),
                    );
                    i += 16;
                }
                while i < HIDDEN {
                    dst[i] -= col[i] as i16;
                    i += 1;
                }
            }
            return;
        }
    }
    for i in 0..HIDDEN {
        dst[i] -= col[i] as i16;
    }
}

/// `dst -= col`, vectorised. Twin of `soma_col`.
#[inline]
fn sub_col(dst: &mut [i16], col: &[i16]) {
    #[cfg(target_arch = "x86_64")]
    {
        if tem_avx2() {
            unsafe {
                use std::arch::x86_64::*;
                let mut i = 0;
                while i + 16 <= HIDDEN {
                    let a = _mm256_loadu_si256(dst.as_ptr().add(i) as *const __m256i);
                    let b = _mm256_loadu_si256(col.as_ptr().add(i) as *const __m256i);
                    _mm256_storeu_si256(
                        dst.as_mut_ptr().add(i) as *mut __m256i,
                        _mm256_sub_epi16(a, b),
                    );
                    i += 16;
                }
                while i < HIDDEN {
                    dst[i] -= col[i];
                    i += 1;
                }
            }
            return;
        }
    }
    for i in 0..HIDDEN {
        dst[i] -= col[i];
    }
}

/// The output sum over both perspectives, vectorised.
///
/// Same shape as the piece-square network's kernel and written from the same
/// scalar reference: clamp in i16, multiply the clamped value by the weight
/// (which fits), then `madd` against the clamped value again to get
/// `x * x * w` accumulated in i32 lanes. Squaring first would leave i16.
fn saida_par(us: &[i16], them: &[i16], w: &[i16]) -> i32 {
    #[cfg(target_arch = "x86_64")]
    {
        if tem_avx2() {
            unsafe {
                use std::arch::x86_64::*;
                let zero = _mm256_setzero_si256();
                let topo = _mm256_set1_epi16(QA as i16);
                let mut acc = _mm256_setzero_si256();
                let mut i = 0;
                while i + 16 <= HIDDEN {
                    for (v, off) in [(us, 0usize), (them, HIDDEN)] {
                        let x = _mm256_loadu_si256(v.as_ptr().add(i) as *const __m256i);
                        let wv =
                            _mm256_loadu_si256(w.as_ptr().add(off + i) as *const __m256i);
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
                while i < HIDDEN {
                    total += screlu(us[i]) * w[i] as i32;
                    total += screlu(them[i]) * w[HIDDEN + i] as i32;
                    i += 1;
                }
                return total;
            }
        }
    }
    let mut total = 0i32;
    for i in 0..HIDDEN {
        total += screlu(us[i]) * w[i] as i32;
        total += screlu(them[i]) * w[HIDDEN + i] as i32;
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

static REDE: std::sync::OnceLock<Option<RedeThreats>> = std::sync::OnceLock::new();

pub fn rede() -> Option<&'static RedeThreats> {
    REDE.get_or_init(|| {
        let path = std::env::var("KESTREL_NNUE_THREATS").ok()?;
        match std::fs::read(&path) {
            Ok(b) => load(&b),
            Err(e) => {
                eprintln!("nnue-threats: nao consegui ler {}: {}", path, e);
                None
            }
        }
    })
    .as_ref()
}

// ---------------------------------------------------------------------------
// Incremental accumulator
//
// Threats are not like piece-square features. Moving any piece rewrites its
// attack map, so a move changes far more than the two or three inputs a
// piece-square set would -- but far less than everything. Measured over 744
// moves of our own games: 58 threat features active in a typical position, and
// a median of 12 of them change per move. Twelve against fifty-eight is a
// five-fold saving, which is worth the machinery; twelve against two is why
// this needs machinery at all and cannot reuse the piece-square path.
//
// The approach is deliberately the simple correct one: enumerate the feature
// set for the new position, diff it against the previous set, and apply only
// the difference. That costs one enumeration per move -- cheap, it is
// bitboard work -- and avoids deriving deltas from the move itself, which is
// where this kind of code goes wrong: a castling rook, an en-passant capture
// or a promotion each rewrite attack maps in ways that are easy to enumerate
// and hard to reason about case by case.
//
// The invariant: `activas` always holds exactly the features that `valores`
// was built from. Update both or neither.
// ---------------------------------------------------------------------------

// MEDIDO, e o resultado foi negativo -- fica aqui porque a medicao vale mais
// do que o codigo:
//
// O caminho ate' aqui, com os numeros de cada passo (mesma posicao, 3s):
//
//   escalar, recomputa tudo ....................... 173k nos/s
//   + AVX2 nas somas e na saida ................... 287k
//   + diff por lista ordenada ..................... 244k  (PIOR)
//   + diff por bitset ............................. 231k  (PIOR)
//   + acumulador indexado por COR, nao por turno ... 321k
//
// As duas tentativas de diff falharam pela mesma razao, que so' apareceu ao
// contar: cento e oito das duzentas features mudavam a cada avaliacao. Nao
// era o xadrez -- era o acumulador estar indexado por QUEM JOGA. A funcao de
// features devolve (perspetiva de quem joga, perspetiva do outro), portanto
// bastava o turno passar para tudo trocar de lugar. Fixando as brancas em
// primeiro e trocando so' na leitura, as mudancas cairam para 39.8 na busca e
// 17.7 numa sequencia de jogo real, e o diff passou a compensar.
//
// A licao, que custou tres tentativas: quando um diff nao paga, contar quanto
// esta' mesmo a mudar antes de o optimizar. As duas primeiras versoes estavam
// CORRECTAS e eram inuteis, porque o problema nao era o diff.
//
// A enumeracao e' barata; o que custa e' somar colunas de pesos. Este
// incremental poupa quarenta e seis aplicacoes e paga uma ordenacao e um
// diff por no' -- e a conta da negativo. Esta CORRECTO (mil e trinta e nove
// posicoes em sequencia, zero divergencias), so' nao e' mais rapido.
//
// O erro de desenho: re-enumerar tudo e comparar depois. Um incremental a
// serio nunca re-enumera -- deriva as alteracoes do proprio lance, no
// make_move, e so' toca no que o lance mexeu. E' um trabalho de outra ordem
// e e' o caminho certo quando se voltar a isto.
//
// Fica compilado e testavel; a avaliacao usa a recomputacao.

/// One side's accumulator plus the feature set it was built from.
///
/// The set is kept as a BITSET, not a sorted list, and that is the whole
/// design. Measured, applying weight columns is 2.53us per node against 0.68us
/// to enumerate the features -- so the win is in applying fewer of them, and a
/// diff is the way to apply fewer. The first attempt sorted both sets and
/// walked them together: correct, and slower than recomputing everything,
/// because sorting a hundred pairs cost more than the forty-six applications
/// it saved.
///
/// A bitset has no such cost. Thirty-one thousand bits is four hundred and
/// ninety-six words; XOR against the previous position gives every change in
/// one branch-free pass, and only the set bits of the result need touching.
/// The two perspectives are diffed independently because they feed separate
/// accumulators -- nothing needs the pairing once the features are known.
const PALAVRAS: usize = (INPUTS + 63) / 64;

pub struct AccThreats {
    pub us: Vec<i16>,
    pub them: Vec<i16>,
    bits_us: Vec<u64>,
    bits_them: Vec<u64>,
    novo_us: Vec<u64>,
    novo_them: Vec<u64>,
    pub valido: bool,
}

impl AccThreats {
    pub fn novo() -> Self {
        AccThreats {
            us: vec![0i16; HIDDEN],
            them: vec![0i16; HIDDEN],
            bits_us: vec![0u64; PALAVRAS],
            bits_them: vec![0u64; PALAVRAS],
            novo_us: vec![0u64; PALAVRAS],
            novo_them: vec![0u64; PALAVRAS],
            valido: false,
        }
    }

    /// Bring the accumulator to `board`, applying only what changed.
    /// Returns how many weight columns were touched.
    pub fn actualiza(&mut self, net: &RedeThreats, board: &Board) -> usize {
        let mut pos = Pos::default();
        for c in 0..2 {
            for t in 0..6 {
                pos.pieces[c][t] = board.pieces[c][t];
            }
        }
        // Pela mesma razao que em `evaluate`: fixar as brancas em primeiro faz
        // do acumulador uma funcao da POSICAO e nao do turno.
        let stm = 0usize;

        for w in self.novo_us.iter_mut() { *w = 0; }
        for w in self.novo_them.iter_mut() { *w = 0; }
        {
            let nu = &mut self.novo_us;
            let nt = &mut self.novo_them;
            map_features_pairs_mode(&pos, stm, 2, &mut |a: usize, b: usize| {
                nu[a >> 6] |= 1u64 << (a & 63);
                nt[b >> 6] |= 1u64 << (b & 63);
            });
        }

        if !self.valido {
            self.us.copy_from_slice(&net.l0b);
            self.them.copy_from_slice(&net.l0b);
        }

        let mut mexidas = 0usize;
        for lado in 0..2 {
            let (novo, velho) = if lado == 0 {
                (&self.novo_us, &self.bits_us)
            } else {
                (&self.novo_them, &self.bits_them)
            };
            for w in 0..PALAVRAS {
                // When the accumulator is being built from nothing there is no
                // previous set, so every active feature is a change.
                let velho_w = if self.valido { velho[w] } else { 0 };
                let mut mudou = novo[w] ^ velho_w;
                while mudou != 0 {
                    let b = mudou.trailing_zeros() as usize;
                    mudou &= mudou - 1;
                    let f = w * 64 + b;
                    let entra = novo[w] & (1u64 << b) != 0;
                    let dst = if lado == 0 { &mut self.us } else { &mut self.them };
                    // A mesma divisao do caminho de recomputacao: as pecas em
                    // i16, as ameacas em i8 alargadas ao somar.
                    let corte = 1 + crate::features::PIECE_FEATURES;
                    if f < corte {
                        let i = f * HIDDEN;
                        let col = &net.l0w[i..i + HIDDEN];
                        if entra { soma_col(dst, col) } else { sub_col(dst, col) }
                    } else {
                        let i = (f - corte) * HIDDEN;
                        let col = &net.l0w_threats[i..i + HIDDEN];
                        if entra { soma_col8(dst, col) } else { sub_col8(dst, col) }
                    }
                    mexidas += 1;
                }
            }
        }
        std::mem::swap(&mut self.bits_us, &mut self.novo_us);
        std::mem::swap(&mut self.bits_them, &mut self.novo_them);
        self.valido = true;
        mexidas
    }

    pub fn valor(&self, net: &RedeThreats, board: &Board) -> i32 {
        let n = board.occ_all.count_ones() as usize;
        let ob = ((n.max(1) - 1) / 4).min(OUT_BUCKETS - 1);
        let base = ob * 2 * HIDDEN;
        let w = &net.l1w[base..base + 2 * HIDDEN];
        let (a, b) = if board.side == Color::White {
            (&self.us, &self.them)
        } else {
            (&self.them, &self.us)
        };
        let soma = saida_par(a, b, w);
        (soma / QA + net.l1b[ob] as i32) * SCALE / (QA * QB)
    }
}

thread_local! {
    static ACC: std::cell::RefCell<AccThreats> = std::cell::RefCell::new(AccThreats::novo());
}

/// Evaluate through the incremental accumulator.
///
/// One per thread: the accumulator carries the previous position's feature
/// set, and two threads sharing it would each invalidate the other's on every
/// node.
pub static MEXIDAS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static CHAMADAS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn evaluate_inc(net: &RedeThreats, board: &Board) -> i32 {
    ACC.with(|a| {
        let mut acc = a.borrow_mut();
        let m = acc.actualiza(net, board);
        MEXIDAS.fetch_add(m as u64, std::sync::atomic::Ordering::Relaxed);
        CHAMADAS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        acc.valor(net, board)
    })
}

/// Does the incremental accumulator agree with a full recompute?
///
/// Over a SEQUENCE, not over independent positions: an accumulator that is
/// right on the first update and drifts on the fiftieth is the failure this
/// exists to catch, and only a chain of real moves produces it.
pub fn verifica(net: &RedeThreats, fens: &[String]) -> (usize, usize, f64) {
    let mut acc = AccThreats::novo();
    let (mut n, mut erradas, mut mexidas_total) = (0usize, 0usize, 0usize);
    for fen in fens {
        let board = Board::from_fen(fen);
        mexidas_total += acc.actualiza(net, &board);
        let inc = acc.valor(net, &board);
        let cheio = evaluate(net, &board);
        n += 1;
        if inc != cheio {
            erradas += 1;
            if erradas <= 3 {
                eprintln!("DIVERGE inc={} cheio={} {}", inc, cheio, fen);
            }
        }
    }
    (n, erradas, mexidas_total as f64 / n.max(1) as f64)
}
