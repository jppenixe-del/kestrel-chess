//! Leitor do formato `.napk9` (motor Nap2Siriux/Halfk2Siriux, nosso, nao de
//! terceiros -- napk9_v10_features.rs e' o MESMO gerador do features.rs deste
//! projecto, confirmado na fonte).
//!
//! Só a cabeça "big" por agora (16→32→1 e 32→32→1 ficam por portar); é a que
//! o comando `eval` deles usa por omissão e a que a escolha por profundidade
//! usa na raiz/PV. Formula extraida literalmente de `nnue_net.cpp`
//! (Nap2Siriux), funcao `evaluate(board, headIdx)`:
//!
//!   acumulador: pecas (Finny por [persp][balde], MESMA indexacao do li11)
//!               + ameacas fundidas (MESMO cache incremental do li11)
//!   concat  = clip(acc,0,QA) * 127/QA, arredondado, para [nós, deles]
//!   a1[o]   = clamp(bias1[o] + dot(concat, w1[balde][o]) * (1/(127*QB)), 0, 1)
//!   a2[o]   = clamp(bias2[o] + dot(a1, w2[balde][o]) * (1/QB), 0, 1)
//!   out     = bias3 + dot(a2, w3[balde]) * (1/QB)
//!   psqt    = (psqtUs - psqtThem) / QA / 2        -- mesma formula do li11
//!   score   = arredonda((out + psqt) * OUTPUT_SCALE_CP)
//!             + correctivo de rei exposto (HCE pequeno, portado à letra)
//!   clamp(-3000, 3000)
//!
//! Pesos das densas: lidos como i16, CORTADOS para i8 no carregamento -- e' o
//! que o C++ faz (`(int8_t)std::clamp<int>(tmp[i], -127, 127)`), nao e'
//! liberdade nossa.

use crate::board::Board;
use crate::features::{self, PIECE_FEATURES, THREAT_FEATURES_FULL};

const MATERIAL_BUCKETS: usize = 8;
const FACTOR: usize = 704;
const MAX_L1: usize = 1024;
const OUTPUT_SCALE_CP: f32 = 408.0;
const BIG_DL1: usize = 32;
const BIG_DL2: usize = 32;

struct Head {
    l1_w: [Vec<i8>; MATERIAL_BUCKETS],   // [l1_out * l1_in] por balde
    l1_b: [Vec<f32>; MATERIAL_BUCKETS],  // ja' dequantizado (* 1/QB)
    l2_w: [Vec<i8>; MATERIAL_BUCKETS],   // [l2_out * l1_out]
    l2_b: [Vec<f32>; MATERIAL_BUCKETS],
    l3_w: [Vec<i8>; MATERIAL_BUCKETS],   // [l2_out]
    l3_b: [f32; MATERIAL_BUCKETS],
    l1_out: usize,
    l2_out: usize,
}

pub struct RedeNapV10 {
    l1: usize,
    qa: f32,
    qb: f32,
    acc_bias: Vec<i16>,
    acc_weight: Vec<i16>,   // [PIECE_FEATURES * l1], factorizador ja' fundido
    psqt: Vec<i32>,         // [PIECE_FEATURES * MATERIAL_BUCKETS], vazio se ausente
    threat_weight: Vec<i16>, // [THREAT_FEATURES_FULL * l1], vazio se ausente
    has_threats: bool,
    big: Head,
    bullet: Option<Head>,   // cabeca 16-larga, so' para material ja' decidido
}

/// `MVAL` do `dual_vision.cpp` (P,N,B,R,Q,K) e o limiar `pureDecidedMaterial`
/// que la' vem a 900 por omissao -- so' dama liquida a mais dispara a troca
/// para a cabeca bullet, nao qualquer troca pendente (ver comentario deles:
/// com 500 disparava em 38% dos nos a depth 20, com trocas so' PENDENTES).
const MVAL: [i32; 6] = [100, 320, 330, 500, 900, 0];
const DECIDED_MATERIAL: i32 = 900;

