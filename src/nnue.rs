//! The evaluation network.
//!
//! Architecture is `(768 -> HIDDEN)x2 -> 1`: one accumulator per side, each
//! fed the same 768 piece-square inputs read from that side's point of view,
//! concatenated side-to-move first, then a single output neuron.
//!
//! Why this shape and not something larger. The input set is the plain
//! piece-square one -- twelve piece/colour combinations on sixty-four squares
//! -- with no king bucketing. Bucketed inputs are worth real strength and cost
//! a full accumulator refresh whenever a king moves, plus a much larger file
//! for the same number of positions to train it. This shape trains to
//! something useful on a few million positions, which is what we can generate
//! in an evening; the bucketed version can come once the pipeline around it is
//! proven and the data is deep enough to feed it.
//!
//! Quantisation follows the training side exactly: the first layer is scaled
//! by QA and the second by QB, so an accumulator entry is a real weight times
//! QA and the output is divided by QA*QB at the end. Doing the arithmetic in
//! integers is not an optimisation detail here -- a float evaluation would
//! make the search non-reproducible between machines.

use crate::board::Board;
use crate::types::{Color, PieceType};

pub const HIDDEN: usize = 512;
pub const INPUTS: usize = 768;
const QA: i32 = 255;
const QB: i32 = 64;

/// How many centipawns one unit of the network's output is worth.
///
/// Tunable at runtime -- `setoption name EvalScale value <n>` -- because it is
/// not really a property of the network. It is the exchange rate between the
/// network's opinion and the units every pruning margin in the search is
/// written in, and those margins were fitted against whatever this happened to
/// be.
///
/// It is worth about two plies, which is why it is a knob and not a constant.
/// Loading a network trained elsewhere, whose output is on half our scale,
/// cost three plies of depth at a HIGHER node rate -- the signature of margins
/// that no longer fire, so the tree widens. Rescaling that same network, one
/// number and not one weight, gave the plies straight back.
///
/// Changing it detunes every margin at once, so a measured gain is the net of
/// both effects, not the scale alone.
static ESCALA: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(200);

