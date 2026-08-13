//! Reads and evaluates the littleindian "li11" network format.
//!
//! This is a genuinely different architecture from this engine's own
//! network (see `nnue.rs`), not just a different file layout: king-bucketed
//! inputs (32 buckets, own king excluded as a feature -- it only chooses the
//! bucket), an optional threats block added into the same accumulator, and a
//! `fc0 -> fc1 -> fc2` stack after the accumulator with two non-standard
//! activations (a paired multiply, then SCReLU+CReLU concatenated) plus a
//! skip connection. Ported literally from `littleindian`'s own
//! `evaluateLI11Impl` (`src/napoleon/nnue_net.cpp`), read function by
//! function rather than guessed from the file format alone.
//!
//! The reference adds a PSQT side-table (`psqt`) straight from piece
//! positions, outside the learned pipeline, and it is PART OF THE SCORE:
//!
//!     psqtBias = ((psqtUs - psqtThem) / qa) / 2
//!     score    = lround((out + psqtBias) * OUTPUT_SCALE_CP)
//!
//! It was previously read and thrown away, on the theory that this module
//! "activates the neural path only". That was wrong in the same way, and for
//! the same reason, as discarding the factoriser block in `nnue_threats`:
//! the PSQT carries the MATERIAL, and the learned stack is trained as a
//! correction on top of it. Keeping the correction and dropping the base
//! leaves an evaluation with no idea what a queen is worth.
//!
//! Measured with it discarded, against Stockfish over 3000 real positions:
//! correlation -0.001 -- pure noise -- with an extra queen priced at 362cp
//! and a balanced position reading +384. Three SPRTs had scored this network
//! 0-169-0 and concluded it was weak; what was being tested was a reader
//! missing half the evaluation.
//!
//! The king-bucket layout, piece-kind mapping and threat feature indexing
//! are NOT re-derived here -- they already exist in `features.rs`
//! (`BUCKET_MAP`, `gather_pieces`, `gather_threats_full`), because that file
//! is itself the reference the C++ comments point back to
//! ("espelho EXATO do gather_threats_full (Rust)"). Reusing it means the
//! feature indexing is exercised by whatever already validates
//! `features.rs`, not a second hand-typed copy that could quietly drift.

use crate::board::Board;
use crate::features::{self, Pos};

const MAGIC: &[u8; 8] = b"NAPKLI11";
const CHUNK_MAGIC: &[u8; 17] = b"COMPRESSED_LEB128";
const PIECE_FEATURES: usize = features::PIECE_FEATURES; // 22528
const THREAT_FEATURES_FULL: usize = features::THREAT_FEATURES_FULL; // 9216
const MATERIAL_BUCKETS: usize = 8;
/// Tecto para os buffers de pilha do `evaluate()` -- mesma ideia do MAX_L1 do
/// littleindian. As redes que carregamos vao ate 512; 1024 fica com folga
/// sem custar nada (e' pilha, nao heap).
const MAX_L1: usize = 1024;
const FC0_REAL: usize = 32;
const FC0_TOTAL: usize = 33;
const FC1_OUT: usize = 32;

pub struct RedeLi11 {
    l1: usize,
    qa: f32,
    qb_fc: f32,
    qb_fc2: f32,
    acc_bias: Vec<i16>,          // [l1]
    acc_weight: Vec<i16>,        // [PIECE_FEATURES * l1]
    fc0_w: Vec<i16>,             // [l1 * FC0_TOTAL], row-major [out][in]
    fc0_b: Vec<i32>,             // [FC0_TOTAL]
    fc1_w: Vec<i16>,             // [64 * FC1_OUT]
    fc1_b: Vec<i32>,             // [FC1_OUT]
    fc2_w: Vec<i16>,             // [FC1_OUT * MATERIAL_BUCKETS]
    fc2_b: Vec<i32>,             // [MATERIAL_BUCKETS]
    has_threats: bool,
    /// Tabela PSQT por (feature, balde de material). Somada a' saida, nao
    /// descartada -- ver a nota do modulo.
    psqt: Vec<i32>,
    threat_weight: Vec<i16>,     // [THREAT_FEATURES_FULL * l1], empty if absent
}

