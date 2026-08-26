//! Stockfish SFNNv16 input features, over plain bitboards.
//!
//! HalfKAv2_hm (22528) + Full_Threats (59808) + PP_3Wide (4560) = 86896.
//!
//! This is deliberately free of any Kestrel type so the exact same file can
//! be dropped into the trainer. A network is welded to the feature mapping it
//! was trained with, and two hand-kept copies of a mapping always drift apart
//! without saying so -- the engine reader and the trainer must share this one.
//!
//! Indexing verified byte-exact against Stockfish's own evaluation via
//! `nnue_sf.rs`, which is a thin wrapper over these functions.

pub const PIECE_DIM: usize = 22528; // HalfKAv2_hm
pub const THREAT_DIM: usize = 59808; // Full_Threats
pub const PAIR_DIM: usize = 4560; // PP_3Wide (96*95/2)
pub const PAIR_BASE: usize = THREAT_DIM; // pawn pairs sit AFTER threats
pub const INPUT_DIM: usize = PIECE_DIM + THREAT_DIM + PAIR_DIM; // 86896

/// Bitboards indexed `[color][piece_type]`, piece types in the order
/// pawn, knight, bishop, rook, queen, king; colour 0 = white.
#[derive(Clone, Copy, Debug, Default)]
pub struct PosBB {
    pub pieces: [[u64; 6]; 2],
}

impl PosBB {
    pub fn occ(&self) -> u64 {
        let mut o = 0;
        for c in 0..2 {
            for p in 0..6 {
                o |= self.pieces[c][p];
            }
        }
        o
    }

    pub fn king_sq(&self, color: usize) -> usize {
        let bb = self.pieces[color][5];
        if bb == 0 { 64 } else { bb.trailing_zeros() as usize }
    }

    /// Piece type and colour on a square, if any.
    pub fn piece_at(&self, sq: usize) -> Option<(usize, usize)> {
        let bit = 1u64 << sq;
        for c in 0..2 {
            for p in 0..6 {
                if self.pieces[c][p] & bit != 0 {
                    return Some((p, c));
                }
            }
        }
        None
    }
}

// ---- attack generation (self-contained; no engine tables) ----

const FILE_A: u64 = 0x0101_0101_0101_0101;
const FILE_H: u64 = FILE_A << 7;

#[inline]
fn shift(bb: u64, d: i32) -> u64 {
    // Shifts that would wrap around a file are masked before moving.
    match d {
        1 => (bb & !FILE_H) << 1,
        -1 => (bb & !FILE_A) >> 1,
        8 => bb << 8,
        -8 => bb >> 8,
        9 => (bb & !FILE_H) << 9,
        7 => (bb & !FILE_A) << 7,
        -7 => (bb & !FILE_H) >> 7,
        -9 => (bb & !FILE_A) >> 9,
        _ => 0,
    }
}

/// Squares a pawn of `color` standing on `sq` attacks.
pub fn pawn_attacks_from(color: usize, sq: usize) -> u64 {
    let b = 1u64 << sq;
    if color == 0 { shift(b, 9) | shift(b, 7) } else { shift(b, -7) | shift(b, -9) }
}

pub fn knight_attacks(sq: usize) -> u64 {
    let b = 1u64 << sq;
    let l1 = (b & !FILE_A) >> 1;
    let l2 = (b & !(FILE_A | (FILE_A << 1))) >> 2;
    let r1 = (b & !FILE_H) << 1;
    let r2 = (b & !(FILE_H | (FILE_H >> 1))) << 2;
    let h1 = l1 | r1;
    let h2 = l2 | r2;
    (h1 << 16) | (h1 >> 16) | (h2 << 8) | (h2 >> 8)
}

pub fn king_attacks(sq: usize) -> u64 {
    let b = 1u64 << sq;
    shift(b, 1) | shift(b, -1) | shift(b, 8) | shift(b, -8)
        | shift(b, 9) | shift(b, 7) | shift(b, -7) | shift(b, -9)
}

fn ray_attacks(sq: usize, occ: u64, dirs: &[i32]) -> u64 {
    let mut out = 0u64;
    for &d in dirs {
        let mut b = 1u64 << sq;
        loop {
            b = shift(b, d);
            if b == 0 {
                break;
            }
            out |= b;
            if occ & b != 0 {
                break;
            }
        }
    }
    out
}

pub fn bishop_attacks(sq: usize, occ: u64) -> u64 {
    ray_attacks(sq, occ, &[9, 7, -7, -9])
}

pub fn rook_attacks(sq: usize, occ: u64) -> u64 {
    ray_attacks(sq, occ, &[1, -1, 8, -8])
}

/// Slider attack generators. The loop-based rays below are portable and need
/// no tables, which is what the trainer wants; a caller that already has magic
/// bitboards should inject them -- profiling put the ray loops at ~10% of
/// search time.
#[derive(Clone, Copy)]
pub struct Deslizantes {
    pub bispo: fn(usize, u64) -> u64,
    pub torre: fn(usize, u64) -> u64,
}

pub const RAIOS: Deslizantes = Deslizantes { bispo: bishop_attacks, torre: rook_attacks };

/// Every piece of either colour that attacks `sq`, given occupancy `occ`.
pub fn attackers_to_com(pos: &PosBB, sq: usize, occ: u64, d: Deslizantes) -> u64 {
    let b = (d.bispo)(sq, occ);
    let r = (d.torre)(sq, occ);
    attackers_to_impl(pos, sq, b, r)
}

pub fn attackers_to(pos: &PosBB, sq: usize, occ: u64) -> u64 {
    attackers_to_com(pos, sq, occ, RAIOS)
}

#[inline]
fn attackers_to_impl(pos: &PosBB, sq: usize, b: u64, r: u64) -> u64 {
    // A white pawn attacks `sq` exactly when a black pawn on `sq` would
    // attack it -- hence the reversed colour on each pawn term.
    (pawn_attacks_from(1, sq) & pos.pieces[0][0])
        | (pawn_attacks_from(0, sq) & pos.pieces[1][0])
        | (knight_attacks(sq) & (pos.pieces[0][1] | pos.pieces[1][1]))
        | (b & (pos.pieces[0][2] | pos.pieces[1][2] | pos.pieces[0][4] | pos.pieces[1][4]))
        | (r & (pos.pieces[0][3] | pos.pieces[1][3] | pos.pieces[0][4] | pos.pieces[1][4]))
        | (king_attacks(sq) & (pos.pieces[0][5] | pos.pieces[1][5]))
}

// ---- orientation / bucket tables (transcribed from Stockfish) ----

