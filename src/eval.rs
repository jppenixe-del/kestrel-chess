use crate::attacks::*;
use crate::bitboard::*;
use crate::board::Board;
use crate::types::*;
use std::sync::OnceLock;

static ATTACKS: OnceLock<Attacks> = OnceLock::new();
fn atk() -> &'static Attacks {
    ATTACKS.get_or_init(Attacks::new)
}

// Tapered piece-square tables (perspetiva das brancas, a1=indice 0 ..
// h8=indice 63 -- peca preta usa espelho vertical). PSQT public
// "PeSTO" (chessprogramming wiki), ponto de partida educacional
// clássico -- valores para afinar via Texel Tuning proprio a seguir,
// nao um estado final. Convertidas de rank8-first (formato da pagina)
// para rank1-first (convencao deste codigo).
#[rustfmt::skip]
const MG_PAWN: [i32; 64] = [
       0,    0,    0,    0,    0,    0,    0,    0,
     -35,   -1,  -20,  -23,  -15,   24,   38,  -22,
     -26,   -4,   -4,  -10,    3,    3,   33,  -12,
     -27,   -2,   -5,   12,   17,    6,   10,  -25,
     -14,   13,    6,   21,   23,   12,   17,  -23,
      -6,    7,   26,   31,   65,   56,   25,  -20,
      98,  134,   61,   95,   68,  126,   34,  -11,
       0,    0,    0,    0,    0,    0,    0,    0,
];
#[rustfmt::skip]
const EG_PAWN: [i32; 64] = [
       0,    0,    0,    0,    0,    0,    0,    0,
      13,    8,    8,   10,   13,    0,    2,   -7,
       4,    7,   -6,    1,    0,   -5,   -1,   -8,
      13,    9,   -3,   -7,   -7,   -8,    3,   -1,
      32,   24,   13,    5,   -2,    4,   17,   17,
      94,  100,   85,   67,   56,   53,   82,   84,
     178,  173,  158,  134,  147,  132,  165,  187,
       0,    0,    0,    0,    0,    0,    0,    0,
];
#[rustfmt::skip]
const MG_KNIGHT: [i32; 64] = [
    -105,  -21,  -58,  -33,  -17,  -28,  -19,  -23,
     -29,  -53,  -12,   -3,   -1,   18,  -14,  -19,
     -23,   -9,   12,   10,   19,   17,   25,  -16,
     -13,    4,   16,   13,   28,   19,   21,   -8,
      -9,   17,   19,   53,   37,   69,   18,   22,
     -47,   60,   37,   65,   84,  129,   73,   44,
     -73,  -41,   72,   36,   23,   62,    7,  -17,
    -167,  -89,  -34,  -49,   61,  -97,  -15, -107,
];
#[rustfmt::skip]
const EG_KNIGHT: [i32; 64] = [
     -29,  -51,  -23,  -15,  -22,  -18,  -50,  -64,
     -42,  -20,  -10,   -5,   -2,  -20,  -23,  -44,
     -23,   -3,   -1,   15,   10,   -3,  -20,  -22,
     -18,   -6,   16,   25,   16,   17,    4,  -18,
     -17,    3,   22,   22,   22,   11,    8,  -18,
     -24,  -20,   10,    9,   -1,   -9,  -19,  -41,
     -25,   -8,  -25,   -2,   -9,  -25,  -24,  -52,
     -58,  -38,  -13,  -28,  -31,  -27,  -63,  -99,
];
#[rustfmt::skip]
const MG_BISHOP: [i32; 64] = [
     -33,   -3,  -14,  -21,  -13,  -12,  -39,  -21,
       4,   15,   16,    0,    7,   21,   33,    1,
       0,   15,   15,   15,   14,   27,   18,   10,
      -6,   13,   13,   26,   34,   12,   10,    4,
      -4,    5,   19,   50,   37,   37,    7,   -2,
     -16,   37,   43,   40,   35,   50,   37,   -2,
     -26,   16,  -18,  -13,   30,   59,   18,  -47,
     -29,    4,  -82,  -37,  -25,  -42,    7,   -8,
];
#[rustfmt::skip]
const EG_BISHOP: [i32; 64] = [
     -23,   -9,  -23,   -5,   -9,  -16,   -5,  -17,
     -14,  -18,   -7,   -1,    4,   -9,  -15,  -27,
     -12,   -3,    8,   10,   13,    3,   -7,  -15,
      -6,    3,   13,   19,    7,   10,   -3,   -9,
      -3,    9,   12,    9,   14,   10,    3,    2,
       2,   -8,    0,   -1,   -2,    6,    0,    4,
      -8,   -4,    7,  -12,   -3,  -13,   -4,  -14,
     -14,  -21,  -11,   -8,   -7,   -9,  -17,  -24,
];
#[rustfmt::skip]
const MG_ROOK: [i32; 64] = [
     -19,  -13,    1,   17,   16,    7,  -37,  -26,
     -44,  -16,  -20,   -9,   -1,   11,   -6,  -71,
     -45,  -25,  -16,  -17,    3,    0,   -5,  -33,
     -36,  -26,  -12,   -1,    9,   -7,    6,  -23,
     -24,  -11,    7,   26,   24,   35,   -8,  -20,
      -5,   19,   26,   36,   17,   45,   61,   16,
      27,   32,   58,   62,   80,   67,   26,   44,
      32,   42,   32,   51,   63,    9,   31,   43,
];
#[rustfmt::skip]
const EG_ROOK: [i32; 64] = [
      -9,    2,    3,   -1,   -5,  -13,    4,  -20,
      -6,   -6,    0,    2,   -9,   -9,  -11,   -3,
      -4,    0,   -5,   -1,   -7,  -12,   -8,  -16,
       3,    5,    8,    4,   -5,   -6,   -8,  -11,
       4,    3,   13,    1,    2,    1,   -1,    2,
       7,    7,    7,    5,    4,   -3,   -5,   -3,
      11,   13,   13,   11,   -3,    3,    8,    3,
      13,   10,   18,   15,   12,   12,    8,    5,
];
#[rustfmt::skip]
const MG_QUEEN: [i32; 64] = [
      -1,  -18,   -9,   10,  -15,  -25,  -31,  -50,
     -35,   -8,   11,    2,    8,   15,   -3,    1,
     -14,    2,  -11,   -2,   -5,    2,   14,    5,
      -9,  -26,   -9,  -10,   -2,   -4,    3,   -3,
     -27,  -27,  -16,  -16,   -1,   17,   -2,    1,
     -13,  -17,    7,    8,   29,   56,   47,   57,
     -24,  -39,   -5,    1,  -16,   57,   28,   54,
     -28,    0,   29,   12,   59,   44,   43,   45,
];
#[rustfmt::skip]
const EG_QUEEN: [i32; 64] = [
     -33,  -28,  -22,  -43,   -5,  -32,  -20,  -41,
     -22,  -23,  -30,  -16,  -16,  -23,  -36,  -32,
     -16,  -27,   15,    6,    9,   17,   10,    5,
     -18,   28,   19,   47,   31,   34,   39,   23,
       3,   22,   24,   45,   57,   40,   57,   36,
     -20,    6,    9,   49,   47,   35,   19,    9,
     -17,   20,   32,   41,   58,   25,   30,    0,
      -9,   22,   22,   27,   27,   19,   10,   20,
];
#[rustfmt::skip]
const MG_KING: [i32; 64] = [
     -15,   36,   12,  -54,    8,  -28,   24,   14,
       1,    7,   -8,  -64,  -43,  -16,    9,    8,
     -14,  -14,  -22,  -46,  -44,  -30,  -15,  -27,
     -49,   -1,  -27,  -39,  -46,  -44,  -33,  -51,
     -17,  -20,  -12,  -27,  -30,  -25,  -14,  -36,
      -9,   24,    2,  -16,  -20,    6,   22,  -22,
      29,   -1,  -20,   -7,   -8,   -4,  -38,  -29,
     -65,   23,   16,  -15,  -56,  -34,    2,   13,
];
#[rustfmt::skip]
const EG_KING: [i32; 64] = [
     -53,  -34,  -21,  -11,  -28,  -14,  -24,  -43,
     -27,  -11,    4,   13,   14,    4,   -5,  -17,
     -19,   -3,   11,   21,   23,   16,    7,   -9,
     -18,   -4,   21,   24,   27,   23,    9,  -11,
      -8,   22,   24,   27,   26,   33,   26,    3,
      10,   17,   23,   15,   20,   45,   44,   13,
     -12,   17,   14,   17,   17,   38,   23,   11,
     -74,  -35,  -18,  -18,  -11,   15,    4,  -17,
];

