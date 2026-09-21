// The network: reading it, keeping it up to date, and asking it for a score.
//
// Shape: (768 x 32 -> 512) x 2 -> 1, plus a PSQT term that skips the hidden
// layer, both read through four output buckets chosen by piece count. One
// hidden layer, clipped ReLU, everything in i16.
//
// The file is a raw dump of the weights in declaration order with no header, so
// the struct below IS the format. Its size is checked on load against the byte
// count, which is the cheapest possible guard against reading a file that is
// almost but not quite this network.

use crate::bitboard::Bitboard;
use crate::types::*;

/// King buckets. Note what this number means: the bucket is the king's own
/// square after folding, not a coarse region, so EVERY king move changes it.
/// That is what makes the refresh cache below load-bearing rather than an
/// optimisation -- see the comment there.
pub const INPUT_BUCKETS: usize = 32;
pub const HIDDEN: usize = 1024;

/// A cabeca. `L1H` sai da primeira camada, `L2H` da segunda, e os dezasseis
/// viram trinta e dois na dupla activacao.
pub const L1H: usize = 16;
pub const L2H: usize = 32;

/// O acumulador e' cortado em `[0, QA]`, e os pesos da l1 estao quantizados
/// por `QB` -- ambos escolhidos pelo treinador, nao aqui.
const QA: i16 = 1024;

/// Pre-deslocamento do produto emparelhado. O `mulhrs` desloca 15 e queremos
/// `(a*b) >> 12`, que e' o que faz um produto de dois valores de dez bits caber
/// num byte.
const DESL: i32 = 3;

/// De soma inteira para unidades reais: o byte traz o produto a dividir por
/// 256, e os pesos estao quantizados por 64.
const DIV: f32 = 1.0 / 16384.0;

/// Das unidades da rede para centipeoes -- o mesmo numero por que o PSQT foi
/// quantizado, e por isso e' que ele se soma DEPOIS da conversao.
const ESCALA: f32 = 400.0;

/// O selo que a biblioteca de treino carimba no fim do ficheiro.
const SELO: usize = 32;
pub const OUT_BUCKETS: usize = 8;
pub const NUM_PECAS: usize = INPUT_BUCKETS * 2 * 6 * 64; // 24576
/// Pawn pairs: 96 slots (48 squares x 2 colours), every unordered pair.
///
/// Deliberately WITHOUT a king bucket. Bucketed, every king move would rebuild
/// the whole block; unbucketed, only pawn moves touch it -- which is what makes
/// this feature cheap where a threat set would not be.
pub const PAIR_DIM: usize = 96 * 95 / 2; // 4560
pub const PAIR_BASE: usize = NUM_PECAS;
pub const NUM_FEATURES: usize = NUM_PECAS + PAIR_DIM; // 29136
/// The size of a network without the pairs. It still loads: it is the same
/// network with those rows at zero.


/// Weights are stored multiplied by this, and the hidden layer is clipped to
/// it. Both facts come from the training quantisation and neither is free to
/// change here.
pub const SCALE: i32 = 1024;

#[repr(C)]
pub struct Network {
    pub l0w: [[i16; HIDDEN]; NUM_FEATURES],
    pub psqt: [[i16; OUT_BUCKETS]; NUM_FEATURES],
    /// A primeira camada da cabeca, em int8: e' 94% do trabalho dela.
    pub l1w: [[i8; HIDDEN]; L1H * OUT_BUCKETS],
    pub l1b: [f32; L1H * OUT_BUCKETS],
    /// Guardada `[saida][entrada]` pelo treinador e VIRADA ao carregar, porque
    /// a cabeca a le' `[entrada][saida]`. As outras duas nao precisam.
    pub l2w: [[f32; L1H * 2]; L2H * OUT_BUCKETS],
    pub l2b: [f32; L2H * OUT_BUCKETS],
    pub l3w: [[f32; L2H]; OUT_BUCKETS],
    pub l3b: [f32; OUT_BUCKETS],
}

extern "C" {
    fn cabeca_avalia(
        largura: i32,
        l1: i32,
        l2: i32,
        qa: i32,
        desl: i32,
        div: f32,
        nos: *const i16,
        eles: *const i16,
        l1w: *const i8,
        l1b: *const f32,
        l2w: *const f32,
        l2b: *const f32,
        sw: *const f32,
        sb: f32,
    ) -> f32;
}

pub const NET_BYTES: usize = std::mem::size_of::<Network>();

/// Does the loaded network have pawn pairs at all?
///
/// A network in the old format has that whole block at zero, and walking the
/// pawns to add rows of zeros costs 13.9% of the node rate for nothing. Read
/// once when the network is loaded, so the check itself costs a load and a
/// branch that predicts perfectly.
pub static TEM_PARES: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[inline]
pub fn tem_pares() -> bool {
    TEM_PARES.load(std::sync::atomic::Ordering::Relaxed)
}

/// The win-rate model the network was trained against.
///
/// These constants are not a choice and not a default: they are the output of a
/// search over hundreds of validated training runs, which is why they carry
/// sixteen digits. Changing one here without changing it in training makes the
/// program announce probabilities the network was never taught to produce.
///
/// What makes them usable directly on our score, with no conversion at all, is
/// that the quantisation stores each weight already multiplied by the training
/// scale. Measured on ten thousand positions with known results, the fit gives
/// k = 0.9939 for this network -- so the raw score IS this model's centipawn.
/// For a network trained at a different scale it would not be, and this would
/// quietly lie.
pub const WDL_OFFSET: f64 = 285.2706341467852;
pub const WDL_SCALING: f64 = 295.6539508488627;

