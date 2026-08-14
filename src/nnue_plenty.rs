//! Reader for a reference GPL-3.0 engine's own published network shape
//! (threat inputs + pawn-pair inputs + 12 king-bucketed PSQ, 640-wide
//! accumulator, three-layer head with a skip from L1 straight to the
//! output). The trained WEIGHTS here are that engine's own -- extracted
//! from its official release binary and used as-is, on the project owner's
//! explicit instruction, exactly as GPL-3.0 permits (the license's copyleft
//! obligations attach to distributing the combined work, not to running or
//! adapting one). The FEATURE-GENERATION CODE below is our own from-scratch
//! reimplementation of the published algorithm, built and verified against
//! that engine's own `eval` UCI command as ground truth -- not copied source.
//!
//! Gated behind `KESTREL_NNUE_PLENTY`; unset, this module does nothing.

use crate::attacks::Attacks;
use crate::board::Board;
use crate::types::{Color, PieceType, Square};
use std::sync::OnceLock;

const KING_BUCKETS: usize = 12;
const L1: usize = 640;
const L2: usize = 16;
const L3: usize = 32;
const OB: usize = 8;
const FEATURE_COUNT: usize = 59808;
const PAWN_SQUARE_COUNT: i32 = 48;
const PAWN_PAIR_FEATURE_COUNT: usize = 96 * 95 / 2;
const THREAT_OFFSET: usize = PAWN_PAIR_FEATURE_COUNT;
const OTHER_COUNT: usize = PAWN_PAIR_FEATURE_COUNT + FEATURE_COUNT;
const INPUT_QUANT: i32 = 255;
const L1_QUANT: i32 = 64;
const INPUT_SHIFT: i32 = 9;
const NETWORK_SCALE: f32 = 287.0;
const L1_NORMALISATION: f32 =
    (1i32 << INPUT_SHIFT) as f32 / (INPUT_QUANT * INPUT_QUANT * L1_QUANT) as f32;

#[rustfmt::skip]
const KING_BUCKET_LAYOUT: [u8; 64] = [
    0,1,2,3,3,2,1,0,
    4,5,6,7,7,6,5,4,
    8,8,9,9,9,9,8,8,
    10,10,10,10,10,10,10,10,
    11,11,11,11,11,11,11,11,
    11,11,11,11,11,11,11,11,
    11,11,11,11,11,11,11,11,
    11,11,11,11,11,11,11,11,
];

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

struct Tables {
    /// [piece][color][origin] -> cumulative offset before this origin,
    /// within that piece/color's own feature block.
    piece_offset: Vec<Vec<Vec<i32>>>,
    /// [piece][color][origin][target] -> rank of `target` among the
    /// squares this piece/color/origin can reach on an empty board.
    attack_index: Vec<Vec<Vec<Vec<i32>>>>,
    /// (excluded, semi_excluded, base_feature) by
    /// (attacking_piece, attacking_color, attacked_piece, attacked_color).
    pair_lookup: Vec<Vec<Vec<Vec<(bool, bool, i32)>>>>,
}

fn build_tables() -> Tables {
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
    debug_assert_eq!(running as usize, FEATURE_COUNT);

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

    Tables { piece_offset, attack_index, pair_lookup }
}

static TABLES: OnceLock<Tables> = OnceLock::new();
fn tables() -> &'static Tables {
    TABLES.get_or_init(build_tables)
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
    let t = tables();
    let (excluded, semi_excluded, base) = t.pair_lookup[attacking_piece][a_c][attacked_piece][d_c];
    if is_pair_excluded(excluded, semi_excluded, a_sq, d_sq) {
        return FEATURE_COUNT as i32;
    }
    base + t.piece_offset[attacking_piece][a_c][a_sq as usize]
        + t.attack_index[attacking_piece][a_c][a_sq as usize][d_sq as usize]
}

#[inline]
fn get_piece_feature(piece: usize, relative_square: i32, relative_color: i32, king_bucket: u8) -> usize {
    (768 * king_bucket as i32 + 384 * relative_color + 64 * piece as i32 + relative_square) as usize
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
    mask
}

#[inline]
fn pawn_pair_index(id_a: i32, id_b: i32) -> usize {
    let (lo, hi) = if id_a < id_b { (id_a, id_b) } else { (id_b, id_a) };
    (hi * (hi - 1) / 2 + lo) as usize
}

#[inline]
fn pawn_id(square: usize, enemy_offset: i32, square_flip: usize) -> i32 {
    enemy_offset + (square ^ square_flip) as i32 - 8
}

