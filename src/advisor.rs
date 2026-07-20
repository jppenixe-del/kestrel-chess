//! Optional LLM tie-breaker for near-equal candidate moves. Entirely
//! opt-in and fail-safe: if `KESTREL_ADVISOR_HOST` isn't set, or the
//! connection/response fails for ANY reason, every function here returns
//! `None` immediately and the caller falls back to the engine's own top
//! choice -- normal play is completely unaffected when this feature is
//! off (the default for every deployment that doesn't set the env var).
//!
//! Deliberately dependency-free (no HTTP/JSON crate): talks plain HTTP/1.1
//! to a local Ollama instance over `std::net::TcpStream`, and extracts the
//! one JSON field it needs with a small hand-rolled scanner rather than a
//! full JSON parser.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub struct Advisor {
    host: String, // "host:port", e.g. "127.0.0.1:11434"
    model: String,
}

impl Advisor {
    /// `KESTREL_ADVISOR_HOST` (e.g. "127.0.0.1:11434") enables the
    /// advisor; `KESTREL_ADVISOR_MODEL` optionally overrides the model
    /// name (defaults to the one used throughout this project's testing).
    pub fn from_env() -> Option<Self> {
        let host = std::env::var("KESTREL_ADVISOR_HOST").ok()?;
        let model = std::env::var("KESTREL_ADVISOR_MODEL").unwrap_or_else(|_| "qwen2.5-coder:7b".to_string());
        Some(Advisor { host, model })
    }

    /// `candidates`: (label, move_uci, score_cp). Returns the chosen
    /// label on success, `None` on any failure (network, timeout,
    /// malformed response, or a response that names none of the
    /// candidates) -- the caller should always have its own fallback.
    pub fn ask(&self, fen: &str, candidates: &[(char, String, i32)]) -> Option<char> {
        let opts: String = candidates.iter().map(|(lab, mv, _)| format!("{}) {}\n", lab, mv)).collect();
        let prompt = format!(
            "You are the strategic advisor for Kestrel, a chess engine with an aggressive, \
             tactical, sacrifice-friendly playing style inspired by Judit Polgar's approach: \
             prefer pressure on the enemy king, practical complexity, and lines that are \
             uncomfortable for a human to defend over passive equality. \
             Position (FEN): {}\n\
             The engine's own search judged these moves as roughly equally good:\n{}\
             Reply with ONLY the single letter of the move that best fits this aggressive style, nothing else.",
            fen, opts
        );
        let body = format!(
            "{{\"model\":\"{}\",\"prompt\":\"{}\",\"stream\":false}}",
            json_escape(&self.model),
            json_escape(&prompt)
        );
        let raw = self.http_post(&body)?;
        let response_field = extract_json_string_field(&raw, "response")?;
        let upper = response_field.to_uppercase();
        for (lab, _, _) in candidates {
            let solo = lab.to_string();
            if upper == solo || upper.contains(&format!("{})", lab)) || upper.contains(&format!("{}.", lab)) {
                return Some(*lab);
            }
        }
        for (lab, mv, _) in candidates {
            if upper.contains(&mv.to_uppercase()) {
                return Some(*lab);
            }
        }
        for (lab, _, _) in candidates {
            if upper.contains(&lab.to_string()) {
                return Some(*lab);
            }
        }
        None
    }

    fn http_post(&self, json_body: &str) -> Option<String> {
        let mut stream = TcpStream::connect(&self.host).ok()?;
        stream.set_read_timeout(Some(Duration::from_secs(15))).ok()?;
        stream.set_write_timeout(Some(Duration::from_secs(5))).ok()?;
        let request = format!(
            "POST /api/generate HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            self.host,
            json_body.len(),
            json_body
        );
        stream.write_all(request.as_bytes()).ok()?;
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).ok()?;
        let text = String::from_utf8_lossy(&buf).into_owned();
        let body_start = text.find("\r\n\r\n")? + 4;
        Some(text[body_start..].to_string())
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// Pulls the string value of one top-level JSON field out of raw JSON
/// text. Deliberately not a general parser -- Ollama's response shape is
/// fixed and this is the only field this project ever needs from it.
fn extract_json_string_field(json: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", field);
    let start = json.find(&needle)? + needle.len();
    let mut out = String::new();
    let mut chars = json[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    match next {
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        other => out.push(other),
                    }
                }
            }
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}