/// Win, draw and loss in thousandths, from the side to move's point of view.
pub fn wdl(score: i32) -> (i32, i32, i32) {
    let sig = |x: f64| 1.0 / (1.0 + (-x).exp());
    let cp = score as f64;
    let w = sig((cp - WDL_OFFSET) / WDL_SCALING);
    let l = sig((-cp - WDL_OFFSET) / WDL_SCALING);
    let wi = (1000.0 * w).round() as i32;
    let li = (1000.0 * l).round() as i32;
    (wi, 1000 - wi - li, li)
}

/// The one network, set once at startup.
///
/// Global rather than carried around because every path that moves a piece
/// needs it, and threading it through the board would put a lifetime on the
/// position itself. Set once and never replaced, so no locking is involved
/// after startup.
static NET: std::sync::OnceLock<Box<Network>> = std::sync::OnceLock::new();

pub fn net() -> Option<&'static Network> {
    NET.get().map(|b| &**b)
}

/// Returns false if a network was already installed.
pub fn install(n: Box<Network>) -> bool {
    NET.set(n).is_ok()
}

/// Pede ao nucleo paginas enormes para a tabela de pesos.
///
/// Sessenta megabytes, e as linhas que um lance toca estao espalhadas por eles
/// ao acaso -- quase todo o acesso e' uma travessia da tabela de paginas numa
/// entrada nova. Medido no outro motor nosso, no mesmo tipo de problema: 26
/// milhoes de faltas de dTLB num bench, 14.2% da busca gasta a percorrer
/// tabelas de paginas.
///
/// As DUAS chamadas sao precisas. `MADV_HUGEPAGE` marca a regiao mas so' actua
/// em faltas futuras, e esta memoria ja' esta' escrita -- foi para la' que se
/// leu o ficheiro. `MADV_COLLAPSE` junta as paginas que ja' la' estao.
///
/// Falhar nao e' erro: sem nucleo que o suporte ou sem memoria contigua para
/// dar, o motor corre como corria.
#[cfg(target_os = "linux")]
fn pede_paginas_enormes(net: &Network) {
    const MADV_HUGEPAGE: i32 = 14;
    const MADV_COLLAPSE: i32 = 25;
    unsafe extern "C" {
        fn madvise(addr: *mut core::ffi::c_void, len: usize, advice: i32) -> i32;
    }
    if std::env::var("HALF2K_HUGE").as_deref() == Ok("0") {
        return;
    }
    let base = net as *const Network as usize;
    let fim = base + std::mem::size_of::<Network>();
    // Alinhado a dois megabytes nas duas pontas: o `madvise` recusa um inicio
    // desalinhado, e uma cauda a mais tocaria memoria que nao e' nossa.
    const M: usize = 2 * 1024 * 1024;
    let ini = base.next_multiple_of(M);
    let f = fim - (fim % M);
    if f <= ini {
        return;
    }
    unsafe {
        madvise(ini as *mut core::ffi::c_void, f - ini, MADV_HUGEPAGE);
        madvise(ini as *mut core::ffi::c_void, f - ini, MADV_COLLAPSE);
    }
}

#[cfg(not(target_os = "linux"))]
fn pede_paginas_enormes(_net: &Network) {}

pub fn load(path: &str) -> std::io::Result<Box<Network>> {
    // Com a ponte ligada a avaliacao vem de la', e manter o nosso acumulador
    // a par custava 54% do motor para um valor que ninguem le'.
    if crate::ponte::ligado() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "ponte activa: a rede propria nao e' carregada",
        ));
    }
    let bytes = std::fs::read(path)?;
    // A network without the pairs is the same network with those rows at zero
    // -- which is literally how the warm start for the new one was built, the
    // old weights copied across and the rest left at zero. So both sizes load,
    // and the engine keeps playing while the new network is still training.
    // O treinador carimba `bullet` repetido no fim; o resto tem de bater ao
    // byte, porque isto e' uma copia crua para uma struct `repr(C)`.
    if bytes.len() != NET_BYTES && bytes.len() != NET_BYTES + SELO {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "network is {} bytes, expected {} (ou {} com o selo)",
                bytes.len(),
                NET_BYTES,
                NET_BYTES + SELO
            ),
        ));
    }
    // SAFETY: `Network` is `repr(C)` and made entirely of `i16`, so every bit
    // pattern is a valid value and there is no padding to leave uninitialised.
    // The length was just checked. Little-endian is assumed, as the file was
    // written on the same class of machine that reads it.
    unsafe {
        let layout = std::alloc::Layout::new::<Network>();
        let raw = std::alloc::alloc_zeroed(layout) as *mut Network;
        if raw.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), raw as *mut u8, NET_BYTES);
        let net = Box::from_raw(raw);
        // A file of the right size can still have the pair block empty -- a
        // network trained before the feature existed and padded, say -- so it
        // is decided by looking rather than by the length.
        pede_paginas_enormes(&*net);
        let vazio = net.l0w[PAIR_BASE..].iter().all(|r| r.iter().all(|&w| w == 0));
        TEM_PARES.store(!vazio, std::sync::atomic::Ordering::Relaxed);
        // A l2w vem `[saida][entrada]` e a cabeca le' `[entrada][saida]`.
        // Virada aqui uma vez, e nao em cada avaliacao.
        let mut net = net;
        for b in 0..OUT_BUCKETS {
            for i in 0..L2H {
                for j in (i + 1)..(L1H * 2) {
                    if j < L2H {
                        let (a, c) = (net.l2w[b * L2H + i][j], net.l2w[b * L2H + j][i]);
                        net.l2w[b * L2H + i][j] = c;
                        net.l2w[b * L2H + j][i] = a;
                    }
                }
            }
        }
        Ok(net)
    }
}