/// Material tapered. Raciocinio:
///  - Peao 100 mg / 115 eg: valor classico "1 pawn = 100cp" no mg;
///    no eg o peao vale mais (proximidade da promocao, menos pecas
///    para o parar).
///  - Cavalo 320 mg / 285 eg: cavalo perde valor sem outras pecas por
///    perto para saltar (a mobilidade eficaz baixa no eg).
///  - Bispo 335 mg / 335 eg: bispo mantem valor no eg (diagonais
///    abertas com menos pecas).
///  - Torre 500 mg / 550 eg: torre ganha no eg (colunas abertas, 7a
///    fileira).
///  - Dama 950 mg / 960 eg: dama mantem-se (ambas fases).
///  - Rei 0: nao conta na soma material.
/// Distintos de PieceType::value() (usado por SEE/MVV-LVA sem fase).
const MG_VALUE: [i32; 6] = [125, 340, 355, 520, 990, 0];
const EG_VALUE: [i32; 6] = [140, 300, 350, 570, 990, 0];

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

// Pesos de eval -- valores proprios sensatos como ponto de partida,
// para afinar via Texel Tuning proprio (ver src/tuning/ quando existir).
// Formato ScorePair (mg, eg), interpolados em positional_terms() pela
// fase actual. Estrutura (mobility por peca e count, threats indexadas
// por defended, king safety com attackers+attack_count, etc.) e' a
// padrao de qualquer motor HCE forte -- Stockfish, Ethereal, Berserk,
// Sirius, todos usam essencialmente a mesma organizacao. Os valores
// abaixo sao meus -- monotonos e razoaveis, mas nao afinados. Isso e'
// o proximo passo (Texel Tuning no server).

// === Structural bonuses (raciocinio explicito) ===
// BISHOP_PAIR: dois bispos cobrem todas as cores; vantagem cresce no
// eg (diagonais abertas com menos pecas). Valor classico 30-50.
const BISHOP_PAIR: (i32, i32) = (30, 55);
// Bispo na longa diagonal central (a1-h8 ou h1-a8) que ataca >=2 casas
// do centro (d4/e4/d5/e5) -- peca activa, small bonus.
const LONG_DIAG_BISHOP: (i32, i32) = (10, 12);
// Cavalo/bispo com peao proprio directamente a frente -- abrigo,
// pequeno bonus por seguranca.
const MINOR_BEHIND_PAWN: (i32, i32) = (5, 6);
// Cavalo em outpost (casa avancada, defendida por peao, sem peao
// inimigo nas colunas adjacentes que possa capturar) -- peca dominante.
const KNIGHT_OUTPOST: (i32, i32) = (25, 20);
// Torre em coluna aberta / semi-aberta. Aberta = mais mg (linhas de
// ataque), eg mantem-se. Semi-open = metade.
const ROOK_OPEN: [(i32, i32); 2] = [(30, 12), (18, 8)];
// Tempo -- lado que joga tem pequena vantagem estrutural (iniciativa).
// Valor classico 15-25.
const TEMPO: (i32, i32) = (22, 15);

// === Mobility ===
// Ideia geral: 0 lances legais = peca presa, penalidade forte. Curva
// concava crescente ate' plateau (mobilidade extra alem de "activa" da'
// diminishing returns). Piece-specific: dama tem 27 slots mas o valor
// da mobilidade e' menor em cada slot (dama ja' e' potente sem precisar
// de mobility). Cavalo tem so' 8 slots mas cada casa vale mais (cavalo
// preso em canto vale muito pouco). eg = ligeiramente mais baixo que
// mg em geral (mobility conta menos com menos pecas para interagir).
const MOBILITY_KNIGHT: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    // 0..=8 lances
    let mg = [-40, -15, -5, 5, 12, 18, 25, 30, 35];
    let eg = [-32, -14, -5, 3, 9, 14, 18, 22, 25];
    let mut i = 0; while i < 9 { t[i] = (mg[i], eg[i]); i += 1; }
    t
};
const MOBILITY_BISHOP: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    // 0..=13 lances
    let mg = [-40, -20, -8, 0, 7, 13, 18, 22, 25, 28, 30, 32, 34, 36];
    let eg = [-30, -18, -8, -2, 5, 10, 14, 18, 20, 22, 24, 25, 26, 27];
    let mut i = 0; while i < 14 { t[i] = (mg[i], eg[i]); i += 1; }
    t
};
const MOBILITY_ROOK: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    // 0..=14 lances -- torre ganha mais no eg (colunas abertas)
    let mg = [-45, -25, -12, -4, 2, 7, 12, 16, 20, 23, 25, 27, 28, 28, 28];
    let eg = [-35, -22, -12, -4, 3, 8, 13, 18, 23, 28, 30, 32, 33, 34, 34];
    let mut i = 0; while i < 15 { t[i] = (mg[i], eg[i]); i += 1; }
    t
};
const MOBILITY_QUEEN: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    // 0..=27 lances. Cada slot vale menos (dama ja' e' potente).
    // Plateau depois de ~20 lances.
    let mg = [-30, -25, -15, -8, -3, 2, 6, 10, 13, 16, 18, 20, 22, 23, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24, 24];
    let eg = [-25, -20, -15, -8, -3, 2, 5, 8, 11, 14, 16, 18, 20, 21, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22, 22];
    let mut i = 0; while i < 28 { t[i] = (mg[i], eg[i]); i += 1; }
    t
};

// === King safety ===
// Peso por peca a atacar a zona do rei inimigo. Dama pesa MUITO (peca
// suprema no ataque), torre pesa ~2x menor, menores pesam menos.
// eg = negativo pequeno -- ataques ao rei importam pouco quando ja
// nao ha muitas pecas para atacar. Baseado no padrao classico
// "attack units" do Stockfish clássico.
const KING_ATTACKER_WEIGHT: [(i32, i32); 4] = [
    (20, -3),   // Cavalo
    (18, -3),   // Bispo
    (35, -5),   // Torre
    (65, -5),   // Dama
];
// Extra por casa da king zone atacada, alem do bonus por atacante.
const KING_ATTACKS: (i32, i32) = (5, 0);