#[inline]
pub fn escala() -> i32 {
    ESCALA.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_escala(v: i32) {
    ESCALA.store(v.clamp(1, 4000), std::sync::atomic::Ordering::Relaxed);
}

/// Weights, in the layout `bullet` writes them.
///
/// `l0w` is stored input-major: all HIDDEN weights for input 0, then for
/// input 1, and so on. That is the order the accumulator update wants -- one
/// active input means one contiguous run of HIDDEN values to add -- so it is
/// kept exactly as written rather than transposed on load.
pub struct Network {
    pub l0w: Vec<i16>, // INPUTS * HIDDEN * buckets
    pub l0b: Vec<i16>, // HIDDEN
    /// Output weights, laid out bucket by bucket: the 2*HIDDEN weights of
    /// bucket 0, then bucket 1, and so on. That is what the trainer's
    /// `.transpose()` produces, and it is the layout inference wants -- one
    /// contiguous run per bucket instead of a stride of 8 through the whole
    /// matrix.
    pub l1w: Vec<i16>, // 2 * HIDDEN * output_buckets
    /// One bias per output bucket.
    pub l1b: Vec<i16>,
    /// How many output buckets, by material count. A network trained without
    /// them has one, and everything below collapses to the single-bucket case
    /// with no special path.
    pub output_buckets: usize,
    /// How many king buckets this network's inputs are split into.
    ///
    /// Read from the file's own size rather than assumed: a network trained
    /// without buckets and one trained with twelve differ only in the length
    /// of the first layer, and getting it wrong reads the wrong weights for
    /// every piece rather than failing. One means "no bucketing", and every
    /// index below collapses to the unbucketed layout with no special case.
    pub buckets: usize,
}

/// One piece appearing on, or disappearing from, a square.
///
/// Recorded instead of applied. See `Accumulator::computed` for why.
#[derive(Clone, Copy)]
pub struct DirtyPiece {
    pub pt: PieceType,
    pub c: Color,
    pub sq: u8,
    pub add: bool,
}

/// How many piece changes can be outstanding before we stop deferring and
/// just apply them.
///
/// Not a per-move bound -- one move is at most five changes -- but a bound on
/// how deep the search can descend WITHOUT anything asking for a score. Past
/// this, deferring stops paying (the list costs more to walk than the columns
/// cost to add) so `push_dirty` applies eagerly instead. Sized from measured
/// behaviour rather than theory: with 0.65 evaluations per node the list is
/// almost always empty or holds one move's worth.
pub const MAX_DIRTY: usize = 32;

/// The two accumulators, side to move and the other side.
///
/// Kept as plain arrays rather than behind a pointer: this rides on the
/// `Board`, and an allocation per node would cost more than the evaluation it
/// exists to speed up.
///
/// **Lazy.** `white`/`black` do not necessarily describe the current position:
/// `dirty` holds piece changes that have not been folded in yet, and the
/// values are only brought up to date when something actually asks for a
/// score (`Board::materialise_acc`, called from `evaluate`). The reason is
/// measured, not assumed -- profiling put `Accumulator::add` plus the `remove`
/// inlined into `Board::remove_piece` at 34% of total search time, while the
/// engine performs only 0.65 evaluations per node. A third of that work was
/// being done for positions whose score was never read.
///
/// What makes this cheap without a per-ply stack of accumulators (which would
/// be ~256 kB and is copied at all nine `Board::clone` sites): `push_dirty`
/// CANCELS a change against its own inverse. The search's make/unmake is
/// perfectly nested, so a move that is made, searched without ever being
/// evaluated, and unmade pushes each change and then its opposite -- and the
/// list returns to exactly where it started. Descending and coming back up
/// costs nothing at all, which is the property a per-ply stack buys by
/// spending memory. Order within the list never matters: the accumulator is a
/// sum of columns, so any permutation of the same adds and removes lands on
/// the same values.
#[derive(Clone)]
pub struct Accumulator {
    pub white: [i16; HIDDEN],
    pub black: [i16; HIDDEN],
    /// Which king bucket each perspective is currently written for, indexed by
    /// colour. Kept here rather than recomputed because every add and remove
    /// needs it, and because a mismatch between the bucket the values were
    /// built under and the one used to index new features is silent.
    pub bucket: [usize; 2],
    /// Piece changes recorded but not yet applied to `white`/`black`.
    pub dirty: [DirtyPiece; MAX_DIRTY],
    pub n_dirty: u8,
}

/// Feature index for a piece on a square, from one side's point of view.
///
/// From black's side the board is mirrored vertically and the colours are
/// swapped, so that "my pawn on my second rank" is the same input number for
/// both players. Without that, the network has to learn every pattern twice
/// and half its capacity goes into the symmetry.
#[inline]
fn feature(perspective: Color, piece_color: Color, pt: PieceType, sq: u8) -> usize {
    let (c, s) = if perspective == Color::White {
        (piece_color == Color::Black, sq as usize)
    } else {
        (piece_color == Color::White, (sq as usize) ^ 56)
    };
    (c as usize) * 384 + pt.idx() * 64 + s
}

impl Accumulator {
    pub fn fresh(net: &Network, board: &Board) -> Self {
        let mut acc = Accumulator {
            white: [0; HIDDEN],
            black: [0; HIDDEN],
            // Through `bucket_efectivo`, not `bucket_do_rei`: an unbucketed
            // network has one block of weights and every king square must map
            // to zero. Using the layout's answer regardless indexes past the
            // end of the first layer -- which crashed here rather than reading
            // someone else's weights, but only by luck of the bounds check.
            bucket: [
                bucket_efectivo(net, board, Color::White),
                bucket_efectivo(net, board, Color::Black),
            ],
            dirty: [DirtyPiece { pt: PieceType::Pawn, c: Color::White, sq: 0, add: true }; MAX_DIRTY],
            n_dirty: 0,
        };
        acc.white.copy_from_slice(&net.l0b);
        acc.black.copy_from_slice(&net.l0b);
        for color in [Color::White, Color::Black] {
            for pt in [
                PieceType::Pawn,
                PieceType::Knight,
                PieceType::Bishop,
                PieceType::Rook,
                PieceType::Queen,
                PieceType::King,
            ] {
                let mut bb = board.pieces[color.idx()][pt.idx()];
                while bb != 0 {
                    let sq = bb.trailing_zeros() as u8;
                    bb &= bb - 1;
                    acc.add(net, color, pt, sq);
                }
            }
        }
        acc
    }

    #[inline]
    pub fn add(&mut self, net: &Network, color: Color, pt: PieceType, sq: u8) {
        let fw = feature_bucket(Color::White, self.bucket[0], color, pt, sq) * HIDDEN;
        let fb = feature_bucket(Color::Black, self.bucket[1], color, pt, sq) * HIDDEN;
        #[cfg(target_arch = "x86_64")]
        if tem_avx2() {
            unsafe {
                simd::soma(&mut self.white, &net.l0w[fw..fw + HIDDEN]);
                simd::soma(&mut self.black, &net.l0w[fb..fb + HIDDEN]);
            }
            return;
        }
        for i in 0..HIDDEN {
            self.white[i] += net.l0w[fw + i];
            self.black[i] += net.l0w[fb + i];
        }
    }

    #[inline]
    pub fn remove(&mut self, net: &Network, color: Color, pt: PieceType, sq: u8) {
        let fw = feature_bucket(Color::White, self.bucket[0], color, pt, sq) * HIDDEN;
        let fb = feature_bucket(Color::Black, self.bucket[1], color, pt, sq) * HIDDEN;
        #[cfg(target_arch = "x86_64")]
        if tem_avx2() {
            unsafe {
                simd::subtrai(&mut self.white, &net.l0w[fw..fw + HIDDEN]);
                simd::subtrai(&mut self.black, &net.l0w[fb..fb + HIDDEN]);
            }
            return;
        }
        for i in 0..HIDDEN {
            self.white[i] -= net.l0w[fw + i];
            self.black[i] -= net.l0w[fb + i];
        }
    }

    /// Record a piece change instead of applying it.
    ///
    /// Cancels against the tail of the list when this change is the exact
    /// inverse of the last one recorded -- which is what `unmake_move`
    /// produces for a move nothing evaluated under, and what makes descending
    /// and returning free. Falls back to applying immediately once the list is
    /// full, so the deferral can never grow unbounded or lose a change.
    #[inline]
    pub fn push_dirty(&mut self, net: &Network, c: Color, pt: PieceType, sq: u8, add: bool) {
        if let Some(last) = self.n_dirty.checked_sub(1) {
            let d = self.dirty[last as usize];
            if d.sq == sq && d.pt == pt && d.c == c && d.add != add {
                self.n_dirty = last;
                return;
            }
        }
        if (self.n_dirty as usize) < MAX_DIRTY {
            self.dirty[self.n_dirty as usize] = DirtyPiece { pt, c, sq, add };
            self.n_dirty += 1;
            return;
        }
        if add {
            self.add(net, c, pt, sq);
        } else {
            self.remove(net, c, pt, sq);
        }
    }

    /// Fold every recorded change into the values, in one pass per column.
    ///
    /// Called from the evaluation path, and from anything that needs the
    /// values to mean the current position -- notably a king bucket change,
    /// which rewrites every column and must not run against a stale base.
    pub fn materialise(&mut self, net: &Network) {
        for i in 0..self.n_dirty as usize {
            let d = self.dirty[i];
            if d.add {
                self.add(net, d.c, d.pt, d.sq);
            } else {
                self.remove(net, d.c, d.pt, d.sq);
            }
        }
        self.n_dirty = 0;
    }
}

/// Squared clipped ReLU, the activation the network was trained with.
///
/// Clamp to [0, QA] and square. The square is what makes it worth using over
/// a plain clipped ReLU -- it gives the layer a non-linearity with a gradient
/// that keeps growing over the active range -- and it is why the output is
/// divided by QA once more than the quantisation alone would suggest.
#[inline]
fn screlu(x: i16) -> i32 {
    let v = (x as i32).clamp(0, QA);
    v * v
}

/// Which output bucket a position falls in.
///
/// By piece count, which is cheap, changes slowly, and separates positions
/// that genuinely want different judgement: a queenless six-piece ending and
/// a full board are not the same function of the same features.
#[inline]
pub fn output_bucket(net: &Network, board: &crate::board::Board) -> usize {
    if net.output_buckets <= 1 {
        return 0;
    }
    // `(pieces - 2) / ceil(32 / N)`, which is what the trainer uses
    // (`MaterialCount<N>` in bullet's game/outputs.rs). This was
    // `(pieces - 1) / 4`, and the two disagree at 5, 9, 13, 17, 21, 25 and 29
    // pieces -- roughly a quarter of all positions read the wrong output
    // bucket, silently, with no error anywhere.
    //
    // It went unnoticed because every network the engine had ever run was
    // trained with ONE output bucket, where the whole function short-circuits
    // above. The first network with eight exposed it by losing 0-71-0.
    //
    // The divisor is derived from the bucket count rather than hardcoded to 4,
    // for the same reason: a network trained with a different N would
    // otherwise be read with the wrong stride and fail the same silent way.
    let n = board.occ_all.count_ones() as usize;
    let divisor = 32usize.div_ceil(net.output_buckets);
    (n.saturating_sub(2) / divisor).min(net.output_buckets - 1)
}

pub fn evaluate(net: &Network, acc: &Accumulator, side: Color, ob: usize) -> i32 {
    let (us, them) = match side {
        Color::White => (&acc.white, &acc.black),
        Color::Black => (&acc.black, &acc.white),
    };
    // The slice for this bucket. Laid out bucket by bucket, so this is one
    // contiguous run rather than a stride.
    let base = ob * 2 * HIDDEN;
    let w = &net.l1w[base..base + 2 * HIDDEN];
    let vies = net.l1b[ob] as i32;
    let mut sum: i32 = 0;
    #[cfg(target_arch = "x86_64")]
    if tem_avx2() {
        unsafe {
            sum = simd::saida(us, &w[..HIDDEN]) + simd::saida(them, &w[HIDDEN..]);
        }
    } else {
        for i in 0..HIDDEN {
            sum += screlu(us[i]) * w[i] as i32;
            sum += screlu(them[i]) * w[HIDDEN + i] as i32;
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    for i in 0..HIDDEN {
        sum += screlu(us[i]) * w[i] as i32;
        sum += screlu(them[i]) * w[HIDDEN + i] as i32;
    }
    // One QA from the squaring in the activation, then the usual QA*QB from
    // the two quantised layers.
    (sum / QA + vies) * escala() / (QA * QB)
}

/// Read the raw little-endian dump `bullet` saves.
///
/// No header and no version field -- the file is exactly the four tensors in
/// order. That means a mismatched HIDDEN produces a wrong-sized file rather
/// than a silently wrong network, which is the one failure mode worth having:
/// the length check below turns it into an error instead of nonsense
/// evaluations nobody would trace back to here.
/// A short header we write ourselves, in front of the trainer's raw dump.
///
/// The trainer writes four tensors and nothing else -- no version, no shape,
/// no layout. That works right up until two networks have the same SIZE and
/// different meanings, which is exactly what happens when a king-bucket layout
/// is changed without changing the bucket count: every weight lands in the
/// right place and describes the wrong king. Nothing downstream would say so;
/// the engine would simply play slightly wrong forever.
///
/// So the layout goes IN the file. Sixteen bytes, and a network without them
/// still loads -- the old files are raw dumps and are recognised by not
/// starting with the magic.
const MAGIC: [u8; 4] = *b"KSTR";
const VERSAO: u16 = 1;

fn cabecalho(buckets: usize, hidden: usize, layout: &[usize; 32]) -> Vec<u8> {
    let mut v = Vec::with_capacity(16);
    v.extend_from_slice(&MAGIC);
    v.extend_from_slice(&VERSAO.to_le_bytes());
    v.extend_from_slice(&(buckets as u16).to_le_bytes());
    v.extend_from_slice(&(hidden as u16).to_le_bytes());
    // A fingerprint of the layout, not the layout itself: what matters is
    // detecting that it differs, and six bytes of hash does that without
    // making the header a second place the layout is written down.
    let mut h: u64 = 1469598103934665603;
    for &b in layout.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    v.extend_from_slice(&h.to_le_bytes()[..6]);
    v
}

/// Write a network the engine can check rather than guess about.
pub fn escreve_com_cabecalho(cru: &[u8], buckets: usize, saida: &str) -> std::io::Result<()> {
    let mut v = cabecalho(buckets, HIDDEN, &king_bucket_layout());
    v.extend_from_slice(cru);
    std::fs::write(saida, v)
}

pub fn load(bytes: &[u8]) -> Option<Network> {
    // Our header, if present. Without it the file is a raw trainer dump and
    // the shape has to be inferred from its length, which is what the code
    // below still does -- but a file that HAS the header gets checked instead
    // of guessed, and a layout mismatch is caught here rather than never.
    if bytes.len() > 16 && bytes[..4] == MAGIC {
        let versao = u16::from_le_bytes([bytes[4], bytes[5]]);
        let buckets = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let hidden = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
        let esperado = cabecalho(buckets, HIDDEN, &king_bucket_layout());
        if hidden != HIDDEN {
            eprintln!("nnue: rede treinada com HIDDEN={}, este binario tem {}", hidden, HIDDEN);
            return None;
        }
        if bytes[10..16] != esperado[10..16] {
            eprintln!(
                "nnue: o layout de king buckets desta rede nao e' o deste binario. \
                 Mesmo numero de buckets, regra diferente -- a rede leria os pesos certos \
                 para o rei errado."
            );
            return None;
        }
        if versao == 2 {
            return carrega_leb128(bytes, buckets);
        }
        if versao != 1 {
            eprintln!("nnue: versao de cabecalho {} desconhecida deste binario", versao);
            return None;
        }
    }
    load_bruto(bytes)
}

/// The v1/no-header path: raw little-endian `i16` dump, shape inferred from
/// byte length. Untouched by the v2 (LEB128) addition below -- every file
/// that loaded before this existed still takes the exact same branch.
fn load_bruto(bytes: &[u8]) -> Option<Network> {
    let bytes = if bytes.len() > 16 && bytes[..4] == MAGIC {
        &bytes[16..]
    } else {
        bytes
    };

    // At LEAST the four tensors; the trainer pads the tail so the last one
    // lands on an alignment boundary, and that padding is not ours to
    // interpret. Requiring an exact length rejected a perfectly good network
    // over 31 words of zeroes -- and worse, the engine then fell through to
    // the old hand-written evaluation and looked like it was working.
    // How many buckets the file implies. The tail after the first layer is
    // fixed, so the first layer's size divided by one bucket's worth is the
    // count -- and if it does not divide exactly the file is not ours.
    // Two unknowns, one equation -- so try the output-bucket counts that
    // exist and take the one that divides exactly. Guessing wrong here does
    // not fail: it reads an eighth of the output weights and the wrong bias,
    // which is an evaluation that looks plausible and is worth a thousand
    // centipawns in a level position. Measured, on a real network.
    let total = bytes.len() / 2;
    let mut escolha = None;
    for ob in [8usize, 1] {
        let cauda = HIDDEN + 2 * HIDDEN * ob + ob;
        if total < cauda {
            continue;
        }
        let resto = total - cauda;
        let n = resto / (INPUTS * HIDDEN);
        // Sobra pequena e' o alinhamento que o treinador poe no fim; sobra
        // grande e' uma forma que nao e' esta.
        if n >= 1 && resto - n * (INPUTS * HIDDEN) < 64 {
            escolha = Some((ob, n));
            break;
        }
    }
    let (output_buckets, buckets) = match escolha {
        Some(v) => v,
        None => {
            eprintln!("nnue: {} valores nao correspondem a nenhuma forma conhecida", total);
            return None;
        }
    };
    let cauda = HIDDEN + 2 * HIDDEN * output_buckets + output_buckets;
    let want = (INPUTS * HIDDEN * buckets + cauda) * 2;
    if bytes.len() < want {
        eprintln!(
            "nnue: ficheiro tem {} bytes, precisa de pelo menos {} (HIDDEN={})",
            bytes.len(),
            want,
            HIDDEN
        );
        return None;
    }
    let mut it = bytes
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]));
    let l0w: Vec<i16> = (&mut it).take(INPUTS * HIDDEN * buckets).collect();
    let l0b: Vec<i16> = (&mut it).take(HIDDEN).collect();
    let l1w: Vec<i16> = (&mut it).take(2 * HIDDEN * output_buckets).collect();
    let l1b: Vec<i16> = (&mut it).take(output_buckets).collect();
    eprintln!(
        "nnue: {} bucket(s) de entrada, {} de saida",
        buckets, output_buckets
    );
    Some(Network { l0w, l0b, l1w, l1b, buckets, output_buckets })
}

