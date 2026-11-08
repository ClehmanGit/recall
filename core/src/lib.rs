//! Recall engine. The Tauri shell and the probe binary both sit on top of this.

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
    // left by versions that listened for a browser extension
    let _ = std::fs::remove_file(data_dir.join("bridge.token"));

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
    Ok(Engine { db, index, ingest, watch, data_dir })
}

