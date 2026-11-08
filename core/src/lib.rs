//! Recall engine. The Tauri shell and the probe binary both sit on top of this.

pub mod bridge;
pub mod chunk;
pub mod feedback;
pub mod rules;
pub mod db;
pub mod embed;
pub mod extract;
pub mod index;
pub mod ingest;
pub mod search;
pub mod watch;

use anyhow::Result;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct Engine {
    pub db: db::Db,
    pub index: index::VecIndex,
    pub ingest: ingest::Ingest,
    pub watch: Arc<Mutex<watch::Watch>>,
    pub token: String,
    pub data_dir: PathBuf,
}

pub fn data_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("Recall")
}

pub fn boot(model_dir: PathBuf) -> Result<Engine> {
    let data_dir = data_dir();
    std::fs::create_dir_all(&data_dir)?;
    let db = db::open(&data_dir.join("recall.sqlite"))?;
    let cfg = embed::ModelConfig::load(&model_dir);
    let index = Arc::new(parking_lot::RwLock::new(index::VectorIndex::open(
        &data_dir.join(format!("vectors-{}.usearch", cfg.dim)),
        cfg.dim,
    )?));
    let ingest = ingest::start(db.clone(), index.clone(), model_dir);
    let watch = watch::start(ingest.tx.clone())?;
    let token = load_or_create_token(&data_dir)?;
    bridge::spawn(db.clone(), ingest.tx.clone(), token.clone(), ingest.progress.clone());

    // watching a large folder tree can take seconds; do it after the window is up
    {
        let (db, watch, tx) = (db.clone(), watch.clone(), ingest.tx.clone());
        std::thread::Builder::new().name("startup-sweep".into()).spawn(move || {
            if let Ok(sources) = db::list_sources(&db) {
                for s in sources.into_iter().filter(|s| s.enabled) {
                    let _ = watch.lock().add_root(Path::new(&s.path));
                    let _ = tx.send(ingest::Job::SweepSource(s.path));
                }
            }
        })?;
    }
    Ok(Engine { db, index, ingest, watch, token, data_dir })
}

fn load_or_create_token(dir: &Path) -> Result<String> {
    let p = dir.join("bridge.token");
    if let Ok(t) = std::fs::read_to_string(&p) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    let seed = format!("{}{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0));
    let t = blake3::hash(seed.as_bytes()).to_hex().to_string();
    std::fs::write(&p, &t)?;
    Ok(t)
}
