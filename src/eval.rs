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

/// Material base -- pontos de partida sensatos, para afinar via Texel
/// Tuning proprio a seguir. Peao=100 (valor classico "1 pawn = 100cp")
/// que da margem clara em posicoes de +1 peao, coisa que a busca em
/// bullet precisa para preferir uma captura de peao a um lance
/// posicional aproximadamente equivalente. Distintos de
/// `PieceType::value()`, que continua a servir SEE/MVV-LVA.
const MG_VALUE: [i32; 6] = [100, 340, 360, 520, 1000, 0];
const EG_VALUE: [i32; 6] = [110, 300, 310, 540, 950, 0];

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

const BISHOP_PAIR: (i32, i32) = (25, 55);
const LONG_DIAG_BISHOP: (i32, i32) = (10, 10);
const MINOR_BEHIND_PAWN: (i32, i32) = (5, 5);
const KNIGHT_OUTPOST: (i32, i32) = (20, 15);
const ROOK_OPEN: [(i32, i32); 2] = [(25, 5), (15, 0)]; // [0]=aberta, [1]=semi-aberta
const TEMPO: (i32, i32) = (20, 15);

// Mobility por peca -- crescente com o numero de casas seguras
// atacadas. Base intuitiva: sem lances = muito mau; muitos lances = ok.
// Valores propios, monotonos. Sirius/SF afinam versoes muito mais
// nuancadas via Texel; deixamos o afinamento para depois.
const MOBILITY_KNIGHT: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    let base = [-30, -15, -5, 0, 5, 10, 15, 20, 25];
    let mut i = 0; while i < 9 { t[i] = (base[i], base[i] - 5); i += 1; }
    t
};
const MOBILITY_BISHOP: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    let base = [-30, -15, -5, 0, 4, 8, 12, 15, 18, 20, 22, 24, 25, 26];
    let mut i = 0; while i < 14 { t[i] = (base[i], base[i] - 5); i += 1; }
    t
};
const MOBILITY_ROOK: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    let base = [-40, -20, -10, -5, 0, 3, 6, 9, 12, 14, 16, 18, 20, 22, 24];
    let mut i = 0; while i < 15 { t[i] = (base[i], base[i]); i += 1; }
    t
};
const MOBILITY_QUEEN: [(i32, i32); 28] = {
    let mut t = [(0i32, 0i32); 28];
    let base = [-30, -20, -10, -5, 0, 2, 4, 6, 8, 10, 12, 13, 14, 15, 16, 17, 18, 19, 20, 20, 20, 20, 20, 20, 20, 20, 20, 20];
    let mut i = 0; while i < 28 { t[i] = (base[i], base[i]); i += 1; }
    t
};

// Peso de atacantes ao king ring por tipo -- damas mais que torres
// mais que menores, padrao classico.
const KING_ATTACKER_WEIGHT: [(i32, i32); 4] = [(15, 0), (15, 0), (30, 0), (60, 0)];
const KING_ATTACKS: (i32, i32) = (5, 0);

