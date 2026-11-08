//! Ingest worker. One background thread owns the embedder and the index; everything
//! else sends it jobs. Files arrive from the watcher or a sweep; chats arrive from
//! the sign-in windows. Identity for files is (device, inode) hashed, so a rename or
//! move keeps the item and a content change re-embeds it.

use crate::chunk::{self, Unit};
use crate::db::{self, Db, NewItem};
use crate::embed::Embedder;
use crate::extract;
use crate::index::VecIndex;
use crate::rules::{Matcher, Rules};
use anyhow::Result;
use chrono::{DateTime, Utc};
use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub enum Job {
    File { path: PathBuf, source: String },
    FileGone { path: PathBuf },
    Chat(ChatPayload),
    SweepSource(String),
    Save,
}

/// One conversation as a sign-in window's reader sends it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatPayload {
    pub platform: String, // claude | gemini | chatgpt
    pub id: String,
    pub title: String,
    pub url: String,
    pub updated_at: String, // RFC3339
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub attachments: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: Option<String>,
    pub role: String,
    pub text: String,
}

/// Progress counters the UI polls. `queued` counts file and chat jobs handed to the
/// worker; `done` counts jobs it has finished (indexed, skipped as unchanged, or failed).
/// Indexing is active while done < queued.
#[derive(Clone, Default)]
pub struct Progress {
    pub queued: Arc<AtomicUsize>,
    pub done: Arc<AtomicUsize>,
    pub current: Arc<Mutex<String>>,
}

impl Progress {
    pub fn snapshot(&self) -> (usize, usize, String) {
        (self.queued.load(Ordering::Relaxed), self.done.load(Ordering::Relaxed), self.current.lock().clone())
    }
}

#[derive(Clone)]
pub struct Ingest {
    pub tx: Sender<Job>,
    pub embedder: Arc<Mutex<Option<Embedder>>>,
    pub progress: Progress,
}

pub fn start(db: Db, index: VecIndex, model_dir: PathBuf) -> Ingest {
    let (tx, rx) = crossbeam_channel::unbounded::<Job>();
    let embedder: Arc<Mutex<Option<Embedder>>> = Arc::new(Mutex::new(None));
    let progress = Progress::default();

    // load the model off the main thread; search degrades to keyword-only until it lands
    {
        let slot = embedder.clone();
        std::thread::Builder::new()
            .name("embedder-load".into())
            .spawn(move || match Embedder::load(&model_dir) {
                Ok(e) => {
                    info!("embedder ready");
                    *slot.lock() = Some(e);
                }
                Err(e) => warn!("embedder unavailable, keyword search only: {e:#}"),
            })
            .expect("spawn loader");
    }

    {
        let embedder = embedder.clone();
        let tx2 = tx.clone();
        let progress = progress.clone();
        std::thread::Builder::new()
            .name("ingest".into())
            .spawn(move || worker(rx, tx2, db, index, embedder, progress))
            .expect("spawn ingest");
    }

    Ingest { tx, embedder, progress }
}

fn worker(rx: Receiver<Job>, tx: Sender<Job>, db: Db, index: VecIndex, embedder: Arc<Mutex<Option<Embedder>>>, progress: Progress) {
    let mut dirty = 0usize;
    // chats go first: every start re-reads the watched folders, which can queue
    // thousands of files, and chats someone just signed in for should not wait
    let (mut chats, mut rest) = (VecDeque::new(), VecDeque::new());
    let sort = |job: Job, chats: &mut VecDeque<Job>, rest: &mut VecDeque<Job>| {
        if matches!(job, Job::Chat(_)) { chats.push_back(job) } else { rest.push_back(job) }
    };
    loop {
        if chats.is_empty() && rest.is_empty() {
            match rx.recv() {
                Ok(job) => sort(job, &mut chats, &mut rest),
                Err(_) => return,
            }
        }
        while let Ok(job) = rx.try_recv() {
            sort(job, &mut chats, &mut rest);
        }
        let Some(job) = chats.pop_front().or_else(|| rest.pop_front()) else { continue };
        let counted = matches!(job, Job::File { .. } | Job::Chat(_));
        let r = match job {
            Job::File { path, source } => {
                *progress.current.lock() = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                guarded(&path.display().to_string(), || ingest_file(&db, &index, &embedder, &path, &source))
            }
            Job::FileGone { path } => {
                let ident = file_ident(&path).unwrap_or_default();
                db::mark_missing(&db, &ident).map(|_| 0)
            }
            Job::Chat(p) => {
                *progress.current.lock() = p.title.clone();
                let what = format!("{}:{}", p.platform, p.id);
                guarded(&what, || ingest_chat(&db, &index, &embedder, p))
            }
            Job::SweepSource(root) => {
                let n = sweep(&tx, &db, &index, &root);
                progress.queued.fetch_add(n, Ordering::Relaxed);
                Ok(0)
            }
            Job::Save => {
                if let Err(e) = index.read().save() {
                    warn!("index save failed: {e:#}");
                }
                dirty = 0;
                Ok(0)
            }
        };
        if counted {
            progress.done.fetch_add(1, Ordering::Relaxed);
            let (q, d, _) = progress.snapshot();
            if d >= q {
                *progress.current.lock() = String::new();
                let _ = tx.send(Job::Save);
            }
        }
        match r {
            Ok(n) => {
                dirty += n;
                if dirty >= 200 {
                    let _ = tx.send(Job::Save);
                }
            }
            Err(e) => warn!("ingest error: {e:#}"),
        }
    }
}

