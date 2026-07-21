use crate::attacks::*;
use crate::bitboard::*;
use crate::board::Board;
use crate::types::*;
use std::sync::OnceLock;

static ATTACKS: OnceLock<Attacks> = OnceLock::new();
fn atk() -> &'static Attacks {
    ATTACKS.get_or_init(Attacks::new)
}

// Tapered piece-square tables e valores de material portados
// literalmente do Sirius v9.0 (`src/eval/eval_constants.h`, mcthouacbb) --
// motor #1 em HCE puro na sua tier, tabelas afinadas por Texel Tuning.
// Duas conversoes de layout, sem alterar valores:
//   1. Sirius armazena rank8-first (row 0 = 8a fileira); Kestrel usa
//      rank1-first (row 0 = 1a fileira). O script
//      `port_sirius_psqt.py` (scratchpad) inverte a ordem das linhas.
//   2. Sirius usa king-buckets: a PSQT do Rei so' preenche files a-d;
//      files e-h vem de um espelho horizontal quando o rei esta' no
//      king-side. Kestrel nao tem esse mecanismo -- o port "materializa"
//      esse espelho, os 4 files da direita sao copias espelhadas dos
//      da esquerda. Resultado equivalente ao par
//      (bucket-0-tabela + espelho horizontal) do Sirius.
//
// Nota importante (2026-07-20): num A/B self-play de 30 jogos, o motor
// com estas tabelas + o `positional_terms` (que continua com pesos
// calibrados a olho, nao afinados em conjunto com estas escalas) marcou
// 26.7% -- interpretei mal esse sinal e cheguei a reverter. Correcao
// registada em memoria (ver feedback_kestrel_nao_reverter_por_self_play_pequeno):
// 30 jogos self-play nao tem resolucao para detectar Elo real; o codigo
// e' correcto (portagem literal validada), o teste era fraco. A afinacao
// dos pesos do positional_terms para a escala nova (Texel Tuning proprio)
// e um passo separado, futuro.
#[rustfmt::skip]
const MG_PAWN: [i32; 64] = [
        0,     0,     0,     0,     0,     0,     0,     0,
       -2,    24,    23,     5,    -1,    -4,    -9,   -16,
      -10,    11,     2,     6,    -3,    -8,   -18,   -20,
       -4,    -9,    16,    16,    13,     3,   -16,   -13,
        6,     1,     9,    10,     4,     1,    -5,    -6,
        9,    -8,     3,     8,    -1,    -1,   -14,     1,
       63,     6,    21,    31,    56,    39,    28,    76,
        0,     0,     0,     0,     0,     0,     0,     0,
];
#[rustfmt::skip]
const EG_PAWN: [i32; 64] = [
        0,     0,     0,     0,     0,     0,     0,     0,
       -7,     6,    67,     6,    -3,    -2,    10,    -4,
       -7,     5,     3,    -3,    -3,    -4,    10,    -8,
       -4,    14,   -27,   -18,   -20,   -14,    11,    -5,
       11,    19,    -5,   -17,   -17,   -10,    12,    10,
       12,    25,    -1,   -11,   -18,    -7,    19,    11,
       90,   103,   108,    91,    69,    68,   103,    90,
        0,     0,     0,     0,     0,     0,     0,     0,
];
#[rustfmt::skip]
const MG_KNIGHT: [i32; 64] = [
      -26,    -9,     0,     6,    -1,   -10,   -14,   -29,
       11,     3,     8,    12,     5,     3,    -5,   -14,
        5,    18,    16,    14,    12,     0,     5,    -6,
       21,    28,    25,    18,    18,    19,    18,     9,
       30,    29,    34,    28,    33,    20,    20,    13,
       15,     8,    30,    25,    20,    23,     9,    11,
        0,   -13,    19,    37,    24,    22,   -18,    -6,
      -54,   -84,  -111,    -6,   -20,   -58,  -100,   -85,
];
#[rustfmt::skip]
const EG_KNIGHT: [i32; 64] = [
      -17,   -19,   -15,    -8,    -3,   -16,   -21,   -35,
       -8,     1,   -13,    -1,    -1,   -12,    -4,   -19,
       -5,    -1,    -1,    15,    10,    -2,    -6,    -7,
        7,     6,    15,    29,    21,    15,     2,     4,
        7,    11,    15,    24,    21,    15,     4,     6,
       -5,    12,    11,    14,    14,     7,     9,   -11,
       -2,    24,    10,    10,    18,    -6,    15,    -9,
     -122,    13,    43,    13,    14,     6,     8,   -68,
];
#[rustfmt::skip]
const MG_BISHOP: [i32; 64] = [
       10,    -7,   -10,    11,     6,    -3,     6,     5,
       23,    28,    26,    11,     2,    13,     9,    18,
       21,    23,    13,    14,    13,     7,    28,    14,
       19,     6,    10,    20,    20,    13,     9,    12,
        6,    12,    13,    16,    25,    16,    11,     6,
       21,     7,    19,    23,    18,    10,    15,     6,
      -13,   -37,   -10,   -28,   -18,   -12,   -12,    -8,
      -25,   -52,   -85,   -67,   -78,   -66,   -47,   -28,
];
#[rustfmt::skip]
const EG_BISHOP: [i32; 64] = [
      -30,     1,    -8,   -14,   -11,    -6,    -9,   -19,
      -14,   -30,   -13,    -5,    -5,   -16,   -26,    -8,
       -9,    -5,    -7,    12,     5,    -9,    -7,   -11,
      -10,     6,    16,    15,    19,    10,     5,    -9,
       -3,    15,    19,    28,    23,    15,    10,    -7,
      -10,    12,     7,    11,    13,     0,     4,    -3,
       -5,     5,     9,    14,    17,    11,   -10,    -1,
      -37,    20,    23,    23,    31,    20,    16,   -15,
];
#[rustfmt::skip]
const MG_ROOK: [i32; 64] = [
      -14,   -18,     7,    13,     7,    -1,     0,    -1,
      -50,    -6,     1,    -2,    -1,    -7,   -12,   -19,
      -16,     6,    -8,    -1,    -1,   -13,    -4,   -18,
      -23,   -10,   -12,   -10,   -12,   -16,   -15,   -13,
      -16,    -6,    -3,    -9,    -2,     0,    -2,    -5,
      -16,    10,    14,    16,    14,     8,     7,    -2,
       25,    12,    18,    16,    22,    14,    13,    20,
       16,    33,    28,    14,    25,    15,    18,    22,
];
#[rustfmt::skip]
const EG_ROOK: [i32; 64] = [
      -35,   -19,   -23,   -29,   -28,   -20,   -24,   -22,
      -27,   -34,   -22,   -21,   -23,   -17,   -22,   -22,
      -25,   -26,    -8,   -16,   -17,    -7,   -15,   -14,
       -3,     4,     9,     7,     4,    10,    11,     2,
        9,    13,    12,     9,     6,     9,    14,    11,
       16,    12,    10,     7,     7,    14,    17,    20,
       10,    22,    19,    21,    24,    23,    21,    19,
       16,    12,    14,    25,    15,    20,    22,    20,
];
#[rustfmt::skip]
const MG_QUEEN: [i32; 64] = [
      -16,   -23,   -15,    -7,    -3,   -10,   -12,   -20,
       -1,     6,     0,    -4,    -7,     5,     0,     1,
       -4,    -1,    -9,   -18,   -11,    -5,     7,     2,
      -15,   -11,   -15,   -20,   -22,    -9,    -2,    -5,
       -6,   -15,   -13,   -26,   -22,     5,     0,     4,
        7,    11,    -4,   -19,    -5,    14,    27,    19,
       34,    32,    10,   -16,    11,    15,    19,    12,
       15,    49,    30,    20,    27,    36,    21,   -11,
];
#[rustfmt::skip]
const EG_QUEEN: [i32; 64] = [
      -34,   -69,   -53,   -44,   -36,   -32,   -31,    -6,
      -63,   -82,   -47,   -21,   -13,   -32,   -28,   -20,
      -19,   -18,     1,    15,    14,     5,    -7,    -7,
       21,    18,    19,    32,    43,    23,    11,     5,
       12,    24,    23,    43,    40,     9,    19,    -4,
        8,    11,    28,    41,    39,     0,   -24,   -12,
       -7,    -6,    26,    43,    27,     3,    -9,    -1,
       28,     3,    28,    36,    24,    10,    13,    33,
];
#[rustfmt::skip]
const MG_KING: [i32; 64] = [
       66,    65,    28,    24,    24,    28,    65,    66,
       66,    53,    23,    -1,    -1,    23,    53,    66,
        9,     2,   -30,   -51,   -51,   -30,     2,     9,
      -55,   -53,   -77,  -103,  -103,   -77,   -53,   -55,
      -24,   -54,   -68,  -131,  -131,   -68,   -54,   -24,
       39,   -19,   -70,  -150,  -150,   -70,   -19,    39,
       37,   -26,   -78,  -101,  -101,   -78,   -26,    37,
       64,     3,   -47,   -67,   -67,   -47,     3,    64,
];
#[rustfmt::skip]
const EG_KING: [i32; 64] = [
      -39,    -4,   -65,  -125,  -125,   -65,    -4,   -39,
       -7,    19,   -41,   -83,   -83,   -41,    19,    -7,
       20,    47,    -8,   -48,   -48,    -8,    47,    20,
       44,    79,    15,   -25,   -25,    15,    79,    44,
       55,   108,    34,    -6,    -6,    34,   108,    55,
       56,   129,    60,    14,    14,    60,   129,    56,
       16,   126,    81,    59,    59,    81,   126,    16,
     -154,    88,    40,    21,    21,    40,    88,  -154,
];

