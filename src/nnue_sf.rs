//! Reader for the official Stockfish-master NNUE network format
//! (HalfKAv2_hm + Full_Threats + PP_3Wide, SFNNv13-class: L1=1024,
//! L2=32, L3=32, 8 output buckets/LayerStacks, double activation with a
//! raw-difference skip term). Reads the actual GPL-3.0 released .nnue
//! file directly -- format understood by reading Stockfish's own source
//! (GPL-3.0, no copyleft issue for running/adapting), values are that
//! network's own trained weights, used as-is on the project owner's
//! explicit instruction, same footing as the PlentyChess and Triumviratus
//! reference imports this session.
//!
//! Gated behind `KESTREL_NNUE_SF`; unset, this module does nothing.

use crate::attacks::Attacks;
use crate::board::Board;
use crate::types::{Color, PieceType, Square};
use std::sync::OnceLock;

const PIECE_DIM: usize = 22528; // HalfKAv2_hm
const THREAT_DIM: usize = 59808; // Full_Threats
const PAIR_DIM: usize = 4560; // PP_3Wide (96*95/2)
const PAIR_BASE: usize = THREAT_DIM; // pawn-pair indices offset AFTER threats
const INPUT_DIM: usize = PIECE_DIM + THREAT_DIM + PAIR_DIM;

const L1: usize = 1024;
const L2: usize = 32; // FC_0_OUTPUTS
const L3: usize = 32; // FC_1_OUTPUTS
const NB: usize = 8; // LayerStacks / PSQTBuckets

const WEIGHT_SCALE_BITS: i32 = 6;
const HIDDEN_ONE_VAL: i64 = 128;
const OUTPUT_SCALE: i64 = 16;
const FT_SHIFT: u32 = 6; // matches SF's WeightScaleBits used for the FT clamp (0..127 post-shift)
const FT_MAX_VAL: i32 = 255; // FtMaxVal in nnue_common.h

#[rustfmt::skip]
const KING_BUCKETS_BASE: [i32; 64] = [
    28,29,30,31, 31,30,29,28,
    24,25,26,27, 27,26,25,24,
    20,21,22,23, 23,22,21,20,
    16,17,18,19, 19,18,17,16,
    12,13,14,15, 15,14,13,12,
     8, 9,10,11, 11,10, 9, 8,
     4, 5, 6, 7,  7, 6, 5, 4,
     0, 1, 2, 3,  3, 2, 1, 0,
];

#[rustfmt::skip]
const ORIENT_HALFKA: [i32; 64] = {
    // files a-d (0-3) -> flip by 7 (SQ_H1) so the king ends on e-h;
    // files e-h (4-7) -> no flip (SQ_A1 = 0). Row-major, rank0=rank1.
    let h = 7; let a = 0;
    [
    h,h,h,h, a,a,a,a,
    h,h,h,h, a,a,a,a,
    h,h,h,h, a,a,a,a,
    h,h,h,h, a,a,a,a,
    h,h,h,h, a,a,a,a,
    h,h,h,h, a,a,a,a,
    h,h,h,h, a,a,a,a,
    h,h,h,h, a,a,a,a,
    ]
};

#[rustfmt::skip]
const ORIENT_THREATS: [i32; 64] = {
    // Full_Threats/PP_3Wide's own OrientTBL: opposite sense from HalfKAv2_hm
    // (files a-d -> SQ_A1=0, e-h -> SQ_H1=7) -- kept as a separate table on
    // purpose, conflating the two cost real time earlier today (PlentyChess).
    let h = 7; let a = 0;
    [
    a,a,a,a, h,h,h,h,
    a,a,a,a, h,h,h,h,
    a,a,a,a, h,h,h,h,
    a,a,a,a, h,h,h,h,
    a,a,a,a, h,h,h,h,
    a,a,a,a, h,h,h,h,
    a,a,a,a, h,h,h,h,
    a,a,a,a, h,h,h,h,
    ]
};

// PieceSquareIndex[relative_color][piece_type] * 64 -- own pieces first
// (planes 0..5 excluding king), then enemy (planes 5..10 excluding king),
// king (own or enemy, both map to plane 10) never actually queried for the
// side's own king in practice.
#[inline]
fn piece_plane(piece: PieceType, relative_enemy: bool) -> i32 {
    let base = match piece {
        PieceType::Pawn => 0,
        PieceType::Knight => 2,
        PieceType::Bishop => 4,
        PieceType::Rook => 6,
        PieceType::Queen => 8,
        PieceType::King => 10,
    };
    if piece == PieceType::King {
        10
    } else if relative_enemy {
        base + 1
    } else {
        base
    }
}

#[rustfmt::skip]
const PIECE_INTERACTION_MAP: [[i32; 6]; 6] = [
    [-1, 0, -1,  1, -1, -1],
    [ 0, 1,  2,  3,  4, -1],
    [ 0, 1,  2,  3, -1, -1],
    [ 0, 1,  2,  3, -1, -1],
    [ 0, 1,  2,  3,  4, -1],
    [-1,-1, -1, -1, -1, -1],
];
const PIECE_TARGET_COUNT: [i32; 6] = [4, 10, 8, 8, 10, 0];

const PIECE_TYPES: [PieceType; 6] = [
    PieceType::Pawn,
    PieceType::Knight,
    PieceType::Bishop,
    PieceType::Rook,
    PieceType::Queen,
    PieceType::King,
];

fn pseudo_attacks_empty(atk: &Attacks, piece: usize, origin: usize, color: usize) -> u64 {
    let sq = origin as Square;
    match piece {
        0 => {
            if (8..56).contains(&origin) {
                atk.pawn[color][origin]
            } else {
                0
            }
        }
        1 => atk.knight[origin],
        5 => atk.king[origin],
        2 => crate::attacks::bishop_attacks(sq, 0),
        3 => crate::attacks::rook_attacks(sq, 0),
        _ => crate::attacks::bishop_attacks(sq, 0) | crate::attacks::rook_attacks(sq, 0),
    }
}

struct ThreatTables {
    piece_offset: Vec<Vec<Vec<i32>>>,
    attack_index: Vec<Vec<Vec<Vec<i32>>>>,
    pair_lookup: Vec<Vec<Vec<Vec<(bool, bool, i32)>>>>,
}

fn build_threat_tables() -> ThreatTables {
    let atk = Attacks::new();
    let mut piece_offset = vec![vec![vec![0i32; 64]; 2]; 6];
    let mut cumulative_piece_offset = [[0i32; 2]; 6];
    let mut cumulative_offset = [[0i32; 2]; 6];
    let mut running = 0i32;
    for color in 0..2 {
        for piece in 0..6 {
            let mut cum = 0i32;
            for origin in 0..64 {
                piece_offset[piece][color][origin] = cum;
                if piece != 0 || (8..56).contains(&origin) {
                    let a = pseudo_attacks_empty(&atk, piece, origin, color);
                    cum += a.count_ones() as i32;
                }
            }
            cumulative_piece_offset[piece][color] = cum;
            cumulative_offset[piece][color] = running;
            running += PIECE_TARGET_COUNT[piece] * cum;
        }
    }
    debug_assert_eq!(running as usize, THREAT_DIM);

    let mut attack_index = vec![vec![vec![vec![0i32; 64]; 64]; 2]; 6];
    for color in 0..2 {
        for piece in 0..6 {
            for origin in 0..64 {
                let a = pseudo_attacks_empty(&atk, piece, origin, color);
                let mut m = a;
                while m != 0 {
                    let target = m.trailing_zeros() as usize;
                    m &= m - 1;
                    let below = a & ((1u64 << target) - 1);
                    attack_index[piece][color][origin][target] = below.count_ones() as i32;
                }
            }
        }
    }

    let mut pair_lookup = vec![vec![vec![vec![(true, false, 0i32); 2]; 6]; 2]; 6];
    for ap in 0..6 {
        for ac in 0..2 {
            for tp in 0..6 {
                for tc in 0..2 {
                    let map = PIECE_INTERACTION_MAP[ap][tp];
                    let mut feature_base = cumulative_offset[ap][ac]
                        + (tc as i32 * (PIECE_TARGET_COUNT[ap] / 2) + map)
                            * cumulative_piece_offset[ap][ac];
                    let enemy = ac != tc;
                    let semi_excluded = ap == tp && (enemy || ap != 0);
                    let excluded = map < 0;
                    if excluded {
                        feature_base = 0;
                    }
                    pair_lookup[ap][ac][tp][tc] = (excluded, semi_excluded, feature_base);
                }
            }
        }
    }
    ThreatTables { piece_offset, attack_index, pair_lookup }
}

static THREAT_TABLES: OnceLock<ThreatTables> = OnceLock::new();
fn threat_tables() -> &'static ThreatTables {
    THREAT_TABLES.get_or_init(build_threat_tables)
}

#[inline]
fn is_pair_excluded(excluded: bool, semi_excluded: bool, attacking_sq: i32, attacked_sq: i32) -> bool {
    let less_than = if attacking_sq < attacked_sq { 1u8 } else { 0 };
    let data = ((semi_excluded && !excluded) as u8) | ((excluded as u8) << 1);
    ((data.wrapping_add(less_than)) & 2) != 0
}

fn get_threat_feature(
    pov: usize,
    attacking_piece: usize,
    attacking_color: usize,
    attacked_piece: usize,
    attacked_color: usize,
    attacking_square: i32,
    attacked_square: i32,
    mirrored: bool,
) -> i32 {
    let square_flip = (if mirrored { 7 } else { 0 }) ^ (if pov == 1 { 56 } else { 0 });
    let a_sq = attacking_square ^ square_flip;
    let d_sq = attacked_square ^ square_flip;
    let a_c = attacking_color ^ pov;
    let d_c = attacked_color ^ pov;
    let t = threat_tables();
    let (excluded, semi_excluded, base) = t.pair_lookup[attacking_piece][a_c][attacked_piece][d_c];
    if is_pair_excluded(excluded, semi_excluded, a_sq, d_sq) {
        return THREAT_DIM as i32;
    }
    base + t.piece_offset[attacking_piece][a_c][a_sq as usize]
        + t.attack_index[attacking_piece][a_c][a_sq as usize][d_sq as usize]
}

fn pp_mask(sq: usize) -> u64 {
    const FILE_A: u64 = 0x0101010101010101;
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

#[inline]
fn pawn_pair_index(id_a: i32, id_b: i32) -> usize {
    let (lo, hi) = if id_a < id_b { (id_a, id_b) } else { (id_b, id_a) };
    (hi * (hi - 1) / 2 + lo) as usize
}

#[inline]
fn pawn_id(square: usize, color_offset: i32, square_flip: usize) -> i32 {
    color_offset + (square ^ square_flip) as i32 - 8
}

// ---- LEB128 (Stockfish's own: standard signed LEB128, sign-extended from
// the last byte's bit 6 -- NOT the zigzag scheme our own .li11 format uses,
// despite sharing the "COMPRESSED_LEB128" magic string). ----
const LEB_MAGIC: &[u8] = b"COMPRESSED_LEB128";

fn leb_decode_one(bytes: &[u8], pos: &mut usize, bits: u32) -> i64 {
    let mut result: i64 = 0;
    let mut shift: u32 = 0;
    loop {
        let byte = bytes[*pos];
        *pos += 1;
        result |= ((byte & 0x7f) as i64) << (shift % 32);
        shift += 7;
        if byte & 0x80 == 0 {
            if shift < bits && (byte & 0x40) != 0 {
                result |= -1i64 << shift;
            }
            break;
        }
    }
    result
}

fn read_leb_i16(bytes: &[u8], off: &mut usize, n: usize) -> Vec<i16> {
    // consume magic + u32 byte count (informational, not re-checked here)
    *off += LEB_MAGIC.len();
    let _byte_count = u32::from_le_bytes(bytes[*off..*off + 4].try_into().unwrap());
    *off += 4;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(leb_decode_one(bytes, off, 16) as i16);
    }
    v
}

fn read_leb_i32(bytes: &[u8], off: &mut usize, n: usize) -> Vec<i32> {
    *off += LEB_MAGIC.len();
    let _byte_count = u32::from_le_bytes(bytes[*off..*off + 4].try_into().unwrap());
    *off += 4;
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        v.push(leb_decode_one(bytes, off, 32) as i32);
    }
    v
}

fn read_le_i16(bytes: &[u8], off: &mut usize, n: usize) -> Vec<i16> {
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        v.push(i16::from_le_bytes([bytes[*off + i * 2], bytes[*off + i * 2 + 1]]));
    }
    *off += n * 2;
    v
}

fn read_le_i8(bytes: &[u8], off: &mut usize, n: usize) -> Vec<i8> {
    let v: Vec<i8> = bytes[*off..*off + n].iter().map(|&b| b as i8).collect();
    *off += n;
    v
}

struct LayerStack {
    // fc_0: (2*L1) -> L2, dense. Layer-local WeightType is i8 here, NOT the
    // feature transformer's own (global) i16 WeightType -- two classes,
    // two independent typedefs, found the hard way by an offset that ran
    // out of file about halfway through the 8 layer stacks.
    fc0w: Vec<i8>,
    fc0b: Vec<i32>,
    // fc_1: (2*L2) -> L3, dense
    fc1w: Vec<i8>,
    fc1b: Vec<i32>,
    // fc_2: (2*L2 + 2*L3) -> 1, dense
    fc2w: Vec<i8>,
    fc2b: i32,
}

fn read_dense_layer(bytes: &[u8], off: &mut usize, in_dim: usize, out_dim: usize) -> (Vec<i8>, Vec<i32>) {
    let bias = read_le_i32_plain(bytes, off, out_dim);
    let w = read_le_i8(bytes, off, in_dim * out_dim);
    (w, bias)
}

fn read_le_i32_plain(bytes: &[u8], off: &mut usize, n: usize) -> Vec<i32> {
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        v.push(i32::from_le_bytes(bytes[*off + i * 4..*off + i * 4 + 4].try_into().unwrap()));
    }
    *off += n * 4;
    v
}

pub struct RedeSf {
    ft_bias: Vec<i16>,          // [L1]
    ft_threat_w: Vec<i8>,       // [THREAT_DIM * L1]
    ft_pair_w: Vec<i8>,         // [PAIR_DIM * L1]
    ft_piece_w: Vec<i16>,       // [PIECE_DIM * L1]
    ft_piece_psqt: Vec<i32>,    // [PIECE_DIM * NB]
    ft_threat_psqt: Vec<i32>,   // [THREAT_DIM * NB]
    ft_pair_psqt: Vec<i32>,     // [PAIR_DIM * NB]
    stacks: Vec<LayerStack>,    // [NB]
    // Header fields, kept verbatim so a net can be written back out in SF's
    // own format. The two u32 hashes are computed by SF from the architecture
    // and verified on load -- a net we serialise must carry the same ones or
    // Stockfish refuses it.
    version: u32,
    hash: u32,
    desc: Vec<u8>,
    ft_header: [u8; 4],
    stack_headers: Vec<[u8; 4]>,
}

