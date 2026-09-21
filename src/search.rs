// The search.
//
// Deliberately a small, sound search rather than a full one: iterative
// deepening with aspiration windows, principal variation search, quiescence,
// transposition table, killers and history, null move and late move
// reductions. Everything beyond that is added one at a time and measured, so
// that what each addition is worth is a number and not an opinion.
//
// Time management is here from the start rather than bolted on later, because
// an engine that loses on the clock is worth nothing whatever it scores at
// infinite time. See `allocate`.

use crate::attacks::Attacks;
use crate::board::Board;
use crate::moves::{Move, MoveFlag};
use crate::movegen::{generate_legal, generate_legal_caps};
use crate::nnue;
use crate::see;
use crate::tt::{Bound, TranspositionTable, TT_EVAL_NONE};
use crate::types::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How much to reduce a late quiet move by, indexed by depth and by how many
/// moves have already been tried.
///
/// A table rather than an expression because the expression wanted logarithms,
/// and the first version computed them in integers -- `ln(3)` and `ln(4)` both
/// truncate to 1, so the reduction was very nearly a constant and the whole
/// point of reducing later moves harder was lost.
///
/// Rebuilt when its two numbers change rather than computed per node: four
/// thousand logarithms is nothing once a move, and real work once a node.
/// Para cada tipo de peca, as casas atacadas por uma peca MENOR do adversario.
///
/// Nao e' o mesmo que "atacada": uma dama onde um peao lhe chega esta' em
/// perigo, uma dama onde so' uma torre lhe chega nao esta'. E' por isso que a
/// mascara e' por tipo e nao uma so'.
fn ameacas_menores(board: &Board, by: Color, atk: &crate::attacks::Attacks) -> [u64; 6] {
    use crate::attacks::{bishop_attacks, rook_attacks};
    let p = &board.pieces[by.idx()];
    let occ = board.occ_all;
    let mut peoes = 0u64;
    let mut b = p[PieceType::Pawn.idx()];
    while b != 0 {
        let sq = b.trailing_zeros() as usize;
        b &= b - 1;
        peoes |= atk.pawn[by.idx()][sq];
    }
    let mut menores = 0u64;
    let mut b = p[PieceType::Knight.idx()];
    while b != 0 {
        let sq = b.trailing_zeros() as usize;
        b &= b - 1;
        menores |= atk.knight[sq];
    }
    let mut b = p[PieceType::Bishop.idx()];
    while b != 0 {
        let sq = b.trailing_zeros() as Square;
        b &= b - 1;
        menores |= bishop_attacks(sq, occ);
    }
    let mut torres = 0u64;
    let mut b = p[PieceType::Rook.idx()];
    while b != 0 {
        let sq = b.trailing_zeros() as Square;
        b &= b - 1;
        torres |= rook_attacks(sq, occ);
    }
    // Peao e rei nao tem "menor" que os ameace de forma util.
    [0, peoes, peoes, peoes | menores, peoes | menores | torres, 0]
}

/// Todas as casas atacadas por um lado, peoes e rei incluidos.
///
/// Uma vez por no', para as duas perguntas que o historico com contexto faz:
/// a peca esta' a fugir de uma ameaca? vai para uma casa ameacada?
fn todas_ameacas(board: &Board, by: Color, atk: &crate::attacks::Attacks) -> u64 {
    use crate::attacks::{bishop_attacks, rook_attacks};
    let p = &board.pieces[by.idx()];
    let occ = board.occ_all;
    let mut m = 0u64;
    // Os peoes todos de uma vez, em dois deslocamentos.
    //
    // Antes iterava-se peao a peao com uma consulta a` tabela por cada um: ate'
    // oito voltas de ciclo e oito acessos a memoria para produzir o mesmo
    // bitboard que duas instrucoes dao. Os ataques de peao sao a unica coisa
    // aqui que nao depende das pecas que estao no caminho, portanto sao os
    // unicos que se podem calcular em bloco.
    let peoes = p[PieceType::Pawn.idx()];
    m |= if by == Color::White {
        ((peoes & !crate::bitboard::FILE_A) << 7) | ((peoes & !crate::bitboard::FILE_H) << 9)
    } else {
        ((peoes & !crate::bitboard::FILE_H) >> 7) | ((peoes & !crate::bitboard::FILE_A) >> 9)
    };
    let mut b = p[PieceType::Knight.idx()];
    while b != 0 {
        let sq = b.trailing_zeros() as usize;
        b &= b - 1;
        m |= atk.knight[sq];
    }
    // A dama entra nas duas listas em vez de ter ciclo proprio.
    //
    // Ela ataca como bispo E como torre, portanto o trabalho e' o mesmo -- as
    // duas consultas magicas fazem-se de qualquer maneira. O que se poupa e' um
    // ciclo inteiro de controlo e a leitura repetida do bitboard das damas.
    let damas = p[PieceType::Queen.idx()];
    let mut b = p[PieceType::Bishop.idx()] | damas;
    while b != 0 {
        let sq = b.trailing_zeros() as Square;
        b &= b - 1;
        m |= bishop_attacks(sq, occ);
    }
    let mut b = p[PieceType::Rook.idx()] | damas;
    while b != 0 {
        let sq = b.trailing_zeros() as Square;
        b &= b - 1;
        m |= rook_attacks(sq, occ);
    }
    m |= atk.king[board.king_sq(by) as usize];
    m
}

/// Em que dos quatro baldes do historico este lance cai.
///
/// A ideia vem do motor de referencia, cuja tabela e' indexada por sete coisas
/// onde a nossa usa tres. As duas que importam sao estas: a peca esta' a fugir
/// de uma ameaca, e vai para uma casa ameacada.
///
/// O mesmo lance de b1 para b5 quer dizer coisas opostas conforme b1 estiver
/// atacada (foge) ou nao (manobra), e conforme b5 estiver atacada (pendura-se)
/// ou nao (segura). Nos metiamos os quatro casos no mesmo balde e perguntavamos
/// a` media o que fazer.
#[inline]
fn balde(ameacadas: u64, mv: &Move) -> usize {
    let de = (ameacadas >> mv.from) & 1;
    let para = (ameacadas >> mv.to) & 1;
    (de * 2 + para) as usize
}

/// As casas de onde cada tipo de peca daria xeque ao rei adversario.
///
/// Calculado do rei para fora, que e' uma vez por no' em vez de uma por lance.
fn casas_de_xeque(board: &Board, contra: Color, atk: &crate::attacks::Attacks) -> [u64; 6] {
    use crate::attacks::{bishop_attacks, rook_attacks};
    let k = board.king_sq(contra);
    let occ = board.occ_all;
    let b = bishop_attacks(k, occ);
    let r = rook_attacks(k, occ);
    [
        atk.pawn[contra.idx()][k as usize],
        atk.knight[k as usize],
        b,
        r,
        b | r,
        0,
    ]
}

/// Quem reduz o que: [vezes, milesimos somados] por termo.
///
/// Sete termos empilham-se no mesmo numero, e o total nao diz qual deles fez o
/// trabalho. Dois podem estar a anular-se e um a fazer tudo.
/// Quantas vezes a ponte estava ligada e nao respondeu.
///
/// Deve ser ZERO sempre. Qualquer valor acima disso quer dizer que houve
/// avaliacoes feitas por outro avaliador sem ninguem pedir.
pub static FALHAS_PONTE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub static QUEM: [(std::sync::atomic::AtomicU64, std::sync::atomic::AtomicI64); 9] = [
    (std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicI64::new(0)),
    (std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicI64::new(0)),
    (std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicI64::new(0)),
    (std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicI64::new(0)),
    (std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicI64::new(0)),
    (std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicI64::new(0)),
    (std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicI64::new(0)),
    (std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicI64::new(0)),
    (std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicI64::new(0)),
];

pub const NOMES_QUEM: [&str; 9] = [
    "tabela", "janela", "piora", "captura", "cut node", "nao-PV", "tt-pv", "historico",
    "tt captura",
];

pub fn quem_ligado() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HALF2K_QUEM").as_deref() == Ok("1"))
}

#[inline]
fn conta_quem(i: usize, d: i32) {
    if quem_ligado() && d != 0 {
        // Vezes, soma COM SINAL, e soma dos MODULOS. As duas ultimas dizem
        // coisas diferentes: a com sinal diz para que lado o termo puxa, a dos
        // modulos diz quanto ele mexe. Um termo equilibrado soma zero e pode
        // estar a fazer todo o trabalho.
        QUEM[i].0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        QUEM[i].1.fetch_add(d as i64, std::sync::atomic::Ordering::Relaxed);
        QUEM_ABS[i].fetch_add(d.unsigned_abs() as u64, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Em que lance o corte aconteceu: 0, 1, 2, 3, 4-7, 8-15, 16+, e "nunca".
///
/// Ordenacao que sabe o que faz corta quase sempre no primeiro. Cada corte
/// mais abaixo e' uma sub-arvore inteira percorrida para nada -- e' ai que a
/// cegueira custa, nao na formula da reducao.
pub static CORTES: [std::sync::atomic::AtomicU64; 8] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Escala do numero que julga um lance tranquilo.
///
/// [0] quantos lances, [1] soma dos modulos, [2] o maior visto,
/// [3] soma dos modulos so' da tabela principal,
/// [4..9] soma dos modulos de cada ply de continuacao.
pub static ESCALA: [std::sync::atomic::AtomicI64; 10] = [
    std::sync::atomic::AtomicI64::new(0), std::sync::atomic::AtomicI64::new(0),
    std::sync::atomic::AtomicI64::new(0), std::sync::atomic::AtomicI64::new(0),
    std::sync::atomic::AtomicI64::new(0), std::sync::atomic::AtomicI64::new(0),
    std::sync::atomic::AtomicI64::new(0), std::sync::atomic::AtomicI64::new(0),
    std::sync::atomic::AtomicI64::new(0), std::sync::atomic::AtomicI64::new(0),
];

pub fn escala_ligada() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HALF2K_ESCALA").as_deref() == Ok("1"))
}

pub fn cego_ligado() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HALF2K_CEGO").as_deref() == Ok("1"))
}

/// Distribuicao do `lmr_depth` do bloco `PodaSF`, por balde.
///
/// A pergunta que este contador respondeu: o divisor que converte historia em
/// profundidade esta' a produzir um sinal ou ruido?
///
/// Medido, tres posicoes a` profundidade 11, por divisor:
///
///     divisor   negativos   devolucao media   |devolucao| media
///        700      36,0%        +0,03 plies       2,10 plies
///       1400      15,6%        -0,05             0,55
///       2800       3,4%        +0,13             0,29
///       8000       3,9%        +0,04             0,04
///
/// A media COM SINAL e' praticamente zero em toda a amplitude enquanto a media
/// dos MODULOS vai a 2,10 plies. Um termo util empurra para um lado consoante o
/// lance presta; este empurra para os dois por igual. E' ruido com dois plies de
/// amplitude, e por isso o varrimento do divisor da' monotono: quanto maior o
/// divisor menos ruido entra, e o "optimo" e' o valor que desliga o termo.
///
/// Baldes: <=-20, -19..-10, -9..-5, -4..-1, 0, 1..3, 4..7, 8..11, 12+
pub static LMRD: [std::sync::atomic::AtomicU64; 9] = [
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];
/// [0] soma da devolucao do historico, [1] quantas vezes, [2] |devolucao| soma
pub static LMRD_DEV: [std::sync::atomic::AtomicI64; 3] = [
    std::sync::atomic::AtomicI64::new(0), std::sync::atomic::AtomicI64::new(0),
    std::sync::atomic::AtomicI64::new(0),
];
pub fn lmrd_ligado() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HALF2K_LMRD").as_deref() == Ok("1"))
}
pub fn conta_lmrd(d: i32, devolucao: i32) {
    if !lmrd_ligado() { return; }
    let b = match d {
        i32::MIN..=-20 => 0, -19..=-10 => 1, -9..=-5 => 2, -4..=-1 => 3,
        0 => 4, 1..=3 => 5, 4..=7 => 6, 8..=11 => 7, _ => 8,
    };
    LMRD[b].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    LMRD_DEV[0].fetch_add(devolucao as i64, std::sync::atomic::Ordering::Relaxed);
    LMRD_DEV[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    LMRD_DEV[2].fetch_add(devolucao.unsigned_abs() as i64, std::sync::atomic::Ordering::Relaxed);
}

#[inline]
pub fn conta_corte(i: usize) {
    if !cego_ligado() {
        return;
    }
    let b = match i {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4..=7 => 4,
        8..=15 => 5,
        _ => 6,
    };
    CORTES[b].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Quem corta, por categoria: 0 tabela, 1 captura, 2 killer, 3 tranquilo.
pub static CORTE_TIPO: [std::sync::atomic::AtomicU64; 4] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Quando o corte veio LOGO ao primeiro lance, de que categoria era esse
/// primeiro. Com o contador do lado, da' a taxa de acerto de cada estagio
/// quando ele poe um lance a` frente -- que e' a medida da ordenacao.
pub static PRIMEIRO_CERTOU: [std::sync::atomic::AtomicU64; 4] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// A CAUDA: nos de corte que precisaram de 17 lances ou mais, pelo estagio de
/// onde veio o primeiro lance tentado.
///
/// Porque' este balde e nao outro: medido, os cortes ao 17o lance ou depois sao
/// **2,3% dos nos de corte e 27,3% dos lances procurados neles**. Sao a parte
/// mais cara da ordem por uma margem enorme, e sao poucos -- portanto se se
/// perceber o que os caracteriza, ha' muito a ganhar num sitio pequeno.
pub static CAUDA: [std::sync::atomic::AtomicU64; 4] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];
/// Dos nos da cauda, quantos tinham lance da tabela disponivel.
pub static CAUDA_TT: [std::sync::atomic::AtomicU64; 2] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Killers por posicao: acertou / falhou quando foi ele o primeiro lance.
/// Os tres pontuam 400k, 390k e 380k, e o melhor tranquilo que o historico
/// conhece vale 120k -- portanto os tres passam SEMPRE a` frente de todos os
/// tranquilos. Se o segundo e o terceiro acertarem pouco, estao a ocupar um
/// lugar que nao merecem.
pub static KILLER_CERTOU: [std::sync::atomic::AtomicU64; NUM_KILLERS] =
    [const { std::sync::atomic::AtomicU64::new(0) }; NUM_KILLERS];
pub static KILLER_FALHOU: [std::sync::atomic::AtomicU64; NUM_KILLERS] =
    [const { std::sync::atomic::AtomicU64::new(0) }; NUM_KILLERS];

/// Quando o corte NAO veio ao primeiro lance, de que categoria era o primeiro
/// -- ou seja, o que estamos a por a` frente que nao devia la' estar.
pub static PRIMEIRO_FALHOU: [std::sync::atomic::AtomicU64; 4] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

#[inline]
pub fn conta_killer(slot: usize, certou: bool) {
    if !cego_ligado() || slot >= NUM_KILLERS {
        return;
    }
    let t = if certou { &KILLER_CERTOU } else { &KILLER_FALHOU };
    t[slot].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn conta_cauda(i: usize, tipo_primeiro: usize, tinha_tt: bool) {
    if !cego_ligado() || i < 16 {
        return;
    }
    if let Some(c) = CAUDA.get(tipo_primeiro) {
        c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    CAUDA_TT[usize::from(tinha_tt)].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn conta_tipos(i: usize, tipo_corte: usize, tipo_primeiro: usize) {
    if !cego_ligado() {
        return;
    }
    use std::sync::atomic::Ordering::Relaxed;
    CORTE_TIPO[tipo_corte.min(3)].fetch_add(1, Relaxed);
    if i > 0 {
        PRIMEIRO_FALHOU[tipo_primeiro.min(3)].fetch_add(1, Relaxed);
    } else {
        PRIMEIRO_CERTOU[tipo_primeiro.min(3)].fetch_add(1, Relaxed);
    }
}

#[inline]
pub fn conta_sem_corte() {
    if cego_ligado() {
        CORTES[7].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub static QUEM_ABS: [std::sync::atomic::AtomicU64; 8] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// [0] lances procurados, [1] re-buscas por reducao, [2] re-buscas por
/// janela, [3] nos gastos nas re-buscas.
pub static REB: [std::sync::atomic::AtomicU64; 4] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// [0] nos, [1] lances pontuados, [2] lances procurados, [3] nos que
/// cortaram sem chegar a um tranquilo.
/// De que e' feita a arvore. Ligado por HALF2K_EST=1 e relatado no fim de cada
/// busca.
///
/// 0 nos da busca principal   1 nos de quiescencia
/// 2 lances procurados        3 nos que cortaram ao PRIMEIRO lance
/// 4 re-buscas por LMR falhada  5 re-buscas de janela cheia
extern "C" {
    fn h2k_argmax_i32(v: *const i32, n: usize, inicio: usize) -> usize;
}

/// Por onde os nos SAEM.
///
/// A pergunta do Joao: as funcoes encadeiam-se pela mesma ordem que as dos
/// outros? Num motor bem organizado a maior parte dos nos sai cedo -- na
/// tabela, ou numa poda barata -- e nunca chega a gerar um lance. Se nos
/// gerarmos lances em quase todos, e' ai' que estao os 2,6x de arvore a mais,
/// e nao na velocidade de cada operacao.
///
/// 0 entrou  1 quiescencia  2 empate/limite  3 tabela  4 tablebases
/// 5 futilidade inversa  6 razoring  7 lance nulo  8 probcut
/// 9 gerou lances  10 sem lances legais
pub static SAIDA: [std::sync::atomic::AtomicU64; 14] = [
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
];

#[inline]
fn marca(i: usize) {
    if est_ligado() {
        SAIDA[i].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub static FORMA: [std::sync::atomic::AtomicU64; 6] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

pub static EST: [std::sync::atomic::AtomicU64; 4] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];

/// Por limite de origem do lance da tabela: [primeiro, cortou] x
/// [exacto, inferior, superior].
pub static TTB: [std::sync::atomic::AtomicU64; 6] = [
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0), std::sync::atomic::AtomicU64::new(0),
];

pub fn ttb_ligado() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HALF2K_TTB").as_deref() == Ok("1"))
}

pub fn est_ligado() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HALF2K_EST").as_deref() == Ok("1"))
}

pub fn reb_ligado() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("HALF2K_REB").as_deref() == Ok("1"))
}

#[inline]
fn conta_reb(i: usize, n: u64) {
    if reb_ligado() {
        REB[i].fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    }
}

fn build_lmr_table(base: i32, div: i32) -> [[i32; 64]; 64] {
    let base = base as f64 / 100.0;
    let div = (div as f64 / 100.0).max(0.01);
    let mut t = [[0i32; 64]; 64];
    for d in 1..64usize {
        for m in 1..64usize {
            // Em MILESIMOS de ply. Truncar aqui deitava fora tudo o que a
            // formula diz entre um ply e dois, antes de os ajustes -- captura,
            // cut node, tt-pv, historico -- poderem usar essa precisao.
            t[d][m] = ((base + (d as f64).ln() * (m as f64).ln() / div) * 1024.0) as i32;
        }
    }
    t
}

/// Correction history: how wrong the static evaluation usually is here.
///
/// The pruning margins are fixed numbers compared against the static score.
/// When that score is *systematically* wrong for a family of positions -- and
/// it is, because the network cannot see what only the search finds -- those
/// margins bite in the wrong place, the same way every time. This keeps a
/// running average of what the search ended up saying minus what the static
/// score said, indexed by pawn structure, and feeds it back next time that
/// structure appears.
///
/// The key is the two pawn bitboards mixed together rather than an incremental
/// key. Derived from the state, it cannot drift out of sync with it, and a
/// heuristic table tolerates collisions by construction.
const CORR_SIZE: usize = 16384;

/// Where one table's entry saturates.
///
/// It was 8192 with a grain of 256, which meant the whole correction, summed
/// over every table, could reach thirty two units -- sixteen centipawns. The
/// reverse futility margin is a hundred and fifty per ply. A correction that
/// small cannot move a pruning decision, which is the only thing it exists to
/// do, and measuring it found exactly the nothing that implies.
const CORR_MAX: i32 = 1024;

/// The most one update may move an entry.
const CORR_MAX_UPDATE: i32 = CORR_MAX / 4;

/// How much each reading is trusted, out of the divisor below.
///
/// Not an average. The tables answer different questions and are not equally
/// good at it: the pawn structure says the most by a wide margin, what remains
/// of each side's force says less, and the shape of the last move says less
/// again. Proportions taken from an engine where they were tuned over games.
const CORR_WEIGHT: [i32; CORR_KINDS] = [203, 109, 109, 121, 72];
const CORR_DIVISOR: i32 = 2048;

/// How many separate readings of "what kind of position is this" the
/// correction is spread over.
///
/// Five, and which five matters more than how many. Two of the first set were
/// a rook-and-queen key and a knight-and-bishop key, and both were dropped:
/// each is a strict subset of the non-pawn key, so they answered a question
/// already answered and were only adding their own collisions to it. The
/// non-pawn key took their place twice, once per colour -- how much force each
/// side has left are different facts and were being merged into one.
///
/// The fifth is the last move as a change rather than as a destination: the
/// difference between the position key before and after it, which carries
/// where the piece came from, what it took and whose move it was. What it
/// learns is "a change of this shape tends to be misread", which is not
/// something the piece-and-square key can express.
pub const CORR_KINDS: usize = 5;

#[inline]
fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A Zobrist key over one subset of the pieces.
///
/// Built from the same per-piece randoms the position key uses, so two boards
/// share a key only when the chosen pieces stand on the same squares in the
/// same colours. That is the whole point and it was what the previous version
/// threw away: it keyed on the occupancy bitboard, which cannot tell a white
/// queen on d1 from a black knight on d1, and on `both()` bitboards that merged
/// the colours outright. A table indexed like that files a correction learned
/// in one position where an unrelated position will read it, and the two teach
/// each other nothing -- measured, switching it on cost fourteen Elo.
#[inline]
fn subset_key(board: &Board, colors: &[Color], types: &[PieceType]) -> u64 {
    let z = crate::zobrist::tabelas();
    let mut k = 0u64;
    for &c in colors {
        for &t in types {
            let mut bb = board.pieces[c.idx()][t.idx()];
            while bb != 0 {
                let sq = bb.trailing_zeros() as usize;
                bb &= bb - 1;
                k ^= z.piece_sq[c.idx()][t.idx()][sq];
            }
        }
    }
    k
}

/// The five indices for this position: pawns, White's remaining force, Black's
/// remaining force, the last move by piece and destination, and the last move
/// as a change of position key.
#[inline]
fn corr_indices(
    board: &Board,
    last: Option<(usize, usize)>,
    hash_delta: u64,
) -> [usize; CORR_KINDS] {
    use Color::{Black, White};
    use PieceType::{Bishop, Knight, Pawn, Queen, Rook};

    const BOTH: [Color; 2] = [White, Black];
    const FORCE: [PieceType; 4] = [Knight, Bishop, Rook, Queen];

    let pawns = subset_key(board, &BOTH, &[Pawn]);
    let np_white = subset_key(board, &[White], &FORCE);
    let np_black = subset_key(board, &[Black], &FORCE);

    let cont = match last {
        Some((pc, to)) => mix((pc as u64) << 8 | to as u64 | 0x5eed_0000_0000),
        None => 0,
    };

    // Zero when there is no previous position to differ from, which files
    // everything at the root in one slot rather than in a random one.
    let trans = if hash_delta == 0 { 0 } else { mix(hash_delta) };

    // The subset keys are XORs of randoms, so mix once more before taking the
    // low bits as an index.
    let m = (CORR_SIZE - 1) as u64;
    [
        (mix(pawns) & m) as usize,
        (mix(np_white) & m) as usize,
        (mix(np_black) & m) as usize,
        (cont & m) as usize,
        (trans & m) as usize,
    ]
}

/// How many continuation tables, and how far back each looks.
/// Quiet moves remembered per ply as having caused a cutoff. Three rather
/// than two: the extra one costs a comparison and catches the case where two
/// different refutations alternate, which two slots lose to immediately.
pub const NUM_KILLERS: usize = 3;

pub const CONT_SLOTS: usize = 5;
/// Quantos plies atras cada tabela de continuacao olha.
///
/// Eram tres, em {1, 2, 4}. A referencia usa cinco, em {1, 2, 3, 4, 6} -- os
/// plies 3 e 6 faltavam-nos. Os dois novos entram com peso zero enquanto a
/// opcao `ContLongo` estiver desligada, portanto a omissao nao muda nada.
pub const CONT_BACK: [usize; CONT_SLOTS] = [1, 2, 4, 3, 6];
/// Os mesmos plies que a busca de referencia le': 1, 3 e 5 -- e sao TODOS do
/// adversario, porque os lados alternam.
///
/// Os nossos sao 1, 2 e 4: um lance do adversario e DOIS nossos. A nota da
/// referencia diz que a escolha dela e' deliberada, e porque': um lance
/// tranquilo e' bom ou mau sobretudo em resposta ao que o adversario anda a
/// fazer, e meter os nossos proprios lances nas mesmas tabelas faz-lhes outra
/// pergunta. Duas perguntas na mesma tabela sao duas respostas a estragarem-se
/// uma a` outra.
pub const CONT_BACK_ADV: [usize; CONT_SLOTS] = [1, 3, 5, 7, 9];
/// Weight per slot when scoring, the reply carrying twice the rest.
pub const CONT_WEIGHT: [i32; CONT_SLOTS] = [2, 1, 1, 1, 1];

/// Where a history entry settles. Separate ceilings because the two tables
/// answer different questions and the continuation one is asked more precisely,
/// so it is allowed to be more emphatic.
const HIST_MAX_MAIN: i32 = 15000;

/// Marca de um lance tranquilo ainda por pontuar. Abaixo de qualquer valor de
/// historico real e de qualquer sentinela, para ficar no fim da lista ate' ser
/// avaliado.
/// Onde um tranquilo ainda por pontuar se senta enquanto espera.
///
/// Era `i32::MIN + 7`, e ai' estava o defeito. A ordem desta busca e': tabela
/// um milhao, capturas boas seiscentos mil, promocao a dama quinhentos mil,
/// killers quatrocentos mil, tranquilos pelo historico (medido: nunca acima de
/// 120 mil), e as capturas mas em MENOS seiscentos mil. Um sentinela no fundo
/// do i32 punha os tranquilos por pontuar abaixo das capturas mas -- ou seja, a
/// busca passava a experimentar sacrificios ja' refutados antes de qualquer
/// lance tranquilo, e so' os pontuava quando mais nada restava. Nao era a ideia
/// de adiar que custava os 36% de nos a mais: era isto.
///
/// Menos quinhentos mil poe-nos onde os tranquilos pertencem: por baixo dos
/// killers e por cima de todas as capturas mas, que no pior caso chegam a menos
/// 585 mil.
const TRANQUILO_POR_PONTUAR: i32 = -500_000;
const HIST_MAX_CONT: i32 = 30000;

/// What one cutoff is worth, by the depth that found it.
///
/// Linear and generous. It was `min(1200, d*d)` -- at depth ten that is a
/// hundred against two thousand, so the tables took eighty cutoffs to reach
/// where they now reach in six, and move ordering spent most of its time acting
/// on information that had gone stale. Ordering is what decides whether late
/// move reductions are reducing the right moves, so this is not a small thing.
/// Escala do bonus do historico, em milesimos do valor herdado.
///
/// A medicao que a motivou: as continuacoes tem tecto de 30000 e a media do
/// que a busca la' le' e' **301** -- um por cento do tecto. Tabelas assim vazias
/// nao distinguem lances, e e' por isso que o tranquilo tentado em primeiro so'
/// corta 7,4% das vezes contra 86,4% do lance da tabela.
pub static HB_ESC: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1000);
pub static HB_TECTO: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(4000);

#[inline]
fn hist_bonus(depth: i32) -> i32 {
    let e = HB_ESC.load(std::sync::atomic::Ordering::Relaxed);
    let t = HB_TECTO.load(std::sync::atomic::Ordering::Relaxed);
    ((200 * depth).min(t) * e / 1000).max(1)
}

/// What one capture cutoff is worth.
///
/// Its own curve, steeper and with an offset, rather than the quiet one. A
/// capture that works is a stronger statement than a quiet move that works --
/// there were fewer of them to choose from and the material says whether it
/// paid -- so the table should move further on it. Reusing the quiet bonus made
/// this three times too small at the depths where it matters.
#[inline]
fn capt_bonus(depth: i32) -> i32 {
    (depth * 680 - 250).clamp(0, 2400)
}

/// Where a capture history entry settles.
const HIST_MAX_CAPT: i32 = 16384;
const HIST_MAX_PC: i32 = 15000;

/// Move towards the ceiling by an amount that shrinks as it is approached, so
/// an entry saturates instead of running away.
#[inline]
fn hist_add(entry: &mut i32, bonus: i32, max: i32) {
    *entry += bonus - *entry * bonus.abs() / max;
}

/// Uma tabela de historico partilhavel, a zeros.
fn cria_hist(n: usize) -> Vec<std::sync::atomic::AtomicI32> {
    (0..n).map(|_| std::sync::atomic::AtomicI32::new(0)).collect()
}

/// As tabelas partilhadas sao planas: os indices aninhados nao sobrevivem a
/// serem atomicos, e um `Vec<AtomicI32>` com aritmetica de indice e' a forma
/// mais simples que mantem uma so' alocacao para todos os fios.
#[inline(always)]
fn ich(k: usize, idx: usize, pc: usize, to: usize) -> usize {
    ((k * (6 * 64) + idx) * 6 + pc) * 64 + to
}

/// Quantas chaves de peoes distintas a tabela guarda. Potencia de dois, para o
/// indice sair de uma mascara e nao de um resto.
///
/// 512, que e' o tamanho da referencia. Com 16384 a tabela dava 50 MB e cada
/// ajudante alocava a sua antes de a trocar pela partilhada -- o texto de 18-09
/// avisa disso mesmo: "uma ajudante que partilha devolve a memoria da tabela
/// propria, senao eram 28 MB por ajudante so' para nao os usar". Aqui sao
/// 1,5 MB.
const TAM_PEAO: usize = 512;

#[inline(always)]
fn ipeao(chave: usize, pc12: usize, to: usize) -> usize {
    (chave * 12 + pc12) * 64 + to
}

#[inline(always)]
fn icorr(k: usize, side: usize, idx: usize) -> usize {
    (k * 2 + side) * CORR_SIZE + idx
}

/// O mesmo que `hist_add`, num sitio que varios fios podem tocar.
///
/// `Relaxed` chega: ninguem depende da ORDEM entre duas actualizacoes, so' de
/// nenhuma delas ler lixo. Em x86-64 compila para o mesmo `mov`.
#[inline(always)]
fn hist_add_at(e: &std::sync::atomic::AtomicI32, bonus: i32, max: i32) {
    let v = e.load(Ordering::Relaxed);
    e.store(v + bonus - v * bonus.abs() / max, Ordering::Relaxed);
}

/// How much of the plan this search has earned, judged between iterations.
///
/// Four readings of "does this position still need thinking", multiplied.
///
/// `effort` is the share of nodes that went into the move we mean to play. A
/// search that has poured almost everything into one move has found its answer
/// and is confirming it; one still splitting nodes across rivals has not
/// decided. This is the sturdiest of the four, being a ratio over millions of
/// nodes rather than a verdict that can turn on one.
///
/// `settle` counts iterations that kept the same root move, and decays fast. It
/// is deliberately the weakest term. Keying elastic time on stability ALONE was
/// tried in an earlier engine of ours and reverted the same day: on a forced
/// recapture, where there is nothing to decide, the root move still changed at
/// three separate depths and each change threw the multiplier back to maximum.
/// Stability may lengthen a search, never on its own, and never without the
/// wall standing behind it.
///
/// `falling` is a score dropping between iterations, which neither of the
/// others can see: effort stays high while a position collapses under it, and
/// stability watches whether the move changed rather than whether it got worse.
/// Only falls count -- paying extra for good news is how a clock is spent on
/// won positions.
///
/// `instability` is a move the search keeps overturning. It separates a hard
/// position from a slow one, which the first two cannot: both read "several
/// moves are equally good" as difficulty, so they fire on quiet positions with
/// many reasonable answers.
fn time_scale(effort_frac: f64, settle: u32, score_drop: i32, changes: u32,
              steady: Option<u32>, escala_max: i32, escala_min: i32) -> f64 {
    let effort = (1.40 - effort_frac) * 1.55;
    let settle = (0.95 + 2.2 * (settle as f64 + 2.6).powf(-1.5)).max(1.0);
    // A score that has stopped moving is confidence, and confidence is time
    // that can be spent elsewhere. The old term only ever added: it paid for a
    // falling score and did nothing for a settled one, so a position that had
    // been quiet for six iterations cost exactly as much as one still being
    // argued about, and there was never anything saved to spend later.
    //
    // It also counts movement in BOTH directions. A score climbing fast is as
    // much a reason to look further as one falling -- something has changed and
    // the previous iterations were describing a different position.
    let falling = match steady {
        Some(k) => [1.39, 1.19, 1.01, 0.93, 0.88][k.min(4) as usize],
        None => {
            if score_drop > 0 {
                (1.0 + score_drop as f64 * 0.004).min(1.5)
            } else {
                1.0
            }
        }
    };
    let instability = (1.0 + changes as f64 * 0.22).min(2.4);
    // O GRAMPO FINAL, em milesimos. 3400 = 3,4x, que e' como estava cravado.
    //
    // Fica ajustavel porque no KestrelStrike o varrimento deste grampo fechou
    // com um numero: sair de 2,45x para 5,00x valeu +17,76 +/- 7,12 em 2076
    // partidas (LLR 2,95, aceite), e ha' um PLANALTO de 4,0 a 5,0 -- entre os
    // dois o motor e' indiferente em 4618 partidas -- com degradacao acima.
    //
    // O half2k esta' em 3,4x, abaixo da borda desse planalto. Mas o elastico
    // dele NAO e' o mesmo: tem outros factores e outros limites por factor,
    // portanto o numero de la' nao se copia -- e' ponto de partida para varrer,
    // nao valor validado. Por isso a omissao nao muda: 3400.
    (effort * settle * falling * instability)
        .clamp(escala_min as f64 / 1000.0, escala_max as f64 / 1000.0)
}

/// How far above the root the continuation tables may reach into the game.
pub const PRE_MOVES: usize = 6;

/// Floor of the base two logarithm, which is what the alternative reduction
/// formula is built on.
#[inline]
fn ilog2i(v: i32) -> i32 {
    if v <= 0 {
        0
    } else {
        31 - (v as u32).leading_zeros() as i32
    }
}

pub const MAX_PLY: usize = 128;
pub const INF: i32 = 32_000;
pub const MATE: i32 = 31_000;
/// Anything at least this large is a mate score, not an evaluation.
pub const MATE_IN_MAX: i32 = MATE - MAX_PLY as i32;


/// Every number the search compares something against.
///
/// They are options rather than constants because not one of them was measured
/// -- each was picked to be sane and then left alone, which is a different
/// thing from being right. Exposed, they can be walked over by a tuner playing
/// games, which is the only process that has ever produced good ones.
///
/// Two are stored multiplied by a hundred, because the shape they belong to is
/// a logarithm and the option protocol only carries integers.
#[derive(Clone, Copy)]
pub struct Params {
    pub rfp_margin: i32,
    /// Declive da barra do SEE que separa captura boa de ma', por ply.
    pub capt_bar_f: i32,
    /// Tecto dessa barra.
    pub capt_bar_max: i32,
    /// O divisor da barra do SEE quando ela vem do MERITO do lance em vez da
    /// profundidade. 0 = como estava. Item 12 do VALIDACOES.md.
    ///
    /// As capturas sao 55,5% dos primeiros lances tentados e cortam 77,3% das
    /// vezes -- e' o termo com mais peso na taxa de corte ao primeiro lance.
    /// Um ponto ganho aqui vale meio ponto no total.
    pub capt_bar_div: i32,
    /// Divisor do peso da historia de capturas na ordem. 256 reproduz o
    /// comportamento herdado (`capt_score / 16`); menos pesa mais.
    pub capt_hist_div: i32,
    pub rfp_improving: i32,
    /// Quanto cresce a reducao quando nao se melhora, em 512 avos.
    pub lmr_piora_f: i32,
    /// Quanto a largura da janela alivia a reducao, em milesimos de ply.
    pub lmr_janela_f: i32,
    pub rfp_depth: i32,
    pub razor_margin: i32,
    pub razor_depth: i32,
    pub nmp_base: i32,
    pub nmp_div: i32,
    pub nmp_prof_div: i32,
    pub recusa_margem: i32,
    pub recusa_limiar: i32,
    pub triagem_n: i32,
    pub ord_see: i32,
    pub ord_mvv: i32,
    pub ord_killer: i32,
    pub ord_hist_n: i32,
    pub ord_xeque: i32,
    pub xeque_banda: i32,
    pub lmp_base: i32,
    pub lmp_depth: i32,
    pub fut_base: i32,
    pub fut_slope: i32,
    pub fut_depth: i32,
    /// Divisor applied to the history score inside the forward futility
    /// margin. In OUR history units, which run about five and a half times
    /// smaller than the scale this value was originally written for.
    pub fut_hist_div: i32,
    pub hist_prune: i32,
    /// Static exchange threshold for quiet moves, as `-x * (d + d*d)`.
    pub see_prune_quiet: i32,
    /// A check is only worth extending for when the position is not already
    /// decided.
    pub check_ext_eval: i32,
    /// What a draw is worth to the side to move, in the units the network
    /// speaks, where two are a centipawn.
    ///
    /// Zero means a draw is a draw. Above zero means the engine would rather
    /// keep playing than repeat, which is worth something when the opponent
    /// evaluates positions the same way we do -- against an engine sharing our
    /// network, agreement about what is equal turns into a repetition, and half
    /// the games end that way.
    ///
    /// It is not free: an engine that refuses a draw it should take loses games
    /// it should have halved. Small, and measured.
    pub contempt: i32,
    pub see_prune: i32,
    pub fut_capt_base: i32,
    pub fut_slope_red: i32,
    pub see_quiet_red: i32,
    pub poda_hist_div: i32,
    pub poda_sf_ttpv: i32,
    pub poda_sf_capt_base: i32,
    pub poda_sf_capt_slope: i32,
    pub poda_sf_capt_hist: i32,
    pub poda_sf_capt_see: i32,
    pub poda_sf_cont_prune: i32,
    pub poda_sf_div: i32,
    pub poda_sf_fut_slope: i32,
    pub poda_sf_fut_base: i32,
    pub poda_sf_see: i32,
    /// x100
    pub lmr_base: i32,
    /// x100
    pub lmr_div: i32,
    /// Quanto a reducao cresce quando o ply SEGUINTE ja' cortou muitas vezes,
    /// em milesimos de ply. 0 = como estava.
    ///
    /// Do KestrelStrike. Um no' cujos filhos nao param de cortar e' um no'
    /// facil. Medido la': 64.076 -> 37.695 nos a` mesma profundidade.
    /// Quanta profundidade se desconta depois de o alpha subir. 0 = como
    /// estava. Do KestrelStrike.
    pub alpha_desc: i32,
    /// A janela em que o desconto age. No binario que jogava: 3 e 12.
    pub ad_min: i32,
    pub ad_max: i32,
    pub cut_cnt_base: i32,
    pub cut_cnt_mais: i32,
    pub lmr_cut: i32,
    pub lmr_hist_div: i32,
    /// O divisor quando o historico fala alto.
    pub lmr_hist_div_forte: i32,
    /// Peso da ameaca por peca menor, em centesimos do valor da peca.
    pub ordem_ameaca_f: i32,
    /// Numerador do optimism. 114 e' o valor da referencia e o do KestrelStrike.
    pub otimismo_f: i32,
    /// Grampo final do elastico do tempo, em milesimos. 3400 = 3,4x.
    pub tm_escala_max: i32,
    /// Peso da historia de peoes, em 32 avos. 32 = peso um.
    pub peao_f: i32,
    /// TECTO DURO SEM INCREMENTO, em percentagem do relogio. 0 = desligado.
    ///
    /// O tecto normal e' um MULTIPLO do optimo, e um multiplo do optimo nao e'
    /// tecto nenhum: se o optimo sobe, o tecto sobe com ele. Com incremento
    /// tudo bem, que o gasto e' reposto; sem incremento cada milissegundo
    /// gasto foi-se para sempre.
    ///
    /// O QUE ISSO CUSTOU, medido no KestrelStrike (PDaYgQnB, 600+0, perdida a`
    /// bandeira ao lance 163): no lance 12, com 501s no relogio, o optimo era
    /// 24,9s e o tecto 24,9 x 5,5 = 137s. O motor gastou 73s e passou por
    /// baixo do tecto a` vontade. Com 10% do relogio o tecto era 50s e aquele
    /// lance nao tinha existido.
    ///
    /// A regra vem do Coda (GPL-3), que a escreveu depois das mesmas derrotas:
    /// "For no-inc sudden death: cap max at 15% of clock, hard at 10%". O
    /// numero transfere-se porque a unidade e' percentagem de relogio e nao
    /// depende da arvore de ninguem.
    ///
    /// E' TECTO, nao orcamento: nao muda o que o motor planeia gastar, so'
    /// impede o lance unico que arruina a partida.
    pub tm_sem_inc_tecto: i32,
    /// CHAO do elastico, em milesimos. 650 = 0,65, que e' como estava cravado.
    ///
    /// Nos lances obvios o motor pede menos do que o chao o deixa gastar, e o
    /// que sobra nao volta. No KestrelStrike baixar de 0,65 para 0,50 estava a
    /// dar +4,01 +/- 12,92 sem veredicto quando a maquina se perdeu.
    pub tm_escala_min: i32,
    /// QUAL PERCENTIL da duracao. 0=p50 1=p60 2=p70 3=p75 4=p80.
    ///
    /// OMISSAO 0 (p50). O KestrelStrike TEM isto -- a mesma tabela, os mesmos
    /// cinco percentis, os mesmos nos (em plies la', em lances aqui) -- e
    /// tambem com a omissao a 0. Recuperado a 20-09 da conversa exportada.
    ///
    /// Nao esta' no binario `ks_1.20260919` porque entrou DEPOIS de esse
    /// binario ser construido: a cadeia KS_TM_CURVA_PCT nao existe em nenhum
    /// dos nove binarios guardados. Por isso nao ha' afinacao para transportar
    /// -- o que ha' e' a peca, e a peca esta' aqui igual.
    ///
    /// O que o binario que JOGAVA ja' tinha, e esta' medido: o horizonte que
    /// cresce (`KS_TM_CURVA = 1`). Com 480.000 ms no relogio ele gasta 16,4 s
    /// ao lance 16 e 5,6 s ao lance 30 -- assume ~19 lances a faltar no
    /// primeiro caso e ~56 no segundo. Com `KS_TM_CURVA=0` volta aos ~20
    /// fixos. Logo a decisao de que o horizonte cresce esta' tomada do lado de
    /// la'; o que falta decidir e' o percentil.
    ///
    /// E isso decide-se com partidas -- p75 contra p50 -- porque orcar pela
    /// mediana e' orcar para metade das partidas rebentarem o orcamento, e sao
    /// essas que acabam a zero.
    pub tm_curva_pct: i32,
    /// O HORIZONTE CRESCE depois de a estimativa ser desmentida, em centesimos
    /// de lance por lance de excesso. 0 = desligado.
    ///
    /// A tabela acaba no lance 110. Sem isto, dai' para a frente dizia sempre
    /// o mesmo numero. Uma partida que passou o ultimo degrau provou que e'
    /// das longas, e sao essas que acabam a` bandeira.
    ///
    /// 100 = um por um.
    pub tm_cresce: i32,
    /// Quanto vale dar xeque sem perder material.
    pub ordem_xeque_f: i32,
    /// Peso da historia peca-casa, em centesimos do peso da tabela principal.
    pub hist_pc_f: i32,
    /// Quantas capturas vulgares se procuram na quiescencia antes de travar.
    pub travao_qs_n: i32,
    /// Credito de um killer, somado ao historico em vez de o substituir.
    /// 30000 e' um quarto do maior historico observado (120293).
    pub killer_bonus: i32,
    pub asp_delta: i32,
    pub asp_depth: i32,
    pub sing_depth: i32,
    pub sing_margin: i32,
    /// How far below the singular window a move has to fall to earn a second
    /// ply rather than one.
    pub double_ext: i32,
    /// Reductions accumulate in 1024ths and divide at the end, so a term can be
    /// worth a third of a ply instead of all or nothing. These are in those
    /// units.
    pub lmr_cut_f: i32,
    /// Reducao a MAIS num no' de corte que nao tem lance da tabela.
    ///
    /// A hipotese era: um no' de corte sem lance da tabela e' onde a ordem nao
    /// tem nada de bom para tentar primeiro, logo devia reduzir-se mais la'.
    /// Sustentava-a a medicao das taxas de acerto do primeiro lance -- tabela
    /// 86,4%, captura 77,3%, killer 49,4%, tranquilo 7,4%.
    ///
    /// MEDIDO E NAO PRESTA, tres posicoes a` profundidade 10:
    ///
    ///     base                       corte ao 1o 73,0%   78.993 nos
    ///     CutNodeLmr                             72,6%   85.490
    ///     CutNodeLmr + LmrCutF=4026              73,5%   82.616
    ///
    /// Pior nos dois eixos, ou quase. Fica o parametro porque custa nada tendo o
    /// `CutNodeLmr` desligado, mas a ideia esta' respondida.
    pub lmr_cut_sem_tt: i32,
    pub lmr_tt_capt_f: i32,
    pub lmr_nonpv_f: i32,
    pub lmr_ttpv_f: i32,
    /// A stored lower bound this far above beta already answers the question.
    ///
    /// In OUR units. The value it came from belongs to a program whose pawn is
    /// about 255 where ours is 200, so it is scaled by that ratio -- the third
    /// time today a constant has been carried across a scale boundary, and the
    /// first two both silently disabled the thing they were meant to control.
    pub probcut_margin: i32,
    /// How far the score may move and still count as settled. Two hundred
    /// is a pawn here, so forty is a fifth of one -- converted, not
    /// carried across from the scale the shape came from.
    pub tm_trend_window: i32,
    /// What a capture is credited with beyond the piece it takes, before the
    /// quiescence margin gives up on it.
    ///
    /// Two hundred is exactly one pawn here, and it got there by reading a
    /// number off another engine without converting it: in the scale it came
    /// from, the same test allows about a pawn and a half. Measured at one
    /// pawn, this threw away 45% of the captures it looked at in a tactical
    /// position -- a great deal for a search whose only job is not to miss
    /// tactics -- and switching it on cost 63 Elo over a thousand games.
    pub qs_margin: i32,
    /// Below this many pieces on the board, nothing is reduced at all.
    pub lmr_endgame_pieces: i32,
    /// A gestao de tempo do pawn. Ver o ramo em `allocate`.
    pub tm_mtg: i32,
    /// Onde entra o lance da tabela que veio de um limite superior. Abaixo das
    /// capturas boas (600.000) e acima dos killers (400.000).
    pub tt_fraco_pont: i32,
    /// Lances estimados no inicio da partida, quando a curva esta' ligada.
    pub tm_mtg_base: i32,
    /// Quanto a estimativa desce por lance, em centesimos.
    pub tm_mtg_declive: i32,
    /// O chao da estimativa, para o fim da partida nao ficar sem reserva.
    pub tm_mtg_min: i32,
    /// Que fraccao do ritmo dele podemos igualar, em por cento. Noventa e
    /// cinco em vez de cem: sempre que ambos vamos ao limite, ganhamos.
    pub tm_predador_pct: i32,
    /// Ate' que relogio o predador se aplica, em segundos. Acima disto ha'
    /// tempo para pensar e acompanhar o ritmo dele nao acrescenta nada.
    pub tm_predador_ate_s: i32,
    /// percent of the increment spent each move
    pub tm_inc_pct: i32,
    pub tm_hard_mult: i32,
    /// percent of what is left that the wall may reach
    pub tm_hard_pct: i32,
    /// Percent of the base allowance by game phase. The opening is played
    /// rather than calculated, and a simplified position has less to find.
    pub tm_open_pct: i32,
    pub tm_early_pct: i32,
    pub tm_mid_pct: i32,
    pub tm_late_pct: i32,
    pub tm_simple_pct: i32,
    /// What to do about the other clock: more when comfortably ahead on it,
    /// less when behind.
    pub tm_ahead_pct: i32,
    pub tm_behind_pct: i32,
    /// Never think for less than this, so a low clock still buys a move that
    /// was looked at rather than one that was guessed.
    /// Ate' onde o orcamento planeia, quando o arbitro nao diz quantos lances
    /// faltam. Nao e' a duracao esperada da partida.
    pub tm_horizonte_max: i32,
    /// Por quantos lances o TmPawn reparte o bolo quando nao ha' movestogo.
    pub tm_pawn_n: i32,
    /// Tecto duro do TmPawn, em centesimos do optimo (200 = 2x, como estava).
    pub tm_pawn_tecto: i32,
    /// Minimo dos lances que faltam na curva do TmPawn.
    pub tm_pawn_curva_min: i32,
    pub tm_floor_ms: i32,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            rfp_margin: 110,
            capt_bar_f: 50,
            capt_bar_max: 250,
            capt_bar_div: 60,
            capt_hist_div: 256,
            rfp_improving: 150,
            lmr_piora_f: 197,
            lmr_janela_f: 577,
            rfp_depth: 9,
            razor_margin: 348,
            razor_depth: 5,
            nmp_base: 4,
            nmp_div: 6,
            nmp_prof_div: 3,
            recusa_margem: 30,
            recusa_limiar: 0,
            triagem_n: 6,
            ord_see: 150,
            ord_mvv: 16,
            ord_killer: 54_000,
            ord_hist_n: 106,
            ord_xeque: 60_000,
            xeque_banda: 0,
            lmp_base: 3,
            lmp_depth: 6,
            fut_base: 100,
            fut_slope: 150,
            fut_depth: 12,
            fut_hist_div: 75,
            hist_prune: 600,
            see_prune: 70,
            fut_capt_base: 234,
            fut_slope_red: 100,
            see_quiet_red: 100,
            poda_hist_div: 4000,
            poda_sf_ttpv: 929,
            poda_sf_capt_base: 234,
            poda_sf_capt_slope: 247,
            poda_sf_capt_hist: 134,
            poda_sf_capt_see: 177,
            poda_sf_cont_prune: 965,
            poda_sf_div: 700,
            poda_sf_fut_slope: 119,
            poda_sf_fut_base: 164,
            poda_sf_see: 23,
            see_prune_quiet: 5,
            check_ext_eval: 75,
            contempt: 0,
            lmr_base: 77,
            lmr_div: 236,
            alpha_desc: 0,
            ad_min: 3,
            ad_max: 12,
            cut_cnt_base: 0,
            cut_cnt_mais: 0,
            lmr_cut: 2,
            lmr_hist_div: 22000,
            lmr_hist_div_forte: 4000,
            ordem_ameaca_f: 2000,
            otimismo_f: 114,
            tm_escala_max: 3400,
            peao_f: 32,
            tm_sem_inc_tecto: 0,
            tm_escala_min: 650,
            tm_curva_pct: 0,
            tm_cresce: 0,
            ordem_xeque_f: 16384,
            hist_pc_f: 2400,
            travao_qs_n: 2,
            killer_bonus: 30000,
            asp_delta: 25,
            asp_depth: 4,
            sing_depth: 5,
            sing_margin: 2,
            double_ext: 40,
            lmr_cut_f: 2048,
            lmr_cut_sem_tt: 933,
            lmr_tt_capt_f: 1079,
            lmr_nonpv_f: 1024,
            lmr_ttpv_f: 1024,
            probcut_margin: 294,
            tm_trend_window: 40,
            qs_margin: 450,
            lmr_endgame_pieces: 0,
            tm_mtg: 27,
            tt_fraco_pont: 550_000,
            tm_mtg_base: 35,
            tm_mtg_declive: 67,
            tm_mtg_min: 14,
            tm_predador_pct: 95,
            tm_predador_ate_s: 120,
            tm_inc_pct: 75,
            tm_hard_mult: 4,
            tm_hard_pct: 25,
            tm_open_pct: 30,
            tm_early_pct: 70,
            tm_mid_pct: 110,
            tm_late_pct: 100,
            tm_simple_pct: 60,
            tm_ahead_pct: 115,
            tm_behind_pct: 85,
            tm_horizonte_max: 50,
            tm_pawn_n: 20,
            tm_pawn_tecto: 200,
            tm_pawn_curva_min: 14,
            tm_floor_ms: 10,
        }
    }
}

