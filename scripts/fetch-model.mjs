// Downloads the embedding model into src-tauri/models/ and writes model.json
// (name, dim, pooling, query prefix) which the engine reads at boot.
//
//   npm run model            bge-small-en-v1.5, int8, ~33 MB, 384-dim   (bundled default)
//   MODEL=qwen npm run model Qwen3-Embedding-0.6B, int8, ~600 MB, 1024-dim
//
// The vector index is keyed by dimension, so switching models starts a fresh
// index and a re-index; Settings › Data › Rebuild index does that.
import { mkdirSync, createWriteStream, existsSync, writeFileSync } from "node:fs";
import { pipeline } from "node:stream/promises";
import { Readable } from "node:stream";

const MODELS = {
  bge: {
    repo: "Xenova/bge-small-en-v1.5",
    file: "onnx/model_quantized.onnx",
    config: { name: "bge-small-en-v1.5", dim: 384, pooling: "cls", query_prefix: "Represent this sentence for searching relevant passages: " },
  },
  qwen: {
    repo: "onnx-community/Qwen3-Embedding-0.6B-ONNX",
    file: "onnx/model_q8.onnx",
    config: { name: "Qwen3-Embedding-0.6B", dim: 1024, pooling: "mean", query_prefix: "" },
  },
};

const pick = MODELS[process.env.MODEL || "bge"];
if (!pick) { console.error(`unknown MODEL; use one of ${Object.keys(MODELS).join(", ")}`); process.exit(1); }

const OUT = new URL("../src-tauri/models/", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
mkdirSync(OUT, { recursive: true });

for (const [remote, local] of [[pick.file, "model.onnx"], ["tokenizer.json", "tokenizer.json"]]) {
  const dest = OUT + local;
  if (existsSync(dest) && !process.env.FORCE) { console.log(`have ${local} (FORCE=1 to refetch)`); continue; }
  const url = `https://huggingface.co/${pick.repo}/resolve/main/${remote}`;
  console.log(`fetching ${url}`);
  const r = await fetch(url, { redirect: "follow" });
  if (!r.ok) { console.error(`failed ${r.status} for ${url}`); process.exit(1); }
  await pipeline(Readable.fromWeb(r.body), createWriteStream(dest));
  console.log(`saved ${dest}`);
}
writeFileSync(OUT + "model.json", JSON.stringify(pick.config, null, 2));
console.log(`model ready: ${pick.config.name} (${pick.config.dim}-dim, ${pick.config.pooling} pooling)`);