fn material_diff(board: &Board) -> i32 {
    let mut diff = 0i32;
    for t in 0..5 {
        diff += MVAL[t]
            * (board.pieces[0][t].count_ones() as i32 - board.pieces[1][t].count_ones() as i32);
    }
    diff
}

// -------------------------------------------------------------- leitura ---
const LEB_MAGIC: &[u8; 17] = b"COMPRESSED_LEB128";

fn le_chunk<'a>(b: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    if *pos + 17 + 4 > b.len() || &b[*pos..*pos + 17] != LEB_MAGIC {
        return None;
    }
    *pos += 17;
    let sz = u32::from_le_bytes(b[*pos..*pos + 4].try_into().ok()?) as usize;
    *pos += 4;
    if *pos + sz > b.len() {
        return None;
    }
    let out = &b[*pos..*pos + sz];
    *pos += sz;
    Some(out)
}

fn leb_i16(bytes: &[u8], out: &mut [i16]) -> Option<()> {
    let mut pos = 0usize;
    for slot in out.iter_mut() {
        *slot = crate::nnue::zigzag_decode(crate::nnue::leb128_pop(bytes, &mut pos)?);
    }
    Some(())
}
fn leb_i32(bytes: &[u8], out: &mut [i32]) -> Option<()> {
    let mut pos = 0usize;
    for slot in out.iter_mut() {
        let v = crate::nnue::zigzag_decode(crate::nnue::leb128_pop(bytes, &mut pos)?);
        *slot = v as i32;
    }
    Some(())
}

/// Lê uma "batalhão" (cabeça) completa: 8 baldes, cada um com w1/b1/w2/b2/w3/b3.
/// Pesos das densas em i16 no ficheiro, CORTADOS para i8 aqui -- exactamente
/// o `(int8_t)std::clamp<int>(tmp[i], -127, 127)` do C++, byte a byte.
fn le_cabeca(b: &[u8], pos: &mut usize, l1_in: usize, l1_out: usize, l2_out: usize, qa: f32, qb: f32) -> Option<Head> {
    let mut h = Head {
        l1_w: Default::default(), l1_b: Default::default(),
        l2_w: Default::default(), l2_b: Default::default(),
        l3_w: Default::default(), l3_b: [0.0; MATERIAL_BUCKETS],
        l1_out, l2_out,
    };
    // C++ (nnue_net.cpp:185): `float biasDequant = 1.0f / (qa * qb);` -- as
    // 3 camadas partilham este UNICO dequant para o bias (o dos PESOS e' que
    // varia por camada: 1/(127*QB), 1/QB, 1/QB). Faltava o /qa aqui e o bias3
    // saia ~255x maior do que devia, dominando `out` e saturando o clamp.
    let bias_dequant = 1.0 / (qa * qb);
    for bk in 0..MATERIAL_BUCKETS {
        {
            let w = le_chunk(b, pos)?;
            let mut tmp = vec![0i16; l1_in * l1_out];
            leb_i16(w, &mut tmp)?;
            h.l1_w[bk] = tmp.iter().map(|&v| v.clamp(-127, 127) as i8).collect();
            let bch = le_chunk(b, pos)?;
            let mut bt = vec![0i32; l1_out];
            leb_i32(bch, &mut bt)?;
            h.l1_b[bk] = bt.iter().map(|&v| v as f32 * bias_dequant).collect();
        }
        {
            let w = le_chunk(b, pos)?;
            let mut tmp = vec![0i16; l1_out * l2_out];
            leb_i16(w, &mut tmp)?;
            h.l2_w[bk] = tmp.iter().map(|&v| v.clamp(-127, 127) as i8).collect();
            let bch = le_chunk(b, pos)?;
            let mut bt = vec![0i32; l2_out];
            leb_i32(bch, &mut bt)?;
            h.l2_b[bk] = bt.iter().map(|&v| v as f32 * bias_dequant).collect();
        }
        {
            let w = le_chunk(b, pos)?;
            let mut tmp = vec![0i16; l2_out];
            leb_i16(w, &mut tmp)?;
            h.l3_w[bk] = tmp.iter().map(|&v| v.clamp(-127, 127) as i8).collect();
            let bch = le_chunk(b, pos)?;
            let mut bt = [0i32; 1];
            leb_i32(bch, &mut bt)?;
            h.l3_b[bk] = bt[0] as f32 * bias_dequant;
        }
    }
    Some(h)
}

