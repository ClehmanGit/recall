//! Chunking. Each extractor yields (locator, text) units; this splits long units
//! into overlapping windows so a chunk stays small enough to embed well, while
//! the locator survives so the card can say where the hit lives.

use crate::db::NewChunk;

pub const TARGET_CHARS: usize = 1200;
pub const OVERLAP_CHARS: usize = 200;
pub const MIN_CHARS: usize = 40;

pub struct Unit {
    pub locator: String,
    pub text: String,
    pub anchor: Option<String>,
}

pub fn chunk_units(units: Vec<Unit>) -> Vec<NewChunk> {
    let mut out = Vec::new();
    let mut ord = 0i64;
    for u in units {
        for piece in split(&u.text) {
            if piece.trim().len() < MIN_CHARS {
                continue;
            }
            out.push(NewChunk {
                ord,
                locator: u.locator.clone(),
                text: piece,
                anchor: u.anchor.clone(),
            });
            ord += 1;
        }
    }
    out
}

/// Split on paragraph boundaries first, then by character windows.
fn split(text: &str) -> Vec<String> {
    let text = normalize(text);
    if text.len() <= TARGET_CHARS {
        return vec![text];
    }
    let mut pieces = Vec::new();
    let mut buf = String::new();
    for para in text.split("\n\n") {
        if buf.len() + para.len() + 2 > TARGET_CHARS && !buf.is_empty() {
            pieces.push(std::mem::take(&mut buf));
        }
        if para.len() > TARGET_CHARS {
            for w in windows(para) {
                pieces.push(w);
            }
        } else {
            if !buf.is_empty() {
                buf.push_str("\n\n");
            }
            buf.push_str(para);
        }
    }
    if !buf.is_empty() {
        pieces.push(buf);
    }
    pieces
}

fn windows(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + TARGET_CHARS).min(chars.len());
        out.push(chars[start..end].iter().collect());
        if end == chars.len() {
            break;
        }
        start = end.saturating_sub(OVERLAP_CHARS);
    }
    out
}

fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blank = 0;
    for line in s.lines() {
        let t = line.trim_end();
        if t.trim().is_empty() {
            blank += 1;
            if blank <= 1 {
                out.push('\n');
            }
        } else {
            if blank > 0 && !out.ends_with("\n\n") && !out.is_empty() {
                out.push('\n');
            }
            blank = 0;
            out.push_str(t);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_one_chunk() {
        let c = chunk_units(vec![Unit { locator: "p.1".into(), text: "a".repeat(100), anchor: None }]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].locator, "p.1");
    }

    #[test]
    fn long_text_overlaps() {
        let c = chunk_units(vec![Unit { locator: "L1".into(), text: "b".repeat(3000), anchor: None }]);
        assert!(c.len() >= 3);
        assert!(c.iter().all(|x| x.text.len() <= TARGET_CHARS));
    }

    #[test]
    fn tiny_pieces_dropped() {
        let c = chunk_units(vec![Unit { locator: "x".into(), text: "hi".into(), anchor: None }]);
        assert!(c.is_empty());
    }
}