fn carrega(bytes: &[u8]) -> Option<RedeSf> {
    let mut o = 0usize;
    // header: version(4) hash(4) size(4) desc(size)
    let version = u32::from_le_bytes(bytes[o..o + 4].try_into().ok()?);
    o += 4;
    let hash = u32::from_le_bytes(bytes[o..o + 4].try_into().ok()?);
    o += 4;
    let size = u32::from_le_bytes(bytes[o..o + 4].try_into().ok()?) as usize;
    o += 4;
    let desc = bytes[o..o + size].to_vec();
    o += size;

    let ft_header: [u8; 4] = bytes[o..o + 4].try_into().ok()?;
    o += 4; // feature transformer's own inner header (hash check, unused here)
    let dbg = std::env::var_os("KESTREL_SF_DEBUG").is_some();

    // Order confirmed from nnue_feature_transformer.h::read_parameters:
    let mut ft_bias = read_leb_i16(bytes, &mut o, L1);
    if dbg { eprintln!("DBG after ft_bias: o={o} bias[0..4]={:?}", &ft_bias[..4]); }
    let mut ft_threat_w = read_le_i8(bytes, &mut o, THREAT_DIM * L1);
    if dbg { eprintln!("DBG after threat_w: o={o}"); }
    let ft_threat_psqt = read_leb_i32(bytes, &mut o, THREAT_DIM * NB);
    if dbg { eprintln!("DBG after threat_psqt: o={o}"); }
    let mut ft_pair_w = read_le_i8(bytes, &mut o, PAIR_DIM * L1);
    if dbg { eprintln!("DBG after pair_w: o={o}"); }
    let ft_pair_psqt = read_leb_i32(bytes, &mut o, PAIR_DIM * NB);
    if dbg { eprintln!("DBG after pair_psqt: o={o}"); }
    let mut ft_piece_w = read_leb_i16(bytes, &mut o, PIECE_DIM * L1);
    if dbg { eprintln!("DBG after piece_w: o={o}"); }
    let ft_piece_psqt = read_leb_i32(bytes, &mut o, PIECE_DIM * NB);
    if dbg { eprintln!("DBG after piece_psqt: o={o} total={}", bytes.len()); }

    // SF's AVX2 build permutes biases/weights in RAM after loading
    // (permute_weights(), PackusEpi16Order) purely so its own packus-based
    // pairwise-activation SIMD trick reads them in the right lanes -- a
    // non-SIMD/scalar build applies no permutation at all and produces the
    // IDENTICAL x[]/fc0_out (verified directly against both builds). Since
    // this reader does the plain scalar computation, it must use the file's
    // raw (canonical) order throughout -- no permutation here.
    if dbg {
        eprintln!("DBG post-permute ft_bias[0..16]={:?}", &ft_bias[..16]);
        eprintln!("DBG post-permute w[king20408][0..16]={:?}", &ft_piece_w[20408 * L1..20408 * L1 + 16]);
        eprintln!("DBG post-permute w[knight19840][0..16]={:?}", &ft_piece_w[19840 * L1..19840 * L1 + 16]);
    }

    // `var_os(..).is_some()` dava-se por ligado com `=0`, porque a variavel
    // fica DEFINIDA. Uma grelha inteira de testes correu com ele ligado nas
    // dezasseis celulas e so' se percebeu porque as linhas que so' diferiam
    // nele davam numeros identicos. Mesma convencao das outras: `=0` desliga.
    let dense_t = match std::env::var("KESTREL_DENSE_T") { Ok(v) => v != "0", Err(_) => false };
    let mut stacks = Vec::with_capacity(NB);
    let mut stack_headers = Vec::with_capacity(NB);
    for bi in 0..NB {
        stack_headers.push(bytes[o..o + 4].try_into().ok()?);
        o += 4; // layer stack's own inner header
        let (fc0w, fc0b) = read_dense_layer(bytes, &mut o, L1, L2);
        let (fc1w, fc1b) = read_dense_layer(bytes, &mut o, 2 * L2, L3);
        let (fc2w, fc2b_vec) = read_dense_layer(bytes, &mut o, 2 * L2 + 2 * L3, 1);
        if dbg && bi == 0 {
            eprintln!("DBG fc0 biases[0..4]={:?}", &fc0b[..4]);
            eprintln!("DBG fc0 W(out=0,in=0..8)={:?}", &fc0w[0 * L1..0 * L1 + 8]);
            eprintln!("DBG fc0 W(out=1,in=0..8)={:?}", &fc0w[1 * L1..1 * L1 + 8]);
            let wsum0: i64 = fc0w[0..L1].iter().enumerate().map(|(i, &v)| (i as i64 + 1) * v as i64).sum();
            let wsum1: i64 = fc0w[L1..2 * L1].iter().enumerate().map(|(i, &v)| (i as i64 + 1) * v as i64).sum();
            eprintln!("DBG fc0 W(out=0) wsum={} W(out=1) wsum={}", wsum0, wsum1);
            eprintln!("DBG fc1 biases[0..4]={:?}", &fc1b[..4]);
            eprintln!("DBG fc1 W(out=0,in=0..8)={:?}", &fc1w[0 * (2 * L2)..0 * (2 * L2) + 8]);
            eprintln!("DBG fc1 W(out=1,in=0..8)={:?}", &fc1w[1 * (2 * L2)..1 * (2 * L2) + 8]);
            eprintln!("DBG fc2 bias={}", fc2b_vec[0]);
            eprintln!("DBG fc2 W(in=0..16)={:?}", &fc2w[0..16]);
        }
        stacks.push(LayerStack { fc0w, fc0b, fc1w, fc1b, fc2w, fc2b: fc2b_vec[0] });
    }

    eprintln!(
        "nnue-sf: rede oficial do Stockfish carregada ({} bytes, {} de sobra)",
        bytes.len(),
        bytes.len() as i64 - o as i64
    );
    Some(RedeSf {
        ft_bias, ft_threat_w, ft_pair_w, ft_piece_w, ft_piece_psqt, ft_threat_psqt, ft_pair_psqt,
        stacks, version, hash, desc, ft_header, stack_headers,
    })
}

/// Parse a Stockfish net from raw bytes (for tooling; the engine itself goes
/// through `rede()`).
pub fn carrega_pub(bytes: &[u8]) -> Option<RedeSf> {
    carrega(bytes)
}

