// Claude: plain REST with the session cookie, same endpoints version one used.
// Each module exposes list() → [{id, title, url, updated_at}] and fetch(summary) → ChatPayload.

const BASE = "https://claude.ai";

async function orgId() {
  const r = await fetch(`${BASE}/api/organizations`, { credentials: "include" });
  if (!r.ok) throw new Error(`claude orgs ${r.status}`);
  const orgs = await r.json();
  const org = orgs.find((o) => (o.capabilities || []).includes("chat")) || orgs[0];
  if (!org) throw new Error("no claude org");
  return org.uuid;
}

export const claude = {
  async list() {
    const org = await orgId();
    const r = await fetch(`${BASE}/api/organizations/${org}/chat_conversations`, { credentials: "include" });
    if (!r.ok) throw new Error(`claude list ${r.status}`);
    const rows = await r.json();
    return rows.map((c) => ({
      id: c.uuid,
      title: c.name || "Untitled",
      url: `${BASE}/chat/${c.uuid}`,
      updated_at: c.updated_at,
      _org: org,
    }));
  },

  async fetch(s) {
    const r = await fetch(`${BASE}/api/organizations/${s._org}/chat_conversations/${s.id}?tree=True&rendering_mode=messages`, { credentials: "include" });
    if (!r.ok) return null;
    const c = await r.json();
    const msgs = (c.chat_messages || []).map((m) => ({
      id: m.uuid,
      role: m.sender === "human" ? "user" : "assistant",
      text: (m.content || []).filter((b) => b.type === "text").map((b) => b.text).join("\n") || m.text || "",
    })).filter((m) => m.text.trim());
    const attachments = (c.chat_messages || []).reduce((n, m) => n + (m.attachments?.length || 0) + (m.files?.length || 0), 0);
    return { platform: "claude", id: s.id, title: s.title, url: s.url, updated_at: c.updated_at || s.updated_at, messages: msgs, attachments };
  },
};