/// Runs one job so that a crash inside a reader (pdf-extract panics on some
/// PDFs) skips that item instead of ending this thread. A dead worker stops all
/// indexing, chats included, until Recall restarts.
fn guarded(what: &str, f: impl FnOnce() -> Result<usize>) -> Result<usize> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("reader crashed on {what}; skipped")))
}

pub fn load_rules(db: &Db) -> Rules {
    db::setting_get::<Rules>(db, "rules").ok().flatten().unwrap_or_default()
}

/// Queues every file under `root` that the rules allow. Returns how many were queued.
pub fn sweep(tx: &Sender<Job>, db: &Db, index: &VecIndex, root: &str) -> usize {
    let m = Matcher::new(load_rules(db));
    let allowed = walk(&m, root);
    if prune(db, index, root, &allowed) > 0 {
        let _ = tx.send(Job::Save);
    }
    let mut n = 0;
    for (p, _) in allowed {
        if let Ok(ident) = file_ident(&p) {
            if db::is_excluded(db, &ident).unwrap_or(false) {
                continue;
            }
        }
        let _ = tx.send(Job::File { path: p, source: root.to_string() });
        n += 1;
    }
    n
}

/// Removes indexed files under `root` that the walk no longer finds: the rules now
/// skip them (a tightened rule, a newly skipped folder) or they were deleted or
/// moved. Also removes stale twins: a file rewritten in place can get a new file
/// id, leaving an old item with the same path. Nothing happens when `root` itself
/// is missing, so a drive that is not plugged in does not empty the index.
fn prune(db: &Db, index: &VecIndex, root: &str, allowed: &[(PathBuf, u64)]) -> usize {
    if !Path::new(root).is_dir() {
        return 0;
    }
    let walked: std::collections::HashMap<String, &PathBuf> = allowed.iter().map(|(p, _)| (file_uri(p), p)).collect();
    let items = db::file_items(db, root).unwrap_or_default();
    let mut per_uri: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (_, uri, _) in &items {
        *per_uri.entry(uri.as_str()).or_default() += 1;
    }
    let mut removed = 0;
    for (id, uri, ident) in &items {
        // the ident is computed from the path exactly as the walk gives it, as when indexed
        let stale = match walked.get(uri) {
            None => true,
            Some(path) => per_uri[uri.as_str()] > 1 && file_ident(path).is_ok_and(|now| now != *ident),
        };
        let id = *id;
        if stale && drop_item(db, index, id) {
            removed += 1;
        }
    }
    if removed > 0 {
        info!("{removed} items under {root} left the index: skipped by the rules, deleted, or duplicates");
    }
    removed
}

/// Removes an item and its passages' vectors. True when it was removed.
fn drop_item(db: &Db, index: &VecIndex, id: i64) -> bool {
    let Ok(chunks) = db::remove_item(db, id) else { return false };
    let mut ix = index.write();
    for c in chunks {
        let _ = ix.remove(c as u64);
    }
    true
}

/// A file the rules no longer allow comes out of the index if it was in it.
fn drop_file(db: &Db, index: &VecIndex, path: &Path) {
    if let Ok(Some(id)) = file_ident(path).and_then(|ident| db::item_id_by_ident(db, &ident)) {
        drop_item(db, index, id);
    }
}

fn file_uri(path: &Path) -> String {
    format!("file:///{}", path.display().to_string().replace('\\', "/"))
}

/// Files under `root` the rules allow, with sizes. Shared by sweep and the rules preview.
pub fn walk(m: &Matcher, root: &str) -> Vec<(PathBuf, u64)> {
    let root_path = Path::new(root);
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root_path)
        .follow_links(m.rules().follow_links)
        .into_iter()
        .filter_entry(|e| m.allows_entry(e.path(), e.depth()))
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        let size = entry.metadata().map(|md| md.len()).unwrap_or(0);
        if extract::supported(p) && m.allows_file(p, size, extract::kind_of(p)) {
            out.push((p.to_path_buf(), size));
        }
    }
    out
}