// ---- Writing SF's own format ----
//
// Mirrors write_leb_128 in nnue_common.h exactly: canonical minimal signed
// LEB128 -- keep emitting 7-bit groups until the value is fully represented
// AND the last group's bit 6 carries the correct sign.
fn leb_encode_one(mut value: i64, out: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        let done = if byte & 0x40 == 0 { value == 0 } else { value == -1 };
        if done {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn write_leb_block<T: Copy + Into<i64>>(vals: &[T], out: &mut Vec<u8>) {
    out.extend_from_slice(LEB_MAGIC);
    let mut body = Vec::new();
    for &v in vals {
        leb_encode_one(v.into(), &mut body);
    }
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
}

fn write_dense_layer(w: &[i8], b: &[i32], out: &mut Vec<u8>) {
    for &v in b {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for &v in w {
        out.push(v as u8);
    }
}

/// Serialise back into Stockfish's own .nnue format, byte for byte.
/// Acesso ao mapeamento de features para o comando `dustbin`.
pub fn board_para_posbb_pub(b: &Board) -> crate::sf_features::PosBB { board_para_posbb(b) }
pub fn threats_pad_pub(p: &crate::sf_features::PosBB, pov: usize, v: &mut Vec<usize>) {
    crate::sf_features::threat_features_padded_com(p, pov, THREAT_DIM, v, MAGIC);
}
pub fn threat_dim_pub() -> usize { THREAT_DIM }

pub fn escreve(net: &RedeSf) -> Vec<u8> {
    let mut out = Vec::with_capacity(96 * 1024 * 1024);
    out.extend_from_slice(&net.version.to_le_bytes());
    out.extend_from_slice(&net.hash.to_le_bytes());
    out.extend_from_slice(&(net.desc.len() as u32).to_le_bytes());
    out.extend_from_slice(&net.desc);

    out.extend_from_slice(&net.ft_header);
    // Same field order as read_parameters().
    write_leb_block(&net.ft_bias, &mut out);
    for &v in &net.ft_threat_w {
        out.push(v as u8);
    }
    write_leb_block(&net.ft_threat_psqt, &mut out);
    for &v in &net.ft_pair_w {
        out.push(v as u8);
    }
    write_leb_block(&net.ft_pair_psqt, &mut out);
    write_leb_block(&net.ft_piece_w, &mut out);
    write_leb_block(&net.ft_piece_psqt, &mut out);

    for (i, st) in net.stacks.iter().enumerate() {
        out.extend_from_slice(&net.stack_headers[i]);
        write_dense_layer(&st.fc0w, &st.fc0b, &mut out);
        write_dense_layer(&st.fc1w, &st.fc1b, &mut out);
        write_dense_layer(&st.fc2w, &[st.fc2b], &mut out);
    }
    out
}

fn board_para_posbb(board: &Board) -> crate::sf_features::PosBB {
    let mut p = crate::sf_features::PosBB::default();
    for c in 0..2 {
        for t in 0..6 {
            p.pieces[c][t] = board.pieces[c][t];
        }
    }
    p
}

// The three feature families live in `sf_features` so the trainer can use the
// exact same mapping; these wrappers only adapt the engine's Board to it.
fn add_piece_features(board: &Board, pov: usize, feats: &mut Vec<(usize, i32)>) {
    let mut v = Vec::with_capacity(32);
    crate::sf_features::piece_features(&board_para_posbb(board), pov, &mut v);
    for idx in v {
        feats.push((idx, 0));
    }
}

/// The engine's magic bitboards, injected into the shared feature code. The
/// portable ray loops it falls back to were ~10% of search time.
const MAGIC: crate::sf_features::Deslizantes = crate::sf_features::Deslizantes {
    bispo: |sq, occ| crate::attacks::bishop_attacks(sq as crate::types::Square, occ),
    torre: |sq, occ| crate::attacks::rook_attacks(sq as crate::types::Square, occ),
};

fn add_threat_features(_atk: &Attacks, board: &Board, pov: usize, feats: &mut Vec<usize>) {
    crate::sf_features::threat_features_com(&board_para_posbb(board), pov, feats, MAGIC);
}

fn add_pair_features(board: &Board, pov: usize, feats: &mut Vec<usize>) {
    crate::sf_features::pair_features(&board_para_posbb(board), pov, feats);
}

// SF's SqrClippedReLU (scalar path): min(127, (x*x) >> (2*WeightScaleBitsLocal+7)),
// no pre-clamp before squaring.
#[inline]
fn clipped_sq(x: i32, weight_scale_bits_local: u32) -> i32 {
    let shift = 2 * weight_scale_bits_local + 7;
    let v = ((x as i64) * (x as i64)) >> shift;
    v.min(127) as i32
}
// SF's ClippedReLU (scalar path): clamp(x >> WeightScaleBitsLocal, 0, 127).
#[inline]
fn clipped_lin(x: i32, weight_scale_bits_local: u32) -> i32 {
    (x >> weight_scale_bits_local).clamp(0, 127)
}

pub fn evaluate(net: &RedeSf, atk: &Attacks, board: &mut Board) -> i32 {
    // `KESTREL_TROCA_POV=1` swaps the two perspectives in the forward pass.
    // A diagnostic, not an option, and it has already answered its question:
    // NO. Our trained nets score a queen up for White as WORSE than the same
    // position without it, which looks like a perspective swap between training
    // and inference. It is not. Swapping here merely exchanges the two results
    // with each other -- on our nets AND on the official one, which is the
    // symmetry you would expect -- so the forward pass reads `us` correctly.
    //
    // 2026-08-16, official net vs ours at superbatch 10 (start position, then
    // White a queen up, then Black a queen up):
    //     official   9   +2578   -1885      swapped:   9   -1885   +2578
    //     v3-sb10 -112    -206     -50      swapped: -112     -50    -206
    // The official net separates the two by ~4400; ours by ~150, in either
    // orientation. The net did not learn material at all -- the fault is
    // upstream of the forward pass, in training. Kept so the question is not
    // asked twice.
    let troca = {
        static V: OnceLock<bool> = OnceLock::new();
        *V.get_or_init(|| std::env::var_os("KESTREL_TROCA_POV").is_some())
    };
    let stm = if troca { 1 - board.side as usize } else { board.side as usize };
    let nstm = 1 - stm;

    let build_acc = |pov: usize| -> (Vec<i16>, Vec<(usize, i32)>, Vec<usize>, Vec<usize>) {
        // i16, not i32: the weights are i16/i8, and widening each one on the
        // way in stops the compiler vectorising the hot loop -- which is where
        // ~90% of the time goes (110 features x 1024 x 2 perspectives). SF
        // keeps its accumulator in i16 for exactly this reason.
        let mut acc = vec![0i16; L1];
        acc.copy_from_slice(&net.ft_bias);
        let mut piece_feats = Vec::with_capacity(32);
        add_piece_features(board, pov, &mut piece_feats);
        let mut threat_feats = Vec::with_capacity(64);
        add_threat_features(atk, board, pov, &mut threat_feats);
        let mut pair_feats = Vec::new();
        add_pair_features(board, pov, &mut pair_feats);

        for &(f, _) in &piece_feats {
            let row = &net.ft_piece_w[f * L1..(f + 1) * L1];
            for (a, &w) in acc.iter_mut().zip(row.iter()) {
                *a = a.wrapping_add(w);
            }
        }
        for &f in &threat_feats {
            let row = &net.ft_threat_w[f * L1..(f + 1) * L1];
            for (a, &w) in acc.iter_mut().zip(row.iter()) {
                *a = a.wrapping_add(w as i16);
            }
        }
        for &f in &pair_feats {
            let pf = f - PAIR_BASE;
            let row = &net.ft_pair_w[pf * L1..(pf + 1) * L1];
            for (a, &w) in acc.iter_mut().zip(row.iter()) {
                *a = a.wrapping_add(w as i16);
            }
        }
        (acc, piece_feats, threat_feats, pair_feats)
    };

    let n = board.occ_all.count_ones() as i32;
    let bucket = (((n - 1) / 4).clamp(0, NB as i32 - 1)) as usize;

    // psqt needs the bucket, redo cleanly per perspective. Threat/pair
    // active features ALSO contribute to psqt via threatAndPpPsqtWeights,
    // always added (apply_psqt<+1> on the "active" list in SF's own
    // refresh-cache code) -- not just piece features.
    let psqt_de = |piece_feats: &[(usize, i32)], threat_feats: &[usize], pair_feats: &[usize]| -> i64 {
        let mut s: i64 = 0;
        for &(f, _) in piece_feats {
            s += net.ft_piece_psqt[f * NB + bucket] as i64;
        }
        for &f in threat_feats {
            s += net.ft_threat_psqt[f * NB + bucket] as i64;
        }
        for &f in pair_feats {
            let pf = f - PAIR_BASE;
            s += net.ft_pair_psqt[pf * NB + bucket] as i64;
        }
        s
    };

    // Incremental path: reuse the cached accumulator and apply only the
    // feature diff. `build_acc` stays as the reference implementation and is
    // what the correctness test compares against.
    let _ = &build_acc;
    let (psqt_s, psqt_n) = ESTADO.with(|c| {
        let mut st = c.borrow_mut();
        if !carrega_do_pai(&mut st, board) {
            acc_incremental(net, atk, board, stm, &mut st);
            acc_incremental(net, atk, board, nstm, &mut st);
        }
        st.valido = true;
        // PSQT from the same unified lists, so threats and pairs are not
        // silently dropped -- the official net does carry those weights.
        // psqt vem do acumulado por perspectiva: o caminho rapido nao
        // mantem as listas de features, logo nao pode somar por cima delas
        // guardar os bitboards DEPOIS dos acc_incremental: sao eles que
        // comparam contra o estado anterior para derivar o lance
        for c in 0..2 {
            for t in 0..6 {
                st.bb[c][t] = board.pieces[c][t];
            }
        }
        guarda_na_pilha(&mut st, board);

        let ps = st.psqt[stm][bucket];
        let pn = st.psqt[nstm][bucket];

        // Pairwise activation done here, straight out of the cached
        // accumulators into a reused buffer -- cloning two 1024-wide
        // accumulators per call was showing up as 14% of runtime in memset.
        let half = L1 / 2;
        let mut x = std::mem::take(&mut st.x);
        // Em `u16`, nao em `i32`. Os dois factores estao presos a [0, 255],
        // logo o produto nao passa de 65025 e cabe inteiro em 16 bits -- subir
        // a 32 obrigava o compilador a metade das pistas por instrucao e a um
        // `imull` escalar por saida, que era o que o perfil mostrava. O
        // deslocamento de 9 substitui a divisao por 512: com factores nao
        // negativos e' a mesma operacao.
        const TETO: i16 = FT_MAX_VAL as i16;
        // KESTREL_SATURA=1 conta que fraccao do acumulador bate no tecto do
        // crelu. Onde tudo satura o gradiente morre, e posicoes de muitas pecas
        // tem mais features activas -- se a nossa rede saturar muito mais do que
        // a de referencia, e' esse o defeito e nao o treino.
        if std::env::var_os("KESTREL_SATURA").is_some() {
            let (mut alto, mut zero, mut tot) = (0u64, 0u64, 0u64);
            for pov in 0..2 {
                for &v in st.acc[pov].iter() {
                    tot += 1;
                    if v >= TETO { alto += 1 } else if v <= 0 { zero += 1 }
                }
            }
            eprintln!("satura {:.1} zeros {:.1}", 100.0 * alto as f64 / tot as f64,
                100.0 * zero as f64 / tot as f64);
        }
        for (pov, base) in [(stm, 0usize), (nstm, half)] {
            let (a, b) = st.acc[pov].split_at(half);
            for (j, (&lo, &hi)) in a.iter().zip(b.iter()).enumerate() {
                let s0 = lo.clamp(0, TETO) as u16;
                let s1 = hi.clamp(0, TETO) as u16;
                x[base + j] = (s0.wrapping_mul(s1) >> 9) as u8;
            }
        }
        st.x = x;
        (ps, pn)
    });
    // SF's transform(): psqt = (psqtAccum[stm][bucket] - psqtAccum[ntm][bucket]) / 2.
    //
    // `KESTREL_ZERA_PSQT=1` anula-o em tempo de leitura, para qualquer rede.
    // Serve para separar o que o CORPO aprendeu do que vem do PSQT: numa rede
    // nossa de 12 superbatches o corpo correlaciona -0,05 com a oficial e a
    // rede toda 0,70, ou seja o material e' tudo e o resto ainda nao existe.
    let psqt = if std::env::var_os("KESTREL_ZERA_PSQT").is_some() {
        0
    } else {
        (psqt_s - psqt_n) / 2
    };

    if std::env::var_os("KESTREL_SF_DEBUG").is_some() {
        let mut pf_stm = Vec::new();
        add_piece_features(board, stm, &mut pf_stm);
        ESTADO.with(|c| {
            let st = c.borrow();
            eprintln!("DBG n={} bucket={} acc_stm[0..16]={:?} acc_ntm[0..16]={:?} psqt={}",
                board.occ_all.count_ones(), bucket, &st.acc[stm][0..16], &st.acc[nstm][0..16], psqt);
        });
    }

    let stack = &net.stacks[bucket];
    let x = ESTADO.with(|c| std::mem::take(&mut c.borrow_mut().x));

    // fc_0 input: SF's FeatureTransformer::transform pairwise activation.
    // Each perspective's own L1-wide accumulator is split into two
    // HalfDimensions/2 halves, each clamped to [0, FtMaxVal], multiplied
    // together and divided by 512 -- producing L1/2 outputs per
    // perspective, concatenated to L1 total (not 2*L1).
    let half = L1 / 2;

    // Weights are stored output-major on disk (file position i = output*InDim
    // + input) -- the get_weight_index_scrambled() SIMD permutation in SF's
    // source only affects the in-RAM destination slot during load, not the
    // sequential file byte order, so a plain scalar reader never needs to
    // replicate it.
    // Uma saida de cada vez, apesar de reler `x` 32 vezes: processar 4 em
    // paralelo foi MEDIDO e ficou 40% PIOR (8,6s vs 6,1s por 300k nos) --
    // quatro linhas de pesos ao mesmo tempo enchem a cache mais do que a
    // reutilizacao de `x` poupa. Nao repetir sem medir.
    let mut fc0_out = [0i32; L2];
    for o in 0..L2 {
        let row = &stack.fc0w[o * L1..(o + 1) * L1];
        fc0_out[o] = produto_u8_i8(&x[..L1], row) + stack.fc0b[o];
    }

    // ac_sqr_0 / ac_0 use WeightScaleBits+1 (SF: SqrClippedReLU<..,
    // WeightScaleBits+1> ac_sqr_0; ClippedReLU<.., WeightScaleBits+1> ac_0;).
    let wsb0 = WEIGHT_SCALE_BITS as u32 + 1;
    let mut concat1 = [0i32; 2 * L2];
    for o in 0..L2 {
        concat1[o] = clipped_sq(fc0_out[o], wsb0);
        concat1[L2 + o] = clipped_lin(fc0_out[o], wsb0);
    }

    let mut fc1_out = [0i32; L3];
    for o in 0..L3 {
        let mut s: i64 = 0;
        let row = &stack.fc1w[o * (2 * L2)..(o + 1) * (2 * L2)];
        for i in 0..2 * L2 {
            s += concat1[i] as i64 * row[i] as i64;
        }
        fc1_out[o] = (s as i32) + stack.fc1b[o];
    }

    // ac_sqr_1 / ac_1 use plain WeightScaleBits.
    let wsb1 = WEIGHT_SCALE_BITS as u32;
    let mut concat2 = [0i32; 2 * L2 + 2 * L3];
    concat2[..2 * L2].copy_from_slice(&concat1);
    for o in 0..L3 {
        concat2[2 * L2 + o] = clipped_sq(fc1_out[o], wsb1);
        concat2[2 * L2 + L3 + o] = clipped_lin(fc1_out[o], wsb1);
    }

    let mut s: i64 = 0;
    for i in 0..2 * L2 + 2 * L3 {
        s += concat2[i] as i64 * stack.fc2w[i] as i64;
    }
    let fc2_out = (s as i32) + stack.fc2b;

    let skip_0 = fc0_out[L2 - 2] - fc0_out[L2 - 1];
    let fwd_out = (fc2_out + skip_0) as i64;

    if std::env::var_os("KESTREL_SF_DEBUG").is_some() {
        eprintln!("DBG fc0_out[0..4]={:?} fc1_out[0..4]={:?} fc2_out={} skip_0={} fwd_out={}",
            &fc0_out[0..4], &fc1_out[0..4], fc2_out, skip_0, fwd_out);
    }

    ESTADO.with(|c| c.borrow_mut().x = x);

    let multiplier: i64 = 600 * OUTPUT_SCALE;
    let denominator: i64 = HIDDEN_ONE_VAL * (1i64 << WEIGHT_SCALE_BITS) * 2;
    let positional = fwd_out * multiplier / denominator;

    let bruto = ((psqt / OUTPUT_SCALE) + (positional / OUTPUT_SCALE)) as i32;

    // O motor tem UMA escala de avaliacao, e nao e' a desta rede.
    //
    // Medido: uma dama a mais vale 2578 nesta rede e 1161 na que o motor tinha
    // quando as margens da busca foram calibradas -- factor 2.22. Todas essas
    // margens (RFP, futility, os termos do LMR, o corte de "lance obvio" a
    // 150cp) comparam contra SCORES, portanto com evals 2.2x maiores disparam
    // 2.2x mais cedo do que foram afinadas para disparar. Nao e' que estejam
    // erradas: e' que estao a ler outra regua.
    //
    // Dividir aqui poe a rede na escala do motor. E' uma transformacao
    // monotona -- nao muda a ordem de nenhum par de posicoes, logo nao muda a
    // qualidade da avaliacao; muda o que as margens veem.
    //
    // Relacao com `KESTREL_ESCALA` (build.rs): essa ja' existia e diz ao WDL
    // quantas unidades internas valem um peao -- 200, medido para a rede
    // antiga. Nao serve aqui sozinha porque as margens de poda NAO a consultam:
    // sao centipeoes fixos no codigo, como o proprio build.rs explica. Ou seja,
    // ha' duas leituras da mesma escala e so' uma delas e' configuravel. Este
    // factor poe o eval na escala que as margens ja' esperam, em vez de mexer
    // nas dezenas de constantes que teriam de mudar todas juntas.
    //
    // `KESTREL_EVAL_FACTOR` em centesimos (222 = 2.22, o factor medido). A 100
    // nao faz nada, que e' o comportamento antigo, para o SPRT medir os dois.
    let fator = eval_factor();
    if fator == 100 { bruto } else { (bruto as i64 * 100 / fator as i64) as i32 }
}

/// Escala da rede em centesimos, por `setoption name EvalFactor value N` ou
/// `KESTREL_EVAL_FACTOR`. Atomica e nao `OnceLock`: o bot troca de rede sem
/// reiniciar, e a escala tem de poder acompanhar.
static FATOR_ESCALA: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

pub fn set_eval_factor(v: i32) {
    FATOR_ESCALA.store(v.clamp(20, 500), std::sync::atomic::Ordering::Relaxed);
}

#[inline]
pub fn eval_factor() -> i32 {
    let v = FATOR_ESCALA.load(std::sync::atomic::Ordering::Relaxed);
    if v > 0 {
        return v;
    }
    let d = std::env::var("KESTREL_EVAL_FACTOR")
        .ok()
        .and_then(|x| x.parse::<i32>().ok())
        .filter(|x| *x >= 20 && *x <= 500)
        .unwrap_or(100);
    FATOR_ESCALA.store(d, std::sync::atomic::Ordering::Relaxed);
    d
}

static REDE: OnceLock<Option<RedeSf>> = OnceLock::new();

pub fn rede() -> Option<&'static RedeSf> {
    REDE.get_or_init(|| {
        let path = std::env::var("KESTREL_NNUE_SF").ok()?;
        let bytes = std::fs::read(&path).ok()?;
        carrega(&bytes)
    })
    .as_ref()
}

pub fn active() -> bool {
    rede().is_some()
}

// ---- Converting bullet's raw f32 weights into an SF network ----

/// Quantisation scales. The feature transformer's scale is fixed by the
/// format: SF clamps the accumulator to FtMaxVal=255 and its pairwise step
/// divides by 512, so 255 is "one". The dense layers use 64, matching
/// WeightScaleBits=6.
pub const CONV_QA: f32 = 256.0;
/// Per-layer weight scales -- they are NOT all the same: fc1 uses half the
/// others. Biases scale by their own layer's weight scale times the hidden
/// quantisation (128).
pub const CONV_W_FC0: f32 = 128.0;
pub const CONV_W_FC1: f32 = 64.0;
pub const CONV_W_FC2: f32 = 128.0;
const HIDDEN_QUANT_ONE: f32 = 128.0;
pub const CONV_B_FC0: f32 = CONV_W_FC0 * HIDDEN_QUANT_ONE;
pub const CONV_B_FC1: f32 = CONV_W_FC1 * HIDDEN_QUANT_ONE;
pub const CONV_B_FC2: f32 = CONV_W_FC2 * HIDDEN_QUANT_ONE;
/// PSQT: nnue2score (600) * weight_scale_out (16).
/// PSQT scale: the derived 600 (nnue2score) * 16 (OutputScale), times 4.
///
/// The 4 is the piece that is still empirical, and it is small enough to be
/// honest about. What it replaces is a factor of 1200, which was not a scale at
/// all -- it was compensating for a PSQT that the trainer initialised 300x too
/// small. With `psqt` given its own initialisation (stdev 0.5 instead of the
/// generic affine's sqrt(2/87601) = 0.0048), the weights land in the right
/// range on their own and this drops back to something close to the derivation.
///
/// Measured at superbatch 3 (30M positions): a queen for White reads +3144 and
/// for Black -1787, against the official net's +2578 and -1885, with the start
/// position and a bare-kings ending both near zero.
/// O `*4` que aqui estava era compensacao, nao escala.
///
/// Medido com um `raw.bin` sintetico que tem SO' a semente e zeros em tudo o
/// resto: com `600*16` o valor da dama sai a 2538, que e' exactamente o que a
/// semente traz. Com o `*4` saia 10152.
pub const CONV_PSQT: f32 = 600.0 * 16.0;

fn quant_round(v: f32, scale: f32) -> f32 {
    (v * scale).round()
}

/// Build an SF network from bullet's raw (f32) tensors, using `molde` for the
/// header fields so Stockfish's architecture-hash check passes.
///
/// `l0w` is feature-major: 1024 contiguous weights per input row, laid out as
/// [factor(704) | dustbin(1) | pieces(22528) | threats(59808) | pairs(4560)].
///
/// Two things are load-bearing here:
///  * The factoriser is FOLDED IN. bullet trains a shared 704-row piece-square
///    factor alongside the king-bucketed rows and does NOT merge it on save;
///    SF's format has no factoriser, so each piece row must become
///    row + factor_row. Skipping this silently wrecks the network.
///  * Threat and pair rows are stored as i8 in SF, so they are clipped to
///    +-127. Measured on a real checkpoint only ~0.02% of them are affected.
#[allow(clippy::too_many_arguments)]
/// A rede oficial escrita no layout do bullet, para voltar a entrar pelo
/// `de_bullet` e sair igual.
///
/// A ideia veio de querermos destilar a rede oficial: professor e aluno tem a
/// MESMA arquitectura, portanto o aluno pode representar o professor
/// exactamente. Aqui leva-se isso ao limite -- em vez de treinar, escrevem-se
/// os pesos da oficial no formato que o treinador produz e passam-se pelo
/// conversor. A resposta certa e' conhecida: tem de sair a rede de partida,
/// byte a byte. Qualquer diferenca e' um bug do `de_bullet`, e nao ha' treino
/// nenhum pelo meio a poder ser culpado.
///
/// As linhas do factorizador e o dustbin vao a zero, que e' o que uma fusao
/// correcta produziria. Os tensores sao escritos no layout TRANSPOSTO e com a
/// saida negada, que e' o que o `de_bullet` passou a assumir por omissao --
/// esta funcao tem de acompanhar esses dois omissos ou deixa de provar nada. Repare-se que o `de_bullet` nem sequer LE' a linha do
/// dustbin: as pecas lem `BASE + f` e os factores `f % FACT`, e o dustbin fica
/// no indice `FACT`, entre os dois. E' isso a raiz do problema -- treinado de
/// um lado, invisivel do outro.
pub fn roundtrip_bullet(net: &RedeSf) -> RedeSf {
    const FACT: usize = 704;
    const BASE: usize = FACT + 1;
    const NIN: usize = BASE + INPUT_DIM;

    let mut l0w = vec![0f32; NIN * L1];
    let mut l0b = vec![0f32; L1];
    for k in 0..L1 {
        l0b[k] = net.ft_bias[k] as f32 / CONV_QA;
    }
    for f in 0..PIECE_DIM {
        for k in 0..L1 {
            l0w[k * NIN + BASE + f] = net.ft_piece_w[f * L1 + k] as f32 / CONV_QA;
        }
    }
    for f in 0..THREAT_DIM {
        for k in 0..L1 {
            l0w[k * NIN + BASE + PIECE_DIM + f] = net.ft_threat_w[f * L1 + k] as f32 / CONV_QA;
        }
    }
    for f in 0..PAIR_DIM {
        for k in 0..L1 {
            l0w[k * NIN + BASE + PIECE_DIM + THREAT_DIM + f] =
                net.ft_pair_w[f * L1 + k] as f32 / CONV_QA;
        }
    }

    let mut psqtw = vec![0f32; NIN * NB];
    for f in 0..PIECE_DIM {
        for b in 0..NB {
            psqtw[b * NIN + BASE + f] = -(net.ft_piece_psqt[f * NB + b] as f32) / CONV_PSQT;
        }
    }
    for f in 0..THREAT_DIM {
        for b in 0..NB {
            psqtw[b * NIN + BASE + PIECE_DIM + f] =
                -(net.ft_threat_psqt[f * NB + b] as f32) / CONV_PSQT;
        }
    }
    for f in 0..PAIR_DIM {
        for b in 0..NB {
            psqtw[b * NIN + BASE + PIECE_DIM + THREAT_DIM + f] =
                -(net.ft_pair_psqt[f * NB + b] as f32) / CONV_PSQT;
        }
    }

    // Densas, no layout que o `de_bullet` le' sem `KESTREL_DENSE_T`.
    let n2 = 2 * L2 + 2 * L3;
    let mut fc0w = vec![0f32; L1 * L2 * NB];
    let mut fc0b = vec![0f32; L2 * NB];
    let mut fc1w = vec![0f32; (2 * L2) * L3 * NB];
    let mut fc1b = vec![0f32; L3 * NB];
    let mut fc2w = vec![0f32; n2 * NB];
    let mut fc2b = vec![0f32; NB];
    for b in 0..NB {
        let st = &net.stacks[b];
        for o in 0..L2 {
            fc0b[b * L2 + o] = st.fc0b[o] as f32 / CONV_B_FC0;
            for i in 0..L1 {
                fc0w[i * (L2 * NB) + b * L2 + o] = st.fc0w[o * L1 + i] as f32 / CONV_W_FC0;
            }
        }
        for o in 0..L3 {
            fc1b[b * L3 + o] = st.fc1b[o] as f32 / CONV_B_FC1;
            for i in 0..2 * L2 {
                fc1w[i * (L3 * NB) + b * L3 + o] =
                    st.fc1w[o * (2 * L2) + i] as f32 / CONV_W_FC1;
            }
        }
        for i in 0..n2 {
            fc2w[i * NB + b] = st.fc2w[i] as f32 / CONV_W_FC2;
        }
        fc2b[b] = st.fc2b as f32 / CONV_B_FC2;
    }

    // Factorizador a zero: e' o que a fusao deve deixar.
    let fc0fw = vec![0f32; L1 * L2];
    let fc0fb = vec![0f32; L2];
    let fc1fw = vec![0f32; (2 * L2) * L3];
    let fc1fb = vec![0f32; L3];
    let fc2fw = vec![0f32; n2];
    let fc2fb = vec![0f32; 1];

    de_bullet(net, &l0w, &l0b, &fc0w, &fc0b, &fc1w, &fc1b, &fc2w, &fc2b, &psqtw,
        &fc0fw, &fc0fb, &fc1fw, &fc1fb, &fc2fw, &fc2fb)
}

pub fn de_bullet(
    molde: &RedeSf,
    l0w: &[f32], l0b: &[f32],
    fc0w: &[f32], fc0b: &[f32],
    fc1w: &[f32], fc1b: &[f32],
    fc2w: &[f32], fc2b: &[f32],
    psqtw: &[f32],
    fc0fw: &[f32], fc0fb: &[f32],
    fc1fw: &[f32], fc1fb: &[f32],
    fc2fw: &[f32], fc2fb: &[f32],
) -> RedeSf {
    const FACT: usize = 704;
    const BASE: usize = FACT + 1; // factor rows + dustbin

    // KESTREL_CONV_T=1 reads every feature-transformer tensor transposed.
    //
    // Not a guess: the trainer evaluates a queen up at +1397 cp while the
    // engine reads the SAME checkpoint, converted, at +40 -- so the network
    // learned material and the conversion loses it. Switching the PSQT off
    // changes nothing (-544 vs -546), which says the PSQT arrives as zeros,
    // and the feature rows are just as dead. One layout error explains all of
    // it at once, and it is cheap to test.
    // LIGADO por omissao desde 2026-08-17, e `KESTREL_CONV_T=0` desliga-o.
    // Estava ao contrario, e por isso TODAS as redes que convertemos ate' aqui
    // sairam mal sem nunca dar erro. Ver o comentario do `negar` para a
    // medicao que o decidiu.
    // DESLIGADO por omissao outra vez, e desta vez com razao: com a ordem dos
    // tensores corrigida, a leitura coluna-maior dos documentos do treinador e'
    // a que mede melhor. Estava ligado porque a ordem errada fazia qualquer
    // leitura parecer meio certa.
    let transposto = match std::env::var("KESTREL_CONV_T") { Ok(v) => v != "0", Err(_) => false };
    let l0_at = |f: usize, k: usize| -> f32 {
        if transposto { l0w[k * NIN + f] } else { l0w[f * L1 + k] }
    };
    // O PSQT tem bandeira PROPRIA, e a razao e' que os dois tensores nao sao
    // declarados da mesma maneira no treinador: o `l0` sai de `new_affine`, o
    // `psqtw` de `new_weights("psqtw", (NB, NIN))` com a forma escrita a mao.
    // Prender os dois ao mesmo interruptor obrigava-os a partilhar uma
    // convencao que nunca partilharam -- e o sintoma era o PSQT a pedir um
    // factor de escala diferente por posicao (so' reis queria +1, rei+dama
    // -0,39, uma abertura -0,05), que e' o que se ve quando as features leem
    // valores umas das outras.
    // Tambem desligado. A preferencia medida pela leitura contraria vinha de
    // NOS escrevermos o `psqt_base.bin` em linha-maior: o bullet carregava-o
    // permutado, e o conversor desfazia a permutacao ao le'-lo da mesma maneira
    // errada. Os dois erros cancelavam-se e nada disto aparecia -- excepto que
    // a semente entrava com os valores das pecas espalhados por linhas de
    // factores e de ameacas, que era o que ela existia para evitar.
    let psqt_t = match std::env::var("KESTREL_PSQT_T") { Ok(v) => v != "0", Err(_) => false };
    let psqt_at = |f: usize, b: usize| -> f32 {
        if psqt_t { psqtw[b * NIN + f] } else { psqtw[f * NB + b] }
    };
    // KESTREL_CONV_NEG=1 nega a saida da rede: a camada final e o PSQT, que sao
    // os dois caminhos por onde o resultado sai. Nao e' uma opcao, e' uma
    // sonda: com CONV_T=1 a rede convertida correlaciona -0,70 com a oficial e
    // o declive do ajuste e' -0,87, ou seja esta' certa mas ao contrario. Se
    // negar poe a correlacao em +0,70, o defeito e' um sinal e nao o mapeamento.
    // LIGADO por omissao, `KESTREL_CONV_NEG=0` desliga.
    //
    // Medido sobre 50 posicoes do bench, contra a rede oficial, com o
    // checkpoint de 12 superbatches:
    //
    //   como estava (CONV_T=0, sem negar)   correlacao -0,18
    //   so' com a transposicao              correlacao -0,70, declive -0,87
    //   com a transposicao e o sinal        correlacao +0,70, declive +0,87
    //
    // O -0,70 e' o que diz o que se passava: a rede convertida nao era lixo,
    // era a avaliacao certa ao contrario. O treinador da' o sinal certo no
    // mesmo checkpoint (+895,6 cp para rei+dama contra rei), logo a inversao
    // nasce aqui. Falta ainda apurar QUAL das convencoes difere -- a saida do
    // bullet ou a nossa leitura dela -- mas a conversao passa a funcionar e a
    // medicao fica registada para quem for a esse fundo.
    // A negacao aplica-se SO' ao PSQT, e nunca a` camada final.
    //
    // Negava as duas, e estava a corrigir uma e a estragar a outra. Medido no
    // corpo sozinho, com o PSQT anulado dos dois lados: sem negacao
    // correlaciona 0,7172, com negacao 0,5315. O PSQT e' que a quer -- a
    // semente que o inicializa nasceu com o sinal ao contrario do que o motor
    // espera, e o motor esta' certo, porque le' a rede oficial ao bit contra o
    // codigo do Stockfish.
    //
    // Isto explica o tecto: metade da rede vinha invertida em todas as
    // conversoes que fizemos, e a metade que estava bem era a que eu negava.
    // DESLIGADA por omissao: a semente passou a ser escrita com o sinal certo,
    // portanto nao ha' nada a compensar. Fica a bandeira para converter
    // checkpoints treinados com a semente antiga.
    let negar_psqt = match std::env::var("KESTREL_CONV_NEG") { Ok(v) => v != "0", Err(_) => false };
    let sinal_psqt: f32 = if negar_psqt { -1.0 } else { 1.0 };
    let mut clipados = 0usize;
    let mut ft_bias = vec![0i16; L1];
    for k in 0..L1 {
        ft_bias[k] = quant_round(l0b[k], CONV_QA) as i16;
    }

    // Pieces: fold the factor row in, keep i16.
    let mut ft_piece_w = vec![0i16; PIECE_DIM * L1];
    for f in 0..PIECE_DIM {
        let fact = f % FACT;
        for k in 0..L1 {
            let v = l0_at(BASE + f, k) + l0_at(fact, k);
            ft_piece_w[f * L1 + k] = quant_round(v, CONV_QA) as i16;
        }
    }

    // Threats and pairs: no factor, and they must fit in i8.
    // Two different scales share this: the feature transformer's threat/pair
    // rows use QA, the dense layers use QB. Passing the wrong one is silent --
    // the net still loads, just evaluates nonsense.
    let mut clip_i8 = |v: f32, escala: f32, clipados: &mut usize| -> i8 {
        let q = quant_round(v, escala);
        if q > 127.0 { *clipados += 1; 127 } else if q < -127.0 { *clipados += 1; -127 } else { q as i8 }
    };
    let mut ft_threat_w = vec![0i8; THREAT_DIM * L1];
    for f in 0..THREAT_DIM {
        for k in 0..L1 {
            ft_threat_w[f * L1 + k] = clip_i8(l0_at(BASE + PIECE_DIM + f, k), CONV_QA, &mut clipados);
        }
    }
    let mut ft_pair_w = vec![0i8; PAIR_DIM * L1];
    for f in 0..PAIR_DIM {
        for k in 0..L1 {
            ft_pair_w[f * L1 + k] =
                clip_i8(l0_at(BASE + PIECE_DIM + THREAT_DIM + f, k), CONV_QA, &mut clipados);
        }
    }

    // PSQT, now trained. Layout is column-major like the rest: psqtw is
    // [NB x NIN], so feature f lives at psqtw[f * NB + b]. The factor rows
    // fold into the piece rows here too.
    const NIN: usize = FACT + 1 + PIECE_DIM + THREAT_DIM + PAIR_DIM;
    let _ = NIN;
    // SF divides psqt by OutputScale (16) and the training used eval_scale
    // 400, so one unit of the trained psqt is 16*400 internal units.
    // QA*QB (32640) was 5x too big and swamped low-piece positions.
    // KESTREL_PSQT_ESC=<f> multiplica a escala do PSQT. O PSQT treinado pelo
    // bullet tem media 0.0056 onde o do pytorch tem 1.54 -- 276x. Ou o bullet
    // nao o treina, ou treina-o noutras unidades; esta bandeira separa as duas.
    let escala_psqt: f32 = if std::env::var_os("KESTREL_SEM_PSQT").is_some() {
        0.0
    } else {
        sinal_psqt * CONV_PSQT
            * std::env::var("KESTREL_PSQT_ESC").ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0)
    };
    let mut ft_piece_psqt = vec![0i32; PIECE_DIM * NB];
    for f in 0..PIECE_DIM {
        let fact = f % FACT;
        for b in 0..NB {
            let v = psqt_at(BASE + f, b) + psqt_at(fact, b);
            ft_piece_psqt[f * NB + b] = quant_round(v, escala_psqt) as i32;
        }
    }
    let mut ft_threat_psqt = vec![0i32; THREAT_DIM * NB];
    for f in 0..THREAT_DIM {
        for b in 0..NB {
            ft_threat_psqt[f * NB + b] =
                quant_round(psqt_at(BASE + PIECE_DIM + f, b), escala_psqt) as i32;
        }
    }
    let mut ft_pair_psqt = vec![0i32; PAIR_DIM * NB];
    for f in 0..PAIR_DIM {
        for b in 0..NB {
            ft_pair_psqt[f * NB + b] = quant_round(
                psqt_at(BASE + PIECE_DIM + THREAT_DIM + f, b), escala_psqt) as i32;
        }
    }

    // bullet stores each dense layer as [out_total, in]; SF wants one stack
    // per bucket, output-major.
    // `var_os(..).is_some()` dava-se por ligado com `=0`, porque a variavel
    // fica DEFINIDA. Uma grelha inteira de testes correu com ele ligado nas
    // dezasseis celulas e so' se percebeu porque as linhas que so' diferiam
    // nele davam numeros identicos. Mesma convencao das outras: `=0` desliga.
    let dense_t = match std::env::var("KESTREL_DENSE_T") { Ok(v) => v != "0", Err(_) => false };
    let mut stacks = Vec::with_capacity(NB);
    for b in 0..NB {
        let mut s_fc0w = vec![0i8; L2 * L1];
        let mut s_fc0b = vec![0i32; L2];
        for o in 0..L2 {
            let src = b * L2 + o;
            s_fc0b[o] = quant_round(fc0b[src] + fc0fb[o], CONV_B_FC0) as i32;
            for i in 0..L1 {
                // [out_total, in], as the comment above says: element (out, in)
                // lives at out * L1 + in. Reading it in-major transposed the
                // whole dense stack, which is where a queen worth +1397 cp in
                // the trainer arrived as +40 in the engine.
                let v = if dense_t { fc0w[src * L1 + i] + fc0fw[o * L1 + i] }
                        else { fc0w[i * (L2 * NB) + src] + fc0fw[i * L2 + o] };
                s_fc0w[o * L1 + i] = clip_i8(v, CONV_W_FC0, &mut clipados);
            }
        }
        let mut s_fc1w = vec![0i8; L3 * (2 * L2)];
        let mut s_fc1b = vec![0i32; L3];
        for o in 0..L3 {
            let src = b * L3 + o;
            s_fc1b[o] = quant_round(fc1b[src] + fc1fb[o], CONV_B_FC1) as i32;
            for i in 0..2 * L2 {
                let v = if dense_t { fc1w[src * (2 * L2) + i] + fc1fw[o * (2 * L2) + i] }
                        else { fc1w[i * (L3 * NB) + src] + fc1fw[i * L3 + o] };
                s_fc1w[o * (2 * L2) + i] = clip_i8(v, CONV_W_FC1, &mut clipados);
            }
        }
        let n2 = 2 * L2 + 2 * L3;
        let mut s_fc2w = vec![0i8; n2];
        for i in 0..n2 {
            s_fc2w[i] = clip_i8(fc2w[i * NB + b] + fc2fw[i], CONV_W_FC2, &mut clipados);
        }
        let s_fc2b = quant_round(fc2b[b] + fc2fb[0], CONV_B_FC2) as i32;

        stacks.push(LayerStack {
            fc0w: s_fc0w, fc0b: s_fc0b, fc1w: s_fc1w, fc1b: s_fc1b, fc2w: s_fc2w, fc2b: s_fc2b,
        });
    }

    eprintln!("nnue-sf: convertido do bullet ({} pesos clipados para i8)", clipados);

    RedeSf {
        ft_bias, ft_threat_w, ft_pair_w, ft_piece_w,
        ft_piece_psqt, ft_threat_psqt, ft_pair_psqt, stacks,
        version: molde.version,
        hash: molde.hash,
        desc: molde.desc.clone(),
        ft_header: molde.ft_header,
        stack_headers: molde.stack_headers.clone(),
    }
}

// ---- Incremental accumulator ----
//
// Measured: the reader was spending ~90% of its time reading ~290 KB of
// weights per node (110 features x 1024 x 2 perspectives), against a 95 MB
// network -- every feature a cache miss. Recomputing that per node caps us at
// ~26k NPS while the bot's own net does 467k.
//
// This is Stockfish's idea, not its code: keep the previous position's
// accumulator and touch only the rows whose features changed. Nodes handed to
// evaluate() in an alpha-beta search are usually one to three moves apart, so
// the diff is small even though the position is not literally the parent.
//
// It deliberately does NOT hook into make/unmake: a single cached state keeps
// the search untouched and the failure mode safe -- a miss just costs a full
// rebuild, never a wrong answer.

/// Unified feature index so the three families can share one diff.
/// [0, PIECE_DIM) pieces | [PIECE_DIM, +THREAT_DIM) threats | then pairs.
const U_THREAT: usize = PIECE_DIM;
const U_PAIR: usize = PIECE_DIM + THREAT_DIM;

/// 1024 i16 add/sub with AVX2: 16 lanes per instruction against the 8 the
/// autovectoriser was settling for. Profiling put this at ~15% of search time.

/// Same, widening i8 weights to i16 on the way in.

/// One weight of one feature row, whichever family `u` falls in. Only used by
/// the delta verifier, to identify a feature from the damage it did.
fn linha_peso(net: &RedeSf, u: usize, i: usize) -> i16 {
    if u < U_THREAT {
        net.ft_piece_w[u * L1 + i]
    } else if u < U_PAIR {
        net.ft_threat_w[(u - U_THREAT) * L1 + i] as i16
    } else {
        net.ft_pair_w[(u - U_PAIR) * L1 + i] as i16
    }
}

#[inline]
/// Uma passagem pelo acumulador por cada linha de peso.
///
/// Fundir as passagens -- manter um pedaco do acumulador em registos enquanto
/// as ~12 linhas de um lance lhe passam por cima, como o Stockfish faz -- foi
/// escrito e MEDIDO: 98184 contra 97506 nps em cinco rondas, ou seja empate,
/// com os melhores tempos tambem empatados. O acumulador sao 2 KiB e ja' fica
/// em L1 entre as chamadas; o que custa e' percorrer as linhas de peso, e
/// nenhuma arrumacao das passagens evita esse trafego. Nao repetir sem uma
/// razao nova.
/// Pedir as linhas a memoria antes de as usar (`_mm_prefetch` no inicio de cada
/// uma, todas de uma vez antes de aplicar qualquer) foi escrito e MEDIDO:
/// 121459 contra 121107 nps em quatro rondas, empate. As ~24 linhas estao
/// espalhadas por ~112 MiB e quase nenhuma esta' em cache, mas o processador
/// ja' as tinha em voo ao mesmo tempo por execucao fora de ordem -- as faltas
/// nao estavam a acontecer em serie, como eu supus. Nao repetir sem uma razao
/// nova.
fn aplica_linha(net: &RedeSf, acc: &mut [i16], u: usize, somar: bool) {
    if u < U_THREAT {
        let row = &net.ft_piece_w[u * L1..(u + 1) * L1];
        if somar {
            for (a, &w) in acc.iter_mut().zip(row) { *a = a.wrapping_add(w); }
        } else {
            for (a, &w) in acc.iter_mut().zip(row) { *a = a.wrapping_sub(w); }
        }
    } else {
        let (f, row) = if u < U_PAIR {
            let f = u - U_THREAT;
            (f, &net.ft_threat_w[f * L1..(f + 1) * L1])
        } else {
            let f = u - U_PAIR;
            (f, &net.ft_pair_w[f * L1..(f + 1) * L1])
        };
        let _ = f;
        if somar {
            for (a, &w) in acc.iter_mut().zip(row) { *a = a.wrapping_add(w as i16); }
        } else {
            for (a, &w) in acc.iter_mut().zip(row) { *a = a.wrapping_sub(w as i16); }
        }
    }
}

/// Active features for one perspective, as sorted unified indices.
fn feats_unificadas(
    atk: &Attacks, board: &Board, pov: usize,
    pecas: &mut Vec<(usize, i32)>, out: &mut Vec<u32>,
    t: &mut Vec<usize>, pr: &mut Vec<usize>,
) {
    pecas.clear();
    out.clear();
    t.clear();
    pr.clear();

    // The three families occupy disjoint index ranges (pieces < threats <
    // pairs), so sorting each one and concatenating gives a sorted whole --
    // cheaper than one sort over all ~91, which profiling put at ~5%.
    add_piece_features(board, pov, pecas);
    let ini = out.len();
    for &(f, _) in pecas.iter() {
        out.push(f as u32);
    }
    out[ini..].sort_unstable();

    add_threat_features(atk, board, pov, t);
    let ini = out.len();
    for &f in t.iter() {
        out.push((U_THREAT + f) as u32);
    }
    out[ini..].sort_unstable();

    add_pair_features(board, pov, pr);
    let ini = out.len();
    for &f in pr.iter() {
        out.push((U_PAIR + (f - PAIR_BASE)) as u32);
    }
    out[ini..].sort_unstable();
}

struct EstadoAcc {
    valido: bool,
    acc: [Vec<i16>; 2],
    feats: [Vec<u32>; 2],
    // scratch, reused to keep this off the allocator in the hot path
    novas: Vec<u32>,
    pecas: Vec<(usize, i32)>,
    bb: [[u64; 6]; 2],
    scratch_t: Vec<usize>,
    scratch_p: Vec<usize>,
    /// Buffers do delta dos pares de peoes, reaproveitados: `Vec::new()` por
    /// lance de peao aparecia como `_int_malloc` no perfil.
    par_sai: Vec<usize>,
    par_entra: Vec<usize>,
    x: Vec<u8>,
    psqt: [[i64; NB]; 2],
    /// Cache de refresh por casa de rei ("finny tables"): para cada
    /// (casa do rei, perspectiva) guarda o acumulador SO' com features de
    /// peca e os bitboards que o geraram. Reconstruir passa a ser aplicar a
    /// diferenca de pecas contra a entrada, em vez de somar as ~32 do zero.
    /// As ameacas e os pares ficam de fora de proposito -- mudam de mais
    /// para valer a pena cachear, e sao somados por cima depois.
    cache: Vec<EntradaCache>,
    /// Um acumulador por lance de profundidade, indexado por `board.prof_acc`.
    ///
    /// A sonda que motivou isto: com um so' estado, o "pai" da proxima
    /// avaliacao esta' la' em 28% dos casos, e um anel dos ultimos 16 estados
    /// so' chega a 58% -- porque o pai nao esta' poucas avaliacoes atras, esta'
    /// um ply acima no caminho, e a busca desce e sobe entre as duas. Por ply,
    /// esta' sempre.
    pilha: Vec<Camada>,
}

/// Uma camada da pilha: o acumulador de uma posicao do caminho actual.
///
/// Guarda-se `bb` com ele porque e' a prova de a que posicao pertence -- e'
/// isso que permite aceitar uma camada deixada por outro ramo a mesma
/// profundidade: o delta reconstroi o tabuleiro antigo a partir do actual mais
/// os eventos, portanto so' exige que a diferenca seja o deslocamento de uma
/// peca, nao que tenha sido o lance realmente jogado.
#[derive(Clone)]
struct Camada {
    acc: [Vec<i16>; 2],
    psqt: [[i64; NB]; 2],
    bb: [[u64; 6]; 2],
    valido: bool,
}

/// Fundo maximo coberto pela pilha. Acima disto o motor volta ao comportamento
/// de um so' estado, que e' sempre correcto, so' mais lento.
const MAX_CAMADAS: usize = 256;

#[derive(Clone)]
struct EntradaCache {
    acc: Vec<i16>,
    psqt: [i64; NB],
    bb: [[u64; 6]; 2],
    valido: bool,
}

impl EstadoAcc {
    fn novo() -> Self {
        EstadoAcc {
            valido: false,
            acc: [vec![0i16; L1], vec![0i16; L1]],
            feats: [Vec::with_capacity(192), Vec::with_capacity(192)],
            novas: Vec::with_capacity(192),
            pecas: Vec::with_capacity(32),
            bb: [[0u64; 6]; 2],
            scratch_t: Vec::with_capacity(128),
            scratch_p: Vec::with_capacity(32),
            par_sai: Vec::with_capacity(32),
            par_entra: Vec::with_capacity(32),
            x: vec![0u8; L1],
            psqt: [[0i64; NB]; 2],
            cache: vec![
                EntradaCache { acc: Vec::new(), psqt: [0; NB], bb: [[0; 6]; 2], valido: false };
                64 * 2
            ],
            pilha: vec![
                Camada {
                    acc: [vec![0i16; L1], vec![0i16; L1]],
                    psqt: [[0i64; NB]; 2],
                    bb: [[0u64; 6]; 2],
                    valido: false,
                };
                MAX_CAMADAS
            ],
        }
    }
}

thread_local! {
    static ESTADO: std::cell::RefCell<EstadoAcc> = std::cell::RefCell::new(EstadoAcc::novo());
}

/// Rebuild `acc` for one perspective from the cached state, applying only the
/// difference. Returns the piece features (the caller needs them for PSQT).
/// Threat deltas straight from the move, using the two board instants the
/// occupancy demands: the piece leaving is evaluated on the OLD board, the
/// piece arriving on the NEW one. Returns false when the change is not a
/// simple move, and the caller rebuilds instead.
fn delta_por_lance(
    net: &RedeSf, board: &Board, pov: usize, st: &mut EstadoAcc,
    ev: &[(usize, usize, usize, bool)],
) -> bool {
    let agora = board_para_posbb(board);
    let mut antes = agora;
    // desfazer os eventos para reconstruir o tabuleiro anterior
    for &(sq, t, c, add) in ev {
        if add { antes.pieces[c][t] &= !(1u64 << sq); } else { antes.pieces[c][t] |= 1u64 << sq; }
    }

    let mut deltas = Vec::with_capacity(64);
    // sem_raios = {from,to} nas saidas, para nao contar duas vezes a mesma
    // descoberta que a chamada da entrada ja' trata
    let mut casas = 0u64;
    for &(sq, _, _, _) in ev { casas |= 1u64 << sq; }

    // Sequential, exactly as the reference does it: the threats are updated
    // piece by piece as the board changes, not from two fixed snapshots.
    //
    // The distinction is not cosmetic. Take `Qxf6`: the knight leaves f6, which
    // reveals f7 to the queen on f3 -- and then the queen itself lands on f6 and
    // blocks it again. The revealing and the re-blocking happen at DIFFERENT
    // board states, and the intermediate one (f6 empty, queen still on f3) is
    // where they cancel. Reading only "before" and "after", that state does not
    // exist, and no ray guard can recover it: we emitted the reveal and never
    // the re-block.
    //
    // So the board is walked through the move instead. Removals are evaluated
    // with the piece still on the board and then cleared; additions are placed
    // first and then evaluated -- the order `remove_piece`/`put_piece` impose.
    // The captured piece comes off FIRST, before the piece that moves leaves
    // its square -- `do_move` runs `remove_piece(to)` and only then
    // `move_piece(from, to)`. A removal whose square also receives an addition
    // is the capture; the order between the two is not free, and getting it
    // backwards leaves a stale threat on `dxe5`-shaped moves.
    // A removal is a CAPTURE when its colour is not the colour that is putting
    // pieces down -- not when "its square also receives a piece". The square
    // test looks right and breaks on en passant, where the captured pawn stands
    // on neither `from` nor `to`: it was then taken for the moving piece, came
    // off in the wrong order and was handed `fromTo`, which belongs only to the
    // piece that moves.
    // the king square of THIS perspective, taken from the final board and held
    // fixed while the move is walked (see `eventos_ameaca`)
    let ksq_pov = agora.king_sq(pov);
    let mut cor_que_entra = 2usize;
    for &(_, _, c, add) in ev {
        if add {
            cor_que_entra = c;
        }
    }
    let mut corrente = antes;
    for capturada in [true, false] {
        for &(sq, t, c, add) in ev {
            if !add && (c != cor_que_entra) == capturada {
                // `noRaysContaining` belongs to the piece that MOVES, and only
                // to it: the original passes `fromTo` on the two halves of
                // `move_piece` and nothing at all on the `remove_piece` of a
                // capture. Passing it everywhere suppresses the capture's half
                // of a discovery while the move's half still fires, which is
                // how `Bxf6` was left holding a threat between a bishop on e7
                // and a bishop on g5 -- a pair that is blocked in both real
                // positions and exists only in the intermediate one.
                let sem = if capturada { !0u64 } else { casas };
                crate::sf_features::eventos_ameaca(
                    &corrente, pov, false, c, t, sq, sem, MAGIC, &mut deltas, ksq_pov);
                corrente.pieces[c][t] &= !(1u64 << sq);
            }
        }
    }
    for &(sq, t, c, add) in ev {
        if add {
            corrente.pieces[c][t] |= 1u64 << sq;
            crate::sf_features::eventos_ameaca(
                &corrente, pov, true, c, t, sq, casas, MAGIC, &mut deltas, ksq_pov);
        }
    }

    // Collapse to at most one change per feature. The active features are a
    // SET, so a feature's delta can only be -1, 0 or +1 -- any other total is a
    // move whose several events each touched the same threat.
    //
    // `Nxf6` is the plain case: the threat (knight e4 -> knight f6) is emitted
    // once as "what the departing knight attacked" and again as "who attacked
    // the captured piece". Two removals of one feature subtract its weight row
    // twice, and the accumulator drifts a little further from the truth with
    // every capture searched -- silently, because nothing downstream can tell a
    // wrong accumulator from a right one.
    deltas.sort_unstable_by_key(|d| d.idx);
    let mut i = 0;
    while i < deltas.len() {
        let idx = deltas[i].idx;
        let mut soma = 0i32;
        while i < deltas.len() && deltas[i].idx == idx {
            soma += if deltas[i].adicionar { 1 } else { -1 };
            i += 1;
        }
        if soma != 0 {
            let somar = soma > 0;
            aplica_linha(net, &mut st.acc[pov], U_THREAT + idx, somar);
            let sinal = if somar { 1i64 } else { -1 };
            for b in 0..NB {
                st.psqt[pov][b] += sinal * net.ft_threat_psqt[idx * NB + b] as i64;
            }
        }
    }
    // Pawn pairs. These were not handled here AT ALL: a plain `a2-a3` changes
    // which pawns pair with which, and the fast path walked straight past it,
    // so every pawn move left the accumulator holding the previous position's
    // pairs. Cheap to redo properly -- the feature only reads the two pawn
    // bitboards, so it is skipped entirely unless a pawn actually moved.
    let mexeu_peao = ev.iter().any(|&(_, t, _, _)| t == 0);
    if mexeu_peao {
        let mut sai = std::mem::take(&mut st.par_sai);
        let mut entra = std::mem::take(&mut st.par_entra);
        sai.clear();
        entra.clear();
        crate::sf_features::pair_delta(&antes, &agora, pov, &mut sai, &mut entra);
        for (lista, somar) in [(&sai, false), (&entra, true)] {
            for &f in lista.iter() {
                let u = U_PAIR + (f - PAIR_BASE);
                aplica_linha(net, &mut st.acc[pov], u, somar);
                let sinal = if somar { 1i64 } else { -1 };
                for b in 0..NB {
                    st.psqt[pov][b] += sinal * net.ft_pair_psqt[(f - PAIR_BASE) * NB + b] as i64;
                }
            }
        }
        st.par_sai = sai;
        st.par_entra = entra;
    }

    // features de peca: indice calculado directamente (o rei desta
    // perspectiva nao mexeu -- o chamador ja' o garantiu)
    let ksq = agora.king_sq(pov);
    for &(sq, t, c, add) in ev {
        let u = crate::sf_features::indice_peca(ksq, pov, sq, t, c);
        aplica_linha(net, &mut st.acc[pov], u, add);
        let sinal = if add { 1i64 } else { -1 };
        for b in 0..NB {
            st.psqt[pov][b] += sinal * net.ft_piece_psqt[u * NB + b] as i64;
        }
    }
    true
}

/// Poe a camada desta posicao em dia sem calcular a avaliacao.
///
/// A busca nao chama `evaluate` em todos os nos: quando a TT ja' tem a
/// avaliacao estatica desta posicao, salta-a -- e com ela saltava tambem o
/// acumulador, deixando os filhos sem pai. Medido a profundidade 14: a camada
/// do pai existia mas era de outro ramo em 56% das avaliacoes, e nessas o
/// motor reconstruia do zero.
///
/// Pagar aqui um delta (~12 linhas de pesos) evita aos filhos uma
/// reconstrucao (~35 linhas mais a enumeracao das features), e um no' que
/// chega aqui vai mesmo procurar filhos -- os cortes por TT ja' retornaram
/// antes. E' o mesmo principio do `find_last_usable_accumulator` do
/// Stockfish, sem precisar de guardar as pecas sujas de cada ply: em vez de
/// remontar a cadeia quando falta, nao a deixamos partir.
pub fn garante_camada(atk: &Attacks, board: &mut Board) {
    let net = match rede() {
        Some(n) if active() => n,
        _ => return,
    };
    ESTADO.with(|c| {
        let mut st = c.borrow_mut();
        if carrega_do_pai(&mut st, board) {
            return;
        }
        acc_incremental(net, atk, board, 0, &mut st);
        acc_incremental(net, atk, board, 1, &mut st);
        st.valido = true;
        for c in 0..2 {
            for t in 0..6 {
                st.bb[c][t] = board.pieces[c][t];
            }
        }
        guarda_na_pilha(&mut st, board);
    });
}

/// Produto interno de um vector de `u8` por um de `i8`.
///
/// O compilador resolvia isto com `vpmovsxbw`, que alarga oito bytes de cada
/// vez para os poder multiplicar em 16 bits. A `vpmaddubsw` faz exactamente
/// esta operacao -- `u8` vezes `i8`, pares somados -- sobre 32 bytes de uma so'
/// vez, e nao ha forma de a pedir sem a escrever.
///
/// Nao satura: `x` nunca passa de 127 (e' `(255*255) >> 9`) e os pesos estao em
/// [-128, 127], logo a soma de dois produtos nao passa de 32512. E' esta
/// margem que torna a instrucao utilizavel, e e' por isso que a rede e'
/// quantizada assim.
///
/// A versao generica fica como referencia e e' a que corre onde nao houver
/// AVX2 -- ao contrario do `aplica_linha`, onde escrever SIMD a mao ja' foi
/// MEDIDO como pior do que deixar o autovectorizador trabalhar.
#[inline]
fn produto_u8_i8(x: &[u8], w: &[i8]) -> i32 {
    debug_assert_eq!(x.len(), w.len());
    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    {
        if x.len() % 32 == 0 {
            return unsafe { produto_u8_i8_avx2(x, w) };
        }
    }
    let mut acc: i32 = 0;
    for (&xi, &wi) in x.iter().zip(w.iter()) {
        acc += xi as i32 * wi as i32;
    }
    acc
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
#[target_feature(enable = "avx2")]
unsafe fn produto_u8_i8_avx2(x: &[u8], w: &[i8]) -> i32 {
    use std::arch::x86_64::*;
    let uns = _mm256_set1_epi16(1);
    let mut soma = _mm256_setzero_si256();
    let n = x.len() / 32;
    for i in 0..n {
        let a = _mm256_loadu_si256(x.as_ptr().add(i * 32) as *const __m256i);
        let b = _mm256_loadu_si256(w.as_ptr().add(i * 32) as *const __m256i);
        // u8 x i8 -> pares somados em i16, depois alargados a i32 com `uns`
        let p = _mm256_maddubs_epi16(a, b);
        soma = _mm256_add_epi32(soma, _mm256_madd_epi16(p, uns));
    }
    // reduzir as oito pistas de 32 bits
    let lo = _mm256_castsi256_si128(soma);
    let hi = _mm256_extracti128_si256(soma, 1);
    let mut r = _mm_add_epi32(lo, hi);
    r = _mm_add_epi32(r, _mm_shuffle_epi32(r, 0b01_00_11_10));
    r = _mm_add_epi32(r, _mm_shuffle_epi32(r, 0b00_01_00_01));
    _mm_cvtsi128_si32(r)
}

/// Poe no estado de trabalho o acumulador do pai, quando ele existe.
///
/// Devolve `true` se a camada desta profundidade JA' e' esta posicao -- caso em
/// que nao ha nada a calcular e as duas perspectivas ja' estao prontas.
fn carrega_do_pai(st: &mut EstadoAcc, board: &Board) -> bool {
    let d = board.prof_acc;
    if d >= MAX_CAMADAS {
        return false;
    }
    if st.pilha[d].valido && st.pilha[d].bb == board.pieces {
        for pov in 0..2 {
            st.acc[pov].copy_from_slice(&st.pilha[d].acc[pov]);
        }
        st.psqt = st.pilha[d].psqt;
        st.bb = st.pilha[d].bb;
        st.valido = true;
        st.feats[0].clear();
        st.feats[1].clear();
        return true;
    }
    if d > 0 && st.pilha[d - 1].valido {
        for pov in 0..2 {
            st.acc[pov].copy_from_slice(&st.pilha[d - 1].acc[pov]);
        }
        st.psqt = st.pilha[d - 1].psqt;
        st.bb = st.pilha[d - 1].bb;
        st.valido = true;
        // As listas de features deixam de corresponder ao acumulador. Vazias e'
        // o estado que o caminho lento le' como "reconstroi", que e' o correcto
        // -- alimentar-lhe uma lista de outra posicao somava tudo duas vezes.
        st.feats[0].clear();
        st.feats[1].clear();
    }
    false
}

fn guarda_na_pilha(st: &mut EstadoAcc, board: &Board) {
    let d = board.prof_acc;
    if d >= MAX_CAMADAS {
        return;
    }
    for pov in 0..2 {
        let (origem, destino) = (&st.acc[pov], &mut st.pilha[d].acc[pov]);
        destino.copy_from_slice(origem);
    }
    st.pilha[d].psqt = st.psqt;
    st.pilha[d].bb = board.pieces;
    st.pilha[d].valido = true;
}

fn acc_incremental(
    net: &RedeSf, atk: &Attacks, board: &Board, pov: usize, st: &mut EstadoAcc,
) {
    // Caminho rapido: se a mudanca desde a ultima avaliacao e' um lance simples
    // e o rei desta perspectiva nao mexeu, os deltas saem do lance -- sem
    // enumerar as ~91 features activas nem ordenar. Cai para o caminho lento
    // (sempre correcto) em qualquer outro caso.
    static SEM_DELTA: OnceLock<bool> = OnceLock::new();
    let sem_delta = *SEM_DELTA.get_or_init(|| std::env::var_os("KESTREL_SEM_DELTA").is_some());
    // Verificacao do delta contra o refresh (`KESTREL_VERIFICA_DELTA=1`).
    // Custa uma reconstrucao por avaliacao, logo so' para depuracao -- mas e'
    // a unica forma de apanhar a posicao EXACTA onde o incremental mente, e o
    // teste das 278 nao o faz (corre a depth 1, onde o delta mal e' usado).
    static VERIFICA: OnceLock<bool> = OnceLock::new();
    let verifica = *VERIFICA.get_or_init(|| std::env::var_os("KESTREL_VERIFICA_DELTA").is_some());
    if verifica && st.valido && !sem_delta {
        if let Some(ev) = eventos_de_casa(&st.bb, &board.pieces) {
            if !rei_invalida_indices(&ev, pov, st, board) {
                let acc_antes = st.acc[pov].clone();
                let psqt_antes = st.psqt[pov];
                let bb_antes = st.bb;
                if delta_por_lance(net, board, pov, st, &ev) {
                    let acc_delta = st.acc[pov].clone();
                    let psqt_delta = st.psqt[pov];
                    st.acc[pov] = acc_antes;
                    st.psqt[pov] = psqt_antes;
                    st.bb = bb_antes;
                    st.valido = false;
                    acc_incremental(net, atk, board, pov, st);
                    if st.acc[pov] != acc_delta || st.psqt[pov] != psqt_delta {
                        // Name the feature instead of counting lanes: the
                        // difference between the two accumulators is a sum of
                        // whole weight rows, so search for the row that matches
                        // it. One hit means one feature wrongly added (or
                        // missed) -- and it says which family and which index.
                        let d: Vec<i32> = st.acc[pov]
                            .iter()
                            .zip(acc_delta.iter())
                            .map(|(c, x)| *c as i32 - *x as i32)
                            .collect();
                        let mut achou = Vec::new();
                        for u in 0..INPUT_DIM {
                            let bate_pos = (0..L1).all(|i| d[i] == linha_peso(net, u, i) as i32);
                            let bate_neg = (0..L1).all(|i| d[i] == -(linha_peso(net, u, i) as i32));
                            if bate_pos || bate_neg {
                                let fam = if u < U_THREAT { "peca" }
                                    else if u < U_PAIR { "ameaca" } else { "par" };
                                achou.push(format!(
                                    "{} {} {}", if bate_pos { "FALTA" } else { "A-MAIS" }, fam, u));
                                if achou.len() >= 4 { break; }
                            }
                        }
                        let acc_dif = st.acc[pov] != acc_delta;
                        let psqt_dif = st.psqt[pov] != psqt_delta;
                        let maxd = d.iter().map(|x| x.abs()).max().unwrap_or(0);
                        let _ = (acc_dif, psqt_dif, maxd);
                        let mut quem = String::new();
                        for a in &achou {
                            if let Some(n) = a.split_whitespace().last().and_then(|x| x.parse::<usize>().ok()) {
                                if n >= U_THREAT && n < U_PAIR {
                                    let f = n - U_THREAT;
                                    'busca: for ap in 0..6 { for ac in 0..2 { for dp in 0..6 { for dc in 0..2 {
                                        for asq in 0..64 { for dsq in 0..64 {
                                            for hm in [false, true] {
                                                let tf = crate::sf_features::get_threat_feature(
                                                    pov, ap, ac, dp, dc, asq as i32, dsq as i32, hm);
                                                if tf >= 0 && tf as usize == f {
                                                    quem = format!("p{} c{} em {} -> p{} c{} em {} hm={}",
                                                        ap, ac, asq, dp, dc, dsq, hm);
                                                    break 'busca;
                                                }
                                            }
                                        }}
                                    }}}}
                                }
                            }
                        }
                        eprintln!("DELTA-MAU pov={} fen={} ev={:?} => {:?} [{}]",
                            pov, board.to_fen(), ev, achou, quem);
                    }
                    st.feats[pov].clear();
                    return;
                }
            }
        }
    }
    if st.valido && !sem_delta {
        if let Some(ev) = eventos_de_casa(&st.bb, &board.pieces) {
            if !rei_invalida_indices(&ev, pov, st, board) && delta_por_lance(net, board, pov, st, &ev) {
                // as listas ficam desactualizadas de proposito: enquanto o
                // caminho rapido pegar, nao sao precisas (o psqt agora e'
                // acumulado). Marca-se para o caminho lento as reconstruir.
                st.feats[pov].clear();
                return;
            }
        }
    }

    let mut pecas = std::mem::take(&mut st.pecas);
    let mut novas = std::mem::take(&mut st.novas);
    let mut t = std::mem::take(&mut st.scratch_t);
    let mut pr = std::mem::take(&mut st.scratch_p);
    feats_unificadas(atk, board, pov, &mut pecas, &mut novas, &mut t, &mut pr);

    // `st.feats[pov]` empty means the fast path ran and deliberately let the
    // lists go stale -- it does not mean "no features were active". Feeding an
    // empty list to the sorted merge below made it ADD every feature of the new
    // position on top of an accumulator that already held the old ones, with
    // nothing subtracted: the accumulator silently doubled.
    //
    // This is what made a single king move poison everything after it. The fast
    // path handles king moves by declining them, so the slow path runs -- and
    // the slow path was the one that broke, not the king logic. A search with no
    // king move never takes this transition, which is exactly why it matched the
    // rebuild on all 18 positions while one `Ke1-f1` diverged from that move on.
    if !st.valido || st.feats[pov].is_empty() {
        // Refresh pela cache da casa de rei: em vez de somar as ~32 features
        // de peca do zero, partir do acumulador guardado para esta casa e
        // aplicar so' as pecas que mudaram desde entao. As ameacas e os pares
        // sao sempre somados por cima (mudam de mais para cachear).
        let ksq = board.king_sq(if pov == 0 { Color::White } else { Color::Black }) as usize;
        let idx = ksq * 2 + pov;
        let mut base_acc;
        let mut base_psqt;
        {
            let e = &st.cache[idx];
            if e.valido {
                base_acc = e.acc.clone();
                base_psqt = e.psqt;
                // diferenca de pecas contra o que gerou a entrada
                for c in 0..2 {
                    for t in 0..6 {
                        let saiu = e.bb[c][t] & !board.pieces[c][t];
                        let entrou = board.pieces[c][t] & !e.bb[c][t];
                        for (mut bb, add) in [(saiu, false), (entrou, true)] {
                            while bb != 0 {
                                let sq = bb.trailing_zeros() as usize;
                                bb &= bb - 1;
                                let u = crate::sf_features::indice_peca(ksq, pov, sq, t, c);
                                aplica_linha(net, &mut base_acc, u, add);
                                let sinal = if add { 1i64 } else { -1 };
                                for b in 0..NB {
                                    base_psqt[b] += sinal * net.ft_piece_psqt[u * NB + b] as i64;
                                }
                            }
                        }
                    }
                }
            } else {
                base_acc = net.ft_bias.clone();
                base_psqt = [0i64; NB];
                for &(f, _) in pecas.iter() {
                    aplica_linha(net, &mut base_acc, f, true);
                    for b in 0..NB {
                        base_psqt[b] += net.ft_piece_psqt[f * NB + b] as i64;
                    }
                }
            }
        }
        // guardar a entrada actualizada (so' pecas)
        {
            let e = &mut st.cache[idx];
            e.acc.clear();
            e.acc.extend_from_slice(&base_acc);
            e.psqt = base_psqt;
            for c in 0..2 {
                for t in 0..6 {
                    e.bb[c][t] = board.pieces[c][t];
                }
            }
            e.valido = true;
        }
        // ameacas e pares por cima
        st.acc[pov].copy_from_slice(&base_acc);
        st.psqt[pov] = base_psqt;
        for &u in novas.iter() {
            let u = u as usize;
            if u >= U_THREAT {
                aplica_linha(net, &mut st.acc[pov], u, true);
            }
        }
    } else {
        // Sorted merge: what is in `novas` and not in `feats` gets added,
        // what is in `feats` and not in `novas` gets subtracted. Duplicates
        // are handled by advancing both sides together.
        let velhas = &st.feats[pov];
        let (mut i, mut j) = (0usize, 0usize);
        let mut mudou = 0usize;
        let acc = &mut st.acc[pov];
        while i < velhas.len() && j < novas.len() {
            match velhas[i].cmp(&novas[j]) {
                std::cmp::Ordering::Equal => { i += 1; j += 1; }
                std::cmp::Ordering::Less => {
                    aplica_linha(net, acc, velhas[i] as usize, false);
                    i += 1; mudou += 1;
                }
                std::cmp::Ordering::Greater => {
                    aplica_linha(net, acc, novas[j] as usize, true);
                    j += 1; mudou += 1;
                }
            }
        }
        while i < velhas.len() {
            aplica_linha(net, acc, velhas[i] as usize, false); i += 1; mudou += 1;
        }
        while j < novas.len() {
            aplica_linha(net, acc, novas[j] as usize, true); j += 1; mudou += 1;
        }
        if std::env::var_os("KESTREL_SF_DIFF").is_some() {
            use std::sync::atomic::{AtomicUsize, Ordering as O};
            static N: AtomicUsize = AtomicUsize::new(0);
            static SOMA: AtomicUsize = AtomicUsize::new(0);
            static TOT: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, O::Relaxed) + 1;
            SOMA.fetch_add(mudou, O::Relaxed);
            TOT.fetch_add(novas.len(), O::Relaxed);
            if n % 200_000 == 0 {
                eprintln!("DIFF media={:.1} de {:.1} features",
                    SOMA.load(O::Relaxed) as f64 / n as f64,
                    TOT.load(O::Relaxed) as f64 / n as f64);
            }
        }
    }

    st.feats[pov].clear();
    st.feats[pov].extend_from_slice(&novas);
    st.psqt[pov] = [0i64; NB];
    for &u in novas.iter() {
        let u = u as usize;
        for b in 0..NB {
            st.psqt[pov][b] += if u < U_THREAT {
                net.ft_piece_psqt[u * NB + b] as i64
            } else if u < U_PAIR {
                net.ft_threat_psqt[(u - U_THREAT) * NB + b] as i64
            } else {
                net.ft_pair_psqt[(u - U_PAIR) * NB + b] as i64
            };
        }
    }
    st.novas = novas;
    st.pecas = pecas;
    st.scratch_t = t;
    st.scratch_p = pr;
}