fn add_piece_features(board: &Board, pov: usize, feats: &mut Vec<usize>) {
    let king_sq = board.king_sq(if pov == 0 { Color::White } else { Color::Black }) as usize;
    let kb = KING_BUCKET_LAYOUT[king_sq ^ (if pov == 1 { 56 } else { 0 })];
    let hm = (king_sq & 7) >= 4;
    for side in 0..2 {
        let color = if side == 0 { Color::White } else { Color::Black };
        for (piece_idx, &pt) in PIECE_TYPES.iter().enumerate() {
            let mut bb = board.pieces[color as usize][pt as usize];
            while bb != 0 {
                let sq = bb.trailing_zeros() as usize;
                bb &= bb - 1;
                let s = sq ^ (if hm { 7 } else { 0 });
                let rel = (s ^ (if pov == 1 { 56 } else { 0 })) as i32;
                let rel_color = if side != pov { 1 } else { 0 };
                feats.push(get_piece_feature(piece_idx, rel, rel_color, kb));
            }
        }
    }
}

fn add_pawn_pair_features(board: &Board, pov: usize, feats: &mut Vec<usize>) {
    let king_sq = board.king_sq(if pov == 0 { Color::White } else { Color::Black }) as usize;
    let mirrored = (king_sq & 7) >= 4;
    let square_flip = (if mirrored { 7 } else { 0 }) ^ (if pov == 1 { 56 } else { 0 });

    let friendly_color = if pov == 0 { Color::White } else { Color::Black };
    let enemy_color = friendly_color.opp();
    let friendly = board.pieces[friendly_color as usize][PieceType::Pawn as usize];
    let enemy = board.pieces[enemy_color as usize][PieceType::Pawn as usize];

    let mut squares = |mut bb: u64| -> Vec<usize> {
        let mut v = Vec::with_capacity(8);
        while bb != 0 {
            v.push(bb.trailing_zeros() as usize);
            bb &= bb - 1;
        }
        v
    };
    let friendly_list = squares(friendly);
    let enemy_list = squares(enemy);

    // Friendly-friendly
    for (i, &s) in friendly_list.iter().enumerate() {
        let id_a = pawn_id(s, 0, square_flip);
        let mask = pp_mask(s);
        for &s2 in &friendly_list[i + 1..] {
            if mask & (1u64 << s2) != 0 {
                feats.push(pawn_pair_index(id_a, pawn_id(s2, 0, square_flip)));
            }
        }
    }
    // Friendly-enemy
    for &s in &friendly_list {
        let id_a = pawn_id(s, 0, square_flip);
        let mask = pp_mask(s);
        for &s2 in &enemy_list {
            if mask & (1u64 << s2) != 0 {
                feats.push(pawn_pair_index(id_a, pawn_id(s2, PAWN_SQUARE_COUNT, square_flip)));
            }
        }
    }
    // Enemy-enemy
    for (i, &s) in enemy_list.iter().enumerate() {
        let id_a = pawn_id(s, PAWN_SQUARE_COUNT, square_flip);
        let mask = pp_mask(s);
        for &s2 in &enemy_list[i + 1..] {
            if mask & (1u64 << s2) != 0 {
                feats.push(pawn_pair_index(id_a, pawn_id(s2, PAWN_SQUARE_COUNT, square_flip)));
            }
        }
    }
}

fn add_threat_features(atk: &Attacks, board: &Board, pov: usize, feats: &mut Vec<usize>) {
    let king_sq = board.king_sq(if pov == 0 { Color::White } else { Color::Black }) as usize;
    let hm = (king_sq & 7) >= 4;
    for side in 0..2 {
        let color = if side == 0 { Color::White } else { Color::Black };
        for (piece_idx, &pt) in PIECE_TYPES.iter().enumerate() {
            let mut bb = board.pieces[color as usize][pt as usize];
            while bb != 0 {
                let index_sq = bb.trailing_zeros() as usize;
                bb &= bb - 1;
                let mut attackers = crate::search::see::attackers_to(atk, board, index_sq as Square, board.occ_all);
                while attackers != 0 {
                    let a_sq = attackers.trailing_zeros() as usize;
                    attackers &= attackers - 1;
                    let (ap, ac) = board
                        .piece_at(a_sq as Square)
                        .expect("attacker square must hold a piece");
                    let tf = get_threat_feature(
                        pov,
                        ap as usize,
                        ac as usize,
                        piece_idx,
                        side,
                        a_sq as i32,
                        index_sq as i32,
                        hm,
                    );
                    if (tf as usize) < FEATURE_COUNT {
                        feats.push(THREAT_OFFSET + tf as usize);
                    }
                }
            }
        }
    }
}