/// Counts for the rules preview: (would index, excluded by rules) under every enabled source.
pub fn preview(db: &Db, rules: &Rules) -> (usize, usize) {
    let m = Matcher::new(rules.clone());
    let permissive = Matcher::new(Rules { skip_dev_folders: false, skip_hidden: false, skip_packages: false, skip_media: false, max_mb: 0, skipped_folders: vec![], only_kinds: vec![], ..rules.clone() });
    let (mut keep, mut all) = (0, 0);
    for s in db::list_sources(db).unwrap_or_default() {
        if !s.enabled {
            continue;
        }
        keep += walk(&m, &s.path).len();
        all += walk(&permissive, &s.path).len();
    }
    (keep, all.saturating_sub(keep))
}

fn ingest_file(
    db: &Db,
    index: &VecIndex,
    embedder: &Arc<Mutex<Option<Embedder>>>,
    path: &Path,
    source: &str,
) -> Result<usize> {
    let meta = std::fs::metadata(path)?;
    let m = Matcher::new(load_rules(db));
    if !m.allows_file(path, meta.len(), extract::kind_of(path)) || m.in_code_project(path, Path::new(source)) {
        drop_file(db, index, path);
        return Ok(0);
    }
    if db::is_excluded(db, &file_ident(path)?)? {
        return Ok(0);
    }
    let hash = blake3::hash(&std::fs::read(path)?).to_hex().to_string();
    let modified: DateTime<Utc> = meta.modified().map(DateTime::<Utc>::from).unwrap_or_else(|_| Utc::now());
    let ex = extract::extract(path)?;
    if m.rules().skip_textless_images && ex.kind == "image" && ex.units.iter().map(|u| u.text.trim().len()).sum::<usize>() < 20 {
        drop_file(db, index, path);
        return Ok(0);
    }
    let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();

    let item = NewItem {
        kind: ex.kind.to_string(),
        source: source.to_string(),
        ident: file_ident(path)?,
        title: path.file_name().and_then(|n| n.to_str()).unwrap_or("untitled").to_string(),
        location: short_location(&folder, source),
        uri: file_uri(path),
        modified_at: modified,
        size_label: ex.size_label,
        size_num: ex.size_num,
        content_hash: Some(hash),
    };
    let (item_id, changed) = db::upsert_item(db, &item)?;
    if !changed {
        return Ok(0);
    }
    embed_and_store(db, index, embedder, item_id, ex.units)
}

fn ingest_chat(
    db: &Db,
    index: &VecIndex,
    embedder: &Arc<Mutex<Option<Embedder>>>,
    p: ChatPayload,
) -> Result<usize> {
    let joined: String = p.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>().join("\n");
    let hash = blake3::hash(joined.as_bytes()).to_hex().to_string();
    let modified = DateTime::parse_from_rfc3339(&p.updated_at)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let ident = format!("{}:{}", p.platform, p.id);
    if db::is_excluded(db, &ident)? {
        return Ok(0);
    }
    let item = NewItem {
        kind: "chat".into(),
        source: p.platform.clone(),
        ident,
        title: if p.title.trim().is_empty() { "Untitled".into() } else { p.title.clone() },
        location: platform_label(&p.platform),
        uri: p.url.clone(),
        modified_at: modified,
        size_label: format!("{} turns", p.messages.len()),
        size_num: p.messages.len() as i64,
        content_hash: Some(hash),
    };
    let (item_id, changed) = db::upsert_item(db, &item)?;
    if !changed {
        return Ok(0);
    }
    // Each message is a unit. Role is kept out of the card (you decided that) but
    // stays in the anchor so a click can deep-link to the exact message.
    let units = p
        .messages
        .into_iter()
        .enumerate()
        .map(|(i, m)| Unit {
            locator: format!("turn {}", i + 1),
            text: m.text,
            anchor: m.id,
        })
        .collect();
    embed_and_store(db, index, embedder, item_id, units)
}