// ---- Move-anchored delta ----
//
// The diff above still enumerates all ~91 features per node just to discover
// which changed. The delta is derivable from the move itself: which squares
// gained or lost a piece. This is step one -- pieces only, which are exact and
// cheap; threats follow the same event model (direct, incoming, discovered)
// but need the two board instants, so they stay on the enumerate path until
// this is proven.

/// Squares whose occupancy changed, as (square, piece, colour, added).
/// Returns None when the change is not expressible as a small set of square
/// events (a rebuild is then cheaper and always correct).
/// The squares that changed between two positions -- but ONLY when they are a
/// single legal move apart.
///
/// The old bound was "at most 6 squares", which let two or more moves through as
/// one event list. That is not a smaller version of the same problem, it is a
/// different one: the sequential replay below walks the board through `remove`
/// then `put`, and `fromTo` names the piece that moves -- both meaningless once
/// two pieces have moved. It produced event lists like four black squares at
/// once (a knight AND a bishop having moved), and the deltas built from them
/// were wrong in a way no ordering could fix.
///
/// So the shape is checked, not just the count: exactly one piece is put down,
/// and it belongs to the side that just played. Anything else falls back to the
/// full rebuild, which is always right.
fn eventos_de_casa(
    antes: &[[u64; 6]; 2], agora: &[[u64; 6]; 2],
) -> Option<Vec<(usize, usize, usize, bool)>> {
    let mut ev = Vec::with_capacity(4);
    for c in 0..2 {
        for t in 0..6 {
            let mut saiu = antes[c][t] & !agora[c][t];
            let mut entrou = agora[c][t] & !antes[c][t];
            while saiu != 0 {
                let sq = saiu.trailing_zeros() as usize;
                saiu &= saiu - 1;
                ev.push((sq, t, c, false));
                if ev.len() > 6 { return None; }
            }
            while entrou != 0 {
                let sq = entrou.trailing_zeros() as usize;
                entrou &= entrou - 1;
                ev.push((sq, t, c, true));
                if ev.len() > 6 { return None; }
            }
        }
    }
    if ev.is_empty() {
        return None;
    }
    let mut adicoes = 0;
    let mut cor_add = 2usize;
    for &(_, _, c, add) in &ev {
        if add {
            adicoes += 1;
            cor_add = c;
        }
    }
    // one piece lands (a castle lands two, and is left to the slow path), and
    // every removal is either that piece leaving or an enemy piece captured
    if adicoes != 1 {
        return None;
    }
    let saidas_proprias = ev.iter().filter(|&&(_, _, c, add)| !add && c == cor_add).count();
    if saidas_proprias != 1 || ev.len() > 3 {
        return None;
    }
    // No king moves on the fast path -- EITHER king, not just this
    // perspective's. Measured: a search with no king move reproduces the full
    // rebuild exactly (0 of 18 positions differ); insert a single `Ke1-f1` and
    // it diverges from that move on (6 of 14), identically whether the
    // perspective whose king moved rebuilds or not. So the fault is in what the
    // delta does with a king as a MOVING PIECE, on the side that keeps its
    // indices -- and it is not the piece features (the PSQT matches exactly)
    // nor the pairs. Until it is found, correctness wins: the rebuild is always
    // right, and it is what the engine does for king moves today anyway.
    // King moves are allowed back on the fast path. They were banned while the
    // fault was thought to be king logic; it was the fast->slow transition the
    // king forced, and with that fixed the ban only costs speed --
    // `rei_invalida_indices` already checks the three lookups that decide
    // whether this perspective's indices survive. Castling is still excluded
    // by the single-addition rule above.
    // And the piece that moves must actually be ABLE to make the move.
    //
    // Counting events is not enough. The stored state can be several plies away
    // from the board being evaluated -- make/unmake walks the tree, it does not
    // walk a game -- so the difference between the two positions is not always
    // one move. When it happens to be "one piece of type T left d2, one piece
    // of type T arrived on d4", the shape test passes and the sequential replay
    // runs on something that never was a move: a knight does not go d2-d4, and
    // neither does a bishop. Both showed up in the residual failures.
    let mut de = 64usize;
    let mut para = 64usize;
    let mut tipo_de = 6usize;
    let mut tipo_para = 6usize;
    for &(sq, t, c, add) in &ev {
        if c != cor_add {
            continue;
        }
        if add {
            para = sq;
            tipo_para = t;
        } else {
            de = sq;
            tipo_de = t;
        }
    }
    if de >= 64 || para >= 64 {
        return None;
    }
    // same piece, or a pawn promoting
    if tipo_de != tipo_para && !(tipo_de == 0 && tipo_para != 0) {
        return None;
    }
    let mut occ = 0u64;
    for c in 0..2 {
        for t in 0..6 {
            occ |= agora[c][t];
        }
    }
    occ &= !(1u64 << para);
    let alcanca = if tipo_de == 0 {
        let df = (de as i32 & 7) - (para as i32 & 7);
        let dr = (para as i32 >> 3) - (de as i32 >> 3);
        let frente = if cor_add == 0 { dr } else { -dr };
        df.abs() <= 1 && (frente == 1 || (frente == 2 && df == 0))
    } else {
        crate::sf_features::alcanca_pseudo(tipo_de, de, para, occ, MAGIC)
    };
    if !alcanca {
        return None;
    }
    Some(ev)
}