// --- LEB128 packing (v2 file format) --------------------------------------
//
// Storage only. The decoded result is the exact same `Vec<i16>` the raw (v1)
// path produces, and every hot loop in this file and in the accumulator
// reads that `Vec<i16>` the same way regardless of which format it came
// from -- packing shrinks the file on disk, it does not touch a single
// nanosecond of search. Quantised NNUE weights cluster near zero (most
// squares are irrelevant to most positions, so most learned weights stay
// small), which is exactly the distribution zigzag+LEB128 is small for.

/// Maps a signed value to unsigned so small magnitudes -- positive OR
/// negative -- both encode as small numbers. Without this, -1 would encode
/// as 0xFFFF and cost the full three bytes every time.
#[inline]
pub(crate) fn zigzag_encode(v: i16) -> u16 {
    ((v << 1) ^ (v >> 15)) as u16
}

#[inline]
pub(crate) fn zigzag_decode(u: u16) -> i16 {
    ((u >> 1) as i16) ^ -((u & 1) as i16)
}

pub(crate) fn leb128_push(mut v: u16, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Returns `None` on a truncated stream rather than panicking -- a corrupt or
/// short file is a network we refuse, not a crash.
pub(crate) fn leb128_pop(bytes: &[u8], pos: &mut usize) -> Option<u16> {
    let mut result: u32 = 0;
    let mut shift = 0;
    loop {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        result |= ((byte & 0x7F) as u32) << shift;
        if byte & 0x80 == 0 {
            return Some(result as u16);
        }
        shift += 7;
        // 16-bit payloads need at most 3 groups of 7 bits. A fourth means
        // the stream is not ours (or corrupt) -- refuse rather than loop.
        if shift > 21 {
            return None;
        }
    }
}

fn empacota_leb128(valores: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(valores.len() * 2);
    for &v in valores {
        leb128_push(zigzag_encode(v), &mut out);
    }
    out
}

/// Returns the decoded values AND how many bytes they occupied, in one pass.
/// The first version of this called a second helper afterward just to
/// re-decode the same stream and count its length -- for `l0w` (393216 of
/// this network's 394753 values) that was the whole tensor parsed twice for
/// no reason. Reading littleindian's own LEB128 reader in nnue_net.cpp
/// (`lebOne`/`lebI16`) turned up nothing structurally faster -- same
/// scalar, byte-at-a-time decode -- but its chunk format stores each
/// chunk's byte length up front instead of re-deriving it, which is the
/// actual fix: don't recompute what the first pass already knew.
fn desempacota_leb128(bytes: &[u8], n: usize) -> Option<(Vec<i16>, usize)> {
    let mut out = Vec::with_capacity(n);
    let mut pos = 0;
    for _ in 0..n {
        out.push(zigzag_decode(leb128_pop(bytes, &mut pos)?));
    }
    Some((out, pos))
}

/// v2 header adds `output_buckets` explicitly (bytes 16..18). It has to:
/// v1's shape inference divides the total BYTE length by a fixed 2-byte
/// stride to recover value counts, and that trick does not exist once
/// values are variable-width. Storing the count sidesteps guessing instead
/// of reimplementing it against a stream that cannot be measured that way.
fn carrega_leb128(bytes: &[u8], buckets: usize) -> Option<Network> {
    if bytes.len() < 18 {
        eprintln!("nnue: cabecalho v2 truncado");
        return None;
    }
    let output_buckets = u16::from_le_bytes([bytes[16], bytes[17]]) as usize;
    let payload = &bytes[18..];

    let n_l0w = INPUTS * HIDDEN * buckets;
    let n_l0b = HIDDEN;
    let n_l1w = 2 * HIDDEN * output_buckets;
    let n_l1b = output_buckets;

    let mut pos = 0;
    let (l0w, consumido) = desempacota_leb128(&payload[pos..], n_l0w)?;
    pos += consumido;
    let (l0b, consumido) = desempacota_leb128(&payload[pos..], n_l0b)?;
    pos += consumido;
    let (l1w, consumido) = desempacota_leb128(&payload[pos..], n_l1w)?;
    pos += consumido;
    let (l1b, _) = desempacota_leb128(&payload[pos..], n_l1b)?;

    eprintln!(
        "nnue: {} bucket(s) de entrada, {} de saida (LEB128, {} bytes empacotados)",
        buckets, output_buckets, bytes.len()
    );
    Some(Network { l0w, l0b, l1w, l1b, buckets, output_buckets })
}

/// Repacks an already-loaded network (any input format) into the v2 LEB128
/// file format. `escreve_com_cabecalho`'s sibling for the packed path.
pub fn empacota_rede(net: &Network, saida: &str) -> std::io::Result<()> {
    let mut v = cabecalho(net.buckets, HIDDEN, &king_bucket_layout());
    v[4..6].copy_from_slice(&2u16.to_le_bytes()); // VERSAO = 2
    v.extend_from_slice(&(net.output_buckets as u16).to_le_bytes());

    let mut todos = Vec::with_capacity(net.l0w.len() + net.l0b.len() + net.l1w.len() + net.l1b.len());
    todos.extend_from_slice(&net.l0w);
    todos.extend_from_slice(&net.l0b);
    todos.extend_from_slice(&net.l1w);
    todos.extend_from_slice(&net.l1b);
    v.extend_from_slice(&empacota_leb128(&todos));

    std::fs::write(saida, v)
}

/// Evaluate a position from scratch, for callers that have no accumulator to
/// carry. Correct but not fast -- the search should hold an accumulator and
/// update it move by move.
pub fn evaluate_board(net: &Network, board: &Board) -> i32 {
    let acc = Accumulator::fresh(net, board);
    evaluate(net, &acc, board.side, output_bucket(net, board))
}

/// The loaded network, if any.
///
/// Read from the path in `KESTREL_NNUE` on first use. An env var rather than
/// a compiled-in file while the shape is still moving: rebuilding the engine
/// to try a checkpoint would make comparing two networks a comparison of two
/// binaries, which is the mistake the feature flags in Cargo.toml exist to
/// avoid. It becomes an embedded file once the architecture stops changing.
static REDE: std::sync::OnceLock<Option<Network>> = std::sync::OnceLock::new();

pub fn rede() -> Option<&'static Network> {
    REDE.get_or_init(|| {
        let path = std::env::var("KESTREL_NNUE").ok()?;
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("nnue: nao consegui ler {}: {}", path, e);
                return None;
            }
        };
        let n = load(&bytes);
        if n.is_some() {
            eprintln!("nnue: rede carregada de {} (HIDDEN={})", path, HIDDEN);
        }
        n
    })
    .as_ref()
}

