//! Hybrid search. Vector similarity blended with FTS keyword rank and a light
//! recency boost, then grouped so each item is one card with its best chunks
//! ranked inside it. A long chat cannot flood the list with itself. Names
//! count too: a file's or chat's name is matched word by word, and each
//! passage is embedded with its item's name, so an item is found by what it
//! is called even when its text never says it.

use crate::db::{self, Db};
use crate::embed::Embedder;
use crate::index::VecIndex;
use anyhow::Result;
use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Query {
    pub q: String,
    /// kinds filter: empty = all. "chat", "pdf", "document", "spreadsheet", "code", "image", "text"
    #[serde(default)]
    pub kinds: Vec<String>,
    /// sources filter: empty = all. platform names or folder paths
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// one item only: a card's "Show more" asks for that card's passages
    #[serde(default)]
    pub item: Option<i64>,
    /// passages per card; HITS_PER_CARD when absent, never more than MAX_HITS_PER_CARD
    #[serde(default)]
    pub hits: Option<usize>,
}
fn default_limit() -> usize {
    30
}

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub chunk_id: i64,
    pub locator: String,
    pub text: String,
    pub anchor: Option<String>,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Card {
    pub item_id: i64,
    pub kind: String,
    pub source: String,
    pub title: String,
    pub location: String,
    pub uri: String,
    pub modified_at: String,
    pub size_label: String,
    pub starred: bool,
    pub score: f32,
    pub hits: Vec<Hit>,
    pub total_hits: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Results {
    pub cards: Vec<Card>,
    pub total_hits: usize,
    pub took_ms: u128,
}

const VEC_K: usize = 120;
const FTS_K: usize = 120;
const W_VEC: f32 = 0.72;
const W_FTS: f32 = 0.20;
const W_RECENT: f32 = 0.08;
/// Added to a card whose name holds the query's words: the full weight when it
/// holds all of them, a quarter when it holds half.
const W_TITLE: f32 = 0.6;
/// Cosine similarity as this model gives it: unrelated text still scores about
/// 0.4-0.55, related text 0.65-0.8 (measured on bge-small). Rescaled so the
/// floor counts as nothing and a strong match as 1, or noise would outweigh
/// a keyword or name match.
const SIM_FLOOR: f32 = 0.45;
const SIM_STRONG: f32 = 0.80;
const TITLE_K: usize = 60;
const HITS_PER_CARD: usize = 5;
/// past this a card is opened instead of grown; the UI stops offering "Show more" here too
pub const MAX_HITS_PER_CARD: usize = 25;

/// A name as words: no extension, underscores and dashes as spaces.
pub fn readable_title(title: &str) -> String {
    let stem = match title.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) => stem,
        _ => title,
    };
    stem.replace(['_', '-'], " ")
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// How much of the query a name holds, 0..1: the share of query words that
/// begin a word of the name ("book" matches "Books"; one-letter words must
/// match whole). Below half counts as no match.
fn title_match(query: &[String], title: &str) -> f32 {
    if query.is_empty() {
        return 0.0;
    }
    let tw = words(&readable_title(title));
    let found = query.iter().filter(|q| tw.iter().any(|t| if q.len() < 2 { t == *q } else { t.starts_with(q.as_str()) })).count();
    let share = found as f32 / query.len() as f32;
    if share >= 0.5 { share } else { 0.0 }
}

