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

/// Peso de ataque por tipo de peca contra a zona do rei inimigo -- pecas
/// maiores pesam mais (uma torre ou dama a apontar ao rei e' mais grave
/// que um cavalo). Isto e' a tradução computavel do "estilo Polgar" que
/// pediram: nao e' a visao literal da Judit Polgar, mas um vies deliberado
/// a favor de pressao ativa sobre o rei inimigo, tatica e iniciativa, em
/// vez de uma avaliacao puramente material/estatica -- o oposto de um
/// estilo tipo Petrosian/profilatico.
fn king_attack_weight(pt: PieceType) -> i32 {
    match pt {
        PieceType::Pawn => 1,
        PieceType::Knight => 3,
        PieceType::Bishop => 3,
        PieceType::Rook => 4,
        PieceType::Queen => 6,
        PieceType::King => 0,
    }
}

/// Avalia mobilidade, pressao sobre o rei inimigo, par de bispos, torres
/// em colunas abertas/semi-abertas e peoes passados -- termos que uma
/// engine puramente material+PST nao capta, e que sao exatamente o que
/// diferencia um estilo agressivo/tatico (Polgar) de um estilo estatico.
/// Devolve a pontuacao da perspetiva das BRANCAS (score_white - score_black),
/// e' o chamador (evaluate) que converte para a convencao negamax.
fn positional_terms(board: &Board) -> i32 {
    let a = atk();
    let occ = board.occ_all;
    let mut score = 0i32;

    let white_king_zone = king_zone(board.king_sq(Color::White));
    let black_king_zone = king_zone(board.king_sq(Color::Black));

    // Tabela de densidade de ataque (estilo "unidades de ataque" do
    // Stockfish classico) -- cresce SUPER-linearmente com o numero de
    // pecas distintas a atacar a zona do rei inimigo. Isto e' o que
    // traduz o lado "sacrificios out-of-the-box" da Judit Polgar: um
    // unico atacante vale pouco (defende-se facilmente), mas 3-4 pecas a
    // apontar ao rei valem muito mais do que a soma das partes -- por
    // isso a busca fica disposta a aceitar um deficit material temporario
    // se isso construir esta configuracao.
    const ATTACK_DENSITY: [i32; 8] = [0, 10, 40, 100, 190, 300, 420, 550];

    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        let own = board.occ_color[c.idx()];
        let enemy_king_zone = if c == Color::White { black_king_zone } else { white_king_zone };
        let mut attacker_count = 0usize;
        let mut attack_units = 0i32;

        // Par de bispos: recompensa manter os dois -- tipico de jogadores
        // que valorizam pecas de longo alcance para abrir o jogo.
        if count(board.pieces[c.idx()][PieceType::Bishop.idx()]) >= 2 {
            score += sign * 30;
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
                let mobility = count(attacks & !own) as i32;
                // Mobilidade pesada com vies agressivo: vale mais para
                // pecas maiores (dama/torre) do que o classico "conta
                // lances" neutro -- recompensa atividade, nao so' opcoes.
                score += sign * mobility * 2;

                // Pressao sobre a zona do rei inimigo -- o termo "Polgar":
                // cada peca nossa que ataca a zona do rei adversario soma,
                // pesada pelo tipo de peca, e conta para a densidade de
                // ataque (ver ATTACK_DENSITY acima).
                let hits = count(attacks & enemy_king_zone) as i32;
                if hits > 0 {
                    score += sign * hits * king_attack_weight(pt);
                    attacker_count += 1;
                    attack_units += hits * king_attack_weight(pt);
                }

                // Torre em coluna aberta/semi-aberta: peca de Polgar tipica
                // -- abrir linhas para as torres em vez de as deixar presas.
                if pt == PieceType::Rook {
                    let file_mask = FILE_A << file_of(s);
                    let own_pawns_on_file = board.pieces[c.idx()][PieceType::Pawn.idx()] & file_mask;
                    let enemy_pawns_on_file = board.pieces[c.opp().idx()][PieceType::Pawn.idx()] & file_mask;
                    if own_pawns_on_file == 0 {
                        score += sign * if enemy_pawns_on_file == 0 { 20 } else { 10 };
                    }
                }

                if pt == PieceType::Knight || pt == PieceType::Bishop {
                    // Menor "atras" de um peao proprio (mesma coluna, uma
                    // fileira a frente da peca no sentido do avanco) --
                    // abrigo tipico antes de reorganizar o desenvolvimento.
                    let f = file_of(s) as i32;
                    let r = rank_of(s) as i32;
                    let front_r = if c == Color::White { r + 1 } else { r - 1 };
                    if (0..8).contains(&front_r)
                        && board.pieces[c.idx()][PieceType::Pawn.idx()] & bb(sq(f as u8, front_r as u8)) != 0
                    {
                        score += sign * 5;
                    }

                    // Outpost: casa avancada (4a-6a fileira do lado
                    // proprio), defendida por peao proprio, e onde nenhum
                    // peao inimigo nas colunas adjacentes pode alguma vez
                    // capturar a peca -- classico posto avancado, dificil
                    // de expulsar.
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
                            score += sign * if pt == PieceType::Knight { 25 } else { 15 };
                        }
                    }
                }

                if pt == PieceType::Bishop {
                    // Bispo na grande diagonal central: ataca pelo menos 2
                    // das 4 casas centrais (d4/e4/d5/e5) -- diagonal aberta
                    // e influente, mais do que um bispo generico de
                    // fianchetto.
                    let center: Bitboard = bb(sq(3, 3)) | bb(sq(4, 3)) | bb(sq(3, 4)) | bb(sq(4, 4));
                    if count(attacks & center) >= 2 {
                        score += sign * 10;
                    }

                    // "Bispo mau": penaliza peoes proprios na mesma cor de
                    // casa do bispo -- bloqueiam o proprio alcance, tipico
                    // de estruturas fechadas onde este bispo fica preso.
                    let bishop_sq_color = (file_of(s) + rank_of(s)) % 2;
                    let mut own_pawns_iter = board.pieces[c.idx()][PieceType::Pawn.idx()];
                    let mut same_color_pawns = 0i32;
                    while own_pawns_iter != 0 {
                        let ps = own_pawns_iter.trailing_zeros() as Square;
                        own_pawns_iter &= own_pawns_iter - 1;
                        if (file_of(ps) + rank_of(ps)) % 2 == bishop_sq_color {
                            same_color_pawns += 1;
                        }
                    }
                    score -= sign * same_color_pawns * 2;
                }
            }
        }

        // Bonus de densidade: quantas pecas DISTINTAS (nao quantas casas)
        // apontam a zona do rei inimigo. Nao-linear de proposito -- e' o
        // que faz a busca preferir juntar 3 atacantes a manter material
        // extra parado, o "sacrificio especulativo" caracteristico.
        let idx = attacker_count.min(ATTACK_DENSITY.len() - 1);
        if idx > 0 {
            score += sign * (ATTACK_DENSITY[idx] + attack_units / 4);
        }

        // Peoes passados: sem peao inimigo a bloquear na mesma coluna ou
        // colunas adjacentes, a frente do peao -- bonus crescente com o
        // avanco (empurrar peoes passados e' iniciativa concreta).
        let mut pawns = board.pieces[c.idx()][PieceType::Pawn.idx()];
        while pawns != 0 {
            let s = pawns.trailing_zeros() as Square;
            pawns &= pawns - 1;
            let f = file_of(s) as i32;
            let r = rank_of(s) as i32;
            let mut blocked = false;
            let enemy_pawns = board.pieces[c.opp().idx()][PieceType::Pawn.idx()];
            for adj in (f - 1)..=(f + 1) {
                if !(0..8).contains(&adj) {
                    continue;
                }
                // mascara "a frente" (na coluna adj, a partir do peao):
                // calculada por percurso simples de fileiras, robusta e
                // facil de verificar (evita bit-tricks frageis).
                let mut m: Bitboard = 0;
                if c == Color::White {
                    for rr in (r + 1)..8 {
                        m |= bb(sq(adj as u8, rr as u8));
                    }
                } else {
                    for rr in 0..r {
                        m |= bb(sq(adj as u8, rr as u8));
                    }
                }
                if enemy_pawns & m != 0 {
                    blocked = true;
                    break;
                }
            }
            if !blocked {
                let advance = if c == Color::White { r } else { 7 - r };
                score += sign * (10 + advance * advance);
            }

            // Peao isolado: nenhum peao proprio nas colunas adjacentes,
            // em qualquer fileira -- fraqueza estrutural classica, nao
            // pode ser defendido por outro peao.
            let mut has_neighbor_file = false;
            for adj in (f - 1)..=(f + 1) {
                if adj == f || !(0..8).contains(&adj) {
                    continue;
                }
                if board.pieces[c.idx()][PieceType::Pawn.idx()] & (FILE_A << adj) != 0 {
                    has_neighbor_file = true;
                    break;
                }
            }
            if !has_neighbor_file {
                score -= sign * 15;
            }

            // Peao defendido por outro peao proprio (cadeia/falange) --
            // truque da tabela de ataque de peao invertida (mesma ideia
            // do SEE em search.rs): as casas de onde um peao proprio
            // atacaria `s` sao as casas atacadas por um peao INIMIGO
            // hipotetico em `s`.
            if a.pawn[c.opp().idx()][s as usize] & board.pieces[c.idx()][PieceType::Pawn.idx()] != 0 {
                score += sign * 6;
            }

            // Falange de peoes: outro peao proprio na mesma fileira, na
            // coluna adjacente -- apoiam-se mutuamente no avanco, tipico
            // de estruturas agressivas de centro.
            for adj in [f - 1, f + 1] {
                if (0..8).contains(&adj) && board.pieces[c.idx()][PieceType::Pawn.idx()] & bb(sq(adj as u8, r as u8)) != 0 {
                    score += sign * 5;
                    break;
                }
            }
        }

        // Peoes dobrados: mais de um peao proprio na mesma coluna --
        // penaliza-se uma vez por peao excedente (contado por coluna,
        // nao por peao, para nao repetir a mesma fraqueza duas vezes).
        for file in 0..8 {
            let n = count(board.pieces[c.idx()][PieceType::Pawn.idx()] & (FILE_A << file)) as i32;
            if n > 1 {
                score -= sign * 12 * (n - 1);
            }
        }
    }
    score
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
