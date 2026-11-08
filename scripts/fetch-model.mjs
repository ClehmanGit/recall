// Downloads the embedding model into src-tauri/models/ and writes model.json
// (name, dim, pooling, query prefix), which the engine reads at boot.
//
//   npm run model     bge-small-en-v1.5, int8, ~33 MB, 384-dim
//
// Both files come from one fixed revision and must match their SHA-256, so the
// installer always bundles exactly this model. A file already present is
// checked too; FORCE=1 fetches again.
import { createHash } from "node:crypto";
import { mkdirSync, existsSync, readFileSync, writeFileSync } from "node:fs";

const MODEL = {
  repo: "Xenova/bge-small-en-v1.5",
  revision: "ea104dacec62c0de699686887e3f920caeb4f3e3",
  files: [
    { remote: "onnx/model_quantized.onnx", local: "model.onnx", sha256: "6c9c6101a956d62dfb5e7190c538226c0c5bb9cb27b651234b6df063ee7dbfe4" },
    { remote: "tokenizer.json", local: "tokenizer.json", sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66" },
  ],
  config: { name: "bge-small-en-v1.5", dim: 384, pooling: "cls", query_prefix: "Represent this sentence for searching relevant passages: " },
};

const OUT = new URL("../src-tauri/models/", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
mkdirSync(OUT, { recursive: true });

const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

for (const f of MODEL.files) {
  const dest = OUT + f.local;
  if (existsSync(dest) && !process.env.FORCE) {
    if (sha256(readFileSync(dest)) === f.sha256) { console.log(`have ${f.local}`); continue; }
    console.log(`${f.local} does not match; fetching again`);
  }
  const url = `https://huggingface.co/${MODEL.repo}/resolve/${MODEL.revision}/${f.remote}`;
  console.log(`fetching ${url}`);
  const r = await fetch(url, { redirect: "follow" });
  if (!r.ok) { console.error(`failed ${r.status} for ${url}`); process.exit(1); }
  const buf = Buffer.from(await r.arrayBuffer());
  const got = sha256(buf);
  if (got !== f.sha256) { console.error(`${f.local}: SHA-256 ${got} is not the expected ${f.sha256}`); process.exit(1); }
  writeFileSync(dest, buf);
  console.log(`saved ${dest}`);
}
writeFileSync(OUT + "model.json", JSON.stringify(MODEL.config, null, 2));
console.log(`model ready: ${MODEL.config.name} (${MODEL.config.dim}-dim, ${MODEL.config.pooling} pooling)`);
