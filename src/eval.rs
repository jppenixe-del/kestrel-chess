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
// h8=indice 63, exatamente como os bitboards -- peca preta usa o espelho
// vertical, flip da fileira). Fonte: PeSTO's Evaluation Function (Ronald
// Friederich / chessprogramming.org), reproduzidas literalmente e so'
// reordenadas de rank8-primeiro (convencao da pagina) para rank1-primeiro
// (convencao deste codigo) -- nenhum valor foi inventado ou afinado.
// Substitui as PST unicas antigas (sem fase) por um par mg/eg por peca,
// interpolado pela fase do jogo (ver `phase()`/`taper()` abaixo) -- e' a
// peca "Tapered Evaluation" da lista do Sirius que faltava por completo.
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

/// Valores de material por fase -- tambem PeSTO literal (o Rei nao tem
/// "valor material", so' PST; o resto da avaliacao ja' assume material
/// suficiente por definicao). Distintos de `PieceType::value()`, que
/// continua a servir SEE/MVV-LVA (troca material simples, sem fase).
const MG_VALUE: [i32; 6] = [82, 337, 365, 477, 1025, 0];
const EG_VALUE: [i32; 6] = [94, 281, 297, 512, 936, 0];

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