/// Does this perspective's piece indexing still hold after the move?
///
/// The old test was "did this side's king move" -- and that is too strong. The
/// king square does not enter the indices directly; it enters through exactly
/// three lookups, and every one of them is a step function that is constant
/// over large parts of the board:
///
///   * `ORIENT_HALFKA[ksq]`  -- the mirror applied to piece squares
///   * `KING_BUCKETS_BASE[ksq ^ flip]` -- which of the 704-row blocks is used
///   * `ORIENT_THREATS[ksq]` -- the mirror applied to threat and pair indices
///
/// If all three read the same for the old and the new square, then EVERY index
/// this perspective produces is unchanged, and the ordinary per-move delta is
/// valid exactly as it stands. Measured on the old test: 8.0% of evaluations
/// in the opening fell back to a full rebuild because the king moved, and 57.1%
/// in a pawn endgame -- of which ~83% kept the same orientation. Nearly half of
/// all evaluations in an endgame were rebuilding three feature tables from
/// scratch to arrive at the numbers they already had.
fn rei_invalida_indices(
    ev: &[(usize, usize, usize, bool)], pov: usize, st: &EstadoAcc, board: &Board,
) -> bool {
    if !ev.iter().any(|&(_, t, c, _)| t == 5 && c == pov) {
        return false;
    }
    if std::env::var_os("KESTREL_REI_ANTIGO").is_some() { return true; }
    let novo = board.king_sq(if pov == 0 { Color::White } else { Color::Black }) as usize;
    let velho = st.bb[pov][5].trailing_zeros() as usize;
    if velho >= 64 || novo >= 64 {
        return true;
    }
    let flip = if pov == 1 { 56 } else { 0 };
    ORIENT_HALFKA[velho] != ORIENT_HALFKA[novo]
        || ORIENT_THREATS[velho] != ORIENT_THREATS[novo]
        || crate::sf_features::KING_BUCKETS_BASE[velho ^ flip]
            != crate::sf_features::KING_BUCKETS_BASE[novo ^ flip]
}