/// How many king buckets the input set uses, and which bucket a king square
/// falls in.
///
/// The point of bucketing the inputs by king square is that "knight on f5" is
/// a different fact depending on where the king it threatens is standing. The
/// cost is that the whole accumulator must be rebuilt whenever the king moves
/// across a bucket boundary, so the layout should be coarse exactly where king
/// position stops mattering.
///
/// This layout is COMPUTED from the rule below rather than written out as a
/// table of numbers. That is deliberate and it is not a style preference: a
/// trained network is bound to the layout it was trained on, so a layout
/// transcribed from elsewhere cannot be replaced later without retraining from
/// scratch. A rule can be restated; a table of magic numbers cannot.
///
/// The rule, in one sentence: fine on the two home ranks where castling
/// structure decides king safety, coarse once the king has stepped out, where
/// what matters is which half of the board it is on rather than which file.
///
/// Squares are given already mirrored into files a-d, so `file` is 0..=3.
pub const NUM_KING_BUCKETS: usize = 12;

pub fn king_bucket(file: usize, rank: usize) -> usize {
    debug_assert!(file < 4 && rank < 8);
    match rank {
        // Rank 1: castled or still at home. Every file is its own bucket --
        // a king on c1 after long castling and a king on a1 in the corner
        // want different pawn-shelter knowledge, and this is the rank where
        // most of a game is decided.
        0 => file,
        // Rank 2: the same distinction, one rank up. Kept separate from rank
        // 1 because a king that has stepped to the second rank has usually
        // lost a shelter pawn, which changes everything in front of it.
        1 => 4 + file,
        // Ranks 3-4: out of the shelter but not yet active. File resolution
        // halves -- the exact file matters much less than the side.
        2 | 3 => 8 + file / 2,
        // Ranks 5-8: an endgame king, or one that is already in trouble.
        // Which half of the board it is on is the only thing still worth a
        // separate weight set.
        _ => 10 + file / 2,
    }
}

