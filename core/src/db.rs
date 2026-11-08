//! SQLite store. Two axes are kept separate on every item and never folded
//! into the embedding: `kind` (what it is) and `source` (where it came from).
//!
//! kind:   chat | document | pdf | spreadsheet | code | image | text
//! source: claude | gemini | chatgpt | <absolute folder path>

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

pub type Db = Arc<Mutex<Connection>>;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
-- deleted text is overwritten on disk, not just unlinked
PRAGMA secure_delete = ON;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS items (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL,
    source      TEXT NOT NULL,
    -- stable identity: platform conversation id, or blake3 of (device,inode) for files
    ident       TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL,
    location    TEXT NOT NULL,          -- folder path, or platform name
    uri         TEXT NOT NULL,          -- deep link: https://claude.ai/chat/<id> or file:///...
    modified_at TEXT NOT NULL,          -- RFC3339
    size_label  TEXT NOT NULL,          -- "41 turns", "14 pages", "312 lines", "1.2 MB"
    size_num    INTEGER NOT NULL DEFAULT 0,
    content_hash TEXT,
    missing     INTEGER NOT NULL DEFAULT 0,
    starred     INTEGER NOT NULL DEFAULT 0,
    indexed_at  TEXT
);
CREATE INDEX IF NOT EXISTS items_kind ON items(kind);
CREATE INDEX IF NOT EXISTS items_source ON items(source);
CREATE INDEX IF NOT EXISTS items_modified ON items(modified_at);

CREATE TABLE IF NOT EXISTS chunks (
    id        INTEGER PRIMARY KEY,
    item_id   INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    ord       INTEGER NOT NULL,
    locator   TEXT NOT NULL,            -- "p.3", "L44", "turn 12", "OCR"
    text      TEXT NOT NULL,
    anchor    TEXT                      -- message uuid / page number / line, for deep links
);
CREATE INDEX IF NOT EXISTS chunks_item ON chunks(item_id);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    text, content='chunks', content_rowid='id', tokenize='porter unicode61'
);
CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;

CREATE TABLE IF NOT EXISTS sources (
    path       TEXT PRIMARY KEY,
    added_at   TEXT NOT NULL,
    enabled    INTEGER NOT NULL DEFAULT 1
);

-- held the browser extension's place per platform; nothing uses it now
DROP TABLE IF EXISTS sync_cursors;

CREATE TABLE IF NOT EXISTS recent_queries (
    q          TEXT PRIMARY KEY,
    at         TEXT NOT NULL,
    results    INTEGER NOT NULL DEFAULT 0
);

-- single items the user excluded from the result card; keyed by ident so a
-- rename stays excluded, with the path kept for the settings list
CREATE TABLE IF NOT EXISTS excluded (
    ident      TEXT PRIMARY KEY,
    label      TEXT NOT NULL,
    kind       TEXT NOT NULL,
    at         TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL
);
"#;

/// Migrations for databases created before a column existed. Each is idempotent.
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE items ADD COLUMN starred INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE recent_queries ADD COLUMN results INTEGER NOT NULL DEFAULT 0",
];

