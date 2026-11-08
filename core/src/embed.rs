//! ONNX embedder. Default model: Qwen3-Embedding-0.6B (1024-dim), fetched by
//! `npm run model` into src-tauri/models/. Any sentence-embedding ONNX export with
//! input_ids/attention_mask works; input and output names are discovered from
//! the session rather than hardcoded.

use anyhow::{Context, Result};
use ort::environment::Environment;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use std::path::Path;
use tokenizers::Tokenizer;

const MAX_TOKENS: usize = 512;

/// `models/model.json`, written by `npm run model`. Defaults match the bundled bge-small.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub dim: usize,
    /// "cls" (bge, e5) or "mean" (MiniLM, Qwen3-Embedding)
    pub pooling: String,
    #[serde(default)]
    pub query_prefix: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self { name: "bge-small-en-v1.5".into(), dim: 384, pooling: "cls".into(), query_prefix: "Represent this sentence for searching relevant passages: ".into() }
    }
}

impl ModelConfig {
    pub fn load(model_dir: &Path) -> Self {
        std::fs::read_to_string(model_dir.join("model.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
}

pub struct Embedder {
    _env: Environment,
    session: Session,
    tokenizer: Tokenizer,
    input_ids: String,
    attention_mask: String,
    output: String,
    pub dim: usize,
    pub cfg: ModelConfig,
}

/// ort's builder errors carry the builder back and are not Send; flatten them here.
fn flat<T, R>(r: std::result::Result<T, ort::Error<R>>) -> Result<T> {
    r.map_err(|e| anyhow::anyhow!("{e}"))
}

impl Embedder {
    pub fn load(model_dir: &Path) -> Result<Self> {
        let cfg = ModelConfig::load(model_dir);
        let model = model_dir.join("model.onnx");
        let tok = model_dir.join("tokenizer.json");
        anyhow::ensure!(model.exists(), "model.onnx missing at {}", model.display());
        anyhow::ensure!(tok.exists(), "tokenizer.json missing at {}", tok.display());

        let env = flat(ort::init().with_name("recall").build())?;
        let mut builder = flat(Session::builder(&env))?;
        builder = flat(builder.with_optimization_level(GraphOptimizationLevel::Level3))?;
        builder = flat(builder.with_intra_threads(num_threads()))?;
        let bytes = std::fs::read(&model).context("read model.onnx")?;
        let session = flat(builder.commit_from_memory(&bytes)).context("load onnx session")?;

        let names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
        let input_ids = pick(&names, &["input_ids"]).context("no input_ids input")?;
        let attention_mask = pick(&names, &["attention_mask"]).context("no attention_mask input")?;
        let outs: Vec<String> = session.outputs().iter().map(|o| o.name().to_string()).collect();
        let output = pick(&outs, &["last_hidden_state", "sentence_embedding", "embeddings"])
            .unwrap_or_else(|| outs[0].clone());

        let mut tokenizer = Tokenizer::from_file(&tok).map_err(|e| anyhow::anyhow!("{e}"))?;
        let _ = tokenizer.with_truncation(Some(tokenizers::TruncationParams {
            max_length: MAX_TOKENS,
            ..Default::default()
        }));

        Ok(Self { _env: env, session, tokenizer, input_ids, attention_mask, output, dim: cfg.dim, cfg })
    }

    /// Embeds a search query, applying the model's query prefix where it has one.
    pub fn embed_query(&mut self, q: &str) -> Result<Vec<f32>> {
        let text = format!("{}{}", self.cfg.query_prefix, q);
        Ok(self.embed(&[text])?.remove(0))
    }

    /// Embeds a batch. Pools per the model config (CLS or masked mean), then
    /// L2-normalises, so cosine similarity is a plain dot product in the index.
    pub fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let encs = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let max_len = encs.iter().map(|e| e.get_ids().len()).max().unwrap_or(1);
        let b = encs.len();

        let mut ids = vec![0i64; b * max_len];
        let mut mask = vec![0i64; b * max_len];
        for (i, e) in encs.iter().enumerate() {
            for (j, &t) in e.get_ids().iter().enumerate() {
                ids[i * max_len + j] = t as i64;
                mask[i * max_len + j] = 1;
            }
        }
        let shape_in = vec![b as i64, max_len as i64];
        let ids_t = flat(Tensor::from_array((shape_in.clone(), ids)))?;
        let mask_t = flat(Tensor::from_array((shape_in, mask.clone())))?;
        let outputs = flat(self.session.run(ort::inputs![
            self.input_ids.as_str() => ids_t,
            self.attention_mask.as_str() => mask_t,
        ]))?;

        let (shape, data) = flat(outputs[self.output.as_str()].try_extract_tensor::<f32>())?;
        let dims: Vec<usize> = shape.iter().map(|&d| d as usize).collect();
        let mut result = Vec::with_capacity(b);

        if dims.len() == 3 && self.cfg.pooling == "cls" {
            let (seq, hidden) = (dims[1], dims[2]);
            for i in 0..b {
                let base = (i * seq) * hidden;
                result.push(l2(data[base..base + hidden].to_vec()));
            }
        } else if dims.len() == 3 {
            // [batch, seq, hidden] → masked mean pool
            let (seq, hidden) = (dims[1], dims[2]);
            for i in 0..b {
                let mut v = vec![0f32; hidden];
                let mut n = 0f32;
                for j in 0..seq.min(max_len) {
                    if mask[i * max_len + j] == 1 {
                        let base = (i * seq + j) * hidden;
                        for (k, slot) in v.iter_mut().enumerate() {
                            *slot += data[base + k];
                        }
                        n += 1.0;
                    }
                }
                if n > 0.0 {
                    for slot in v.iter_mut() {
                        *slot /= n;
                    }
                }
                result.push(l2(v));
            }
        } else {
            // [batch, hidden] already pooled
            let hidden = dims[dims.len() - 1];
            for i in 0..b {
                result.push(l2(data[i * hidden..(i + 1) * hidden].to_vec()));
            }
        }
        Ok(result)
    }
}

fn pick(names: &[String], wanted: &[&str]) -> Option<String> {
    for w in wanted {
        if let Some(n) = names.iter().find(|n| n.as_str() == *w) {
            return Some(n.clone());
        }
    }
    None
}

fn l2(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}

fn num_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get().min(8)).unwrap_or(4)
}