#[cfg(test)]
mod testes_delta {
    use super::*;

    /// Do the per-move threat deltas reproduce the true set difference?
    ///
    /// `delta_bate_com_enumeracao` in `sf_features` already checks that ONE
    /// event produces the right removals. This checks the thing that actually
    /// runs: a whole move, whose several events are composed. The composition
    /// is where a feature touched by two events gets counted twice, and no
    /// existing test could see it -- the 278-position suite runs at depth 1,
    /// where the incremental path is barely used.
    fn deltas_do_lance(
        antes: &crate::sf_features::PosBB, agora: &crate::sf_features::PosBB, pov: usize,
        ev: &[(usize, usize, usize, bool)],
    ) -> std::collections::HashMap<usize, i32> {
        let mut casas = 0u64;
        for &(sq, _, _, _) in ev {
            casas |= 1u64 << sq;
        }
        let mut saida = Vec::new();
        let ksq_pov = agora.king_sq(pov);
        let mut cor_que_entra = 2usize;
        for &(_, _, c, add) in ev {
            if add {
                cor_que_entra = c;
            }
        }
        let mut corrente = *antes;
        for capturada in [true, false] {
            for &(sq, t, c, add) in ev {
                if !add && (c != cor_que_entra) == capturada {
                    let sem = if capturada { !0u64 } else { casas };
                    crate::sf_features::eventos_ameaca(
                        &corrente, pov, false, c, t, sq, sem, MAGIC, &mut saida, ksq_pov);
                    corrente.pieces[c][t] &= !(1u64 << sq);
                }
            }
        }
        for &(sq, t, c, add) in ev {
            if add {
                corrente.pieces[c][t] |= 1u64 << sq;
                crate::sf_features::eventos_ameaca(
                    &corrente, pov, true, c, t, sq, casas, MAGIC, &mut saida, ksq_pov);
            }
        }
        let _ = agora;
        let mut m = std::collections::HashMap::new();
        for d in &saida {
            *m.entry(d.idx).or_insert(0) += if d.adicionar { 1 } else { -1 };
        }
        m.retain(|_, v| *v != 0);
        for v in m.values_mut() {
            *v = (*v).clamp(-1, 1);
        }
        m
    }