// --- LEB128 + zigzag, generic over the decoded width -----------------------
//
// Mirrors littleindian's own `lebOne`/`lebI16`/`lebI32`: a scalar,
// byte-at-a-time varint decode with zigzag sign mapping, generic over an
// i64 accumulator and clamped to the target width by the caller. Written
// fresh rather than reusing this engine's own i16-only LEB128 decoder in
// `nnue.rs` -- this format also carries i32 tensors (biases), and the two
// decoders serve genuinely different file formats besides.
fn leb_one(bytes: &[u8], pos: &mut usize) -> i64 {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    loop {
        let Some(&byte) = bytes.get(*pos) else { return 0 };
        *pos += 1;
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 63 {
            return 0;
        }
    }
    ((result >> 1) as i64) ^ -((result & 1) as i64)
}

fn leb_i16(bytes: &[u8], out: &mut [i16]) {
    let mut pos = 0;
    for slot in out.iter_mut() {
        if pos >= bytes.len() {
            break;
        }
        *slot = leb_one(bytes, &mut pos).clamp(-32768, 32767) as i16;
    }
}

fn leb_i32(bytes: &[u8], out: &mut [i32]) {
    let mut pos = 0;
    for slot in out.iter_mut() {
        if pos >= bytes.len() {
            break;
        }
        *slot = leb_one(bytes, &mut pos).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    }
}

fn read_chunk<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    if *pos + 17 + 4 > bytes.len() {
        return None;
    }
    if &bytes[*pos..*pos + 17] != CHUNK_MAGIC {
        return None;
    }
    *pos += 17;
    let len = u32::from_le_bytes(bytes[*pos..*pos + 4].try_into().ok()?) as usize;
    *pos += 4;
    if *pos + len > bytes.len() {
        return None;
    }
    let chunk = &bytes[*pos..*pos + len];
    *pos += len;
    Some(chunk)
}

pub fn load(bytes: &[u8]) -> Option<RedeLi11> {
    if bytes.len() < 24 || &bytes[0..8] != MAGIC {
        return None;
    }
    let l1 = u32::from_le_bytes(bytes[8..12].try_into().ok()?) as usize;
    let qa = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as f32;
    let qb_fc = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as f32;
    let qb_fc2 = u32::from_le_bytes(bytes[20..24].try_into().ok()?) as f32;
    let mut pos = 24;

    let mut acc_bias = vec![0i16; l1];
    leb_i16(read_chunk(bytes, &mut pos)?, &mut acc_bias);

    let mut acc_weight = vec![0i16; PIECE_FEATURES * l1];
    leb_i16(read_chunk(bytes, &mut pos)?, &mut acc_weight);

    // O PSQT E' PARCELA, nao residuo -- ver a nota do modulo.
    let mut psqt = vec![0i32; PIECE_FEATURES * MATERIAL_BUCKETS];
    leb_i32(read_chunk(bytes, &mut pos)?, &mut psqt);
    {
        // Um bloco de zeros e um bloco mal lido produzem o mesmo silencio.
        let nz = psqt.iter().filter(|&&v| v != 0).count();
        let amp: i64 = psqt.iter().map(|&v| (v as i64).abs()).sum::<i64>()
            / (psqt.len() as i64).max(1);
        eprintln!("nnue_li11: psqt {} valores, {} nao-nulos, amplitude media {}",
                  psqt.len(), nz, amp);
    }

    let mut fc0_w = vec![0i16; l1 * FC0_TOTAL];
    leb_i16(read_chunk(bytes, &mut pos)?, &mut fc0_w);
    let mut fc0_b = vec![0i32; FC0_TOTAL];
    leb_i32(read_chunk(bytes, &mut pos)?, &mut fc0_b);

    let mut fc1_w = vec![0i16; 64 * FC1_OUT];
    leb_i16(read_chunk(bytes, &mut pos)?, &mut fc1_w);
    let mut fc1_b = vec![0i32; FC1_OUT];
    leb_i32(read_chunk(bytes, &mut pos)?, &mut fc1_b);

    let mut fc2_w = vec![0i16; FC1_OUT * MATERIAL_BUCKETS];
    leb_i16(read_chunk(bytes, &mut pos)?, &mut fc2_w);
    let mut fc2_b = vec![0i32; MATERIAL_BUCKETS];
    leb_i32(read_chunk(bytes, &mut pos)?, &mut fc2_b);

    // Threats are an optional trailing chunk (absent when the network was
    // trained with NAPK_THREATS=none). A missing/short chunk here is not an
    // error -- it just means this network has none, same contract as the
    // reference's own `hasThreats`.
    let mut has_threats = false;
    let mut threat_weight = Vec::new();
    if let Some(chunk) = read_chunk(bytes, &mut pos) {
        threat_weight = vec![0i16; THREAT_FEATURES_FULL * l1];
        leb_i16(chunk, &mut threat_weight);
        has_threats = threat_weight.iter().any(|&v| v != 0);
    }

    Some(RedeLi11 {
        l1, qa, qb_fc, qb_fc2,
        acc_bias, acc_weight,
        fc0_w, fc0_b, fc1_w, fc1_b, fc2_w, fc2_b,
        has_threats, threat_weight, psqt,
    })
}