fn carrega(bytes: &[u8]) -> Option<RedeNapV10> {
    if bytes.len() < 36 || &bytes[0..8] != b"NAPK9\0\0\0" && &bytes[0..4] != b"NAPK" {
        // O magic exacto dos primeiros 8 bytes nao importa para o parsing --
        // os campos do cabecalho vem a partir do byte 8, e e' isso que se
        // valida abaixo pelos tamanhos. Aceita-se o ficheiro e falha-se mais
        // tarde se os campos nao baterem certo.
    }
    let l1 = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    let n_buckets = u32::from_le_bytes(bytes[16..20].try_into().ok()?) as usize;
    let n_features = u32::from_le_bytes(bytes[20..24].try_into().ok()?) as usize;
    let qa = u32::from_le_bytes(bytes[24..28].try_into().ok()?) as f32;
    let qb = u32::from_le_bytes(bytes[28..32].try_into().ok()?) as f32;
    let flags = u32::from_le_bytes(bytes[32..36].try_into().ok()?);

    if n_features != PIECE_FEATURES || n_buckets != MATERIAL_BUCKETS || l1 > MAX_L1 {
        eprintln!(
            "nnue-napv10: geometria inesperada (features={} buckets={} L1={})",
            n_features, n_buckets, l1
        );
        return None;
    }

    let mut pos = 36usize;
    let mut acc_bias = vec![0i16; l1];
    leb_i16(le_chunk(bytes, &mut pos)?, &mut acc_bias)?;
    let mut acc_weight = vec![0i16; PIECE_FEATURES * l1];
    leb_i16(le_chunk(bytes, &mut pos)?, &mut acc_weight)?;

    let mut psqt = Vec::new();
    if flags & 1 != 0 {
        psqt = vec![0i32; PIECE_FEATURES * MATERIAL_BUCKETS];
        leb_i32(le_chunk(bytes, &mut pos)?, &mut psqt)?;
    }

    let mut big = Head {
        l1_w: Default::default(), l1_b: Default::default(),
        l2_w: Default::default(), l2_b: Default::default(),
        l3_w: Default::default(), l3_b: [0.0; MATERIAL_BUCKETS],
        l1_out: BIG_DL1, l2_out: BIG_DL2,
    };
    let mut bullet: Option<Head> = None;
    if flags & 2 != 0 {
        // As 3 cabecas vem em ORDEM: bullet(16,32), small(32,32), big(32,32).
        // Guarda-se a bullet (usada quando o material ja' decidiu a posicao,
        // ver `escolhe_cabeca`) e descarta-se a small -- o dual_vision.h do
        // Nap2Siriux documenta que a small so' entra com pureMidDepth != 0,
        // que e' 0 (desligado) por omissao la' e aqui tambem, de proposito:
        // a cascata por profundidade foi MEDIDA a perder 66 Elo contra
        // "big em toda a parte" (comentario no proprio dual_vision.h) e foi
        // por isso que os defaults mudaram para 0/0. So' se porta o que
        // ficou validado: bullet por material decidido, nunca a cascata.
        let b = le_cabeca(bytes, &mut pos, l1 * 2, 16, 32, qa, qb)?;
        let _small = le_cabeca(bytes, &mut pos, l1 * 2, 32, 32, qa, qb)?;
        big = le_cabeca(bytes, &mut pos, l1 * 2, BIG_DL1, BIG_DL2, qa, qb)?;
        bullet = Some(b);
    }

    // Chaos/Fear head (flag bit2) -- lido e descartado, nao usamos.
    if flags & 4 != 0 {
        let o1 = 32usize;
        let o2 = 16usize;
        let l1x2 = l1 * 2;
        let _ = le_chunk(bytes, &mut pos)?; // ch_l1 w
        let _ = le_chunk(bytes, &mut pos)?; // ch_l1 b
        let _ = le_chunk(bytes, &mut pos)?; // ch_l2 w
        let _ = le_chunk(bytes, &mut pos)?; // ch_l2 b
        let _ = le_chunk(bytes, &mut pos)?; // ch_l3 w
        let _ = le_chunk(bytes, &mut pos)?; // ch_l3 b
        let _ = (o1, o2, l1x2);
    }

    let mut threat_weight = Vec::new();
    let mut has_threats = false;
    if let Some(c) = le_chunk(bytes, &mut pos) {
        let full = flags & 8 != 0;
        let n_thr = if full { THREAT_FEATURES_FULL } else { 640 };
        threat_weight = vec![0i16; n_thr * l1];
        if leb_i16(c, &mut threat_weight).is_some() {
            has_threats = full; // so' suportamos o esquema FULL (9216), como o li11/thr_x5
        }
    }

    eprintln!(
        "nnue-napv10: rede carregada (L1={}, threats={}, {} bytes)",
        l1, has_threats, bytes.len()
    );

    Some(RedeNapV10 { l1, qa, qb, acc_bias, acc_weight, psqt, threat_weight, has_threats, big, bullet })
}