pub struct RedePlenty {
    psq: Vec<i16>,       // [768*KING_BUCKETS][L1], row-major
    other: Vec<i8>,      // [OTHER_COUNT][L1], row-major
    bias: Vec<i16>,      // [L1]
    l1w: Vec<i8>,        // [OB][L1][L2], detransposed to natural (ft,l2) order
    l1b: Vec<f32>,       // [OB][L2]
    l2w: Vec<f32>,       // [OB][2*L2][L3]
    l2b: Vec<f32>,       // [OB][L3]
    l3w: Vec<f32>,       // [OB][L3+2*L2]
    l3b: Vec<f32>,       // [OB]
}

fn align64(x: usize) -> usize {
    (x + 63) / 64 * 64
}

/// Un-shuffles the AVX2 `dpbusd`-ready weight layout the file stores back
/// into plain (feature, output) order. Every group of 4 consecutive
/// features has its L2_SIZE columns split into two 8-wide halves, and
/// within each half the 4 features are interleaved byte-by-byte -- that is
/// what a `vpdpbusd` lane actually consumes, and it is not the same order
/// a naive `feature * L2 + output` read would give. Found by comparing this
/// engine's own instrumented build against a naive reader byte for byte;
/// see the session notes, not restated here.
fn detranspose_l1(flat: &[i8]) -> Vec<i8> {
    let mut nat = vec![0i8; L1 * L2];
    for f_base in (0..L1).step_by(4) {
        for half in 0..2 {
            for lane in 0..8 {
                let base = f_base * L2 + half * 32 + lane * 4;
                let l2 = half * 8 + lane;
                for p in 0..4 {
                    nat[(f_base + p) * L2 + l2] = flat[base + p];
                }
            }
        }
    }
    nat
}

fn carrega(bytes: &[u8]) -> Option<RedePlenty> {
    let mut off = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let start = align64(off);
        let end = start + n;
        if end > bytes.len() {
            return None;
        }
        off = end;
        Some(&bytes[start..end])
    };

    let psq_bytes = take(768 * KING_BUCKETS * L1 * 2)?;
    let other_bytes = take(OTHER_COUNT * L1)?;
    let bias_bytes = take(L1 * 2)?;
    let l1w_bytes = take(OB * L1 * L2)?;
    let l1b_bytes = take(OB * L2 * 4)?;
    let l2w_bytes = take(OB * 2 * L2 * L3 * 4)?;
    let l2b_bytes = take(OB * L3 * 4)?;
    let l3w_bytes = take(OB * (L3 + 2 * L2) * 4)?;
    let l3b_bytes = take(OB * 4)?;

    // The struct's own alignment (64, its most-aligned member) rounds
    // `sizeof(NetworkData)` up past the last real field -- trailing padding,
    // not a parsing error.
    if bytes.len() < off || bytes.len() > align64(off) {
        eprintln!(
            "nnue-plenty: {} bytes a mais/a menos no fim (esperava {}..={})",
            bytes.len() as i64 - off as i64,
            off,
            align64(off)
        );
        return None;
    }

    let read_i16 = |b: &[u8]| -> Vec<i16> {
        b.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect()
    };
    let read_i8 = |b: &[u8]| -> Vec<i8> { b.iter().map(|&x| x as i8).collect() };
    let read_f32 = |b: &[u8]| -> Vec<f32> {
        b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
    };

    let l1w_raw = read_i8(l1w_bytes);
    let mut l1w = Vec::with_capacity(OB * L1 * L2);
    for b in 0..OB {
        l1w.extend(detranspose_l1(&l1w_raw[b * L1 * L2..(b + 1) * L1 * L2]));
    }

    eprintln!("nnue-plenty: rede carregada ({} bytes)", bytes.len());
    Some(RedePlenty {
        psq: read_i16(psq_bytes),
        other: read_i8(other_bytes),
        bias: read_i16(bias_bytes),
        l1w,
        l1b: read_f32(l1b_bytes),
        l2w: read_f32(l2w_bytes),
        l2b: read_f32(l2b_bytes),
        l3w: read_f32(l3w_bytes),
        l3b: read_f32(l3b_bytes),
    })
}

fn build_accumulator(atk: &Attacks, board: &Board, net: &RedePlenty, pov: usize) -> [i32; L1] {
    let mut piece_feats = Vec::with_capacity(32);
    add_piece_features(board, pov, &mut piece_feats);
    let mut other_feats = Vec::with_capacity(96);
    add_pawn_pair_features(board, pov, &mut other_feats);
    add_threat_features(atk, board, pov, &mut other_feats);

    let mut acc = [0i32; L1];
    for k in 0..L1 {
        acc[k] = net.bias[k] as i32;
    }
    for f in piece_feats {
        let row = &net.psq[f * L1..(f + 1) * L1];
        for k in 0..L1 {
            acc[k] += row[k] as i32;
        }
    }
    for f in other_feats {
        let row = &net.other[f * L1..(f + 1) * L1];
        for k in 0..L1 {
            acc[k] += row[k] as i32;
        }
    }
    acc
}