    fn verdade(
        antes: &crate::sf_features::PosBB, agora: &crate::sf_features::PosBB, pov: usize,
    ) -> std::collections::HashMap<usize, i32> {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        crate::sf_features::threat_features(antes, pov, &mut a);
        crate::sf_features::threat_features(agora, pov, &mut b);
        let mut m = std::collections::HashMap::new();
        for f in &a {
            *m.entry(*f).or_insert(0) -= 1;
        }
        for f in &b {
            *m.entry(*f).or_insert(0) += 1;
        }
        m.retain(|_, v| *v != 0);
        m
    }

    #[test]
    fn dama_ameacada_por_nao_dama() {
    // can_slider_threat no SF: uma dama so' conta como ameacada por outra dama.
    // Se `get_threat_feature` nao filtra isso, geramos features que o SF nao tem.
    let mut fora = 0;
    let mut dentro = 0;
    for slider in [2usize, 3, 4] {
        for sc in 0..2 {
            for dc in 0..2 {
                for a in 0..64 {
                    for d in 0..64 {
                        let tf = crate::sf_features::get_threat_feature(
                            0, slider, sc, 4, dc, a as i32, d as i32, false);
                        if tf >= 0 && (tf as usize) < 59808 {
                            if slider == 4 { dentro += 1 } else { fora += 1 }
                        }
                    }
                }
            }
        }
    }
    eprintln!("dama ameacada por bispo/torre: {fora} indices validos; por dama: {dentro}");
    assert_eq!(fora, 0, "geramos ameacas a dama vindas de nao-damas que o SF filtra");
}