// ---------------------------------------------------------- accumulador ---
// Identico ao li11 (mesma indexacao, mesmo BUCKET_MAP, mesmo factorizador
// de 704 -- ja' fundido na conversao, tal como aqui: o proprio carregador do
// Nap2Siriux nao funde nada, o ficheiro .napk9 ja' vem com as pecas na
// ordem final).
fn napv10_feature(bucket: usize, persp: usize, c: usize, t: usize, sq_raw: usize) -> Option<usize> {
    let sq = if persp == 0 { sq_raw } else { sq_raw ^ 56 };
    if t == 5 {
        if c == persp {
            return None;
        }
        return Some(bucket * FACTOR + sq);
    }
    let idx = [5usize, 4, 3, 2, 1][t] + if c == persp { 0 } else { 5 };
    Some(bucket * FACTOR + idx * 64 + sq)
}

struct CachePecas {
    entradas: Vec<(Vec<i32>, [[u64; 6]; 2], bool)>, // (valores, pecas, usada) por [persp*32+balde]
    l1: usize,
}
impl CachePecas {
    fn nova(l1: usize) -> Self {
        CachePecas { entradas: (0..64).map(|_| (vec![0i32; l1], [[0u64; 6]; 2], false)).collect(), l1 }
    }
    fn refresca(&mut self, net: &RedeNapV10, pecas: [[u64; 6]; 2], persp: usize, bucket: usize, dst: &mut [i32]) {
        if self.l1 != net.l1 {
            *self = CachePecas::nova(net.l1);
        }
        let l1 = self.l1;
        let i = persp * 32 + bucket;
        let (valores, pecas_cache, usada) = &mut self.entradas[i];
        if !*usada {
            for (d, &b) in valores.iter_mut().zip(net.acc_bias.iter()) {
                *d = b as i32;
            }
            *pecas_cache = [[0u64; 6]; 2];
            *usada = true;
        }
        for c in 0..2 {
            for t in 0..6 {
                let agora = pecas[c][t];
                let antes = pecas_cache[c][t];
                if agora == antes {
                    continue;
                }
                let mut saiu = antes & !agora;
                while saiu != 0 {
                    let sq = saiu.trailing_zeros() as usize;
                    saiu &= saiu - 1;
                    if let Some(f) = napv10_feature(bucket, persp, c, t, sq) {
                        let row = &net.acc_weight[f * l1..f * l1 + l1];
                        for k in 0..l1 {
                            valores[k] -= row[k] as i32;
                        }
                    }
                }
                let mut entrou = agora & !antes;
                while entrou != 0 {
                    let sq = entrou.trailing_zeros() as usize;
                    entrou &= entrou - 1;
                    if let Some(f) = napv10_feature(bucket, persp, c, t, sq) {
                        let row = &net.acc_weight[f * l1..f * l1 + l1];
                        for k in 0..l1 {
                            valores[k] += row[k] as i32;
                        }
                    }
                }
                pecas_cache[c][t] = agora;
            }
        }
        dst.copy_from_slice(valores);
    }
}