#[rustfmt::skip]
pub const KING_BUCKETS_BASE: [i32; 64] = [
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
pub const ORIENT_HALFKA: [i32; 64] = {
    // HalfKAv2_hm::OrientTBL -- files a-d flip by 7 so the king lands on e-h.
    let h = 7; let a = 0;
    [
    h,h,h,h, a,a,a,a,  h,h,h,h, a,a,a,a,
    h,h,h,h, a,a,a,a,  h,h,h,h, a,a,a,a,
    h,h,h,h, a,a,a,a,  h,h,h,h, a,a,a,a,
    h,h,h,h, a,a,a,a,  h,h,h,h, a,a,a,a,
    ]
};

#[rustfmt::skip]
pub const ORIENT_THREATS: [i32; 64] = {
    // FullThreats/PP_3Wide::OrientTBL -- the OPPOSITE sense from HalfKAv2_hm.
    // Kept as its own table on purpose: conflating the two cost real time.
    let h = 7; let a = 0;
    [
    a,a,a,a, h,h,h,h,  a,a,a,a, h,h,h,h,
    a,a,a,a, h,h,h,h,  a,a,a,a, h,h,h,h,
    a,a,a,a, h,h,h,h,  a,a,a,a, h,h,h,h,
    a,a,a,a, h,h,h,h,  a,a,a,a, h,h,h,h,
    ]
};

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

/// PieceSquareIndex plane for a piece type, relative to the perspective.
#[inline]
fn piece_plane(piece: usize, relative_enemy: bool) -> i32 {
    if piece == 5 {
        return 10; // both kings share the king plane
    }
    let base = (piece as i32) * 2;
    if relative_enemy { base + 1 } else { base }
}

fn pseudo_attacks_empty(piece: usize, origin: usize, color: usize) -> u64 {
    match piece {
        0 => {
            if (8..56).contains(&origin) { pawn_attacks_from(color, origin) } else { 0 }
        }
        1 => knight_attacks(origin),
        5 => king_attacks(origin),
        2 => bishop_attacks(origin, 0),
        3 => rook_attacks(origin, 0),
        _ => bishop_attacks(origin, 0) | rook_attacks(origin, 0),
    }
}

/// Tabelas de indexacao das ameacas, achatadas.
///
/// A versao anterior era `Vec<Vec<Vec<Vec<i32>>>>`: ler uma entrada custava
/// quatro saltos de ponteiro encadeados, nenhum deles capaz de comecar antes
/// do anterior chegar da memoria, sobre 768 alocacoes de 256 bytes espalhadas
/// pelo heap. Em C ninguem escreveria isto -- escrever-se-ia
/// `int attack_index[6][2][64][64]`, contiguo. A forma "idiomatica" em Rust
/// era, aqui, a pior escolha possivel de layout, e esta funcao corre ~90 vezes
/// por reconstrucao do acumulador.
///
/// Agora sao dois blocos contiguos, com o `piece_offset` ja' somado dentro do
/// `attack_index` -- o que era duas leituras dispersas passa a ser uma.
pub struct ThreatTables {
    /// [peca][cor][origem][alvo], 6*2*64*64 = 49152 entradas. Ja' inclui o
    /// `piece_offset` da origem.
    ///
    /// `i16` e nao `i32`: o maior valor possivel e' a soma dos ataques da dama
    /// sobre as 64 origens, ~1400, muito dentro de 32767. Metade da largura
    /// significa 96 KiB em vez de 192 -- o dobro da tabela por linha de cache,
    /// e o conjunto todo cabe folgado na L2.
    attack_index: Box<[i16]>,
    /// [atacante][cor_at][alvo][cor_alvo], 6*2*6*2 = 144 entradas.
    ///
    /// Os dois booleanos vivem nos dois bits de baixo do proprio inteiro: a
    /// tupla `(bool, bool, i32)` gastava 8 bytes por causa do alinhamento, e
    /// esta forma gasta 4. A base cabe: o maximo e' `THREAT_DIM` (59808), que
    /// deslocado 2 bits ainda e' um quarto de milhao.
    pair_lookup: Box<[i32]>,
}

/// Empacota (excluido, semi-excluido, base) num so' inteiro.
#[inline(always)]
const fn empacota_par(excluido: bool, semi: bool, base: i32) -> i32 {
    (base << 2) | ((semi as i32) << 1) | (excluido as i32)
}

#[inline(always)]
const fn desempacota_par(v: i32) -> (bool, bool, i32) {
    ((v & 1) != 0, (v & 2) != 0, v >> 2)
}

#[inline(always)]
const fn idx_ataque(peca: usize, cor: usize, origem: usize, alvo: usize) -> usize {
    ((peca * 2 + cor) * 64 + origem) * 64 + alvo
}

#[inline(always)]
const fn idx_par(ap: usize, ac: usize, tp: usize, tc: usize) -> usize {
    ((ap * 2 + ac) * 6 + tp) * 2 + tc
}

fn build_threat_tables() -> ThreatTables {
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
                    cum += pseudo_attacks_empty(piece, origin, color).count_ones() as i32;
                }
            }
            cumulative_piece_offset[piece][color] = cum;
            cumulative_offset[piece][color] = running;
            running += PIECE_TARGET_COUNT[piece] * cum;
        }
    }
    debug_assert_eq!(running as usize, THREAT_DIM);

    // O `piece_offset` da origem entra aqui: a linha inteira nasce com ele, e
    // os alvos atacados somam-lhe a contagem. Uma leitura em vez de duas.
    let mut attack_index = vec![0i16; 6 * 2 * 64 * 64].into_boxed_slice();
    for color in 0..2 {
        for piece in 0..6 {
            for origin in 0..64 {
                let base = piece_offset[piece][color][origin] as i16;
                let ini = idx_ataque(piece, color, origin, 0);
                for v in attack_index[ini..ini + 64].iter_mut() {
                    *v = base;
                }
                let a = pseudo_attacks_empty(piece, origin, color);
                let mut m = a;
                while m != 0 {
                    let target = m.trailing_zeros() as usize;
                    m &= m - 1;
                    let below = a & ((1u64 << target) - 1);
                    attack_index[ini + target] += below.count_ones() as i16;
                }
            }
        }
    }

    let mut pair_lookup = vec![empacota_par(true, false, 0); 6 * 2 * 6 * 2].into_boxed_slice();
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
                    pair_lookup[idx_par(ap, ac, tp, tc)] =
                        empacota_par(excluded, semi_excluded, feature_base);
                }
            }
        }
    }
    ThreatTables { attack_index, pair_lookup }
}

static THREAT_TABLES: std::sync::OnceLock<ThreatTables> = std::sync::OnceLock::new();
pub fn threat_tables() -> &'static ThreatTables {
    THREAT_TABLES.get_or_init(build_threat_tables)
}

#[inline]
fn is_pair_excluded(excluded: bool, semi_excluded: bool, attacking_sq: i32, attacked_sq: i32) -> bool {
    let less_than = if attacking_sq < attacked_sq { 1u8 } else { 0 };
    let data = ((semi_excluded && !excluded) as u8) | ((excluded as u8) << 1);
    ((data.wrapping_add(less_than)) & 2) != 0
}

#[allow(clippy::too_many_arguments)]
/// Invólucro sobre `get_threat_feature_t`. Tinha cinco variaveis locais
/// calculadas e NUNCA usadas (a chamada passa os parametros originais) e nao
/// estava marcado como `inline` -- so' o salto valia 6,9% do
/// `delta_por_lance`. Agora e' o que sempre foi: um reencaminhamento.
#[inline(always)]
pub fn get_threat_feature(
    pov: usize,
    attacking_piece: usize,
    attacking_color: usize,
    attacked_piece: usize,
    attacked_color: usize,
    attacking_square: i32,
    attacked_square: i32,
    mirrored: bool,
) -> i32 {
    get_threat_feature_t(threat_tables(), pov, attacking_piece, attacking_color,
        attacked_piece, attacked_color, attacking_square, attacked_square, mirrored)
}

