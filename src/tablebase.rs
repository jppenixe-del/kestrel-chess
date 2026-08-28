// A rede externa vive atras da feature `bot`, e NAO esta' no build por omissao.
//
// O bot que corre na nossa maquina pode consultar o tablebase da Lichess e o
// conselheiro; um motor entregue a uma lista de rating nao pode consultar nada
// -- e' ajuda externa, e "opcao desligada por omissao" nao serve como garantia
// porque o operador nao tem como a verificar. Fechado por feature, o binario
// entregue nao contem o URL nem o socket, e um `strings` no ficheiro prova-o.
//
//     motor entregue:  cargo build --release
//     bot da casa:     cargo build --release --features bot

//! The endgames that are already solved, asked instead of searched.
//!
//! Up to seven pieces the result of chess is computed and published. A position
//! inside that is not something to evaluate; it is something to look up, and
//! there is no depth of search that improves on an exact answer.
//!
//! Worth more than it looks. Measured over 600 of our own games, 22% reach
//! seven pieces or fewer -- and the difference is not half a pawn. In a
//! king-and-pawn ending measured here the position is WON and only one of the
//! four sensible moves keeps the win; the other three draw. An evaluation
//! averaging 44cp of error over endgame positions cannot separate those. The
//! table does not have to: it knows.
//!
//! Off by default and enabled with `setoption name OnlineTablebase value true`,
//! so a build behaves identically to before unless asked. It is a UCI option
//! rather than something the client does, which means any client gets it and
//! the engine reports the move as its own -- with zero thinking time, because
//! there was nothing to think about.
//!
//! Shelling out to curl rather than taking a dependency. This project has none
//! and an HTTPS client is a large thing to add for one request per endgame.
//!
//! Every failure is silent and safe: no network, a slow answer, a shape we did
//! not expect, and it returns None and the search runs as always. After a
//! failure it stays quiet for five minutes rather than paying the timeout again
//! on every move -- that cost lands on OUR clock.

use crate::board::Board;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const MAX_PIECES: u32 = 7;
/// Cem milissegundos, e nem um a mais.
///
/// A consulta corre no NOSSO relogio, antes de o motor pensar. Com 1.2s, nos
/// ultimos segundos de um bullet a consulta era o orcamento inteiro do lance:
/// trocava-se uma jogada perfeita por uma bandeira, que e' o pior negocio
/// possivel. E a rede daqui chega ao servico em 46ms, portanto cem
/// milissegundos e' folga suficiente para uma resposta que va' mesmo chegar.
///
/// Se nao responder a tempo, joga-se. Uma jogada boa a horas bate uma jogada
/// perfeita fora de horas, e a tabela nao vale nada a quem perdeu por tempo.
const TIMEOUT_S: &str = "0.10";

static ENABLED: AtomicBool = AtomicBool::new(false);
static QUIET_UNTIL: AtomicI64 = AtomicI64::new(0);
static CACHE: Mutex<Option<HashMap<String, Option<Hit>>>> = Mutex::new(None);

#[derive(Clone, Debug)]
pub struct Hit {
    pub best: String,
    /// Ours, from the side to move: 1 win, 0 draw, -1 loss.
    pub wdl: i32,
    pub dtz: i32,
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

#[cfg(not(feature = "bot"))]
pub fn enabled() -> bool {
    false
}

#[cfg(not(feature = "bot"))]
pub fn probe(_board: &Board) -> Option<Hit> {
    None
}

#[cfg(feature = "bot")]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

#[cfg(feature = "bot")]
fn now_s() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The exact answer for this position, or None for every other case.
#[cfg(feature = "bot")]
pub fn probe(board: &Board) -> Option<Hit> {
    if !enabled() || board.occ_all.count_ones() > MAX_PIECES {
        return None;
    }
    // Castling rights are not part of a tablebase position.
    if board.castling != 0 {
        return None;
    }
    if now_s() < QUIET_UNTIL.load(Ordering::Relaxed) {
        return None;
    }
    let fen = board.to_fen();
    {
        let mut g = CACHE.lock().ok()?;
        if let Some(m) = g.as_ref() {
            if let Some(v) = m.get(&fen) {
                return v.clone();
            }
        } else {
            *g = Some(HashMap::new());
        }
    }
    let hit = fetch(&fen);
    if let Ok(mut g) = CACHE.lock() {
        if let Some(m) = g.as_mut() {
            m.insert(fen, hit.clone());
        }
    }
    hit
}

#[cfg(feature = "bot")]
fn fetch(fen: &str) -> Option<Hit> {
    let url = format!(
        "https://tablebase.lichess.org/standard?fen={}",
        fen.replace(' ', "_")
    );
    let t = Instant::now();
    let out = std::process::Command::new("curl")
        .args(["-s", "--max-time", TIMEOUT_S, &url])
        .output();
    let body = match out {
        Ok(o) if o.status.success() && !o.stdout.is_empty() => o.stdout,
        _ => {
            QUIET_UNTIL.store(now_s() + 300, Ordering::Relaxed);
            eprintln!("tablebase: nao respondeu em {:?}, calado 5min", t.elapsed());
            return None;
        }
    };
    let text = String::from_utf8_lossy(&body);
    parse(&text)
}

/// The one field that matters and the first move, pulled out by hand.
///
/// A JSON parser for two fields is more code than the code it would replace,
/// and the shape here is fixed: `"category"` at the top level is the result for
/// the side to move, and the `"moves"` array arrives already ordered best
/// first. The category ON a move is from the point of view of whoever RECEIVES
/// the position, so our winning move is the one that leaves him lost.
#[cfg(feature = "bot")]
fn parse(text: &str) -> Option<Hit> {
    let cat = field_str(text, "\"category\"")?;
    let wdl = match cat.as_str() {
        "win" | "cursed-win" => 1,
        "loss" | "blessed-loss" => -1,
        "draw" => 0,
        _ => return None,
    };
    let dtz = field_i64(text, "\"dtz\"").unwrap_or(0) as i32;
    let moves_at = text.find("\"moves\"")?;
    let rest = &text[moves_at..];
    let best = field_str(rest, "\"uci\"")?;
    if best.len() < 4 {
        return None;
    }
    Some(Hit { best, wdl, dtz })
}

#[cfg(feature = "bot")]
fn field_str(text: &str, key: &str) -> Option<String> {
    let i = text.find(key)? + key.len();
    let rest = &text[i..];
    let c = rest.find(':')? + 1;
    let rest = &rest[c..];
    let q = rest.find('"')? + 1;
    let end = rest[q..].find('"')?;
    Some(rest[q..q + end].to_string())
}

fn field_i64(text: &str, key: &str) -> Option<i64> {
    let i = text.find(key)? + key.len();
    let rest = &text[i..];
    let c = rest.find(':')? + 1;
    let s: String = rest[c..]
        .trim_start()
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '-')
        .collect();
    s.parse().ok()
}

/// Kept so the unused-import warning does not hide a real one later.
pub fn _touch() {
    let _ = Duration::from_secs(1);
}