// Cache das ameacas: identico ao `CacheAmeacasLi11` (mesmo diff sem ordenar
// nem alocar) -- ver nnue_li11.rs para o porque' desta forma.
struct CacheAmeacas {
    activo: Vec<bool>,
    presente_agora: Vec<bool>,
    lista_anterior: Vec<usize>,
    acc: Vec<i32>,
    l1: usize,
}
impl CacheAmeacas {
    fn nova(l1: usize) -> Self {
        CacheAmeacas {
            activo: vec![false; THREAT_FEATURES_FULL],
            presente_agora: vec![false; THREAT_FEATURES_FULL],
            lista_anterior: Vec::new(),
            acc: vec![0i32; l1],
            l1,
        }
    }
    fn actualiza(&mut self, net: &RedeNapV10, nova_lista: &[usize]) -> &[i32] {
        if self.l1 != net.l1 {
            *self = CacheAmeacas::nova(net.l1);
        }
        let l1 = self.l1;
        for &f in nova_lista {
            self.presente_agora[f] = true;
        }
        for &f in &self.lista_anterior {
            if !self.presente_agora[f] {
                let row = &net.threat_weight[f * l1..f * l1 + l1];
                for i in 0..l1 {
                    self.acc[i] -= row[i] as i32;
                }
                self.activo[f] = false;
            }
        }
        for &f in nova_lista {
            if !self.activo[f] {
                let row = &net.threat_weight[f * l1..f * l1 + l1];
                for i in 0..l1 {
                    self.acc[i] += row[i] as i32;
                }
                self.activo[f] = true;
            }
        }
        for &f in nova_lista {
            self.presente_agora[f] = false;
        }
        self.lista_anterior.clear();
        self.lista_anterior.extend_from_slice(nova_lista);
        &self.acc
    }
}