/// Valores de material por fase -- Sirius v9.0 literais (afinados por
/// Texel Tuning). Note-se a diferenca em relacao aos classicos: peao
/// vale muito pouco no mg (65cp) mas muito no eg (138), o cavalo/bispo
/// eg (450/475) valem mais que a torre mg (411), e a dama tem eg de
/// 1957. Distintos de `PieceType::value()`, que continua a servir
/// SEE/MVV-LVA (troca material simples, sem fase).
const MG_VALUE: [i32; 6] = [65, 305, 320, 411, 844, 0];
const EG_VALUE: [i32; 6] = [138, 450, 475, 816, 1957, 0];

/// Incremento de fase por peca -- 4 cavalos+4 bispos+4 torres+2 damas =
/// 4*1+4*1+4*2+2*4 = 24 = fase maxima (abertura). Fase 0 = so' reis e
/// peoes (final puro). Peao nao conta (fase so' mede pecas maiores).
const PHASE_INC: [i32; 6] = [0, 1, 1, 2, 4, 0];
const MAX_PHASE: i32 = 24;

fn mirror_idx(color: Color, s: Square) -> usize {
    if color == Color::White {
        s as usize
    } else {
        let f = file_of(s);
        let r = 7 - rank_of(s);
        (r * 8 + f) as usize
    }
}