// King danger units (mg channel of the accumulation above) go through
// this saturating, roughly-quadratic lookup before being added to the
// score, instead of straight in. Classical engines all do this
// (Stockfish's pre-NNUE king safety and derivatives): several
// attackers combining is much more than additively dangerous, because
// they can cover each other's escape squares/overload defenders in a
// way a lone piece can't. A flat linear sum lets a single lurking
// queen (65 units) already outweigh real pawn-shelter damage
// regardless of backup. Table is self-derived (identity below the
// ~100-unit mark that one or two ordinary attackers land in -- keeps
// today's already-validated single/double-attacker behaviour
// unchanged -- then grows superlinearly once several attackers
// combine past that, capped so it can never swamp material). Not
// copied from any specific engine's tuned safety table.
const KING_DANGER_TABLE: [i32; 128] = {
    let mut t = [0i32; 128];
    let mut i = 0;
    while i < 128 {
        let d = i as i32;
        let v = if d <= 100 { d } else { 100 + (d - 100) * (d - 100) / 40 };
        t[i] = if v > 500 { 500 } else { v };
        i += 1;
    }
    t
};

// Pawn shelter/storm: indexado por "offset" (distancia em ranks entre o
// rei e o peao relevante, offset=1 e' o peao imediatamente a frente).
// Shelter (peao proprio): offset 1 intacto = zero custo; cada rank extra
// de avanco e' abrigo perdido sem ganho nenhum em troca. Storm (peao
// inimigo): o inverso -- quanto mais perto do rei, mais perigoso.
const PAWN_SHELTER: [(i32, i32); 4] = [(0, 0), (-10, -2), (-24, -4), (-34, -6)];
const SHELTER_OPEN: (i32, i32) = (-30, -6);
const PAWN_STORM: [(i32, i32); 4] = [(-38, -8), (-22, -5), (-10, -2), (0, 0)];

/// Rank offset (sempre positivo, "para a frente" do rei) do peao mais
/// perto do rei nesta bitboard (ja filtrada a uma unica coluna). `None`
/// se nao houver nenhum peao dessa cor "a frente" do rei nessa coluna.
fn shield_pawn_offset(pawns_on_file: Bitboard, king_rank: i32, white: bool) -> Option<i32> {
    let mut bbp = pawns_on_file;
    let mut best: Option<i32> = None;
    while bbp != 0 {
        let s = bbp.trailing_zeros() as Square;
        bbp &= bbp - 1;
        let r = rank_of(s) as i32;
        let off = if white { r - king_rank } else { king_rank - r };
        if off > 0 {
            best = Some(best.map_or(off, |b| b.min(off)));
        }
    }
    best
}

// === Threats ===
// Estrutura standard (indexed por tipo da peca atacada e por
// "defendida pelo inimigo?"). Raciocinio para os valores:
//
// UNDEFENDED = ganho de material em quase todos os casos. O bonus
// aproxima o valor da peca ganha, com desconto por: possivel fuga
// do alvo, tempo consumido, contra-ameaca. Tipico ~50-70% do valor
// nominal.
//
// DEFENDED = recaptura, quase sempre equal ou pequeno ganho. Peao
// defendido a peao defendido vale zero (recaptura pura); dama
// defendida atacada por menor vale eg mais no eg (troca de dama por
// menor + peao passado por baixo pressao).
//
// Ordem interna: [Pawn, Knight, Bishop, Rook, Queen, King].

// THREAT_BY_PAWN: um peao vale 100mg; ganhar 1 peao com um peao vale
// ~70mg (peao inicial pode ser recapturado se defendido depois; se
// pendurado tira ~1 peao inteiro).
const THREAT_BY_PAWN: [[(i32, i32); 6]; 2] = [
    // undefended (peao inimigo pendurado) - ganho quase full material
    [(70, 60), (85, 55), (85, 55), (95, 55), (85, 40), (0, 0)],
    // defended - trocamos peao por peao (equal); vs peca maior, ainda
    // ganho porque a peca tem de sair. Especialmente eg.
    [(0, 5), (25, 15), (25, 15), (30, 20), (25, 10), (0, 0)],
];

// THREAT_BY_KNIGHT: cavalo pode forkar 2 pecas (bonus grande vs torre/
// dama undefended). Cavalo x cavalo = 0 (troca), cavalo x bispo pequena
// pressao. Rook fork por cavalo e' patente 200+cp mas so' considera
// aqui uma ameaca simples.
const THREAT_BY_KNIGHT: [[(i32, i32); 6]; 2] = [
    // undefended
    [(5, 20), (0, 0), (30, 25), (65, 25), (50, 25), (0, 0)],
    // defended
    [(0, 5), (0, 0), (15, 15), (35, 20), (35, 25), (0, 0)],
];

// THREAT_BY_BISHOP: bispo x torre e' padrao "attack on rank" tipico.
// Bispo x bispo = 0 (troca), bispo x dama vale mais eg (dama nao pode
// facilmente sair da diagonal).
const THREAT_BY_BISHOP: [[(i32, i32); 6]; 2] = [
    // undefended
    [(5, 20), (30, 25), (0, 0), (60, 25), (45, 45), (0, 0)],
    // defended
    [(0, 5), (12, 15), (0, 0), (35, 25), (40, 50), (0, 0)],
];

// THREAT_BY_ROOK: torre x cavalo/bispo = pressao clara (torre vale
// mais). Torre x torre = troca equal. Torre x dama = grande bonus.
const THREAT_BY_ROOK: [[(i32, i32); 6]; 2] = [
    // undefended
    [(0, 20), (30, 30), (30, 30), (0, 0), (55, 25), (0, 0)],
    // defended
    [(-5, 5), (5, 12), (10, 8), (0, 0), (40, 55), (0, 0)],
];

// THREAT_BY_QUEEN: dama e' o topo, atacar peca inimiga menor com dama
// e' pressao mas nao tanto (dama nao quer trocar por peca menor).
// Bonus modesto. Se defendida, e' quase mau para nos (dama presa).
const THREAT_BY_QUEEN: [[(i32, i32); 6]; 2] = [
    // undefended
    [(5, 15), (18, 20), (18, 22), (12, 10), (0, 0), (0, 0)],
    // defended
    [(0, 5), (0, 5), (-5, 15), (-5, 5), (0, 0), (0, 0)],
];

// THREAT_BY_KING: rei so' ataca coisas se nao defendidas (senao morre).
// Padrao end-game (rei activo).
const THREAT_BY_KING: [(i32, i32); 6] = [(30, 20), (35, 30), (65, 25), (55, 10), (0, 0), (0, 0)];

// Hit-queen: peca menor a UM movimento de atacar a dama inimiga (a
// partir de casa segura). Valores baixos (a ameaca ainda nao aconteceu).
const KNIGHT_HIT_QUEEN: (i32, i32) = (8, 5);
const BISHOP_HIT_QUEEN: (i32, i32) = (14, 12);
const ROOK_HIT_QUEEN: (i32, i32) = (14, 5);
// Peao a UM push de atacar peca inimiga nao-peao (a partir de casa
// safe). Padrao "pawn storm creates threat".
const PUSH_THREAT: (i32, i32) = (12, 15);
// Casas restritas ao adversario (nos double-attackamos, eles nao
// double-defendem, mas eles ainda atacam). Reflecte "controlo do
// espaco". Valor pequeno.
const RESTRICTED_SQUARES: (i32, i32) = (2, 3);