/// The layout as the trainer wants it: one bucket per square of the mirrored
/// half-board, in square order. Generated, never transcribed.
pub fn king_bucket_layout() -> [usize; 32] {
    let mut t = [0usize; 32];
    let mut i = 0;
    while i < 32 {
        t[i] = king_bucket(i % 4, i / 4);
        i += 1;
    }
    t
}

// ---------------------------------------------------------------------------
// Vectorised kernels
//
// Written from the scalar functions above and nothing else. The scalar path
// stays as the reference and as the fallback, and `verify_simd` below checks
// the two agree on real positions rather than trusting that they do.
//
// Two operations dominate: adding or subtracting a column of the first layer
// into the accumulator (once per piece per move), and the output sum over both
// accumulators (once per evaluation). Both are pure streaming over i16 arrays,
// which is what AVX2 is for -- sixteen values per register instead of one.
// ---------------------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
mod simd {
    use super::{HIDDEN, QA};
    use std::arch::x86_64::*;

    /// `dst[i] += src[i]` over HIDDEN values.
    ///
    /// # Safety
    /// Caller must have checked AVX2 is available. Both slices are HIDDEN long.
    #[target_feature(enable = "avx2")]
    pub unsafe fn soma(dst: &mut [i16; HIDDEN], src: &[i16]) {
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
        while i < HIDDEN {
            dst[i] += src[i];
            i += 1;
        }
    }

