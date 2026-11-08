//! Vector index. usearch HNSW keyed by chunk id, persisted to disk beside the
//! database and reloaded at startup. Cosine over L2-normalised vectors.

use anyhow::{Context, Result};
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use usearch::{Index, IndexOptions, MetricKind, ScalarKind};

pub type VecIndex = Arc<RwLock<VectorIndex>>;

pub struct VectorIndex {
    index: Index,
    path: PathBuf,
}

impl VectorIndex {
    pub fn open(path: &Path, dim: usize) -> Result<Self> {
        let opts = IndexOptions {
            dimensions: dim,
            metric: MetricKind::Cos,
            // half precision: half the memory and disk, no visible change in ranking
            quantization: ScalarKind::F16,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 64,
            multi: false,
        };
        let index = Index::new(&opts).context("create usearch index")?;
        if path.exists() {
            index.load(path.to_string_lossy().as_ref()).context("load index")?;
        } else {
            index.reserve(10_000)?;
        }
        Ok(Self { index, path: path.to_path_buf() })
    }

    pub fn add(&mut self, id: u64, v: &[f32]) -> Result<()> {
        if self.index.size() + 1 > self.index.capacity() {
            self.index.reserve(self.index.capacity() * 2 + 1024)?;
        }
        if self.index.contains(id) {
            self.index.remove(id)?;
        }
        self.index.add(id, v)?;
        Ok(())
    }

    pub fn remove(&mut self, id: u64) -> Result<()> {
        if self.index.contains(id) {
            self.index.remove(id)?;
        }
        Ok(())
    }

    pub fn contains(&self, id: u64) -> bool {
        self.index.contains(id)
    }

    /// Removes the vectors of these chunks, so deleted text leaves no trace here.
    pub fn forget(&mut self, chunk_ids: &[i64]) {
        for &id in chunk_ids {
            let _ = self.remove(id as u64);
        }
    }

    /// Empties the index (Rebuild index).
    pub fn clear(&mut self) -> Result<()> {
        self.index.reset()?;
        self.index.reserve(10_000)?;
        Ok(())
    }

    /// Returns (chunk_id, cosine similarity in 0..1).
    pub fn search(&self, q: &[f32], k: usize) -> Result<Vec<(u64, f32)>> {
        let m = self.index.search(q, k)?;
        Ok(m.keys
            .iter()
            .zip(m.distances.iter())
            .map(|(&id, &d)| (id, 1.0 - d))
            .collect())
    }

    pub fn len(&self) -> usize {
        self.index.size()
    }

    pub fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        self.index.save(self.path.to_string_lossy().as_ref())?;
        Ok(())
    }
}