// === Pawn structure ===
// Todas as tabelas indexadas por RANK RELATIVO (rank 0 = nossa 1a
// fileira; rank 7 = 8a fileira / promocao). Slots 0/7 sao 0 porque
// peoes nao existem la.
//
// PAWN_PHALANX: peao adjacente na mesma fileira -- estrutura forte,
// especialmente perto de promocao (peoes avancados juntos suportam
// promocao). Cresce quase quadraticamente com rank.
const PAWN_PHALANX: [(i32, i32); 8] = [
    (0, 0), (5, 0), (10, 5), (18, 12), (35, 30), (65, 100), (110, 175), (0, 0),
];
// DEFENDED_PAWN: peao suportado por outro peao proprio -- estabilidade.
// Menos importante que phalanx (que promove junto), mas relevante.
const DEFENDED_PAWN: [(i32, i32); 8] = [
    (0, 0), (0, 0), (12, 10), (10, 12), (18, 22), (35, 55), (70, 110), (0, 0),
];
// ISOLATED_PAWN: sem peoes adjacentes -- fraqueza estrutural (nao pode
// ser defendido por peao). Pior no eg (fica exposto sem pecas para
// tapar).
const ISOLATED_PAWN: (i32, i32) = (-10, -12);
// DOUBLED_PAWN: por peao excedente na mesma coluna. Mg moderado (bloqueia
// avanco proprio); eg severo (torre atras nao chega a promover).
const DOUBLED_PAWN: (i32, i32) = (-10, -25);
// PASSED_PAWN: nenhum peao inimigo no caminho para a promocao.
// Cresce fortemente com rank (mais perto da promocao). Baixo mg (mais
// pecas no meio da tabuleiro), alto eg (perto do fim, peao passado
// muitas vezes ganha).
const PASSED_PAWN: [(i32, i32); 8] = [
    (0, 0), (0, 0), (0, 0), (-10, 5), (5, 40), (35, 110), (100, 200), (0, 0),
];
// BACKWARD_PAWN: no pawn on an adjacent file can ever support it (none
// sit level with or behind it) AND its advance square is controlled by
// an enemy pawn -- stuck, can't safely push, can't be defended by a
// pawn. Mild penalty (structural, not material): Ethereal/Sirius list
// it but it's a smaller effect than isolation.
const BACKWARD_PAWN: (i32, i32) = (-6, -10);
// CANDIDATE_PASSED_PAWN: not passed yet (an enemy pawn still contests
// its file or a neighboring file ahead), but the local pawn count says
// it wins the race after likely trades -- own supporters on adjacent
// files at-or-behind >= enemy blockers on adjacent files ahead. Real
// but smaller than an actual passed pawn's bonus (see PASSED_PAWN).
const CANDIDATE_PASSED_PAWN: (i32, i32) = (6, 18);
// BAD_BISHOP: per own pawn sitting on the bishop's own square color --
// each one is a square that piece can never influence and often has
// to be defended twice. Small per-pawn penalty, worse in the endgame
// (fewer other pieces to compensate for the bad bishop's blind squares).
const BAD_BISHOP: (i32, i32) = (-2, -4);

/// Runtime-adjustable copy of every constant `positional_terms()` uses
/// (mobility/king-safety/threats/pawn-structure -- NOT material/PST,
/// those stay compile-time consts read via the incremental board
/// accumulators in board.rs, a performance-critical path this struct
/// deliberately doesn't touch). `Default` just copies the existing
/// consts field-by-field -- never retyped by hand -- so building this
/// struct cannot introduce a transcription error: `default_weights()`
/// is byte-for-byte what `positional_terms()` already computed before
/// this struct existed. This is the prerequisite for real Texel Tuning
/// (see src/tuning.rs): the tuner builds its own `Weights`, nudges
/// fields, and calls `positional_terms(board, &candidate)` to score
/// datasets -- the live search keeps using `default_weights()`
/// unchanged until a tuning run's result is deliberately copied back
/// into the consts above.
#[derive(Clone)]
pub struct Weights {
    pub bishop_pair: (i32, i32),
    pub long_diag_bishop: (i32, i32),
    pub minor_behind_pawn: (i32, i32),
    pub knight_outpost: (i32, i32),
    pub rook_open: [(i32, i32); 2],
    pub tempo: (i32, i32),
    pub mobility_knight: [(i32, i32); 28],
    pub mobility_bishop: [(i32, i32); 28],
    pub mobility_rook: [(i32, i32); 28],
    pub mobility_queen: [(i32, i32); 28],
    pub king_attacker_weight: [(i32, i32); 4],
    pub king_attacks: (i32, i32),
    pub king_danger_table: [i32; 128],
    pub pawn_shelter: [(i32, i32); 4],
    pub shelter_open: (i32, i32),
    pub pawn_storm: [(i32, i32); 4],
    pub threat_by_pawn: [[(i32, i32); 6]; 2],
    pub threat_by_knight: [[(i32, i32); 6]; 2],
    pub threat_by_bishop: [[(i32, i32); 6]; 2],
    pub threat_by_rook: [[(i32, i32); 6]; 2],
    pub threat_by_queen: [[(i32, i32); 6]; 2],
    pub threat_by_king: [(i32, i32); 6],
    pub knight_hit_queen: (i32, i32),
    pub bishop_hit_queen: (i32, i32),
    pub rook_hit_queen: (i32, i32),
    pub push_threat: (i32, i32),
    pub restricted_squares: (i32, i32),
    pub pawn_phalanx: [(i32, i32); 8],
    pub defended_pawn: [(i32, i32); 8],
    pub isolated_pawn: (i32, i32),
    pub doubled_pawn: (i32, i32),
    pub passed_pawn: [(i32, i32); 8],
    pub backward_pawn: (i32, i32),
    pub candidate_passed_pawn: (i32, i32),
    pub bad_bishop: (i32, i32),
}

impl Default for Weights {
    fn default() -> Self {
        Weights {
            bishop_pair: BISHOP_PAIR,
            long_diag_bishop: LONG_DIAG_BISHOP,
            minor_behind_pawn: MINOR_BEHIND_PAWN,
            knight_outpost: KNIGHT_OUTPOST,
            rook_open: ROOK_OPEN,
            tempo: TEMPO,
            mobility_knight: MOBILITY_KNIGHT,
            mobility_bishop: MOBILITY_BISHOP,
            mobility_rook: MOBILITY_ROOK,
            mobility_queen: MOBILITY_QUEEN,
            king_attacker_weight: KING_ATTACKER_WEIGHT,
            king_attacks: KING_ATTACKS,
            king_danger_table: KING_DANGER_TABLE,
            pawn_shelter: PAWN_SHELTER,
            shelter_open: SHELTER_OPEN,
            pawn_storm: PAWN_STORM,
            threat_by_pawn: THREAT_BY_PAWN,
            threat_by_knight: THREAT_BY_KNIGHT,
            threat_by_bishop: THREAT_BY_BISHOP,
            threat_by_rook: THREAT_BY_ROOK,
            threat_by_queen: THREAT_BY_QUEEN,
            threat_by_king: THREAT_BY_KING,
            knight_hit_queen: KNIGHT_HIT_QUEEN,
            bishop_hit_queen: BISHOP_HIT_QUEEN,
            rook_hit_queen: ROOK_HIT_QUEEN,
            push_threat: PUSH_THREAT,
            restricted_squares: RESTRICTED_SQUARES,
            pawn_phalanx: PAWN_PHALANX,
            defended_pawn: DEFENDED_PAWN,
            isolated_pawn: ISOLATED_PAWN,
            doubled_pawn: DOUBLED_PAWN,
            passed_pawn: PASSED_PAWN,
            backward_pawn: BACKWARD_PAWN,
            candidate_passed_pawn: CANDIDATE_PASSED_PAWN,
            bad_bishop: BAD_BISHOP,
        }
    }
}