#[inline(always)]
fn flip_rank(s: Square) -> Square {
    s ^ 56
}

#[inline(always)]
fn flip_file(s: Square) -> Square {
    s ^ 7
}

/// Which king bucket, and whether this perspective reads the board mirrored.
///
/// Both are functions of the king square and both have to travel together: a
/// king on d1 and a king on e1 fold to the same bucket while needing opposite
/// mirrors, so anything keyed on the bucket alone will hand one perspective the
/// other's values and every square comes back flipped.
#[inline(always)]
pub fn bucket_and_mirror(perspective: Color, king_sq: Square) -> (usize, bool) {
    let ks = if perspective == Color::Black {
        flip_rank(king_sq)
    } else {
        king_sq
    };
    let mirror = file_of(ks) >= 4;
    let ks = if mirror { flip_file(ks) } else { ks };
    (4 * rank_of(ks) as usize + file_of(ks) as usize, mirror)
}

/// The input number for one piece, seen from one perspective.
///
/// Black's perspective sees the board upside down and with the colours
/// exchanged, so that both sides present the network with the same problem.
/// Taking the bucket and mirror as arguments rather than recomputing them keeps
/// this honest: the caller has already decided which bucket it is working in,
/// and a feature computed under a different one would be silently wrong.
#[inline(always)]
pub fn feature(
    perspective: Color,
    bucket: usize,
    mirror: bool,
    piece_color: Color,
    pt: PieceType,
    s: Square,
) -> usize {
    let (pc, s) = if perspective == Color::Black {
        (piece_color.opp(), flip_rank(s))
    } else {
        (piece_color, s)
    };
    let s = if mirror { flip_file(s) } else { s };
    s as usize + pt.idx() * 64 + pc.idx() * 384 + bucket * 768
}

/// The squares that pair with each square: its own file and the two beside
/// it, ranks 2 to 7, itself excluded. The same mask the trainer uses.
const fn pp_mask_calc(sq: usize) -> u64 {
    const FILE_A: u64 = 0x0101_0101_0101_0101;
    let file = (sq & 7) as u32;
    let mut mask = FILE_A << file;
    if file > 0 {
        mask |= FILE_A << (file - 1);
    }
    if file < 7 {
        mask |= FILE_A << (file + 1);
    }
    mask & !(0xFFu64 | (0xFFu64 << 56)) & !(1u64 << sq)
}

pub static PP_MASK: [u64; 64] = {
    let mut t = [0u64; 64];
    let mut i = 0;
    while i < 64 {
        t[i] = pp_mask_calc(i);
        i += 1;
    }
    t
};

/// A pawn's slot from one perspective: 0..47 if it is ours, 48..95 if it is
/// theirs. The `- 8` drops the first rank, where pawns cannot be.
#[inline]
fn peao_id(casa: usize, nosso: bool, espelho: usize) -> i32 {
    (if nosso { 0 } else { 48 }) + (casa ^ espelho) as i32 - 8
}

/// The pair's index, unordered: the same pair gives the same number either
/// way round.
#[inline]
fn par_indice(a: i32, b: i32) -> usize {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    (hi * (hi - 1) / 2 + lo) as usize
}

/// Every pawn pair on the board, added into one perspective.
///
/// Unlike the trainer, where the two perspectives had to be emitted paired one
/// for one, each side sums into its own accumulator here and the order does not
/// matter.
fn soma_pares(
    net: &Network,
    peoes: &[u64; 2],
    perspectiva: Color,
    valores: &mut [i16; HIDDEN],
    psqt: &mut [i32; OUT_BUCKETS],
) {
    if !tem_pares() {
        return;
    }
    let espelho = if perspectiva == Color::Black { 56 } else { 0 };
    let nossos = peoes[perspectiva.idx()];
    let todos = peoes[0] | peoes[1];
    let mut bb = todos;
    while bb != 0 {
        let sq = bb.trailing_zeros() as usize;
        bb &= bb - 1;
        // Only the squares above, so each pair comes out once.
        let mut m = PP_MASK[sq] & todos & !((1u64 << sq) - 1);
        if m == 0 {
            continue;
        }
        let id_a = peao_id(sq, nossos >> sq & 1 != 0, espelho);
        while m != 0 {
            let sq2 = m.trailing_zeros() as usize;
            m &= m - 1;
            let id_b = peao_id(sq2, nossos >> sq2 & 1 != 0, espelho);
            let f = PAIR_BASE + par_indice(id_a, id_b);
            apply(valores, &net.l0w[f], true);
            for b in 0..OUT_BUCKETS {
                psqt[b] += net.psqt[f][b] as i32;
            }
        }
    }
}