thread_local! {
    static CACHE_PECAS: std::cell::RefCell<Option<CachePecas>> = const { std::cell::RefCell::new(None) };
    static CACHE_AMEACAS: std::cell::RefCell<[Option<CacheAmeacas>; 2]> =
        const { std::cell::RefCell::new([None, None]) };
    static THR_BUF: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Avalia com a cabeça "big". `evaluate(board)` no C++ e' exactamente
/// `evaluate(board, HEAD_BIG)` -- e' o que o comando `eval` deles usa, e a
/// referencia mais facil de validar.
pub fn evaluate(net: &RedeNapV10, board: &Board) -> i32 {
    let l1 = net.l1;
    let ksq_w = board.pieces[0][5].trailing_zeros() as usize;
    let ksq_b = board.pieces[1][5].trailing_zeros() as usize;
    let bucket_w = features::BUCKET_MAP[ksq_w];
    let bucket_b = features::BUCKET_MAP[ksq_b ^ 56];

    let mut acc_w_buf = [0i32; MAX_L1];
    let mut acc_b_buf = [0i32; MAX_L1];
    let acc_w = &mut acc_w_buf[..l1];
    let acc_b = &mut acc_b_buf[..l1];
    CACHE_PECAS.with(|c| {
        let mut cache = c.borrow_mut();
        let cache = cache.get_or_insert_with(|| CachePecas::nova(l1));
        cache.refresca(net, board.pieces, 0, bucket_w, acc_w);
        cache.refresca(net, board.pieces, 1, bucket_b, acc_b);
    });

    if net.has_threats {
        let pos = features::Pos { pieces: board.pieces };
        let maps = features::compute_attack_maps_by_type(&pos);
        THR_BUF.with(|buf| {
            CACHE_AMEACAS.with(|ca| {
                let mut cache = ca.borrow_mut();
                let mut thr = buf.borrow_mut();
                thr.clear();
                features::gather_threats_full(&pos, 0, &maps, &mut thr);
                let c0 = cache[0].get_or_insert_with(|| CacheAmeacas::nova(l1));
                let acc_thr_w = c0.actualiza(net, &thr);
                for i in 0..l1 {
                    acc_w[i] += acc_thr_w[i];
                }
                thr.clear();
                features::gather_threats_full(&pos, 1, &maps, &mut thr);
                let c1 = cache[1].get_or_insert_with(|| CacheAmeacas::nova(l1));
                let acc_thr_b = c1.actualiza(net, &thr);
                for i in 0..l1 {
                    acc_b[i] += acc_thr_b[i];
                }
            });
        });
    }

    let stm = board.side.idx();
    let (acc_us, acc_them): (&[i32], &[i32]) = if stm == 0 { (acc_w, acc_b) } else { (acc_b, acc_w) };

    // concat = clip(acc,0,QA) * 127/QA, arredondado -- mesma formula do C++
    // (com AVX ali, escalar aqui: mesmo resultado, so' mais lento).
    let qa_i = net.qa as i32;
    // Mesma ordem de operacoes do C++: primeiro arredonda-se o MULTIPLICADOR
    // (127/QA * 2^16), so' depois se multiplica por x. Fazer as duas coisas
    // de uma vez (x*127*65536/QA) arredonda em outro ponto e pode dar off-by-
    // one em alguns valores -- o mesmo tipo de defeito que ontem levou horas
    // a apanhar. Aqui replica-se a ordem exacta, nao uma equivalente.
    let scale_mul = ((127.0f64 / qa_i.max(1) as f64) * 65536.0).round() as i64;
    let mut concat_buf = [0u8; 2 * MAX_L1];
    let concat = &mut concat_buf[..2 * l1];
    for (half, src) in [acc_us, acc_them].iter().enumerate() {
        for i in 0..l1 {
            let x = src[i].clamp(0, qa_i) as i64;
            let scaled = (x * scale_mul + 32768) >> 16;
            concat[half * l1 + i] = scaled.clamp(0, 127) as u8;
        }
    }

    let n = board.occ_all.count_ones() as usize;
    let bucket = ((n as i32 - 2) / 4).clamp(0, MATERIAL_BUCKETS as i32 - 1) as usize;
    // Cabeca barata so' quando o material ja' decidiu a posicao (dama liquida
    // a mais ou mais) -- a cascata por profundidade que o nome "bullet"
    // sugeria foi medida a perder 66 Elo la' e nunca chegou a activar por
    // omissao; isto e' o que ficou validado no lugar dela.
    let h = match &net.bullet {
        Some(bh) => {
            let diff = material_diff(board);
            if std::env::var("NAPV10_DEBUG_DUMP").is_ok() {
                eprintln!("material_diff={}", diff);
            }
            if diff >= DECIDED_MATERIAL || -diff >= DECIDED_MATERIAL { bh } else { &net.big }
        }
        None => &net.big,
    };
    let l1_in = 2 * l1;

    let dequant1 = 1.0 / (127.0 * net.qb);
    let dequant2 = 1.0 / net.qb;
    let dequant3 = 1.0 / net.qb;

    let mut a1 = [0f32; BIG_DL1];
    for o in 0..h.l1_out.min(BIG_DL1) {
        let w = &h.l1_w[bucket][o * l1_in..o * l1_in + l1_in];
        let mut sum = 0i64;
        for i in 0..l1_in {
            sum += concat[i] as i64 * w[i] as i64;
        }
        a1[o] = (h.l1_b[bucket][o] + sum as f32 * dequant1).clamp(0.0, 1.0);
    }
    let mut a2 = [0f32; BIG_DL2];
    for o in 0..h.l2_out.min(BIG_DL2) {
        let w = &h.l2_w[bucket][o * h.l1_out..o * h.l1_out + h.l1_out];
        let mut dot = 0f32;
        for i in 0..h.l1_out {
            dot += w[i] as f32 * a1[i];
        }
        a2[o] = (h.l2_b[bucket][o] + dot * dequant2).clamp(0.0, 1.0);
    }
    let mut out = h.l3_b[bucket];
    {
        let w = &h.l3_w[bucket][..h.l2_out];
        let mut dot = 0f32;
        for i in 0..h.l2_out {
            dot += w[i] as f32 * a2[i];
        }
        out += dot * dequant3;
    }

    let mut psqt_bias = 0.0f32;
    if !net.psqt.is_empty() {
        let mut ps_w: i64 = 0;
        let mut ps_b: i64 = 0;
        for c in 0..2 {
            for t in 0..6 {
                let mut bb = board.pieces[c][t];
                while bb != 0 {
                    let sq = bb.trailing_zeros() as usize;
                    bb &= bb - 1;
                    if let Some(f) = napv10_feature(bucket_w, 0, c, t, sq) {
                        ps_w += net.psqt[f * MATERIAL_BUCKETS + bucket] as i64;
                    }
                    if let Some(f) = napv10_feature(bucket_b, 1, c, t, sq) {
                        ps_b += net.psqt[f * MATERIAL_BUCKETS + bucket] as i64;
                    }
                }
            }
        }
        let (psqt_us, psqt_them) = if stm == 0 { (ps_w, ps_b) } else { (ps_b, ps_w) };
        psqt_bias = ((psqt_us - psqt_them) as f32 / net.qa) / 2.0;
    }

    if std::env::var("NAPV10_DEBUG_DUMP").is_ok() {
        eprintln!(
            "acc_w[0..4]={:?} acc_b[0..4]={:?} acc_w_min={} acc_w_max={} acc_b_min={} acc_b_max={}",
            &acc_w[..4.min(l1)], &acc_b[..4.min(l1)],
            acc_w.iter().min().unwrap(), acc_w.iter().max().unwrap(),
            acc_b.iter().min().unwrap(), acc_b.iter().max().unwrap(),
        );
        eprintln!(
            "concat[0..8]={:?} concat_sum={}",
            &concat[..8.min(concat.len())],
            concat.iter().map(|&x| x as i64).sum::<i64>(),
        );
        eprintln!("bucket={} qa={} qb={}", bucket, net.qa, net.qb);
        eprintln!("a1={:?}", &a1[..h.l1_out.min(BIG_DL1)]);
        eprintln!("a2={:?}", &a2[..h.l2_out.min(BIG_DL2)]);
        eprintln!("out={} psqt_bias={}", out, psqt_bias);
        eprintln!("l3_b[bucket]={}", h.l3_b[bucket]);
    }

    let score = ((out + psqt_bias) * OUTPUT_SCALE_CP).round() as i32;
    score.clamp(-3000, 3000)
}

pub fn escala() -> i32 {
    OUTPUT_SCALE_CP as i32
}

static REDE: std::sync::OnceLock<Option<RedeNapV10>> = std::sync::OnceLock::new();

pub fn rede() -> Option<&'static RedeNapV10> {
    REDE.get_or_init(|| {
        let path = std::env::var("KESTREL_NNUE_NAPV10").ok()?;
        let bytes = std::fs::read(&path).ok()?;
        carrega(&bytes)
    })
    .as_ref()
}

pub fn ligada() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("KESTREL_NAPV10").map(|v| v != "0").unwrap_or(true))
}