pub fn open(path: &Path) -> Result<Db> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path).context("open sqlite")?;
    conn.execute_batch(SCHEMA).context("apply schema")?;
    for m in MIGRATIONS {
        let _ = conn.execute(m, []); // fails harmlessly when the column exists
    }
    Ok(Arc::new(Mutex::new(conn)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: i64,
    pub kind: String,
    pub source: String,
    pub ident: String,
    pub title: String,
    pub location: String,
    pub uri: String,
    pub modified_at: DateTime<Utc>,
    pub size_label: String,
    pub size_num: i64,
    pub content_hash: Option<String>,
    pub missing: bool,
    pub starred: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewItem {
    pub kind: String,
    pub source: String,
    pub ident: String,
    pub title: String,
    pub location: String,
    pub uri: String,
    pub modified_at: DateTime<Utc>,
    pub size_label: String,
    pub size_num: i64,
    pub content_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewChunk {
    pub ord: i64,
    pub locator: String,
    pub text: String,
    pub anchor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Chunk {
    pub id: i64,
    pub item_id: i64,
    pub ord: i64,
    pub locator: String,
    pub text: String,
    pub anchor: Option<String>,
}

/// Returns the item id, and whether the content changed (so chunks need re-embedding).
pub fn upsert_item(db: &Db, item: &NewItem) -> Result<(i64, bool)> {
    let conn = db.lock();
    let existing: Option<(i64, Option<String>)> = conn
        .query_row(
            "SELECT id, content_hash FROM items WHERE ident = ?1",
            params![item.ident],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    match existing {
        Some((id, old_hash)) => {
            let changed = old_hash != item.content_hash;
            conn.execute(
                "UPDATE items SET kind=?2, source=?3, title=?4, location=?5, uri=?6, modified_at=?7,
                 size_label=?8, size_num=?9, content_hash=?10, missing=0 WHERE id=?1",
                params![
                    id, item.kind, item.source, item.title, item.location, item.uri,
                    item.modified_at.to_rfc3339(), item.size_label, item.size_num, item.content_hash
                ],
            )?;
            Ok((id, changed))
        }
        None => {
            conn.execute(
                "INSERT INTO items (kind, source, ident, title, location, uri, modified_at, size_label, size_num, content_hash)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    item.kind, item.source, item.ident, item.title, item.location, item.uri,
                    item.modified_at.to_rfc3339(), item.size_label, item.size_num, item.content_hash
                ],
            )?;
            Ok((conn.last_insert_rowid(), true))
        }
    }
}

/// Replaces an item's chunks. Returns the old chunk ids (whose vectors must go)
/// and the new ones in order.
pub fn replace_chunks(db: &Db, item_id: i64, chunks: &[NewChunk]) -> Result<(Vec<i64>, Vec<i64>)> {
    let mut conn = db.lock();
    let tx = conn.transaction()?;
    let old = chunk_ids_where(&tx, "item_id = ?1", params![item_id])?;
    tx.execute("DELETE FROM chunks WHERE item_id = ?1", params![item_id])?;
    let mut ids = Vec::with_capacity(chunks.len());
    {
        let mut stmt = tx.prepare(
            "INSERT INTO chunks (item_id, ord, locator, text, anchor) VALUES (?1,?2,?3,?4,?5)",
        )?;
        for c in chunks {
            stmt.execute(params![item_id, c.ord, c.locator, c.text, c.anchor])?;
            ids.push(tx.last_insert_rowid());
        }
    }
    tx.execute(
        "UPDATE items SET indexed_at = ?2 WHERE id = ?1",
        params![item_id, Utc::now().to_rfc3339()],
    )?;
    tx.commit()?;
    Ok((old, ids))
}

/// The stored content hash of an item; None if unknown or never fully embedded.
pub fn content_hash(db: &Db, ident: &str) -> Result<Option<String>> {
    Ok(db.lock()
        .query_row("SELECT content_hash FROM items WHERE ident = ?1", params![ident], |r| r.get::<_, Option<String>>(0))
        .optional()?
        .flatten())
}

pub fn mark_present(db: &Db, ident: &str) -> Result<()> {
    db.lock().execute("UPDATE items SET missing = 0 WHERE ident = ?1 AND missing != 0", params![ident])?;
    Ok(())
}

/// Items stored while the model was still loading: their text is searchable
/// by keyword but has no vectors yet.
pub fn has_unembedded(db: &Db) -> bool {
    db.lock().query_row("SELECT 1 FROM items WHERE content_hash IS NULL LIMIT 1", [], |_| Ok(())).optional().ok().flatten().is_some()
}

pub fn mark_missing(db: &Db, ident: &str) -> Result<()> {
    db.lock()
        .execute("UPDATE items SET missing = 1 WHERE ident = ?1", params![ident])?;
    Ok(())
}

pub fn get_item(db: &Db, id: i64) -> Result<Option<Item>> {
    let conn = db.lock();
    conn.query_row(
        "SELECT id, kind, source, ident, title, location, uri, modified_at, size_label, size_num, content_hash, missing, starred
         FROM items WHERE id = ?1",
        params![id],
        row_to_item,
    )
    .optional()
    .map_err(Into::into)
}

pub fn get_chunks(db: &Db, ids: &[i64]) -> Result<Vec<Chunk>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let conn = db.lock();
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, item_id, ord, locator, text, anchor FROM chunks WHERE id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
        Ok(Chunk {
            id: r.get(0)?,
            item_id: r.get(1)?,
            ord: r.get(2)?,
            locator: r.get(3)?,
            text: r.get(4)?,
            anchor: r.get(5)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Keyword hits via FTS5, returned as (chunk_id, bm25 score, lower is better).
pub fn fts_search(db: &Db, query: &str, limit: usize) -> Result<Vec<(i64, f64)>> {
    let conn = db.lock();
    let safe = sanitize_fts(query);
    if safe.is_empty() {
        return Ok(vec![]);
    }
    let mut stmt = conn.prepare(
        "SELECT rowid, bm25(chunks_fts) FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY bm25(chunks_fts) LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![safe, limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn sanitize_fts(q: &str) -> String {
    q.split_whitespace()
        .map(|w| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect::<Vec<_>>()
        .join(" OR ")
}


#[derive(Debug, Clone, Serialize)]
pub struct SourceRow {
    pub path: String,
    pub enabled: bool,
    pub count: i64,
}

pub fn list_sources(db: &Db) -> Result<Vec<SourceRow>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT s.path, s.enabled, (SELECT COUNT(*) FROM items i WHERE i.source = s.path AND i.missing = 0)
         FROM sources s ORDER BY s.added_at",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SourceRow { path: r.get(0)?, enabled: r.get::<_, i64>(1)? == 1, count: r.get(2)? })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn add_source(db: &Db, path: &str) -> Result<()> {
    db.lock().execute(
        "INSERT OR IGNORE INTO sources (path, added_at) VALUES (?1, ?2)",
        params![path, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

/// Indexed files under one source folder: item id, file:/// uri and ident.
pub fn file_items(db: &Db, source: &str) -> Result<Vec<(i64, String, String)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare("SELECT id, uri, ident FROM items WHERE source = ?1 AND kind != 'chat'")?;
    let rows = stmt.query_map(params![source], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Takes one item out of the index (its passages go with it) and returns the
/// passage ids, so their vectors can be dropped too. Unlike exclude_item it is
/// not remembered: if the rules allow the file again, it comes back.
fn chunk_ids_where(conn: &Connection, filter: &str, args: impl rusqlite::Params) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(&format!("SELECT id FROM chunks WHERE {filter}"))?;
    let ids = stmt.query_map(args, |r| r.get(0))?.filter_map(|r| r.ok()).collect();
    Ok(ids)
}

/// Deletes the item; returns its chunk ids, whose vectors must go too.
pub fn remove_item(db: &Db, item_id: i64) -> Result<Vec<i64>> {
    let conn = db.lock();
    let chunks = chunk_ids_where(&conn, "item_id = ?1", params![item_id])?;
    conn.execute("DELETE FROM items WHERE id = ?1", params![item_id])?;
    Ok(chunks)
}

pub fn item_id_by_ident(db: &Db, ident: &str) -> Result<Option<i64>> {
    Ok(db.lock().query_row("SELECT id FROM items WHERE ident = ?1", params![ident], |r| r.get(0)).optional()?)
}

/// Forgets a folder and everything read from it; returns the chunk ids whose
/// vectors must go too.
pub fn remove_source(db: &Db, path: &str) -> Result<Vec<i64>> {
    let conn = db.lock();
    let chunks = chunk_ids_where(&conn, "item_id IN (SELECT id FROM items WHERE source = ?1)", params![path])?;
    conn.execute("DELETE FROM sources WHERE path = ?1", params![path])?;
    conn.execute("DELETE FROM items WHERE source = ?1", params![path])?;
    Ok(chunks)
}

pub fn platform_counts(db: &Db) -> Result<Vec<(String, i64)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT source, COUNT(*) FROM items WHERE kind = 'chat' GROUP BY source ORDER BY source",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}



/// Chats held for a platform: conversation id -> modified_at (RFC3339). The
/// sign-in windows read only chats missing here or newer, so a backup resumes
/// where it stopped. Hidden chats count as held, so they are not read again.
pub fn chat_versions(db: &Db, platform: &str) -> Result<std::collections::HashMap<String, String>> {
    let conn = db.lock();
    let prefix = format!("{platform}:");
    let mut out = std::collections::HashMap::new();
    let mut stmt = conn.prepare("SELECT ident, modified_at FROM items WHERE kind = 'chat' AND source = ?1 AND content_hash IS NOT NULL")?;
    for row in stmt.query_map(params![platform], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (ident, modified) = row?;
        if let Some(id) = ident.strip_prefix(&prefix) {
            out.insert(id.to_string(), modified);
        }
    }
    let mut stmt = conn.prepare("SELECT ident FROM excluded WHERE kind = 'chat' AND ident LIKE ?1")?;
    for row in stmt.query_map(params![format!("{prefix}%")], |r| r.get::<_, String>(0))? {
        if let Some(id) = row?.strip_prefix(&prefix) {
            out.insert(id.to_string(), "9999-12-31T00:00:00+00:00".into());
        }
    }
    Ok(out)
}

pub fn chunk_count(db: &Db) -> Result<i64> {
    Ok(db.lock().query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?)
}

pub fn push_recent_query(db: &Db, q: &str, results: usize) -> Result<()> {
    db.lock().execute(
        "INSERT INTO recent_queries (q, at, results) VALUES (?1, ?2, ?3) ON CONFLICT(q) DO UPDATE SET at = excluded.at, results = excluded.results",
        params![q.trim(), Utc::now().to_rfc3339(), results as i64],
    )?;
    Ok(())
}

pub fn recent_queries(db: &Db, n: usize) -> Result<Vec<(String, i64)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare("SELECT q, results FROM recent_queries ORDER BY at DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![n as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn clear_recent_queries(db: &Db) -> Result<()> {
    db.lock().execute("DELETE FROM recent_queries", [])?;
    Ok(())
}

pub fn remove_recent_query(db: &Db, q: &str) -> Result<()> {
    db.lock().execute("DELETE FROM recent_queries WHERE q = ?1", params![q.trim()])?;
    Ok(())
}

fn row_to_item(r: &rusqlite::Row<'_>) -> rusqlite::Result<Item> {
    let modified: String = r.get(7)?;
    Ok(Item {
        id: r.get(0)?,
        kind: r.get(1)?,
        source: r.get(2)?,
        ident: r.get(3)?,
        title: r.get(4)?,
        location: r.get(5)?,
        uri: r.get(6)?,
        modified_at: DateTime::parse_from_rfc3339(&modified)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        size_label: r.get(8)?,
        size_num: r.get(9)?,
        content_hash: r.get(10)?,
        missing: r.get::<_, i64>(11)? == 1,
        starred: r.get::<_, i64>(12)? == 1,
    })
}

const ITEM_COLS: &str = "id, kind, source, ident, title, location, uri, modified_at, size_label, size_num, content_hash, missing, starred";

/// Chat apps switched off in Settings. Their chats stay in the index, so switching
/// back on brings them back at once, but they are left out of search and lists.
pub fn hidden_platforms(db: &Db) -> Vec<String> {
    let enabled: std::collections::HashMap<String, bool> = setting_get(db, "platform_enabled").ok().flatten().unwrap_or_default();
    crate::PLATFORMS.into_iter().filter(|p| enabled.get(*p) == Some(&false)).map(String::from).collect()
}

/// ` AND source NOT IN (...)` for the hidden chat apps. The names come from a fixed list.
fn hidden_clause(db: &Db) -> String {
    let hidden = hidden_platforms(db);
    if hidden.is_empty() { return String::new(); }
    format!(" AND source NOT IN ({})", hidden.iter().map(|p| format!("'{p}'")).collect::<Vec<_>>().join(","))
}

pub fn starred(db: &Db, limit: usize) -> Result<Vec<Item>> {
    let hide = hidden_clause(db);
    let conn = db.lock();
    let mut stmt = conn.prepare(&format!("SELECT {ITEM_COLS} FROM items WHERE starred = 1 AND missing = 0{hide} ORDER BY modified_at DESC LIMIT ?1"))?;
    let rows = stmt.query_map(params![limit as i64], row_to_item)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn recently_indexed(db: &Db, limit: usize) -> Result<Vec<(Item, String)>> {
    let hide = hidden_clause(db);
    let conn = db.lock();
    let mut stmt = conn.prepare(&format!("SELECT {ITEM_COLS}, indexed_at FROM items WHERE missing = 0 AND indexed_at IS NOT NULL{hide} ORDER BY indexed_at DESC LIMIT ?1"))?;
    let rows = stmt.query_map(params![limit as i64], |r| Ok((row_to_item(r)?, r.get::<_, String>(13)?)))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Items for browsing views, newest modified first, optionally limited to kinds.
/// A page of items by modified date, newest first unless `oldest`. Paged so the timeline and
/// gallery reach the whole library, not only its newest few hundred items.
pub fn browse(db: &Db, kinds: &[String], limit: usize, offset: usize, oldest: bool) -> Result<Vec<Item>> {
    let hide = hidden_clause(db);
    let conn = db.lock();
    let mut sql = format!("SELECT {ITEM_COLS} FROM items WHERE missing = 0{hide}");
    if !kinds.is_empty() {
        let ph = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        sql.push_str(&format!(" AND kind IN ({ph})"));
    }
    sql.push_str(if oldest { " ORDER BY modified_at ASC, id ASC LIMIT ? OFFSET ?" } else { " ORDER BY modified_at DESC, id DESC LIMIT ? OFFSET ?" });
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = kinds.iter().map(|k| Box::new(k.clone()) as Box<dyn rusqlite::ToSql>).collect();
    params.push(Box::new(limit as i64));
    params.push(Box::new(offset as i64));
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())), row_to_item)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// An item's passages in document order, for the quick view.
pub fn item_chunks(db: &Db, item_id: i64, limit: usize) -> Result<Vec<Chunk>> {
    let conn = db.lock();
    let mut stmt = conn.prepare("SELECT id, item_id, ord, locator, text, anchor FROM chunks WHERE item_id = ?1 ORDER BY ord LIMIT ?2")?;
    let rows = stmt.query_map(params![item_id, limit as i64], |r| {
        Ok(Chunk { id: r.get(0)?, item_id: r.get(1)?, ord: r.get(2)?, locator: r.get(3)?, text: r.get(4)?, anchor: r.get(5)? })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn set_star(db: &Db, item_id: i64, on: bool) -> Result<()> {
    db.lock().execute("UPDATE items SET starred = ?2 WHERE id = ?1", params![item_id, on as i64])?;
    Ok(())
}

/// Removes the item and records the exclusion so sweeps and syncs skip it.
/// Returns the item and its chunk ids, whose vectors must go too.
pub fn exclude_item(db: &Db, item_id: i64) -> Result<(Option<Item>, Vec<i64>)> {
    let item = get_item(db, item_id)?;
    let mut chunks = Vec::new();
    if let Some(it) = &item {
        let conn = db.lock();
        chunks = chunk_ids_where(&conn, "item_id = ?1", params![item_id])?;
        conn.execute(
            "INSERT OR REPLACE INTO excluded (ident, label, kind, at) VALUES (?1, ?2, ?3, ?4)",
            params![it.ident, if it.kind == "chat" { it.title.clone() } else { it.uri.trim_start_matches("file:///").to_string() }, it.kind, Utc::now().to_rfc3339()],
        )?;
        conn.execute("DELETE FROM items WHERE id = ?1", params![item_id])?;
    }
    Ok((item, chunks))
}

pub fn unexclude(db: &Db, ident: &str) -> Result<()> {
    db.lock().execute("DELETE FROM excluded WHERE ident = ?1", params![ident])?;
    Ok(())
}

pub fn is_excluded(db: &Db, ident: &str) -> Result<bool> {
    Ok(db.lock().query_row("SELECT 1 FROM excluded WHERE ident = ?1", params![ident], |_| Ok(())).optional()?.is_some())
}

#[derive(Debug, Clone, Serialize)]
pub struct ExcludedRow {
    pub ident: String,
    pub label: String,
    pub kind: String,
    pub at: String,
}

pub fn list_excluded(db: &Db) -> Result<Vec<ExcludedRow>> {
    let conn = db.lock();
    let mut stmt = conn.prepare("SELECT ident, label, kind, at FROM excluded ORDER BY at DESC")?;
    let rows = stmt.query_map([], |r| Ok(ExcludedRow { ident: r.get(0)?, label: r.get(1)?, kind: r.get(2)?, at: r.get(3)? }))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn setting_get<T: serde::de::DeserializeOwned>(db: &Db, key: &str) -> Result<Option<T>> {
    let v: Option<String> = db.lock().query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0)).optional()?;
    Ok(match v {
        Some(s) => Some(serde_json::from_str(&s)?),
        None => None,
    })
}

pub fn setting_set<T: Serialize>(db: &Db, key: &str, value: &T) -> Result<()> {
    db.lock().execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}

pub fn setting_del(db: &Db, key: &str) -> Result<()> {
    db.lock().execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(())
}

pub fn set_source_enabled(db: &Db, path: &str, enabled: bool) -> Result<()> {
    db.lock().execute("UPDATE sources SET enabled = ?2 WHERE path = ?1", params![path, enabled as i64])?;
    Ok(())
}

pub fn item_count(db: &Db) -> Result<i64> {
    Ok(db.lock().query_row("SELECT COUNT(*) FROM items WHERE missing = 0", [], |r| r.get(0))?)
}

/// Drops every item and chunk. Sources, rules, exclusions and stars on nothing survive.
pub fn clear_index(db: &Db) -> Result<()> {
    let conn = db.lock();
    conn.execute_batch("DELETE FROM chunks; DELETE FROM items;")?;
    drop(conn);
    shrink(db);
    Ok(())
}

/// After a hide or removal: fold the write-ahead log back in and truncate it,
/// so deleted text does not linger in it.
pub fn shrink(db: &Db) {
    let _ = db.lock().execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(platform: &str, id: &str, at: &str) -> NewItem {
        NewItem {
            kind: "chat".into(),
            source: platform.into(),
            ident: format!("{platform}:{id}"),
            title: id.into(),
            location: platform.into(),
            uri: String::new(),
            modified_at: DateTime::parse_from_rfc3339(at).unwrap().with_timezone(&Utc),
            size_label: "1 turns".into(),
            size_num: 1,
            content_hash: Some(id.into()),
        }
    }

    #[test]
    fn chat_versions_lists_one_platform_and_counts_hidden_chats_as_held() {
        let path = std::env::temp_dir().join(format!("recall-test-{}.sqlite", std::process::id()));
        let db = open(&path).unwrap();
        upsert_item(&db, &chat("claude", "a", "2026-10-01T10:00:00Z")).unwrap();
        upsert_item(&db, &chat("claude", "b", "2026-10-02T10:00:00Z")).unwrap();
        upsert_item(&db, &chat("chatgpt", "c", "2026-10-03T10:00:00Z")).unwrap();
        let (id_b, _) = upsert_item(&db, &chat("claude", "b", "2026-10-02T10:00:00Z")).unwrap();
        exclude_item(&db, id_b).unwrap();

        let v = chat_versions(&db, "claude").unwrap();
        assert_eq!(v.len(), 2);
        assert!(v["a"].starts_with("2026-10-01T10:00:00"));
        assert!(v["b"].starts_with("9999"), "a hidden chat is never read again");
        assert!(!v.contains_key("c"));
        drop(db);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_switched_off_chat_app_is_left_out_of_lists() {
        let path = std::env::temp_dir().join(format!("recall-hidden-test-{}.sqlite", std::process::id()));
        let db = open(&path).unwrap();
        upsert_item(&db, &chat("claude", "a", "2026-10-01T10:00:00Z")).unwrap();
        upsert_item(&db, &chat("chatgpt", "b", "2026-10-02T10:00:00Z")).unwrap();
        let sources = |v: Vec<Item>| v.into_iter().map(|i| i.source).collect::<Vec<_>>();
        assert_eq!(browse(&db, &[], 10, 0, false).unwrap().len(), 2);
        setting_set(&db, "platform_enabled", &std::collections::HashMap::from([("chatgpt".to_string(), false)])).unwrap();
        assert_eq!(hidden_platforms(&db), ["chatgpt"]);
        assert_eq!(sources(browse(&db, &[], 10, 0, false).unwrap()), ["claude"]);
        assert_eq!(chat_versions(&db, "chatgpt").unwrap().len(), 1, "still held, so switching back on needs no re-download");
        drop(db);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn browse_pages_both_ways_without_gaps_or_repeats() {
        let path = std::env::temp_dir().join(format!("recall-browse-test-{}.sqlite", std::process::id()));
        let db = open(&path).unwrap();
        for d in 1..=9 {
            upsert_item(&db, &chat("claude", &format!("c{d}"), &format!("2026-10-0{d}T10:00:00Z"))).unwrap();
        }
        let title = |v: Vec<Item>| v.into_iter().map(|i| i.title).collect::<Vec<_>>();
        let newest: Vec<_> = (0..3).flat_map(|p| title(browse(&db, &[], 3, p * 3, false).unwrap())).collect();
        assert_eq!(newest, ["c9", "c8", "c7", "c6", "c5", "c4", "c3", "c2", "c1"]);
        assert_eq!(title(browse(&db, &[], 4, 0, true).unwrap()), ["c1", "c2", "c3", "c4"], "oldest first reaches the true oldest");
        assert!(browse(&db, &[], 3, 9, false).unwrap().is_empty());
        assert_eq!(browse(&db, &["pdf".into()], 10, 0, false).unwrap().len(), 0);
        drop(db);
        let _ = std::fs::remove_file(&path);
    }
}