/// One side's half of the hidden layer, plus its PSQT running total.
#[derive(Clone)]
pub struct Half {
    pub values: [i16; HIDDEN],
    pub psqt: [i32; OUT_BUCKETS],
    pub bucket: usize,
    pub mirror: bool,
}

impl Half {
    fn empty() -> Self {
        Half {
            values: [0; HIDDEN],
            psqt: [0; OUT_BUCKETS],
            bucket: 0,
            mirror: false,
        }
    }
}

#[derive(Clone)]
pub struct Accumulator {
    pub half: [Half; 2], // indexed by perspective
}

/// Build one perspective from nothing.
pub fn rebuild_half(
    net: &Network,
    pieces: &[[Bitboard; 6]; 2],
    king_sq: Square,
    perspective: Color,
) -> Half {
    let (bucket, mirror) = bucket_and_mirror(perspective, king_sq);
    let mut h = Half::empty();
    h.bucket = bucket;
    h.mirror = mirror;
    for c in [Color::White, Color::Black] {
        for pt in ALL_PIECES {
            let mut bb = pieces[c.idx()][pt.idx()];
            while bb != 0 {
                let s = bb.trailing_zeros() as Square;
                bb &= bb - 1;
                let f = feature(perspective, bucket, mirror, c, pt, s);
                apply(&mut h.values, &net.l0w[f], true);
                for b in 0..OUT_BUCKETS {
                    h.psqt[b] += net.psqt[f][b] as i32;
                }
            }
        }
    }
    // The pairs, summed like any other feature. A full recomputation is
    // right here: rebuilding a perspective from nothing is what this is for.
    let peoes = [
        pieces[Color::White.idx()][PieceType::Pawn.idx()],
        pieces[Color::Black.idx()][PieceType::Pawn.idx()],
    ];
    soma_pares(net, &peoes, perspective, &mut h.values, &mut h.psqt);
    h
}

impl Accumulator {
    pub fn empty() -> Self {
        Accumulator {
            half: [Half::empty(), Half::empty()],
        }
    }

    /// Rebuild any perspective whose king has moved into a different bucket, or
    /// across the middle where the mirror flips.
    ///
    /// Called after the move has been applied, so the values it finds were
    /// updated under the OLD bucket and cannot be patched -- under a new bucket
    /// the same piece on the same square is a different input number, so every
    /// one of them is wrong at once. Rebuilding is the only correct answer;
    /// making it cheap is a separate problem.
    ///
    /// Returns how many perspectives were rebuilt, so the cost can be measured
    /// rather than assumed.
    pub fn refresh(
        &mut self,
        net: &Network,
        pieces: &[[Bitboard; 6]; 2],
        kings: [Square; 2],
    ) -> u32 {
        let mut done = 0;
        let peoes = [
            pieces[Color::White.idx()][PieceType::Pawn.idx()],
            pieces[Color::Black.idx()][PieceType::Pawn.idx()],
        ];
        for p in [Color::White, Color::Black] {
            let (bucket, mirror) = bucket_and_mirror(p, kings[p.idx()]);
            let h = &mut self.half[p.idx()];
            if h.bucket == bucket && h.mirror == mirror {
                continue;
            }
            with_cache(|c| c.refresh(net, pieces, p, bucket, mirror, h));
            // The cache holds the PIECE contribution only -- its loop applies
            // piece features and nothing else -- so the pairs have to go back
            // on here. They do not belong in the cache: they carry no king
            // bucket, so one entry per bucket would store the same block over
            // and over and a bucket change would still have to redo it.
            //
            // Left out, the pairs vanished on every king move, which is a
            // quarter of them, and the incremental updates then added onto a
            // base that no longer had them. Measured before this line existed:
            // 28.3% over 76 games, -161.6 Elo, against the same network
            // without pairs.
            soma_pares(net, &peoes, p, &mut h.values, &mut h.psqt);
            done += 1;
        }
        done
    }

    /// Build both perspectives from nothing. Correct always, and slow enough
    /// that it is only used to start a position and as the yardstick the
    /// incremental path is checked against.
    pub fn fresh(net: &Network, pieces: &[[Bitboard; 6]; 2], kings: [Square; 2]) -> Self {
        Accumulator {
            half: [
                rebuild_half(net, pieces, kings[0], Color::White),
                rebuild_half(net, pieces, kings[1], Color::Black),
            ],
        }
    }