fn embed_and_store(
    db: &Db,
    index: &VecIndex,
    embedder: &Arc<Mutex<Option<Embedder>>>,
    item_id: i64,
    units: Vec<Unit>,
) -> Result<usize> {
    let chunks = chunk::chunk_units(units);
    let ids = db::replace_chunks(db, item_id, &chunks)?;
    let mut guard = embedder.lock();
    let Some(e) = guard.as_mut() else {
        // model not ready: chunks are in FTS already; vectors get backfilled by a later sweep
        return Ok(chunks.len());
    };
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
    for (batch_ids, batch_texts) in ids.chunks(16).zip(texts.chunks(16)) {
        let vecs = e.embed(batch_texts)?;
        let mut idx = index.write();
        for (id, v) in batch_ids.iter().zip(vecs.iter()) {
            idx.add(*id as u64, v)?;
        }
    }
    Ok(chunks.len())
}

/// Stable identity across renames: hash of (volume, inode) on Unix, (volume serial, file index) on Windows.
pub fn file_ident(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return Ok(format!("f:{:x}:{:x}", meta.dev(), meta.ino()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // file_index/volume_serial need the by_handle API on stable; fall back to a path+created hash
        let created = meta.created().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let key = format!("{}|{}|{}", path.display(), created, meta.file_attributes());
        return Ok(format!("f:{}", blake3::hash(key.as_bytes()).to_hex()));
    }
    #[allow(unreachable_code)]
    Ok(format!("f:{}", blake3::hash(path.display().to_string().as_bytes()).to_hex()))
}

fn short_location(folder: &str, source: &str) -> String {
    // "~/Downloads/papers" style: strip the home prefix
    let home = dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default();
    let f = folder.replace('\\', "/");
    let h = home.replace('\\', "/");
    if !h.is_empty() && f.starts_with(&h) {
        format!("~{}", &f[h.len()..])
    } else if f.is_empty() {
        source.to_string()
    } else {
        f
    }
}

pub fn platform_label(p: &str) -> String {
    match p {
        "claude" => "Claude".into(),
        "gemini" => "Gemini".into(),
        "chatgpt" => "ChatGPT".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::guarded;

    #[test]
    fn a_reader_crash_becomes_an_error_and_the_thread_goes_on() {
        let r = guarded("broken.pdf", || panic!("unwrap on None"));
        assert!(r.unwrap_err().to_string().contains("broken.pdf"));
        assert_eq!(guarded("fine.txt", || Ok(3)).unwrap(), 3);
    }
}

#[cfg(test)]
mod prune_tests {
    use super::{file_uri, prune};
    use crate::db::{self, NewItem};

    fn test_index(dir: &std::path::Path) -> crate::index::VecIndex {
        std::sync::Arc::new(parking_lot::RwLock::new(crate::index::VectorIndex::open(&dir.join("v.usearch"), 4).unwrap()))
    }

    fn file_item(source: &str, path: &std::path::Path, ident: &str) -> NewItem {
        NewItem {
            kind: "text".into(),
            source: source.into(),
            ident: ident.into(),
            title: path.file_name().unwrap().to_string_lossy().into(),
            location: source.into(),
            uri: file_uri(path),
            modified_at: chrono::Utc::now(),
            size_label: "1 line".into(),
            size_num: 1,
            content_hash: None,
        }
    }

    #[test]
    fn prune_drops_skipped_deleted_and_twin_items_but_not_for_an_unplugged_drive() {
        let root = std::env::temp_dir().join(format!("recall-prune-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let (kept, skipped, gone) = (root.join("kept.txt"), root.join("skipped.txt"), root.join("gone.txt"));
        std::fs::write(&kept, "a").unwrap();
        std::fs::write(&skipped, "b").unwrap();
        let src = root.display().to_string();
        let db = db::open(&root.join("t.sqlite")).unwrap();
        let real = |p: &std::path::Path| super::file_ident(p).unwrap();
        db::upsert_item(&db, &file_item(&src, &kept, &real(&kept))).unwrap();
        db::upsert_item(&db, &file_item(&src, &kept, "f:old-id")).unwrap(); // the file was rewritten: a twin
        db::upsert_item(&db, &file_item(&src, &skipped, &real(&skipped))).unwrap();
        db::upsert_item(&db, &file_item(&src, &gone, "f:deleted")).unwrap();

        // the rules allow only kept.txt; skipped.txt exists, gone.txt was deleted
        let index = test_index(&root);
        assert_eq!(prune(&db, &index, &src, &[(kept.clone(), 1)]), 3);
        let left: Vec<(String, String)> = db::file_items(&db, &src).unwrap().into_iter().map(|(_, u, i)| (u, i)).collect();
        assert_eq!(left, [(file_uri(&kept), real(&kept))], "only the current record of the allowed file stays");

        // a source folder that is not there (unplugged drive) removes nothing
        db::upsert_item(&db, &file_item(&src, &gone, "f:deleted")).unwrap();
        assert_eq!(prune(&db, &index, &root.join("unplugged").display().to_string(), &[]), 0);
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }
}