/// Name, current value, and the range a tuner may walk it over.
pub type ParamSpec = (&'static str, fn(&Params) -> i32, fn(&mut Params, i32), i32, i32);

pub const PARAM_SPECS: &[ParamSpec] = &[
    ("HistBonusEsc", |_| crate::search::HB_ESC.load(std::sync::atomic::Ordering::Relaxed),
       |_, v| crate::search::HB_ESC.store(v, std::sync::atomic::Ordering::Relaxed), 100, 8000),
    ("HistBonusTecto", |_| crate::search::HB_TECTO.load(std::sync::atomic::Ordering::Relaxed),
       |_, v| crate::search::HB_TECTO.store(v, std::sync::atomic::Ordering::Relaxed), 500, 30000),
    ("CaptBarF", |p| p.capt_bar_f, |p, v| p.capt_bar_f = v, 0, 300),
    ("CaptBarMax", |p| p.capt_bar_max, |p, v| p.capt_bar_max = v, 0, 1200),
    ("CaptBarDiv", |p| p.capt_bar_div, |p, v| p.capt_bar_div = v, 0, 200),
    ("CaptHistDiv", |p| p.capt_hist_div, |p, v| p.capt_hist_div = v, 16, 2048),
    ("RfpMargin", |p| p.rfp_margin, |p, v| p.rfp_margin = v, 40, 300),
    ("RfpImproving", |p| p.rfp_improving, |p, v| p.rfp_improving = v, 0, 300),
    ("LmrPioraF", |p| p.lmr_piora_f, |p, v| p.lmr_piora_f = v, 0, 512),
    ("LmrJanelaF", |p| p.lmr_janela_f, |p, v| p.lmr_janela_f = v, 0, 1500),
    ("RfpDepth", |p| p.rfp_depth, |p, v| p.rfp_depth = v, 2, 12),
    ("RazorMargin", |p| p.razor_margin, |p, v| p.razor_margin = v, 50, 900),
    ("RazorDepth", |p| p.razor_depth, |p, v| p.razor_depth = v, 1, 10),
    ("NmpBase", |p| p.nmp_base, |p, v| p.nmp_base = v, 2, 8),
    ("NmpDiv", |p| p.nmp_div, |p, v| p.nmp_div = v, 2, 12),
    ("NmpProfDiv", |p| p.nmp_prof_div, |p, v| p.nmp_prof_div = v, 2, 12),
    ("RecusaMargem", |p| p.recusa_margem, |p, v| p.recusa_margem = v, 0, 200),
    ("RecusaLimiar", |p| p.recusa_limiar, |p, v| p.recusa_limiar = v, 0, 600),
    ("TriagemN", |p| p.triagem_n, |p, v| p.triagem_n = v, 1, 32),
    ("OrdSee", |p| p.ord_see, |p, v| p.ord_see = v, 0, 1000),
    ("OrdMvv", |p| p.ord_mvv, |p, v| p.ord_mvv = v, 0, 200),
    ("OrdKiller", |p| p.ord_killer, |p, v| p.ord_killer = v, 0, 300000),
    ("OrdHistN", |p| p.ord_hist_n, |p, v| p.ord_hist_n = v, 0, 600),
    ("OrdXeque", |p| p.ord_xeque, |p, v| p.ord_xeque = v, 0, 300000),
    ("XequeBanda", |p| p.xeque_banda, |p, v| p.xeque_banda = v, 0, 900000),
    ("LmpBase", |p| p.lmp_base, |p, v| p.lmp_base = v, 1, 10),
    ("LmpDepth", |p| p.lmp_depth, |p, v| p.lmp_depth = v, 2, 12),
    ("FutBase", |p| p.fut_base, |p, v| p.fut_base = v, 20, 400),
    ("FutSlope", |p| p.fut_slope, |p, v| p.fut_slope = v, 30, 300),
    ("FutDepth", |p| p.fut_depth, |p, v| p.fut_depth = v, 2, 16),
    ("FutHistDiv", |p| p.fut_hist_div, |p, v| p.fut_hist_div = v, 20, 200),
    ("HistPrune", |p| p.hist_prune, |p, v| p.hist_prune = v, 100, 2000),
    ("SeePrune", |p| p.see_prune, |p, v| p.see_prune = v, 20, 250),
    // A inclinacao da futilidade e a margem do SEE, em percentagem, para quando
    // a poda usa a profundidade REDUZIDA. Cem deixa como esta'; duzentos duplica,
    // que e' o que compensa uma profundidade que fica tipicamente a metade.
    ("FutSlopeRed", |p| p.fut_slope_red, |p, v| p.fut_slope_red = v, 50, 400),
    ("SeeQuietRed", |p| p.see_quiet_red, |p, v| p.see_quiet_red = v, 50, 400),
    ("PodaHistDiv", |p| p.poda_hist_div, |p, v| p.poda_hist_div = v, 500, 40000),
    ("PodaSfTtpv", |p| p.poda_sf_ttpv, |p, v| p.poda_sf_ttpv = v, 0, 3000),
    ("PodaSfCaptBase", |p| p.poda_sf_capt_base, |p, v| p.poda_sf_capt_base = v, 0, 800),
    ("PodaSfCaptSlope", |p| p.poda_sf_capt_slope, |p, v| p.poda_sf_capt_slope = v, 0, 800),
    ("PodaSfCaptHist", |p| p.poda_sf_capt_hist, |p, v| p.poda_sf_capt_hist = v, 0, 600),
    ("PodaSfCaptSee", |p| p.poda_sf_capt_see, |p, v| p.poda_sf_capt_see = v, 20, 500),
    ("PodaSfContPrune", |p| p.poda_sf_cont_prune, |p, v| p.poda_sf_cont_prune = v, 100, 8000),
    ("PodaSfDiv", |p| p.poda_sf_div, |p, v| p.poda_sf_div = v, 100, 8000),
    ("PodaSfFutSlope", |p| p.poda_sf_fut_slope, |p, v| p.poda_sf_fut_slope = v, 0, 400),
    ("PodaSfFutBase", |p| p.poda_sf_fut_base, |p, v| p.poda_sf_fut_base = v, 0, 600),
    ("PodaSfSee", |p| p.poda_sf_see, |p, v| p.poda_sf_see = v, 1, 120),
    ("FutCaptBase", |p| p.fut_capt_base, |p, v| p.fut_capt_base = v, 0, 800),
    ("SeePruneQuiet", |p| p.see_prune_quiet, |p, v| p.see_prune_quiet = v, 1, 40),
    ("CheckExtEval", |p| p.check_ext_eval, |p, v| p.check_ext_eval = v, 0, 400),
    ("Contempt", |p| p.contempt, |p, v| p.contempt = v, 0, 100),
    ("LmrBase", |p| p.lmr_base, |p, v| p.lmr_base = v, 0, 200),
    ("LmrDiv", |p| p.lmr_div, |p, v| p.lmr_div = v, 120, 400),
    ("AlphaDesc", |p| p.alpha_desc, |p, v| p.alpha_desc = v, 0, 3),
    ("AdMin", |p| p.ad_min, |p, v| p.ad_min = v, 0, 12),
    ("AdMax", |p| p.ad_max, |p, v| p.ad_max = v, 4, 32),
    ("CutCnt", |p| p.cut_cnt_base, |p, v| p.cut_cnt_base = v, 0, 1024),
    ("CutCntMais", |p| p.cut_cnt_mais, |p, v| p.cut_cnt_mais = v, 0, 2048),
    ("LmrCut", |p| p.lmr_cut, |p, v| p.lmr_cut = v, 0, 4),
    ("LmrHistDiv", |p| p.lmr_hist_div, |p, v| p.lmr_hist_div = v, 4096, 65536),
    ("LmrHistDivForte", |p| p.lmr_hist_div_forte, |p, v| p.lmr_hist_div_forte = v, 1000, 30000),
    ("OrdemAmeacaF", |p| p.ordem_ameaca_f, |p, v| p.ordem_ameaca_f = v, 0, 8000),
    ("OtimismoF", |p| p.otimismo_f, |p, v| p.otimismo_f = v, 0, 400),
    ("TmEscalaMax", |p| p.tm_escala_max, |p, v| p.tm_escala_max = v, 1000, 8000),
    ("PeaoF", |p| p.peao_f, |p, v| p.peao_f = v, 0, 256),
    ("TmSemIncTecto", |p| p.tm_sem_inc_tecto, |p, v| p.tm_sem_inc_tecto = v, 0, 60),
    ("TmEscalaMin", |p| p.tm_escala_min, |p, v| p.tm_escala_min = v, 300, 1000),
    ("TmCurvaPct", |p| p.tm_curva_pct, |p, v| p.tm_curva_pct = v, 0, 4),
    ("TmCresce", |p| p.tm_cresce, |p, v| p.tm_cresce = v, 0, 200),
    ("OrdemXequeF", |p| p.ordem_xeque_f, |p, v| p.ordem_xeque_f = v, 0, 65536),
    ("HistPecaCasaF", |p| p.hist_pc_f, |p, v| p.hist_pc_f = v, 0, 12800),
    ("TravaoQsN", |p| p.travao_qs_n, |p, v| p.travao_qs_n = v, 1, 32),
    ("KillerBonus", |p| p.killer_bonus, |p, v| p.killer_bonus = v, 0, 400000),
    ("AspDelta", |p| p.asp_delta, |p, v| p.asp_delta = v, 8, 80),
    ("AspDepth", |p| p.asp_depth, |p, v| p.asp_depth = v, 2, 10),
    ("SingDepth", |p| p.sing_depth, |p, v| p.sing_depth = v, 4, 12),
    ("SingMargin", |p| p.sing_margin, |p, v| p.sing_margin = v, 1, 8),
    ("DoubleExt", |p| p.double_ext, |p, v| p.double_ext = v, 5, 200),
    ("LmrCutF", |p| p.lmr_cut_f, |p, v| p.lmr_cut_f = v, 0, 6000),
    ("LmrCutSemTt", |p| p.lmr_cut_sem_tt, |p, v| p.lmr_cut_sem_tt = v, 0, 3000),
    ("LmrTtCaptF", |p| p.lmr_tt_capt_f, |p, v| p.lmr_tt_capt_f = v, 0, 3000),
    ("LmrNonPvF", |p| p.lmr_nonpv_f, |p, v| p.lmr_nonpv_f = v, 0, 2048),
    ("LmrTtPvF", |p| p.lmr_ttpv_f, |p, v| p.lmr_ttpv_f = v, 0, 2048),
    ("ProbcutMargin", |p| p.probcut_margin, |p, v| p.probcut_margin = v, 100, 1024),
    ("TmTrendWindow", |p| p.tm_trend_window, |p, v| p.tm_trend_window = v, 5, 200),
    ("QsMargin", |p| p.qs_margin, |p, v| p.qs_margin = v, 50, 900),
    ("LmrEndgamePieces", |p| p.lmr_endgame_pieces,
     |p, v| p.lmr_endgame_pieces = v, 0, 12),
    ("TmMtg", |p| p.tm_mtg, |p, v| p.tm_mtg = v, 15, 70),
    ("TtFracoPont", |p| p.tt_fraco_pont, |p, v| p.tt_fraco_pont = v, 300_000, 900_000),
    ("TmMtgBase", |p| p.tm_mtg_base, |p, v| p.tm_mtg_base = v, 20, 60),
    ("TmMtgDeclive", |p| p.tm_mtg_declive, |p, v| p.tm_mtg_declive = v, 0, 200),
    ("TmMtgMin", |p| p.tm_mtg_min, |p, v| p.tm_mtg_min = v, 6, 30),
    ("TmPredadorPct", |p| p.tm_predador_pct, |p, v| p.tm_predador_pct = v, 50, 120),
    ("TmPredadorAteS", |p| p.tm_predador_ate_s, |p, v| p.tm_predador_ate_s = v, 0, 600),
    ("TmIncPct", |p| p.tm_inc_pct, |p, v| p.tm_inc_pct = v, 20, 95),
    ("TmHardMult", |p| p.tm_hard_mult, |p, v| p.tm_hard_mult = v, 1, 8),
    ("TmHardPct", |p| p.tm_hard_pct, |p, v| p.tm_hard_pct = v, 15, 70),
    ("TmOpenPct", |p| p.tm_open_pct, |p, v| p.tm_open_pct = v, 20, 120),
    ("TmEarlyPct", |p| p.tm_early_pct, |p, v| p.tm_early_pct = v, 40, 140),
    ("TmMidPct", |p| p.tm_mid_pct, |p, v| p.tm_mid_pct = v, 60, 160),
    ("TmLatePct", |p| p.tm_late_pct, |p, v| p.tm_late_pct = v, 60, 160),
    ("TmSimplePct", |p| p.tm_simple_pct, |p, v| p.tm_simple_pct = v, 30, 140),
    ("TmAheadPct", |p| p.tm_ahead_pct, |p, v| p.tm_ahead_pct = v, 100, 180),
    ("TmBehindPct", |p| p.tm_behind_pct, |p, v| p.tm_behind_pct = v, 40, 100),
    ("TmHorizonteMax", |p| p.tm_horizonte_max, |p, v| p.tm_horizonte_max = v, 10, 80),
    ("TmPawnN", |p| p.tm_pawn_n, |p, v| p.tm_pawn_n = v, 8, 40),
    ("TmPawnTecto", |p| p.tm_pawn_tecto, |p, v| p.tm_pawn_tecto = v, 200, 800),
    ("TmPawnCurvaMin", |p| p.tm_pawn_curva_min, |p, v| p.tm_pawn_curva_min = v, 8, 40),
    ("TmFloorMs", |p| p.tm_floor_ms, |p, v| p.tm_floor_ms = v, 1, 200),
];

impl Params {
    pub fn set(&mut self, name: &str, value: i32) -> bool {
        for (n, _, put, lo, hi) in PARAM_SPECS {
            if n.eq_ignore_ascii_case(name) {
                put(self, value.clamp(*lo, *hi));
                return true;
            }
        }
        false
    }
}

/// Techniques that are switched off until they have earned their place.
///
/// Every one is off by default, so the engine out of the box searches with the
/// smaller, settled set of ideas. What each is worth then has an answer
/// rather than an opinion: turn exactly one on, play a match, read the number.
/// A feature that cannot be switched off is a feature nobody ever measured.
///
/// Measured at 8+0.08 against this engine with everything off, roughly two
/// hundred games each, so the interval on any one of them is about seventy Elo
/// wide and only the extremes below mean anything:
///
///   NmpCutNode  +24    RfpDamp  +20    Razoring  +14
///   Rule50Fade   -4    TtCutCredit -4   CheckExt   -6
///   CaptureHist  -8    Probcut -12     CorrHist  -14
///   LmpImproving -24   TtPvLmr -25     QsFutility -130
///
/// The last one is the only result that needed no second sample: three wins
/// against forty-eight losses. It was not the idea, it was a line of it, and
/// the same turned out to be true of the correction history -- both are
/// commented where they are implemented. That is the lesson these numbers
/// actually carry: a technique that is worth Elo in every strong engine and
/// negative here is a bug report, not a measurement.
#[derive(Clone, Copy)]
pub struct Features {
    /// O horizonte do orcamento sai do relogio em vez de ser uma constante.
    pub tm_horizonte: bool,

    /// A gestao de tempo do pawn: bolo com o incremento dentro, tecto simples.

    /// A reducao do lance nulo a crescer com a profundidade.
    /// Recusar a repeticao imediata quando estamos a` frente e ha' alternativa.
    /// As tabelas de continuacao a ler SO' os lances do adversario.
    /// Dentro da busca de referencia: usar a NOSSA reducao em vez da dela.
    /// O argmax do `pick` em C++ com AVX2, em vez do ciclo em Rust.
    /// Gerar e pontuar os tranquilos so' quando a busca chega a eles.
    /// Exame caro so' a quem o olhar barato poe a` frente.
    /// Ordenacao por soma continua, sem bandas.
    /// O termo do xeque somado a` ordenacao por bandas.
    /// O historico separado por contexto de ameaca.
    /// Podar pela profundidade REDUZIDA, nao pela nominal.
    /// Futilidade tambem para capturas, descontando o que a captura ganha.
    pub fut_capturas: bool,
    pub poda_reduzida: bool,

    /// O historico devolve profundidade a` poda reduzida: um lance que as
    /// tabelas preferem e' julgado a uma profundidade maior do que a reducao
    /// crua lhe daria, e um que elas desprezam a uma menor.
    ///
    /// Sozinha esta linha custou 54 e 63 Elo em duas doses (`ph_4000`,
    /// `ph_2000`), o que diz que a devolucao nao se transplanta a` parte do
    /// conjunto em que vive.
    pub poda_hist: bool,

    /// O Step 15 deles inteiro, em vez de pecas soltas.
    pub poda_sf: bool,
    pub hist_contexto: bool,
    pub xeque_na_ordem: bool,
    pub ordem_continua: bool,
    pub triagem: bool,
    /// A singular so' com limite INFERIOR, como a referencia. Do
    /// KestrelStrike: aceitar exactos abre um conjunto muito maior de
    /// sondagens, e la' media-se 749 sondagens a` profundidade 14 das quais 74%
    /// estendem e 0,1% reduzem.
    pub sing_so_inferior: bool,
    pub gera_etapas: bool,
    pub pick_cpp: bool,
    pub cont_adversario: bool,
    pub recusa_repeticao: bool,
    pub nmp_profundidade: bool,
    pub contempt_atento: bool,
    pub tm_pawn: bool,
    /// Limpar os killers do ply filho ao entrar num no'.
    pub killer_fresco: bool,
    /// Os lances que faltam calculados por uma curva, e o incremento no bolo.
    pub tm_curva: bool,
    /// A forma da curva vinda do relogio em vez do numero do lance.
    pub tm_relogio: bool,
    /// Devolver logo quando so' ha' um lance legal.
    pub lance_unico: bool,
    /// Olhar para o relogio do adversario: tecto pela razao, piso pelo ritmo.
    pub tm_adversario: bool,
    /// Pontuar os lances tranquilos so' quando a busca chega a eles.
    pub pontua_tarde: bool,
    /// O lance da tabela vindo de um limite superior entra depois das capturas
    /// boas, em vez de a` frente de tudo.
    pub tt_fraco: bool,
    /// O lance da tabela de limite superior nao gasta um lugar da poda por
    /// indice.
    pub tt_sem_lmp: bool,
    /// Learn how wrong the static evaluation usually is for a pawn structure,
    /// and feed it back.
    pub corr_hist: bool,
    /// Ask quiescence directly when a node is far enough behind.
    pub razoring: bool,
    /// Fade the evaluation towards a draw as the fifty move counter runs out.
    pub rule50_fade: bool,
    /// Reduce less at a node that once earned a full window.
    pub ttpv_lmr: bool,
    /// Search a ply shallower when the table has no move to try first.
    pub iir: bool,
    /// A IIR nao se aplica a nos ALL (nao-PV e nao-corte). Ver o patch.
    pub iir_no_all: bool,
    /// Politica de escrita da TT (preservar o lance + guarda de profundidade, em par).
    pub tt_politica: bool,
    /// Repeticao a distancia de um lance (tabela cuckoo). Ver `cuckoo.rs`.
    pub cuckoo: bool,

    /// O optimism da referencia, informado a` avaliacao a cada iteracao.
    ///
    ///     optimism = OTIMISMO_F * media / (|media| + 85)
    ///
    /// e a avaliacao usa-o como `(nnue*material + optimism*7675) / 91000`,
    /// depois de o abrir pela discordancia das cabecas (`/476`). A `media` e' a
    /// nota da raiz alisada entre iteracoes -- nao a ultima, que salta.
    ///
    /// O numerador 114 e' o que o KestrelStrike expoe como `KS_OTIMISMO` e
    /// traz LIGADO na versao que o bot joga (medido no binario: omissao 114).
    ///
    /// So' tem efeito pela ponte: e' la' que a formula vive.
    pub otimismo: bool,

    /// A historia de peoes na ordenacao dos tranquilos.
    ///
    /// O que ela sabe e a de-para nao: que um lance e' bom NESTA estrutura de
    /// peoes. A chave muda quando a estrutura muda, e a tabela esquece o que
    /// aprendeu para uma estrutura que ja' nao existe.
    pub hist_peao: bool,

    /// As continuacoes e a correccao passam a ser UMA tabela para todos os
    /// fios, em vez de uma por fio.
    ///
    /// A sessao de 18-09 isolou isto a ler o SF19: eles partilham tres
    /// familias de historico, nos partilhavamos so' a tabela de transposicao.
    /// Medido la': "a quatro fios fazemos 3,17x os nos de um fio, mas chegamos
    /// ao ply 21 onde ela chega ao 25. Os nos estao la'; a aprendizagem e' que
    /// nao circula." No KestrelStrike deu +2,75 +/- 8,21 em 1640 partidas.
    ///
    /// A uma thread nao muda nada -- nao ha' com quem partilhar.
    pub hist_part: bool,
    /// Futilidade inversa so' sem lance na TT, ou com uma captura la' guardada.
    pub rfp_tt_capt: bool,
    /// TmPawn: os lances que faltam vem da curva medida, nao de `TmPawnN` plano.
    pub tm_pawn_curva: bool,
    /// Cut the late move count in half when things are not improving.
    pub lmp_improving: bool,
    /// In quiescence, skip a capture that cannot come near alpha even if it
    /// wins everything it takes.
    ///
    /// Measured five times and negative every time: minus a hundred and thirty
    /// as first written, minus sixty-three after four structural fixes, minus
    /// twenty-eight once the margin was converted into this engine's units.
    /// Rather than a sixth attempt, a count of what it actually discards.
    ///
    /// Of the captures it throws away, 71% in an opening and 62% in a tactical
    /// position pass the static exchange test. They win material or trade
    /// level, and the exchange filter running in the same loop would have kept
    /// every one of them.
    ///
    /// That is the whole answer. Quiescence here already prunes by exchange at
    /// a threshold of zero, so anything losing material is gone before this
    /// test is reached, and what it is left deciding about are the sound
    /// captures. A static margin then removes the moves the search exists to
    /// examine. The technique is not wrong in general; it is wrong on top of a
    /// filter that has already done its work.
    pub qs_futility: bool,
    /// Spend less when most of the tree went to the move that won anyway.
    pub tm_node_effort: bool,
    /// Let a settled score buy time back, instead of only paying for a
    /// falling one. Measured in the sibling engine at +15.0 Elo over 1003
    /// games, together with a moves-left curve that this one already has
    /// in another form.
    pub tm_trend: bool,
    /// Trust a stored lower bound far enough above beta without re-searching.
    pub probcut: bool,
    /// Only try the null move where the node is expected to fail high.
    pub nmp_cut_node: bool,
    /// On a reverse futility cutoff, return part of the way to the estimate
    /// rather than all of it.
    pub rfp_damp: bool,
    /// Extend a move that gives check.
    pub check_ext: bool,
    /// Credit the stored move when the table itself produces the cutoff.
    pub tt_cut_credit: bool,
    /// Remember which captures worked, not only what they take.
    pub capture_hist: bool,
    /// Use the transcribed search instead of this one.
    ///
    /// Not a feature but an experiment: same board, same network, same table,
    /// same clock, and the search swapped whole. Whichever way it comes out
    /// says where the difference lives.
    /// A reducao com a fraccao que a formula lhe da'.
    pub lmr_fino: bool,
    /// Reduzir proporcionalmente mais quando a posicao nao esta' a melhorar.
    pub lmr_piora: bool,
    /// Reduzir menos onde a janela ainda e' larga.
    pub lmr_janela: bool,
    /// O histórico com peso comparavel aos termos cegos.
    pub lmr_hist_forte: bool,
    /// A ordenacao a olhar para a posicao, e nao so' para o passado.
    pub ordem_posicao: bool,
    /// O que costuma resultar com esta peca nesta casa.
    pub hist_pc: bool,
    /// Travao na quiescencia: a partir da enesima captura vulgar, saltar.
    pub travao_qs: bool,
    pub killer_compete: bool,
    pub cont_longo: bool,
    pub sem_killers: bool,
    /// Baixar a profundidade a cada falha por cima seguida.
    pub asp_baixa: bool,
    /// Use the integer-logarithm reduction formula instead of the table.
    ///
    /// The last place where a number in this search is an invention rather
    /// than something measured. Everything else came across with its value;
    /// the reduction shape did not, because it was written before the
    /// reference was read closely, and it is the highest-leverage part of a
    /// search to be guessing at.
    pub log_lmr: bool,

    /// Reduce harder at a node that is expected to fail high.
    ///
    /// Unlike the rest, this is ON by default: it belongs in the baseline
    /// rather than on top of it, and the switch is here to measure it, not to
    /// leave it out.
    pub cut_node_lmr: bool,

    /// Reduzir mais quando o lance da tabela e' uma captura.
    pub lmr_tt_captura: bool,

    /// Skip a quiet move the history has consistently disliked.
    pub history_prune: bool,
    /// Spend longer when the score is falling, less when the best move has
    /// stopped changing.
    pub tm_stability: bool,

    /// Reduce late captures too, not only late quiet moves.
    pub lmr_captures: bool,
}

impl Default for Features {
    fn default() -> Self {
        Features {
            // On by default since 2026-09-01: 1302 games at 16+0.16 put it at
            // +4.8 Elo either way of 19, which is not a gain anyone can bank but
            // is not a loss either, and it was measured after the keys were
            // rebuilt to tell pieces apart -- before that it cost fourteen.
            corr_hist: true,
            tm_horizonte: false,

            fut_capturas: false,
            poda_reduzida: false,
            poda_hist: false,
            poda_sf: false,
            hist_contexto: false,
            xeque_na_ordem: false,
            ordem_continua: false,
            triagem: false,
            sing_so_inferior: false,
            gera_etapas: false,
            pick_cpp: false,
            cont_adversario: false,
            recusa_repeticao: false,
            nmp_profundidade: false,
            contempt_atento: false,
            tm_pawn: false,
            killer_fresco: false,
            tm_curva: false,
            tm_relogio: false,
            lance_unico: false,
            tm_adversario: false,
            pontua_tarde: false,
            tt_fraco: false,
            tt_sem_lmp: false,
            razoring: false,
            rule50_fade: false,
            ttpv_lmr: false,
            iir: true,
            iir_no_all: false,
            tt_politica: false,
            cuckoo: false,
            otimismo: false,
            hist_peao: false,
            hist_part: false,
            rfp_tt_capt: false,
            tm_pawn_curva: false,
            lmp_improving: false,
            qs_futility: false,
            tm_node_effort: true,
            tm_trend: false,
            probcut: false,
            // On by default since 2026-09-01: 1015 games at 16+0.16,
            // +5.8 Elo either way of 21.
            nmp_cut_node: true,
            // On by default since 2026-09-01: 1000 games at 16+0.16, +10.8 Elo
            // either way of 22. Not proof, and not negative, which is the bar.
            rfp_damp: true,
            check_ext: false,
            tt_cut_credit: false,
            capture_hist: false,
            asp_baixa: false,
            lmr_fino: false,
            lmr_piora: false,
            lmr_janela: false,
            lmr_hist_forte: false,
            ordem_posicao: false,
            hist_pc: false,
            travao_qs: false,
            killer_compete: false,
            cont_longo: false,
            sem_killers: false,
            log_lmr: false,
            // Off since 2026-09-01. Switching it OFF measured +6.5 Elo over
            // 1021 games at 16+0.16 -- inside the noise like everything at
            // this sample size, but the direction is that it was costing.
            //
            // It had been on since the search was written, on the strength
            // of belonging in the baseline, and nobody had ever asked it for
            // a number. That is the point of measuring removals: a technique
            // that arrived with the first draft is no more entitled to its
            // place than one proposed yesterday.
            cut_node_lmr: false,
            lmr_tt_captura: false,
            history_prune: true,
            tm_stability: true,
            // Off since 2026-09-01. Switching it OFF measured +9.6 Elo over
            // 1009 games at 16+0.16, the second of two reduction extras to
            // fail the same way -- reducing harder at cut nodes cost 6.5.
            //
            // Two independent measurements saying the same thing is worth
            // more than either: the reduction here is already too deep, and
            // anything that deepens it takes. Which fits what the game
            // records were saying about conversion -- winning positions
            // drawn rather than finished.
            lmr_captures: false,
        }
    }
}

impl Features {
    /// UCI option name to field, for `setoption`.
    pub fn set(&mut self, name: &str, on: bool) -> bool {
        match name {
            "tmhorizonte" => self.tm_horizonte = on,

            "futcapturas" => self.fut_capturas = on,
            "podareduzida" => self.poda_reduzida = on,
            "podahist" => self.poda_hist = on,
            // O `PodaSF` IMPLICA o `CaptureHist`.
            //
            // O bloco le' o historico de capturas em dois sitios; sem a tabela
            // ligada o `capt_score` devolve 0 e o `credit_capture` nem a enche,
            // portanto testar um sem o outro mede a forma incompleta -- que e'
            // exactamente o erro que os `ph_*` a -54 e -63 Elo ja' pagaram.
            // A tabela sozinha continua a poder ser ligada por si.
            "podasf" => {
                self.poda_sf = on;
                if on {
                    self.capture_hist = true;
                }
            }
            "histcontexto" => self.hist_contexto = on,
            "xequenaordem" => self.xeque_na_ordem = on,
            "ordemcontinua" => self.ordem_continua = on,
            "triagem" => self.triagem = on,
            "singsoinferior" => self.sing_so_inferior = on,
            "geraetapas" => self.gera_etapas = on,
            "pickcpp" => self.pick_cpp = on,
            "contadversario" => self.cont_adversario = on,
            "recusarepeticao" => self.recusa_repeticao = on,
            "nmpprofundidade" => self.nmp_profundidade = on,
            "contemptatento" => self.contempt_atento = on,
            "tmpawn" => self.tm_pawn = on,


            "semponte" => set_sem_ponte(on),



            // `SemAmeacas` e `SemPares` NAO sao opcoes de jogo e por isso
            // deixaram de estar aqui.
            //
            // As ameacas e os pares de peoes sao arquitectura da rede, nao
            // ideias em prova: nao ha' configuracao em que se queira jogar sem
            // eles. Enquanto estiveram neste `match` -- que e' um match livre,
            // alcancavel por `setoption` venha o nome de onde vier -- qualquer
            // interface ou arbitro podia partir a avaliacao com uma linha, e o
            // motor jogava na mesma sem se queixar. E' a mesma familia de
            // desfecho que a guarda da rede em falta existe para impedir.
            //
            // Continuam a existir como REGUA, so' por ambiente:
            // `KESTREL_SEM_AMEACAS=1` / `KESTREL_SEM_PARES=1`, que ninguem
            // define por acidente e que o motor anuncia alto quando ve'.


            "semhibrido" => crate::nnue_sf::set_sem_hibrido(on),
            "killerfresco" => self.killer_fresco = on,
            "tmcurva" => self.tm_curva = on,
            "tmrelogio" => self.tm_relogio = on,
            "lanceunico" => self.lance_unico = on,
            "tmadversario" => self.tm_adversario = on,
            "pontuatarde" => self.pontua_tarde = on,
            "ttfraco" => self.tt_fraco = on,
            "ttsemlmp" => self.tt_sem_lmp = on,
            "corrhist" => self.corr_hist = on,
            "razoring" => self.razoring = on,
            "rule50fade" => self.rule50_fade = on,
            "ttpvlmr" => self.ttpv_lmr = on,
            "iir" => self.iir = on,
            "iirnoall" => self.iir_no_all = on,
            "cuckoo" => self.cuckoo = on,
            "h2kotimismo" => self.otimismo = on,
            "h2khistpeao" => self.hist_peao = on,
            "h2khistpart" => self.hist_part = on,
            "rfpttcapt" => self.rfp_tt_capt = on,
            "tmpawncurva" => self.tm_pawn_curva = on,
            "ttpolitica" => { self.tt_politica = on; crate::tt::set_politica(on); }
            "lmpimproving" => self.lmp_improving = on,
            "qsfutility" => self.qs_futility = on,
            "tmnodeeffort" => self.tm_node_effort = on,
            "tmtrend" => self.tm_trend = on,
            "probcut" => self.probcut = on,
            "nmpcutnode" => self.nmp_cut_node = on,
            "rfpdamp" => self.rfp_damp = on,
            "checkext" => self.check_ext = on,
            "ttcutcredit" => self.tt_cut_credit = on,
            "capturehist" => self.capture_hist = on,
            "aspbaixa" => self.asp_baixa = on,
            "lmrfino" => self.lmr_fino = on,
            "lmrpiora" => self.lmr_piora = on,
            "lmrjanela" => self.lmr_janela = on,
            "lmrhistforte" => self.lmr_hist_forte = on,
            "ordemposicao" => self.ordem_posicao = on,
            "histpecacasa" => self.hist_pc = on,
            "travaoqs" => self.travao_qs = on,
            "killercompete" => self.killer_compete = on,
            "contlongo" => self.cont_longo = on,
            "semkillers" => self.sem_killers = on,
            "loglmr" => self.log_lmr = on,
            "cutnodelmr" => self.cut_node_lmr = on,
            "lmrttcaptura" => self.lmr_tt_captura = on,
            "historyprune" => self.history_prune = on,
            "tmstability" => self.tm_stability = on,
            "lmrcaptures" => self.lmr_captures = on,
            _ => return false,
        }
        true
    }

    /// The ones outside the settled set. All default off.
    pub const EXTRA: [&'static str; 52] = [
        "FutCapturas",
        "PodaReduzida",
        "PodaHist",
        "PodaSF",
        "HistContexto",
        "XequeNaOrdem",
        "OrdemContinua",
        "Triagem",
        "SingSoInferior",
        "GeraEtapas",
        "PickCpp",
        "ContAdversario",
        "RecusaRepeticao",
        "NmpProfundidade",
        "ContemptAtento",
        "TmHorizonte",
        "TmPawn",
        "SemPonte",
        "SemHibrido",
        "KillerFresco",
        "TmCurva",
        "TmRelogio",
        "LanceUnico",
        "TmAdversario",
        "PontuaTarde",
        "TtFraco",
        "TtSemLmp",
        "TmTrend",
        "CutNodeLmr",
        "LmrTtCaptura",
        "LmrCaptures",
        "Razoring",
        "Rule50Fade",
        "TtPvLmr",
        "LmpImproving",
        "QsFutility",
        "Probcut",
        "CheckExt",
        "TtCutCredit",
        "CaptureHist",
        "LogLmr",
        // Aqui e nao na linha de base: e' uma pergunta em aberto ate' mil
        // partidas dizerem alguma coisa.
        "AspBaixa",
        "LmrFino",
        "LmrPiora",
        "LmrJanela",
        "LmrHistForte",
        "OrdemPosicao",
        "HistPecaCasa",
        "TravaoQs",
        "KillerCompete",
        "ContLongo",
        "SemKillers",
    ];

    /// The ones it does have, so they are in the baseline. All default on.
    pub const BASELINE: [&'static str; 7] =
        ["HistoryPrune", "TmStability", "IIR", "TmNodeEffort",
     "CorrHist", "RfpDamp", "NmpCutNode"];
}

/// Os lances a que a medicao foi feita. Irregulares de proposito: a densidade
/// das partidas cai com o comprimento, e medir de dez em dez para la' do lance
/// 60 seria medir ruido.
const FALTAM_LANCE: [f64; 9] = [0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 80.0, 110.0];

/// Lances que FALTAM a quem joga, por percentil da duracao, medido em 2224
/// partidas nossas.
///
/// ORCAR PELA MEDIANA E' ORCAR PARA METADE DAS PARTIDAS REBENTAREM O ORCAMENTO,
/// e sao essas que acabam a zero. E' o defeito de desenho por inteiro, e a
/// unica maneira de lhe fugir e' escolher um percentil mais alto:
///
///     p50   rebentam 50% das partidas
///     p60   rebentam 40%
///     p70   rebentam 30%
///     p75   rebentam 25%
///     p80   rebentam 20%
///
/// Nao se gasta menos tempo no total -- distribui-se por um horizonte que esta'
/// certo para tres partidas em quatro em vez de uma em duas.
///
/// A p50 e' a que foi validada (+36,97 +/- 12,92) e e' a omissao. Subir daqui
/// passa pelo mesmo crivo.
const FALTAM_PCT: [[f64; 9]; 5] = [
    [65.5, 55.5, 45.5, 37.0, 29.5, 23.5, 19.0, 15.5, 14.0], // p50
    [71.5, 61.5, 51.5, 43.0, 35.0, 28.5, 23.5, 20.5, 20.5], // p60
    [78.0, 68.0, 58.5, 49.5, 41.5, 35.0, 30.5, 27.5, 27.0], // p70
    [82.0, 72.0, 62.5, 53.5, 45.5, 39.5, 34.5, 31.5, 30.5], // p75
    [87.0, 77.0, 67.0, 59.0, 51.0, 44.0, 39.5, 36.5, 37.5], // p80
];

/// Estimativa dos lances que faltam a quem joga, para `jogados` lances ja' feitos por cada lado.
///
/// Os dois ultimos nos -- lances 80 e 110 -- NAO estavam aqui, e a falta deles
/// era um defeito e nao uma simplificacao. A tabela ia so' ate' ao lance 60 e
/// seguia o declive do ultimo troco, que ao lance 80 dava DEZ lances quando a
/// medida diz quinze e meio, e ao lance 110 dava MENOS TRES. O `minimo` tapava
/// isso -- ou seja, o chao de seguranca estava a fazer o trabalho dos pontos
/// que faltavam, e era por isso que tinha de valer 14.
///
/// Com os dois nos no sitio, o `minimo` volta a ser o que diz que e'. E o erro
/// corrigia-se precisamente onde dói: nas partidas longas, que sao as que
/// acabam a` bandeira.
pub(crate) fn faltam_lances_pct(jogados: u32, minimo: i32, pct: i32, cresce: i32) -> u64 {
    let tab = &FALTAM_PCT[(pct.clamp(0, 4)) as usize];
    let x = jogados as f64;
    // Antes do primeiro no' e depois do ultimo, fica o valor da ponta: a
    // medicao nao diz nada para la' dela e inventar um declive foi o que
    // causou o problema.
    if x <= FALTAM_LANCE[0] {
        return (tab[0].round() as i64).max(minimo.max(1) as i64) as u64;
    }
    let ultimo = FALTAM_LANCE.len() - 1;
    if x >= FALTAM_LANCE[ultimo] {
        // DEPOIS DO FIM DA TABELA A ESTIMATIVA JA' FOI DESMENTIDA.
        //
        // Devolver o valor da ponta parece prudente -- a medicao nao diz nada
        // para la' dela -- e esta' errado na consequencia: dai' para a frente
        // dizia sempre o mesmo numero, fizesse a partida 120 lances ou 250.
        // Deixava de ser estimativa e voltava a ser constante, que e' o defeito
        // que a tabela veio corrigir. So' que escondido no ultimo degrau em vez
        // de em todos.
        //
        // Uma partida que passou o ultimo degrau PROVOU que e' das longas. A
        // partir dai' o horizonte cresce com o que ela ja' durou.
        //
        // 100 = um por um: cada lance de excesso acrescenta um lance ao que
        // falta. 0 = como estava.
        let base = tab[ultimo];
        let extra = if cresce > 0 {
            (x - FALTAM_LANCE[ultimo]) * cresce as f64 / 100.0
        } else {
            0.0
        };
        return ((base + extra).round() as i64).max(minimo.max(1) as i64) as u64;
    }
    let mut i = 0;
    while i + 1 < ultimo && x > FALTAM_LANCE[i + 1] {
        i += 1;
    }
    let (x0, x1) = (FALTAM_LANCE[i], FALTAM_LANCE[i + 1]);
    let v = tab[i] + (tab[i + 1] - tab[i]) * (x - x0) / (x1 - x0);
    (v.round() as i64).max(minimo.max(1) as i64) as u64
}

/// A tabela cuckoo, construida uma vez. Nao ha' estado por partida aqui.
static CUCKOO: std::sync::OnceLock<crate::cuckoo::Cuckoo> = std::sync::OnceLock::new();

#[derive(Default, Clone)]
pub struct Limits {
    pub wtime: Option<u64>,
    pub btime: Option<u64>,
    pub winc: u64,
    pub binc: u64,
    pub movestogo: Option<u64>,
    pub movetime: Option<u64>,
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
    pub infinite: bool,
}

pub struct Searcher {
    pub tt: std::sync::Arc<TranspositionTable>,
    pub atk: Attacks,
    pub stop: Arc<AtomicBool>,
    /// Milliseconds held back from every allocation to cover the time between
    /// deciding on a move and the move being seen by whoever is counting.
    ///
    /// Not a nicety. Measured over sixty games at 5+0.05 without it, thirty-one
    /// were lost on the clock; with it, none of twenty-eight were. The default
    /// is deliberately generous, because the cost of being wrong is asymmetric:
    /// too large loses a little strength, too small loses whole games.
    pub move_overhead: u64,
    /// Quantas buscas correm ao mesmo tempo. 1 = como sempre foi, e nesse caso
    /// nao nasce fio nenhum -- a arvore fica identica ao byte.
    pub threads: usize,
    pub features: Features,
    /// A pontuacao da ultima iteracao completa na raiz, do nosso lado. E' o
    /// unico sitio da busca que sabe se estamos a ganhar ou a perder.
    raiz_aval: i32,
    pub params: Params,
    pub(crate) lmr: [[i32; 64]; 64],

    pub(crate) nodes: u64,
    start: Instant,
    soft: Duration,
    hard: Duration,
    pub(crate) stopped: bool,

    killers: [[Option<Move>; NUM_KILLERS]; MAX_PLY],
    history: [[[[i32; 4]; 64]; 64]; 2],
    /// O mapa de ameacas, guardado POR PLY.
    ///
    /// Calcula-lo por lance seria dezasseis ciclos sobre as pecas vezes dez
    /// lances por no'. Dentro de um no' a posicao e' a mesma, portanto calcula-se
    /// uma vez e reaproveita-se.
    ///
    /// A primeira versao tinha um so' lugar, e isso duplicava o trabalho sem se
    /// dar por ela: o filho calculava o seu mapa e despejava o do pai, e quando
    /// o pai voltava para creditar o historico no corte tinha de o calcular
    /// outra vez. Dois mapas por no' em vez de um.
    ///
    /// Um lugar por ply resolve-o de forma exacta, porque a recursao e' em
    /// profundidade: o do pai so' e' preciso outra vez depois de todos os filhos
    /// voltarem, e nessa altura ninguem lhe mexeu. O indice sai de
    /// `self.keys.len()`, que ja' e' a profundidade -- a pilha das chaves e'
    /// empilhada antes de recursar e desempilhada ao voltar.
    cache_ameacas: Vec<std::cell::Cell<(u64, u64)>>,
    /// O que costuma resultar com esta peca nesta casa: [peca][para].
    ///
    /// A tabela principal e' de-para e nunca pode dizer isto: aprende que
    /// g3-f5 resulta, nao que um cavalo em f5 e' bom, porque cada casa de
    /// partida guarda a sua propria conta.
    histpc: [[i32; 64]; 6],
    /// `[side][pawn structure]`.
    /// `[kind][side][key]`.
    corr: std::sync::Arc<Vec<std::sync::atomic::AtomicI32>>,
    /// What was played at each ply, as (piece, destination). Continuation
    /// history is indexed by this: a move is good or bad largely in reply to
    /// something, and a table that ignores what came before cannot say which.
    played: [Option<(usize, usize)>; MAX_PLY],
    /// `[slot][prev piece * 64 + prev to][piece][to]`, one table per distance
    /// back.
    conthist: std::sync::Arc<Vec<std::sync::atomic::AtomicI32>>,
    /// [chave de peoes][peca COM COR][casa]. Doze pecas e nao seis: a cor
    /// conta, porque um peao branco em e4 e um preto em e5 nao dizem a mesma
    /// coisa sobre a estrutura.
    ///
    /// O KestrelStrike tem-na (`hist_peao`, `KS_PEAO_F`, omissao 32 e LIGADA na
    /// versao que o bot joga) e partilha-a entre fios, como as continuacoes e a
    /// correccao. Aqui e' igual.
    histpeao: std::sync::Arc<Vec<std::sync::atomic::AtomicI32>>,
    /// Estas duas tabelas sao minhas, ou sao emprestadas de quem me lancou?
    /// So' quem e' dono as limpa -- uma ajudante a limpar apagava a cada busca
    /// tudo o que a principal aprendeu.
    hist_proprio: bool,
    /// `[moving piece][destination][captured piece]`.
    ///
    /// What a capture takes is known before it is played; whether taking it
    /// works is not. Most valuable victim answers the first question and calls
    /// it the second -- so a queen recapture that always loses to a pin keeps
    /// being tried first, forever, because the queen is still the biggest piece
    /// on the square.
    capthist: Vec<[[i32; 6]; 64]>,
    /// Zobrist keys along the path plus the game so far, for repetition.
    pub(crate) keys: Vec<u64>,
    /// How many of `keys` are game history rather than search path.
    root_keys: usize,

    pub(crate) pv: [[Option<Move>; MAX_PLY]; MAX_PLY],
    pub(crate) pv_len: [usize; MAX_PLY],
    /// O maior relogio visto nesta partida, que e' o do primeiro lance. Serve
    /// de referencia para saber que fraccao ainda la' esta'.
    pub(crate) relogio_maximo: u64,
    /// O relogio dele no nosso lance anterior, para se lhe medir o ritmo.
    pub(crate) relogio_dele: u64,
    /// Quanto ele gastou no ultimo lance.
    pub(crate) ritmo_dele: u64,
    /// The static score at each ply, so a node can ask whether things have
    /// been getting better for the side to move. A position that is improving
    /// deserves a tighter margin than one that is falling apart, because the
    /// reason to prune is confidence and there is less of it on the way down.
    pub(crate) eval_stack: [i32; MAX_PLY],
    /// A move this ply is pretending does not exist, while it finds out
    /// whether that move was the only one holding the position up.
    pub(crate) excluded: [Option<Move>; MAX_PLY],
    /// The move played at each ply, for the transcribed search's continuation
    /// tables, which index by a move rather than by a piece and square.
    pub(crate) played_moves: Vec<Option<Move>>,
    /// The transcribed search keeps its own tables: same shapes and ceilings as
    /// the transcribed search, which are not the shapes the other one uses.
    /// The last few moves of the game, for the plies above the root.
    pub(crate) pre_moves: [Option<Move>; PRE_MOVES],
    /// How many nodes each root move cost this iteration. A move that took
    /// most of the tree and still came out best was not a close call, and time
    /// management can read that.
    root_effort: Vec<(Move, u64)>,
    /// How often the tables answered instead of the search.
    pub tb_hits: u64,
    /// A largura da janela na raiz. Sem isto, a largura de um no' nao tem com
    /// o que ser comparada -- `delta` vivia dentro do ciclo da aspiracao e
    /// morria la'.
    pub(crate) root_delta: i32,
    /// Every root move with what this iteration thought of it.
    ///
    /// Keeping only the best one leaves nothing to fall back on when the best
    /// one repeats: there is no second opinion, only a move and no reason to
    /// prefer anything else. With the whole list scored, a repetition can be
    /// declined in favour of something that is nearly as good, and how much
    /// worse we are willing to accept is a number rather than an accident.
    root_scores: Vec<(Move, i32)>,
    /// Which plies got there by passing. Two passes in a row prove nothing:
    /// the side to move has effectively been given a free tempo twice, and the
    /// position being searched is not one that can occur.
    pub(crate) null_at: [bool; MAX_PLY],
    /// Quantas vezes cada ply ja' cortou por beta nesta busca.
    ///
    /// Vem do KestrelStrike, recuperado a 20-09. Um no' cujos filhos nao param
    /// de cortar e' um no' facil, e reduz-se mais la'. Nos nao tinhamos nem o
    /// contador nem o termo.
    pub(crate) cut_cnt: [i32; MAX_PLY + 8],
}

/// The score of a position from the side to move's point of view.
/// Material left on the board, from White, in the units the network speaks.
///
/// The endgame knowledge asks who has the material before it asks anything
/// else, and a network answers with a position rather than a count. This is the
/// count.
fn material_white(board: &Board) -> i32 {
    let mut v = 0;
    for pt in [
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
    ] {
        let val = value_in_eval_units(pt);
        v += val * board.pieces[Color::White.idx()][pt.idx()].count_ones() as i32;
        v -= val * board.pieces[Color::Black.idx()][pt.idx()].count_ones() as i32;
    }
    v
}

/// Avaliar sem a ponte importada, com o nosso leitor em Rust.
///
/// `H2K_SEM_PONTE=1` ou `setoption name SemPonte value true`.
static ATTACKS: std::sync::OnceLock<crate::attacks::Attacks> = std::sync::OnceLock::new();
/// Um so' `Attacks` para todo o processo: construi-lo por avaliacao custaria
/// mais do que a avaliacao.
pub fn atk() -> &'static crate::attacks::Attacks {
    ATTACKS.get_or_init(crate::attacks::Attacks::new)
}

pub fn sem_ponte() -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    if SEM_PONTE.load(Relaxed) {
        return true;
    }
    static ENV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENV.get_or_init(|| std::env::var_os("H2K_SEM_PONTE").is_some())
}
pub static SEM_PONTE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub fn set_sem_ponte(v: bool) {
    SEM_PONTE.store(v, std::sync::atomic::Ordering::Relaxed);
}