    /// The pairs that involve ONE square, put on or taken off.
    ///
    /// A pawn appearing on or leaving a square changes exactly the pairs that
    /// involve it -- about five of them -- and leaves every other pair alone.
    /// The first version rebuilt the whole block for both perspectives and cost
    /// 45% of the node rate on average, 69% in a middlegame with all sixteen
    /// pawns still on.
    ///
    /// The board is already updated by the time this is called, which makes
    /// both directions the same loop: after a removal the pawns that remain are
    /// exactly the ones that were paired with the square, and after an addition
    /// they are exactly the ones now paired with it.
    pub fn pares_de_uma_casa(
        &mut self,
        net: &Network,
        peoes: &[u64; 2],
        cor: Color,
        sq: Square,
        add: bool,
    ) {
        if !tem_pares() {
            return;
        }
        let todos = peoes[0] | peoes[1];
        let vizinhos = PP_MASK[sq as usize] & todos;
        if vizinhos == 0 {
            return;
        }
        for p in [Color::White, Color::Black] {
            let h = &mut self.half[p.idx()];
            let espelho = if p == Color::Black { 56 } else { 0 };
            let nossos = peoes[p.idx()];
            // The colour of the pawn that moved comes in as a PARAMETER, not
            // from the bitboard. On a removal it is no longer there, so the
            // bitboard would say it belongs to nobody and the pair would come
            // out with the wrong index -- nothing to signal it, just weights
            // read from the wrong row.
            let id_a = peao_id(sq as usize, cor == p, espelho);
            // As linhas primeiro, todas; a travessia do acumulador depois, uma
            // so'. Ao contrario, sao tantas travessias quantos os vizinhos.
            let mut linhas: [&[i16; HIDDEN]; 24] = [&net.l0w[0]; 24];
            let mut n = 0usize;
            let sinal = if add { 1 } else { -1 };
            let mut m = vizinhos;
            while m != 0 && n < 24 {
                let sq2 = m.trailing_zeros() as usize;
                m &= m - 1;
                let id_b = peao_id(sq2, nossos >> sq2 & 1 != 0, espelho);
                let f = PAIR_BASE + par_indice(id_a, id_b);
                simd::pede(&net.l0w[f]);
                linhas[n] = &net.l0w[f];
                n += 1;
                for b in 0..OUT_BUCKETS {
                    h.psqt[b] += sinal * net.psqt[f][b] as i32;
                }
            }
            aplica_varias(&mut h.values, &linhas[..n], add);
        }
    }

    /// One piece appearing on or leaving a square, for both perspectives.
    #[inline]
    /// Varias mudancas de peca, numa so' passagem pelo acumulador.
    ///
    /// O ganho nao e' aritmetico, e' de memoria: cada valor e' lido e escrito
    /// uma vez em vez de uma por peca. Numa captura sao seis passagens de dois
    /// kilobytes que passam a duas.
    pub fn update_lote(&mut self, net: &Network, lote: &[(PieceType, Color, Square, bool)]) {
        for p in [Color::White, Color::Black] {
            let h = &mut self.half[p.idx()];
            let mut mais = [0usize; 4];
            let mut menos = [0usize; 4];
            let (mut na, mut ns) = (0usize, 0usize);
            for &(pt, c, sq, add) in lote {
                let f = feature(p, h.bucket, h.mirror, c, pt, sq);
                if add {
                    mais[na] = f;
                    na += 1;
                } else {
                    menos[ns] = f;
                    ns += 1;
                }
                let sinal = if add { 1 } else { -1 };
                for b in 0..OUT_BUCKETS {
                    h.psqt[b] += sinal * net.psqt[f][b] as i32;
                }
            }
            let w = &net.l0w;
            for i in 0..na {
                simd::pede(&w[mais[i]]);
            }
            for i in 0..ns {
                simd::pede(&w[menos[i]]);
            }
            match (na, ns) {
                (1, 1) => aplica_lote::<1, 1>(&mut h.values, [&w[mais[0]]], [&w[menos[0]]]),
                (1, 2) => aplica_lote::<1, 2>(
                    &mut h.values,
                    [&w[mais[0]]],
                    [&w[menos[0]], &w[menos[1]]],
                ),
                (2, 1) => aplica_lote::<2, 1>(
                    &mut h.values,
                    [&w[mais[0]], &w[mais[1]]],
                    [&w[menos[0]]],
                ),
                (2, 2) => aplica_lote::<2, 2>(
                    &mut h.values,
                    [&w[mais[0]], &w[mais[1]]],
                    [&w[menos[0]], &w[menos[1]]],
                ),
                _ => {
                    // Qualquer outra combinacao pelo caminho antigo: sao raras
                    // e nao vale a pena uma variante para cada uma.
                    for i in 0..na {
                        apply(&mut h.values, &w[mais[i]], true);
                    }
                    for i in 0..ns {
                        apply(&mut h.values, &w[menos[i]], false);
                    }
                }
            }
        }
    }

    pub fn update(&mut self, net: &Network, c: Color, pt: PieceType, s: Square, add: bool) {
        for p in [Color::White, Color::Black] {
            let h = &mut self.half[p.idx()];
            let f = feature(p, h.bucket, h.mirror, c, pt, s);
            apply(&mut h.values, &net.l0w[f], add);
            let sign = if add { 1 } else { -1 };
            for b in 0..OUT_BUCKETS {
                h.psqt[b] += sign * net.psqt[f][b] as i32;
            }
        }
    }