/// SCReLU-pairwise activation, byte for byte matching the AVX2
/// `_mm256_packus_epi16` lane-crossing shuffle: within every 32-value
/// block the middle two 8-wide sub-blocks swap. The weight table is
/// written assuming that exact order.
fn pairwise_activate(acc: &[i32; L1]) -> [i32; L1 / 2] {
    let half = L1 / 2;
    let inverse_shift = 16 - INPUT_SHIFT;
    let mut vals = [0i32; L1 / 2];
    for i in 0..half {
        let a = acc[i].clamp(0, INPUT_QUANT);
        let b = acc[i + half].clamp(0, INPUT_QUANT);
        let shifted = (a as i64) << inverse_shift;
        let prod = (shifted * b as i64) >> 16;
        vals[i] = prod.clamp(0, 255) as i32;
    }
    let mut out = [0i32; L1 / 2];
    let mut base = 0;
    while base < half {
        out[base..base + 8].copy_from_slice(&vals[base..base + 8]);
        out[base + 8..base + 16].copy_from_slice(&vals[base + 16..base + 24]);
        out[base + 16..base + 24].copy_from_slice(&vals[base + 8..base + 16]);
        out[base + 24..base + 32].copy_from_slice(&vals[base + 24..base + 32]);
        base += 32;
    }
    out
}

pub fn evaluate(net: &RedePlenty, atk: &Attacks, board: &mut Board) -> i32 {
    let stm = board.side as usize;
    let nstm = 1 - stm;

    let stm_acc = build_accumulator(atk, board, net, stm);
    let nstm_acc = build_accumulator(atk, board, net, nstm);
    let stm_pw = pairwise_activate(&stm_acc);
    let nstm_pw = pairwise_activate(&nstm_acc);
    let mut pairwise = [0i32; L1];
    pairwise[..L1 / 2].copy_from_slice(&stm_pw);
    pairwise[L1 / 2..].copy_from_slice(&nstm_pw);

    let n = board.occ_all.count_ones() as i32;
    let divisor = (32 + OB as i32 - 1) / OB as i32;
    let bucket = ((n - 2) / divisor).clamp(0, OB as i32 - 1) as usize;

    let mut l1_matmul = [0i64; L2];
    let bucket_l1w = &net.l1w[bucket * L1 * L2..(bucket + 1) * L1 * L2];
    for ft in 0..L1 {
        let p = pairwise[ft];
        if p == 0 {
            continue;
        }
        let row = &bucket_l1w[ft * L2..(ft + 1) * L2];
        for l2 in 0..L2 {
            l1_matmul[l2] += p as i64 * row[l2] as i64;
        }
    }

    let l1_bias = &net.l1b[bucket * L2..(bucket + 1) * L2];
    let mut l1_out = [0f32; 2 * L2];
    for l2 in 0..L2 {
        let r = l1_matmul[l2] as f32 * L1_NORMALISATION + l1_bias[l2];
        l1_out[l2] = r.clamp(0.0, 1.0);
        l1_out[L2 + l2] = (r * r).clamp(0.0, 1.0);
    }

    let l2_bias = &net.l2b[bucket * L3..(bucket + 1) * L3];
    let l2w_bucket = &net.l2w[bucket * 2 * L2 * L3..(bucket + 1) * 2 * L2 * L3];
    let mut l2_out = [0f32; L3];
    l2_out.copy_from_slice(l2_bias);
    for l1 in 0..2 * L2 {
        let v = l1_out[l1];
        if v == 0.0 {
            continue;
        }
        let row = &l2w_bucket[l1 * L3..(l1 + 1) * L3];
        for l3 in 0..L3 {
            l2_out[l3] += v * row[l3];
        }
    }
    for v in l2_out.iter_mut() {
        let c = v.clamp(0.0, 1.0);
        *v = c * c;
    }

    let l3w_bucket = &net.l3w[bucket * (L3 + 2 * L2)..(bucket + 1) * (L3 + 2 * L2)];
    let mut result = net.l3b[bucket];
    for l3 in 0..L3 {
        result += l2_out[l3] * l3w_bucket[l3];
    }
    for l1 in 0..2 * L2 {
        result += l1_out[l1] * l3w_bucket[L3 + l1];
    }

    (result * NETWORK_SCALE) as i32
}

static REDE: OnceLock<Option<RedePlenty>> = OnceLock::new();

pub fn rede() -> Option<&'static RedePlenty> {
    REDE.get_or_init(|| {
        let path = std::env::var("KESTREL_NNUE_PLENTY").ok()?;
        let bytes = std::fs::read(&path).ok()?;
        carrega(&bytes)
    })
    .as_ref()
}

pub fn active() -> bool {
    rede().is_some()
}