fn pst_mg(kind: PieceType, color: Color, s: Square) -> i32 {
    let idx = mirror_idx(color, s);
    match kind {
        PieceType::Pawn => MG_PAWN[idx],
        PieceType::Knight => MG_KNIGHT[idx],
        PieceType::Bishop => MG_BISHOP[idx],
        PieceType::Rook => MG_ROOK[idx],
        PieceType::Queen => MG_QUEEN[idx],
        PieceType::King => MG_KING[idx],
    }
}

fn pst_eg(kind: PieceType, color: Color, s: Square) -> i32 {
    let idx = mirror_idx(color, s);
    match kind {
        PieceType::Pawn => EG_PAWN[idx],
        PieceType::Knight => EG_KNIGHT[idx],
        PieceType::Bishop => EG_BISHOP[idx],
        PieceType::Rook => EG_ROOK[idx],
        PieceType::Queen => EG_QUEEN[idx],
        PieceType::King => EG_KING[idx],
    }
}

/// Contribuicao (mg, eg, incremento de fase) de UMA peca numa casa, do
/// ponto de vista das BRANCAS (ja' com o sinal aplicado: positivo para
/// brancas, negativo para pretas) -- usado por board.rs para manter
/// `mg_score`/`eg_score`/`phase` actualizados incrementalmente em
/// add_piece()/remove_piece(), em vez de recalcular material_pst() do
/// zero em cada no' da busca (era o maior custo por no' que faltava
/// tornar incremental, ver "Incrementally updated evaluation" na lista
/// do Sirius).
pub fn piece_contribution(kind: PieceType, color: Color, s: Square) -> (i32, i32, i32) {
    let sign = if color == Color::White { 1 } else { -1 };
    let mg = sign * (MG_VALUE[kind.idx()] + pst_mg(kind, color, s));
    let eg = sign * (EG_VALUE[kind.idx()] + pst_eg(kind, color, s));
    (mg, eg, PHASE_INC[kind.idx()])
}

/// Zona do rei: a propria casa + as 8 vizinhas (igual ao king_attacks).
fn king_zone(king_sq: Square) -> Bitboard {
    atk().king[king_sq as usize] | bb(king_sq)
}

// Constantes tunadas do Sirius v9.0 (eval_constants.h) -- afinadas EM
// CONJUNTO com o MATERIAL e as PSQT acima por Texel Tuning. Portar as
// tabelas sem estas constantes deixa duas escalas em conflito (as
// tabelas na escala Sirius, os pesos "posicionais" a olho na escala
// PeSTO). Layout ScorePair = (mg, eg); interpolados pela fase do jogo
// em positional_terms(). Nomes na convencao do proprio Sirius --
// facilita cross-check contra o ficheiro fonte quando houver
// actualizacoes.
const BISHOP_PAIR: (i32, i32) = (22, 65);
const LONG_DIAG_BISHOP: (i32, i32) = (12, 12);
const MINOR_BEHIND_PAWN: (i32, i32) = (2, 9);
const KNIGHT_OUTPOST: (i32, i32) = (18, 18);
const ROOK_OPEN: [(i32, i32); 2] = [(29, 2), (19, -1)]; // [0]=aberta, [1]=semi-aberta
const TEMPO: (i32, i32) = (28, 20);

// MOBILITY[piece][count] -- Sirius: Knight 0..=8, Bishop 0..=13, Rook
// 0..=14, Queen 0..=27. Usamos 4 tabelas com o comprimento maximo
// possivel (a Dama). Portagem literal.
const MOBILITY_KNIGHT: [(i32, i32); 28] = [
    (-20,-101),(-34,-46),(-19,-9),(-8,8),(2,18),(8,28),(17,33),(24,36),(34,27),
    (0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),
];
const MOBILITY_BISHOP: [(i32, i32); 28] = [
    (-25,-111),(-41,-59),(-21,-27),(-10,-6),(-4,5),(0,15),(2,22),(5,26),(5,28),(9,29),(7,30),(13,23),(18,23),(48,-8),
    (0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),
];
const MOBILITY_ROOK: [(i32, i32); 28] = [
    (-58,-89),(-48,-66),(-16,-40),(-8,-23),(0,-12),(4,-2),(4,9),(6,14),(7,17),(9,23),(12,29),(13,35),(14,39),(17,41),(44,21),
    (0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),(0,0),
];
const MOBILITY_QUEEN: [(i32, i32); 28] = [
    (-29,-65),(-48,-72),(-59,-67),(-36,-132),(-24,-94),(-11,-55),(-3,-39),(1,-20),(1,-2),(2,11),(5,18),(5,28),(7,33),(7,41),
    (9,45),(10,44),(9,49),(10,49),(12,49),(14,44),(17,41),(22,28),(21,28),(31,12),(20,23),(25,0),(21,-27),(-23,4),
];