    /// The score, from the side to move's point of view.
    pub fn eval(&self, net: &Network, stm: Color, piece_count: u32) -> i32 {
        let bucket = ((piece_count.saturating_sub(2) / 4) as usize).min(OUT_BUCKETS - 1);
        let us = &self.half[stm.idx()];
        let them = &self.half[stm.opp().idx()];

        // So' os pesos do balde escolhido entram numa avaliacao, que e' porque
        // ter oito baldes custa por no' o mesmo que ter quatro.
        let saida = unsafe {
            cabeca_avalia(
                HIDDEN as i32,
                L1H as i32,
                L2H as i32,
                QA as i32,
                DESL,
                DIV,
                us.values.as_ptr(),
                them.values.as_ptr(),
                net.l1w[bucket * L1H].as_ptr(),
                net.l1b[bucket * L1H..].as_ptr(),
                net.l2w[bucket * L2H].as_ptr(),
                net.l2b[bucket * L2H..].as_ptr(),
                net.l3w[bucket].as_ptr(),
                net.l3b[bucket],
            )
        };

        // O PSQT esta' quantizado pela mesma escala por que a saida se
        // converte, logo soma-se depois da conversao e nao antes.
        (saida * ESCALA) as i32 + us.psqt[bucket] - them.psqt[bucket]
    }
}

/// Add or subtract one column of weights, through the vector path if there is
/// one.
#[inline]
/// Somas e subtraccoes todas numa passagem.
///
/// `A` e `S` sao const generics de proposito: com as contagens conhecidas em
/// compilacao os ciclos interiores desenrolam-se e o valor fica em registo
/// enquanto leva com tudo. Com fatias de tamanho so' conhecido em execucao o
/// compilador tem de manter os ciclos, e volta a haver uma passagem por linha.
/// O mesmo, para um numero de linhas que so' se sabe em execucao.
#[inline]
fn aplica_varias(dst: &mut [i16; HIDDEN], linhas: &[&[i16; HIDDEN]], add: bool) {
    if linhas.is_empty() {
        return;
    }
    #[cfg(target_arch = "x86_64")]
    if has_avx2() {
        // SAFETY: a feature acabou de ser verificada e as fatias tem HIDDEN de
        // comprimento pelo tipo.
        unsafe {
            if add {
                simd::varias::<true>(dst, linhas);
            } else {
                simd::varias::<false>(dst, linhas);
            }
            return;
        }
    }
    for i in 0..HIDDEN {
        let mut v = dst[i];
        for r in linhas {
            v = if add { v.wrapping_add(r[i]) } else { v.wrapping_sub(r[i]) };
        }
        dst[i] = v;
    }
}

#[inline]
fn aplica_lote<const A: usize, const S: usize>(
    dst: &mut [i16; HIDDEN],
    mais: [&[i16; HIDDEN]; A],
    menos: [&[i16; HIDDEN]; S],
) {
    #[cfg(target_arch = "x86_64")]
    if has_avx2() {
        // SAFETY: a feature acabou de ser verificada, e todas as fatias tem
        // HIDDEN de comprimento pelo tipo.
        unsafe {
            simd::lote(dst, mais, menos);
            return;
        }
    }
    for i in 0..HIDDEN {
        let mut v = dst[i];
        for r in mais.iter() {
            v = v.wrapping_add(r[i]);
        }
        for r in menos.iter() {
            v = v.wrapping_sub(r[i]);
        }
        dst[i] = v;
    }
}

fn apply(dst: &mut [i16; HIDDEN], col: &[i16; HIDDEN], add: bool) {
    #[cfg(target_arch = "x86_64")]
    if has_avx2() {
        // SAFETY: the feature was just checked, and both slices are HIDDEN long
        // by their types.
        unsafe {
            if add {
                simd::add(dst, col);
            } else {
                simd::sub(dst, col);
            }
            return;
        }
    }
    if add {
        for i in 0..HIDDEN {
            dst[i] += col[i];
        }
    } else {
        for i in 0..HIDDEN {
            dst[i] -= col[i];
        }
    }
}

/// `sum of clamp(acc[i], 0, SCALE) * w[i]`.
#[inline]
fn output(acc: &[i16; HIDDEN], w: &[i16]) -> i32 {
    #[cfg(target_arch = "x86_64")]
    if has_avx2() {
        // SAFETY: feature checked; `acc` is HIDDEN long and so is `w`.
        unsafe {
            return simd::output(acc, w);
        }
    }
    let mut total = 0i32;
    for i in 0..HIDDEN {
        total += (acc[i] as i32).clamp(0, SCALE) * w[i] as i32;
    }
    total
}

#[cfg(target_arch = "x86_64")]
mod simd {
    #[allow(unused_imports)]
    use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
    use super::{HIDDEN, SCALE};
    use std::arch::x86_64::*;