/// `H2K_ORC=1` faz o motor imprimir o orcamento de cada lance.
pub fn tempo_debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("H2K_ORC").is_some())
}

fn evaluate(board: &mut Board, fade: bool) -> i32 {
    // A rede do outro motor, quando pedida. O conhecimento de finais e o
    // amortecimento por cima continuam a ser nossos -- e' so' o valor cru que
    // muda de origem.
    // `SemPonte`: avalia com o NOSSO leitor em Rust em vez do importado.
    //
    // Faltava-nos a manete mais importante de todas. Cem manetes cobrem a
    // busca, mas a avaliacao inteira -- os 70 ficheiros vendorizados e a ponte
    // que conduz o tabuleiro -- entrava sempre, sem forma de a desligar.
    //
    // Isto da' tres respostas de uma vez. Se o bloqueio de 14 segundos
    // desaparecer, esta' provado que e' da ponte e nao da busca, sem depurador.
    // Se a forca nao mudar muito, a importacao nao paga a complicacao que
    // custa. E da' a base a que o Triumviratus chama "byte-identico": com ela
    // ligada, nada de importado entra na avaliacao, e qualquer diferenca
    // medida dai' para a frente e' da importacao e de mais nada.
    //
    // Os dois caminhos leem a MESMA rede e ja' se provou que concordam:
    // declive 1,177 (que e' o `factor` de 0,85 invertido) com R2 de 0,999999 e
    // residuo de 0,57 centipeoes em 80 posicoes.
    if !sem_ponte() && crate::ponte::ligado() {
        if let Some(v) = crate::ponte::avalia(&|| board.to_fen()) {
            let mut raw = v;
            if fade {
                raw = raw * (200 - board.halfmove.min(100) as i32) / 200;
            }
            return raw;
        }
        // A ponte esta' ligada e NAO respondeu. Nao se cai daqui em silencio.
        //
        // O contexto da ponte e' `thread_local` e nasce UMA vez por thread: se
        // uma thread lhe tocar antes de a rede estar carregada, `Ponte::nova()`
        // devolve `None` e essa thread fica sem ponte para sempre. A cascata
        // abaixo apanhava-a e seguia para o `nnue.rs`, que e' o NOSSO leitor e
        // nao tem ameacas nenhumas -- ou, sem rede la', devolvia zero.
        //
        // Jogar com um avaliador diferente do que se pensa nao pode ser uma
        // coisa que aconteca calada. Diz-se alto, nos dois canais, e conta-se.
        FALHAS_PONTE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        static AVISADO: std::sync::Once = std::sync::Once::new();
        AVISADO.call_once(|| {
            let m = "ERRO: a ponte esta' ligada mas nao respondeu -- esta thread \
                     ficou sem contexto e a avaliacao ia cair para o leitor \
                     proprio, que NAO tem ameacas. Os resultados desta sessao \
                     nao tem significado.";
            println!("info string {m}");
            eprintln!("{m}");
        });
    }
    if sem_ponte() {
        if let Some(net) = crate::nnue_sf::rede() {
            let mut raw = crate::nnue_sf::evaluate(net, atk(), board);
            if fade {
                raw = raw * (200 - board.halfmove.min(100) as i32) / 200;
            }
            return raw;
        }
    }
    let net = match nnue::net() {
        Some(n) => n,
        None => return 0,
    };
    // AMEACAS, por diferenca.
    //
    // A versao anterior reconstruia TUDO a cada no' (`Accumulator::fresh`), o
    // que estava correcto e custava caro: 103k nps contra 352k. O bloco tem
    // ~192 features activas e so' ~9,8 mudam por lance, portanto aplicar o
    // delta em vez de tudo corta a aplicacao 19x.
    //
    // As pecas e os pares continuam a vir do acumulador incremental do motor
    // (`board.acc`), que ja' estava certo; as ameacas vivem num acumulador
    // proprio e sao somadas aqui.
    let acc = match board.acc.as_ref() {
        Some(a) => a,
        None => return 0,
    };
    let mut raw = acc.eval(net, board.side, board.occ_all.count_ones());

    // Endgame knowledge, where counting material is simply wrong.
    //
    // A network trained on positions is confident about endings it has barely
    // seen, and confidently wrong in a particular way: it scores two knights
    // against a bare king as an advantage, when that position cannot be won at
    // all, and it scores a rook against a bare king as an advantage of the same
    // size, when that one is a forced mate. An engine that cannot tell those
    // apart will trade into the draw and decline the win.
    //
    // Two kinds of answer, because endings need two. Some positions have a
    // known value and the evaluation should be replaced rather than nudged --
    // a theoretical draw is worth nothing whatever the material says, and a won
    // ending is about progress rather than material: driving the defending king
    // to the edge, and to the right corner. Others are right about who is
    // better and wrong about whether it can be converted, and those are scaled.
    if board.occ_all.count_ones() <= 7 {
        let mat = material_white(board);
        let q_minus_p = 0;
        if let Some((strong, verdict)) = crate::endgame::probe(board, mat, q_minus_p) {
            let mut v = match verdict {
                crate::endgame::Verdict::Exact(x) => x,
                crate::endgame::Verdict::Scale(s) => {
                    let base = if strong == Color::White { raw } else { -raw };
                    let base = if board.side == Color::White { base } else { -base };
                    base * s / crate::endgame::SCALE_NORMAL
                }
            };
            // The module speaks for the strong side; the search wants the side
            // to move.
            if strong != board.side {
                v = -v;
            }
            return v;
        }
        // Nothing known, but a decisive material edge with the loser's king
        // still running: give the search a reason to walk it to the edge, which
        // is the one thing the material count cannot say.
        let drive = crate::endgame::conversion_drive(board, mat);
        raw += if board.side == Color::White { drive } else { -drive };
    }

    // Fade towards a draw as the fifty move counter runs out. A network trained
    // on positions is confident about a position that is about to stop counting
    // for anything, and without this the search happily walks into a draw it
    // thinks it is winning.
    if fade {
        raw * (200 - board.halfmove.min(100) as i32) / 200
    } else {
        raw
    }
}