// KING_ATTACKER_WEIGHT[Knight, Bishop, Rook, Queen]
const KING_ATTACKER_WEIGHT: [(i32, i32); 4] = [(54, -2), (22, -2), (22, -7), (4, -9)];
const KING_ATTACKS: (i32, i32) = (7, 0);

// Ameacas -- Sirius: quando uma peca nossa ataca uma peca inimiga, soma
// bonus indexado pelo tipo da peca atacada; para as pecas maiores, ha
// tabela separada para "defendida pelo inimigo" vs nao. Isto e' o que
// resolve o caso classico "peao inimigo ameaca o meu peao/peca em k
// lances a frente" que a busca a profundidade baixa nao ve directamente.
// Ordem: [Pawn, Knight, Bishop, Rook, Queen, King].
const THREAT_BY_PAWN: [(i32, i32); 6] = [(-7,-19),(73,41),(65,72),(72,50),(56,24),(0,0)];
// [defended=0 nao-defendido / 1 defendido][target_piece]
const THREAT_BY_KNIGHT: [[(i32, i32); 6]; 2] = [
    [(5,37),(12,85),(50,33),(86,13),(41,8),(0,0)],
    [(-8,11),(9,79),(38,29),(71,45),(50,46),(0,0)],
];
const THREAT_BY_BISHOP: [[(i32, i32); 6]; 2] = [
    [(3,34),(36,44),(12,102),(58,35),(61,53),(0,0)],
    [(-5,4),(20,21),(4,76),(56,60),(63,74),(0,0)],
];
const THREAT_BY_ROOK: [[(i32, i32); 6]; 2] = [
    [(-3,50),(35,52),(45,49),(-12,50),(67,-10),(0,0)],
    [(-10,10),(8,15),(19,4),(1,22),(54,85),(0,0)],
];
const THREAT_BY_QUEEN: [[(i32, i32); 6]; 2] = [
    [(8,21),(25,30),(18,65),(16,12),(-2,-17),(0,0)],
    [(-5,16),(2,8),(-9,37),(-7,7),(-19,1),(0,0)],
];
const THREAT_BY_KING: [(i32, i32); 6] = [(39,18),(33,38),(99,33),(83,8),(0,0),(0,0)];
// Peca menor a atacar a dama inimiga (nao contado nos threat-by-X porque
// esses ja' cobrem a captura directa; estes sao para o padrao de
// "cavalo/bispo/torre em jogada seguinte deposita ameaca na dama").
const KNIGHT_HIT_QUEEN: (i32, i32) = (7, 2);
const BISHOP_HIT_QUEEN: (i32, i32) = (16, 15);
const ROOK_HIT_QUEEN: (i32, i32) = (18, 0);
// Bonus quando um push de peao (avanco de 1 casa) CRIARIA uma nova
// ameaca a peca inimiga. Simulado por check-single-square-forward.
const PUSH_THREAT: (i32, i32) = (13, 17);
// Casas atacadas por nos e nao defendidas pelo inimigo -- pequeno bonus
// por casa restrita ao adversario.
const RESTRICTED_SQUARES: (i32, i32) = (2, 3);

// Peoes -- todas as tabelas indexadas por rank (0=1a, 7=8a; entrada 0 e
// 7 sao 0 porque peoes nao existem la, mantidos para simplicidade de
// index).
const PAWN_PHALANX: [(i32, i32); 8] = [
    (0,0),(4,-5),(11,0),(18,10),(40,34),(70,135),(104,178),(0,0)
];
const DEFENDED_PAWN: [(i32, i32); 8] = [
    (0,0),(0,0),(16,7),(10,10),(18,24),(38,57),(71,119),(0,0)
];
// ISOLATED_PAWN[4] no Sirius e indexed por count acumulado de peoes
// isolados; simplificamos para "peso por peao isolado individualmente"
// usando o slot 0 (single isolated pawn).
const ISOLATED_PAWN: (i32, i32) = (-6, 6);
// DOUBLED_PAWN[4] no Sirius indexed por file (a/h=0, b/g=1, c/f=2,
// d/e=3). Usamos a media por simplicidade.
const DOUBLED_PAWN: (i32, i32) = (-3, -32);
// PASSED_PAWN[defended][blocked][rank]; simplificamos usando o slot
// mais comum (undefended, unblocked) e indexed by rank.
const PASSED_PAWN: [(i32, i32); 8] = [
    (0,0),(0,0),(0,0),(-31,-30),(-11,26),(22,140),(115,229),(0,0)
];

