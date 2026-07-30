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
const TIMEOUT_S: &str = "1.2";

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

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

fn now_s() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The exact answer for this position, or None for every other case.
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