    /// # Safety
    /// AVX2 must be available. Both slices are HIDDEN long.
    #[target_feature(enable = "avx2")]
    pub unsafe fn add(dst: &mut [i16; HIDDEN], src: &[i16; HIDDEN]) {
        let mut i = 0;
        while i + 16 <= HIDDEN {
            let a = _mm256_loadu_si256(dst.as_ptr().add(i) as *const __m256i);
            let b = _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i);
            _mm256_storeu_si256(
                dst.as_mut_ptr().add(i) as *mut __m256i,
                _mm256_add_epi16(a, b),
            );
            i += 16;
        }
    }

    /// # Safety
    /// As `add`.
    #[target_feature(enable = "avx2")]
    /// Todas as linhas numa passagem, dezasseis valores de cada vez.
    ///
    /// As contagens sao const generics para os ciclos interiores desaparecerem
    /// em compilacao: o que fica e' um ciclo com quatro ou cinco instrucoes
    /// por bloco de dezasseis, e o acumulador atravessado uma so' vez.
    #[target_feature(enable = "avx2")]
    pub unsafe fn lote<const A: usize, const S: usize>(
        dst: &mut [i16; HIDDEN],
        mais: [&[i16; HIDDEN]; A],
        menos: [&[i16; HIDDEN]; S],
    ) {
        let mut i = 0;
        while i < HIDDEN {
            let mut v = _mm256_loadu_si256(dst.as_ptr().add(i) as *const __m256i);
            let mut k = 0;
            while k < A {
                v = _mm256_add_epi16(
                    v,
                    _mm256_loadu_si256(mais[k].as_ptr().add(i) as *const __m256i),
                );
                k += 1;
            }
            let mut k = 0;
            while k < S {
                v = _mm256_sub_epi16(
                    v,
                    _mm256_loadu_si256(menos[k].as_ptr().add(i) as *const __m256i),
                );
                k += 1;
            }
            _mm256_storeu_si256(dst.as_mut_ptr().add(i) as *mut __m256i, v);
            i += 16;
        }
    }

    /// Varias linhas de uma vez, com a contagem so' conhecida em execucao.
    ///
    /// O ciclo interior sobre as linhas fica, mas o exterior -- o que atravessa
    /// o acumulador -- corre uma vez. E' o segundo que custa: sao dois
    /// kilobytes lidos e escritos por travessia.
    #[target_feature(enable = "avx2")]
    pub unsafe fn varias<const ADD: bool>(dst: &mut [i16; HIDDEN], linhas: &[&[i16; HIDDEN]]) {
        let mut i = 0;
        while i < HIDDEN {
            let mut v = _mm256_loadu_si256(dst.as_ptr().add(i) as *const __m256i);
            for r in linhas {
                let b = _mm256_loadu_si256(r.as_ptr().add(i) as *const __m256i);
                v = if ADD { _mm256_add_epi16(v, b) } else { _mm256_sub_epi16(v, b) };
            }
            _mm256_storeu_si256(dst.as_mut_ptr().add(i) as *mut __m256i, v);
            i += 16;
        }
    }

    /// Pede a linha a` memoria sem esperar por ela.
    ///
    /// Dois kilobytes num sitio ao acaso de uma tabela de sessenta megabytes.
    /// Pedida assim que o indice se sabe, chega enquanto os indices seguintes
    /// estao a ser calculados.
    #[inline]
    pub fn pede(linha: &[i16; HIDDEN]) {
        // SAFETY: `_mm_prefetch` nao le' nem escreve -- e' uma sugestao, e um
        // endereco invalido e' ignorado.
        unsafe {
            _mm_prefetch(linha.as_ptr() as *const i8, _MM_HINT_T0);
            _mm_prefetch((linha.as_ptr() as *const i8).add(64), _MM_HINT_T0);
        }
    }

    pub unsafe fn sub(dst: &mut [i16; HIDDEN], src: &[i16; HIDDEN]) {
        let mut i = 0;
        while i + 16 <= HIDDEN {
            let a = _mm256_loadu_si256(dst.as_ptr().add(i) as *const __m256i);
            let b = _mm256_loadu_si256(src.as_ptr().add(i) as *const __m256i);
            _mm256_storeu_si256(
                dst.as_mut_ptr().add(i) as *mut __m256i,
                _mm256_sub_epi16(a, b),
            );
            i += 16;
        }
    }

    /// Clipped ReLU and the dot product in one pass.
    ///
    /// `_mm256_madd_epi16` multiplies sixteen i16 pairs and adds them in pairs
    /// into eight i32 lanes, which is exactly a dot product. Keeping eight
    /// separate lanes also keeps overflow far away: each carries an eighth of
    /// the total where a scalar loop puts everything in one i32.
    ///
    /// # Safety
    /// AVX2 must be available. `acc` is HIDDEN long and `w` at least HIDDEN.
    #[target_feature(enable = "avx2")]
    pub unsafe fn output(acc: &[i16; HIDDEN], w: &[i16]) -> i32 {
        let zero = _mm256_setzero_si256();
        let top = _mm256_set1_epi16(SCALE as i16);
        let mut sum = _mm256_setzero_si256();
        let mut i = 0;
        while i + 16 <= HIDDEN {
            let x = _mm256_loadu_si256(acc.as_ptr().add(i) as *const __m256i);
            let wv = _mm256_loadu_si256(w.as_ptr().add(i) as *const __m256i);
            let c = _mm256_min_epi16(_mm256_max_epi16(x, zero), top);
            sum = _mm256_add_epi32(sum, _mm256_madd_epi16(c, wv));
            i += 16;
        }
        let lo = _mm256_castsi256_si128(sum);
        let hi = _mm256_extracti128_si256(sum, 1);
        let mut s = _mm_add_epi32(lo, hi);
        s = _mm_add_epi32(s, _mm_shuffle_epi32(s, 0b01_00_11_10));
        s = _mm_add_epi32(s, _mm_shuffle_epi32(s, 0b10_11_00_01));
        _mm_cvtsi128_si32(s)
    }
}