// Ameacas -- estrutura standard (indexed por tipo da peca inimiga
// atacada e por "defendida pelo inimigo?"). Valores crescentes com o
// valor da peca atacada: atacar dama > torre > menor > peao. Atacar
// peca defendida vale menos.
// Ordem: [Pawn, Knight, Bishop, Rook, Queen, King].
//
// THREAT_BY_PAWN e' indexed por defended para capturar o caso que faz
// diferenca em jogo real: peao inimigo pendurado (atacado por peao
// nosso E sem defensor) e' ganho de material efectivo -- vale muito.
// Se estiver defendido e' recaptura ~equal, vale pouco. Este era o
// "bug do recapturar" observado em jogo (N7671Omx): motor jogava Nc6
// deixando peao em d6 pendurado, valuation = 0 (ambos casos eram
// "atacar peao inimigo") em vez de refletir o material a ganhar.
const THREAT_BY_PAWN: [[(i32, i32); 6]; 2] = [
    // [0] = alvo NAO defendido -> bonus grande (ganho material)
    [(70, 60), (85, 60), (85, 60), (95, 55), (85, 40), (0, 0)],
    // [1] = alvo defendido -> recaptura tipica, sinal moderado
    [(0, 5), (25, 15), (25, 15), (30, 20), (25, 10), (0, 0)],
];
// [defended=0 nao-defendido / 1 defendido][target_piece]
const THREAT_BY_KNIGHT: [[(i32, i32); 6]; 2] = [
    [(5, 15), (0, 0), (30, 20), (60, 25), (40, 20), (0, 0)],   // nao defendido
    [(0, 5), (0, 0), (15, 15), (30, 20), (30, 30), (0, 0)],    // defendido
];
const THREAT_BY_BISHOP: [[(i32, i32); 6]; 2] = [
    [(5, 15), (25, 25), (0, 0), (50, 25), (40, 40), (0, 0)],
    [(0, 5), (10, 15), (0, 0), (30, 25), (40, 50), (0, 0)],
];
const THREAT_BY_ROOK: [[(i32, i32); 6]; 2] = [
    [(0, 15), (25, 30), (25, 30), (0, 0), (50, 20), (0, 0)],
    [(-5, 5), (5, 10), (10, 5), (0, 0), (40, 60), (0, 0)],
];
const THREAT_BY_QUEEN: [[(i32, i32); 6]; 2] = [
    [(5, 15), (15, 20), (15, 25), (10, 10), (0, 0), (0, 0)],
    [(0, 5), (0, 5), (-5, 15), (-5, 5), (0, 0), (0, 0)],
];
const THREAT_BY_KING: [(i32, i32); 6] = [(30, 15), (30, 25), (60, 20), (50, 5), (0, 0), (0, 0)];
// "hit queen": peca nossa a 1 movimento de atacar a dama inimiga.
const KNIGHT_HIT_QUEEN: (i32, i32) = (8, 5);
const BISHOP_HIT_QUEEN: (i32, i32) = (12, 12);
const ROOK_HIT_QUEEN: (i32, i32) = (12, 5);
// Push threat: avanco de peao para casa segura que criaria nova ameaca
// a peca inimiga nao-peao. Padrao HCE, usado em muitos motores.
const PUSH_THREAT: (i32, i32) = (10, 12);
// Casas restritas ao adversario (nos temos 2+ ataques, eles nao).
const RESTRICTED_SQUARES: (i32, i32) = (2, 3);

// Peoes -- tabelas indexadas por rank relativo (0..7; 0 e 7 nunca
// aplicam porque peoes nunca la estao).
const PAWN_PHALANX: [(i32, i32); 8] = [(0,0),(5,0),(10,5),(15,10),(30,25),(60,80),(100,150),(0,0)];
const DEFENDED_PAWN: [(i32, i32); 8] = [(0,0),(0,0),(12,10),(10,12),(18,25),(35,55),(70,110),(0,0)];
const ISOLATED_PAWN: (i32, i32) = (-8, -8);
const DOUBLED_PAWN: (i32, i32) = (-8, -20);
const PASSED_PAWN: [(i32, i32); 8] = [(0,0),(0,0),(0,0),(-20,-15),(-5,25),(30,100),(80,180),(0,0)];

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

        // Threats por peao -- agora indexed por defended tambem.
        let mut t = attacked_by_pt[us][PieceType::Pawn.idx()] & their_pieces;
        while t != 0 {
            let s = t.trailing_zeros() as Square;
            t &= t - 1;
            let defended = (defended_bb & bb(s)) != 0;
            if let Some((pt, _)) = board.piece_at(s) {
                let entry = THREAT_BY_PAWN[defended as usize][pt.idx()];
                mg += sign * entry.0;
                eg += sign * entry.1;
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