/// A piece value in the units the network speaks.
///
/// The table that travels with the board is in ordinary centipawns, where a
/// pawn is 100. This network answers on a scale with two units to the
/// centipawn, so anything that compares a piece against an evaluation has to
/// convert. Not converting made the quiescence margin twice as harsh as
/// intended -- the same class of mistake that made history pruning never fire
/// at all, in the other direction. Static exchange is exempt: it is centipawns
/// end to end, input and output, so a threshold handed to it belongs in
/// centipawns too.
#[inline]
fn value_in_eval_units(pt: PieceType) -> i32 {
    pt.value() * 2
}

/// Does this side have anything but pawns and a king?
///
/// The question null move pruning asks: with only pawns left, having to move is
/// often a disadvantage, so a side that passes and still looks fine proves
/// nothing about a side that has to play.
pub(crate) fn has_pieces_pub(board: &Board, side: Color) -> bool {
    has_pieces(board, side)
}

fn has_pieces(board: &Board, side: Color) -> bool {
    let p = &board.pieces[side.idx()];
    p[PieceType::Knight.idx()]
        | p[PieceType::Bishop.idx()]
        | p[PieceType::Rook.idx()]
        | p[PieceType::Queen.idx()]
        != 0
}

/// The evaluation, exposed for the UCI `eval` command.
pub fn debug_eval(board: &mut Board, fade: bool) -> i32 {
    // A ponte conduz um tabuleiro a par do nosso, e quem o move e' a busca.
    // Fora dela ela fica onde a ultima procura a deixou, por isso um pedido de
    // avaliacao respondia sempre sobre essa posicao e nao sobre a que lhe
    // davam: devolvia o mesmo numero na posicao inicial, com as pretas sem dama
    // e num final de dama contra rei nu.
    //
    // Um diagnostico que devolve uma constante plausivel e' pior do que um que
    // falha: quem o usa para conferir duas versoes ve' numeros iguais e conclui
    // que estao de acordo. E' a mesma familia do motor que jogava com avaliacao
    // zero sem se queixar.
    if crate::ponte::ligado() {
        crate::ponte::raiz(&board.to_fen());
    }
    evaluate(board, fade)
}

fn mate_score(ply: usize) -> i32 {
    -MATE + ply as i32
}

pub fn is_mate(score: i32) -> bool {
    score.abs() >= MATE_IN_MAX
}

/// Moving a mate score in and out of the table: stored relative to the node it
/// was found at, used relative to the root. Without this a mate found deep in
/// one branch is reported as being that many moves away from wherever the entry
/// is read next.
pub(crate) fn score_to_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE_IN_MAX {
        score + ply as i32
    } else if score <= -MATE_IN_MAX {
        score - ply as i32
    } else {
        score
    }
}

pub(crate) fn score_from_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE_IN_MAX {
        score - ply as i32
    } else if score <= -MATE_IN_MAX {
        score + ply as i32
    } else {
        score
    }
}

impl Searcher {
    pub fn new(hash_mb: usize, stop: Arc<AtomicBool>) -> Self {
        Searcher {
            tt: std::sync::Arc::new(TranspositionTable::new(hash_mb)),
            atk: Attacks::new(),
            stop,
            move_overhead: 30,
            threads: 1,
            features: Features::default(),
            raiz_aval: 0,
            params: Params::default(),
            lmr: build_lmr_table(Params::default().lmr_base, Params::default().lmr_div),
            nodes: 0,
            start: Instant::now(),
            soft: Duration::from_secs(0),
            hard: Duration::from_secs(0),
            stopped: false,
            killers: [[None; NUM_KILLERS]; MAX_PLY],
            history: [[[[0; 4]; 64]; 64]; 2],
            cache_ameacas: (0..MAX_PLY + 8).map(|_| std::cell::Cell::new((0, 0))).collect(),
            histpc: [[0; 64]; 6],
            corr: std::sync::Arc::new(cria_hist(CORR_KINDS * 2 * CORR_SIZE)),
            played: [None; MAX_PLY],
            conthist: std::sync::Arc::new(cria_hist(CONT_SLOTS * 6 * 64 * 6 * 64)),
            histpeao: std::sync::Arc::new(cria_hist(TAM_PEAO * 12 * 64)),
            hist_proprio: true,
            capthist: vec![[[0; 6]; 64]; 6],
            keys: Vec::with_capacity(1024),
            root_keys: 0,
            pv: [[None; MAX_PLY]; MAX_PLY],
            pv_len: [0; MAX_PLY],
            relogio_maximo: 0,
            relogio_dele: 0,
            ritmo_dele: 0,
            eval_stack: [0; MAX_PLY],
            excluded: [None; MAX_PLY],
            played_moves: vec![None; MAX_PLY],
            pre_moves: [None; PRE_MOVES],
            root_effort: Vec::with_capacity(256),
            tb_hits: 0,
            root_delta: 1,
            root_scores: Vec::with_capacity(256),
            null_at: [false; MAX_PLY],
            cut_cnt: [0; MAX_PLY + 8],
        }
    }

    /// Uma busca irma: partilha a TABELA e o sinal de paragem, e mais nada.
    ///
    /// Os historicos ficam de fora de proposito, para ja'. E' a diferenca que a
    /// sessao de 18-09 isolou ao ler o SF19: eles partilham tres familias
    /// inteiras -- continuacao, peoes e correccao -- e nos so' partilhamos a
    /// tabela. Medido la': a quatro fios fazemos 3,17x os nos de um, mas
    /// chegamos ao ply 21 onde eles chegam ao 25. "Os nos estao la'; a
    /// aprendizagem e' que nao circula." Isso e' o passo seguinte e mede-se
    /// contra ISTO, senao nao se sabe o que cada metade rendeu.
    fn ajudante(&self) -> Searcher {
        let mut a = Searcher::new(1, std::sync::Arc::clone(&self.stop));
        a.tt = std::sync::Arc::clone(&self.tt);
        a.features = self.features.clone();
        a.params = self.params.clone();
        a.move_overhead = self.move_overhead;
        a.threads = 1;
        if self.features.hist_part {
            // As duas familias que o half2k tem. A terceira -- historia de
            // peoes -- nao existe aqui e fica na lista do que falta.
            a.conthist = std::sync::Arc::clone(&self.conthist);
            a.histpeao = std::sync::Arc::clone(&self.histpeao);
            a.corr = std::sync::Arc::clone(&self.corr);
            a.hist_proprio = false;
        }
        a.params_changed();
        a
    }

    /// O laco de um ajudante: as mesmas profundidades, a mesma raiz, sem
    /// imprimir nada e sem relogio proprio.
    ///
    /// Sem desvio de profundidade de proposito. A sessao de 18-09 explica
    /// porque': com fios em profundidades DIFERENTES a votacao pesa por
    /// profundidade e "uma thread que saltou para um numero alto por um caminho
    /// raso ganha peso exactamente por isso". Todos percorrem as mesmas
    /// profundidades e divergem no caminho, que e' o que se quer.
    fn corre_ajudante(&mut self, board: &mut Board, max_depth: u32) -> Option<(Move, i32, u32)> {
        // A RAIZ, neste fio. O contexto da ponte e' `thread_local` e nasce
        // vazio em cada fio novo: sem isto o ajudante avalia sobre um tabuleiro
        // por inicializar, e o motor pendura logo a seguir a` profundidade 1.
        // E' o mesmo aviso que a sessao de 18-09 deixou escrito -- "uma thread
        // que lhe toque antes de a rede estar carregada fica sem ponte para
        // sempre".
        if crate::ponte::ligado() {
            crate::ponte::raiz(&board.to_fen());
        }
        // So' o `stop` partilhado o para: o relogio e' do fio principal.
        self.hard = Duration::from_secs(86_400);
        self.soft = Duration::from_secs(86_400);
        self.start = Instant::now();
        let mut melhor: Option<(Move, i32, u32)> = None;
        let mut anterior = 0;
        let mut media: Option<i32> = None;
        for depth in 1..=max_depth {
            self.root_scores.clear();
            let nota = self.aspiration(board, depth as i32, anterior);
            if self.stopped {
                break;
            }
            anterior = nota;
            if self.features.otimismo {
                let m = match media { None => nota, Some(a) => (nota + a) / 2 };
                media = Some(m);
                crate::ponte::otimismo(self.params.otimismo_f * m / (m.abs() + 85));
            }
            if let Some(&(m, sc)) = self.root_scores.iter().max_by_key(|(_, sc)| *sc) {
                // Com a profundidade a que ESTE resultado foi obtido. Sem ela,
                // um ajudante que o relogio apanhou na 12 vota com o mesmo peso
                // que o principal na 17 -- e pode derruba-lo.
                melhor = Some((m, sc, depth));
            }
        }
        melhor
    }

    /// O LANCE mais votado, nao a busca com a nota mais alta.
    ///
    /// Peso `nota - menor_nota + VOTO_PESO`. Uma busca muito abaixo das outras
    /// vota pouco; uma a` frente vota muito. O peso 24 e' o do KestrelStrike e
    /// transfere-se sem conversao -- os dois motores tem a mesma escala interna
    /// (o dobro do centipeao: ambos imprimem `cp` a dividir por dois).
    fn vota(cand: &[(Move, i32)]) -> Option<Move> {
        const VOTO_PESO: i64 = 24;
        let menor = cand.iter().map(|(_, s)| *s).min()?;
        let mut votos: Vec<(Move, i64)> = Vec::with_capacity(cand.len());
        for &(m, sc) in cand {
            let peso = (sc as i64 - menor as i64) + VOTO_PESO;
            match votos.iter_mut().find(|(vm, _)| *vm == m) {
                Some((_, v)) => *v += peso,
                None => votos.push((m, peso)),
            }
        }
        votos.into_iter().max_by_key(|(_, v)| *v).map(|(m, _)| m)
    }

    /// Call after changing any parameter, so anything derived from one is
    /// rebuilt rather than left describing the old value.
    pub fn params_changed(&mut self) {
        self.lmr = build_lmr_table(self.params.lmr_base, self.params.lmr_div);
    }

    pub fn set_game_history(&mut self, keys: Vec<u64>) {
        self.keys = keys;
        self.root_keys = self.keys.len();
    }

    /// The moves actually played before this search started.
    ///
    /// Continuation history asks "what is a good reply to what just happened",
    /// and at the top of the tree what just happened is in the GAME, not in the
    /// search. Without this the tables are empty for the first plies of every
    /// search -- which is where most of the tree is -- and the ordering there
    /// runs on the butterfly table alone.
    ///
    /// Measured against the program this search was transcribed from: its
    /// average history score per reduced move was -7757 against ours at -1219,
    /// six times less opinionated, and this was why.
    ///
    /// Stored in the slots below zero, so that `ply - 1` at the root reaches
    /// the last move of the game rather than nothing.
    pub fn set_game_moves(&mut self, moves: &[Move]) {
        self.pre_moves = [None; PRE_MOVES];
        for (i, mv) in moves.iter().rev().take(PRE_MOVES).enumerate() {
            self.pre_moves[i] = Some(*mv);
        }
    }

    /// The move `back` plies before `ply`, reaching into the game when the
    /// search runs out.
    #[inline]
    pub(crate) fn move_back(&self, ply: usize, back: usize) -> Option<Move> {
        if ply >= back {
            self.played_moves[ply - back]
        } else {
            self.pre_moves.get(back - ply - 1).copied().flatten()
        }
    }

    pub fn clear(&mut self) {
        self.tt.clear();
        self.killers = [[None; NUM_KILLERS]; MAX_PLY];
        self.history = [[[[0; 4]; 64]; 64]; 2];
        self.histpc = [[0; 64]; 6];
        // So' o DONO limpa. Ver a nota no campo `hist_proprio`.
        if self.hist_proprio {
            for e in self.corr.iter() {
                e.store(0, Ordering::Relaxed);
            }
            for e in self.conthist.iter() {
                e.store(0, Ordering::Relaxed);
            }
            for e in self.histpeao.iter() {
                e.store(0, Ordering::Relaxed);
            }
        }
        self.capthist = vec![[[0; 6]; 64]; 6];
    }

    /// What the last move changed about the position key.
    ///
    /// The key stack holds every position back to the start of the game, with
    /// the current one on top, so the difference between the top two is the
    /// move that was just made -- from, to, what it took and whose it was, all
    /// in one number. Zero when there is nothing above.
    #[inline]
    fn hash_delta(&self) -> u64 {
        let n = self.keys.len();
        if n >= 2 {
            self.keys[n - 1] ^ self.keys[n - 2]
        } else {
            0
        }
    }

    /// The static score, adjusted by what the search has been saying about
    /// positions with this pawn structure.
    #[inline]
    fn corrected(&self, board: &Board, raw: i32, ply: usize) -> i32 {
        let last = if ply > 0 { self.played[ply - 1] } else { None };
        let idx = corr_indices(board, last, self.hash_delta());
        let side = board.side.idx();
        let mut total = 0i32;
        for k in 0..CORR_KINDS {
            total += self.corr[icorr(k, side, idx[k])].load(Ordering::Relaxed) * CORR_WEIGHT[k];
        }
        let c = total / CORR_DIVISOR;
        (raw + c).clamp(-MATE_IN_MAX + 1, MATE_IN_MAX - 1)
    }

    /// Learn from the difference, weighted by how deep the search that found
    /// it went.
    /// Move each reading towards what the search actually said.
    ///
    /// The amount is the miss scaled by how deep the search that found it went,
    /// capped, and then applied so that an entry approaches its ceiling instead
    /// of slamming into it. The previous version multiplied the miss by 256
    /// before capping, so any miss above thirty two units saturated the target
    /// and every update looked the same size.
    #[inline]
    fn learn_correction(&mut self, board: &Board, diff: i32, depth: i32, ply: usize) {
        let last = if ply > 0 { self.played[ply - 1] } else { None };
        let idx = corr_indices(board, last, self.hash_delta());
        let side = board.side.idx();
        let bonus = (diff * depth / 8).clamp(-CORR_MAX_UPDATE, CORR_MAX_UPDATE);
        for k in 0..CORR_KINDS {
            let e = &self.corr[icorr(k, side, idx[k])];
            let v = e.load(Ordering::Relaxed);
            let v = v + bonus - v * bonus.abs() / CORR_MAX;
            e.store(v.clamp(-CORR_MAX, CORR_MAX), Ordering::Relaxed);
        }
    }

    /// Which continuation table each slot points at, this ply.
    ///
    /// One, two and four plies back. The first is the move being replied to and
    /// carries twice the weight of the others: what makes a quiet move good is
    /// most often what the opponent just did, and only after that what we were
    /// doing before.
    #[inline]
    fn cont_slots(&self, ply: usize) -> [Option<usize>; CONT_SLOTS] {
        let mut out = [None; CONT_SLOTS];
        let tabela: &[usize; CONT_SLOTS] = if self.features.cont_adversario {
            &CONT_BACK_ADV
        } else {
            &CONT_BACK
        };
        for (k, back) in tabela.iter().enumerate() {
            if ply >= *back {
                if let Some((pc, to)) = self.played[ply - back] {
                    out[k] = Some(pc * 64 + to);
                }
            } else if let Some(mv) = self.pre_moves.get(back - ply - 1).copied().flatten() {
                // Above the root: the piece is unknown here, so the move itself
                // stands in for it. Consistent within the table, which is all
                // an index has to be.
                out[k] = Some((mv.from as usize % 6) * 64 + mv.to as usize);
            }
        }
        out
    }