// ---------------------------------------------------------------------------
// Refresh cache
//
// The bucket here is the folded king square itself, so every king move
// invalidates one whole perspective, and king moves are around a quarter of all
// moves. Rebuilding from nothing means adding all thirty-two pieces back, and
// each of those reads a kilobyte from a twenty-five megabyte table, so the cost
// is memory rather than arithmetic and no amount of vectorising touches it.
//
// The cache turns the rebuild into a difference. Keep one accumulator per
// (perspective, bucket, mirror) alongside the piece placement that produced it;
// on a refresh, start from that and apply only the pieces that have moved
// since. A piece that stayed put contributes the same feature and needs no work
// at all, which is the whole point.
//
// The invariant that makes it safe: an entry always holds an accumulator that
// exactly matches its stored placement. Update both together or neither, and a
// stale entry becomes impossible rather than merely unlikely -- a quietly wrong
// accumulator shows up as an evaluation that is subtly off in rare positions,
// which is the hardest kind of bug to trace back to here.
//
// Keyed by the mirror as well as the bucket, and it has to be: a king on d1 and
// a king on e1 fold to the same bucket while needing opposite mirrors. Keyed by
// bucket alone, an entry built for one is handed to the other and every square
// comes back flipped.
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct CacheEntry {
    values: [i16; HIDDEN],
    psqt: [i32; OUT_BUCKETS],
    /// The placement `values` was built from.
    pieces: [[Bitboard; 6]; 2],
    used: bool,
}

pub struct RefreshCache {
    entries: Vec<CacheEntry>,
}

impl RefreshCache {
    pub fn new() -> Self {
        RefreshCache {
            entries: vec![
                CacheEntry {
                    values: [0; HIDDEN],
                    psqt: [0; OUT_BUCKETS],
                    pieces: [[0; 6]; 2],
                    used: false,
                };
                2 * 2 * INPUT_BUCKETS
            ],
        }
    }

    #[inline]
    fn index(perspective: Color, bucket: usize, mirror: bool) -> usize {
        (perspective.idx() * 2 + mirror as usize) * INPUT_BUCKETS + bucket
    }

    /// Bring one perspective up to date, starting from whatever this bucket was
    /// last seen holding. Returns how many piece updates it took, so what the
    /// cache saves can be measured rather than assumed.
    pub fn refresh(
        &mut self,
        net: &Network,
        pieces: &[[Bitboard; 6]; 2],
        perspective: Color,
        bucket: usize,
        mirror: bool,
        dst: &mut Half,
    ) -> usize {
        let e = &mut self.entries[Self::index(perspective, bucket, mirror)];
        if !e.used {
            // Nothing here yet. An empty accumulator for this network is all
            // zeros -- there is no hidden layer bias to seed it from -- so the
            // entry starts from an empty board and the first refresh pays the
            // full price. Every later one starts from this.
            e.values = [0; HIDDEN];
            e.psqt = [0; OUT_BUCKETS];
            e.pieces = [[0; 6]; 2];
            e.used = true;
        }

        let mut touched = 0usize;
        for c in [Color::White, Color::Black] {
            for pt in ALL_PIECES {
                let now = pieces[c.idx()][pt.idx()];
                let before = e.pieces[c.idx()][pt.idx()];
                let mut gone = before & !now;
                while gone != 0 {
                    let sq = gone.trailing_zeros() as Square;
                    gone &= gone - 1;
                    let f = feature(perspective, bucket, mirror, c, pt, sq);
                    apply(&mut e.values, &net.l0w[f], false);
                    for b in 0..OUT_BUCKETS {
                        e.psqt[b] -= net.psqt[f][b] as i32;
                    }
                    touched += 1;
                }
                let mut arrived = now & !before;
                while arrived != 0 {
                    let sq = arrived.trailing_zeros() as Square;
                    arrived &= arrived - 1;
                    let f = feature(perspective, bucket, mirror, c, pt, sq);
                    apply(&mut e.values, &net.l0w[f], true);
                    for b in 0..OUT_BUCKETS {
                        e.psqt[b] += net.psqt[f][b] as i32;
                    }
                    touched += 1;
                }
                // Placement and values move together, always.
                e.pieces[c.idx()][pt.idx()] = now;
            }
        }

        dst.values.copy_from_slice(&e.values);
        dst.psqt = e.psqt;
        dst.bucket = bucket;
        dst.mirror = mirror;
        touched
    }
}

thread_local! {
    /// Per thread rather than shared: two threads on one cache would each
    /// invalidate the other on every king move, which costs more than having no
    /// cache at all.
    static CACHE: std::cell::RefCell<RefreshCache> =
        std::cell::RefCell::new(RefreshCache::new());
}

pub fn with_cache<R>(f: impl FnOnce(&mut RefreshCache) -> R) -> R {
    CACHE.with(|c| f(&mut c.borrow_mut()))
}

/// Whether the vector path may be used. Decided once.
#[cfg(target_arch = "x86_64")]
pub fn has_avx2() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        // An escape hatch so the two paths can be compared on one machine
        // without rebuilding -- which is how you find out whether the vector
        // path is right, not just fast.
        if std::env::var_os("HALF2K_NO_SIMD").is_some() {
            return false;
        }
        std::is_x86_feature_detected!("avx2")
    })
}

#[cfg(not(target_arch = "x86_64"))]
pub fn has_avx2() -> bool {
    false
}
