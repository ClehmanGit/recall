// ChatGPT: a JWT from /api/auth/session, then the backend-api conversation endpoints.

const BASE = "https://chatgpt.com";

async function bearer() {
  const r = await fetch(`${BASE}/api/auth/session`, { credentials: "include" });
  if (!r.ok) throw new Error(`chatgpt session ${r.status}`);
  const s = await r.json();
  if (!s.accessToken) throw new Error("chatgpt not signed in");
  return `Bearer ${s.accessToken}`;
}

export const chatgpt = {
  async list() {
    const auth = await bearer();
    const out = [];
    let offset = 0;
    for (;;) {
      const r = await fetch(`${BASE}/backend-api/conversations?offset=${offset}&limit=100&order=updated`, { headers: { authorization: auth } });
      if (!r.ok) throw new Error(`chatgpt list ${r.status}`);
      const page = await r.json();
      for (const c of page.items || []) {
        out.push({ id: c.id, title: c.title || "Untitled", url: `${BASE}/c/${c.id}`, updated_at: c.update_time, _auth: auth });
      }
      offset += (page.items || []).length;
      if (!page.items?.length || offset >= (page.total || 0) || offset >= 2000) break;
    }
    return out;
  },

  async fetch(s) {
    const r = await fetch(`${BASE}/backend-api/conversation/${s.id}`, { headers: { authorization: s._auth } });
    if (!r.ok) return null;
    const c = await r.json();
    // mapping is a tree; walk from current_node back to the root for the active branch
    const nodes = c.mapping || {};
    const chain = [];
    let cur = c.current_node;
    while (cur && nodes[cur]) { chain.unshift(nodes[cur]); cur = nodes[cur].parent; }
    const msgs = chain
      .map((n) => n.message)
      .filter((m) => m && m.author && ["user", "assistant"].includes(m.author.role))
      .map((m) => ({
        id: m.id,
        role: m.author.role,
        text: (m.content?.parts || []).filter((p) => typeof p === "string").join("\n"),
      }))
      .filter((m) => m.text.trim());
    return {
      platform: "chatgpt", id: s.id, title: c.title || s.title, url: s.url,
      updated_at: new Date((c.update_time || 0) * 1000).toISOString(), messages: msgs, attachments: 0,
    };
  },
};