/// Este par de tipos/cores esta' excluido em AMBAS as perspectivas?
///
/// `excluded` sai de `PIECE_INTERACTION_MAP[atacante][alvo] < 0`, que so'
/// depende dos tipos de peca. As cores entram no indice ja' rodadas pela
/// perspectiva (`cor ^ pov`), mas o mapa nao as consulta -- logo o valor e' o
/// mesmo visto dos dois lados. E' isso que permite saltar estes pares nas duas
/// listas ao mesmo tempo sem lhes perder o alinhamento.
///
/// O que NAO e' invariante e' o `semi_excluded`: depende de `a_sq < d_sq` em
/// coordenadas ja' viradas pela perspectiva, e e' de la' que vem a assimetria.
/// Medido no bench: 39,4% das ameacas caiam no dustbin, e so' 1,06% eram
/// assimetricas -- ou seja 97% do dustbin nao era preciso para nada.
#[inline]
pub fn par_excluido_dos_dois_lados(pov: usize, ap: usize, ac: usize, tp: usize, tc: usize) -> bool {
    let t = threat_tables();
    let (excluido, _, _) =
        desempacota_par(t.pair_lookup[idx_par(ap, ac ^ pov, tp, tc ^ pov)]);
    excluido
}

/// Variante com a tabela ja' na mao, para quem chama isto em ciclo: poupa um
/// `OnceLock::get_or_init` (carga atomica mais salto) por chamada, e a
/// enumeracao das ameacas chama-o dezenas de vezes por posicao.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn get_threat_feature_t(
    t: &ThreatTables,
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
    // Indices por construcao dentro dos limites: peca < 6, cor < 2, casas < 64.
    // Os `debug_assert` guardam-no nos testes; em release nao ha verificacao,
    // que e' o que separa este acesso do equivalente em C.
    debug_assert!(attacking_piece < 6 && attacked_piece < 6);
    debug_assert!((0..64).contains(&a_sq) && (0..64).contains(&d_sq));
    let (excluded, semi_excluded, base) = desempacota_par(unsafe {
        *t.pair_lookup
            .get_unchecked(idx_par(attacking_piece, a_c, attacked_piece, d_c))
    });
    if is_pair_excluded(excluded, semi_excluded, a_sq, d_sq) {
        return THREAT_DIM as i32;
    }
    base + unsafe {
        *t.attack_index
            .get_unchecked(idx_ataque(attacking_piece, a_c, a_sq as usize, d_sq as usize))
    } as i32
}