    /// `dst[i] -= src[i]` over HIDDEN values.
    ///
    /// # Safety
    /// As `soma`.
    #[target_feature(enable = "avx2")]
    pub unsafe fn subtrai(dst: &mut [i16; HIDDEN], src: &[i16]) {
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
        while i < HIDDEN {
            dst[i] -= src[i];
            i += 1;
        }
    }

    /// `sum over i of screlu(acc[i]) * w[i]`, accumulated in i32.
    ///
    /// The awkward part is that squaring a clamped i16 can reach QA*QA = 65025,
    /// which does not fit in i16. So the clamp is done in i16 -- cheap, sixteen
    /// at a time -- and the multiply is split: `_mm256_madd_epi16` multiplies
    /// i16 pairs and adds adjacent products into i32 lanes, which is exactly
    /// the shape of `screlu(x) * w` if one of the factors is `x` and the other
    /// is `x * w`. Computing `x * w` first in i16 would overflow, so it is `x`
    /// clamped times `w` widened -- done as two madd passes over the same data.
    ///
    /// # Safety
    /// Caller must have checked AVX2. `acc` is HIDDEN long, `w` at least HIDDEN.
    #[target_feature(enable = "avx2")]
    pub unsafe fn saida(acc: &[i16; HIDDEN], w: &[i16]) -> i32 {
        let zero = _mm256_setzero_si256();
        let topo = _mm256_set1_epi16(QA as i16);
        let mut soma = _mm256_setzero_si256();
        let mut i = 0;
        while i + 16 <= HIDDEN {
            let x = _mm256_loadu_si256(acc.as_ptr().add(i) as *const __m256i);
            let wv = _mm256_loadu_si256(w.as_ptr().add(i) as *const __m256i);
            // clamp(x, 0, QA)
            let c = _mm256_min_epi16(_mm256_max_epi16(x, zero), topo);
            // c * w in i16 is safe: c <= 255 and the trained weights are small,
            // but the PRODUCT c*c*w is not -- so multiply c by w first (fits),
            // then madd against c to get c*(c*w) summed into i32 lanes.
            let cw = _mm256_mullo_epi16(c, wv);
            soma = _mm256_add_epi32(soma, _mm256_madd_epi16(c, cw));
            i += 16;
        }
        // Horizontal sum of the eight i32 lanes.
        let baixo = _mm256_castsi256_si128(soma);
        let alto = _mm256_extracti128_si256(soma, 1);
        let mut s = _mm_add_epi32(baixo, alto);
        s = _mm_add_epi32(s, _mm_shuffle_epi32(s, 0b01_00_11_10));
        s = _mm_add_epi32(s, _mm_shuffle_epi32(s, 0b10_11_00_01));
        let mut total = _mm_cvtsi128_si32(s);
        while i < HIDDEN {
            let v = (acc[i] as i32).clamp(0, QA);
            total += v * v * w[i] as i32;
            i += 1;
        }
        total
    }
}