    #[test]
    fn delta_do_lance_bate_com_a_verdade() {
        let fens = [
            "r1bqk2r/ppp1bppp/n4n2/3Pp1B1/4N3/P7/1PP1NPPP/R2QKB1R w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "rn1qkb1r/5ppp/bpn1p3/p1ppP3/3P1P2/N1P1BN2/PP4PP/R2QKB1R w KQkq - 0 1",
            "r2qkb1r/ppp2ppp/3p1nb1/8/2nBPP2/2N5/PPP3PP/R2QK1NR w KQkq - 0 1",
            "r2qkb1r/ppp2ppp/3p1nb1/4n3/2BBP3/2N2P2/PPP3PP/R2QK1NR w KQkq - 0 1",
            "rn1qr1k1/1bppppbp/pp3np1/6B1/2PP4/2NBPN2/PP3PPP/R2Q1RK1 w - - 0 1",
        ];
        let atk = crate::attacks::Attacks::new();
        let mut mau = 0;
        let mut total = 0;
        for f in fens {
            let mut board = crate::board::Board::from_fen(f);
            let lances = crate::movegen::generate_legal(&mut board, &atk);
            for mv in &lances {
                let antes_bb = board_para_posbb(&board);
                let undo = board.make_move(mv);
                let agora_bb = board_para_posbb(&board);
                let ev = match eventos_de_casa(
                    &[antes_bb.pieces[0], antes_bb.pieces[1]],
                    &[agora_bb.pieces[0], agora_bb.pieces[1]],
                ) {
                    Some(e) => e,
                    None => {
                        board.unmake_move(mv, &undo);
                        continue;
                    }
                };
                for pov in 0..2 {
                    if rei_mexeu_teste(&ev, pov) {
                        continue;
                    }
                    total += 1;
                    let d = deltas_do_lance(&antes_bb, &agora_bb, pov, &ev);
                    let v = verdade(&antes_bb, &agora_bb, pov);
                    if d != v {
                        mau += 1;
                        if mau <= 3 {
                            let mut faltam: Vec<_> =
                                v.iter().filter(|(k, val)| d.get(k) != Some(val)).collect();
                            faltam.sort();
                            let mut sobram: Vec<_> =
                                d.iter().filter(|(k, val)| v.get(k) != Some(val)).collect();
                            sobram.sort();
                            let (mut fa, mut fb) = (Vec::new(), Vec::new());
                            crate::sf_features::threat_features(&antes_bb, pov, &mut fa);
                            crate::sf_features::threat_features(&agora_bb, pov, &mut fb);
                            for (k, _) in sobram.iter() {
                                for ap in 0..6 { for ac in 0..2 { for dp in 0..6 { for dc in 0..2 {
                                for a in 0..64 { for dsq in 0..64 {
                                    let tf = crate::sf_features::get_threat_feature(
                                        pov, ap, ac, dp, dc, a as i32, dsq as i32, false);
                                    let tf2 = crate::sf_features::get_threat_feature(
                                        pov, ap, ac, dp, dc, a as i32, dsq as i32, true);
                                    for (t, hm) in [(tf, false), (tf2, true)] {
                                        if t >= 0 && t as usize == **k {
                                            eprintln!("  idx {} = atacante(p{} c{}) em {} -> alvo(p{} c{}) em {} [hm={}]",
                                                k, ap, ac, a, dp, dc, dsq, hm);
                                        }
                                    }
                                }}}}}}
                            }
                            let onde: Vec<String> = sobram
                                .iter()
                                .map(|(k, v)| {
                                    format!(
                                        "idx {} ({:+}) antes={} agora={}",
                                        k, v, fa.contains(k), fb.contains(k)
                                    )
                                })
                                .collect();
                            eprintln!(
                                "\nlance {:?} pov={} eventos={:?}\n  verdade diz: {:?}\n  delta  diz: {:?}",
                                mv, pov, ev, faltam, onde
                            );
                        }
                    }
                }
                board.unmake_move(mv, &undo);
            }
        }
        assert_eq!(mau, 0, "{mau} de {total} (lance, perspectiva) com delta errado");
    }

    /// The same check, but over ALL three families at once.
    ///
    /// The threats-only version passes while whole moves still diverge, so the
    /// error is in one of the other two: the delta is simulated as a SET here
    /// -- start from the features of the previous position, apply exactly what
    /// `delta_por_lance` would apply, and the result must be the feature set of
    /// the new position. No network needed, and it names the wrong feature
    /// instead of leaving 1019 differing accumulator lanes to interpret.
    #[test]
    fn delta_completo_bate_com_a_verdade() {
        let fens = [
            "r2qkb1r/ppp2ppp/3p1nb1/8/2nBPP2/2N5/PPP3PP/R2QK1NR w KQkq - 0 1",
            "r2qkb1r/ppp2ppp/3p1nb1/4n3/2BBP3/2N2P2/PPP3PP/R2QK1NR w KQkq - 0 1",
            "rn1qr1k1/1bppppbp/pp3np1/6B1/2PP4/2NBPN2/PP3PPP/R2Q1RK1 w - - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        ];
        let atk = crate::attacks::Attacks::new();
        let (mut mau, mut total) = (0, 0);
        for f in fens {
            let mut board = crate::board::Board::from_fen(f);
            let lances = crate::movegen::generate_legal(&mut board, &atk);
            for mv in &lances {
                let antes_bb = board_para_posbb(&board);
                let undo = board.make_move(mv);
                let agora_bb = board_para_posbb(&board);
                let ev = match eventos_de_casa(
                    &[antes_bb.pieces[0], antes_bb.pieces[1]],
                    &[agora_bb.pieces[0], agora_bb.pieces[1]],
                ) {
                    Some(e) => e,
                    None => {
                        board.unmake_move(mv, &undo);
                        continue;
                    }
                };
                for pov in 0..2 {
                    // The engine's own condition, not "did the king move" --
                    // the whole point of `rei_invalida_indices` is that most
                    // king moves keep every index, and those DO take the delta.
                    if rei_invalida_indices_teste(&ev, pov, &antes_bb, &agora_bb) {
                        continue;
                    }
                    total += 1;
                    let mut conj: std::collections::BTreeSet<usize> =
                        conjunto_unificado(&antes_bb, pov);
                    let alvo = conjunto_unificado(&agora_bb, pov);

                    // threats, exactly as delta_por_lance composes them
                    for (idx, soma) in deltas_do_lance(&antes_bb, &agora_bb, pov, &ev) {
                        if soma > 0 {
                            conj.insert(U_THREAT + idx);
                        } else {
                            conj.remove(&(U_THREAT + idx));
                        }
                    }
                    // pairs, only when a pawn moved
                    if ev.iter().any(|&(_, t, _, _)| t == 0) {
                        let (mut pa, mut pb) = (Vec::new(), Vec::new());
                        crate::sf_features::pair_features(&antes_bb, pov, &mut pa);
                        crate::sf_features::pair_features(&agora_bb, pov, &mut pb);
                        for f in &pa {
                            if !pb.contains(f) {
                                conj.remove(&(U_PAIR + (f - PAIR_BASE)));
                            }
                        }
                        for f in &pb {
                            if !pa.contains(f) {
                                conj.insert(U_PAIR + (f - PAIR_BASE));
                            }
                        }
                    }
                    // pieces
                    let ksq = agora_bb.king_sq(pov);
                    for &(sq, t, c, add) in &ev {
                        let u = crate::sf_features::indice_peca(ksq, pov, sq, t, c);
                        if add {
                            conj.insert(u);
                        } else {
                            conj.remove(&u);
                        }
                    }

                    if conj != alvo {
                        mau += 1;
                        if mau <= 3 {
                            let fam = |u: &usize| {
                                if *u < U_THREAT { "peca" }
                                else if *u < U_PAIR { "ameaca" }
                                else { "par" }
                            };
                            let sobra: Vec<String> = conj.difference(&alvo)
                                .map(|u| format!("{} {}", fam(u), u)).collect();
                            let falta: Vec<String> = alvo.difference(&conj)
                                .map(|u| format!("{} {}", fam(u), u)).collect();
                            eprintln!("\nlance {:?} pov={} ev={:?}\n  a mais: {:?}\n  a menos: {:?}",
                                mv, pov, ev, sobra, falta);
                        }
                    }
                }
                board.unmake_move(mv, &undo);
            }
        }
        assert_eq!(mau, 0, "{mau} de {total} com o delta completo errado");
    }

    fn conjunto_unificado(
        pos: &crate::sf_features::PosBB, pov: usize,
    ) -> std::collections::BTreeSet<usize> {
        let mut out = std::collections::BTreeSet::new();
        let ksq = pos.king_sq(pov);
        for c in 0..2 {
            for t in 0..6 {
                let mut bb = pos.pieces[c][t];
                while bb != 0 {
                    let sq = bb.trailing_zeros() as usize;
                    bb &= bb - 1;
                    out.insert(crate::sf_features::indice_peca(ksq, pov, sq, t, c));
                }
            }
        }
        let mut v = Vec::new();
        crate::sf_features::threat_features(pos, pov, &mut v);
        for f in &v {
            out.insert(U_THREAT + f);
        }
        v.clear();
        crate::sf_features::pair_features(pos, pov, &mut v);
        for f in &v {
            out.insert(U_PAIR + (f - PAIR_BASE));
        }
        out
    }

    fn rei_invalida_indices_teste(
        ev: &[(usize, usize, usize, bool)], pov: usize,
        antes: &crate::sf_features::PosBB, agora: &crate::sf_features::PosBB,
    ) -> bool {
        if !ev.iter().any(|&(_, t, c, _)| t == 5 && c == pov) {
            return false;
        }
        let (velho, novo) = (antes.king_sq(pov), agora.king_sq(pov));
        if velho >= 64 || novo >= 64 {
            return true;
        }
        let flip = if pov == 1 { 56 } else { 0 };
        ORIENT_HALFKA[velho] != ORIENT_HALFKA[novo]
            || ORIENT_THREATS[velho] != ORIENT_THREATS[novo]
            || crate::sf_features::KING_BUCKETS_BASE[velho ^ flip]
                != crate::sf_features::KING_BUCKETS_BASE[novo ^ flip]
    }

    fn rei_mexeu_teste(ev: &[(usize, usize, usize, bool)], pov: usize) -> bool {
        ev.iter().any(|&(_, t, c, _)| t == 5 && c == pov)
    }
}