/// Avalia mobilidade, pressao sobre o rei inimigo, par de bispos, torres
/// em colunas abertas/semi-abertas e estrutura de peoes usando os pesos
/// tunados do Sirius v9.0 (ver constantes acima) -- consistente com as
/// tabelas PSQT/MATERIAL desta seccao. Acumula (mg, eg) e interpola pela
/// fase do jogo (mesma logica de material_pst). Devolve da perspetiva
/// das BRANCAS (score_white - score_black), interpolado; o chamador
/// (evaluate) converte para a convencao negamax.
/// Bitboard de todas as casas atacadas por peoes de `by`. Combinacao
/// classica: shifts diagonais dos peoes ao invez de um loop.
fn pawn_attacks_by(board: &Board, by: Color) -> Bitboard {
    let pawns = board.pieces[by.idx()][PieceType::Pawn.idx()];
    if by == Color::White {
        // brancas atacam para NW e NE (rank+1, file-1 / file+1)
        (pawns & !FILE_A) << 7 | (pawns & !FILE_H) << 9
    } else {
        // pretas atacam para SW e SE
        (pawns & !FILE_A) >> 9 | (pawns & !FILE_H) >> 7
    }
}

fn positional_terms(board: &Board) -> i32 {
    let a = atk();
    let occ = board.occ_all;
    let mut mg = 0i32;
    let mut eg = 0i32;

    // === EvalData estilo Sirius (`eval.cpp` EvalData/initEvalData) ===
    // Bitboards acumulados por cor para: casas atacadas por cada tipo
    // de peca (attacked_by_pt), casas atacadas total (attacked), casas
    // atacadas por 2 ou mais pecas (attacked_by_2). Precisamos destes
    // agregados para calcular ameacas correctamente: "peca inimiga
    // defendida" = casa atacada 2x por eles OU atacada 1x por peao
    // deles OU atacada por qualquer peca deles e nao atacada 2x por nos.
    let mut attacked_by_pt: [[Bitboard; 6]; 2] = [[0; 6]; 2];
    let mut attacked: [Bitboard; 2] = [0; 2];
    let mut attacked_by_2: [Bitboard; 2] = [0; 2];

    // Peoes: ataques diagonais em massa via shifts.
    for c in [Color::White, Color::Black] {
        let pa = pawn_attacks_by(board, c);
        attacked_by_2[c.idx()] |= attacked[c.idx()] & pa;
        attacked[c.idx()] |= pa;
        attacked_by_pt[c.idx()][PieceType::Pawn.idx()] |= pa;

        // Rei.
        let ks = board.king_sq(c);
        let ka = a.king[ks as usize];
        attacked_by_2[c.idx()] |= attacked[c.idx()] & ka;
        attacked[c.idx()] |= ka;
        attacked_by_pt[c.idx()][PieceType::King.idx()] |= ka;
    }

    let white_king_zone = king_zone(board.king_sq(Color::White));
    let black_king_zone = king_zone(board.king_sq(Color::Black));

    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let own = board.occ_color[c.idx()];
        let enemy_king_zone = if c == Color::White { black_king_zone } else { white_king_zone };
        let mut king_attackers = 0i32;
        let mut king_attack_units = (0i32, 0i32);

        if count(board.pieces[c.idx()][PieceType::Bishop.idx()]) >= 2 {
            mg += sign * BISHOP_PAIR.0;
            eg += sign * BISHOP_PAIR.1;
        }

        for pt in [PieceType::Knight, PieceType::Bishop, PieceType::Rook, PieceType::Queen] {
            let mut bbp = board.pieces[c.idx()][pt.idx()];
            while bbp != 0 {
                let s = bbp.trailing_zeros() as Square;
                bbp &= bbp - 1;
                let attacks = match pt {
                    PieceType::Knight => a.knight[s as usize],
                    PieceType::Bishop => bishop_attacks(s, occ),
                    PieceType::Rook => rook_attacks(s, occ),
                    PieceType::Queen => queen_attacks(s, occ),
                    _ => 0,
                };
                // Registar em EvalData para a fase de threats abaixo.
                attacked_by_2[c.idx()] |= attacked[c.idx()] & attacks;
                attacked[c.idx()] |= attacks;
                attacked_by_pt[c.idx()][pt.idx()] |= attacks;

                let mobility = count(attacks & !own) as usize;
                let mob_table = match pt {
                    PieceType::Knight => &MOBILITY_KNIGHT,
                    PieceType::Bishop => &MOBILITY_BISHOP,
                    PieceType::Rook => &MOBILITY_ROOK,
                    PieceType::Queen => &MOBILITY_QUEEN,
                    _ => &MOBILITY_KNIGHT,
                };
                let m = mob_table[mobility.min(27)];
                mg += sign * m.0;
                eg += sign * m.1;

                let hits = count(attacks & enemy_king_zone) as i32;
                if hits > 0 {
                    king_attackers += 1;
                    let widx = match pt {
                        PieceType::Knight => 0,
                        PieceType::Bishop => 1,
                        PieceType::Rook => 2,
                        PieceType::Queen => 3,
                        _ => 0,
                    };
                    let w = KING_ATTACKER_WEIGHT[widx];
                    king_attack_units.0 += w.0 + hits * KING_ATTACKS.0;
                    king_attack_units.1 += w.1 + hits * KING_ATTACKS.1;
                }

                if pt == PieceType::Rook {
                    let file_mask = FILE_A << file_of(s);
                    let own_pawns_on_file = board.pieces[c.idx()][PieceType::Pawn.idx()] & file_mask;
                    let enemy_pawns_on_file = board.pieces[c.opp().idx()][PieceType::Pawn.idx()] & file_mask;
                    if own_pawns_on_file == 0 {
                        let idx = if enemy_pawns_on_file == 0 { 0 } else { 1 };
                        mg += sign * ROOK_OPEN[idx].0;
                        eg += sign * ROOK_OPEN[idx].1;
                    }
                }

                if pt == PieceType::Knight || pt == PieceType::Bishop {
                    let f = file_of(s) as i32;
                    let r = rank_of(s) as i32;
                    let front_r = if c == Color::White { r + 1 } else { r - 1 };
                    if (0..8).contains(&front_r)
                        && board.pieces[c.idx()][PieceType::Pawn.idx()] & bb(sq(f as u8, front_r as u8)) != 0
                    {
                        mg += sign * MINOR_BEHIND_PAWN.0;
                        eg += sign * MINOR_BEHIND_PAWN.1;
                    }

                    if pt == PieceType::Knight {
                        let own_side_rank = if c == Color::White { r } else { 7 - r };
                        if (3..=5).contains(&own_side_rank) {
                            let defended = a.pawn[c.opp().idx()][s as usize] & board.pieces[c.idx()][PieceType::Pawn.idx()] != 0;
                            let mut ever_attackable = false;
                            for adj in [f - 1, f + 1] {
                                if (0..8).contains(&adj) && board.pieces[c.opp().idx()][PieceType::Pawn.idx()] & (FILE_A << adj) != 0 {
                                    ever_attackable = true;
                                }
                            }
                            if defended && !ever_attackable {
                                mg += sign * KNIGHT_OUTPOST.0;
                                eg += sign * KNIGHT_OUTPOST.1;
                            }
                        }
                    }
                }

                if pt == PieceType::Bishop {
                    let center: Bitboard = bb(sq(3, 3)) | bb(sq(4, 3)) | bb(sq(3, 4)) | bb(sq(4, 4));
                    if count(attacks & center) >= 2 {
                        mg += sign * LONG_DIAG_BISHOP.0;
                        eg += sign * LONG_DIAG_BISHOP.1;
                    }
                }
            }
        }

        if king_attackers >= 2 {
            mg += sign * king_attack_units.0;
            eg += sign * king_attack_units.1;
        }
    }

    // === Ameacas (Sirius `evaluateThreats`) ===
    // Aplica por cor: bonus para cada peca inimiga que a nossa peca de
    // tipo X ATACA, indexada pelo tipo alvo e por "defended".
    // defended = attackedBy2[them] | attackedBy[them][PAWN] |
    //            (attacked[them] & ~attackedBy2[us])
    // (formula literal do Sirius; a intuicao: peca inimiga esta
    // "defendida" se qq peca ou peao deles defende, EXCEPTO quando nos
    // temos MAIS atacantes que eles defensores.)
    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let us = c.idx();
        let them = c.opp().idx();
        let their_pieces = board.occ_color[them];
        let their_queen = board.pieces[them][PieceType::Queen.idx()];
        let their_king = board.pieces[them][PieceType::King.idx()];

        let defended_bb: Bitboard = attacked_by_2[them]
            | attacked_by_pt[them][PieceType::Pawn.idx()]
            | (attacked[them] & !attacked_by_2[us]);

        // Threats por peao.
        let mut t = attacked_by_pt[us][PieceType::Pawn.idx()] & their_pieces;
        while t != 0 {
            let s = t.trailing_zeros() as Square;
            t &= t - 1;
            if let Some((pt, _)) = board.piece_at(s) {
                mg += sign * THREAT_BY_PAWN[pt.idx()].0;
                eg += sign * THREAT_BY_PAWN[pt.idx()].1;
            }
        }
        // Threats por cavalo/bispo/torre/dama.
        for (pt_us, table) in [
            (PieceType::Knight, &THREAT_BY_KNIGHT),
            (PieceType::Bishop, &THREAT_BY_BISHOP),
            (PieceType::Rook, &THREAT_BY_ROOK),
            (PieceType::Queen, &THREAT_BY_QUEEN),
        ] {
            let mut t = attacked_by_pt[us][pt_us.idx()] & their_pieces;
            // Dama nao conta ameacas ao rei (o mate cobre isso).
            if pt_us == PieceType::Queen {
                t &= !their_king;
            }
            while t != 0 {
                let s = t.trailing_zeros() as Square;
                t &= t - 1;
                let defended = (defended_bb & bb(s)) != 0;
                if let Some((tgt, _)) = board.piece_at(s) {
                    let entry = table[defended as usize][tgt.idx()];
                    mg += sign * entry.0;
                    eg += sign * entry.1;
                }
            }
        }
        // Threats por rei -- so' contra pecas nao-defendidas.
        let mut t = attacked_by_pt[us][PieceType::King.idx()] & their_pieces & !defended_bb;
        while t != 0 {
            let s = t.trailing_zeros() as Square;
            t &= t - 1;
            if let Some((pt, _)) = board.piece_at(s) {
                mg += sign * THREAT_BY_KING[pt.idx()].0;
                eg += sign * THREAT_BY_KING[pt.idx()].1;
            }
        }

        // Restricted squares: casas onde nos temos 2+ atacantes, eles
        // nao tem 2+, mas eles atacam pelo menos 1 vez. Sirius:
        // attackedBy2[us] & ~attackedBy2[them] & attacked[them].
        let restricted = attacked_by_2[us] & !attacked_by_2[them] & attacked[them];
        let n_restr = count(restricted) as i32;
        mg += sign * RESTRICTED_SQUARES.0 * n_restr;
        eg += sign * RESTRICTED_SQUARES.1 * n_restr;

        // Push threats: um peao nosso pode avancar 1 casa (ou 2 se
        // ainda esta' na fileira inicial) para uma casa "segura" e
        // ATACAR uma peca nao-peao inimiga a partir dai. `safe` =
        // casas nao defendidas OU casas onde nos temos mais atacantes.
        let empty = !occ;
        let own_pawns = board.pieces[us][PieceType::Pawn.idx()];
        let one_push = if c == Color::White {
            (own_pawns << 8) & empty
        } else {
            (own_pawns >> 8) & empty
        };
        // Second push (para peoes na fileira inicial, o "empurrao
        // duplo"): sobre o subconjunto do one_push que caiu na 3a
        // fileira relativa.
        let rank3_bb: Bitboard = if c == Color::White { RANK_3 } else { RANK_6 };
        let two_push = if c == Color::White {
            ((one_push & rank3_bb) << 8) & empty
        } else {
            ((one_push & rank3_bb) >> 8) & empty
        };
        let pushes = one_push | two_push;
        let safe = !defended_bb
            | (attacked[us] & !attacked_by_pt[them][PieceType::Pawn.idx()] & !attacked_by_2[them]);
        let safe_pushes = pushes & safe;
        // Casas atacadas por peoes-nossos-simulados-nas-safe_pushes:
        let push_attacks_on_enemy = if c == Color::White {
            ((safe_pushes & !FILE_A) << 7) | ((safe_pushes & !FILE_H) << 9)
        } else {
            ((safe_pushes & !FILE_A) >> 9) | ((safe_pushes & !FILE_H) >> 7)
        };
        let non_pawn_enemies = their_pieces & !board.pieces[them][PieceType::Pawn.idx()];
        let n_push_threats = count(push_attacks_on_enemy & non_pawn_enemies) as i32;
        mg += sign * PUSH_THREAT.0 * n_push_threats;
        eg += sign * PUSH_THREAT.1 * n_push_threats;

        // Hit-queen: peca menor/torre nossa esta' a UMA-JOGADA de
        // atacar a dama inimiga a partir de casa segura.
        if count(their_queen) == 1 {
            let qs = their_queen.trailing_zeros() as Square;
            let targets_base = safe & !own_pawns;
            let knight_hits = a.knight[qs as usize];
            let bishop_hits = bishop_attacks(qs, occ);
            let rook_hits = rook_attacks(qs, occ);
            // Sirius: knight hits nao precisa de attackedBy2[us], mas
            // bishop/rook precisam (targets &= attackedBy2[us]).
            let n_knight_hit = count(targets_base & knight_hits & attacked_by_pt[us][PieceType::Knight.idx()]) as i32;
            mg += sign * KNIGHT_HIT_QUEEN.0 * n_knight_hit;
            eg += sign * KNIGHT_HIT_QUEEN.1 * n_knight_hit;
            let targets_double = targets_base & attacked_by_2[us];
            let n_bishop_hit = count(targets_double & bishop_hits & attacked_by_pt[us][PieceType::Bishop.idx()]) as i32;
            mg += sign * BISHOP_HIT_QUEEN.0 * n_bishop_hit;
            eg += sign * BISHOP_HIT_QUEEN.1 * n_bishop_hit;
            let n_rook_hit = count(targets_double & rook_hits & attacked_by_pt[us][PieceType::Rook.idx()]) as i32;
            mg += sign * ROOK_HIT_QUEEN.0 * n_rook_hit;
            eg += sign * ROOK_HIT_QUEEN.1 * n_rook_hit;
        }
    }

    // === Estrutura de peoes (mantem-se por cor, dentro de novo loop) ===
    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };

        // Estrutura de peoes.
        let own_pawns = board.pieces[c.idx()][PieceType::Pawn.idx()];
        let enemy_pawns = board.pieces[c.opp().idx()][PieceType::Pawn.idx()];
        let mut pawns = own_pawns;
        while pawns != 0 {
            let s = pawns.trailing_zeros() as Square;
            pawns &= pawns - 1;
            let f = file_of(s) as i32;
            let r = rank_of(s) as i32;
            let rel_rank = if c == Color::White { r as usize } else { (7 - r) as usize };

            // Peao passado.
            let mut blocked = false;
            for adj in (f - 1)..=(f + 1) {
                if !(0..8).contains(&adj) { continue; }
                let mut m: Bitboard = 0;
                if c == Color::White {
                    for rr in (r + 1)..8 { m |= bb(sq(adj as u8, rr as u8)); }
                } else {
                    for rr in 0..r { m |= bb(sq(adj as u8, rr as u8)); }
                }
                if enemy_pawns & m != 0 { blocked = true; break; }
            }
            if !blocked {
                mg += sign * PASSED_PAWN[rel_rank].0;
                eg += sign * PASSED_PAWN[rel_rank].1;
            }

            // Peao isolado.
            let mut has_neighbor = false;
            for adj in (f - 1)..=(f + 1) {
                if adj == f || !(0..8).contains(&adj) { continue; }
                if own_pawns & (FILE_A << adj) != 0 { has_neighbor = true; break; }
            }
            if !has_neighbor {
                mg += sign * ISOLATED_PAWN.0;
                eg += sign * ISOLATED_PAWN.1;
            }

            // Peao defendido por outro peao proprio (usa mesmo truque
            // reversed pawn-attack table do SEE em search.rs).
            if a.pawn[c.opp().idx()][s as usize] & own_pawns != 0 {
                mg += sign * DEFENDED_PAWN[rel_rank].0;
                eg += sign * DEFENDED_PAWN[rel_rank].1;
            }

            // Falange (outro peao proprio na mesma fileira, coluna
            // adjacente).
            for adj in [f - 1, f + 1] {
                if (0..8).contains(&adj) && own_pawns & bb(sq(adj as u8, r as u8)) != 0 {
                    mg += sign * PAWN_PHALANX[rel_rank].0;
                    eg += sign * PAWN_PHALANX[rel_rank].1;
                    break;
                }
            }
        }

        // Peoes dobrados (por peao excedente na mesma coluna).
        for file in 0..8 {
            let n = count(own_pawns & (FILE_A << file)) as i32;
            if n > 1 {
                mg += sign * DOUBLED_PAWN.0 * (n - 1);
                eg += sign * DOUBLED_PAWN.1 * (n - 1);
            }
        }
    }

    // Tempo -- bonus para quem tem a jogar. Aplicado como (mg,eg) do
    // ponto de vista das brancas: se e' a vez das brancas, +TEMPO; se
    // e' a vez das pretas, -TEMPO. Sirius aplica assim mesmo.
    let tempo_sign = if board.side == Color::White { 1 } else { -1 };
    mg += tempo_sign * TEMPO.0;
    eg += tempo_sign * TEMPO.1;

    // Interpolacao final pela fase actual do board (mesma logica de
    // material_pst; fase mantida incrementalmente em add_piece/
    // remove_piece).
    let phase = board.phase.min(MAX_PHASE);
    (mg * phase + eg * (MAX_PHASE - phase)) / MAX_PHASE
}