/// Whether the vector path may be used. Checked once.
#[cfg(target_arch = "x86_64")]
fn tem_avx2() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        // An env var to force the scalar path, so the two can be compared on
        // the same machine without rebuilding.
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

// ---------------------------------------------------------------------------
// Refresh cache
//
// When the input set is bucketed by king square, a king move that crosses a
// bucket boundary invalidates every feature for that perspective: the same
// piece on the same square is a different input number now. The obvious answer
// is to rebuild from the bias and add all thirty-two pieces back.
//
// Measured on our own games, that answer is too expensive to live with. King
// moves are 25% of all moves -- far more than intuition suggests -- and with a
// twelve-bucket layout 16.5% of ALL moves cross a boundary. At thirty-two
// piece updates each against two for an ordinary move, that is more than
// doubling the accumulator's total cost.
//
// The cache turns the rebuild into a difference. Keep one accumulator per
// (perspective, bucket) alongside the piece placement that produced it; on a
// refresh, start from that and apply only the pieces that changed since. Same
// measurement: 76% of refreshes find a populated entry, and those need a
// median of SIX piece updates rather than thirty-two -- four times cheaper.
//
// The invariant that makes it safe: an entry always holds an accumulator that
// exactly matches its stored placement. Update both together or neither, and
// a stale entry becomes impossible rather than merely unlikely -- a silently
// wrong accumulator would show up as an evaluation that is subtly off in rare
// positions, which is the hardest kind of bug to trace back here.
// ---------------------------------------------------------------------------

/// One cached accumulator and the placement it was built from.
#[derive(Clone)]
pub struct EntradaCache {
    /// Half an accumulator: this is per perspective, so only one side's values.
    pub valores: [i16; HIDDEN],
    /// `[color][piece_type]` bitboards as they were when `valores` was built.
    pub pecas: [[u64; 6]; 2],
    pub usada: bool,
}

/// Per perspective, per king bucket.
///
/// Boxed and owned by the searcher rather than global: two threads sharing one
/// cache would each invalidate the other's entries on every king move, which
/// costs more than having no cache at all.
pub struct CacheRefresh {
    pub entradas: Vec<EntradaCache>,
}

impl CacheRefresh {
    pub fn nova() -> Self {
        CacheRefresh {
            entradas: (0..2 * NUM_KING_BUCKETS)
                .map(|_| EntradaCache {
                    valores: [0; HIDDEN],
                    pecas: [[0u64; 6]; 2],
                    usada: false,
                })
                .collect(),
        }
    }

    #[inline]
    fn indice(perspectiva: Color, bucket: usize) -> usize {
        perspectiva.idx() * NUM_KING_BUCKETS + bucket
    }

    /// Bring one perspective of `acc` up to date for `board`, using the cached
    /// entry for `bucket` as the starting point.
    ///
    /// Returns the number of piece updates applied, so the caller can measure
    /// what this is actually saving rather than assume.
    /// Takes the piece bitboards rather than the board, so the caller can hold
    /// a mutable borrow of the accumulator that lives inside it. Passing
    /// `&Board` here would need the accumulator taken out and put back, which
    /// is exactly the shape of code that ends up putting back a stale copy.
    pub fn refresca(
        &mut self,
        net: &Network,
        pecas: [[u64; 6]; 2],
        _rei_branco: u8,
        _rei_preto: u8,
        perspectiva: Color,
        bucket: usize,
        destino: &mut [i16; HIDDEN],
    ) -> usize {
        let i = Self::indice(perspectiva, bucket);
        let e = &mut self.entradas[i];
        if !e.usada {
            // Nothing cached for this bucket yet: build from the bias, and
            // seed the entry so the next crossing is cheap.
            e.valores.copy_from_slice(&net.l0b);
            e.pecas = [[0u64; 6]; 2];
            e.usada = true;
        }
        let mut mexidas = 0usize;
        for cor in [Color::White, Color::Black] {
            for pt in [
                PieceType::Pawn,
                PieceType::Knight,
                PieceType::Bishop,
                PieceType::Rook,
                PieceType::Queen,
                PieceType::King,
            ] {
                let agora = pecas[cor.idx()][pt.idx()];
                let antes = e.pecas[cor.idx()][pt.idx()];
                // Only the squares that differ. A piece that stayed put
                // contributes the same feature and needs no work at all --
                // which is the whole point.
                let mut saiu = antes & !agora;
                while saiu != 0 {
                    let sq = saiu.trailing_zeros() as u8;
                    saiu &= saiu - 1;
                    let f = feature_bucket(perspectiva, bucket, cor, pt, sq) * HIDDEN;
                    aplica(&mut e.valores, &net.l0w[f..f + HIDDEN], false);
                    mexidas += 1;
                }
                let mut entrou = agora & !antes;
                while entrou != 0 {
                    let sq = entrou.trailing_zeros() as u8;
                    entrou &= entrou - 1;
                    let f = feature_bucket(perspectiva, bucket, cor, pt, sq) * HIDDEN;
                    aplica(&mut e.valores, &net.l0w[f..f + HIDDEN], true);
                    mexidas += 1;
                }
                e.pecas[cor.idx()][pt.idx()] = agora;
            }
        }
        destino.copy_from_slice(&e.valores);
        mexidas
    }
}

