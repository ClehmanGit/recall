//! Filesystem watcher. Broad watch on each source root, narrow ingest: only
//! supported extensions reach the worker, debounced so a file being written
//! in pieces is picked up once.

use crate::extract;
use crate::ingest::Job;
use anyhow::Result;
use crossbeam_channel::Sender;
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub struct Watch {
    debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
    roots: Arc<Mutex<HashMap<PathBuf, String>>>,
}

pub fn start(tx: Sender<Job>) -> Result<Arc<Mutex<Watch>>> {
    let roots: Arc<Mutex<HashMap<PathBuf, String>>> = Arc::new(Mutex::new(HashMap::new()));
    let roots2 = roots.clone();
    let debouncer = new_debouncer(
        Duration::from_millis(1500),
        None,
        move |res: DebounceEventResult| {
            let Ok(events) = res else { return };
            let roots = roots2.lock();
            for ev in events {
                for path in &ev.paths {
                    if !extract::supported(path) {
                        continue;
                    }
                    let Some(source) = roots.iter().find(|(r, _)| path.starts_with(r)).map(|(_, s)| s.clone()) else {
                        continue;
                    };
                    use notify::EventKind::*;
                    match ev.kind {
                        Remove(_) => {
                            let _ = tx.send(Job::FileGone { path: path.clone() });
                        }
                        Create(_) | Modify(_) | Any => {
                            if path.exists() {
                                let _ = tx.send(Job::File { path: path.clone(), source: source.clone() });
                            } else {
                                let _ = tx.send(Job::FileGone { path: path.clone() });
                            }
                        }
                        _ => {}
                    }
                }
            }
        },
    )?;
    Ok(Arc::new(Mutex::new(Watch { debouncer, roots })))
}

impl Watch {
    pub fn add_root(&mut self, root: &Path) -> Result<()> {
        self.debouncer.watch(root, RecursiveMode::Recursive)?;
        self.roots.lock().insert(root.to_path_buf(), root.display().to_string());
        Ok(())
    }

    pub fn remove_root(&mut self, root: &Path) -> Result<()> {
        let _ = self.debouncer.unwatch(root);
        self.roots.lock().remove(root);
        Ok(())
    }
}
