// Recall Bridge service worker.
// Every 15 minutes (and on demand from the popup) each platform module lists
// conversations, compares against the cursor the desktop app last stored, fetches
// only what changed, and POSTs the batch to 127.0.0.1:47617.
//
// The cursor is the newest `updated_at` the app has accepted for that platform.
// Version one keyed on the conversation list order, which broke when platforms
// changed sorting; comparing timestamps is what makes new chats keep arriving.

import { claude } from "./platforms/claude.js";
import { gemini } from "./platforms/gemini.js";
import { chatgpt } from "./platforms/chatgpt.js";

const APP = "http://127.0.0.1:47617";
const PLATFORMS = { claude, gemini, chatgpt };

chrome.runtime.onInstalled.addListener(() => chrome.alarms.create("sync", { periodInMinutes: 15 }));
chrome.alarms.onAlarm.addListener((a) => { if (a.name === "sync") syncAll(); });
chrome.runtime.onMessage.addListener((msg, _s, reply) => {
  if (msg.type === "sync") { syncAll().then(() => reply({ ok: true })); return true; }
  if (msg.type === "status") { getStatus().then(reply); return true; }
});

async function token() {
  const { token } = await chrome.storage.local.get("token");
  return token || "";
}

async function appFetch(path, init = {}) {
  const t = await token();
  return fetch(APP + path, { ...init, headers: { "content-type": "application/json", "x-recall-token": t, ...(init.headers || {}) } });
}

async function getStatus() {
  const s = await chrome.storage.local.get(["last", "token"]);
  let app = false;
  try { app = (await fetch(APP + "/health")).ok; } catch {}
  return { app, hasToken: !!s.token, last: s.last || {} };
}

async function syncAll() {
  const last = {};
  for (const [name, mod] of Object.entries(PLATFORMS)) {
    try {
      last[name] = await syncOne(name, mod);
    } catch (e) {
      last[name] = { error: String(e), at: Date.now() };
    }
  }
  await chrome.storage.local.set({ last });
}

async function syncOne(name, mod) {
  const curRes = await appFetch(`/cursor/${name}`);
  if (curRes.status === 401) throw new Error("token rejected; paste the token from Recall settings");
  const cursor = (await curRes.json()) || null;
  const since = cursor ? Date.parse(cursor) : 0;

  const list = await mod.list();
  const changed = list.filter((c) => Date.parse(c.updated_at) > since);
  if (!changed.length) return { at: Date.now(), sent: 0, cursor };

  const conversations = [];
  let newest = cursor;
  // oldest first so the cursor only advances past what has been sent
  changed.sort((a, b) => Date.parse(a.updated_at) - Date.parse(b.updated_at));
  for (const c of changed.slice(0, 200)) {
    const full = await mod.fetch(c);
    if (full) {
      conversations.push(full);
      if (!newest || Date.parse(full.updated_at) > Date.parse(newest)) newest = full.updated_at;
    }
    if (conversations.length === 25) {
      await post(name, conversations.splice(0), newest);
    }
  }
  if (conversations.length) await post(name, conversations, newest);
  return { at: Date.now(), sent: changed.length, cursor: newest };
}

async function post(platform, conversations, cursor) {
  const r = await appFetch("/ingest", { method: "POST", body: JSON.stringify({ platform, conversations, cursor }) });
  if (!r.ok) throw new Error(`app returned ${r.status}`);
}