    /// Decide how long this move may take.
    ///
    /// Two limits, because they answer different questions. `soft` is checked
    /// only between iterations: passing it means there is not enough left to
    /// make another depth worthwhile, and the move we have is the move we play.
    /// `hard` is checked inside the search and is a wall -- crossing it means
    /// abandoning the iteration in progress and using the last completed one.
    ///
    /// Everything is taken from the clock AFTER the overhead is removed, and
    /// `hard` is capped so that even the wall cannot spend what we do not have.
    fn allocate(&mut self, limits: &Limits, board: &Board) {
        let side = board.side;
        self.start = Instant::now();

        if limits.infinite || limits.depth.is_some() || limits.nodes.is_some() {
            self.soft = Duration::from_secs(86_400);
            self.hard = Duration::from_secs(86_400);
            return;
        }

        if let Some(mt) = limits.movetime {
            let usable = mt.saturating_sub(self.move_overhead).max(1);
            self.soft = Duration::from_millis(usable);
            self.hard = Duration::from_millis(usable);
            return;
        }

        let (time, inc, opp_time) = match side {
            Color::White => (limits.wtime, limits.winc, limits.btime),
            Color::Black => (limits.btime, limits.binc, limits.wtime),
        };

        // A gestao de tempo do pawn, portada tal e qual.
        //
        // Sao cinco linhas e nao precisa de ser afinada por controlo, ao
        // contrario da nossa. Duas diferencas fazem-na funcionar:
        //
        //   * o incremento entra no BOLO -- `relogio + inc*(n-1)` -- em vez de
        //     ser somado uma vez a` parte. A nossa faz `relogio/mtg + inc`, que
        //     conta UM incremento: a 120+1 orca 5,4 s e recebe 1 s, e a
        //     diferenca sai do capital ate' a bandeira cair.
        //
        //   * o tecto e' `min(80% do relogio, 2x o optimo)`. Simples e
        //     impossivel de furar. O nosso e' o minimo de tres termos com um
        //     multiplicador de emergencia que a 5+0.05 esta' presto no piso e a
        //     120+1 esta' solto no tecto -- o mesmo parametro partido pelas
        //     duas pontas.
        //
        // Aplicado a 120+1: bolo 149000, optimo 4867 ms, tecto 9634 ms.
        // Sustentavel, e nunca perde por bandeira.
        //
        // Entra DESLIGADO. Ver `TmPawn`.
        if self.features.tm_pawn {
            if let Some(t) = time {
                let i = inc;
                // Sem movestogo -- morte subita, que e' o caso no Lichess -- o
                // pawn reparte por 30. Isso e' um decaimento geometrico: gasta-se
                // sempre 1/30 do que RESTA, portanto o relogio nunca se acaba e
                // sobra sempre. Medido no bot a 1+0: mediana de 12,4 s por usar
                // em 60, com 0,57 s por lance. Para um motor que ganha 239 Elo
                // ao passar de 1x para 4x o tempo, devolver um quinto do relogio
                // e' das piores maneiras de perder forca. Fica em parametro para
                // se poder medir onde e' o joelho.
                let nn = if self.features.tm_pawn_curva && limits.movestogo.is_none() {
                    // `fullmove` comeca em 1; a curva e' por lances JA' JOGADOS.
                    faltam_lances_pct(
                        board.fullmove.saturating_sub(1),
                        self.params.tm_pawn_curva_min,
                        self.params.tm_curva_pct,
                        self.params.tm_cresce,
                    )
                } else {
                    self.params.tm_pawn_n.max(1) as u64
                };
                let n = match limits.movestogo {
                    Some(m) => m.max(1).min(nn),
                    None => nn,
                };
                let bolo = t + i * n.saturating_sub(1);
                let optimo = (bolo / n).saturating_sub(self.move_overhead).max(1);
                let tecto = (8 * t / 10)
                    .min(optimo * self.params.tm_pawn_tecto.max(100) as u64 / 100)
                    .saturating_sub(self.move_overhead)
                    .max(1);
                let tecto = self.aperta_sem_inc(tecto, t, i);
                self.soft = Duration::from_millis(optimo);
                self.hard = Duration::from_millis(tecto.max(optimo.min(tecto)));
                return;
            }
        }
        let time = match time {
            Some(t) => t,
            None => {
                self.soft = Duration::from_secs(86_400);
                self.hard = Duration::from_secs(86_400);
                return;
            }
        };

        // What is actually ours to spend. `saturating_sub` and the floor of one
        // millisecond matter: in time trouble the clock can be below the
        // overhead, and an allocation of zero would still have to make a move,
        // just without having thought about it.
        let usable = time.saturating_sub(self.move_overhead).max(1);

        // How many more moves to plan for. With a real count given, use it.
        //
        // Without one this is a guess, and the guess decides how much of the
        // clock is ever spent: dividing what REMAINS by n means spending 1/n
        // and keeping the rest, every move, so after k moves ((n-1)/n)^k of the
        // clock is still there. It was 46, and our games run 81 moves a side --
        // (45/46)^81 is 0.17, and 32 games at 40+0.4 measured 0.15 left over,
        // 10.6 seconds a game never used. At 27, (26/27)^81 is 0.05.
        //
        // Pessimism here is not free: it is a permanent tax on every move, paid
        // to insure against flagging in games that run long.
        if self.features.tm_relogio {
            self.relogio_maximo = self.relogio_maximo.max(time);
        }
        let mtg = match limits.movestogo {
            Some(n) => n.max(1),
            None if self.features.tm_relogio && self.relogio_maximo > 0 => {
                // A fraccao do relogio que ainda la' esta'. Cheio quer dizer
                // muito jogo pela frente e gasta-se com conta; a acabar quer
                // dizer repartir o que resta por poucos lances.
                let frac = (time * 1000 / self.relogio_maximo).min(1000) as i32;
                let base = self.params.tm_mtg_base;
                let min = self.params.tm_mtg_min;
                (min + (base - min) * frac / 1000).max(min) as u64
            }
            None if self.features.tm_curva => {
                // Lances por jogar sao muitos no inicio e poucos no fim; uma
                // constante trata as duas pontas por igual. A curva desce com
                // a partida, que e' o que evita gastar de mais na abertura.
                let lance = board.fullmove as i32;
                let est = self.params.tm_mtg_base - self.params.tm_mtg_declive * lance / 100;
                est.max(self.params.tm_mtg_min) as u64
            }
            None if self.features.tm_horizonte => {
                // O horizonte encolhe com o relogio: com pouco tempo nao ha'
                // margem para planear longe, e planear longe com pouco tempo e'
                // exactamente como se perde por bandeira.
                if time < 1000 {
                    (time / 20).max(2)
                } else {
                    self.params.tm_horizonte_max as u64
                }
            }
            None => self.params.tm_mtg as u64,
        }
        .max(1);

        // The increment is income, so most of it can be spent every move
        // without the clock moving. Not all of it: the part held back is what
        // slowly rebuilds a buffer over a long game.
        // O incremento no BOLO em vez de parcela a` parte. Somado por fora, ele
        // entra inteiro em todos os lances, incluindo aqueles em que o relogio
        // ja' nao o comporta; dentro do bolo, e' o rendimento dos lances que
        // faltam a ser repartido por eles.
        let mut base = if self.features.tm_horizonte {
            // O recurso inteiro: o que esta' no relogio, mais os incrementos que
            // ainda se vao receber dentro do horizonte, menos a sobrecarga de
            // CADA lance futuro. Descontava-se a sobrecarga uma vez so' e ela
            // paga-se em todos -- num horizonte de 27 isso e' quase um segundo
            // de optimismo, e e' o que fica por pagar quando a bandeira cai.
            (time + inc * mtg.saturating_sub(1))
                .saturating_sub(self.move_overhead * (2 + mtg))
                .max(1)
                / mtg
        } else if self.features.tm_curva || self.features.tm_relogio {
            (usable + inc * mtg.saturating_sub(1)) / mtg
        } else {
            usable / mtg + inc * self.params.tm_inc_pct as u64 / 100
        };

        // What the position is worth spending on, by where the game is.
        //
        // The opening is played rather than calculated: the answer is either
        // known or is one of several equally playable moves, and a quarter of
        // a clock can disappear before the game has started. A simplified
        // position has less left to find. The middle is where thinking pays,
        // so that is where the money goes.
        let ply = (board.fullmove as i32 - 1) * 2 + (side == Color::Black) as i32;
        let pieces = board.occ_all.count_ones() as i32;
        let phase = if ply < 12 {
            self.params.tm_open_pct
        } else if ply < 24 {
            self.params.tm_early_pct
        } else if ply < 45 {
            self.params.tm_mid_pct
        } else if ply < 65 {
            self.params.tm_late_pct
        } else if pieces <= 10 {
            self.params.tm_simple_pct
        } else {
            100
        };
        base = base * phase as u64 / 100;

        // And by the other clock, which is half of the game.
        //
        // A comfortable lead on the clock is an asset to spend; being behind on
        // it is a reason not to, because the opponent can simply keep playing
        // and let the difference do the work. Overall health rather than the
        // pace of any one move.
        if let Some(opp) = opp_time.filter(|t| *t > 0) {
            let ratio = time * 10 / opp;
            let pressure = if ratio > 15 {
                self.params.tm_ahead_pct
            } else if ratio < 7 {
                self.params.tm_behind_pct
            } else {
                100
            };
            base = base * pressure as u64 / 100;
        }

        // Two ceilings on the wall, and the second is the one that matters.
        //
        // Twice the plan lets a critical move think a little longer. Two
        // fifths of what is left stops that from becoming a way to spend the
        // clock.
        let hard = self.aperta_sem_inc(
            (base * self.params.tm_hard_mult as u64)
                .min(usable * self.params.tm_hard_pct as u64 / 100),
            usable,
            inc,
        );
        // A floor under both, so that a clock this low still buys a move that
        // was looked at rather than one that was guessed. It cannot make the
        // engine spend what it does not have: the floor is itself capped by
        // what is actually left.
        let floor = (self.params.tm_floor_ms as u64).min(usable);
        // O relogio dele. O tecto acompanha a razao entre os dois -- o lance que
        // merece seis segundos merece-os a` frente no relogio e nao os pode ter
        // quando ele tem tres vezes o nosso e simplesmente nos sobrevive. So' o
        // tecto: a seguranca do nosso relogio nunca depende do dele.
        let mut hard = hard;
        let mut piso_dele = 0u64;
        if self.features.tm_adversario {
            let dele = match side {
                Color::White => limits.btime.unwrap_or(0),
                Color::Black => limits.wtime.unwrap_or(0),
            };
            if dele > 0 && time > 0 {
                // Continua e nao em degraus: em degraus, 1,49x e 1,51x eram
                // mundos diferentes.
                let razao = (time * 10 / dele).clamp(3, 25);
                let ajuste = (10 + (razao as i64 - 10) / 2).clamp(5, 15) as u64;
                hard = hard * ajuste / 10;
            }
            // O ritmo dele, medido: quanto o relogio dele desceu desde o nosso
            // lance anterior, descontado o incremento que ele ganhou.
            if self.relogio_dele > 0 && dele > 0 {
                let inc_dele = match side {
                    Color::White => limits.binc,
                    Color::Black => limits.winc,
                };
                self.ritmo_dele = (self.relogio_dele + inc_dele).saturating_sub(dele);
            }
            self.relogio_dele = dele;
            // Piso, nao tecto: nao se deixa um adversario lento pensar o dobro
            // de nos. O tecto duro sobre o NOSSO relogio manda por cima disto.
            // So' com relogio curto. O limiar e' o NOSSO relogio e nao a
            // cadencia anunciada: uma partida a 3+2 que chegou aos vinte
            // segundos esta' em regime de bullet, e e' ai' que isto serve.
            let curto = time <= self.params.tm_predador_ate_s as u64 * 1000;
            if self.ritmo_dele > 0 && curto {
                piso_dele = self.ritmo_dele * self.params.tm_predador_pct as u64 / 100;
            }
        }

        let soft = base.min(hard).max(floor).max(piso_dele.min(hard));
        let hard = hard.max(soft);

        self.soft = Duration::from_millis(soft.max(1));
        self.hard = Duration::from_millis(hard.max(1));
    }

    #[inline]
    /// O lance com que esperamos que o adversario responda: o segundo da
    /// variante principal. E' o que o UCI pede no campo `ponder`.
    pub fn pv_resposta(&self) -> Option<Move> {
        if self.pv_len[0] > 1 {
            self.pv[0][1]
        } else {
            None
        }
    }

    pub(crate) fn out_of_time(&mut self) -> bool {
        if self.stopped {
            return true;
        }
        // Checking the clock is a syscall, so it is not done every node. But
        // the interval is a floor on how long the search can run without
        // noticing, and it has to be small enough to fit inside the smallest
        // allocation we will ever make. At 2048 it was not: a first iteration
        // in a middle game position is under two thousand nodes, so in real
        // time trouble the whole of it ran without the clock being read once,
        // and the engine sailed past a forty millisecond wall by taking a
        // hundred. At 512 the blind spot is a couple of milliseconds.
        if self.nodes & 511 == 0
            && (self.start.elapsed() >= self.hard || self.stop.load(Ordering::Relaxed))
        {
            self.stopped = true;
        }
        self.stopped
    }

    /// Would playing this move land straight back on a position already seen?
    #[inline]
    fn repeats_at_once(&self, board: &Board, mv: Move) -> bool {
        let mut b = board.clone();
        let undo = b.make_move(&mv);
        let h = b.hash;
        b.unmake_move(&mv, &undo);
        self.keys.iter().rev().take(64).any(|k| *k == h)
    }

    /// Has this position already occurred? One earlier occurrence is enough to
    /// treat it as drawn inside the search -- waiting for the third makes the
    /// search miss the repetition it is about to walk into.
    fn is_repetition(&self, board: &Board) -> bool {
        let back = (board.halfmove as usize).min(self.keys.len());
        // Walking back from the top, offset zero is this position, so the
        // positions with the same side to move are the EVEN offsets: two plies
        // ago, four, six. Skipping one instead of two sampled the odd ones,
        // every one of which has the other side to move, and the side to move
        // is part of the key -- so the test could not match and never did.
        //
        // Measured before the fix: three hundred and thirty four thousand
        // calls across three positions, zero detections. The engine had no
        // repetition detection at all. It would announce eight pawns of
        // advantage while playing the move that made a threefold, because for
        // the search that line was not a draw.
        self.keys
            .iter()
            .rev()
            .take(back)
            .skip(2)
            .step_by(2)
            .any(|k| *k == board.hash)
    }

    pub(crate) fn is_draw(&self, board: &Board) -> bool {
        board.halfmove >= 100 || self.is_repetition(board)
    }

    /// What a drawn position is worth, seen from the side to move at `ply`.
    ///
    /// The value belongs to the root, not to whoever happens to be on the move.
    /// Returning `-contempt` at every node makes both sides reluctant, and in a
    /// negamax that is not a preference, it is a contradiction: the root reads
    /// its own draws as costing `contempt` and the opponent's draws as gaining
    /// it, so the same drawn position is worth two different things depending
    /// on the parity of the ply it was found at.
    ///
    /// Measured with the old version, at a contempt of twenty over 176 games:
    /// fifty wins, forty-two draws and eighty-four losses, which is 68 Elo
    /// worse and the whole interval below zero. The engine was declining draws
    /// in positions it was losing, which is where it wanted them.
    ///
    /// Alternating with the ply is what makes the root read a draw as costing
    /// `contempt` everywhere.
    ///
    /// Getting the sign right did not make the idea pay. Re-measured with the
    /// alternation in place, at the same contempt of twenty over 183 games:
    /// fifty-three wins, fifty draws, eighty losses, 52 Elo worse and still the
    /// whole interval below zero. Better than the 68 it cost with both sides
    /// reluctant, and still a loss.
    ///
    /// It is not that the number is too big. Twenty here is a tenth of a pawn,
    /// half what the engine this was taken from uses. Whatever half points are
    /// won by refusing a repetition are being paid for by refusing one that
    /// should have been taken, and no flat number tells those apart. Scaling it
    /// by how far ahead the search already thinks it is would, and that is a
    /// different piece of work.
    ///
    /// `ContemptAtento` e' a peca de trabalho que a nota acima deixou por
    /// fazer. O numero fixo perde porque paga as repeticoes que recusa de
    /// menos com as que recusa de mais: numa posicao perdida o empate e' o
    /// melhor resultado que ha' e recusa-lo e' entregar a partida. Escalado
    /// pelo que a raiz ja' pensa, o desprezo existe onde e' barato -- de igual
    /// para cima -- e desaparece onde custa a partida inteira.
    ///
    /// A rampa vai de zero a meio peao: a menos de meio peao atras nao ha'
    /// desprezo nenhum, a zero ha'-lo por inteiro, e no meio e' proporcional,
    /// para nao haver um degrau onde uma pontuacao a oscilar um centipeao muda
    /// o valor de todos os empates da arvore.
    #[inline]
    pub(crate) fn draw_score(&self, ply: usize) -> i32 {
        let desprezo = if self.features.contempt_atento {
            const ATRAS: i32 = 50;
            let r = self.raiz_aval;
            if r <= -ATRAS {
                0
            } else if r < 0 {
                self.params.contempt * (r + ATRAS) / ATRAS
            } else {
                self.params.contempt
            }
        } else {
            self.params.contempt
        };
        if ply & 1 == 1 {
            desprezo
        } else {
            -desprezo
        }
    }

    pub fn go(&mut self, board: &mut Board, limits: &Limits, info: bool) -> Option<Move> {
        // O tabuleiro deles parte de onde o nosso parte. Sem isto, os lances
        // vao para cima de uma posicao por inicializar.
        if crate::ponte::ligado() {
            crate::ponte::raiz(&board.to_fen());
        }
        // Um lance so' nao se pensa. Medido: 350 ms gastos numa posicao com um
        // unico lance legal, que a 1+0 e' um terco do orcamento de um lance.
        if self.features.lance_unico && !limits.infinite {
            let unicos = crate::movegen::generate_legal(board, &self.atk);
            if unicos.len() == 1 {
                let mv = unicos[0];
                if info {
                    // A avaliacao DEPOIS do lance forcado, negada -- e nao a de
                    // antes.
                    //
                    // A de antes esta' sistematicamente errada e sempre no mesmo
                    // sentido: um lance e' forcado tipicamente porque estamos em
                    // xeque, e nessa posicao ainda nao recapturamos. Medido numa
                    // recaptura forcada: a estatica de antes dava -452, a busca
                    // dava +866. Mil trezentos e dezoito centipeoes de erro.
                    //
                    // Isso nao seria grave se ninguem lesse o numero, mas a
                    // ponte do Lichess le'-o para decidir desistir e para
                    // aceitar empates. Uma manete que so' devia poupar relogio
                    // podia entregar partidas ganhas.
                    //
                    // Um make/unmake e uma avaliacao contra os 350 ms que a
                    // manete poupa: nao se sente.
                    let undo = board.make_move(&mv);
                    crate::ponte::lance(&mv);
                    let e = -evaluate(board, self.features.rule50_fade);
                    board.unmake_move(&mv, &undo);
                    crate::ponte::desfaz(&mv);
                    println!(
                        "info depth 1 seldepth 1 score cp {} nodes 1 nps 0 time 0 pv {}",
                        e / 2,
                        mv.to_uci()
                    );
                }
                return Some(mv);
            }
        }

        // If the tables have settled this position there is nothing to search
        // for. They know who wins and in how many moves, and the move they give
        // is the one that makes progress against the fifty move rule -- which a
        // search maximising a score will not choose, because every move that
        // keeps the win looks equally winning to it.
        if !limits.infinite && limits.depth.is_none() {
            if let Some((mv, wdl)) = crate::tb::melhor_jogada_raiz(board, &self.atk) {
                if info {
                    let cp = match wdl {
                        crate::tb::Wdl::Ganha => MATE_IN_MAX - 1,
                        crate::tb::Wdl::Perde => -(MATE_IN_MAX - 1),
                        crate::tb::Wdl::Empata => 0,
                    };
                    println!(
                        "info depth 1 seldepth 1 score cp {} nodes 1 nps 0 tbhits 1 time 0 pv {}",
                        cp,
                        mv.to_uci()
                    );
                }
                return Some(mv);
            }
        }

        self.allocate(limits, board);
        self.nodes = 0;
        self.tb_hits = 0;
        self.stopped = false;
        self.stop.store(false, Ordering::Relaxed);
        self.tt.increase_gen();
        self.keys.truncate(self.root_keys);

        let max_depth = limits.depth.unwrap_or(MAX_PLY as u32 - 2).min(MAX_PLY as u32 - 2);

        let mut best: Option<Move> = None;
        let mut best_score = 0;

        // Time management state that only makes sense across iterations.
        let mut last_best: Option<Move> = None;
        let mut best_move_changes = 0i32;
        let mut iters_since_change = 0i32;
        let mut average_score = 0i32;
        // Consecutive iterations whose score stayed near that average.
        // Capped: past four the position has settled and counting further
        // says nothing more.
        let mut eval_steady = 0u32;
        let base_soft = self.soft;

        // A nota da raiz ALISADA, para o optimism. Nao se usa a ultima: ela
        // salta entre iteracoes e o optimism saltava com ela. Media simples com
        // a anterior, que e' o que a referencia faz.
        let mut media_raiz: Option<i32> = None;
        // A ultima profundidade que este fio COMPLETOU, para pesar os votos.
        let mut ultima_prof: u32 = 0;

        // OS AJUDANTES. Com `threads == 1` nada disto acontece e o caminho
        // fica byte a byte o que era -- e' a porta de entrada desta alteracao.
        let mut fios = Vec::new();
        if self.threads > 1 {
            for _ in 1..self.threads {
                let mut a = self.ajudante();
                a.set_game_history(self.keys.clone());
                let mut tab = board.clone();
                let prof = max_depth;
                // PILHA GRANDE, e nao a de omissao.
                //
                // O `Searcher` leva dentro de si a tabela de PV
                // (`[[Option<Move>; MAX_PLY]; MAX_PLY]`), o butterfly e os
                // killers -- sao centenas de kilobytes que viajam com o
                // objecto para a pilha do fio novo. Com os 2 MB por omissao o
                // fio nascia e nao voltava, e o motor ficava pendurado no `go`
                // sem panico nenhum e sem uma linha de `info`. Nao e' margem a
                // mais: e' o tamanho do que la' vai.
                fios.push(
                    std::thread::Builder::new()
                        .stack_size(32 * 1024 * 1024)
                        .spawn(move || a.corre_ajudante(&mut tab, prof))
                        .expect("nao consegui lancar o fio ajudante"),
                );
            }
        }

        for depth in 1..=max_depth {
            let iter_start = self.start.elapsed();
            self.root_effort.clear();
            self.root_scores.clear();
            let score = self.aspiration(board, depth as i32, best_score);
            if !self.stopped {
                ultima_prof = depth;
            }
            if self.features.otimismo {
                let m = match media_raiz {
                    None => score,
                    Some(a) => (score + a) / 2,
                };
                media_raiz = Some(m);
                // `otimismo = F * media / (|media| + 85)`, como na referencia.
                let o = self.params.otimismo_f * m / (m.abs() + 85);
                crate::ponte::otimismo(o);
            }

            // An aborted iteration has searched only part of the move list, so
            // its best move is not the best move -- it is whatever happened to
            // come first. Keep the previous depth.
            if self.stopped && depth > 1 {
                break;
            }

            best_score = score;
            self.raiz_aval = score;
            if self.pv_len[0] > 0 {
                best = self.pv[0][0];
            }

            // Decline a repetition when something else is nearly as good.
            //
            // Contempt already makes a repeated position score badly INSIDE
            // the search, but only where the search reaches one. A move that
            // repeats immediately -- straight back into a position already on
            // the board twice -- is decided here, where the whole root list is
            // in hand and the alternatives have scores.
            // Recusar a repeticao imediata.
            //
            // Isto ja' aqui estava, mas trancado atras de `contempt > 0` -- e o
            // contempt esta' a zero e vai continuar, porque foi medido tres
            // vezes e perde as tres. Resultado: o bloco nunca correu.
            //
            // Sao coisas diferentes. O contempt desconta TODOS os empates da
            // arvore inteira, incluindo os das posicoes perdidas, onde o empate
            // e' o melhor resultado que ha' -- foi por isso que perdeu. Isto so'
            // olha para uma coisa: o lance que devolve a posicao ao tabuleiro
            // pela terceira vez, na raiz, com a lista toda em mao e as
            // alternativas ja' pontuadas. Se houver outra a menos de
            // `RecusaMargem`, joga-se essa.
            //
            // E so' quando NAO estamos pior. Numa posicao ma' a repeticao e' o
            // que nos salva, e recusa-la e' entregar a partida -- que e'
            // exactamente o erro que o contempt fixo cometia.
            //
            // O bot empatou tres vezes seguidas com o MalanChess (2700) e uma
            // com o banerot (2738), em partidas de 125 a 149 lances. Contra
            // adversarios duzentos pontos abaixo, cada um desses empates custa
            // cinco ou seis pontos de rating.
            //
            // MEDIDO a` primeira tentativa, contra um adversario 270 pontos
            // abaixo, com limiar zero e margem 60: 242 vitorias, 32 derrotas,
            // 43 empates, contra 205-1-112 da base. Os empates cairam de 112
            // para 43, que era o objectivo -- mas as derrotas subiram de UMA
            // para trinta e duas. Estando nos 270 pontos acima, converter um
            // empate devia dar vitoria quase sempre e estava a dar cinquenta
            // por cento: a condicao era frouxa de mais. Aceitava alternativas
            // ate' 30 centipeoes piores e bastava a posicao nao estar negativa.
            //
            // Dai' o limiar: nao "nao estou pior", mas "estou claramente a
            // ganhar". Uma posicao de +0,5 que se repete pode muito bem ser um
            // empate justo, e trocar essa repeticao por um lance 30 centipeoes
            // pior e' como se perde a partida.
            let margem_recusa = if self.params.contempt > 0 {
                Some(self.params.contempt)
            } else if self.features.recusa_repeticao && best_score >= self.params.recusa_limiar {
                Some(self.params.recusa_margem)
            } else {
                None
            };
            if let Some(margem_recusa) = margem_recusa
                .filter(|_| !(self.features.contempt_atento && best_score <= -50))
            {
                if let Some(b) = best {
                    if self.repeats_at_once(board, b) {
                        let margin = margem_recusa;
                        let alt = self
                            .root_scores
                            .iter()
                            .filter(|(m, _)| *m != b && !self.repeats_at_once(board, *m))
                            .max_by_key(|(_, sc)| *sc);
                        if let Some((m, sc)) = alt {
                            if *sc >= best_score - margin {
                                best = Some(*m);
                            }
                        }
                    }
                }
            }

            if info {
                self.print_info(depth, score);
            }

            if limits.nodes.map_or(false, |n| self.nodes >= n) {
                break;
            }

            // What the position is telling us about how long to keep going.
            //
            // Three signals, and they answer different questions. A score that
            // is falling means the move we have is worse than we thought and
            // the alternatives deserve another look. A best move that has
            // stopped changing means the answer has settled and more time buys
            // nothing. And a move that took most of the tree to itself and
            // still came out on top was never a close call.
            if depth == 1 {
                average_score = score;
            } else {
                // Against the average before it swallows this score:
                // comparing with one that already has is an easier question and
                // makes the window mean half what it says.
                let ediff = (score - average_score).abs();
                eval_steady = if ediff <= self.params.tm_trend_window {
                    (eval_steady + 1).min(4)
                } else {
                    0
                };
                average_score = (score + 9 * average_score) / 10;
            }
            if best != last_best {
                last_best = best;
                iters_since_change = 0;
                best_move_changes += 1;
            } else {
                iters_since_change += 1;
            }

            let spent: u64 = self.root_effort.iter().map(|(_, n)| *n).sum();
            let on_best = best
                .and_then(|b| self.root_effort.iter().find(|(m, _)| *m == b))
                .map(|(_, n)| *n)
                .unwrap_or(0);
            // With nothing measured yet, claim the middle rather than either
            // end: an unknown share should neither buy time nor spend it.
            let effort_frac = if spent > 0 && self.features.tm_node_effort {
                on_best as f64 / spent as f64
            } else {
                0.4
            };
            let drop = if self.features.tm_stability {
                (average_score - score).max(0)
            } else {
                0
            };
            let settle = if self.features.tm_stability {
                iters_since_change as u32
            } else {
                0
            };
            let changes = if self.features.tm_stability {
                best_move_changes.max(0) as u32
            } else {
                0
            };
            let steady = if self.features.tm_trend {
                Some(eval_steady)
            } else {
                None
            };
            // Com a `TmPawn`, o elastico tambem e' o dele. Portar a alocacao
            // sem portar o elastico foi meia correccao: os nossos factores
            // chegam a 3,40 e o motor estica quase todos os lances ao maximo
            // -- medido, factores de 1,91 a 3,38 em lances seguidos, com
            // base=3439ms a virar soft=6778ms.
            //
            // O do pawn:
            //   avaliacao a cair  clamp(1 + (media - melhor)/100, 1.00, 1.75)
            //   estabilidade      clamp(1 - iters_sem_mudar/(2*prof), 0.75, 1.00)
            //   instabilidade     clamp(0.9 + mudancas/(2*prof),     1.00, 1.50)
            //
            // Duas diferencas fazem-no funcionar: a estabilidade ENCOLHE ate'
            // 0,75 -- posicao facil gasta menos, e e' dai' que vem a poupanca
            // que sustenta o resto da partida; e os factores sao relativos a`
            // PROFUNDIDADE, portanto uma mudanca de lance pesa metade a
            // profundidade 20 do que pesa a 10.
            let factor = if self.features.tm_pawn {
                let prof = (depth.max(1)) as f64;
                let cair = (1.0 + drop as f64 / 100.0).clamp(1.0, 1.75);
                let estab = (1.0 - settle as f64 / (2.0 * prof)).clamp(0.75, 1.0);
                let instab = (0.9 + changes as f64 / (2.0 * prof)).clamp(1.0, 1.5);
                cair * estab * instab
            } else {
                time_scale(effort_frac, settle, drop, changes, steady,
                           self.params.tm_escala_max, self.params.tm_escala_min)
            };

            // The scaling moves the plan, never the wall. Whatever the position
            // says, a move cannot spend more than the clock allows.
            let soft = base_soft.mul_f64(factor).min(self.hard);
            self.soft = soft;
            // Diagnostico: o orcamento que o motor se da' a si proprio, lance a
            // lance. Sem isto so' se ve' o tempo GASTO, e nao da' para saber se
            // ele gastou o que pediu ou se passou do que pediu.
            if tempo_debug() {
                eprintln!(
                    "ORC base={}ms factor={:.2} soft={}ms hard={}ms gasto={}ms",
                    base_soft.as_millis(), factor, soft.as_millis(),
                    self.hard.as_millis(), self.start.elapsed().as_millis()
                );
            }

            // Is there room for another iteration, not is there room for the
            // one just finished.
            //
            // Each depth costs roughly twice the one before, so stopping only
            // once the plan is already spent means routinely starting an
            // iteration that cannot fit and letting the wall end it. The spend
            // then settles at about twice the plan, which is enough to break
            // even against the increment: measured over a fifty-nine move game
            // at 8+0.08, the engine used 12.69 seconds of a 12.72 second
            // budget and flagged. Nothing looked wrong move by move -- the
            // longest was 0.72 seconds -- because nothing was wrong move by
            // move.
            //
            // Predicting the next one instead leaves the margin the increment
            // is supposed to build.
            let elapsed = self.start.elapsed();
            let last = elapsed.saturating_sub(iter_start);
            if elapsed + last * 2 >= self.soft {
                break;
            }
        }

        // Os ajudantes so' param pelo sinal partilhado: o relogio e' deste
        // fio. Sem este `store` ficavam a procurar para sempre.
        if !fios.is_empty() {
            self.stop.store(true, Ordering::Relaxed);
            let mut cand: Vec<(Move, i32)> = Vec::with_capacity(fios.len() + 1);
            // A profundidade que o fio principal completou. Um ajudante que
            // tenha ficado dois plies atras ja' nao esta' a ver a mesma coisa e
            // nao vota: o proposito do Lazy SMP e' divergir no caminho, nao
            // decidir com menos informacao.
            let prof_principal = ultima_prof;
            if let (Some(b), true) = (best, best_score > -MATE) {
                cand.push((b, best_score));
            }
            for f in fios {
                if let Ok(Some((m, sc, d))) = f.join() {
                    if d + 2 >= prof_principal {
                        cand.push((m, sc));
                    }
                }
            }
            if cand.len() > 1 {
                if let Some(v) = Searcher::vota(&cand) {
                    best = Some(v);
                }
            }
        }

        // Never return nothing: if even depth one was cut short, play the first
        // legal move rather than forfeit.
        if est_ligado() {
            self.relata_forma();
            self.relata_saidas();
        }
        best.or_else(|| generate_legal(board, &self.atk).into_iter().next())
    }

    /// De que e' feita a arvore que acabamos de construir.
    ///
    /// A pergunta que isto responde: chegar a` profundidade 14 custa-nos 2,67s
    /// contra 0,88s do Triumviratus nas mesmas cinco posicoes, com nos por
    /// segundo equivalentes -- portanto o excesso e' arvore, nao velocidade. O
    /// racio e' CONSTANTE nas profundidades 10, 12 e 14 (2,6x, 2,9x, 2,6x) e o
    /// factor de ramificacao e' igual ao deles, 1,62 contra 1,63. Um excesso
    /// uniforme, e nao um que cresce, nao aponta para a forma da reducao --
    /// essa composedagia-se com a profundidade. Aponta para alguma coisa que se
    /// paga em todos os nos.
    fn relata_saidas(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        let n: Vec<u64> = SAIDA.iter().map(|a| a.load(Relaxed)).collect();
        let ent = n[0].max(1) as f64;
        let nomes = ["entrou", "quiescencia", "empate/limite", "CORTOU pela tabela",
                     "tablebases", "futilidade inv.", "sondas a` tabela",
                     "encontrou entrada", "entrada com limite", "GEROU LANCES",
                     "entrada funda o bastante", "chegou ao teste final",
                     "barrado: limite nao serve", "barrado: regra dos 50"];
        let mut s = String::new();
        let nomes: Vec<&str> = nomes.to_vec();
        for (i, nome) in nomes.iter().enumerate() {
            if i == 0 { continue; }
            s.push_str(&format!(" {}={} ({:.1}%)", nome, n[i], n[i] as f64 * 100.0 / ent));
        }
        println!("info string SAIDAS entrou={}{}", n[0], s);
    }

    fn relata_forma(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        let p = FORMA[0].load(Relaxed);
        let q = FORMA[1].load(Relaxed);
        let l = FORMA[2].load(Relaxed);
        let c1 = FORMA[3].load(Relaxed);
        let rl = FORMA[4].load(Relaxed);
        let rj = FORMA[5].load(Relaxed);
        let tot = (p + q).max(1);
        println!(
            "info string FORMA principal={} ({:.1}%) quiescencia={} ({:.1}%) \
lances_procurados/no={:.2} corte1={:.1}% rebusca_lmr={} ({:.1}% dos nos) rebusca_janela={} ({:.1}%)",
            p, p as f64 * 100.0 / tot as f64,
            q, q as f64 * 100.0 / tot as f64,
            l as f64 / p.max(1) as f64,
            c1 as f64 * 100.0 / p.max(1) as f64,
            rl, rl as f64 * 100.0 / p.max(1) as f64,
            rj, rj as f64 * 100.0 / p.max(1) as f64,
        );
    }

    fn print_info(&self, depth: u32, score: i32) {
        let ms = self.start.elapsed().as_millis().max(1) as u64;
        let nps = self.nodes * 1000 / ms;
        let score_str = if is_mate(score) {
            let plies = MATE - score.abs();
            let moves = (plies + 1) / 2;
            format!("mate {}", if score > 0 { moves } else { -moves })
        } else {
            // Two internal units to the centipawn, from the training
            // quantisation. The win/draw/loss figures below are NOT converted:
            // that model was fitted against the internal units and its offset
            // and scaling are in them, so handing it centipawns would quietly
            // halve every probability it reports.
            format!("cp {}", score / 2)
        };
        let (w, d, l) = nnue::wdl(score);
        let mut pv = String::new();
        for i in 0..self.pv_len[0] {
            if let Some(m) = self.pv[0][i] {
                pv.push(' ');
                pv.push_str(&m.to_uci());
            }
        }
        println!(
            "info depth {} score {} wdl {} {} {} nodes {} nps {} time {} pv{}",
            depth, score_str, w, d, l, self.nodes, nps, ms, pv
        );
    }