const fn pp_mask_calc(sq: usize) -> u64 {
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

/// Mascara das casas que emparelham com cada casa, calculada na compilacao.
/// Era recalculada dentro do ciclo, uma vez por peao e por chamada.
static PP_MASK: [u64; 64] = {
    let mut t = [0u64; 64];
    let mut i = 0;
    while i < 64 {
        t[i] = pp_mask_calc(i);
        i += 1;
    }
    t
};

#[inline]
fn pawn_pair_index(id_a: i32, id_b: i32) -> usize {
    let (lo, hi) = if id_a < id_b { (id_a, id_b) } else { (id_b, id_a) };
    (hi * (hi - 1) / 2 + lo) as usize
}

#[inline]
fn pawn_id(square: usize, color_offset: i32, square_flip: usize) -> i32 {
    color_offset + (square ^ square_flip) as i32 - 8
}

// ---- the three feature families ----

/// HalfKAv2_hm indices for `pov`. Both kings are active features.
pub fn piece_features(pos: &PosBB, pov: usize, feats: &mut Vec<usize>) {
    let king_sq = pos.king_sq(pov);
    if king_sq >= 64 {
        return;
    }
    let flip = if pov == 1 { 56 } else { 0 };
    let orient = ORIENT_HALFKA[king_sq] as usize;
    let bucket_base = KING_BUCKETS_BASE[king_sq ^ flip] * 704;
    for side in 0..2 {
        let relative_enemy = side != pov;
        for piece in 0..6 {
            let mut bb = pos.pieces[side][piece];
            while bb != 0 {
                let sq = bb.trailing_zeros() as usize;
                bb &= bb - 1;
                let s = (sq ^ orient ^ flip) as i32;
                let idx = s + piece_plane(piece, relative_enemy) * 64 + bucket_base;
                feats.push(idx as usize);
            }
        }
    }
}

/// HalfKAv2_hm index of ONE piece, for delta updates. `king_sq` is the
/// perspective's own king, which sets both the bucket and the mirror -- if it
/// moved, every index changes and a delta is not valid at all.
pub fn indice_peca(king_sq: usize, pov: usize, sq: usize, piece: usize, color: usize) -> usize {
    let flip = if pov == 1 { 56 } else { 0 };
    let orient = ORIENT_HALFKA[king_sq] as usize;
    let bucket_base = KING_BUCKETS_BASE[king_sq ^ flip] * 704;
    let s = (sq ^ orient ^ flip) as i32;
    (s + piece_plane(piece, color != pov) * 64 + bucket_base) as usize
}

/// Full_Threats indices for `pov` (already absolute, i.e. not offset).
pub fn threat_features(pos: &PosBB, pov: usize, feats: &mut Vec<usize>) {
    threat_features_com(pos, pov, feats, RAIOS)
}

pub fn threat_features_com(pos: &PosBB, pov: usize, feats: &mut Vec<usize>, d: Deslizantes) {
    let king_sq = pos.king_sq(pov);
    if king_sq >= 64 {
        return;
    }
    let hm = ORIENT_THREATS[king_sq] != 0;
    let occ = pos.occ();
    for side in 0..2 {
        for piece in 0..6 {
            let mut bb = pos.pieces[side][piece];
            while bb != 0 {
                let index_sq = bb.trailing_zeros() as usize;
                bb &= bb - 1;
                let mut attackers = attackers_to_com(pos, index_sq, occ, d);
                while attackers != 0 {
                    let a_sq = attackers.trailing_zeros() as usize;
                    attackers &= attackers - 1;
                    let (ap, ac) = match pos.piece_at(a_sq) {
                        Some(v) => v,
                        None => continue,
                    };
                    let tf = get_threat_feature(
                        pov, ap, ac, piece, side, a_sq as i32, index_sq as i32, hm,
                    );
                    if (tf as usize) < THREAT_DIM {
                        feats.push(tf as usize);
                    }
                }
            }
        }
    }
}

/// Threat features for `pov`, but emitting `dustbin` in place of the pairs SF
/// excludes instead of dropping them.
///
/// Why this exists: exclusion depends on `attacking_sq < attacked_sq` AFTER
/// the per-perspective square flip, so the same physical threat can be
/// excluded from one perspective and kept from the other. Anything that wants
/// the two perspectives index-aligned (the trainer, which consumes feature
/// PAIRS) must keep a placeholder rather than silently shortening one side.
pub fn threat_features_padded(pos: &PosBB, pov: usize, dustbin: usize, feats: &mut Vec<usize>) {
    threat_features_padded_com(pos, pov, dustbin, feats, RAIOS)
}

pub fn threat_features_padded_com(
    pos: &PosBB, pov: usize, dustbin: usize, feats: &mut Vec<usize>, d: Deslizantes,
) {
    let king_sq = pos.king_sq(pov);
    if king_sq >= 64 {
        return;
    }
    let hm = ORIENT_THREATS[king_sq] != 0;
    let occ = pos.occ();
    for side in 0..2 {
        for piece in 0..6 {
            let mut bb = pos.pieces[side][piece];
            while bb != 0 {
                let index_sq = bb.trailing_zeros() as usize;
                bb &= bb - 1;
                let mut attackers = attackers_to_com(pos, index_sq, occ, d);
                while attackers != 0 {
                    let a_sq = attackers.trailing_zeros() as usize;
                    attackers &= attackers - 1;
                    let (ap, ac) = match pos.piece_at(a_sq) {
                        Some(v) => v,
                        None => continue,
                    };
                    // Um par excluido nas DUAS perspectivas nao precisa de
                    // almofada: salta-se dos dois lados e o alinhamento 1:1
                    // mantem-se. Isto tira ~97% do dustbin (39,4% -> 1,06% das
                    // ameacas) sem o motor mudar nada -- ele ja' ignora estas.
                    // Fica so' a assimetria verdadeira, que e' a unica que
                    // OBRIGA a uma entrada de um lado sem par do outro.
                    if par_excluido_dos_dois_lados(pov, ap, ac, piece, side) {
                        continue;
                    }
                    let tf = get_threat_feature(
                        pov, ap, ac, piece, side, a_sq as i32, index_sq as i32, hm,
                    );
                    feats.push(if (tf as usize) < THREAT_DIM { tf as usize } else { dustbin });
                }
            }
        }
    }
}

/// PP_3Wide indices for `pov`, already offset by PAIR_BASE.
/// Os pares de peoes activos.
///
/// A versao anterior materializava duas `Vec` de casas por chamada (duas
/// alocacoes no heap, numa funcao que corre uma vez por reconstrucao e duas
/// por lance de peao) e depois cruzava-as com um ciclo duplo, testando cada
/// combinacao contra a mascara. Aqui a mascara faz o trabalho: intersecta-se
/// com o bitboard e so' se visitam os pares que existem mesmo.
///
/// Para os pares da mesma cor, `& !((1 << s) - 1)` deixa so' as casas acima de
/// `s`, que e' o que impede contar o mesmo par duas vezes -- a mascara ja' nao
/// contem `s`.
pub fn pair_features(pos: &PosBB, pov: usize, feats: &mut Vec<usize>) {
    let king_sq = pos.king_sq(pov);
    if king_sq >= 64 {
        return;
    }
    let mirrored = ORIENT_THREATS[king_sq] != 0;
    let square_flip = (if mirrored { 7 } else { 0 }) ^ (if pov == 1 { 56 } else { 0 });

    let friendly = pos.pieces[pov][0];
    let enemy = pos.pieces[1 - pov][0];

    let mut bb = friendly;
    while bb != 0 {
        let s = bb.trailing_zeros() as usize;
        bb &= bb - 1;
        let id_a = pawn_id(s, 0, square_flip);
        let mask = PP_MASK[s];
        let acima = !((1u64 << s) - 1);
        let mut m = mask & friendly & acima;
        while m != 0 {
            let s2 = m.trailing_zeros() as usize;
            m &= m - 1;
            feats.push(PAIR_BASE + pawn_pair_index(id_a, pawn_id(s2, 0, square_flip)));
        }
    }
    let mut bb = friendly;
    while bb != 0 {
        let s = bb.trailing_zeros() as usize;
        bb &= bb - 1;
        let id_a = pawn_id(s, 0, square_flip);
        let mut m = PP_MASK[s] & enemy;
        while m != 0 {
            let s2 = m.trailing_zeros() as usize;
            m &= m - 1;
            feats.push(PAIR_BASE + pawn_pair_index(id_a, pawn_id(s2, 48, square_flip)));
        }
    }
    let mut bb = enemy;
    while bb != 0 {
        let s = bb.trailing_zeros() as usize;
        bb &= bb - 1;
        let id_a = pawn_id(s, 48, square_flip);
        let acima = !((1u64 << s) - 1);
        let mut m = PP_MASK[s] & enemy & acima;
        while m != 0 {
            let s2 = m.trailing_zeros() as usize;
            m &= m - 1;
            feats.push(PAIR_BASE + pawn_pair_index(id_a, pawn_id(s2, 48, square_flip)));
        }
    }
}

/// So' os pares de peoes que MUDARAM entre duas posicoes.
///
/// A versao anterior enumerava todos os pares das duas posicoes, ordenava as
/// duas listas e cruzava-as -- duas alocacoes, dois `sort` e uma fusao por cada
/// lance de peao da busca. Um par so' pode mudar se envolver uma casa onde os
/// bitboards de peao diferem, e essas sao tipicamente duas.
///
/// Um par com as duas casas alteradas seria emitido duas vezes; a mascara
/// `mudou & abaixo(s)` deixa-o sair so' quando se processa a casa mais baixa.
pub fn pair_delta(
    antes: &PosBB, agora: &PosBB, pov: usize,
    saem: &mut Vec<usize>, entram: &mut Vec<usize>,
) {
    let king_sq = agora.king_sq(pov);
    if king_sq >= 64 {
        return;
    }
    let mirrored = ORIENT_THREATS[king_sq] != 0;
    let square_flip = (if mirrored { 7 } else { 0 }) ^ (if pov == 1 { 56 } else { 0 });

    let (fa, ea) = (antes.pieces[pov][0], antes.pieces[1 - pov][0]);
    let (fb, eb) = (agora.pieces[pov][0], agora.pieces[1 - pov][0]);
    let mudou = (fa ^ fb) | (ea ^ eb);
    if mudou == 0 {
        return;
    }

    for (amigos, inimigos, saida) in [(fa, ea, &mut *saem), (fb, eb, &mut *entram)] {
        let peoes = amigos | inimigos;
        let mut m = mudou;
        while m != 0 {
            let s = m.trailing_zeros() as usize;
            m &= m - 1;
            if (peoes >> s) & 1 == 0 {
                continue;
            }
            let meu = (amigos >> s) & 1 != 0;
            let id_a = pawn_id(s, if meu { 0 } else { 48 }, square_flip);
            let ja_feitos = mudou & ((1u64 << s) - 1);
            let mut p = PP_MASK[s] & peoes & !ja_feitos;
            while p != 0 {
                let s2 = p.trailing_zeros() as usize;
                p &= p - 1;
                let off = if (amigos >> s2) & 1 != 0 { 0 } else { 48 };
                saida.push(PAIR_BASE + pawn_pair_index(id_a, pawn_id(s2, off, square_flip)));
            }
        }
    }
}

/// Pares de peoes para uma rede de 768 entradas, sem espelho horizontal.
///
/// O `pair_features` espelha na horizontal conforme a casa do rei, porque a
/// arquitectura de onde veio tem baldes de rei e as features de peca ja' vem
/// espelhadas. Uma rede `768x2` nao tem baldes nenhuns: a unica transformacao
/// de perspectiva e' `sq ^ 56`. Espelhar so' os pares deixaria as duas familias
/// a discordar sobre o que e' a mesma posicao.
///
/// Devolve indices em [0, PAIR_DIM), sem deslocamento -- quem chama poe-nos
/// onde quiser no seu espaco de entradas.
///
/// As duas perspectivas produzem SEMPRE o mesmo numero de pares (o conjunto de
/// peoes e' o mesmo, so' muda a numeracao), portanto isto encaixa numa API que
/// exige um indice por lado sem precisar de almofada nenhuma. E' essa a
/// diferenca em relacao as ameacas, e e' o que torna esta feature barata.
pub fn pair_features_768(pos: &PosBB, pov: usize, feats: &mut Vec<usize>) {
    let flip = if pov == 1 { 56 } else { 0 };
    let amigos = pos.pieces[pov][0];
    let inimigos = pos.pieces[1 - pov][0];
    let peoes = amigos | inimigos;

    let mut bb = peoes;
    while bb != 0 {
        let s = bb.trailing_zeros() as usize;
        bb &= bb - 1;
        let meu = (amigos >> s) & 1 != 0;
        let id_a = pawn_id(s, if meu { 0 } else { 48 }, flip);
        // so' as casas acima de `s`, para cada par sair uma vez
        let mut m = PP_MASK[s] & peoes & !((1u64 << s) - 1);
        while m != 0 {
            let s2 = m.trailing_zeros() as usize;
            m &= m - 1;
            let off = if (amigos >> s2) & 1 != 0 { 0 } else { 48 };
            feats.push(pawn_pair_index(id_a, pawn_id(s2, off, flip)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pares_768_alinham_entre_perspectivas() {
        // A propriedade que a API do treinador exige: mesmo numero de features
        // dos dois lados, para cada par poder ser emitido como um so' `f(a,b)`.
        for fen in [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "4k3/pp1p1ppp/8/2pPp3/2P1P3/8/PP3PPP/4K3 w - c6 0 1",
            "8/2p1p1p1/1p1p1p1p/8/8/P1P1P1P1/1P1P1P1P/8 w - - 0 1",
            "8/8/4k3/8/8/4K3/8/8 w - - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        ] {
            let p = from_fen(fen);
            let (mut a, mut b) = (Vec::new(), Vec::new());
            pair_features_768(&p, 0, &mut a);
            pair_features_768(&p, 1, &mut b);
            assert_eq!(a.len(), b.len(), "contagens diferentes em {fen}");
            for &i in a.iter().chain(b.iter()) {
                assert!(i < PAIR_DIM, "indice {i} fora de {PAIR_DIM} em {fen}");
            }
        }
    }

    fn from_fen(fen: &str) -> PosBB {
        let mut p = PosBB::default();
        let board = fen.split_whitespace().next().unwrap();
        let mut sq: i32 = 56;
        for ch in board.chars() {
            match ch {
                '/' => sq -= 16,
                '1'..='8' => sq += ch as i32 - '0' as i32,
                _ => {
                    let c = if ch.is_uppercase() { 0 } else { 1 };
                    let t = match ch.to_ascii_lowercase() {
                        'p' => 0, 'n' => 1, 'b' => 2, 'r' => 3, 'q' => 4, 'k' => 5,
                        _ => continue,
                    };
                    p.pieces[c][t] |= 1u64 << sq;
                    sq += 1;
                }
            }
        }
        p
    }

    /// The trainer pairs features across perspectives by position, so the two
    /// perspectives must enumerate the same features in the same order.
    /// Pieces and pawn pairs do so naturally; threats only do once excluded
    /// pairs are padded rather than dropped -- this is exactly the bug the
    /// dustbin exists to prevent, so it is worth a test that would catch its
    /// removal.
    #[test]
    fn perspectives_stay_aligned() {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR",
            "r1bq1rk1/ppp2ppp/5n2/2bp4/2NPP3/2P5/PP3PPP/RNBQK2R",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8",
            "7k/8/8/8/3PP3/8/8/K7",
            "r2q1rk1/ppp1ppbp/2np1npB/8/2PP4/2N2P2/PP1Q1PPP/R3KB1R",
        ];
        const DUSTBIN: usize = INPUT_DIM;
        for fen in fens {
            let p = from_fen(fen);

            let (mut a, mut b) = (Vec::new(), Vec::new());
            piece_features(&p, 0, &mut a);
            piece_features(&p, 1, &mut b);
            assert_eq!(a.len(), b.len(), "piece features misaligned: {fen}");

            a.clear(); b.clear();
            pair_features(&p, 0, &mut a);
            pair_features(&p, 1, &mut b);
            assert_eq!(a.len(), b.len(), "pawn-pair features misaligned: {fen}");

            a.clear(); b.clear();
            threat_features_padded(&p, 0, DUSTBIN, &mut a);
            threat_features_padded(&p, 1, DUSTBIN, &mut b);
            assert_eq!(a.len(), b.len(), "padded threats misaligned: {fen}");

            // And the padding is load-bearing: without it the two sides really
            // do come out different lengths on some positions.
            let (mut ua, mut ub) = (Vec::new(), Vec::new());
            threat_features(&p, 0, &mut ua);
            threat_features(&p, 1, &mut ub);
            assert_eq!(
                ua.len(), a.iter().filter(|&&x| x != DUSTBIN).count(),
                "padded/unpadded disagree on kept threats: {fen}"
            );
            assert_eq!(ub.len(), b.iter().filter(|&&x| x != DUSTBIN).count());
        }
    }

    /// Every index a position can produce must be inside the declared space.
    #[test]
    fn indices_in_range() {
        let p = from_fen("r2q1rk1/ppp1ppbp/2np1npB/8/2PP4/2N2P2/PP1Q1PPP/R3KB1R");
        for pov in 0..2 {
            let mut v = Vec::new();
            piece_features(&p, pov, &mut v);
            assert!(v.iter().all(|&x| x < PIECE_DIM), "piece index out of range");
            v.clear();
            threat_features(&p, pov, &mut v);
            assert!(v.iter().all(|&x| x < THREAT_DIM), "threat index out of range");
            v.clear();
            pair_features(&p, pov, &mut v);
            assert!(
                v.iter().all(|&x| (PAIR_BASE..PAIR_BASE + PAIR_DIM).contains(&x)),
                "pawn-pair index out of range"
            );
        }
    }
}

#[cfg(test)]
mod medicao {
    use super::*;
    /// How many features a position actually activates -- `max_active` must
    /// cover the worst case, but every unit above it costs real memory in the
    /// trainer's batch buffers (batch_size * max_active * 2 perspectives).
    #[test]
    #[ignore]
    fn conta_features_activas() {
        use std::io::BufRead;
        // Suite de posicoes por variavel de ambiente: um caminho fixo so'
        // existe na maquina de quem o escreveu.
        let caminho = match std::env::var("KESTREL_EPD") {
            Ok(v) => v,
            Err(_) => return, // sem suite, nada a contar
        };
        let f = std::fs::File::open(caminho).unwrap();
        let mut pior = 0usize;
        let mut soma = 0usize;
        let mut n = 0usize;
        for linha in std::io::BufReader::new(f).lines().take(4000) {
            let l = linha.unwrap();
            let mut p = PosBB::default();
            let board = l.split_whitespace().next().unwrap();
            let mut sq: i32 = 56;
            for ch in board.chars() {
                match ch {
                    '/' => sq -= 16,
                    '1'..='8' => sq += ch as i32 - '0' as i32,
                    _ => {
                        let c = if ch.is_uppercase() { 0 } else { 1 };
                        let t = match ch.to_ascii_lowercase() {
                            'p'=>0,'n'=>1,'b'=>2,'r'=>3,'q'=>4,'k'=>5,_=>continue };
                        p.pieces[c][t] |= 1u64 << sq; sq += 1;
                    }
                }
            }
            let mut v = Vec::new();
            piece_features(&p, 0, &mut v);
            threat_features_padded(&p, 0, usize::MAX, &mut v);
            pair_features(&p, 0, &mut v);
            pior = pior.max(v.len());
            soma += v.len(); n += 1;
        }
        println!("posicoes={n}  media={}  PIOR={pior}", soma / n.max(1));
    }
}

#[cfg(test)]
mod perf {
    use super::*;
    use std::time::Instant;

    fn pos_teste() -> PosBB {
        let mut p = PosBB::default();
        for (c, t, sqs) in [
            (0usize,0usize,vec![8,9,10,11,12,13,14,15]), (0,1,vec![1,6]), (0,2,vec![2,5]),
            (0,3,vec![0,7]), (0,4,vec![3]), (0,5,vec![4]),
            (1,0,vec![48,49,50,51,52,53,54,55]), (1,1,vec![57,62]), (1,2,vec![58,61]),
            (1,3,vec![56,63]), (1,4,vec![59]), (1,5,vec![60]),
        ] { for s in sqs { p.pieces[c][t] |= 1u64 << s; } }
        p
    }

    /// Onde vai o tempo: gerar as features, ou somar o acumulador?
    #[test]
    #[ignore]
    fn onde_esta_o_tempo() {
        let p = pos_teste();
        let n = 20_000;

        let t = Instant::now();
        let mut total = 0usize;
        for _ in 0..n {
            let mut v = Vec::with_capacity(256);
            piece_features(&p, 0, &mut v);
            total += v.len();
        }
        println!("pecas    {:>7.1} us/pos", t.elapsed().as_micros() as f64 / n as f64);

        let t = Instant::now();
        for _ in 0..n {
            let mut v = Vec::with_capacity(256);
            threat_features(&p, 0, &mut v);
            total += v.len();
        }
        println!("ameacas  {:>7.1} us/pos", t.elapsed().as_micros() as f64 / n as f64);

        let t = Instant::now();
        for _ in 0..n {
            let mut v = Vec::with_capacity(64);
            pair_features(&p, 0, &mut v);
            total += v.len();
        }
        println!("pares    {:>7.1} us/pos", t.elapsed().as_micros() as f64 / n as f64);

        // custo do attackers_to isolado
        let occ = p.occ();
        let t = Instant::now();
        let mut acc = 0u64;
        for _ in 0..n { for sq in 0..64 { acc ^= attackers_to(&p, sq, occ); } }
        println!("attackers_to x64 {:>7.1} us/pos", t.elapsed().as_micros() as f64 / n as f64);
        assert!(total > 0 && acc != 12345);
    }
}

// ---- Threat deltas anchored on the move ----
//
// Instead of enumerating every active threat to find which changed, derive
// the change from the move: for a piece entering or leaving a square, the
// threats that move are its own (direct), those aimed at that square
// (incoming), and slider attacks revealed or blocked through it (discovered).
//
// `occ` must be the occupancy at that exact instant -- before the piece left
// its origin, or after it reached its destination. Using one board for both
// silently loses discovered attacks, which is the failure this design exists
// to avoid.

/// Squares along the slider..through line, starting just past `slider_sq` and
/// continuing beyond `through` to the edge -- what gets revealed or blocked
/// past `through`, seen from `slider_sq`. Only meaningful for aligned squares.
pub fn ray_pass_bb(slider_sq: usize, through: usize) -> u64 {
    let (sf, sr) = ((slider_sq & 7) as i32, (slider_sq >> 3) as i32);
    let (tf, tr) = ((through & 7) as i32, (through >> 3) as i32);
    let (df, dr) = (tf - sf, tr - sr);
    if df != 0 && dr != 0 && df.abs() != dr.abs() {
        return 0;
    }
    if df == 0 && dr == 0 {
        return 0;
    }
    let step_f = (df > 0) as i32 - (df < 0) as i32;
    let step_r = (dr > 0) as i32 - (dr < 0) as i32;
    let mut out = 0u64;
    // starts just past slider_sq, not at `through` -- a bit-exact test caught
    // this once already
    let (mut f, mut r) = (sf + step_f, sr + step_r);
    while (0..8).contains(&f) && (0..8).contains(&r) {
        out |= 1u64 << (r * 8 + f);
        f += step_f;
        r += step_r;
    }
    out
}

/// A threat feature that changed, and whether it was added or removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeltaAmeaca {
    pub idx: usize,
    pub adicionar: bool,
    /// The squares the threat runs between, kept so the caller can check the
    /// emitted delta against reality instead of trusting the ray heuristics.
    pub a_sq: usize,
    pub d_sq: usize,
}

/// Does `a_sq` threaten `d_sq` in this exact position?
///
/// The single source of truth for whether a threat feature is active, used to
/// check every delta the ray logic proposes. The ray guards can tell that a
/// blocker left a line, but not that another event on the same move put a
/// blocker straight back -- `Qxf6` lifts the knight off f6 and lands the queen
/// on it, so f7 is revealed and re-blocked inside one move. Rather than grow
/// more guards, each proposed change is confirmed against both positions:
/// an addition must be absent before and present after, a removal the reverse.
pub fn ameaca_existe(pos: &PosBB, pov: usize, a_sq: usize, d_sq: usize, d: Deslizantes) -> bool {
    let (ap, ac) = match pos.piece_at(a_sq) { Some(v) => v, None => return false };
    let (dp, _dc) = match pos.piece_at(d_sq) { Some(v) => v, None => return false };
    if dp == 5 || ap == 5 {
        // kings are never threat targets, and a king attacker is only ever
        // recorded through the incoming side, which uses the same table
        if dp == 5 { return false; }
    }
    let occ = pos.occ();
    if ataques_de(ap, ac, a_sq, occ, d) & (1u64 << d_sq) != 0 {
        return true;
    }
    // pawns also threaten their push square, and only against pawns
    if ap == 0 && dp == 0 {
        let empurra = ((1u64 << a_sq) << 8) | ((1u64 << a_sq) >> 8);
        if empurra & (1u64 << d_sq) != 0 {
            let _ = pov;
            return true;
        }
    }
    false
}

/// Can a piece of this type on `de` reach `para` with this occupancy? Used to
/// reject event lists that look like a move but are not one.
pub fn alcanca_pseudo(piece: usize, de: usize, para: usize, occ: u64, d: Deslizantes) -> bool {
    ataques_de(piece, 0, de, occ, d) & (1u64 << para) != 0
}

/// Pseudo-attacks of `piece` of `color` standing on `sq`, given `occ`.
fn ataques_de(piece: usize, color: usize, sq: usize, occ: u64, d: Deslizantes) -> u64 {
    match piece {
        0 => pawn_attacks_from(color, sq),
        1 => knight_attacks(sq),
        2 => (d.bispo)(sq, occ),
        3 => (d.torre)(sq, occ),
        4 => (d.bispo)(sq, occ) | (d.torre)(sq, occ),
        _ => king_attacks(sq),
    }
}

/// Threat features that change when `piece` of `color` is put on (`add`) or
/// taken off (`!add`) square `s`, with `pos`/`occ` at that exact instant.
///
/// `sem_raios` mirrors the original's noRaysContaining: a slider is skipped
/// for discovered attacks when its ray past `s` contains every square in it
/// (typically {from,to} of a quiet move), so the call at `from` does not
/// redo the discovery the call at `to` already handles. Pass `!0` to never skip.
#[allow(clippy::too_many_arguments)]
pub fn eventos_ameaca(
    pos: &PosBB, pov: usize, add: bool, color: usize, piece: usize, s: usize,
    sem_raios: u64, d: Deslizantes, out: &mut Vec<DeltaAmeaca>, king_sq: usize,
) {
    let mut rels = Vec::with_capacity(32);
    relacoes_ameaca(pos, add, color, piece, s, sem_raios, d, &mut rels);
    relacoes_para_deltas(&rels, pov, king_sq, out);
}

/// One threat relation -- attacker piece/colour/square against target
/// piece/colour/square -- with no perspective baked in.
///
/// The costly half of a threat update is finding these: attacks out of the
/// square, the sliders aligned through it, and what each ray reveals or blocks.
/// None of it depends on which side we are looking from; only the feature
/// INDEX does, via `get_threat_feature`'s `pov` and the horizontal mirror of
/// that side's king. We were running the whole enumeration once per
/// perspective, which is why threats cost 19% of search time here against 2%
/// in the reference for the same job -- not a worse algorithm, the same
/// algorithm run twice. Splitting it lets the caller enumerate once and index
/// twice.
#[derive(Clone, Copy)]
pub struct RelAmeaca {
    pub adicionar: bool,
    pub ap: u8,
    pub ac: u8,
    pub a_sq: u8,
    pub dp: u8,
    pub dc: u8,
    pub d_sq: u8,
}

/// The perspective-free half: which threat relations change.
#[allow(clippy::too_many_arguments)]
pub fn relacoes_ameaca(
    pos: &PosBB, add: bool, color: usize, piece: usize, s: usize,
    sem_raios: u64, d: Deslizantes, out: &mut Vec<RelAmeaca>,
) {
    let occ = pos.occ();

    let mut empurra = |flag: bool, ap: usize, ac: usize, a_sq: usize, dp: usize, dc: usize, d_sq: usize| {
        out.push(RelAmeaca {
            adicionar: flag,
            ap: ap as u8, ac: ac as u8, a_sq: a_sq as u8,
            dp: dp as u8, dc: dc as u8, d_sq: d_sq as u8,
        });
    };

    let reis = pos.pieces[0][5] | pos.pieces[1][5];
    let occ_sem_reis = occ ^ reis;
    let r_att = (d.torre)(s, occ);
    let b_att = (d.bispo)(s, occ);
    let torre_dama = pos.pieces[0][3] | pos.pieces[1][3] | pos.pieces[0][4] | pos.pieces[1][4];
    let bispo_dama = pos.pieces[0][2] | pos.pieces[1][2] | pos.pieces[0][4] | pos.pieces[1][4];
    let sliders = (torre_dama & r_att) | (bispo_dama & b_att);

    // Sliders aligned through `s`: what they reveal or block beyond it.
    let mut processa_sliders = |mut locais: u64, directos: bool,
                                empurra: &mut dyn FnMut(bool, usize, usize, usize, usize, usize, usize)| {
        while locais != 0 {
            let slider_sq = locais.trailing_zeros() as usize;
            locais &= locais - 1;
            let (sp, sc) = match pos.piece_at(slider_sq) { Some(v) => v, None => continue };

            let ray = ray_pass_bb(slider_sq, s);
            let descoberto = ray & (r_att | b_att) & occ_sem_reis;
            if descoberto != 0 && (ray & sem_raios) != sem_raios {
                let alvo = descoberto.trailing_zeros() as usize;
                if let Some((tp, tc)) = pos.piece_at(alvo) {
                    empurra(!add, sp, sc, slider_sq, tp, tc, alvo);
                }
            }
            if directos {
                empurra(add, sp, sc, slider_sq, piece, color, s);
            }
        }
    };

    if piece == 5 {
        processa_sliders(sliders, false, &mut empurra);
        // A king is never a threat TARGET (`occ_sem_reis` takes both kings out
        // of every target set), but it very much is a threat SOURCE -- see the
        // `king_attacks(s) & reis` term in `incoming` below. Returning here
        // without emitting those left every (king -> piece) threat untouched
        // whenever a king moved, so the accumulator kept the threats the king
        // made from the square it had just left.
        //
        // It only showed up on the opponent's perspective, because our own
        // king moving forces a full rebuild anyway -- which is why a search
        // with no king moves matched the rebuild exactly, and one with a single
        // `Ke1-f1` diverged from that move onward.
        // EXPERIENCIA: o rei nao emite ameacas directas neste conjunto de
        // features -- medido, 1048846 de 1048846 (100%) caem fora do espaco.
        return;
    }

    let cavalos = pos.pieces[0][1] | pos.pieces[1][1];
    // The pawn terms below LOOK perspective-dependent and are not. `meus` was
    // `if color == pov { pawn_attacks_from(pov) } else { pawn_attacks_from(1-pov) }`,
    // which is `pawn_attacks_from(color)` in both branches; and `incoming` was
    // the union of the two directions against the two pawn sets, a set that is
    // symmetric under swapping `pov`. Writing them by COLOUR says the same
    // thing and drops the last reason this enumeration needed a perspective.
    let peoes = [pos.pieces[0][0], pos.pieces[1][0]];

    // A MASCARA, nao um OR. Um peao so' ameaca cavalo e torre
    // (`PIECE_INTERACTION_MAP[0]` = `[-1, 0, -1, 1, -1, -1]`), mas
    // `ataques_de` devolve tudo o que ele ataca -- peoes, bispos e damas
    // incluidos, todos mortos. Sao 1912581 de 2653570 relacoes de peao (72%)
    // geradas e indexadas para nada.
    let torres = pos.pieces[0][3] | pos.pieces[1][3];
    let alvos_validos = if piece == 0 { cavalos | torres } else { !0u64 };
    let mut ameacados = ataques_de(piece, color, s, occ, d) & occ_sem_reis & alvos_validos;
    // O termo `king_attacks(s) & reis` sai daqui pela mesma razao que o bloco
    // do rei acima: (rei -> peca) nao existe neste conjunto de features. Medido,
    // 610441 de 610441 (100%) caiam fora do espaco -- geradas, indexadas e
    // deitadas fora.
    let mut incoming = knight_attacks(s) & cavalos;

    // A pawn threatens ONLY knights and rooks: `PIECE_INTERACTION_MAP[0]` is
    // `[-1, 0, -1, 1, -1, -1]`. Pawn->pawn relations and the push relations were
    // dropped when the feature set went from 60720 to 59808 inputs, but the code
    // that generated them stayed, so every one of those tuples was built,
    // indexed, and then discarded by the `idx < THREAT_DIM` test downstream.
    //
    if piece == 0 {
    } else if piece == 1 || piece == 3 {
        incoming |= (pawn_attacks_from(0, s) & peoes[1]) | (pawn_attacks_from(1, s) & peoes[0]);
    }

    // Por TIPO, nao por casa. O `piece_at` varre ate' 12 bitboards com ramos
    // imprevisiveis para descobrir o que esta' numa casa; percorrendo um
    // bitboard de cada tipo o tipo e' conhecido de graca e so' a cor precisa de
    // um teste. O conjunto visitado e' o mesmo -- a ORDEM e' que muda, e a
    // ordem nao importa porque estas relacoes sao somadas, nao aplicadas em
    // sequencia (a assinatura do bench e' quem o prova).
    for tp in 0..6 {
        let brancas = pos.pieces[0][tp];
        let mut b = ameacados & (brancas | pos.pieces[1][tp]);
        while b != 0 {
            let alvo = b.trailing_zeros() as usize;
            b &= b - 1;
            let tc = usize::from(brancas & (1u64 << alvo) == 0);
            empurra(add, piece, color, s, tp, tc, alvo);
        }
    }

    processa_sliders(sliders, true, &mut empurra);

    for sp in 0..6 {
        let brancas = pos.pieces[0][sp];
        let mut b = incoming & (brancas | pos.pieces[1][sp]);
        while b != 0 {
            let src = b.trailing_zeros() as usize;
            b &= b - 1;
            let sc = usize::from(brancas & (1u64 << src) == 0);
            empurra(add, sp, sc, src, piece, color, s);
        }
    }
}

/// The perspective-dependent half: relations -> feature indices for one side.
///
/// `king_sq` comes from the caller and is NOT read off the position. Walking a
/// move means the board passes through a state where the king has been taken
/// off its old square and not yet put on the new one -- for `Ke1-f1`, a board
/// with no white king at all. Deriving it here made every call in that window
/// hit `king_sq >= 64` and return in silence, so the whole move contributed
/// nothing to the opponent's perspective. The orientation is guaranteed
/// unchanged by the caller, so the final square is the right one throughout.
pub fn relacoes_para_deltas(
    rels: &[RelAmeaca], pov: usize, king_sq: usize, out: &mut Vec<DeltaAmeaca>,
) {
    if king_sq >= 64 {
        return;
    }
    let hm = hm_de_rei(king_sq);
    for r in rels {
        let tf = indice_relacao(r, pov, hm);
        if tf < THREAT_DIM {
            out.push(DeltaAmeaca {
                idx: tf,
                adicionar: r.adicionar,
                a_sq: r.a_sq as usize,
                d_sq: r.d_sq as usize,
            });
        }
    }
}

/// Horizontal mirror for a perspective, from its king square.
#[inline(always)]
pub fn hm_de_rei(king_sq: usize) -> bool {
    ORIENT_THREATS[king_sq] != 0
}

/// Feature index of one relation for one perspective. `THREAT_DIM` or above
/// means the pair is excluded and the relation contributes nothing.
#[inline(always)]
pub fn indice_relacao(r: &RelAmeaca, pov: usize, hm: bool) -> usize {
    get_threat_feature(
        pov, r.ap as usize, r.ac as usize, r.dp as usize, r.dc as usize,
        r.a_sq as i32, r.d_sq as i32, hm,
    ) as usize
}

#[cfg(test)]
mod delta {
    use super::*;

    fn from_fen(fen: &str) -> PosBB {
        let mut p = PosBB::default();
        let mut sq: i32 = 56;
        for ch in fen.split_whitespace().next().unwrap().chars() {
            match ch {
                '/' => sq -= 16,
                '1'..='8' => sq += ch as i32 - '0' as i32,
                _ => {
                    let c = if ch.is_uppercase() { 0 } else { 1 };
                    let t = match ch.to_ascii_lowercase() {
                        'p'=>0,'n'=>1,'b'=>2,'r'=>3,'q'=>4,'k'=>5,_=>continue };
                    p.pieces[c][t] |= 1u64 << sq; sq += 1;
                }
            }
        }
        p
    }

    fn conjunto(p: &PosBB, pov: usize) -> Vec<usize> {
        let mut v = Vec::new();
        threat_features(p, pov, &mut v);
        v.sort_unstable();
        v
    }

    /// The delta must reproduce exactly what full enumeration says changed.
    /// A wrong delta is silent in play, so this compares the two directly:
    /// take a real position, lift a piece off, and check that applying the
    /// events to the "before" set lands on the "after" set.
    #[test]
    fn delta_bate_com_enumeracao() {
        let fens = [
            "r1bq1rk1/ppp2ppp/5n2/2bp4/2NPP3/2P5/PP3PPP/RNBQK2R",
            "r2q1rk1/ppp1ppbp/2np1npB/8/2PP4/2N2P2/PP1Q1PPP/R3KB1R",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR",
        ];
        let mut testados = 0;
        for fen in fens {
            let antes = from_fen(fen);
            for pov in 0..2 {
                // remove each non-king piece in turn
                for c in 0..2 {
                    for t in 0..5 {
                        let mut bb = antes.pieces[c][t];
                        while bb != 0 {
                            let sq = bb.trailing_zeros() as usize;
                            bb &= bb - 1;
                            let mut depois = antes;
                            depois.pieces[c][t] &= !(1u64 << sq);

                            let esperado = conjunto(&depois, pov);
                            let base = conjunto(&antes, pov);

                            // events for taking that piece off, on the board
                            // as it stood BEFORE the removal
                            let mut ev = Vec::new();
                            eventos_ameaca(&antes, pov, false, c, t, sq, !0u64, RAIOS, &mut ev, antes.king_sq(pov));

                            let mut conta = std::collections::HashMap::new();
                            for f in &base { *conta.entry(*f).or_insert(0i32) += 1; }
                            for d in &ev {
                                *conta.entry(d.idx).or_insert(0i32) += if d.adicionar { 1 } else { -1 };
                            }
                            let mut obtido: Vec<usize> =
                                conta.into_iter().filter(|&(_, n)| n > 0).map(|(f, _)| f).collect();
                            obtido.sort_unstable();

                            assert_eq!(obtido, esperado,
                                "delta errado ao tirar peca {t} cor {c} de {sq}, pov {pov}, fen {fen}");
                            testados += 1;
                        }
                    }
                }
            }
        }
        println!("delta validado em {testados} remocoes");
    }
}