static DEFAULT_WEIGHTS: OnceLock<Weights> = OnceLock::new();
/// A/B testing hook for a tuning run's output, same reversible pattern
/// as `KESTREL_EVAL_MODE` above: with the env var unset (every
/// deployment, including the live bot, unless someone deliberately
/// sets it) this is byte-for-byte `Weights::default()`. Lets a
/// candidate weight set from `kestrel tune` be exercised by the real
/// position suite / a real game before ever touching the compiled-in
/// consts -- nothing is "deployed" by running the tuner, only by a
/// deliberate later commit copying values back into the consts.
pub fn default_weights() -> &'static Weights {
    DEFAULT_WEIGHTS.get_or_init(|| {
        if let Ok(path) = std::env::var("KESTREL_TUNED_WEIGHTS") {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let parsed: Vec<i32> = text.trim().split(',').filter_map(|s| s.parse().ok()).collect();
                let base = Weights::default();
                if parsed.len() == base.to_vec().len() {
                    eprintln!("KESTREL_TUNED_WEIGHTS: loaded {} scalars from {}", parsed.len(), path);
                    return base.from_vec(&parsed);
                } else {
                    eprintln!("KESTREL_TUNED_WEIGHTS: length mismatch ({} vs expected {}), ignoring", parsed.len(), base.to_vec().len());
                }
            }
        }
        Weights::default()
    })
}

impl Weights {
    /// Flattens every tunable scalar into one Vec<i32>, fixed order,
    /// matching `apply_vec()` exactly -- lets the tuner (src/tuning.rs)
    /// treat this as one flat parameter vector for coordinate descent
    /// instead of hand-writing a perturbation loop per field.
    /// `king_danger_table` is deliberately excluded: it's a derived
    /// smooth curve (see its own comment above), not something to let
    /// 128 independent coordinate-descent steps chew on with a small
    /// dataset -- tune the pieces that feed it instead.
    pub fn to_vec(&self) -> Vec<i32> {
        let mut v = Vec::with_capacity(512);
        macro_rules! pair { ($p:expr) => { v.push($p.0); v.push($p.1); } }
        macro_rules! pairs { ($arr:expr) => { for p in $arr.iter() { pair!(p); } } }
        pair!(self.bishop_pair);
        pair!(self.long_diag_bishop);
        pair!(self.minor_behind_pawn);
        pair!(self.knight_outpost);
        pairs!(self.rook_open);
        pair!(self.tempo);
        pairs!(self.mobility_knight);
        pairs!(self.mobility_bishop);
        pairs!(self.mobility_rook);
        pairs!(self.mobility_queen);
        pairs!(self.king_attacker_weight);
        pair!(self.king_attacks);
        pairs!(self.pawn_shelter);
        pair!(self.shelter_open);
        pairs!(self.pawn_storm);
        for row in self.threat_by_pawn.iter() { pairs!(row); }
        for row in self.threat_by_knight.iter() { pairs!(row); }
        for row in self.threat_by_bishop.iter() { pairs!(row); }
        for row in self.threat_by_rook.iter() { pairs!(row); }
        for row in self.threat_by_queen.iter() { pairs!(row); }
        pairs!(self.threat_by_king);
        pair!(self.knight_hit_queen);
        pair!(self.bishop_hit_queen);
        pair!(self.rook_hit_queen);
        pair!(self.push_threat);
        pair!(self.restricted_squares);
        pairs!(self.pawn_phalanx);
        pairs!(self.defended_pawn);
        pair!(self.isolated_pawn);
        pair!(self.doubled_pawn);
        pairs!(self.passed_pawn);
        pair!(self.backward_pawn);
        pair!(self.candidate_passed_pawn);
        pair!(self.bad_bishop);
        v
    }

    /// Inverse of `to_vec()` -- rebuilds a full `Weights` from a flat
    /// vector in the exact same field order. `king_danger_table` is
    /// copied from `self` unchanged (see `to_vec` doc).
    pub fn from_vec(&self, v: &[i32]) -> Weights {
        let mut i = 0;
        macro_rules! next { () => { { let x = v[i]; i += 1; x } } }
        macro_rules! pair { () => { (next!(), next!()) } }
        macro_rules! pairs { ($n:expr) => { { let mut a = [(0i32,0i32); $n]; for j in 0..$n { a[j] = pair!(); } a } } }
        let bishop_pair = pair!();
        let long_diag_bishop = pair!();
        let minor_behind_pawn = pair!();
        let knight_outpost = pair!();
        let rook_open = pairs!(2);
        let tempo = pair!();
        let mobility_knight = pairs!(28);
        let mobility_bishop = pairs!(28);
        let mobility_rook = pairs!(28);
        let mobility_queen = pairs!(28);
        let king_attacker_weight = pairs!(4);
        let king_attacks = pair!();
        let pawn_shelter = pairs!(4);
        let shelter_open = pair!();
        let pawn_storm = pairs!(4);
        let threat_by_pawn = [pairs!(6), pairs!(6)];
        let threat_by_knight = [pairs!(6), pairs!(6)];
        let threat_by_bishop = [pairs!(6), pairs!(6)];
        let threat_by_rook = [pairs!(6), pairs!(6)];
        let threat_by_queen = [pairs!(6), pairs!(6)];
        let threat_by_king = pairs!(6);
        let knight_hit_queen = pair!();
        let bishop_hit_queen = pair!();
        let rook_hit_queen = pair!();
        let push_threat = pair!();
        let restricted_squares = pair!();
        let pawn_phalanx = pairs!(8);
        let defended_pawn = pairs!(8);
        let isolated_pawn = pair!();
        let doubled_pawn = pair!();
        let passed_pawn = pairs!(8);
        let backward_pawn = pair!();
        let candidate_passed_pawn = pair!();
        let bad_bishop = pair!();
        assert_eq!(i, v.len(), "from_vec: length mismatch with to_vec's field order");
        Weights {
            bishop_pair, long_diag_bishop, minor_behind_pawn, knight_outpost, rook_open, tempo,
            mobility_knight, mobility_bishop, mobility_rook, mobility_queen,
            king_attacker_weight, king_attacks, king_danger_table: self.king_danger_table,
            pawn_shelter, shelter_open, pawn_storm,
            threat_by_pawn, threat_by_knight, threat_by_bishop, threat_by_rook, threat_by_queen, threat_by_king,
            knight_hit_queen, bishop_hit_queen, rook_hit_queen, push_threat, restricted_squares,
            pawn_phalanx, defended_pawn, isolated_pawn, doubled_pawn, passed_pawn,
            backward_pawn, candidate_passed_pawn, bad_bishop,
        }
    }
}

/// Full evaluate(), but with a caller-supplied `Weights` instead of
/// `default_weights()` -- what the tuner calls to score a position
/// under a candidate parameter vector. Mirrors `evaluate()`'s material
/// + positional composition exactly.
pub fn evaluate_with_weights(board: &Board, w: &Weights) -> i32 {
    let p = positional_terms(board, w);
    let p_signed = if board.side == Color::White { p } else { -p };
    material_pst(board) + p_signed
}

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