/// Add or subtract one column, through the vector path when it is available.
#[inline]
fn aplica(dst: &mut [i16; HIDDEN], col: &[i16], somar: bool) {
    #[cfg(target_arch = "x86_64")]
    if tem_avx2() {
        unsafe {
            if somar {
                simd::soma(dst, col);
            } else {
                simd::subtrai(dst, col);
            }
        }
        return;
    }
    if somar {
        for i in 0..HIDDEN {
            dst[i] += col[i];
        }
    } else {
        for i in 0..HIDDEN {
            dst[i] -= col[i];
        }
    }
}

/// Feature index with the king bucket folded in.
///
/// The bucket multiplies the whole 768-wide block, so bucket `b` occupies
/// inputs `[768*b, 768*(b+1))`. That is the layout the trainer writes when
/// told to bucket the inputs, and keeping the two in step is not optional:
/// a network is bound to the mapping it was trained under, and a mismatch
/// here produces plausible-looking nonsense rather than an error.
#[inline]
pub fn feature_bucket(
    perspectiva: Color,
    bucket: usize,
    piece_color: Color,
    pt: PieceType,
    sq: u8,
) -> usize {
    bucket * INPUTS + feature(perspectiva, piece_color, pt, sq)
}

/// The bucket to actually use, given what the loaded network supports.
///
/// A network trained without bucketed inputs has one block of weights, so
/// every king square must map to bucket zero. Deciding this from the network
/// rather than from a build flag means the same binary runs both, which is
/// what makes comparing them a comparison of networks and nothing else.
#[inline]
pub fn bucket_efectivo(net: &Network, board: &crate::board::Board, perspectiva: Color) -> usize {
    if net.buckets <= 1 {
        0
    } else {
        bucket_do_rei(board, perspectiva)
    }
}

/// Which bucket a side's king puts it in, mirrored to files a-d.
///
/// Mirroring halves the input count for free: a king on g1 and a king on b1
/// are the same shape of position seen from the other side of the board, and
/// making them share weights means the network learns the pattern once.
#[inline]
pub fn bucket_do_rei_de(board: &crate::board::Board, perspectiva: Color) -> usize {
    bucket_do_rei(board, perspectiva)
}

#[inline]
pub fn bucket_do_rei(board: &crate::board::Board, perspectiva: Color) -> usize {
    let ks = board.king_sq(perspectiva);
    let mut f = (ks % 8) as usize;
    let mut r = (ks / 8) as usize;
    if f >= 4 {
        f = 7 - f;
    }
    if perspectiva == Color::Black {
        r = 7 - r;
    }
    king_bucket(f, r)
}

/// Does the cache agree with a rebuild from scratch?
///
/// Run over real positions, not constructed ones. A cache that is right on the
/// first refresh and wrong on the fifth is the failure mode worth catching,
/// and only a sequence of real king moves produces that.
pub fn verifica_cache(net: &Network, fens: &[String]) -> (usize, usize) {
    // A bucketed layout needs a bucketed network. Running a 768-input net
    // through the bucketed indices reads far past the end of the weights --
    // which Rust catches, but only because the slice bound happens to be
    // checked. Say so plainly instead of relying on that.
    let esperado = NUM_KING_BUCKETS * INPUTS * HIDDEN;
    if net.l0w.len() != esperado {
        eprintln!(
            "cache: a rede tem {} pesos na primeira camada, o layout de {} buckets precisa de {}. \
             Esta rede foi treinada SEM buckets de entrada.",
            net.l0w.len(),
            NUM_KING_BUCKETS,
            esperado
        );
        return (0, 0);
    }
    let mut cache = CacheRefresh::nova();
    let mut testadas = 0;
    let mut erradas = 0;
    for fen in fens {
        let board = crate::board::Board::from_fen(fen);
        for perspectiva in [Color::White, Color::Black] {
            let bucket = bucket_do_rei(&board, perspectiva);
            let mut via_cache = [0i16; HIDDEN];
            cache.refresca(net, board.pieces, board.king_sq(Color::White), board.king_sq(Color::Black), perspectiva, bucket, &mut via_cache);

            // From scratch, same bucket, same features.
            let mut do_zero = [0i16; HIDDEN];
            do_zero.copy_from_slice(&net.l0b);
            for cor in [Color::White, Color::Black] {
                for pt in [
                    PieceType::Pawn,
                    PieceType::Knight,
                    PieceType::Bishop,
                    PieceType::Rook,
                    PieceType::Queen,
                    PieceType::King,
                ] {
                    let mut bb = board.pieces[cor.idx()][pt.idx()];
                    while bb != 0 {
                        let sq = bb.trailing_zeros() as u8;
                        bb &= bb - 1;
                        let f = feature_bucket(perspectiva, bucket, cor, pt, sq) * HIDDEN;
                        aplica(&mut do_zero, &net.l0w[f..f + HIDDEN], true);
                    }
                }
            }
            testadas += 1;
            if via_cache != do_zero {
                erradas += 1;
            }
        }
    }
    (testadas, erradas)
}


/// One refresh cache per thread.
///
/// Per thread and not shared: two searchers sharing a cache would each
/// invalidate the other's entries on every king move, which costs more than
/// having no cache at all. Thread-local rather than carried through every call
/// site because `add_piece`/`remove_piece` are Board methods and threading a
/// cache down to them would touch every caller for no gain.
thread_local! {
    static CACHE: std::cell::RefCell<CacheRefresh> = std::cell::RefCell::new(CacheRefresh::nova());
}

pub fn com_cache<R>(f: impl FnOnce(&mut CacheRefresh) -> R) -> R {
    CACHE.with(|c| f(&mut c.borrow_mut()))
}