/// On by default, same convention as `nnue_threats::AMEACAS` -- whether this
/// network actually gets used is decided by which file `KESTREL_NNUE_LI11`
/// points at (or its absence), and this switch exists on top of that so a
/// loaded li11 network can be compared against with it on and off, over
/// UCI, without restarting with a different environment.
static LI11_LIGADA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

#[inline]
pub fn li11_ligada() -> bool {
    LI11_LIGADA.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_li11(v: bool) {
    LI11_LIGADA.store(v, std::sync::atomic::Ordering::Relaxed);
}

/// This network's own exchange rate between its output and centipawns.
///
/// NOT `nnue::escala()`. That one was walked across a real range and
/// SPRT-tested for THIS engine's own network -- there is no reason to
/// expect the number that came out of that search also fits a network
/// with a different architecture, trained separately, at a different
/// internal scale. The reference engine's own hardcoded OUTPUT_SCALE_CP
/// (400) is the starting default here, not a proven value: it is what
/// littleindian ships with, not a number this project has measured. Needs
/// its own SPRT before it means anything more than "plausible".
static LI11_ESCALA: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(400);

#[inline]
pub fn escala() -> i32 {
    LI11_ESCALA.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_escala(v: i32) {
    LI11_ESCALA.store(v.clamp(1, 4000), std::sync::atomic::Ordering::Relaxed);
}

static REDE: std::sync::OnceLock<Option<RedeLi11>> = std::sync::OnceLock::new();

pub fn rede() -> Option<&'static RedeLi11> {
    REDE.get_or_init(|| {
        let path = std::env::var("KESTREL_NNUE_LI11").ok()?;
        let bytes = std::fs::read(&path).ok()?;
        let n = load(&bytes);
        if let Some(net) = &n {
            eprintln!("nnue_li11: rede carregada de {} (L1={} threats={})", path, net.l1, net.has_threats);
        } else {
            eprintln!("nnue_li11: {} nao e' uma rede li11 valida", path);
        }
        n
    })
    .as_ref()
}

/// out[o] = bias[o] + dot(w[o*n_in..+n_in], input[0..n_in]), i64 accumulation
/// to match the reference's own `int64_t sum` (weights and input are small
/// enough that i32 would not overflow here either, but this is what it
/// does).
fn dense_u8(w: &[i16], b: &[i32], input: &[u8], n_in: usize, n_out: usize, out: &mut [i32]) {
    for o in 0..n_out {
        let row = &w[o * n_in..o * n_in + n_in];
        let mut sum: i64 = b[o] as i64;
        for i in 0..n_in {
            sum += row[i] as i64 * input[i] as i64;
        }
        out[o] = sum as i32;
    }
}

// --- Piece accumulator cache (Finny table) ---------------------------------
//
// Ported from littleindian's own li11FinnyResolve/li11ApplyDiff/li11PlyResolve
// (nnue_net.cpp) -- one cached accumulator per (perspective, king bucket),
// diffed against the piece placement it was built from rather than rebuilt
// from scratch every call. Threats are NOT part of this cache: the reference
// recomputes them fresh every call too (`gatherThreatsFull` inside
// `evaluateLI11Impl`, unconditionally) and fuses them into whichever
// accumulator this cache hands back. Matching that division of labour
// exactly, not guessing at a different one -- caching threats as well would
// need tracking which threat features a move could invalidate beyond the
// piece that moved (attack lines opened or closed for OTHER pieces), which
// neither this engine's own threats module nor the reference actually does.
//
// This engine's own network keeps its equivalent cache in its Board-carried
// Accumulator, updated through add_piece/remove_piece. This one is
// self-contained in a thread-local instead: Board doesn't carry an li11
// accumulator, and adding one would mean threading it through every
// make_move/unmake_move call site for a network that may not even be
// loaded. A thread-local diffed against the CURRENT board's own piece
// bitboards needs no such wiring -- correct regardless of how the position
// was reached, same safety invariant as the reference's own cache: an entry
// always holds an accumulator that exactly matches its stored placement.
struct EntradaCacheLi11 {
    valores: Vec<i32>,
    pecas: [[u64; 6]; 2],
    usada: bool,
}

struct CacheRefreshLi11 {
    entradas: Vec<EntradaCacheLi11>,
    l1: usize,
}

const NUM_KING_BUCKETS_LI11: usize = 32;

impl CacheRefreshLi11 {
    fn nova(l1: usize) -> Self {
        CacheRefreshLi11 {
            entradas: (0..2 * NUM_KING_BUCKETS_LI11)
                .map(|_| EntradaCacheLi11 { valores: vec![0i32; l1], pecas: [[0u64; 6]; 2], usada: false })
                .collect(),
            l1,
        }
    }

    fn refresca(&mut self, net: &RedeLi11, pecas: [[u64; 6]; 2], persp: usize, bucket: usize, destino: &mut [i32]) {
        if self.l1 != net.l1 {
            *self = CacheRefreshLi11::nova(net.l1);
        }
        let l1 = self.l1;
        let i = persp * NUM_KING_BUCKETS_LI11 + bucket;
        let e = &mut self.entradas[i];
        if !e.usada {
            for (dst, &b) in e.valores.iter_mut().zip(net.acc_bias.iter()) {
                *dst = b as i32;
            }
            e.pecas = [[0u64; 6]; 2];
            e.usada = true;
        }
        for c in 0..2 {
            for t in 0..6 {
                let agora = pecas[c][t];
                let antes = e.pecas[c][t];
                if agora == antes {
                    continue;
                }
                let mut saiu = antes & !agora;
                while saiu != 0 {
                    let sq = saiu.trailing_zeros() as usize;
                    saiu &= saiu - 1;
                    if let Some(f) = li11_feature(bucket, persp, c, t, sq) {
                        let row = &net.acc_weight[f * l1..f * l1 + l1];
                        for k in 0..l1 {
                            e.valores[k] -= row[k] as i32;
                        }
                    }
                }
                let mut entrou = agora & !antes;
                while entrou != 0 {
                    let sq = entrou.trailing_zeros() as usize;
                    entrou &= entrou - 1;
                    if let Some(f) = li11_feature(bucket, persp, c, t, sq) {
                        let row = &net.acc_weight[f * l1..f * l1 + l1];
                        for k in 0..l1 {
                            e.valores[k] += row[k] as i32;
                        }
                    }
                }
                e.pecas[c][t] = agora;
            }
        }
        destino.copy_from_slice(&e.valores);
    }
}

/// Same indexing `features::gather_pieces` computes in bulk, but for one
/// (colour, piece type, square) at a time -- what a diff-based cache needs.
/// `None` for the perspective's own king: it chooses the bucket, it is not
/// itself a feature.
#[inline]
fn li11_feature(bucket: usize, persp: usize, c: usize, t: usize, sq_raw: usize) -> Option<usize> {
    let sq = if persp == 0 { sq_raw } else { sq_raw ^ 56 };
    if t == 5 {
        if c != persp { Some(bucket * 704 + sq) } else { None }
    } else {
        let idx = features::piece_idx(t) + if c == persp { 0 } else { 5 };
        Some(bucket * 704 + idx * 64 + sq)
    }
}

/// Cache incremental das ameacas, por diferenca directa contra a chamada
/// anterior -- SEM ordenar, SEM alocar por chamada.
///
/// A tentativa anterior (ver o comentario em `evaluate`) fez exactamente esta
/// ideia mas com uma lista ORDENADA e alocacoes por chamada, e a
/// contabilidade custou mais do que poupou (67k vs 112k nps). O diff em si
/// era pequeno -- 3,5 de 42 ameacas mudam por chamada, medido -- o problema
/// nunca foi a ideia, foi o custo de a executar.
///
/// Aqui o diff e' O(|lista velha| + |lista nova|), com dois vectores
/// reutilizados (nunca `Vec::new()` dentro do loop) e nada a ordenar: um
/// array de presenca (`activo`) do tamanho do espaco de features, tocado so'
/// nos indices que aparecem nas listas -- nunca percorrido inteiro.
///
/// CORRECTO por construcao, independente de quao parecidas sejam duas
/// chamadas consecutivas: o resultado e' sempre "remove o que saiu, adiciona
/// o que entrou" a partir do estado realmente activo, nunca uma aproximacao.
/// Duas chamadas para posicoes completamente distintas dao o resultado
/// certo -- so' nao ganham velocidade nenhuma, o que e' o pior caso e nao um
/// caso errado.
struct CacheAmeacasLi11 {
    activo: Vec<bool>,        // THREAT_FEATURES_FULL, por perspectiva
    presente_agora: Vec<bool>, // scratch, tocado e limpo a cada chamada
    lista_anterior: Vec<usize>,
    acc: Vec<i32>,
    l1: usize,
}

impl CacheAmeacasLi11 {
    fn nova(l1: usize) -> Self {
        CacheAmeacasLi11 {
            activo: vec![false; crate::features::THREAT_FEATURES_FULL],
            presente_agora: vec![false; crate::features::THREAT_FEATURES_FULL],
            lista_anterior: Vec::new(),
            acc: vec![0i32; l1],
            l1,
        }
    }

    /// `nova_lista` e' a lista de features activas AGORA (de `gather_threats_full`).
    /// Actualiza `self.acc` para as somar, devolve-o pronto a fundir no
    /// acumulador de pecas.
    fn actualiza(&mut self, net: &RedeLi11, nova_lista: &[usize]) -> &[i32] {
        if self.l1 != net.l1 {
            *self = CacheAmeacasLi11::nova(net.l1);
        }
        let l1 = self.l1;

        for &f in nova_lista {
            self.presente_agora[f] = true;
        }
        // Saiu: estava activo, nao esta na lista nova.
        for &f in &self.lista_anterior {
            if !self.presente_agora[f] {
                let row = &net.threat_weight[f * l1..f * l1 + l1];
                for i in 0..l1 {
                    self.acc[i] -= row[i] as i32;
                }
                self.activo[f] = false;
            }
        }
        // Entrou: esta na lista nova, nao estava activo.
        for &f in nova_lista {
            if !self.activo[f] {
                let row = &net.threat_weight[f * l1..f * l1 + l1];
                for i in 0..l1 {
                    self.acc[i] += row[i] as i32;
                }
                self.activo[f] = true;
            }
        }
        // Limpa o scratch (so' os indices tocados, nunca o array inteiro) e
        // troca os buffers -- zero alocacoes depois do arranque.
        for &f in nova_lista {
            self.presente_agora[f] = false;
        }
        self.lista_anterior.clear();
        self.lista_anterior.extend_from_slice(nova_lista);
        &self.acc
    }
}

thread_local! {
    static CACHE: std::cell::RefCell<Option<CacheRefreshLi11>> = const { std::cell::RefCell::new(None) };
    /// Reused across calls instead of a fresh `Vec::new()` each time.
    static THR_BUF: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Uma entrada por perspectiva -- ameacas nao partilham estado entre elas.
    static CACHE_AMEACAS: std::cell::RefCell<[Option<CacheAmeacasLi11>; 2]> =
        const { std::cell::RefCell::new([None, None]) };
}

/// Piece accumulator comes from the thread-local Finny cache (see above);
/// threats are recomputed fresh every call and fused in after, matching the
/// reference's own division of labour exactly.
pub fn evaluate(net: &RedeLi11, board: &Board) -> i32 {
    let l1 = net.l1;
    let pos = Pos { pieces: board.pieces };

    let ksq_w = board.pieces[0][5].trailing_zeros() as usize;
    let ksq_b = board.pieces[1][5].trailing_zeros() as usize;
    let bucket_w = features::BUCKET_MAP[ksq_w];
    let bucket_b = features::BUCKET_MAP[ksq_b ^ 56];

    // Pilha, nao heap -- eram dois `vec![0i32; l1]` por chamada, ou seja
    // duas alocacoes por avaliacao. A 100 mil avaliacoes/seg (ja' com o cache
    // das ameacas), isso e' 200 mil malloc/free por segundo so' para escrever
    // valores que cabem num array de tamanho fixo.
    let mut acc_w_buf = [0i32; MAX_L1];
    let mut acc_b_buf = [0i32; MAX_L1];
    let acc_w = &mut acc_w_buf[..l1];
    let acc_b = &mut acc_b_buf[..l1];
    CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        let cache = cache.get_or_insert_with(|| CacheRefreshLi11::nova(l1));
        cache.refresca(net, board.pieces, 0, bucket_w, acc_w);
        cache.refresca(net, board.pieces, 1, bucket_b, acc_b);
    });

    // Incremental por diferenca directa (ver `CacheAmeacasLi11`), nao mais
    // recomputado por inteiro a cada chamada. A tentativa anterior com a
    // mesma ideia (lista ordenada, alocada por chamada) piorou o NPS porque a
    // contabilidade custava mais do que poupava -- a ideia sempre esteve
    // certa, o custo de a executar e' que nao. Este caminho nao ordena nem
    // aloca depois do arranque, e e' matematicamente correcto mesmo entre
    // duas chamadas para posicoes sem relacao nenhuma (nesse caso so' nao
    // ganha velocidade, nunca da' um valor errado).
    //
    // Partilha a opcao "Threats" com nnue_threats.rs, como antes: sem isto,
    // `setoption name Threats value false` so' silenciava a OUTRA rede de
    // ameacas.
    if net.has_threats && crate::nnue_threats::ameacas_ligadas() {
        let maps = features::compute_attack_maps_by_type(&pos);
        THR_BUF.with(|buf| {
            CACHE_AMEACAS.with(|ca| {
                let mut cache = ca.borrow_mut();
                let mut thr = buf.borrow_mut();

                thr.clear();
                features::gather_threats_full(&pos, 0, &maps, &mut thr);
                let c0 = cache[0].get_or_insert_with(|| CacheAmeacasLi11::nova(l1));
                let acc_thr_w = c0.actualiza(net, &thr);
                for i in 0..l1 {
                    acc_w[i] += acc_thr_w[i];
                }

                thr.clear();
                features::gather_threats_full(&pos, 1, &maps, &mut thr);
                let c1 = cache[1].get_or_insert_with(|| CacheAmeacasLi11::nova(l1));
                let acc_thr_b = c1.actualiza(net, &thr);
                for i in 0..l1 {
                    acc_b[i] += acc_thr_b[i];
                }
            });
        });
    }

    let (acc_us, acc_them): (&[i32], &[i32]) = if board.side == crate::types::Color::White {
        (&acc_w[..], &acc_b[..])
    } else {
        (&acc_b[..], &acc_w[..])
    };

    // Paired activation: clip [0,QA] over the whole L1, multiply halves
    // (stays in [0,QA]), rescale to [0,127] -- same convention on both
    // perspectives, written into the two halves of `concat`.
    let qa_i = net.qa as i32;
    let half = l1 / 2;
    let mut concat_buf = [0u8; MAX_L1];
    let concat = &mut concat_buf[..l1];
    for i in 0..half {
        let a = acc_us[i].clamp(0, qa_i);
        let b = acc_us[i + half].clamp(0, qa_i);
        let prod = (a * b) / qa_i.max(1);
        let rescaled = (prod * 127 + qa_i.max(1) / 2) / qa_i.max(1);
        concat[i] = rescaled.clamp(0, 127) as u8;
    }
    for i in 0..half {
        let a = acc_them[i].clamp(0, qa_i);
        let b = acc_them[i + half].clamp(0, qa_i);
        let prod = (a * b) / qa_i.max(1);
        let rescaled = (prod * 127 + qa_i.max(1) / 2) / qa_i.max(1);
        concat[half + i] = rescaled.clamp(0, 127) as u8;
    }

    let mut fc0_raw = [0i32; FC0_TOTAL];
    dense_u8(&net.fc0_w, &net.fc0_b, concat, l1, FC0_TOTAL, &mut fc0_raw);
    let dequant_fc0 = (1.0 / net.qb_fc) / 127.0;
    let bias_dequant_fc0 = 1.0 / net.qb_fc;
    let mut fc0_out = [0f32; FC0_TOTAL];
    for i in 0..FC0_TOTAL {
        fc0_out[i] = net.fc0_b[i] as f32 * bias_dequant_fc0
            + (fc0_raw[i] - net.fc0_b[i]) as f32 * dequant_fc0;
    }

    let mut concat64 = [0f32; 64];
    for i in 0..FC0_REAL {
        let c = fc0_out[i].clamp(0.0, 1.0);
        concat64[i] = c * c; // screlu: clamp(x,0,1)^2
        concat64[FC0_REAL + i] = c; // crelu: clamp(x,0,1)
    }

    let bias_dequant_fc1 = 1.0 / net.qb_fc;
    let w_dequant_fc1 = 1.0 / net.qb_fc;
    let mut fc1_out = [0f32; FC1_OUT];
    for o in 0..FC1_OUT {
        let row = &net.fc1_w[o * 64..o * 64 + 64];
        let mut dot = 0f32;
        for i in 0..64 {
            dot += row[i] as f32 * concat64[i];
        }
        fc1_out[o] = (net.fc1_b[o] as f32 * bias_dequant_fc1 + dot * w_dequant_fc1).clamp(0.0, 1.0);
    }

    let bias_dequant_fc2 = 1.0 / net.qb_fc2;
    let w_dequant_fc2 = 1.0 / net.qb_fc2;
    let mut fc2_out = [0f32; MATERIAL_BUCKETS];
    for o in 0..MATERIAL_BUCKETS {
        let row = &net.fc2_w[o * FC1_OUT..o * FC1_OUT + FC1_OUT];
        let mut dot = 0f32;
        for i in 0..FC1_OUT {
            dot += row[i] as f32 * fc1_out[i];
        }
        fc2_out[o] = net.fc2_b[o] as f32 * bias_dequant_fc2 + dot * w_dequant_fc2;
    }

    let piece_count: u32 = board.pieces.iter().flatten().map(|bb| bb.count_ones()).sum();
    let bucket = (((piece_count as i32) - 1) / 4).clamp(0, (MATERIAL_BUCKETS - 1) as i32) as usize;

    static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *DEBUG.get_or_init(|| std::env::var("LI11_DEBUG").is_ok()) {
        eprintln!("bucket={} accUs[0..4]={},{},{},{} accThem[0..4]={},{},{},{}",
            bucket, acc_us[0], acc_us[1], acc_us[2], acc_us[3],
            acc_them[0], acc_them[1], acc_them[2], acc_them[3]);
        eprintln!("fc0_out[0..4]={:.4},{:.4},{:.4},{:.4} skip(fc0_out[32])={:.4}",
            fc0_out[0], fc0_out[1], fc0_out[2], fc0_out[3], fc0_out[FC0_REAL]);
        eprintln!("fc1_out[0..4]={:.4},{:.4},{:.4},{:.4}",
            fc1_out[0], fc1_out[1], fc1_out[2], fc1_out[3]);
        eprintln!("fc2_out[bucket={}]={:.4}", bucket, fc2_out[bucket]);
        // Despejo completo, para comparar valor a valor contra Python sem
        // adivinhar a partir de 4 amostras.
        if let Ok(caminho) = std::env::var("LI11_DEBUG_DUMP") {
            use std::io::Write;
            if let Ok(mut f) = std::fs::File::create(&caminho) {
                let _ = writeln!(f, "acc_us={:?}", acc_us);
                let _ = writeln!(f, "acc_them={:?}", acc_them);
                let _ = writeln!(f, "fc0_out={:?}", fc0_out);
                let _ = writeln!(f, "fc1_out={:?}", fc1_out);
                let _ = writeln!(f, "fc2_out={:?}", fc2_out);
                let _ = writeln!(f, "bucket={}", bucket);
            }
        }
    }

    // fc0_out[FC0_REAL] (index 32) is the skip term -- unclamped, added
    // straight onto the bucket's fc2 output. Same role as this engine's own
    // network reading straight from the accumulator to one linear layer,
    // except li11 keeps an extra unclipped path alongside the MLP.
    let skip = fc0_out[FC0_REAL];

    // PSQT, do lado de cada perspectiva. Mapeamento tirado directamente do
    // `evaluateLI11Impl` do littleindian:
    //
    //     psW += psqt[makeFeat(b_w, kWk, sq) * MATERIAL_BUCKETS + bucket]
    //     psB += psqt[makeFeat(b_b, kBk, sq ^ 56) * MATERIAL_BUCKETS + bucket]
    //     psqtBias = ((psqtUs - psqtThem) / qa) / 2
    //
    // Feature-major (a feature multiplica, o balde soma), e as features sao
    // as MESMAS que alimentam o acumulador -- por isso reutiliza-se
    // `li11_feature` em vez de reescrever a indexacao, que e' onde estas
    // coisas divergem em silencio.
    let mut ps = [0i64; 2];
    for persp in 0..2usize {
        let bk = if persp == 0 { bucket_w } else { bucket_b };
        for c in 0..2usize {
            for t in 0..6usize {
                let mut bb = board.pieces[c][t];
                while bb != 0 {
                    let sq = bb.trailing_zeros() as usize;
                    bb &= bb - 1;
                    if let Some(f) = li11_feature(bk, persp, c, t, sq) {
                        ps[persp] += net.psqt[f * MATERIAL_BUCKETS + bucket] as i64;
                    }
                }
            }
        }
    }
    let stm = board.side.idx();
    let psqt_us = ps[if stm == 0 { 0 } else { 1 }] as f32;
    let psqt_them = ps[if stm == 0 { 1 } else { 0 }] as f32;
    let psqt_bias = ((psqt_us - psqt_them) / net.qa) / 2.0;

    let out = fc2_out[bucket] + skip + psqt_bias;

    // This network's OWN scale (see `escala()` above), not the main
    // network's -- the two were calibrated separately and there is no
    // reason to expect one number fits both.
    let score = (out * escala() as f32).round() as i32;
    score.clamp(-3000, 3000)
}