pub fn positional_terms(board: &Board, w: &Weights) -> i32 {
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

    // Indexed by the ATTACKING color (not the color whose king is in
    // danger) -- king_attack_units[White] is how much White's pieces
    // threaten Black's king, contributed with White's (+1) sign below,
    // same convention the code already used locally before this was
    // hoisted out of the loop.
    let mut king_attackers = [0i32; 2];
    let mut king_attack_units = [(0i32, 0i32); 2];

    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let own = board.occ_color[c.idx()];
        let enemy_king_zone = if c == Color::White { black_king_zone } else { white_king_zone };
        let ci = c.idx();

        if count(board.pieces[c.idx()][PieceType::Bishop.idx()]) >= 2 {
            mg += sign * w.bishop_pair.0;
            eg += sign * w.bishop_pair.1;
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

                // Mobility area excludes squares attacked by enemy
                // pawns (moving there just hangs the piece for a pawn,
                // not real mobility) as well as own-occupied squares.
                // Standard refinement (Stockfish "mobility area").
                let enemy_pawn_attacks = attacked_by_pt[c.opp().idx()][PieceType::Pawn.idx()];
                let mobility = count(attacks & !own & !enemy_pawn_attacks) as usize;
                let mob_table = match pt {
                    PieceType::Knight => &w.mobility_knight,
                    PieceType::Bishop => &w.mobility_bishop,
                    PieceType::Rook => &w.mobility_rook,
                    PieceType::Queen => &w.mobility_queen,
                    _ => &w.mobility_knight,
                };
                let m = mob_table[mobility.min(27)];
                mg += sign * m.0;
                eg += sign * m.1;

                let hits = count(attacks & enemy_king_zone) as i32;
                if hits > 0 {
                    king_attackers[ci] += 1;
                    let widx = match pt {
                        PieceType::Knight => 0,
                        PieceType::Bishop => 1,
                        PieceType::Rook => 2,
                        PieceType::Queen => 3,
                        _ => 0,
                    };
                    let aw = w.king_attacker_weight[widx];
                    king_attack_units[ci].0 += aw.0 + hits * w.king_attacks.0;
                    king_attack_units[ci].1 += aw.1 + hits * w.king_attacks.1;
                }

                if pt == PieceType::Rook {
                    let file_mask = FILE_A << file_of(s);
                    let own_pawns_on_file = board.pieces[c.idx()][PieceType::Pawn.idx()] & file_mask;
                    let enemy_pawns_on_file = board.pieces[c.opp().idx()][PieceType::Pawn.idx()] & file_mask;
                    if own_pawns_on_file == 0 {
                        let idx = if enemy_pawns_on_file == 0 { 0 } else { 1 };
                        mg += sign * w.rook_open[idx].0;
                        eg += sign * w.rook_open[idx].1;
                    }
                }

                if pt == PieceType::Knight || pt == PieceType::Bishop {
                    let f = file_of(s) as i32;
                    let r = rank_of(s) as i32;
                    let front_r = if c == Color::White { r + 1 } else { r - 1 };
                    if (0..8).contains(&front_r)
                        && board.pieces[c.idx()][PieceType::Pawn.idx()] & bb(sq(f as u8, front_r as u8)) != 0
                    {
                        mg += sign * w.minor_behind_pawn.0;
                        eg += sign * w.minor_behind_pawn.1;
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
                                mg += sign * w.knight_outpost.0;
                                eg += sign * w.knight_outpost.1;
                            }
                        }
                    }
                }

                if pt == PieceType::Bishop {
                    let center: Bitboard = bb(sq(3, 3)) | bb(sq(4, 3)) | bb(sq(3, 4)) | bb(sq(4, 4));
                    if count(attacks & center) >= 2 {
                        mg += sign * w.long_diag_bishop.0;
                        eg += sign * w.long_diag_bishop.1;
                    }

                    // Bispo mau: penalidade por cada peao proprio na
                    // mesma cor de casa que o bispo -- casas que essa
                    // peca nunca pode influenciar, muitas vezes tem de
                    // ser defendidas por outra peca em vez dele.
                    let bishop_light = (rank_of(s) + file_of(s)) % 2 == 1;
                    let own_pawns_same_color = if bishop_light { LIGHT_SQUARES } else { !LIGHT_SQUARES };
                    let n = count(board.pieces[c.idx()][PieceType::Pawn.idx()] & own_pawns_same_color) as i32;
                    mg += sign * w.bad_bishop.0 * n;
                    eg += sign * w.bad_bishop.1 * n;
                }
            }
        }

        // === Pawn shelter / storm around own king ===
        // Missing piece found after a real bullet loss (2026-07-21,
        // GLUlNq1Q): White played g4/g5 in front of its own castled
        // king with no concrete follow-up, and lost material to ...Bxh3
        // a few moves later. At bullet depth the search never calculated
        // that far -- what should have stopped the push is the STATIC
        // eval already pricing in "my own king shield just moved 2
        // squares forward", the same way a human's intuition flags a
        // self-weakening pawn storm before calculating anything concrete.
        // This is a universal HCE component (Stockfish's ShelterStrength/
        // UnblockedStorm, Ethereal/Berserk equivalents all encode some
        // version of it); values below are mine, reasoned from scratch:
        // an intact shield pawn one square ahead of the king costs
        // nothing, each extra square it advances trades king safety for
        // nothing in return, and an enemy pawn closing in on the king's
        // file is progressively more dangerous the closer it gets.
        let kf = file_of(board.king_sq(c)) as i32;
        let kr = rank_of(board.king_sq(c)) as i32;
        let white = c == Color::White;
        for f in (kf - 1).max(0)..=(kf + 1).min(7) {
            let file_mask = FILE_A << f;
            let own_pawns = board.pieces[c.idx()][PieceType::Pawn.idx()] & file_mask;
            let enemy_pawns = board.pieces[c.opp().idx()][PieceType::Pawn.idx()] & file_mask;
            match shield_pawn_offset(own_pawns, kr, white) {
                None => {
                    mg += sign * w.shelter_open.0;
                    eg += sign * w.shelter_open.1;
                }
                Some(off) => {
                    let idx = (off - 1).clamp(0, 3) as usize;
                    mg += sign * w.pawn_shelter[idx].0;
                    eg += sign * w.pawn_shelter[idx].1;
                }
            }
            if let Some(off) = shield_pawn_offset(enemy_pawns, kr, white) {
                let idx = (off - 1).clamp(0, 3) as usize;
                mg += sign * w.pawn_storm[idx].0;
                eg += sign * w.pawn_storm[idx].1;
            }
        }
    }

    // === Safe checks + queen-gated king danger (Ethereal's approach,
    // architecture ported not values) ===
    // Deferred to its own pass after both colors' attacked[]/
    // attacked_by_pt[] are fully known -- a "safe" square (no enemy
    // defender at all, conservative but simple) can only be judged once
    // the DEFENDING side's full attack set exists, which isn't true yet
    // mid-loop above when processing the attacking side first.
    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let us = c.idx();
        let them = c.opp().idx();
        let enemy_king_sq = board.king_sq(c.opp());
        let own_occ = board.occ_color[us];
        // Conservative "safe": zero enemy defenders on the square at
        // all (not even the king). Undercounts some genuinely-safe
        // checks where we'd have enough backup to win the exchange,
        // but is cheap and never wrongly counts an unsafe one.
        let safe = !attacked[them];

        let knight_checks = a.knight[enemy_king_sq as usize];
        let bishop_checks = bishop_attacks(enemy_king_sq, occ);
        let rook_checks = rook_attacks(enemy_king_sq, occ);
        let queen_checks = bishop_checks | rook_checks;

        let n_knight = count(knight_checks & attacked_by_pt[us][PieceType::Knight.idx()] & !own_occ & safe) as i32;
        let n_bishop = count(bishop_checks & attacked_by_pt[us][PieceType::Bishop.idx()] & !own_occ & safe) as i32;
        let n_rook = count(rook_checks & attacked_by_pt[us][PieceType::Rook.idx()] & !own_occ & safe) as i32;
        let n_queen = count(queen_checks & attacked_by_pt[us][PieceType::Queen.idx()] & !own_occ & safe) as i32;

        // Reuses the existing per-hit `king_attacks` weight as the unit
        // value for a free check (queen checks weighted double -- by
        // far the most dangerous piece to let deliver one for free)
        // instead of adding new tunable fields: keeps this additive to
        // the Weights struct and to tune_fast's king-field sentinel
        // detection in main.rs, which already special-cases every
        // OTHER king-safety field as "nonlinear, not tuned here".
        let safe_check_units = n_knight + n_bishop + n_rook + 2 * n_queen;
        if safe_check_units > 0 {
            king_attackers[us] += 1;
            king_attack_units[us].0 += safe_check_units * w.king_attacks.0;
            king_attack_units[us].1 += safe_check_units * w.king_attacks.1;
        }

        // Queen-gate: with the defending side's queen off the board, a
        // single attacker rarely turns into a real mating attack --
        // require at least 2. With her still on board, one already
        // matters (Ethereal: `kingAttackersCount > 1 - popcount(queens)`).
        let defender_has_queen = board.pieces[them][PieceType::Queen.idx()] != 0;
        let threshold = if defender_has_queen { 1 } else { 2 };
        if king_attackers[us] >= threshold {
            let danger_idx = king_attack_units[us].0.clamp(0, 127) as usize;
            mg += sign * w.king_danger_table[danger_idx];
            eg += sign * king_attack_units[us].1;
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

        // Threats por peao -- agora indexed por defended tambem.
        let mut t = attacked_by_pt[us][PieceType::Pawn.idx()] & their_pieces;
        while t != 0 {
            let s = t.trailing_zeros() as Square;
            t &= t - 1;
            let defended = (defended_bb & bb(s)) != 0;
            if let Some((pt, _)) = board.piece_at(s) {
                let entry = w.threat_by_pawn[defended as usize][pt.idx()];
                mg += sign * entry.0;
                eg += sign * entry.1;
            }
        }
        // Threats por cavalo/bispo/torre/dama.
        for (pt_us, table) in [
            (PieceType::Knight, &w.threat_by_knight),
            (PieceType::Bishop, &w.threat_by_bishop),
            (PieceType::Rook, &w.threat_by_rook),
            (PieceType::Queen, &w.threat_by_queen),
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
                mg += sign * w.threat_by_king[pt.idx()].0;
                eg += sign * w.threat_by_king[pt.idx()].1;
            }
        }

        // Restricted squares: casas onde nos temos 2+ atacantes, eles
        // nao tem 2+, mas eles atacam pelo menos 1 vez. Sirius:
        // attackedBy2[us] & ~attackedBy2[them] & attacked[them].
        let restricted = attacked_by_2[us] & !attacked_by_2[them] & attacked[them];
        let n_restr = count(restricted) as i32;
        mg += sign * w.restricted_squares.0 * n_restr;
        eg += sign * w.restricted_squares.1 * n_restr;

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
        mg += sign * w.push_threat.0 * n_push_threats;
        eg += sign * w.push_threat.1 * n_push_threats;

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
            mg += sign * w.knight_hit_queen.0 * n_knight_hit;
            eg += sign * w.knight_hit_queen.1 * n_knight_hit;
            let targets_double = targets_base & attacked_by_2[us];
            let n_bishop_hit = count(targets_double & bishop_hits & attacked_by_pt[us][PieceType::Bishop.idx()]) as i32;
            mg += sign * w.bishop_hit_queen.0 * n_bishop_hit;
            eg += sign * w.bishop_hit_queen.1 * n_bishop_hit;
            let n_rook_hit = count(targets_double & rook_hits & attacked_by_pt[us][PieceType::Rook.idx()]) as i32;
            mg += sign * w.rook_hit_queen.0 * n_rook_hit;
            eg += sign * w.rook_hit_queen.1 * n_rook_hit;
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
                mg += sign * w.passed_pawn[rel_rank].0;
                eg += sign * w.passed_pawn[rel_rank].1;
            } else {
                // Peao atrasado: nenhum peao proprio numa coluna adjacente
                // ao mesmo nivel ou atras pode alguma vez apoiar o avanco
                // deste peao, E a casa de avanco esta controlada por peao
                // inimigo -- preso, nao avanca em seguranca nem e' defendido.
                let front_r = if c == Color::White { r + 1 } else { r - 1 };
                let mut supported_ever = false;
                for adj in [f - 1, f + 1] {
                    if !(0..8).contains(&adj) { continue; }
                    let mut m: Bitboard = 0;
                    if c == Color::White {
                        for rr in 0..=r { m |= bb(sq(adj as u8, rr as u8)); }
                    } else {
                        for rr in r..8 { m |= bb(sq(adj as u8, rr as u8)); }
                    }
                    if own_pawns & m != 0 { supported_ever = true; break; }
                }
                if !supported_ever && (0..8).contains(&front_r) {
                    let front_sq = sq(f as u8, front_r as u8);
                    if a.pawn[c.idx()][front_sq as usize] & enemy_pawns != 0 {
                        mg += sign * w.backward_pawn.0;
                        eg += sign * w.backward_pawn.1;
                    }
                }

                // Peao passado candidato: nenhum peao inimigo na MESMA
                // coluna a frente (essa parte da corrida ja' esta' livre),
                // e nas colunas adjacentes a frente o numero de bloqueadores
                // inimigos nao excede o numero de apoiadores proprios ao
                // mesmo nivel ou atras -- depois de uma troca razoavel,
                // este peao fica realmente passado.
                if enemy_pawns & (FILE_A << f) == 0 {
                    let mut enemy_ahead = 0u32;
                    let mut own_support = 0u32;
                    for adj in [f - 1, f + 1] {
                        if !(0..8).contains(&adj) { continue; }
                        let mut ahead: Bitboard = 0;
                        let mut behind: Bitboard = 0;
                        if c == Color::White {
                            for rr in (r + 1)..8 { ahead |= bb(sq(adj as u8, rr as u8)); }
                            for rr in 0..=r { behind |= bb(sq(adj as u8, rr as u8)); }
                        } else {
                            for rr in 0..r { ahead |= bb(sq(adj as u8, rr as u8)); }
                            for rr in r..8 { behind |= bb(sq(adj as u8, rr as u8)); }
                        }
                        enemy_ahead += count(enemy_pawns & ahead);
                        own_support += count(own_pawns & behind);
                    }
                    if enemy_ahead >= 1 && enemy_ahead <= own_support {
                        mg += sign * w.candidate_passed_pawn.0;
                        eg += sign * w.candidate_passed_pawn.1;
                    }
                }
            }

            // Peao isolado.
            let mut has_neighbor = false;
            for adj in (f - 1)..=(f + 1) {
                if adj == f || !(0..8).contains(&adj) { continue; }
                if own_pawns & (FILE_A << adj) != 0 { has_neighbor = true; break; }
            }
            if !has_neighbor {
                mg += sign * w.isolated_pawn.0;
                eg += sign * w.isolated_pawn.1;
            }

            // Peao defendido por outro peao proprio (usa mesmo truque
            // reversed pawn-attack table do SEE em search.rs).
            if a.pawn[c.opp().idx()][s as usize] & own_pawns != 0 {
                mg += sign * w.defended_pawn[rel_rank].0;
                eg += sign * w.defended_pawn[rel_rank].1;
            }

            // Falange (outro peao proprio na mesma fileira, coluna
            // adjacente).
            for adj in [f - 1, f + 1] {
                if (0..8).contains(&adj) && own_pawns & bb(sq(adj as u8, r as u8)) != 0 {
                    mg += sign * w.pawn_phalanx[rel_rank].0;
                    eg += sign * w.pawn_phalanx[rel_rank].1;
                    break;
                }
            }
        }

        // Peoes dobrados (por peao excedente na mesma coluna).
        for file in 0..8 {
            let n = count(own_pawns & (FILE_A << file)) as i32;
            if n > 1 {
                mg += sign * w.doubled_pawn.0 * (n - 1);
                eg += sign * w.doubled_pawn.1 * (n - 1);
            }
        }
    }

    // Tempo -- bonus para quem tem a jogar. Aplicado como (mg,eg) do
    // ponto de vista das brancas: se e' a vez das brancas, +w.tempo; se
    // e' a vez das pretas, -w.tempo. Sirius aplica assim mesmo.
    let tempo_sign = if board.side == Color::White { 1 } else { -1 };
    mg += tempo_sign * w.tempo.0;
    eg += tempo_sign * w.tempo.1;

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
    let raw = if eval_mode_material_only() {
        material_pst(board)
    } else {
        material_pst(board) + positional_terms_signed(board)
    };
    scale_endgame(board, raw)
}