    /// Search the root with a window around the last score, widening on a
    /// failure rather than starting wide every time.
    fn aspiration(&mut self, board: &mut Board, depth: i32, prev: i32) -> i32 {
        // Wider at low depth, where the previous score is a poor guide, and
        // narrowing as it becomes a good one.
        let mut delta = 5 + self.params.asp_delta * 8 / depth.max(1);
        let (mut alpha, mut beta) = if depth <= self.params.asp_depth || is_mate(prev) {
            (-INF, INF)
        } else {
            (prev - delta, prev + delta)
        };

        // Quantas falhas por cima seguidas. Cada uma tira um ply a` busca de
        // repeticao: uma posicao que ja' decidiu vai falhar na mesma direccao,
        // e confirma-la a` profundidade cheia e' pagar a arvore toda para saber
        // o que ja' se sabia.
        let mut falhas = 0i32;
        loop {
            self.root_delta = (beta - alpha).max(1);
            // Once a bound is this far from level, the window has stopped being
            // a guess worth narrowing and has become an obstacle: a position
            // that decided is going to keep failing in the same direction, and
            // each failure costs a whole re-search to widen by a step.
            if alpha < -1000 {
                alpha = -INF;
            }
            if beta > 1000 {
                beta = INF;
            }

            let d = if self.features.asp_baixa {
                (depth - falhas).max(1)
            } else {
                depth
            };
            let score = self.negamax(board, d, alpha, beta, 0, true, false);
            if self.stopped {
                return score;
            }
            if score <= alpha {
                // Failing low means the position is worse than believed, and
                // the upper bound has to move with the lower one or the next
                // attempt fails low again at once.
                beta = (alpha + beta) / 2;
                alpha = (score - delta).max(-INF);
                // Uma janela que ja' oscilou nos dois sentidos nao decidiu
                // nada, e a profundidade volta ao que era.
                falhas = 0;
            } else if score >= beta {
                if self.features.asp_baixa {
                    // O limite de baixo sobe com o de cima: sem isso a
                    // repeticao volta a percorrer terreno ja' coberto.
                    alpha = (beta - delta).max(alpha);
                }
                beta = (score + delta).min(INF);
                falhas += 1;
            } else {
                return score;
            }
            delta += delta / 2;
        }
    }

    /// `cut_node` says this node is expected to fail high.
    ///
    /// It is not a guess made here: it is handed down. The first child of a
    /// principal variation node is another one; every later child of a
    /// principal variation node is expected to fail high; the children of a
    /// node expected to fail high are expected to fail low, and the other way
    /// round. Knowing which kind of node you are in is worth something, because
    /// a node that is expected to fail high will do it on one of the first
    /// moves or not at all, so the late ones there can be reduced harder than
    /// the same moves somewhere else.
    #[inline]
    fn killer_fresco_on(&self) -> bool {
        self.features.killer_fresco
    }