/// Avaliacao: material + PST + termos posicionais/taticos ("estilo
/// Polgar" -- pressao sobre o rei inimigo, mobilidade pesada, iniciativa
/// via peoes passados e torres ativas). Devolve da perspetiva de quem
/// tem a jogar (convencao negamax).
///
/// 2026-07-20 (teste A/B -- investigacao da queda de resultados, ver
/// NOTAS_PROXIMA_SESSAO.md "proximos passos" #1): a variavel de ambiente
/// KESTREL_EVAL_MODE=material desliga positional_terms_signed por
/// completo, isolando se os termos "Polgar" ajudam ou atrapalham face a
/// so' material+PST. Por omissao (variavel ausente ou qualquer outro
/// valor) o comportamento fica EXATAMENTE como antes -- nada muda para o
/// motor "normal" que a arena ja usa. Ler o env UMA vez (OnceLock),
/// nao a cada chamada de evaluate() (custaria NPS real).
static EVAL_MODE_MATERIAL_ONLY: OnceLock<bool> = OnceLock::new();
fn eval_mode_material_only() -> bool {
    *EVAL_MODE_MATERIAL_ONLY.get_or_init(|| {
        std::env::var("KESTREL_EVAL_MODE").map(|v| v == "material").unwrap_or(false)
    })
}
pub fn evaluate(board: &Board) -> i32 {
    if eval_mode_material_only() {
        material_pst(board)
    } else {
        material_pst(board) + positional_terms_signed(board)
    }
}