/// Endgame scale factor (Ethereal's approach, ported architecture not
/// values -- own thresholds below): known drawish/hard-to-convert
/// material patterns get their eval shrunk toward zero, in proportion
/// to how "scaled down" that material pattern actually plays in
/// practice. Applied to the WHOLE already-tapered eval rather than
/// splitting mg/eg and rescaling only eg separately -- by the time any
/// of these patterns fire, so little material is left that `phase` is
/// already deep in the endgame anyway, so the approximation costs
/// little accuracy for a lot less invasive a change (keeps
/// `positional_terms()` exactly linear in its weights, which
/// `tune_fast` in main.rs relies on -- see the comment there).
/// Symmetric (doesn't care whose turn it is), so it's safe to apply
/// after `material_pst`/`positional_terms_signed` have already flipped
/// sign for side-to-move.
const SCALE_NORMAL: i32 = 128;

fn scale_endgame(board: &Board, raw: i32) -> i32 {
    if raw == 0 {
        return 0;
    }
    let scale = endgame_scale_factor(board, raw);
    if scale == SCALE_NORMAL {
        return raw;
    }
    raw * scale / SCALE_NORMAL
}

fn endgame_scale_factor(board: &Board, raw: i32) -> i32 {
    let w = Color::White.idx();
    let b = Color::Black.idx();
    let wp = board.pieces[w][PieceType::Pawn.idx()];
    let bp = board.pieces[b][PieceType::Pawn.idx()];
    let wn = board.pieces[w][PieceType::Knight.idx()];
    let bn = board.pieces[b][PieceType::Knight.idx()];
    let wb = board.pieces[w][PieceType::Bishop.idx()];
    let bb_ = board.pieces[b][PieceType::Bishop.idx()];
    let wr = board.pieces[w][PieceType::Rook.idx()];
    let br = board.pieces[b][PieceType::Rook.idx()];
    let wq = board.pieces[w][PieceType::Queen.idx()];
    let bq = board.pieces[b][PieceType::Queen.idx()];

    let n_wp = count(wp) as i32;
    let n_bp = count(bp) as i32;
    let n_wn = count(wn);
    let n_bn = count(bn);
    let n_wb = count(wb);
    let n_bb = count(bb_);
    let n_wr = count(wr);
    let n_br = count(br);
    let n_wq = count(wq);
    let n_bq = count(bq);

    // Opposite-colored bishops: exactly one bishop each, on different
    // square colors. Classic drawing fortress even a pawn or two up.
    // Scales down further, the fewer other pieces are left to help
    // convert (bishops-only < one-knight-each < one-rook-each).
    if n_wb == 1 && n_bb == 1 {
        let wb_sq = wb.trailing_zeros();
        let bb_sq = bb_.trailing_zeros();
        let wb_light = (rank_of(wb_sq as Square) + file_of(wb_sq as Square)) % 2 == 1;
        let bb_light = (rank_of(bb_sq as Square) + file_of(bb_sq as Square)) % 2 == 1;
        if wb_light != bb_light {
            if n_wn == 0 && n_bn == 0 && n_wr == 0 && n_br == 0 && n_wq == 0 && n_bq == 0 {
                return 64;
            }
            if n_wr == 1 && n_br == 1 && n_wn == 0 && n_bn == 0 && n_wq == 0 && n_bq == 0 {
                return 96;
            }
            if n_wn == 1 && n_bn == 1 && n_wr == 0 && n_br == 0 && n_wq == 0 && n_bq == 0 {
                return 106;
            }
        }
    }

    // A single minor piece (knight or bishop), nothing else but pawns,
    // for the side with more total material -- can't force a win
    // against a lone king even with extra pawns, only a fortress/
    // blockade at best. True draw scale.
    let w_minors_only = n_wr == 0 && n_wq == 0 && (n_wn + n_wb) <= 1;
    let b_minors_only = n_br == 0 && n_bq == 0 && (n_bn + n_bb) <= 1;
    if w_minors_only && n_br == 0 && n_bq == 0 && n_bn == 0 && n_bb == 0 && n_bp == 0 {
        return 0;
    }
    if b_minors_only && n_wr == 0 && n_wq == 0 && n_wn == 0 && n_wb == 0 && n_wp == 0 {
        return 0;
    }

    // Fallback: scale down with how few pawns the stronger side has
    // left -- fewer pawns left to shelter a passer/create a second
    // weakness makes converting a material edge progressively harder.
    // Gated to queenless positions only: this function scales the
    // WHOLE already-tapered eval (not just the eg component the way
    // Ethereal's mg/eg-split version does), so applying it unconditionally
    // would also shrink ordinary middlegame evals whenever pawn counts
    // differ -- wrong, since in the midgame this pattern says nothing
    // about convertibility. No queens is a cheap, real proxy for "this
    // is actually an endgame" that keeps the approximation safe.
    if n_wq == 0 && n_bq == 0 {
        let strong_pawns = if raw > 0 { n_wp } else { n_bp };
        return (96 + 8 * strong_pawns).min(SCALE_NORMAL);
    }
    SCALE_NORMAL
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
    if board.side == Color::White {
        material_pst_white(board)
    } else {
        -material_pst_white(board)
    }
}

/// Same as `material_pst()` but always from White's perspective
/// (no side-to-move flip) -- what the fast linear-feature tuner
/// (src/main.rs `tune_fast`) needs, since it builds its per-position
/// bias directly in White's POV to match `positional_terms()`'s own
/// convention, rather than negamax's STM-relative one.
pub fn material_pst_white(board: &Board) -> i32 {
    let phase = board.phase.min(MAX_PHASE);
    (board.mg_score * phase + board.eg_score * (MAX_PHASE - phase)) / MAX_PHASE
}

fn positional_terms_signed(board: &Board) -> i32 {
    let p = positional_terms(board, default_weights());
    if board.side == Color::White {
        p
    } else {
        -p
    }
}