pub fn run(
    db: &Db,
    index: &VecIndex,
    embedder: &Arc<Mutex<Option<Embedder>>>,
    query: &Query,
) -> Result<Results> {
    let t0 = std::time::Instant::now();
    let q = query.q.trim();
    if q.is_empty() {
        return Ok(Results { cards: vec![], total_hits: 0, took_ms: 0 });
    }

    // vector leg (skipped while the model is still loading; keyword leg still runs)
    let mut vec_scores: HashMap<i64, f32> = HashMap::new();
    if let Some(e) = embedder.lock().as_mut() {
        if let Ok(v) = e.embed_query(q) {
            for (id, sim) in index.read().search(&v, VEC_K)? {
                vec_scores.insert(id as i64, ((sim - SIM_FLOOR) / (SIM_STRONG - SIM_FLOOR)).clamp(0.0, 1.0));
            }
        }
    }

    // keyword leg: bm25 is negative-is-better; squash to 0..1 by rank
    let fts = db::fts_search(db, q, FTS_K)?;
    let mut fts_scores: HashMap<i64, f32> = HashMap::new();
    let n = fts.len().max(1) as f32;
    for (rank, (id, _)) in fts.iter().enumerate() {
        fts_scores.insert(*id, 1.0 - (rank as f32 / n));
    }

    // name leg: items whose name holds the query's words
    let qw = words(q);
    let mut named: Vec<(i64, f32)> = db::item_titles(db)?
        .into_iter()
        .map(|(id, title)| (id, title_match(&qw, &title).powi(2)))
        .filter(|(_, s)| *s > 0.0)
        .collect();
    named.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    named.truncate(TITLE_K);
    let title_scores: HashMap<i64, f32> = named.into_iter().collect();

    let mut all_ids: Vec<i64> = vec_scores.keys().chain(fts_scores.keys()).copied().collect();
    all_ids.sort_unstable();
    all_ids.dedup();

    let chunks = db::get_chunks(db, &all_ids)?;
    let hidden = db::hidden_platforms(db);
    let mut items: HashMap<i64, db::Item> = HashMap::new();
    let mut by_item: HashMap<i64, Vec<Hit>> = HashMap::new();
    let now = Utc::now();

    let shown = |item: &db::Item| {
        !item.missing
            && !hidden.contains(&item.source)
            && (query.kinds.is_empty() || query.kinds.iter().any(|k| k == &item.kind))
            && (query.sources.is_empty() || query.sources.iter().any(|s| s == &item.source))
    };
    let recency = |item: &db::Item| (-((now - item.modified_at).num_days().max(0) as f32) / 180.0).exp(); // half-ish weight at six months

    for c in chunks {
        if query.item.is_some_and(|only| only != c.item_id) {
            continue;
        }
        let item = match items.get(&c.item_id) {
            Some(i) => i.clone(),
            None => match db::get_item(db, c.item_id)? {
                Some(i) => {
                    items.insert(c.item_id, i.clone());
                    i
                }
                None => continue,
            },
        };
        if !shown(&item) {
            continue;
        }
        let score = W_VEC * vec_scores.get(&c.id).copied().unwrap_or(0.0)
            + W_FTS * fts_scores.get(&c.id).copied().unwrap_or(0.0)
            + W_RECENT * recency(&item);
        by_item.entry(c.item_id).or_default().push(Hit {
            chunk_id: c.id,
            locator: c.locator,
            text: c.text,
            anchor: c.anchor,
            score,
        });
    }

    // a name match whose text matched nothing still gets a card, opening on its first passages
    for &item_id in title_scores.keys() {
        if by_item.contains_key(&item_id) || query.item.is_some_and(|only| only != item_id) {
            continue;
        }
        let Some(item) = db::get_item(db, item_id)? else { continue };
        if !shown(&item) {
            continue;
        }
        let base = W_RECENT * recency(&item);
        let hits: Vec<Hit> = db::item_chunks(db, item_id, 2)?
            .into_iter()
            .map(|c| Hit { chunk_id: c.id, locator: c.locator, text: c.text, anchor: c.anchor, score: base })
            .collect();
        if hits.is_empty() {
            continue;
        }
        items.insert(item_id, item);
        by_item.insert(item_id, hits);
    }

    let mut total_hits = 0;
    let mut cards: Vec<Card> = by_item
        .into_iter()
        .map(|(item_id, mut hits)| {
            hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
            total_hits += hits.len();
            let total = hits.len();
            // card score: best hit, a small bonus for breadth, and the name match
            let score = hits[0].score + 0.02 * ((total as f32).ln()) + W_TITLE * title_scores.get(&item_id).copied().unwrap_or(0.0);
            hits.truncate(query.hits.unwrap_or(HITS_PER_CARD).clamp(1, MAX_HITS_PER_CARD));
            let it = &items[&item_id];
            Card {
                item_id,
                kind: it.kind.clone(),
                source: it.source.clone(),
                title: it.title.clone(),
                location: it.location.clone(),
                uri: it.uri.clone(),
                modified_at: it.modified_at.to_rfc3339(),
                size_label: it.size_label.clone(),
                starred: it.starred,
                score,
                hits,
                total_hits: total,
            }
        })
        .collect();

    cards.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    cards.truncate(query.limit);
    if query.item.is_none() {
        let _ = db::push_recent_query(db, q, cards.len()); // a card growing is not a new search
    }

    Ok(Results { cards, total_hits, took_ms: t0.elapsed().as_millis() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewChunk, NewItem};

    fn add(db: &Db, title: &str, text: &str) -> i64 {
        let item = NewItem {
            kind: "pdf".into(),
            source: "C:/books".into(),
            ident: format!("f:{title}"),
            title: title.into(),
            location: "books".into(),
            uri: format!("file:///C:/books/{title}"),
            modified_at: Utc::now(),
            size_label: "1 page".into(),
            size_num: 1,
            content_hash: Some(title.into()),
        };
        let (id, _) = db::upsert_item(db, &item).unwrap();
        db::replace_chunks(db, id, &[NewChunk { ord: 0, locator: "p.1".into(), text: text.into(), anchor: None }]).unwrap();
        id
    }

    #[test]
    fn an_item_is_found_by_its_name_when_its_text_never_says_it() {
        let dir = std::env::temp_dir().join(format!("recall-search-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = db::open(&dir.join("t.sqlite")).unwrap();
        let book = add(&db, "Nameless_book.pdf", "The universe may be infinite in extent.");
        add(&db, "Kitchen quote.pdf", "Cabinets and install, valid for 30 days.");
        let index = Arc::new(parking_lot::RwLock::new(crate::index::VectorIndex::open(&dir.join("v.usearch"), 4).unwrap()));
        let none = Arc::new(Mutex::new(None));
        let by_name = run(&db, &index, &none, &Query { q: "nameless book".into(), limit: 10, ..Default::default() }).unwrap();
        assert_eq!(by_name.cards.first().map(|c| c.item_id), Some(book), "found by its name");
        let by_text = run(&db, &index, &none, &Query { q: "universe infinite".into(), limit: 10, ..Default::default() }).unwrap();
        assert_eq!(by_text.cards.first().map(|c| c.item_id), Some(book), "still found by its text");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn names_read_as_words() {
        assert_eq!(readable_title("Nameless_book-v2.pdf"), "Nameless book v2");
        assert_eq!(readable_title("Comparing the two kitchen quotes"), "Comparing the two kitchen quotes");
        let q = words("nameless book");
        assert_eq!(title_match(&q, "Nameless_books.pdf"), 1.0);
        assert_eq!(title_match(&q, "Book club notes.docx"), 0.5);
        assert_eq!(title_match(&q, "Kitchen quote.pdf"), 0.0);
    }
}
