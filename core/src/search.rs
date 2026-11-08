//! Hybrid search. Vector similarity blended with FTS keyword rank and a light
//! recency boost, then grouped so each item is one card with its best chunks
//! ranked inside it. A long chat cannot flood the list with itself.

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
const HITS_PER_CARD: usize = 5;
/// past this a card is opened instead of grown; the UI stops offering "Show more" here too
pub const MAX_HITS_PER_CARD: usize = 25;

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
                vec_scores.insert(id as i64, sim.max(0.0));
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

    let mut all_ids: Vec<i64> = vec_scores.keys().chain(fts_scores.keys()).copied().collect();
    all_ids.sort_unstable();
    all_ids.dedup();

    let chunks = db::get_chunks(db, &all_ids)?;
    let hidden = db::hidden_platforms(db);
    let mut items: HashMap<i64, db::Item> = HashMap::new();
    let mut by_item: HashMap<i64, Vec<Hit>> = HashMap::new();
    let now = Utc::now();

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
        if item.missing || hidden.contains(&item.source) {
            continue;
        }
        if !query.kinds.is_empty() && !query.kinds.iter().any(|k| k == &item.kind) {
            continue;
        }
        if !query.sources.is_empty() && !query.sources.iter().any(|s| s == &item.source) {
            continue;
        }
        let age_days = (now - item.modified_at).num_days().max(0) as f32;
        let recency = (-age_days / 180.0).exp(); // half-ish weight at six months
        let score = W_VEC * vec_scores.get(&c.id).copied().unwrap_or(0.0)
            + W_FTS * fts_scores.get(&c.id).copied().unwrap_or(0.0)
            + W_RECENT * recency;
        by_item.entry(c.item_id).or_default().push(Hit {
            chunk_id: c.id,
            locator: c.locator,
            text: c.text,
            anchor: c.anchor,
            score,
        });
    }

    let mut total_hits = 0;
    let mut cards: Vec<Card> = by_item
        .into_iter()
        .map(|(item_id, mut hits)| {
            hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
            total_hits += hits.len();
            let total = hits.len();
            // card score: best hit plus a small bonus for breadth
            let score = hits[0].score + 0.02 * ((total as f32).ln());
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
