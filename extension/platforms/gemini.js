// Gemini: Google's batchexecute RPC framework. The CSRF token (SNlM0e) comes from the
// page HTML; RPC ids MaZiqc (list) and hNvQHb (fetch) were worked out from HAR capture
// in version one. The envelope parsing is defensive because Google reshapes these
// arrays without notice: if a shape stops matching, the module returns [] and the
// popup shows the error rather than silently indexing nothing.

const BASE = "https://gemini.google.com";
const RPC_LIST = "MaZiqc";
const RPC_GET = "hNvQHb";

let csrf = null;
async function csrfToken() {
  if (csrf) return csrf;
  const html = await (await fetch(`${BASE}/app`, { credentials: "include" })).text();
  const m = html.match(/"SNlM0e":"([^"]+)"/);
  if (!m) throw new Error("gemini not signed in");
  csrf = m[1];
  return csrf;
}

async function rpc(id, payload) {
  const at = await csrfToken();
  const body = new URLSearchParams({
    "f.req": JSON.stringify([[[id, JSON.stringify(payload), null, "generic"]]]),
    at,
  });
  const r = await fetch(`${BASE}/_/BardChatUi/data/batchexecute?rpcids=${id}&rt=c`, {
    method: "POST", credentials: "include", body,
    headers: { "content-type": "application/x-www-form-urlencoded;charset=UTF-8" },
  });
  if (!r.ok) throw new Error(`gemini rpc ${id} ${r.status}`);
  const text = await r.text();
  // response is )]}'\n then length-prefixed JSON chunks; find the chunk carrying our rpc id
  for (const line of text.split("\n")) {
    if (!line.startsWith("[[")) continue;
    try {
      const arr = JSON.parse(line);
      const hit = arr.find((e) => e[0] === "wrb.fr" && e[1] === id);
      if (hit && hit[2]) return JSON.parse(hit[2]);
    } catch {}
  }
  throw new Error(`gemini rpc ${id}: envelope not recognised`);
}

export const gemini = {
  async list() {
    const out = [];
    let cursor = null;
    for (let page = 0; page < 20; page++) {
      const res = await rpc(RPC_LIST, [cursor, 50]);
      const rows = Array.isArray(res?.[0]) ? res[0] : [];
      for (const row of rows) {
        const id = row?.[0];
        if (typeof id !== "string") continue;
        const title = row?.[1] || "Untitled";
        const ts = row?.[4]?.[0] ?? row?.[3]?.[0]; // seconds; position has moved before
        const updated_at = ts ? new Date(ts * 1000).toISOString() : new Date().toISOString();
        out.push({ id, title, url: `${BASE}/app/${id.replace(/^c_/, "")}`, updated_at });
      }
      cursor = res?.[1] || null;
      if (!cursor || !rows.length) break;
    }
    return out;
  },

  async fetch(s) {
    const res = await rpc(RPC_GET, [s.id, 50, null, 0]);
    const turns = Array.isArray(res?.[0]) ? res[0] : [];
    const messages = [];
    for (const t of turns) {
      const user = t?.[2]?.[0]?.[0];
      const model = t?.[3]?.[0]?.[1]?.[0]?.[0];
      const id = t?.[0]?.[0];
      if (typeof user === "string" && user.trim()) messages.push({ id, role: "user", text: user });
      if (typeof model === "string" && model.trim()) messages.push({ id, role: "assistant", text: model });
    }
    if (!messages.length) return null;
    return { platform: "gemini", id: s.id, title: s.title, url: s.url, updated_at: s.updated_at, messages, attachments: 0 };
  },
};