    fn negamax(
        &mut self,
        board: &mut Board,
        mut depth: i32,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        pv_node: bool,
        cut_node: bool,
    ) -> i32 {
        self.pv_len[ply] = 0;

        // Os killers do ply abaixo ficaram la' da ultima sub-arvore, que nao
        // tem nada a ver com esta. Herda-los e' promover lances a` frente da
        // lista por razao nenhuma, e a classe acerta 66% onde as capturas
        // acertam 83-87%. O motor da nossa arquitectura limpa-os e mede +12,15
        // Elo nisso.
        if self.killer_fresco_on() && ply + 1 < MAX_PLY {
            self.killers[ply + 1] = [None; NUM_KILLERS];
        }

        // Repoe-se DOIS plies a` frente: o contador do ply SEGUINTE tem de
        // sobreviver a esta visita para a reducao o poder ler.
        if ply + 2 < MAX_PLY + 8 {
            self.cut_cnt[ply + 2] = 0;
        }

        if depth <= 0 {
            marca(1);
            return self.quiescence(board, alpha, beta, ply);
        }

        self.nodes += 1;
        marca(0);
        if est_ligado() { FORMA[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
        if self.out_of_time() {
            return 0;
        }

        let root = ply == 0;
        let in_check = board.in_check(board.side, &self.atk);

        if !root {
            if self.is_draw(board) {
                marca(2);
                return self.draw_score(ply);
            }
            if ply >= MAX_PLY - 1 {
                return evaluate(board, self.features.rule50_fade);
            }

            // Below the size the tables cover, the result is not an estimate.
            // Returning it ends the subtree at once, and ends it with the right
            // answer -- which is worth more than the nodes saved, because the
            // endings the tables cover are the ones a network reads worst.
            //
            // The score is placed just inside the mate range so it outranks any
            // evaluation without ever being mistaken for a real mate, and the
            // distance to the root keeps a shorter win preferred to a longer
            // one even though the tables here cannot say how long either is.
            if let Some(w) = crate::tb::sondar(board) {
                match w {
                    // A drawn table position is settled and can be returned
                    // whatever else is loaded: there is no progress to measure
                    // in a draw, so nothing is lost by not knowing the distance.
                    crate::tb::Wdl::Empata => {
                        self.tb_hits += 1;
                        return self.draw_score(ply);
                    }
                    // A won one is only useful when the set can say how long
                    // the win takes. Told merely that it is won, every move
                    // that keeps it scores the same, the search has nothing to
                    // choose between them, and a rook up becomes a draw by the
                    // fifty move rule. Measured: king and rook against king
                    // went from a1a6, which restricts the king, to e1e2, which
                    // does nothing at all.
                    _ if crate::tb::tem_dtz() => {
                        self.tb_hits += 1;
                        let v = if w == crate::tb::Wdl::Ganha {
                            MATE_IN_MAX - 2 - ply as i32
                        } else {
                            -(MATE_IN_MAX - 2 - ply as i32)
                        };
                        return v;
                    }
                    _ => {}
                }
            }
            // REPETICAO A` DISTANCIA DE UM LANCE. O teste acima so' sabe que repetiu DEPOIS de
            // repetir; se quem joga pode forcar a repeticao daqui, ha' um empate garantido e o
            // alpha nao pode ficar abaixo dele. Padrao do Stockfish; ver `cuckoo.rs`.
            if self.features.cuckoo {
                let empate = self.draw_score(ply);
                if alpha < empate {
                    let cuc = CUCKOO.get_or_init(|| {
                        crate::cuckoo::Cuckoo::novo(crate::zobrist::tabelas(), &self.atk)
                    });
                    if cuc.repeticao_a_vista(board, &self.keys, self.root_keys, &self.atk) {
                        alpha = empate;
                        if alpha >= beta {
                            return alpha;
                        }
                    }
                }
            }
            // Mate distance pruning: no line from here can beat a mate already
            // found closer to the root.
            let a = alpha.max(mate_score(ply));
            let b = beta.min(-mate_score(ply + 1));
            if a >= b {
                marca(4);
                return a;
            }
            alpha = a;
        }

        // A node searching with a move excluded is asking a different question
        // from the one the table answered, so it must not take the answer --
        // nor leave its own answer behind for a node that is asking the
        // ordinary question.
        let excluded = self.excluded[ply];
        let entry = self.tt.probe(board.hash);
        if est_ligado() {
            marca(6); // sondas
            if entry.is_some() { marca(7); }         // encontrou entrada
        }
        let mut tt_move = None;
        let mut tt_bound = Bound::NoBound;
        let mut tt_pv = pv_node;
        if let Some(e) = entry {
            tt_move = e.best;
            tt_bound = e.bound;
            tt_pv |= e.pv;
            if est_ligado() {
                marca(8);
                if e.depth >= depth { marca(10); }
            }
            if excluded.is_none() && !pv_node && e.depth >= depth && e.has_bound() {
                let s = score_from_tt(e.score, ply);
                let usable = match e.bound {
                    Bound::Exact => true,
                    Bound::Lower => s >= beta,
                    Bound::Upper => s <= alpha,
                    Bound::NoBound => false,
                };
                // Not near the fifty move wall: there the same position is
                // worth different things depending on how much counter is left,
                // and the table does not know which one it stored.
                if est_ligado() {
                    marca(11);                                   // chegou ao teste final
                    if !usable { marca(12); }                    // o limite nao serve
                    if usable && board.halfmove >= 90 { marca(13); } // barrado pela regra dos 50
                }
                if usable && board.halfmove < 90 {
                    // Credit the stored move on the way out. It just caused a
                    // cutoff, which is the same evidence a searched move would
                    // have produced, and returning without recording it lets
                    // the tables go cold in exactly the positions that come
                    // back most often -- the ones the table keeps answering.
                    if self.features.tt_cut_credit && s >= beta {
                        if let Some(m) = tt_move {
                            // As close to a legality test as is affordable here:
                            // one of ours on the origin square, and nothing of
                            // ours on the destination. It does not prove the
                            // move is legal, which is why this is off.
                            let ours = board
                                .piece_at(m.from)
                                .is_some_and(|(_, c)| c == board.side);
                            let free = board
                                .piece_at(m.to)
                                .is_none_or(|(_, c)| c != board.side);
                            if ours && free && !m.is_capture() && m.promotion.is_none() {
                                let side = board.side.idx();
                                let slots = self.cont_slots(ply);
                                self.credit(board, m, side, &slots, hist_bonus(depth));
                                if !self.killers[ply].iter().any(|k| *k == Some(m)) {
                                    for j in (1..NUM_KILLERS).rev() {
                                        self.killers[ply][j] = self.killers[ply][j - 1];
                                    }
                                    self.killers[ply][0] = Some(m);
                                }
                            }
                        }
                    }
                    marca(3);
                    return s;
                }
            }
        }

        // The raw number is kept separately, because it is what goes back into
        // the table at the bottom of this node.
        //
        // Storing the corrected value there instead is a quiet disaster: the
        // next visit reads it, applies the correction a second time, stores
        // that, and the error compounds every time the position is reached.
        // Nothing about it looks wrong from outside -- the evaluation stays
        // plausible while drifting.
        let raw_static_eval = if in_check {
            TT_EVAL_NONE as i32
        } else {
            match entry {
                Some(e) if e.static_eval != TT_EVAL_NONE => e.static_eval as i32,
                _ => {
                    let e = evaluate(board, self.features.rule50_fade);
                    self.tt.store_eval_only(board.hash, e as i16);
                    e
                }
            }
        };
        let static_eval = raw_static_eval;
        // The table keeps the raw number, the search uses the corrected one.
        // Deliberately different: the table is shared, and whoever reads it
        // later applies their own correction.
        let static_eval = if in_check || !self.features.corr_hist {
            static_eval
        } else {
            self.corrected(board, static_eval, ply)
        };

        self.eval_stack[ply] = static_eval;
        // Is the side to move better off than it was two plies ago?
        //
        // A ply spent in check has no static evaluation and its slot holds a
        // sentinel, not a score. Comparing against the sentinel made every
        // position for two plies after any check look like it was improving,
        // because anything beats minus thirty two thousand -- so reverse
        // futility pruned harder and late move pruning cut later, both on a
        // fact that was not one. Step back four plies when two are not usable,
        // and claim nothing when neither is.
        let usable = |v: i32| v != TT_EVAL_NONE as i32;
        let improving = if in_check {
            false
        } else if ply >= 2 && usable(self.eval_stack[ply - 2]) {
            static_eval > self.eval_stack[ply - 2]
        } else if ply >= 4 && usable(self.eval_stack[ply - 4]) {
            static_eval > self.eval_stack[ply - 4]
        } else {
            false
        };

        // Two evaluations from here on, and they are not the same number.
        //
        // `static_eval` is what the network says, corrected, and it is what
        // `improving` and the forward futility margin compare against -- both
        // want a value that means the same thing at every ply, which a score
        // borrowed from a search does not.
        //
        // `pruning_eval` is that value improved by what the table already
        // knows. A stored lower bound above the static score, or an upper
        // bound below it, is a better estimate than the static score by
        // definition: a search went and found out. Whole-node pruning should
        // use the better one, and it was using the worse one.
        let mut pruning_eval = static_eval;
        if !in_check {
            if let Some(e) = entry {
                if e.has_bound() {
                    let ts = score_from_tt(e.score, ply);
                    let better = match e.bound {
                        Bound::Exact => true,
                        Bound::Lower => ts > static_eval,
                        Bound::Upper => ts < static_eval,
                        Bound::NoBound => false,
                    };
                    if better {
                        pruning_eval = ts;
                    }
                }
            }
        }

        if !pv_node && !in_check {
            // Reverse futility: so far ahead that giving away the margin still
            // beats beta, and the opponent has no way to take it all back in
            // the remaining depth. A ply that is improving can afford a
            // narrower margin, since the trend is evidence in the same
            // direction as the score.
            let margin = self.params.rfp_margin * depth
                - self.params.rfp_improving * improving as i32;
            // `rfp_tt_capt`: sem lance na tabela, ou com uma captura la' guardada, a entrada
            // nao promete um plano tranquilo bom e cortar pela estatica nao descarta um.
            if depth < self.params.rfp_depth
                && pruning_eval - margin >= beta
                && pruning_eval.abs() < MATE_IN_MAX
                && (!self.features.rfp_tt_capt || tt_move.map_or(true, |m| m.is_capture()))
            {
                // Part of the way to the estimate rather than all of it. The
                // margin establishes that the node is above beta, not by how
                // much, and returning the whole distance passes upwards a
                // confidence that was never earned.
                marca(5);
                return if self.features.rfp_damp {
                    beta + (pruning_eval - beta) / 3
                } else {
                    pruning_eval
                };
            }

            // Razoring: so far behind that even the quiescence search is
            // unlikely to find enough, so ask it directly instead of spending
            // a full width on the answer. If it turns out to be wrong the
            // score comes back above alpha and the node is searched properly.
            // The plain static score here, not the one the table improved.
            // Razoring asks whether the position is so far behind that only a
            // capture sequence could save it; a bound borrowed from a search
            // has already priced those in, and asking with it is asking a
            // question that has been answered.
            //
            // And not when alpha is already decisive -- a margin has nothing to
            // say about a position that is being mated.
            if self.features.razoring
                && depth <= self.params.razor_depth
                && alpha.abs() < 2000
                && static_eval + self.params.razor_margin * depth <= alpha
            {
                let q = self.quiescence(board, alpha, alpha + 1, ply);
                if q < alpha {
                    marca(6);
                    return q;
                }
            }

            // Null move: hand the opponent a free move and see whether the
            // position still holds. Not with only pawns left, where passing is
            // often the best move there is and the conclusion would be wrong.
            // The reduction grows with how far above beta we already are,
            // rather than with depth: the question null move asks is whether
            // the position is so good it survives giving away a move, and how
            // good it is answers that better than how deep we are.
            //
            // The extra conditions matter: the
            // raw static score has to be at least as good as the uncorrected
            // one, and the uncorrected one has to be within reach of beta. A
            // position that only looks good because the table said so is not
            // one to hand a free move away in.
            // Only where the node is expected to fail high. Elsewhere the
            // question null move asks -- is this so good it survives giving a
            // move away -- is not the question the node is there to answer.
            if (!self.features.nmp_cut_node || cut_node)
                && depth >= 3
                && pruning_eval >= beta
                && pruning_eval >= self.eval_stack[ply]
                && self.eval_stack[ply]
                    >= beta - 20 * depth - 40 * improving as i32 + 100
                && has_pieces(board, board.side)
                && !(ply > 0 && self.null_at[ply - 1])
            {
                // A reducao do lance nulo, a crescer com a profundidade.
                //
                // Sem o termo da profundidade -- que e' como estava -- reduz-se
                // quatro plies a` profundidade 3 e os MESMOS quatro a`
                // profundidade 14. A pergunta do lance nulo e' sempre a mesma
                // ("isto e' tao bom que sobrevive a dar um lance de borla?"),
                // mas o custo de a fazer cresce com a profundidade, e a
                // confianca na resposta tambem: quanto mais fundo se esta',
                // mais barata sai a verificacao em proporcao ao que ela poupa.
                // O motor de referencia soma profundidade/3 por isso mesmo.
                //
                // Medido a` profundidade 14, nas mesmas tres posicoes: o
                // Triumviratus chega la' com 2,6 vezes menos nos do que nos, com
                // nos por segundo equivalentes. A diferenca e' toda arvore, e
                // uma reducao que nao cresce e' uma das razoes por que ela nao
                // encolhe onde devia.
                let mut r = self.params.nmp_base
                    + ((pruning_eval - beta) / 200).min(self.params.nmp_div);
                if self.features.nmp_profundidade {
                    r += depth / self.params.nmp_prof_div;
                }
                let undo = board.make_null_move();
                crate::ponte::nulo();
                self.keys.push(board.hash);
                self.null_at[ply] = true;
                // Passing the position over expects the opposite of whatever
                // this node expects.
                let score =
                    -self.negamax(board, depth - r, -beta, -beta + 1, ply + 1, false, !cut_node);
                self.null_at[ply] = false;
                self.keys.pop();
                board.unmake_null_move(&undo);
                crate::ponte::desfaz_nulo();
                if score >= beta {
                    // A mate score from a null move search is an artefact of
                    // the free move; report the bound instead.
                    marca(7);
                    return if is_mate(score) { beta } else { score };
                }
            }
        }

        // Nothing in the table for a node this deep means no move worth
        // trying first, and searching at full depth to discover one costs more
        // than finding it a ply shallower and coming back.
        // `iir_no_all`: um no' ALL (nem PV nem de corte) vai procurar tudo de
        // qualquer maneira; encolher-lhe a profundidade so' lhe tira qualidade.
        if self.features.iir
            && depth >= 4
            && tt_move.is_none()
            && (!self.features.iir_no_all || pv_node || cut_node)
        {
            depth -= 1;
        }

        // A stored lower bound far enough above beta already answers the
        // question this node was about to ask, even at a depth we would not
        // normally trust. It cost a search once; there is no reason to pay
        // again to be told the same thing by a smaller margin.
        if self.features.probcut && !pv_node && !in_check && excluded.is_none() {
            if let Some(e) = entry {
                if matches!(e.bound, Bound::Lower | Bound::Exact)
                    && e.depth >= depth - 2
                    && beta.abs() < MATE_IN_MAX
                {
                    let ts = score_from_tt(e.score, ply);
                    if !is_mate(ts) && ts >= beta + self.params.probcut_margin {
                        marca(8);
                        return ts;
                    }
                }
            }
        }

        marca(9);
        // Geracao por etapas.
        //
        // Um em cada quatro nos chega aqui, e desses 92% cortam sem precisar de
        // um unico lance tranquilo. Gerar a lista toda, filtrar-lhe a
        // legalidade e pontuar cada tranquilo com o historico e as tabelas de
        // continuacao e' trabalho feito e deitado fora em massa -- e' por isso
        // que `score_moves` mais `pick` somam 12,8% do tempo, tanto como o
        // proprio `negamax`.
        //
        // Aqui geram-se so' as capturas. Os tranquilos entram mais tarde, e so'
        // se a busca la' chegar: o gatilho e' o `pick` deixar de encontrar
        // alguma coisa acima da banda dos killers, que e' exactamente o momento
        // em que a tabela, as capturas boas e as promocoes se esgotaram.
        //
        // Em xeque nao ha' etapas: as fugas incluem lances tranquilos e a lista
        // tem de vir inteira.
        let etapas = self.features.gera_etapas && !in_check;
        let mut faltam_tranquilos = etapas;
        let mut moves = if etapas {
            crate::movegen::generate_legal_caps(board, &self.atk)
        } else {
            generate_legal(board, &self.atk)
        };
        if moves.is_empty() && !faltam_tranquilos {
            return if in_check { mate_score(ply) } else { 0 };
        }
        let (mut scores, mut hist) = self.score_moves(board, &moves, tt_move, tt_bound, ply, depth);
        if est_ligado() {
            use std::sync::atomic::Ordering::Relaxed;
            EST[0].fetch_add(1, Relaxed);
            EST[1].fetch_add(moves.len() as u64, Relaxed);
        }

        let mut best_score = -INF;
        let mut best_move = None;
        let alpha_orig = alpha;
        let mut searched_quiets: Vec<Move> = Vec::new();
        let mut tipo_primeiro = 3usize;
        let mut slot_primeiro: Option<usize> = None;
        let mut searched_captures: Vec<Move> = Vec::new();
        // Com geracao por etapas, uma posicao AFOGADA deixa de ser apanhada pela
        // verificacao de antes do ciclo: a lista das capturas nasce vazia, os
        // tranquilos sao gerados la' dentro e tambem nao ha' nenhum. Sem isto o
        // no' caia no `return alpha` la' em baixo e um empate por afogamento
        // passava a valer o que a janela dissesse.
        let mut algum_lance = false;

        let tt_score_for_singular = entry
            .filter(|e| e.has_bound())
            .map(|e| score_from_tt(e.score, ply));

        // Once the quiet moves are done with, the captures behind them are not.
        let mut skip_quiets = false;

        let mut tranquilos_pontuados = false;
        // O indice avanca no TOPO, nao no fim: o corpo tem `continue`s, e num
        // `while` eles saltariam o incremento e prendiam o ciclo. Foi o que
        // aconteceu a` primeira -- o motor deixou de devolver lance nenhum.
        let mut proximo = 0usize;
        loop {
            // Os tranquilos entram em dois casos: quando a lista das capturas se
            // esgota, e quando o melhor que resta ja' esta' abaixo da banda dos
            // killers -- que e' o momento em que a tabela, as capturas boas e as
            // promocoes acabaram.
            //
            // O primeiro caso faltava-me a` primeira tentativa: numa posicao sem
            // capturas nenhumas a lista nasce vazia, o ciclo nunca corria, os
            // tranquilos nunca eram gerados e o no' devolvia lixo. A arvore
            // encolhia para um terco e parecia um ganho enorme.
            if faltam_tranquilos
                && (proximo >= moves.len()
                    || scores[proximo..].iter().copied().max().unwrap_or(i32::MIN) < 400_000)
            {
                let tranquilos = crate::movegen::generate_legal_quiets(board, &self.atk);
                if !tranquilos.is_empty() {
                    let (o2, h2) =
                        self.score_moves(board, &tranquilos, tt_move, tt_bound, ply, depth);
                    moves.extend_from_slice(&tranquilos);
                    scores.extend_from_slice(&o2);
                    hist.extend_from_slice(&h2);
                }
                faltam_tranquilos = false;
            }
            if proximo >= moves.len() {
                break;
            }
            let i = proximo;
            proximo += 1;
            algum_lance = true;
            if est_ligado() {
                EST[2].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if self.features.pick_cpp {
                Self::pick_cpp(&mut moves, &mut scores, &mut hist, i);
            } else {
                Self::pick(&mut moves, &mut scores, &mut hist, i);
            }
            // Chegamos ao fim dos nao-tranquilos: agora sim vale a pena saber o
            // que as tabelas acham dos que sobram.
            //
            // A pergunta e' feita DEPOIS do `pick` e sobre o lance escolhido.
            // Com o sentinela na banda certa, o primeiro tranquilo por pontuar
            // a ser escolhido e' exactamente o momento em que a tabela, as
            // capturas boas e os killers se esgotaram -- e ainda antes das
            // capturas mas. Perguntar antes obrigava a uma varredura da lista
            // por cada lance, que e' o que o `pick` ja' faz.
            if self.features.pontua_tarde
                && !tranquilos_pontuados
                && scores[i] == TRANQUILO_POR_PONTUAR
            {
                self.pontua_tranquilos(board, &moves, &mut scores, &mut hist, i, ply);
                tranquilos_pontuados = true;
                Self::pick(&mut moves, &mut scores, &mut hist, i);
            }
            let mv = moves[i];
            if Some(mv) == excluded {
                continue;
            }
            if est_ligado() {
                FORMA[2].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            let is_quiet = !mv.is_capture() && mv.promotion.is_none();
            // Categoria, para o instrumento: 0 tabela, 1 captura, 2 killer,
            // 3 tranquilo. A ordem importa -- o lance da tabela pode tambem
            // ser captura, e conta como o primeiro por ser essa a razao de
            // estar a` frente.
            if ttb_ligado() && i == 0 && Some(mv) == tt_move {
                use std::sync::atomic::Ordering::Relaxed;
                let b = match tt_bound {
                    Bound::Exact => 0,
                    Bound::Lower => 2,
                    Bound::Upper => 4,
                    Bound::NoBound => 9,
                };
                if b < 6 {
                    TTB[b].fetch_add(1, Relaxed);
                }
            }
            let tipo_mv = if Some(mv) == tt_move {
                0usize
            } else if !is_quiet {
                1
            } else if self.killers[ply].iter().any(|k| *k == Some(mv)) {
                2
            } else {
                3
            };
            if i == 0 {
                tipo_primeiro = tipo_mv;
                slot_primeiro = self.killers[ply].iter().position(|k| *k == Some(mv));
            }
            if skip_quiets && is_quiet {
                continue;
            }
            let mut extension = 0;

            // The order below is not ours to choose. Late move count, then the
            // exchange test on captures, then history, then the static margin,
            // then the exchange test on quiets -- the sequence the engines that
            // measured it settled on, cheapest question first so the expensive
            // ones are never asked about a move that is already gone.
            //
            // Everything here needs a score already in hand: without one, the
            // node has nothing to compare a margin against and skipping moves
            // risks reporting a mate that is not there.
            if !root
                && !pv_node
                && !in_check
                && best_score > -MATE_IN_MAX
                && has_pieces(board, board.side)
            {
                // A PROFUNDIDADE REDUZIDA, e nao a nominal.
                //
                // Antes de decidir podar, calcula-se a que profundidade o lance
                // VAI mesmo ser procurado -- a nominal menos a reducao -- e e'
                // com essa que se julga. Um lance que vai levar tres plies de
                // corte e' julgado com as margens de um no' tres plies mais
                // raso, que sao muito mais apertadas, e cai. Antes julgavamos
                // como se fosse procurado a` profundidade toda, e sobrevivia.
                //
                // Dois motores independentes fazem-no, e e' a diferenca que
                // explica os quatro para um: a` profundidade 12, com a MESMA
                // rede dos dois lados, eles chegam la' com 77.897 nos e nos com
                // 388.850.
                //
                // A estimativa sai da tabela de reducao, que e' o termo
                // dominante e o unico disponivel neste ponto -- os ajustes
                // finais so' aparecem depois das extensoes.
                let prof_poda = if self.features.poda_reduzida {
                    let r_est = if depth >= 3 && is_quiet && !in_check {
                        (self.lmr[(depth as usize).min(63)][i.min(63)] / 1024)
                            .clamp(0, (depth - 1).max(0))
                    } else {
                        0
                    };
                    // O historico DEVOLVE profundidade. Sem isto podamos os
                    // lances que o historico prefere com o mesmo rigor que os
                    // que ele despreza, e sao justamente esses que mais custa
                    // deitar fora -- o que explica bem os 30 a 52 Elo que as
                    // quatro doses anteriores perderam.
                    //
                    // A forma e' a deles (`lmrDepth += history / divisor`), a
                    // escala e' nossa: o divisor fica em parametro porque o
                    // nosso historico nao tem de ter a mesma amplitude que o
                    // deles, e supor que tem ja' nos custou uma vez.
                    let base = depth - 1 - r_est;
                    let p = if self.features.poda_hist {
                        base + hist[i] / self.params.poda_hist_div.max(1)
                    } else {
                        base
                    };
                    p.max(0)
                } else {
                    depth
                };

                // ================= A PODA DELES, INTEIRA =================
                //
                // Sete tentativas anteriores falharam todas pela mesma razao:
                // levavam PECAS. A profundidade reduzida sem o resto (-38,7 a
                // -52), a margem escalada sem mexer no `r` (-30 a -33), e a
                // linha do historico transplantada sozinha (-54 a -63). A
                // conclusao que tirei foi que uma peca solta nao reconstroi o
                // conjunto; a resposta e' levar o conjunto.
                //
                // Aqui esta' o Step 15 deles tal como e', com a mesma ordem, os
                // mesmos ramos e as mesmas FORMAS -- linear na futilidade dos
                // tranquilos, linear com valor da vitima nas capturas,
                // QUADRATICA no SEE. E, sobretudo, com a mesma moeda comum: um
                // `lmr_depth` unico que nasce da reducao a serio e que os tres
                // testes leem.
                //
                // O que NAO se importa sao as escalas de historia. As
                // constantes deles (-4136, o divisor ~3000) sao para somas que
                // vao a ~105000; as nossas tabelas topam em 15000 principal,
                // 30000 continuacao. Ja' nos custou um teste morto -- uma
                // condicao que nunca disparou e devolveu contagens de nos
                // identicas ao byte. Por isso ficam em parametro.
                if self.features.poda_sf {
                    // A moeda comum. O `r` deles ja' inclui o termo do ttPv
                    // ANTES da poda; o nosso vinha so' da tabela crua.
                    let mut r1024 = self.lmr[(depth as usize).min(63)][i.min(63)];
                    // Sem o `ttpv_lmr` por cima: dentro do `poda_sf` o termo do
                    // ttPv nao e' uma ideia em prova, e' parte da forma que
                    // fomos buscar. Com a guarda anterior nunca disparava --
                    // `ttpv_lmr` esta' a false por omissao e o `TtpvLmr` nem
                    // esta' na EXTRA, logo nem e' anunciado no `uci`. Medido:
                    // `PodaSfTtpv` no minimo e no maximo davam os MESMOS 148635
                    // nos. Quem quiser o `r` sem este termo poe `PodaSfTtpv=0`,
                    // que e' para isso que ele e' parametro.
                    if tt_pv {
                        r1024 += self.params.poda_sf_ttpv;
                    }
                    let new_depth = depth - 1;
                    let mut lmr_depth = new_depth - r1024 / 1024;
                    // Uma vez por lance, como eles. Um xeque vai pelo ramo das
                    // capturas mesmo sendo tranquilo: e' forcante, e julga-lo
                    // com as margens dos tranquilos deita fora linhas.
                    let gives_check = self.da_xeque(board, &mv);
                    // O `hist[i]` e' ZERO para toda a captura por construcao
                    // (ver o `return 0` no ramo `is_capture()` de onde ele sai),
                    // portanto os dois termos de historia deste bloco eram
                    // estruturalmente nulos: `PodaSfCaptHist` no minimo e no
                    // maximo davam os mesmos 148635 nos, e o `* 34 / 1024` da
                    // margem do SEE tambem nunca somou nada. O SF le' aqui o
                    // historico DE CAPTURAS, que ja' existe na arvore.
                    //
                    // NOTA: so' tem valores com `CaptureHist=true` -- e' esse
                    // interruptor que alimenta a tabela (`credit_capture` sai
                    // cedo sem ele). Testar o `PodaSF` sem ele volta a medir a
                    // forma incompleta.
                    let capt_h = if !is_quiet { self.capt_score(board, &mv) } else { 0 };

                    if !is_quiet || gives_check {
                        // --- capturas e xeques ---
                        if !gives_check && lmr_depth < 8 {
                            let vitima = if mv.flag == MoveFlag::EnPassant {
                                PieceType::Pawn.value()
                            } else {
                                board.piece_at(mv.to).map(|(pt, _)| pt.value()).unwrap_or(0)
                            };
                            if static_eval
                                + self.params.poda_sf_capt_base
                                + self.params.poda_sf_capt_slope * lmr_depth
                                + vitima
                                + self.params.poda_sf_capt_hist * capt_h / 1024
                                <= alpha
                            {
                                continue;
                            }
                        }
                        // SEE das capturas, com a margem a crescer com a
                        // profundidade NOMINAL -- nao com a reduzida.
                        let margem = self.params.poda_sf_capt_see * depth
                            + capt_h * 34 / 1024;
                        if !see::see_ge(&self.atk, board, &mv, -margem) {
                            continue;
                        }
                    } else if !pv_node {
                        // `!pv_node`: nao se poda tranquilo nenhum num no' de
                        // PV. E' a linha principal, a que a busca esta' a
                        // defender, e uma margem que falha ali deita fora a
                        // variante em vez de uma sub-arvore lateral.
                        //
                        // Mal se ve' na contagem de nos -- os nos de PV sao
                        // poucos -- mas muda o que a busca guarda, que e' onde
                        // isto se paga.
                        // --- tranquilos ---
                        // Poda pelo historico de continuacao, ANTES de tudo o
                        // resto: um lance que as tabelas desprezam ha' muito
                        // nao merece o no'.
                        if hist[i] < -self.params.poda_sf_cont_prune * depth {
                            continue;
                        }
                        // O historico DEVOLVE profundidade. Sozinha esta linha
                        // deu -54 Elo; aqui vem com o resto do conjunto.
                        let devolucao = hist[i] / self.params.poda_sf_div.max(1);
                        lmr_depth += devolucao;
                        conta_lmrd(lmr_depth, devolucao);

                        if !in_check
                            && lmr_depth < 12
                            && static_eval
                                + self.params.poda_sf_fut_slope * lmr_depth
                                + if static_eval > alpha { 90 } else { 0 }
                                + self.params.poda_sf_fut_base
                                <= alpha
                        {
                            // `continue` e mais nada: poda ESTE lance e nao
                            // os que vem a seguir.
                            //
                            // Aqui estava `skip_quiets = true`, que parava
                            // todos os tranquilos que faltavam no no' a`
                            // PRIMEIRA margem que falhasse. A ordem nao e'
                            // monotona na futilidade -- um lance que falha a
                            // margem nao diz nada sobre o seguinte -- por isso
                            // isso deitava fora lances por associacao.
                            //
                            // MEDIDO em partidas, mesmas condicoes (8+0.08,
                            // UHO, aberturas iguais, o remendo em primeiro):
                            //
                            //     com `skip_quiets`   563 partidas  -17,4 Elo
                            //     com `continue`      605 partidas   -4,1 Elo
                            //
                            // Treze Elo por uma palavra. Parar por contagem de
                            // lances e' outra coisa e tem o seu proprio sitio;
                            // parar por uma margem falhada nao se justifica.
                            continue;
                        }

                        lmr_depth = lmr_depth.max(0);

                        // QUADRATICA. Um multiplicador unico sobre a nossa
                        // margem linear nao imita isto, e era isso que as
                        // tentativas anteriores faziam.
                        if !see::see_ge(
                            &self.atk,
                            board,
                            &mv,
                            -self.params.poda_sf_see * lmr_depth * lmr_depth,
                        ) {
                            continue;
                        }
                    }
                } else {
                    if is_quiet {
                        // Late move pruning: past a certain count at low depth,
                        // the ordering has been wrong often enough that the rest
                        // are not worth the nodes.
                        //
                        // It stops the QUIETS, not the loop. Losing captures score
                        // below every quiet move and are therefore last in the
                        // list, so breaking here threw all of them away as well --
                        // a rule about quiet moves silently deleting captures.
                        let full = self.params.lmp_base + depth * depth;
                        let count = if !self.features.lmp_improving || improving {
                            full
                        } else {
                            full / 2
                        };
                        // O lance da tabela vindo de um limite superior nao
                        // gasta um lugar: ele foi posto a` frente sem o ter
                        // ganho, e sem isto empurra para fora da poda um lance
                        // que estava na fronteira.
                        let indice = if self.features.tt_sem_lmp
                            && tt_bound == Bound::Upper
                            && tt_move.is_some()
                            && i > 0
                        {
                            i - 1
                        } else {
                            i
                        };
                        if depth <= self.params.lmp_depth && indice >= count as usize {
                            skip_quiets = true;
                            continue;
                        }

                        // History pruning. A quiet move the tables have disliked
                        // this consistently, at a depth this shallow, is not worth
                        // the node. The threshold grows with the square of the
                        // depth so that it only bites where being wrong is cheap.
                        //
                        // The constant is in OUR history units and had to be. Taken
                        // straight from a design whose tables run to about
                        // 105000, against ours that cap near 24500, it never once
                        // fired -- the two runs came back with byte-identical node
                        // counts, which is what a dead branch looks like from
                        // outside.

                        if self.features.history_prune
                            && prof_poda <= 4
                            && hist[i] < -self.params.hist_prune * prof_poda * prof_poda
                        {
                            continue;
                        }

                        // Futility: even handed the margin, this move does not
                        // reach alpha, and a quiet move does not change the
                        // material to make up the difference. The history term
                        // belongs here: a move the tables like is worth trying even
                        // when the margin says otherwise, and one they dislike is
                        // worth less than the margin suggests. Its divisor is in
                        // OUR history units, which run about five and a half times
                        // smaller.
                        //
                        // This one stopped the loop too. Quiets are ordered by
                        // history, so a later quiet does fail the same test -- but
                        // the captures behind them do not, and were going with it.
                        let hist_term = hist[i] / self.params.fut_hist_div.max(1);
                        // A inclinacao sobe quando a profundidade desce.
                        //
                        // O erro da primeira tentativa: troquei `depth` por
                        // `prof_poda` e deixei os coeficientes como estavam. As
                        // margens deles sao feitas PARA a profundidade reduzida --
                        // 119 por ply nos tranquilos, 234 + 247 por ply nas capturas
                        // -- e as nossas foram afinadas para a nominal. Meter um
                        // numero menor na mesma formula encolhe a margem, e a poda
                        // passou a cortar o que nao devia: -38,7 Elo em 892
                        // partidas.
                        //
                        // `FutSlopeRed` em percentagem: 100 deixa como esta', 200
                        // duplica a inclinacao para compensar uma profundidade que
                        // fica tipicamente a metade.
                        let inclin = if self.features.poda_reduzida {
                            self.params.fut_slope * self.params.fut_slope_red / 100
                        } else {
                            self.params.fut_slope
                        };
                        if prof_poda <= self.params.fut_depth
                            && static_eval
                                + self.params.fut_base
                                + inclin * prof_poda
                                + hist_term
                                <= alpha
                        {
                            skip_quiets = true;
                            continue;
                        }

                        // A quiet move can still lose material -- walking a piece
                        // onto a square where it is taken for nothing. Static
                        // exchange says so before the search has to find out, and
                        // it is asked last because it is the dearest question here.
                        if prof_poda <= 8
                            && !see::see_ge(
                                &self.atk,
                                board,
                                &mv,
                                -(self.params.see_prune_quiet * self.params.see_quiet_red / 100)
                                    * (prof_poda + prof_poda * prof_poda),
                            )
                        {
                            continue;
                        }
                    } else {
                        // FUTILIDADE PARA CAPTURAS.
                        //
                        // Nao a tinhamos de todo: a futilidade so' se aplicava a
                        // lances tranquilos, e uma captura que nao chega perto de
                        // alpha era procurada na mesma.
                        //
                        // Os dois motores que li fazem-no, e o cinder de forma
                        // elegante: em vez de escrever um caso a` parte para
                        // capturas, ele aplica UM teste a todos os lances e desconta
                        // o que o lance GANHA -- `!pos.gaining(m, margem)`. Uma
                        // captura que traz material suficiente sobrevive; uma que
                        // nao traz cai como qualquer tranquilo.
                        //
                        // Aqui o ganho e' o valor da peca comida, que e' o limite
                        // superior do que a captura pode trazer.
                        if self.features.fut_capturas && !in_check && prof_poda < 8 {
                            let vitima = if mv.flag == MoveFlag::EnPassant {
                                PieceType::Pawn.value()
                            } else {
                                board.piece_at(mv.to).map(|(pt, _)| pt.value()).unwrap_or(0)
                            };
                            let ganho = vitima
                                + mv.promotion
                                    .map(|p| p.value() - PieceType::Pawn.value())
                                    .unwrap_or(0);
                            if static_eval
                                + self.params.fut_capt_base
                                + self.params.fut_slope * prof_poda
                                + ganho
                                <= alpha
                            {
                                continue;
                            }
                        }
                        if depth <= 8
                            && !see::see_ge(&self.atk, board, &mv, -self.params.see_prune * depth)
                        {
                            // A capture that loses more than the depth could plausibly
                            // win back.
                            continue;
                        }
                    }
                }
            }

            // Singular extension. If the table says this move is good enough to
            // fail high, search every OTHER move against a window just below
            // that. If they all fall short, this move is the only one holding
            // the position up, and a line that hangs on one move deserves
            // another ply to be sure of it.
            if !root
                && excluded.is_none()
                && Some(mv) == tt_move
                && depth >= self.params.sing_depth
                && ply < MAX_PLY - 8
            {
                if let Some(ts) = tt_score_for_singular {
                    let e = entry.unwrap();
                    if e.depth >= depth - 3
                        // A referencia so' testa BOUND_LOWER; nos aceitamos
                        // tambem os exactos, o que abre um conjunto muito maior
                        // de sondagens. Do KestrelStrike, recuperado a 20-09.
                        && (matches!(e.bound, Bound::Lower)
                            || (!self.features.sing_so_inferior
                                && matches!(e.bound, Bound::Exact)))
                        && !is_mate(ts)
                    {
                        let target = ts - self.params.sing_margin * depth;
                        self.excluded[ply] = Some(mv);
                        let s = self.negamax(
                            board,
                            (depth - 1) / 2,
                            target - 1,
                            target,
                            ply,
                            false,
                            cut_node,
                        );
                        self.excluded[ply] = None;
                        if self.stopped {
                            return 0;
                        }
                        if s < target {
                            extension = 1;
                            // Not merely singular but singular by a distance:
                            // every alternative fell a long way short, so the
                            // line is even narrower than one ply of extension
                            // says. Outside the principal variation only, where
                            // being wrong costs a subtree rather than the move
                            // we play.
                            if !pv_node && s < target - self.params.double_ext {
                                extension = 2;
                            }
                        } else if target >= beta {
                            // Every other move also beats beta, so the position
                            // is winning for reasons that do not depend on this
                            // one and the whole subtree can go.
                            return target;
                        } else if !pv_node && !is_mate(s) && s >= beta {
                            return s;
                        } else if ts >= beta {
                            // The table says this move fails high, and the
                            // search just said it is not the only one that
                            // does. A node with several good answers is the
                            // opposite of the case worth extending, so take a
                            // ply off rather than adding one.
                            extension = -1;
                        }
                    }
                }
            }

            let nodes_before = self.nodes;
            self.played[ply] = board
                .piece_at(mv.from)
                .map(|(pt, _)| (pt.idx(), mv.to as usize));
            let undo = board.make_move(&mv);
            crate::ponte::lance(&mv);
            // Ask for the child's entry now. The probe happens a function call
            // and a check detection later, which is enough to cover part of the
            // trip to memory -- and that trip was a fifth of the whole search.
            self.tt.prefetch(board.hash);
            self.keys.push(board.hash);

            // A move that gives check is forcing: the reply is constrained and
            // the line is worth another ply. Only while the score says the game
            // is still a contest, since a check in a decided position extends
            // something that changes nothing.
            if self.features.check_ext
                && board.in_check(board.side, &self.atk)
                && static_eval != TT_EVAL_NONE as i32
                && static_eval.abs() > self.params.check_ext_eval
            {
                extension = extension.max(1);
            }

            let new_depth = depth - 1 + extension;

            let mut did_lmr = false;
            let mut searched_again = false;
            let mut score;
            if i == 0 {
                // The first move of a principal variation node leads to another
                // one; anywhere else the child expects the opposite of us.
                let child_cut = if pv_node { false } else { !cut_node };
                score =
                    -self.negamax(board, new_depth, -beta, -alpha, ply + 1, pv_node, child_cut);
            } else {
                // Late move reductions: the ordering has already put the moves
                // most likely to be best first, so the ones at the back are
                // searched shallower until one of them proves otherwise.
                // Late captures are reduced too, outside the principal
                // variation. A capture is not automatically worth a full look
                // just for being a capture -- the ones that were worth it are
                // already at the front of the list, and the ones down here have
                // been sorted below quiet moves by static exchange for a
                // reason.
                // Nothing is reduced once the board is nearly empty.
                //
                // Off by default, and the reason is worth writing down because
                // the idea sounded right and the measurement said otherwise.
                // It was put in to fix a real fault: king and rook against
                // king, four seconds, and the engine reports five and a half
                // pawns rather than the mate. Switching reductions off in that
                // position took the depth from nineteen to thirteen and left
                // the score where it was.
                //
                // Which located the fault somewhere else. That mate is up to
                // sixteen moves away -- thirty-two plies -- and no depth this
                // search reaches can prove it. The engine would have to be
                // driven there by the evaluation, and the network gives it the
                // value of a rook without distinguishing a king pinned to the
                // edge from one standing in the middle. It is an evaluation
                // that cannot restrict a king, not a search that reduces the
                // move which would. Tablebases are the answer to that, which is
                // why every strong engine carries them for these endings.
                let poucas_pecas =
                    board.occ_all.count_ones() <= self.params.lmr_endgame_pieces as u32;
                let reducible = !poucas_pecas
                    && (is_quiet
                        || (self.features.lmr_captures && !pv_node && depth >= 3));

                did_lmr = true;
                let mut r = 0;
                if self.features.log_lmr {
                    // The other shape, whole. Integer logarithms of the
                    // move number and the depth, a ply back for anything
                    // tactical or on the principal variation, the history
                    // divided by the ceiling one table reaches, two plies at a
                    // node expected to fail high, and one always.
                    //
                    // Our history tables were rebuilt to its scale earlier, so
                    // the divisor transfers without conversion.
                    if depth > 2 && i >= 1 + 2 * root as usize && (!pv_node || is_quiet) {
                        let n = i as i32 + 1;
                        r = ilog2i(n) / 2 + ilog2i(depth) / 2
                            - (!is_quiet || pv_node) as i32
                            - (hist[i] + 15000) / 30000
                            + 2 * cut_node as i32
                            + 1;
                        r = r.clamp(0, (new_depth - 1).max(0));
                    }
                } else if depth >= 3 && reducible && !in_check {
                    // Accumulated in 1024ths and divided at the end, so a term
                    // can be worth a third of a ply instead of all or nothing.
                    // The first version added whole plies, and the cut node term
                    // alone was two of them where a third of one is the right
                    // size. Five times too much, which is exactly why measuring
                    // it found it doing no good.
                    // A tabela ja' vem em milesimos.
                    // A MESMA grandeza que as podas leem. Ver `r_partilhado`.
                    let r1024 = self.r_partilhado(
                        depth, i, alpha, beta, improving, is_quiet, cut_node,
                        pv_node, tt_pv, tt_move, hist[i],
                        self.cut_cnt[ply + 1], true,
                    );
                    r = (r1024 / 1024).clamp(0, (new_depth - 1).max(0));
                }
                // A reduced scout search is looking for a reason to stop, so
                // the child is treated as expecting to fail high.
                conta_reb(0, 1);
                score =
                    -self.negamax(board, new_depth - r, -alpha - 1, -alpha, ply + 1, false, true);
                if score > alpha && r > 0 {
                    if est_ligado() { FORMA[4].fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
                    // A reducao estava errada: o lance merecia a profundidade
                    // inteira e o trabalho reduzido foi deitado fora.
                    searched_again = true;
                    let antes = self.nodes;
                    conta_reb(1, 1);
                    score = -self.negamax(
                        board,
                        new_depth,
                        -alpha - 1,
                        -alpha,
                        ply + 1,
                        false,
                        !cut_node,
                    );
                    conta_reb(3, self.nodes - antes);
                }
                if score > alpha && score < beta {
                    if est_ligado() { FORMA[5].fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
                    // A janela nula nao chegou: e' preciso o valor exacto, e a
                    // busca anterior so' deu um limite.
                    let antes = self.nodes;
                    conta_reb(2, 1);
                    score = -self.negamax(board, new_depth, -beta, -alpha, ply + 1, true, false);
                    conta_reb(3, self.nodes - antes);
                }
            }

            self.keys.pop();
            board.unmake_move(&mv, &undo);
            crate::ponte::desfaz(&mv);

            if root {
                self.root_effort.push((mv, self.nodes - nodes_before));
                // A move searched with a null window that failed low has no
                // real score, only a bound. Recorded anyway: it is still
                // evidence of the ordering, and the fallback below only ever
                // looks at moves close to the best, which fail-lows are not.
                self.root_scores.push((mv, score));
            }

            // A quiet move that was reduced and then had to be searched again
            // has told us something either way: it was worth the second look,
            // or it was not. Both are worth recording, and neither shows up in
            // the cutoff update, which only ever sees the move that ended the
            // node.
            if did_lmr && searched_again && is_quiet {
                let credit = if score > best_score {
                    hist_bonus(depth)
                } else {
                    -hist_bonus(depth)
                };
                let side = board.side.idx();
                let slots = self.cont_slots(ply);
                self.credit(board, mv, side, &slots, credit);
            }

            if self.stopped {
                return 0;
            }

            // At the root, and only there, give up on an iteration that has
            // already gone well past what was planned for the whole move. The
            // soft limit is otherwise consulted only between iterations, so a
            // single long one sails past it and the wall is all that catches
            // it -- much later, and much more expensively.
            //
            // Safe to stop here because a root move that has finished has a
            // real score: the best so far is a genuine best-so-far, not
            // whatever happened to be first.
            if root && i > 0 && self.start.elapsed() >= self.soft * 2 {
                self.stopped = true;
            }

            if score > best_score {
                best_score = score;
                best_move = Some(mv);

                if score > alpha {
                    alpha = score;
                    self.update_pv(ply, mv);

                    // DESCONTO DE PROFUNDIDADE DEPOIS DE O ALPHA SUBIR.
                    //
                    // Do KestrelStrike, recuperado a 20-09. Assim que um lance
                    // sobe o alpha, este no' ja' tem um melhor a serio. Os
                    // lances que vem a seguir nao tem de provar que sao bons em
                    // absoluto -- so' tem de bater ESSE. Procura-los a`
                    // profundidade inteira e' pagar por uma prova que ja' nao e'
                    // precisa.
                    //
                    // A janela existe para o desconto so' agir onde paga: em
                    // cima a nota ainda salta e descontar cega a raiz; em baixo
                    // ja' nao ha' profundidade que valha a pena descontar. E
                    // nunca em cima de um mate, onde a nota nao e' uma medida
                    // de quanto.
                    if ply > 0
                        && self.params.alpha_desc > 0
                        && depth > self.params.ad_min
                        && depth < self.params.ad_max
                        && !is_mate(score)
                    {
                        depth -= self.params.alpha_desc;
                    }

                    if alpha >= beta {
                        // Actualizacoes pequenas e pouco frequentes escalam
                        // bem: so' conta quando NAO houve extensao dupla, ou em
                        // no' de PV.
                        if extension < 2 || pv_node {
                            self.cut_cnt[ply] += 1;
                        }
                        if is_quiet {
                            self.on_beta_cutoff(board, mv, ply, depth, &searched_quiets);
                        } else {
                            self.credit_capture(board, mv, capt_bonus(depth));
                        }
                        // Captures tried and passed over were wrong here
                        // whatever ended the node, quiet or not.
                        for c in &searched_captures {
                            if *c != mv {
                                self.credit_capture(board, *c, -capt_bonus(depth));
                            }
                        }
                        // Em que lance o corte aconteceu. E' aqui que a
                        // ordenacao se mede: cada corte abaixo do primeiro e'
                        // uma sub-arvore percorrida para nada.
                        conta_corte(i);
                        if ttb_ligado() && i == 0 && Some(mv) == tt_move {
                            use std::sync::atomic::Ordering::Relaxed;
                            let b = match tt_bound {
                                Bound::Exact => 1,
                                Bound::Lower => 3,
                                Bound::Upper => 5,
                                Bound::NoBound => 9,
                            };
                            if b < 6 {
                                TTB[b].fetch_add(1, Relaxed);
                            }
                        }
                        conta_tipos(i, tipo_mv, tipo_primeiro);
                        conta_cauda(i, tipo_primeiro, tt_move.is_some());
                        if est_ligado() && i == 0 {
                            FORMA[3].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        if let Some(k) = slot_primeiro {
                            conta_killer(k, i == 0);
                        }
                        break;
                    }
                }
            }

            if is_quiet {
                searched_quiets.push(mv);
            } else {
                searched_captures.push(mv);
            }
        }

        if best_score == -INF {
            if !algum_lance {
                return if in_check { mate_score(ply) } else { 0 };
            }
            return alpha;
        }

        // Learn the correction from what the search ended up saying, but only
        // where it contradicts the static score in a direction worth trusting:
        // below it and below beta, so there is an upper bound proving the
        // static score was optimistic, or above it with a move to show why.
        // Anywhere else the number is the product of a cutoff, not a reading of
        // the position. Captures are excluded: there the jump comes from
        // material rather than from misreading, and it would teach the table
        // the wrong thing.
        if self.features.corr_hist
            && !in_check
            && best_score.abs() < MATE_IN_MAX
            && !best_move.map_or(false, |m| m.is_capture())
            && ((best_score < static_eval && best_score < beta)
                || (best_score > static_eval && best_move.is_some()))
        {
            self.learn_correction(board, best_score - static_eval, depth, ply);
        }

        let bound = if best_score >= beta {
            Bound::Lower
        } else if best_score > alpha_orig {
            Bound::Exact
        } else {
            Bound::Upper
        };
        let se = if in_check {
            TT_EVAL_NONE
        } else {
            raw_static_eval as i16
        };
        if excluded.is_none() {
            self.tt.store(
                board.hash,
                depth,
                score_to_tt(best_score, ply),
                bound,
                best_move,
                tt_pv,
                se,
            );
        }

        best_score
    }

    fn quiescence(&mut self, board: &mut Board, mut alpha: i32, beta: i32, ply: usize) -> i32 {
        self.nodes += 1;
        if est_ligado() { FORMA[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
        if self.out_of_time() {
            return 0;
        }
        if ply >= MAX_PLY - 1 {
            return evaluate(board, self.features.rule50_fade);
        }
        if self.is_draw(board) {
            return self.draw_score(ply);
        }

        let in_check = board.in_check(board.side, &self.atk);
        let alpha_orig = alpha;

        // The table is worth reading here too. Quiescence is most of the tree,
        // the same capture sequences transpose constantly, and every hit that
        // returns saves not just a node but the whole tail behind it.
        let entry = self.tt.probe(board.hash);
        let mut tt_move = None;
        if let Some(e) = entry {
            tt_move = e.best;
            if e.has_bound() {
                let sc = score_from_tt(e.score, ply);
                let usable = match e.bound {
                    Bound::Exact => true,
                    Bound::Lower => sc >= beta,
                    Bound::Upper => sc <= alpha,
                    Bound::NoBound => false,
                };
                if usable {
                    return sc;
                }
            }
        }

        // Standing pat: the side to move is not obliged to capture, so the
        // static score is a floor. Not while in check, where every move is
        // forced and there is nothing to stand on.
        let mut static_eval = TT_EVAL_NONE as i32;
        let mut stand = TT_EVAL_NONE as i32;
        if !in_check {
            static_eval = match entry {
                Some(e) if e.static_eval != TT_EVAL_NONE => e.static_eval as i32,
                _ => evaluate(board, self.features.rule50_fade),
            };

            // The floor to stand on is the better of the static score and
            // whatever the table already established, for the same reason the
            // full search prefers it: a stored bound on the right side of the
            // static score came from a search that went and found out. Standing
            // on the worse number means searching captures to reach a value
            // already in hand.
            let mut floor = static_eval;
            if let Some(e) = entry {
                if e.has_bound() {
                    let ts = score_from_tt(e.score, ply);
                    let better = match e.bound {
                        Bound::Exact => true,
                        Bound::Lower => ts > static_eval,
                        Bound::Upper => ts < static_eval,
                        Bound::NoBound => false,
                    };
                    if better {
                        floor = ts;
                    }
                }
            }
            if floor >= beta {
                return floor;
            }
            if floor > alpha {
                alpha = floor;
            }
            stand = floor;

        }

        let mut moves = if in_check {
            generate_legal(board, &self.atk)
        } else {
            generate_legal_caps(board, &self.atk)
        };
        if in_check && moves.is_empty() {
            return mate_score(ply);
        }
        let (mut scores, _) = self.score_moves(board, &moves, tt_move, Bound::NoBound, ply, 1);

        let mut best = if in_check { -INF } else { stand };
        let mut best_move = None;

        let mut hist_unused: Vec<i32> = vec![0; moves.len()];
        // A casa que o lance anterior acabou de ocupar. Uma captura ai' e' a
        // outra metade de uma troca ja' comecada e nao entra na conta.
        let casa_anterior = if ply > 0 {
            self.played[ply - 1].map(|(_, to)| to)
        } else {
            None
        };
        let mut vulgares = 0i32;

        for i in 0..moves.len() {
            Self::pick(&mut moves, &mut scores, &mut hist_unused, i);
            let mv = moves[i];

            // O travao. Nao reduz nem testa: salta. As isencoes sao o que o
            // torna seguro -- uma promocao muda a posicao por mais do que uma
            // contagem sabe julgar, e uma recaptura na casa que se acabou de
            // tomar e' a outra metade de uma troca. O que sobra e' a terceira,
            // quarta, quinta maneira de tomar alguma coisa, e a essa altura as
            // duas primeiras ja' disseram quanto o no' vale.
            if self.features.travao_qs
                && !in_check
                && mv.promotion.is_none()
                && Some(mv.to as usize) != casa_anterior
            {
                vulgares += 1;
                if vulgares > self.params.travao_qs_n {
                    continue;
                }
            }

            // The margin first, the exchange second.
            //
            // Both of these throw captures away and the order decides which one
            // gets to. The margin asks whether a capture could reach alpha even
            // if nothing were taken back; the exchange asks whether it loses
            // material. Running the exchange first hands it every capture the
            // margin would have dismissed for free, and static exchange is by
            // far the more expensive question.
            if self.features.qs_futility && !in_check && best.abs() < MATE_IN_MAX {
                // Promotions are exempt. What this test knows how to price is a
                // captured piece, and a promotion's value is not in what it
                // takes -- a pawn reaching the last rank changes the position by
                // more than the margin can express, and pruning it by material
                // is pruning it for the wrong reason.
                let captured = if mv.promotion.is_some() {
                    None
                } else if mv.flag == MoveFlag::EnPassant {
                    Some(PieceType::Pawn)
                } else {
                    board.piece_at(mv.to).map(|(pt, _)| pt)
                };

                // And there has to be something to take. A move with nothing on
                // the target square was being credited with a captured value of
                // zero and pruned on that basis, which is not a bound on
                // anything.
                if let Some(pt) = captured {
                    // Measured from the floor the node is standing on, which is
                    // what the capture has to beat -- not from the raw static
                    // score. Trying it the other way round was the first thing
                    // I changed here and it did not help, which was the clue
                    // that the fault was elsewhere.
                    let futile = stand + value_in_eval_units(pt) + self.params.qs_margin;
                    if stand != TT_EVAL_NONE as i32 && futile <= alpha {
                        // The value it could not beat is an honest lower bound
                        // on this node. The capture was not searched, but it was
                        // not refuted either, and if every remaining move is
                        // skipped the same way then this is the best the node
                        // can show for itself. Dropping it makes the node return
                        // less than it knows, and the underestimate travels up
                        // the tree and into the table.
                        if futile > best {
                            best = futile;
                        }
                        continue;
                    }
                }
            }

            // A capture that loses material cannot raise the floor we are
            // already standing on, and following it is how quiescence chases
            // every recapture to the horizon instead of settling.
            if !in_check && !see::see_ge(&self.atk, board, &mv, 0) {
                continue;
            }

            let undo = board.make_move(&mv);
            crate::ponte::lance(&mv);
            self.tt.prefetch(board.hash);
            self.keys.push(board.hash);
            let score = -self.quiescence(board, -beta, -alpha, ply + 1);
            self.keys.pop();
            board.unmake_move(&mv, &undo);
            crate::ponte::desfaz(&mv);

            if self.stopped {
                return 0;
            }
            if score > best {
                best = score;
                if score > alpha {
                    alpha = score;
                    best_move = Some(mv);
                    if alpha >= beta {
                        break;
                    }
                }
            }
        }

        let bound = if best >= beta {
            Bound::Lower
        } else if best > alpha_orig {
            Bound::Exact
        } else {
            Bound::Upper
        };
        self.tt.store(
            board.hash,
            0,
            score_to_tt(best, ply),
            bound,
            best_move,
            false,
            if in_check { TT_EVAL_NONE } else { static_eval as i16 },
        );

        best
    }

    pub(crate) fn update_pv_pub(&mut self, ply: usize, mv: Move) {
        self.update_pv(ply, mv);
    }

    fn update_pv(&mut self, ply: usize, mv: Move) {
        self.pv[ply][0] = Some(mv);
        let child = self.pv_len[ply + 1].min(MAX_PLY - ply - 2);
        for i in 0..child {
            self.pv[ply][i + 1] = self.pv[ply + 1][i];
        }
        self.pv_len[ply] = child + 1;
    }

    fn on_beta_cutoff(
        &mut self,
        board: &Board,
        mv: Move,
        ply: usize,
        depth: i32,
        searched: &[Move],
    ) {
        if !self.killers[ply].iter().any(|k| *k == Some(mv)) {
            for i in (1..NUM_KILLERS).rev() {
                self.killers[ply][i] = self.killers[ply][i - 1];
            }
            self.killers[ply][0] = Some(mv);
        }
        let side = board.side.idx();
        let bonus = hist_bonus(depth);
        let slots = self.cont_slots(ply);

        self.credit(board, mv, side, &slots, bonus);
        for q in searched {
            if *q != mv {
                self.credit(board, *q, side, &slots, -bonus);
            }
        }
    }

    /// O `r` PARTILHADO -- uma so' grandeza que a reducao e as podas leem.
    ///
    /// Existe por arrumacao, nao por mudanca: o calculo da reducao estava
    /// escrito por extenso no meio do ciclo de lances e passou para aqui sem
    /// uma virgula de diferenca.
    ///
    /// Verificado como se verifica uma arrumacao: a base da' **369473 nos**
    /// antes e depois, ao no', nas quatro posicoes de referencia a`
    /// profundidade 12. Se alguem mexer nos termos, e' esse numero que tem de
    /// mudar de proposito e nao por acidente.
    ///
    /// Fica aqui tambem para o dia em que se quiser experimentar podar pela
    /// MESMA grandeza com que se reduz -- hoje sao duas contas diferentes, e
    /// essa e' uma experiencia por fazer, nao um defeito.
    ///
    /// `contar` fica falso na chamada da poda para nao poluir os contadores do
    /// `quem`, que foram medidos com o caminho da reducao.
    #[allow(clippy::too_many_arguments)]
    fn r_partilhado(
        &self,
        depth: i32,
        i: usize,
        alpha: i32,
        beta: i32,
        improving: bool,
        is_quiet: bool,
        cut_node: bool,
        pv_node: bool,
        tt_pv: bool,
        tt_move: Option<Move>,
        hist_i: i32,
        // Quantas vezes o ply SEGUINTE ja' cortou. Ver `cut_cnt_base`.
        cortes_filho: i32,
        contar: bool,
    ) -> i32 {
        let cq = |k: usize, d: i32| if contar { conta_quem(k, d) };
        let mut r1024 = if self.features.lmr_fino {
            self.lmr[(depth as usize).min(63)][i.min(63)]
        } else {
            (self.lmr[(depth as usize).min(63)][i.min(63)] / 1024) * 1024
        };
        cq(0, r1024);
        // O ply SEGUINTE ja' cortou muitas vezes: no' facil, reduz-se mais.
        //
        // Do KestrelStrike, recuperado a 20-09. Os valores da referencia sao
        // 264 e 1095 em 1024 avos; aqui a grandeza ja' e' a mesma, portanto
        // entram tal e qual. Omissao 0 -- quem decide sao as partidas.
        if cortes_filho > 1 {
            r1024 += self.params.cut_cnt_base
                + self.params.cut_cnt_mais * i32::from(cortes_filho > 2);
        }
        if self.features.lmr_janela {
            let d = (beta - alpha).max(0).min(self.root_delta);
            let x = d * self.params.lmr_janela_f / self.root_delta.max(1);
            r1024 -= x;
            cq(1, -x);
        }
        if self.features.lmr_piora && !improving {
            let x = r1024 * self.params.lmr_piora_f / 512;
            r1024 += x;
            cq(2, x);
        }
        if !is_quiet {
            let x = r1024 / 2;
            r1024 -= x;
            cq(3, -x);
        }
        if self.features.cut_node_lmr && cut_node {
            // Reduz mais num no' de corte, e mais ainda se nem lance da tabela
            // houver. Ver a nota do `lmr_cut_sem_tt`: medido, e nao presta.
            let x = self.params.lmr_cut_f
                + if tt_move.is_none() { self.params.lmr_cut_sem_tt } else { 0 };
            r1024 += x;
            cq(4, x);
        }
        if self.features.lmr_tt_captura
            && tt_move.map_or(false, |m| m.is_capture() || m.promotion.is_some())
        {
            r1024 += self.params.lmr_tt_capt_f;
            cq(8, self.params.lmr_tt_capt_f);
        }
        if !pv_node {
            r1024 += self.params.lmr_nonpv_f;
            cq(5, self.params.lmr_nonpv_f);
        }
        if self.features.ttpv_lmr && tt_pv {
            r1024 -= self.params.lmr_ttpv_f;
            cq(6, -self.params.lmr_ttpv_f);
        }
        let (dv, tecto) = if self.features.lmr_hist_forte {
            (self.params.lmr_hist_div_forte.max(1), 4096)
        } else {
            (self.params.lmr_hist_div.max(1), 2048)
        };
        let x = (hist_i * 1024 / dv).clamp(-tecto, tecto);
        r1024 -= x;
        cq(7, -x);
        r1024
    }

    /// What a capture is worth by past results, zero when the table is off.
    #[inline]
    fn capt_score(&self, board: &Board, mv: &Move) -> i32 {
        if !self.features.capture_hist {
            return 0;
        }
        let pc = match board.piece_at(mv.from) {
            Some((pt, _)) => pt.idx(),
            None => return 0,
        };
        let victim = if mv.flag == MoveFlag::EnPassant {
            PieceType::Pawn.idx()
        } else {
            match board.piece_at(mv.to) {
                Some((pt, _)) => pt.idx(),
                None => return 0,
            }
        };
        self.capthist[pc][mv.to as usize][victim]
    }

    /// Move a capture for or against by past results.
    #[inline]
    fn credit_capture(&mut self, board: &Board, mv: Move, bonus: i32) {
        if !self.features.capture_hist {
            return;
        }
        let pc = match board.piece_at(mv.from) {
            Some((pt, _)) => pt.idx(),
            None => return,
        };
        let victim = if mv.flag == MoveFlag::EnPassant {
            PieceType::Pawn.idx()
        } else {
            match board.piece_at(mv.to) {
                Some((pt, _)) => pt.idx(),
                None => return,
            }
        };
        hist_add(&mut self.capthist[pc][mv.to as usize][victim], bonus, HIST_MAX_CAPT);
    }

    /// Move one move for and every other move against, by the same amount.
    #[inline]
    fn credit(
        &mut self,
        board: &Board,
        mv: Move,
        side: usize,
        slots: &[Option<usize>; CONT_SLOTS],
        bonus: i32,
    ) {
        let b_ctx = self.balde_de(board, &mv);
        hist_add(
            &mut self.history[side][mv.from as usize][mv.to as usize][b_ctx],
            bonus,
            HIST_MAX_MAIN,
        );
        if let Some(pc) = board.piece_at(mv.from).map(|(pt, _)| pt.idx()) {
            if self.features.hist_pc {
                hist_add(&mut self.histpc[pc][mv.to as usize], bonus, HIST_MAX_PC);
            }
            for (k, slot) in slots.iter().enumerate() {
                if let Some(idx) = slot {
                    hist_add_at(
                        &self.conthist[ich(k, *idx, pc, mv.to as usize)],
                        bonus,
                        HIST_MAX_CONT,
                    );
                }
            }
            if self.features.hist_peao {
                let cp = self.chave_peao(board);
                hist_add_at(
                    &self.histpeao[ipeao(cp, pc + 6 * side, mv.to as usize)],
                    bonus,
                    HIST_MAX_CONT,
                );
            }
        }
    }

    /// Score every move once. The caller then takes the best remaining one at
    /// a time, because most nodes cut off after two or three and sorting the
    /// other thirty-seven is work thrown away.
    /// O que as tabelas acham de um lance tranquilo. E' a parte cara da
    /// pontuacao -- quatro consultas dispersas -- e a unica que vale a pena
    /// adiar.
    fn hist_de(
        &self,
        board: &Board,
        mv: &Move,
        side: usize,
        slots: &[Option<usize>; CONT_SLOTS],
        chave_peao: usize,
    ) -> i32 {
        if mv.is_capture() || mv.promotion.is_some() {
            return 0;
        }
        // Termo a termo igual ao caminho lento que isto substitui. Qualquer
        // diferenca aqui muda a arvore, e da primeira vez um termo a mais
        // custou exactamente isso.
        let mut h = self.history[side][mv.from as usize][mv.to as usize][self.balde_de(board, mv)];
        if let Some(pc) = board.piece_at(mv.from).map(|(pt, _)| pt.idx()) {
            for (k, slot) in slots.iter().enumerate() {
                if k >= 3 && !self.features.cont_longo {
                    continue;
                }
                if let Some(idx) = slot {
                    h += CONT_WEIGHT[k]
                        * self.conthist[ich(k, *idx, pc, mv.to as usize)].load(Ordering::Relaxed);
                }
            }
            if self.features.hist_peao {
                // A cor conta: doze pecas, nao seis.
                let pc12 = pc + 6 * side;
                h += self.params.peao_f
                    * self.histpeao[ipeao(chave_peao, pc12, mv.to as usize)]
                        .load(Ordering::Relaxed)
                    / 32;
            }
        }
        h
    }

    /// Aperta o tecto quando NAO ha' incremento. Ver `tm_sem_inc_tecto`.
    ///
    /// So' quando o incremento e' zero: com incremento o gasto e' reposto e a
    /// regra nao se aplica. Devolve o tecto tal e qual quando esta' desligada,
    /// para a omissao nao mudar nada.
    #[inline]
    fn aperta_sem_inc(&self, tecto: u64, relogio: u64, inc: u64) -> u64 {
        let pct = self.params.tm_sem_inc_tecto;
        if pct <= 0 || inc > 0 {
            return tecto;
        }
        tecto.min(relogio * pct as u64 / 100).max(1)
    }

    /// A chave da estrutura de peoes, reduzida ao tamanho da tabela.
    ///
    /// Uma vez por no', e nao por lance: e' a mesma para todos os lances da
    /// mesma posicao.
    #[inline]
    fn chave_peao(&self, board: &Board) -> usize {
        use Color::{Black, White};
        use PieceType::Pawn;
        (subset_key(board, &[White, Black], &[Pawn]) as usize) & (TAM_PEAO - 1)
    }

    /// Este lance da' xeque directo?
    ///
    /// O termo que o outro motor tem e nos nao. Directo apenas -- a peca que
    /// se move a atacar o rei da sua casa de destino -- que e' o que ele
    /// tambem faz (`gives_direct_check`). Xeques a` descoberta ficam de fora:
    /// apanha-los exigia refazer os raios todos e o termo deixaria de ser
    /// barato, que e' a unica razao para caber na ordenacao.
    #[inline]
    fn da_xeque(&self, board: &Board, mv: &Move) -> bool {
        let them = board.side.opp();
        let ksq = board.king_sq(them);
        let alvo = crate::bitboard::bb(ksq);
        let occ = (board.occ_all & !crate::bitboard::bb(mv.from))
            | crate::bitboard::bb(mv.to);
        let pt = match mv.promotion {
            Some(p) => p,
            None => match board.piece_at(mv.from) {
                Some((p, _)) => p,
                None => return false,
            },
        };
        match pt {
            PieceType::Pawn => {
                self.atk.pawn[board.side.idx()][mv.to as usize] & alvo != 0
            }
            PieceType::Knight => self.atk.knight[mv.to as usize] & alvo != 0,
            PieceType::Bishop => {
                crate::attacks::bishop_attacks(mv.to, occ) & alvo != 0
            }
            PieceType::Rook => crate::attacks::rook_attacks(mv.to, occ) & alvo != 0,
            PieceType::Queen => {
                (crate::attacks::bishop_attacks(mv.to, occ)
                    | crate::attacks::rook_attacks(mv.to, occ))
                    & alvo
                    != 0
            }
            PieceType::King => false,
        }
    }

    /// Em que balde do historico este lance cai, nesta posicao.
    #[inline]
    fn balde_de(&self, board: &Board, mv: &Move) -> usize {
        if !self.features.hist_contexto {
            return 0;
        }
        let i = self.keys.len().min(self.cache_ameacas.len() - 1);
        let (chave, mapa) = self.cache_ameacas[i].get();
        let mapa = if chave == board.hash && chave != 0 {
            mapa
        } else {
            let novo = todas_ameacas(board, board.side.opp(), &self.atk);
            self.cache_ameacas[i].set((board.hash, novo));
            novo
        };
        balde(mapa, mv)
    }

    fn score_moves(
        &self,
        board: &Board,
        moves: &[Move],
        tt_move: Option<Move>,
        tt_bound: Bound,
        ply: usize,
        depth: i32,
    ) -> (Vec<i32>, Vec<i32>) {
        let side = board.side.idx();
        // Hoisted: the continuation slots depend on the ply, not on the move,
        // and looking them up per move turned a couple of array reads into a
        // couple per move in the hottest loop there is.
        let slots = self.cont_slots(ply);
        // A chave da estrutura de peoes, uma vez por no'. Zero quando a manete
        // esta' desligada, e ai' nem se calcula.
        let cp_no = if self.features.hist_peao { self.chave_peao(board) } else { 0 };
        // What the tables actually think of each move, kept apart from where it
        // goes in the list.
        //
        // These were the same number, and it was wrong. Ordering uses large
        // sentinels -- a million for the table move, four hundred thousand for
        // a killer, plus or minus six hundred thousand for a capture -- so any
        // reduction scaled by "the score" saturated on the sentinel and had
        // nothing to do with history at all. Every killer and table move was
        // being reduced two plies less, and every losing capture two plies
        // more, on the strength of a tag rather than a fact.
        // So' para a ordem. Nao entra no `hist`: esse alimenta a reducao, e um
        // termo grande la' dentro satura o clamp e poda a busca em vez de a
        // ordenar -- foi o que custou 301 Elo a' primeira tentativa.
        let pcb: Vec<i32> = if self.features.hist_pc {
            moves
                .iter()
                .map(|mv| {
                    if mv.is_capture() || mv.promotion.is_some() {
                        return 0;
                    }
                    match board.piece_at(mv.from) {
                        Some((pt, _)) => {
                            self.params.hist_pc_f * self.histpc[pt.idx()][mv.to as usize] / 100
                        }
                        None => 0,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        // TRIAGEM.
        //
        // Cada tranquilo custa aqui um acesso ao historico principal MAIS tres
        // acessos dispersos a`s tabelas de continuacao -- e sao os dispersos que
        // custam, porque cada um e' uma ida a` memoria que nao esta' na cache.
        // Pagamos esse exame a toda a gente na fila e depois usamos dois ou
        // tres lances.
        //
        // A triagem faz o que uma urgencia faz: um olhar barato ordena a fila --
        // aqui, o historico principal sozinho, um acesso -- e o exame caro vai
        // so' aos que esse olhar poe a` frente. Quem fica no fundo mantem a nota
        // grosseira, e como 92% dos nos cortam antes de la' chegar, ninguem da'
        // pela diferenca.
        //
        // O erro que se aceita e' ordenar mal quem estava no fundo da fila. E'
        // o mesmo erro que a triagem aceita, e pela mesma razao.
        let triagem = self.features.triagem;
        let baratos: Vec<i32> = if triagem {
            moves
                .iter()
                .map(|mv| {
                    if mv.is_capture() || mv.promotion.is_some() {
                        i32::MIN
                    } else {
                        self.history[side][mv.from as usize][mv.to as usize]
                            [self.balde_de(board, mv)]
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        // O corte da fila: a nota do enesimo melhor pelo olhar barato.
        let corte = if triagem {
            let mut v: Vec<i32> = baratos.iter().copied().filter(|x| *x != i32::MIN).collect();
            let n = (self.params.triagem_n as usize).min(v.len());
            if n == 0 {
                i32::MAX
            } else {
                // Seleccao do enesimo, nao ordenacao completa.
                //
                // A primeira versao ordenava o vector inteiro para ler UM
                // valor. Medido: custava mais do que os acessos a`s tabelas de
                // continuacao que evitava -- 13.365 ciclos por no' contra 12.702
                // da base. A triagem tem de ser mais barata do que o exame que
                // dispensa, senao e' so' mais uma fila.
                let (_, k, _) = v.select_nth_unstable_by(n - 1, |a, b| b.cmp(a));
                *k
            }
        } else {
            i32::MIN
        };

        let hist: Vec<i32> = moves
            .iter()
            .enumerate()
            .map(|(n_mv, mv)| {
                if triagem && !mv.is_capture() && mv.promotion.is_none() && baratos[n_mv] < corte {
                    // Fundo da fila: fica com a nota do olhar barato.
                    return baratos[n_mv];
                }
                let mv = &*mv;
                if mv.is_capture() || mv.promotion.is_some() {
                    return 0;
                }
                // Adiado: este lance so' e' avaliado se a busca chegar a ele.
                // Medido pelo outro agente: 92% dos nos cortam sem precisar de
                // nenhum tranquilo.
                if self.features.pontua_tarde
                    && Some(*mv) != tt_move
                    && !self.killers[ply].iter().any(|k| *k == Some(*mv))
                {
                    return TRANQUILO_POR_PONTUAR;
                }
                let mut h =
                    self.history[side][mv.from as usize][mv.to as usize][self.balde_de(board, mv)];
                if escala_ligada() {
                    use std::sync::atomic::Ordering::Relaxed;
                    ESCALA[0].fetch_add(1, Relaxed);
                    ESCALA[3].fetch_add((h as i64).abs(), Relaxed);
                }
                if let Some(pc) = board.piece_at(mv.from).map(|(pt, _)| pt.idx()) {
                    for (k, slot) in slots.iter().enumerate() {
                        if let Some(idx) = slot {
                            // Os dois ultimos (plies 3 e 6) so' entram com a
                            // opcao ligada -- assim a omissao fica identica ao
                            // que estava.
                            if k >= 3 && !self.features.cont_longo {
                                continue;
                            }
                            let c = CONT_WEIGHT[k]
                                * self.conthist[ich(k, *idx, pc, mv.to as usize)]
                                    .load(Ordering::Relaxed);
                            if escala_ligada() {
                                ESCALA[4 + k.min(5)]
                                    .fetch_add((c as i64).abs(), std::sync::atomic::Ordering::Relaxed);
                            }
                            h += c;
                        }
                    }
                    if self.features.hist_peao {
                        // A cor conta: doze pecas, nao seis. Um peao branco em
                        // e4 e um preto em e5 nao dizem o mesmo da estrutura.
                        h += self.params.peao_f
                            * self.histpeao[ipeao(cp_no, pc + 6 * side, mv.to as usize)]
                                .load(Ordering::Relaxed)
                            / 32;
                    }
                }
                if escala_ligada() {
                    use std::sync::atomic::Ordering::Relaxed;
                    ESCALA[1].fetch_add((h as i64).abs(), Relaxed);
                    ESCALA[2].fetch_max((h as i64).abs(), Relaxed);
                }
                h
            })
            .collect();

        // Uma vez por no', nao uma por lance -- e so' quando sao precisas.
        let (menores, xeque) = if self.features.ordem_posicao {
            (
                ameacas_menores(board, board.side.opp(), &self.atk),
                casas_de_xeque(board, board.side.opp(), &self.atk),
            )
        } else {
            ([0u64; 6], [0u64; 6])
        };

        // O termo do xeque, somado por cima do que a ordem der.
        //
        // MEDIDO: com ele, 173 das 195 posicoes do Win At Chess contra 155 sem
        // ele -- dezoito a mais, e o mesmo numero que o motor de referencia
        // resolve com a NOSSA rede. Sem ele, 154. E' o termo inteiro que compra
        // a visao, nao a forma da ordenacao.
        //
        // A questao que isto responde: dava para ter essa visao sem pagar os 38%
        // de arvore da soma continua? Aqui as bandas ficam como estao e o xeque
        // entra so' como parcela.
        let order: Vec<i32> = moves
            .iter()
            .enumerate()
            .map(|(n, mv)| {
                // ORDENACAO POR SOMA CONTINUA, sem bandas.
                //
                // A nossa ordem por bandas -- um milhao para o lance da tabela,
                // seiscentos mil para as capturas boas, quatrocentos mil para os
                // killers, o historico para os tranquilos, MENOS seiscentos mil
                // para as capturas mas -- proibe comparacoes que deviam ser
                // possiveis. Uma captura que perde meio peao fica setecentos mil
                // pontos abaixo do pior tranquilo, quando pode muito bem ser o
                // melhor lance do no'; e um tranquilo com historico forte nunca
                // passa a` frente de uma captura boa, seja qual for o historico.
                //
                // Aqui somam-se as parcelas em vez de as arrumar em classes: o
                // valor REAL da troca -- o `see` devolve-o, nao apenas uma
                // comparacao com um limiar --, a vitima como desempate, o
                // historico das capturas, e um credito ao killer que o historico
                // pode ultrapassar. Os pesos sao parametros para se afinarem.
                //
                // O lance da tabela continua em cima: procura-lo primeiro nao e'
                // uma questao de ordenacao, e' o que da' sentido a ter tabela.
                if self.features.ordem_continua && Some(*mv) != tt_move {
                    let mut r = 0i32;
                    if mv.is_capture() || mv.promotion.is_some() {
                        let s = see::see(&self.atk, board, mv);
                        // Escala: o historico de um tranquilo forte chega a
                        // 135.000 (medido). Para uma captura de dama ficar
                        // acima disso, o peso do `see` tem de por o peao nos
                        // quinze mil -- e nao nos duzentos, que foi o erro da
                        // primeira versao e que afundou as capturas por baixo
                        // de todos os tranquilos, triplicando a arvore.
                        r += self.params.ord_see * s;
                        let victim = if mv.flag == MoveFlag::EnPassant {
                            PieceType::Pawn.value()
                        } else {
                            board.piece_at(mv.to).map(|(pt, _)| pt.value()).unwrap_or(0)
                        };
                        r += self.params.ord_mvv * victim / 16;
                        r += self.capt_score(board, mv) / 16;
                    } else {
                        // As proporcoes do outro motor, afinadas por SPSA sobre
                        // milhares de partidas:
                        //   see 443, historico 106, xeque 60, killer 54
                        // O SEE pesa quatro vezes o historico -- ao contrario do
                        // que eu tinha adivinhado -- e ha' um termo para "da'
                        // xeque" que nos nao tinhamos de todo.
                        r += hist[n] * self.params.ord_hist_n / 135;
                        if let Some(k) = self.killers[ply].iter().position(|kk| *kk == Some(*mv)) {
                            r += self.params.ord_killer * (NUM_KILLERS as i32 - k as i32)
                                / NUM_KILLERS as i32;
                        }
                        if self.params.ord_xeque > 0 && self.da_xeque(board, mv) {
                            r += self.params.ord_xeque;
                        }
                    }
                    return r;
                }
                if Some(*mv) == tt_move {
                    // Um limite superior quer dizer que naquele no' todos os
                    // lances falharam em baixo: o guardado e' o menos mau, nao
                    // um que provou cortar. Medido: vai primeiro 21.538 vezes e
                    // corta 13,8%, contra 66,8% de um limite inferior -- e o que
                    // fica atras dele sao as capturas boas, que acertam 83-87%.
                    if self.features.tt_fraco && tt_bound == Bound::Upper {
                        self.params.tt_fraco_pont
                    } else {
                        1_000_000
                    }
                } else if mv.is_capture() {
                    let victim = if mv.flag == MoveFlag::EnPassant {
                        PieceType::Pawn.value()
                    } else {
                        board
                            .piece_at(mv.to)
                            .map(|(pt, _)| pt.value())
                            .unwrap_or(0)
                    };
                    let attacker = board
                        .piece_at(mv.from)
                        .map(|(pt, _)| pt.value())
                        .unwrap_or(0);
                    let mvv = victim * 16 - attacker;
                    // A capture that loses material is not a good move that
                    // happens to be violent, and putting it ahead of the quiet
                    // moves on the strength of what it takes is how a search
                    // spends its first three tries on refuted sacrifices.
                    // Below everything, then, but still ahead of nothing.
                    // The bar for a capture counting as good drops with depth.
                    // Deep in the tree there is room to find out whether a
                    // capture that looks slightly losing actually is; near the
                    // leaves there is not, so only the clearly good ones go
                    // first.
                    // A barra do SEE e o peso da historia de capturas, ambos
                    // varriveis: as capturas sao 55,5% dos primeiros lances
                    // tentados e cortam 77,3% das vezes -- o termo com mais
                    // peso na taxa de corte ao primeiro lance, que esta' em
                    // 73,0%. Um ponto ganho aqui vale meio ponto no total.
                    let past = self.capt_score(board, mv) * 16
                        / self.params.capt_hist_div.max(1);
                    // O CORTE PELO MERITO DO LANCE, e nao pela profundidade.
                    //
                    // Item 12 do VALIDACOES.md, recuperado a 21-09: "o corte
                    // captura boa/ma vem da PROFUNDIDADE; o deles vem do valor
                    // do lance" -- arvore +40 a +90%. O KestrelStrike ja' o
                    // tinha arranjado; aqui faltava.
                    //
                    // Com a barra vinda da profundidade, a historia mexia na
                    // ORDEM e mais nada: uma captura que a tabela adora e outra
                    // que ela despreza eram julgadas boas ou mas pelo MESMO
                    // criterio. Agora uma captura que ja' provou valer leva uma
                    // barra mais permissiva, que e' o que ela merece.
                    //
                    // O `capt_score` ja' estava calculado na linha de cima --
                    // so' nao entrava aqui.
                    let bar = if self.params.capt_bar_div > 0 {
                        -(mvv + past) / self.params.capt_bar_div
                    } else {
                        (-self.params.capt_bar_f * (depth - 1))
                            .max(-self.params.capt_bar_max)
                    };
                    if see::see_ge(&self.atk, board, mv, bar) {
                        600_000 + mvv + past
                    } else {
                        -600_000 + mvv + past
                    }
                } else if mv.promotion == Some(PieceType::Queen) {
                    500_000
                } else if let Some(k) =
                    self.killers[ply].iter().position(|k| *k == Some(*mv))
                {
                    if self.features.sem_killers {
                            // Sem killers: vale o que o historico diz, como
                            // qualquer outro tranquilo.
                            //
                            // A referencia tirou-os em tres passos, e os dois
                            // ultimos passaram como NAO-REGRESSAO (limites
                            // <-1.75, 0.25>): tira-los nao deu Elo, deu igual e
                            // simplificou. Nao sao vantagem que se perde -- sao
                            // muleta de uma ordenacao pobre. Aqui acertam 65,7%
                            // contra 87,2% das capturas, e sao 42% de todo o
                            // desperdicio da primeira tentativa.
                            //
                            // Eles precisaram de compensar a media do historico
                            // antes do LMR (`fill(-658)`); aqui nao, porque o
                            // `score_moves` ja' devolve `order` e `hist`
                            // separados e so' o `order` muda.
                            hist[n]
                        } else if self.features.killer_compete {
                            // MEDIDO: os tres killers acertam 67,8%, 67,7% e
                            // 68,4% quando vao a` frente -- iguais entre si, e
                            // muito abaixo dos 87,2% das capturas. Nao sao o
                            // segundo e o terceiro que estragam; e' a classe.
                            //
                            // Com 400_000 fixo, o melhor tranquilo que o
                            // historico conhece (120_293, medido) perde para o
                            // terceiro killer -- nao competem, passam sempre.
                            // Aqui somam-se: o killer entra com credito, mas um
                            // tranquilo com historico forte pode ultrapassa-lo.
                            hist[n] + self.params.killer_bonus - 10_000 * k as i32
                        } else {
                            400_000 - 10_000 * k as i32
                        }
                } else if self.features.hist_pc {
                    hist[n] + pcb[n]
                } else if self.features.ordem_posicao {
                    let mut v = hist[n];
                    if let Some((pt, _)) = board.piece_at(mv.from) {
                        let t = pt.idx();
                        // Fugir de quem lhe chega, e nao ir para onde lhe
                        // chegam. O historico nunca aprende isto: e' uma
                        // propriedade DESTA posicao, nao do lance.
                        let de = (menores[t] >> mv.from & 1) as i32;
                        let para = (menores[t] >> mv.to & 1) as i32;
                        v += pt.value() * self.params.ordem_ameaca_f * (de - para) / 100;
                        // Xeque que nao perde material vai a` frente de tudo o
                        // que o historico tenha a dizer.
                        if xeque[t] >> mv.to & 1 != 0
                            && see::see_ge(&self.atk, board, mv, -75)
                        {
                            v += self.params.ordem_xeque_f;
                        }
                    }
                    v
                } else if self.features.pontua_tarde && hist[n] == TRANQUILO_POR_PONTUAR {
                    // Fica por pontuar. Os sentinelas acima -- um milhao para o
                    // lance da tabela, seiscentos mil para as capturas,
                    // quatrocentos mil para os killers -- garantem que os
                    // tranquilos so' sao olhados depois deles, e a essa altura
                    // ja' se sabe se foram precisos.
                    TRANQUILO_POR_PONTUAR
                } else {
                    hist[n]
                }
            })
            .collect();

        // O termo do xeque, somado por cima do que a ordem der.
        //
        // MEDIDO: 173 das 195 posicoes do Win At Chess com ele, contra 155 sem --
        // dezoito a mais, e o mesmo que o motor de referencia resolve com a NOSSA
        // rede. E' o termo inteiro que compra a visao, nao a forma da ordenacao:
        // a soma continua sem ele resolve 154, igual a`s bandas.
        //
        // Aplicado aqui, por cima, para se poder ter com as BANDAS -- e assim
        // saber se a visao vem sem os 38% de arvore que a soma continua custa.
        let order: Vec<i32> = if self.features.xeque_na_ordem {
            moves
                .iter()
                .zip(order.into_iter())
                .map(|(mv, o)| {
                    if o >= 900_000 || !self.da_xeque(board, mv) {
                        return o;
                    }
                    // Banda propria para o xeque, em vez de um bonus somado.
                    //
                    // Somado, os 60.000 nao chegam para nada: um xeque tranquilo
                    // com historico zero fica nos 60.000, ainda abaixo dos
                    // killers (400.000) e muito abaixo das capturas (600.000) --
                    // ou seja, nunca sobe. Uma banda poe o xeque como CLASSE, que
                    // e' o que as capturas ja' sao, e dentro dela os lances
                    // continuam ordenados pelo que valiam.
                    if self.params.xeque_banda > 0 {
                        self.params.xeque_banda + o.clamp(-99_000, 99_000)
                    } else {
                        o + self.params.ord_xeque
                    }
                })
                .collect()
        } else {
            order
        };
        (order, hist)
    }

    /// Bring the best remaining move to `at`, keeping scores in step.
    #[inline]
    /// Preencher a pontuacao dos tranquilos, quando a busca chega a eles.
    fn pontua_tranquilos(
        &self,
        board: &Board,
        moves: &[Move],
        scores: &mut [i32],
        hist: &mut [i32],
        de: usize,
        ply: usize,
    ) {
        let side = board.side.idx();
        let slots = self.cont_slots(ply);
        let cp = if self.features.hist_peao { self.chave_peao(board) } else { 0 };
        for n in de..moves.len() {
            if scores[n] != TRANQUILO_POR_PONTUAR {
                continue;
            }
            let h = self.hist_de(board, &moves[n], side, &slots, cp);
            hist[n] = h;
            scores[n] = h;
        }
    }

    fn pick(moves: &mut [Move], scores: &mut [i32], hist: &mut [i32], at: usize) {
        let mut best = at;
        for j in at + 1..moves.len() {
            if scores[j] > scores[best] {
                best = j;
            }
        }
        Self::troca(moves, scores, hist, at, best);
    }

    /// O mesmo, com o maximo achado em C++ por AVX2.
    ///
    /// Tem de devolver o MESMO indice que a versao acima, empates incluidos --
    /// o Rust usa `>` estrito e fica com o primeiro maximo. Um argmax que
    /// ficasse com o ultimo mudava a ordem dos lances e com ela a arvore, e
    /// entao ja' nao estariamos a medir velocidade.
    fn pick_cpp(moves: &mut [Move], scores: &mut [i32], hist: &mut [i32], at: usize) {
        let best = unsafe { h2k_argmax_i32(scores.as_ptr(), scores.len(), at) };
        Self::troca(moves, scores, hist, at, best);
    }

    #[inline]
    fn troca(moves: &mut [Move], scores: &mut [i32], hist: &mut [i32], at: usize, best: usize) {
        moves.swap(at, best);
        scores.swap(at, best);
        hist.swap(at, best);
    }
}

#[cfg(test)]
mod testes_tempo {
    use super::faltam_lances_pct;

    /// A funcao antiga, para os testes que ja' existiam continuarem a dizer o
    /// que diziam: a mediana e' o percentil 0.
    fn faltam_lances(jogados: u32, minimo: i32) -> u64 {
        faltam_lances_pct(jogados, minimo, 0, 0)
    }

    #[test]
    fn curva_bate_com_a_mediana_medida() {
        assert!(matches!(faltam_lances(0, 14), 65 | 66));
        assert_eq!(faltam_lances(20, 14), 46);   // 45,5 arredonda para 46
        assert_eq!(faltam_lances(60, 14), 19);
    }

    /// Os dois nos que faltavam a` tabela, e que o `minimo` tapava.
    ///
    /// Antes desta correccao o lance 80 dava 14 (a extrapolacao dava 10 e o
    /// chao corrigia) quando a medida diz 15,5. Um horizonte curto de mais nas
    /// partidas LONGAS -- que sao precisamente as que acabam a` bandeira.
    #[test]
    fn os_nos_de_80_e_110_vem_da_medicao_e_nao_do_chao() {
        assert_eq!(faltam_lances(80, 14), 16);   // 15,5 arredonda para 16
        assert_eq!(faltam_lances(110, 14), 14);
        // e para la' do ultimo no' fica o valor da ponta, nao um declive
        assert_eq!(faltam_lances(200, 14), 14);
    }

    /// Subir o percentil TEM de alargar o horizonte em todo o lado, senao nao
    /// esta' a fazer o que diz.
    #[test]
    fn o_percentil_alarga_o_horizonte() {
        for lance in [0u32, 20, 40, 60, 80] {
            let mut ant = 0;
            for pct in 0..5 {
                let v = faltam_lances_pct(lance, 14, pct, 0);
                assert!(v >= ant, "o percentil {} encolheu no lance {}", pct, lance);
                ant = v;
            }
        }
        // e a diferenca tem de ser real, nao arredondamento
        assert!(faltam_lances_pct(40, 14, 3, 0) > faltam_lances_pct(40, 14, 0, 0) + 10);
    }

    /// A estimativa tem de se corrigir quando a partida a desmente.
    #[test]
    fn o_horizonte_cresce_depois_do_fim_da_tabela() {
        // desligado: constante depois do lance 110, que e' o defeito
        assert_eq!(faltam_lances_pct(110, 14, 0, 0), faltam_lances_pct(200, 14, 0, 0));
        // ligado: cresce com o excesso
        assert!(faltam_lances_pct(200, 14, 0, 100) > faltam_lances_pct(110, 14, 0, 100) + 50);
        // e NAO mexe onde a tabela ainda manda
        for l in [0u32, 40, 80, 110] {
            assert_eq!(faltam_lances_pct(l, 14, 0, 0), faltam_lances_pct(l, 14, 0, 100),
                       "mexeu no lance {}, onde a tabela ainda manda", l);
        }
    }
    #[test]
    fn nunca_cresce_e_respeita_o_minimo() {
        let mut ant = u64::MAX;
        for j in 0..200 {
            let v = faltam_lances(j, 14);
            assert!(v <= ant, "cresceu no lance {}", j);
            assert!(v >= 14);
            ant = v;
        }
    }
}