/// So' material + PST, sem os termos posicionais caros (mobilidade/
/// varrimento de ataques). Usada na quiescence, onde a busca ja' passa
/// por MUITOS nos so' para resolver capturas -- pedido explicito: "ela
/// tem de poder jogar bullet com as suas tecnicas". A riqueza posicional
/// fica reservada aos nos reais do negamax, onde influencia a escolha de
/// lances; na quiescence so' precisamos de um "stand pat" rapido e
/// decente. Reduz bastante o custo por no sem perder a personalidade nas
/// decisoes que realmente importam.
pub fn evaluate_fast(board: &Board) -> i32 {
    material_pst(board)
}

/// Le' os acumuladores incrementais mantidos por add_piece()/remove_piece()
/// (ver board.rs) em vez de percorrer todas as pecas -- era a soma mais
/// cara paga em TODOS os nos (evaluate_fast() e' chamada em RFP/razoring/
/// futility/IID, e evaluate() chama-a tambem via este material_pst()).
/// A soma completa (loop por todas as pecas) so' acontece uma vez, na
/// construcao do board (ver Board::recompute_eval_accumulators).
fn material_pst(board: &Board) -> i32 {
    let phase = board.phase.min(MAX_PHASE);
    let score = (board.mg_score * phase + board.eg_score * (MAX_PHASE - phase)) / MAX_PHASE;
    if board.side == Color::White {
        score
    } else {
        -score
    }
}

fn positional_terms_signed(board: &Board) -> i32 {
    let p = positional_terms(board);
    if board.side == Color::White {
        p
    } else {
        -p
    }
}
