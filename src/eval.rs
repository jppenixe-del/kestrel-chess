use crate::attacks::*;
use crate::bitboard::*;
use crate::board::Board;
use crate::types::*;
use std::sync::OnceLock;

static ATTACKS: OnceLock<Attacks> = OnceLock::new();
fn atk() -> &'static Attacks {
    ATTACKS.get_or_init(Attacks::new)
}

// Piece-square tables (perspetiva das brancas, a1=indice 0 .. h8=indice 63,
// exatamente como os bitboards). Valores classicos simples -- ponto de
// partida, nao afinados. Peca preta usa o espelho vertical (flip da fileira).
#[rustfmt::skip]
const PAWN_PST: [i32; 64] = [
     0,  0,  0,  0,  0,  0,  0,  0,
     5, 10, 10,-20,-20, 10, 10,  5,
     5, -5,-10,  0,  0,-10, -5,  5,
     0,  0,  0, 20, 20,  0,  0,  0,
     5,  5, 10, 25, 25, 10,  5,  5,
    10, 10, 20, 30, 30, 20, 10, 10,
    50, 50, 50, 50, 50, 50, 50, 50,
     0,  0,  0,  0,  0,  0,  0,  0,
];
#[rustfmt::skip]
const KNIGHT_PST: [i32; 64] = [
    -50,-40,-30,-30,-30,-30,-40,-50,
    -40,-20,  0,  5,  5,  0,-20,-40,
    -30,  5, 10, 15, 15, 10,  5,-30,
    -30,  0, 15, 20, 20, 15,  0,-30,
    -30,  5, 15, 20, 20, 15,  5,-30,
    -30,  0, 10, 15, 15, 10,  0,-30,
    -40,-20,  0,  0,  0,  0,-20,-40,
    -50,-40,-30,-30,-30,-30,-40,-50,
];
#[rustfmt::skip]
const BISHOP_PST: [i32; 64] = [
    -20,-10,-10,-10,-10,-10,-10,-20,
    -10,  5,  0,  0,  0,  0,  5,-10,
    -10, 10, 10, 10, 10, 10, 10,-10,
    -10,  0, 10, 10, 10, 10,  0,-10,
    -10,  5,  5, 10, 10,  5,  5,-10,
    -10,  0,  5, 10, 10,  5,  0,-10,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -20,-10,-10,-10,-10,-10,-10,-20,
];
#[rustfmt::skip]
const ROOK_PST: [i32; 64] = [
     0,  0,  0,  5,  5,  0,  0,  0,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
    -5,  0,  0,  0,  0,  0,  0, -5,
     5, 10, 10, 10, 10, 10, 10,  5,
     0,  0,  0,  0,  0,  0,  0,  0,
];
#[rustfmt::skip]
const QUEEN_PST: [i32; 64] = [
    -20,-10,-10, -5, -5,-10,-10,-20,
    -10,  0,  5,  0,  0,  0,  0,-10,
    -10,  5,  5,  5,  5,  5,  0,-10,
      0,  0,  5,  5,  5,  5,  0, -5,
     -5,  0,  5,  5,  5,  5,  0, -5,
    -10,  0,  5,  5,  5,  5,  0,-10,
    -10,  0,  0,  0,  0,  0,  0,-10,
    -20,-10,-10, -5, -5,-10,-10,-20,
];
#[rustfmt::skip]
const KING_MID_PST: [i32; 64] = [
     20, 30, 10,  0,  0, 10, 30, 20,
     20, 20,  0,  0,  0,  0, 20, 20,
    -10,-20,-20,-20,-20,-20,-20,-10,
    -20,-30,-30,-40,-40,-30,-30,-20,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
    -30,-40,-40,-50,-50,-40,-40,-30,
];

fn pst(kind: PieceType, color: Color, s: Square) -> i32 {
    let idx = if color == Color::White {
        s as usize
    } else {
        // espelha verticalmente: fileira 0<->7, mantem coluna
        let f = file_of(s);
        let r = 7 - rank_of(s);
        (r * 8 + f) as usize
    };
    match kind {
        PieceType::Pawn => PAWN_PST[idx],
        PieceType::Knight => KNIGHT_PST[idx],
        PieceType::Bishop => BISHOP_PST[idx],
        PieceType::Rook => ROOK_PST[idx],
        PieceType::Queen => QUEEN_PST[idx],
        PieceType::King => KING_MID_PST[idx],
    }
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
        }
    }
    score
}

/// Avaliacao: material + PST + termos posicionais/taticos ("estilo
/// Polgar" -- pressao sobre o rei inimigo, mobilidade pesada, iniciativa
/// via peoes passados e torres ativas). Devolve da perspetiva de quem
/// tem a jogar (convencao negamax).
pub fn evaluate(board: &Board) -> i32 {
    material_pst(board) + positional_terms_signed(board)
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

fn material_pst(board: &Board) -> i32 {
    let mut score = 0i32;
    for c in [Color::White, Color::Black] {
        let sign = if c == Color::White { 1 } else { -1 };
        for pt in ALL_PIECES {
            let mut bbp = board.pieces[c.idx()][pt.idx()];
            while bbp != 0 {
                let s = bbp.trailing_zeros() as Square;
                bbp &= bbp - 1;
                score += sign * (pt.value() + pst(pt, c, s));
            }
        }
    }
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
