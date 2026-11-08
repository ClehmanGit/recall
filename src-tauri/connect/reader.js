// Runs inside Recall's own sign-in window for each chat app, injected by the app
// before the page loads. It reads conversations the way the site's own page does,
// with the session that lives in this window, and hands them to the app over the
// webview's internal channel. Nothing here talks to any other address.
//
// Two kinds of backup:
// - quick: starred or pinned chats and the most recent ones. Runs on sign-in and
//   on the app's regular sync, so the newest chats are searchable in seconds.
// - full: everything else, one chat at a time with a pause between, a limited
//   number per run. The app only starts it while the computer is idle or at night.
// Both ask the app which chats it already holds (`connect_known`, id -> updated_at)
// and read only chats that are new or changed, so a full backup resumes where the
// last one stopped.
(() => {
  if (window.top !== window || window.__recall) return;
  const T = window.__TAURI_INTERNALS__;
  if (!T) return; // a sign-in page on another domain: nothing to do here
  const inv = (cmd, args) => T.invoke(cmd, args);
  const host = location.host;
  const platform = host.includes("claude") ? "claude" : host.includes("gemini") ? "gemini" : host.includes("chatgpt") ? "chatgpt" : null;
  if (!platform) return;

  const QUICK_RECENT = 30;   // most recent chats in a quick backup, besides starred ones
  // chats read at once in a quick backup; ChatGPT refuses parallel reads (429)
  const QUICK_PARALLEL = { claude: 3, gemini: 3, chatgpt: 1 };
  // pause between chats in a full backup
  const FULL_PAUSE_MS = { claude: 1500, gemini: 1500, chatgpt: 3000 };
  // refusals in a row (429) before a run stops and leaves it for later
  const MAX_REFUSED = 3;
  const TIMEOUT_MS = 30000;

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const iso = (v) => (typeof v === "number" ? new Date(v * 1000).toISOString() : v || null);
  const time = (v) => { const t = Date.parse(v); return Number.isNaN(t) ? 0 : t; };

  // fetch with a time limit, so a request that never answers becomes an error the
  // person can see instead of a sync that goes quiet; waits and retries on 429.
  // The body is read inside the limit too: a response whose body stalls hangs
  // as surely as one that never starts. Returns { ok, status, text(), json() }.
  async function get(url, opts = {}) {
    for (let attempt = 0; ; attempt++) {
      const ac = new AbortController();
      const timer = setTimeout(() => ac.abort(), TIMEOUT_MS);
      let r, body;
      try {
        r = await fetch(url, { credentials: "include", ...opts, signal: ac.signal });
        body = await r.text();
      } catch (e) {
        if (e.name === "AbortError") throw new Error(`no answer from ${new URL(url, location.href).pathname} after ${TIMEOUT_MS / 1000} s`);
        throw e;
      } finally {
        clearTimeout(timer);
      }
      if (r.status === 429) {
        // a short wait is retried here; a long one (ChatGPT asks about 50 s per
        // read once it is rationing) ends the run so the app can pause instead
        const asked = Number(r.headers.get("retry-after")) || 0;
        if (attempt < 2 && asked <= 15) { await sleep(asked > 0 ? asked * 1000 : 5000 * (attempt + 1)); continue; }
        const e = new Error(`429, asked to wait ${asked || "a while"} s`);
        e.limited = true;
        throw e;
      }
      return { ok: r.ok, status: r.status, text: async () => body, json: async () => JSON.parse(body) };
    }
  }

  // Hidden windows: Chromium slows timers in hidden pages and can freeze them
  // after about five minutes, and Tauri cannot turn that off on Windows. A page
  // holding a Web Lock is not frozen, so each backup runs inside one.
  // `ifAvailable`: a backup asked for while one runs is skipped, not queued.
  const holdingLock = (fn) => (navigator.locks ? navigator.locks.request(`recall-${platform}`, { ifAvailable: true }, (lock) => (lock ? fn() : null)) : fn());

  // ---- claude ----
  const claude = (() => {
    const BASE = "https://claude.ai";
    async function orgId() {
      const r = await get(`${BASE}/api/organizations`);
      if (!r.ok) throw new Error(`not signed in (${r.status})`);
      const orgs = await r.json();
      const org = orgs.find((o) => (o.capabilities || []).includes("chat")) || orgs[0];
      if (!org) throw new Error("no organization");
      return org.uuid;
    }
    return {
      async signedIn() { try { await orgId(); return true; } catch { return false; } },
      async list() {
        const org = await orgId();
        const r = await get(`${BASE}/api/organizations/${org}/chat_conversations`);
        if (!r.ok) throw new Error(`list ${r.status}`);
        return (await r.json()).map((c) => ({ id: c.uuid, title: c.name || "Untitled", url: `${BASE}/chat/${c.uuid}`, updated_at: c.updated_at, starred: !!c.is_starred, _org: org }));
      },
      async fetch(s) {
        const r = await get(`${BASE}/api/organizations/${s._org}/chat_conversations/${s.id}?tree=True&rendering_mode=messages`);
        if (!r.ok) throw new Error(`chat ${r.status}`);
        const c = await r.json();
        const messages = (c.chat_messages || []).map((m) => ({
          id: m.uuid, role: m.sender === "human" ? "user" : "assistant",
          text: (m.content || []).filter((b) => b.type === "text").map((b) => b.text).join("\n") || m.text || "",
        })).filter((m) => m.text.trim());
        const attachments = (c.chat_messages || []).reduce((n, m) => n + (m.attachments?.length || 0) + (m.files?.length || 0), 0);
        return { platform: "claude", id: s.id, title: s.title, url: s.url, updated_at: c.updated_at || s.updated_at, messages, attachments };
      },
    };
  })();

  // ---- gemini ----
  // Google's batchexecute calls. Ids, arguments and positions as documented by
  // several open-source exporters in October 2026 (see FINDINGS.md). Google can
  // rename or reshape them with any deploy; when a shape stops matching, the error
  // says so in plain words.
  const gemini = (() => {
    const BASE = "https://gemini.google.com";
    let tokens = null;
    async function pageTokens() {
      if (tokens) return tokens;
      const html = await (await get(`${BASE}/app`)).text();
      const pick = (k) => (html.match(new RegExp(`"${k}":"([^"]+)"`)) || [])[1];
      const at = pick("SNlM0e");
      if (!at) throw new Error("not signed in");
      tokens = { at, bl: pick("cfb2h"), sid: pick("FdrFJe") };
      return tokens;
    }
    async function rpc(id, args, path = "/app") {
      const { at, bl, sid } = await pageTokens();
      const q = new URLSearchParams({ rpcids: id, "source-path": path, hl: "en", rt: "c" });
      if (bl) q.set("bl", bl);
      if (sid) q.set("f.sid", sid);
      const body = new URLSearchParams({ "f.req": JSON.stringify([[[id, JSON.stringify(args), null, "generic"]]]), at });
      const r = await get(`${BASE}/_/BardChatUi/data/batchexecute?${q}`, {
        method: "POST", body,
        headers: { "content-type": "application/x-www-form-urlencoded;charset=UTF-8", "x-same-domain": "1" },
      });
      if (!r.ok) throw new Error(`rpc ${id} ${r.status}`);
      const text = await r.text();
      for (const line of text.split("\n")) {
        if (!line.startsWith("[")) continue;
        try {
          const arr = JSON.parse(line);
          const hit = arr.find((e) => Array.isArray(e) && e[0] === "wrb.fr" && e[1] === id);
          if (hit) return hit[2] ? JSON.parse(hit[2]) : null;
        } catch {}
      }
      throw new Error(`Gemini changed how it answers (${id}); importing a Google Takeout export still works`);
    }
    return {
      async signedIn() { try { await pageTokens(); return true; } catch { return false; } },
      // stream 1 holds pinned chats, stream 0 the rest; each chat appears once.
      // Row times are creation times: a new message does not move them.
      // A quick backup lists the pinned chats and the newest 40 only.
      async list(quick) {
        const out = [], seen = new Set();
        for (const stream of [1, 0]) {
          let cursor = null;
          for (let page = 0; page < (quick && stream === 0 ? 2 : 40); page++) {
            const res = await rpc("MaZiqc", [20, cursor, [stream, null, 1]]);
            if (res != null && !Array.isArray(res)) throw new Error("Gemini chat list has a shape Recall does not recognise");
            const rows = Array.isArray(res?.[2]) ? res[2] : [];
            for (const row of rows) {
              const id = row?.[0];
              if (typeof id !== "string" || seen.has(id)) continue;
              seen.add(id);
              const ts = row?.[5]?.[0];
              out.push({ id, title: row?.[1] || "Untitled", url: `${BASE}/app/${id.replace(/^c_/, "")}`, updated_at: ts ? iso(ts) : null, starred: stream === 1 || row?.[2] === true });
            }
            const next = res?.[1];
            if (!next || !rows.length || next === cursor) break;
            cursor = next;
          }
        }
        return out;
      },
      async fetch(s) {
        const res = await rpc("hNvQHb", [s.id, 1000, null, 1, [1], [4], null, 1], `/app/${s.id.replace(/^c_/, "")}`);
        const turns = Array.isArray(res?.[0]) ? res[0].slice().reverse() : []; // newest first in the answer
        const messages = [];
        let newest = 0;
        for (const t of turns) {
          const id = t?.[0]?.[1] ?? t?.[0]?.[0];
          const user = t?.[2]?.[0]?.[0];
          const model = t?.[3]?.[0]?.[0]?.[1]?.[0] ?? t?.[3]?.[0]?.[1]?.[0]?.[0];
          const ts = t?.[4]?.[0];
          if (typeof ts === "number") newest = Math.max(newest, ts);
          if (typeof user === "string" && user.trim()) messages.push({ id: id ? `${id}:user` : null, role: "user", text: user });
          if (typeof model === "string" && model.trim()) messages.push({ id: id ? `${id}:model` : null, role: "assistant", text: model });
        }
        if (!messages.length) return null;
        return { platform: "gemini", id: s.id, title: s.title, url: s.url, updated_at: newest ? iso(newest) : s.updated_at || new Date().toISOString(), messages, attachments: 0 };
      },
    };
  })();

  // ---- chatgpt ----
  const chatgpt = (() => {
    const BASE = "https://chatgpt.com";
    async function bearer() {
      const r = await get(`${BASE}/api/auth/session`);
      if (!r.ok) throw new Error(`session ${r.status}`);
      const s = await r.json();
      if (!s.accessToken) throw new Error("not signed in");
      return `Bearer ${s.accessToken}`;
    }
    return {
      async signedIn() { try { await bearer(); return true; } catch { return false; } },
      // newest first; a quick backup lists the first page only. `total` is the
      // site's own count, so progress can be shown without listing everything.
      async list(quick) {
        const auth = await bearer(); const out = []; let offset = 0;
        for (;;) {
          const r = await get(`${BASE}/backend-api/conversations?offset=${offset}&limit=100&order=updated`, { headers: { authorization: auth } });
          if (!r.ok) throw new Error(`list ${r.status}`);
          const page = await r.json();
          for (const c of page.items || []) out.push({ id: c.id, title: c.title || "Untitled", url: `${BASE}/c/${c.id}`, updated_at: iso(c.update_time), starred: !!(c.is_starred || c.pinned_time), _auth: auth });
          offset += (page.items || []).length;
          out.total = page.total || offset;
          if (quick || !page.items?.length || offset >= (page.total || 0) || offset >= 5000) break;
        }
        return out;
      },
      async fetch(s) {
        const r = await get(`${BASE}/backend-api/conversation/${s.id}`, { headers: { authorization: s._auth } });
        if (!r.ok) throw new Error(`chat ${r.status}`);
        const c = await r.json();
        const nodes = c.mapping || {}; const chain = []; let cur = c.current_node;
        while (cur && nodes[cur]) { chain.unshift(nodes[cur]); cur = nodes[cur].parent; }
        const messages = chain.map((n) => n.message)
          .filter((m) => m && m.author && ["user", "assistant"].includes(m.author.role))
          .map((m) => ({ id: m.id, role: m.author.role, text: (m.content?.parts || []).filter((p) => typeof p === "string").join("\n") }))
          .filter((m) => m.text.trim());
        return { platform: "chatgpt", id: s.id, title: c.title || s.title, url: s.url, updated_at: iso(c.update_time) || s.updated_at, messages, attachments: 0 };
      },
    };
  })();

  const mod = { claude, gemini, chatgpt }[platform];
  let running = null;

  const progress = (pass, stage, done, total) => inv("connect_progress", { platform, pass, stage, done, total }).catch(() => {});

  // which chats to read in each kind of backup
  function choose(kind, list, known) {
    const stale = (c) => !(c.id in known) || (c.updated_at && time(c.updated_at) > time(known[c.id]));
    if (kind === "full") return list.filter(stale);
    // gemini lists newest activity first but its row times do not move, so its
    // quick backup re-reads the top of the list and drops unchanged chats after
    const recent = platform === "gemini" ? list : [...list].sort((a, b) => time(b.updated_at) - time(a.updated_at));
    const pick = new Map();
    for (const c of list) if (c.starred) pick.set(c.id, c);
    for (const c of recent.slice(0, QUICK_RECENT)) pick.set(c.id, c);
    return [...pick.values()].filter((c) => platform === "gemini" || stale(c));
  }

  async function backup(kind, limit) {
    if (running) return;
    running = kind;
    let sent = 0;
    try {
      const known = (await inv("connect_known", { platform })) || {};
      await progress(kind, "listing", 0, 0);
      const list = await mod.list(kind === "quick");
      const todo = choose(kind, list, known).slice(0, limit);
      const fetched = new Set();
      let batch = [], done = 0, failed = 0, lastError = null, refused = 0;
      const flush = async () => {
        if (!batch.length) return;
        const out = batch.splice(0);
        sent += out.length;
        await inv("connect_ingest", { platform, conversations: out, cursor: null });
      };
      const step = kind === "quick" ? QUICK_PARALLEL[platform] : 1;
      for (let i = 0; i < todo.length; i += step) {
        const part = todo.slice(i, i + step);
        const got = await Promise.all(part.map((c) => mod.fetch(c).then(
          (full) => { fetched.add(c.id); refused = 0; return full; },
          (e) => { failed++; lastError = e; if (e && e.limited) refused = MAX_REFUSED; else if (String(e && e.message).includes("429")) refused++; return null; },
        )));
        for (const full of got) {
          if (!full) continue;
          if (known[full.id] && time(full.updated_at) <= time(known[full.id])) continue; // unchanged
          batch.push(full);
        }
        done += part.length;
        if (batch.length >= 25) await flush();
        await progress(kind, "reading", done, todo.length);
        if (refused >= MAX_REFUSED) break; // the site wants us to slow down; the app waits
        if (kind === "full") await sleep(FULL_PAUSE_MS[platform]);
      }
      await flush();
      // counts only when the whole list was read; a quick backup lists the
      // newest page, and the sites' own totals are not reliable (ChatGPT's first
      // page claimed 101 of 2,148). The app keeps the last full count.
      const complete = kind === "full" || platform === "claude";
      const total = complete ? list.length : null;
      const remaining = complete ? choose("full", list, known).filter((c) => !fetched.has(c.id)).length : null;
      const error = refused >= MAX_REFUSED
        ? `the site asked Recall to slow down (429) after ${done} of ${todo.length} chats; Recall waits and tries again later`
        : failed ? `${failed} of ${todo.length} chats could not be read (${(lastError && lastError.message) || lastError}); they are tried again next time` : null;
      await inv("connect_report", { platform, signedIn: true, synced: sent, error, pass: kind, total, remaining });
    } catch (e) {
      const signedIn = await mod.signedIn();
      await inv("connect_report", { platform, signedIn, synced: sent, error: String((e && e.message) || e), pass: kind, total: null, remaining: null });
    } finally {
      running = null;
    }
  }

  const quick = () => holdingLock(() => backup("quick", 200));
  const full = (limit) => holdingLock(() => backup("full", limit || 100));

  // Sign-in can finish without a page load: Claude's "Continue with Google"
  // uses a pop-up, and a code typed into the page swaps views in place. So
  // until signed in, look again every few seconds for ten minutes after each
  // page load or Connect click. Only a change is reported, not every look.
  let watch = 0, looksLeft = 0;
  async function check(again) {
    clearTimeout(watch);
    if (!again) looksLeft = 200;
    const ok = await mod.signedIn();
    if (!again || ok) await inv("connect_report", { platform, signedIn: ok, synced: 0, error: null, pass: null, total: null, remaining: null });
    if (ok) { quick(); return; }
    if (looksLeft-- > 0) watch = setTimeout(() => check(true), 3000);
  }

  // `sync` kept so the app's existing calls still work; it runs a quick backup
  window.__recall = { check, quick, full, sync: quick, busy: () => running };
  // the page may still be loading; the readers only need cookies, so start now
  setTimeout(check, 400);
})();
