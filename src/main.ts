// outside the desktop shell (plain `npm run web`), run on sample data
if (!("__TAURI_INTERNALS__" in window)) (await import("./mock")).installMock();

import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import "@fontsource-variable/source-serif-4";
import { api } from "./api";
import { $, esc, toast, shortPath, assetUrl, SPIN, FOLDER_SVG, PLATFORM_MARK, PLATFORM_LABEL } from "./ui";
import { state, runSearch, pickFolders } from "./views/search";
import { renderSettings, connectionLine } from "./views/settings";
import { renderWelcome, welcomeOnScreen } from "./views/welcome";
import { renderGallery, renderTimeline } from "./views/browse";

const sourcesEl = $<HTMLDivElement>("#sources");
const indexingDlg = $<HTMLDialogElement>("#indexing");
const q = $<HTMLInputElement>("#q");

let wasIndexing = false;
let adding = 0; // folders being registered right now, before the engine reports progress
let knownSources = new Set<string>();
let chatStates = ""; // chat app connection states, to redraw Settings when one changes

// ---- views: search (home when empty) and settings, routed by hash ----

function route() {
  const h = location.hash.replace(/^#/, "");
  const [view, qs] = h.split("?");
  const name = view.startsWith("settings") ? "settings" : view === "gallery" || view === "timeline" || view === "welcome" ? view : "search";
  for (const v of ["search", "gallery", "timeline", "welcome", "settings"]) $(`#view-${v}`).classList.toggle("hidden", v !== name);
  document.querySelectorAll<HTMLElement>(".nav-item").forEach((n) => n.classList.toggle("active", n.dataset.view === name));
  if (name === "settings") renderSettings(new URLSearchParams(qs ?? ""));
  else if (name === "gallery") renderGallery($("#gallery"));
  else if (name === "timeline") renderTimeline($("#timeline"));
  else if (name === "welcome") renderWelcome($("#welcome"), true);
  else if (!q.value.trim()) runSearch();
}
window.addEventListener("hashchange", () => { route(); $("#add-menu").classList.remove("on"); });
document.querySelectorAll<HTMLAnchorElement>(".nav-item").forEach((a) => a.addEventListener("click", (e) => {
  e.preventDefault();
  const v = a.dataset.view;
  location.hash = v === "search" ? "" : `#${v}`;
  if (v === "search") q.focus();
}));

document.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") { e.preventDefault(); location.hash = ""; q.focus(); q.select(); }
  if ((e.ctrlKey || e.metaKey) && e.key === ",") { e.preventDefault(); location.hash = "#settings"; }
});

// ---- sidebar sources + status poll ----

function sourceRow(id: string, icon: string, name: string, n: number, isNew: boolean): string {
  const off = state.sources.length && !state.sources.includes(id) ? "off" : "";
  return `<div class="src ${off} ${isNew ? "enter" : ""}" data-source="${esc(id)}" title="${esc(id)}">${icon}<span class="name">${esc(name)}</span><span class="n">${n.toLocaleString()}</span></div>`;
}

async function refreshStatus() {
  const s = await api.status();
  const rows: string[] = [];
  const seen = new Set<string>();
  for (const [p, n] of s.platforms) {
    const m = PLATFORM_MARK[p];
    rows.push(sourceRow(p, m ? `<img src="${assetUrl(m)}" alt="">` : FOLDER_SVG, PLATFORM_LABEL[p] ?? p, n, !knownSources.has(p)));
    seen.add(p);
  }
  for (const src of s.sources) {
    rows.push(sourceRow(src.path, FOLDER_SVG, shortPath(src.path), src.count, !knownSources.has(src.path)));
    seen.add(src.path);
  }
  const next = rows.join("");
  if (sourcesEl.innerHTML !== next) sourcesEl.innerHTML = next;
  knownSources = seen;

  const indexing = s.queued > s.done;
  const starting = !s.embedder_ready && !indexing;
  const busy = indexing || starting || adding > 0;
  const prog = $("#progress");
  prog.classList.toggle("on", busy);
  prog.classList.toggle("indeterminate", busy && (!indexing || s.queued === 0));
  if (indexing) $("#bar").style.width = `${Math.round((s.done / Math.max(s.queued, 1)) * 100)}%`;
  else if (wasIndexing) {
    toast("Up to date");
    if (state.lastQuery) runSearch(); else if (!location.hash.startsWith("#settings")) runSearch();
  }
  wasIndexing = indexing;

  const parts = indexing
    ? [`<span class="state">${SPIN}Adding ${s.done.toLocaleString()} of ${s.queued.toLocaleString()}</span>`, s.current ? `<span class="file">${esc(s.current)}</span>` : ""]
    : adding > 0
      ? [`<span class="state">${SPIN}Reading the folder</span>`, `<span>This can take a while for large folders</span>`]
      : starting
        ? [`<span class="state">${SPIN}Starting the search engine</span>`, `<span>${s.items.toLocaleString()} items ready to browse</span>`]
        : [`<span class="state"><i class="dot"></i>${s.items.toLocaleString()} items</span>`, `<span>${s.chunks.toLocaleString()} chunks</span>`];
  $("#banner-text").innerHTML = parts.filter(Boolean).join(`<span class="sep"></span>`);
  $("#feedback-btn").classList.toggle("hidden", !s.feedback_enabled);

  // the guide's steps tick off as folders are added and chat apps connect
  const guide = welcomeOnScreen();
  if (guide) renderWelcome(guide, guide.id === "welcome", s);

  // chat apps in Settings: redraw when a connection changes state (its buttons
  // change), otherwise just keep each app's line current while a backup runs
  const states = Object.entries(s.connections).map(([p, c]) => `${p}:${c.state}`).sort().join(",");
  const onSettings = location.hash.startsWith("#settings");
  if (onSettings && chatStates && states !== chatStates) renderSettings();
  else if (onSettings) document.querySelectorAll<HTMLElement>("#settings .plat-row").forEach((row) => {
    const el = row.querySelector<HTMLElement>(".plat-line");
    const line = connectionLine(s, row.dataset.platform!);
    if (el && el.textContent !== line) el.textContent = line;
  });
  chatStates = states;
}

// after signing in, the sign-in window closes and Recall comes forward; say what happens next
if ("__TAURI_INTERNALS__" in window) {
  const { listen } = await import("@tauri-apps/api/event");
  listen<string>("connect-signed-in", (e) => {
    toast(`Signed in to ${PLATFORM_LABEL[e.payload] ?? e.payload}. Copying your recent and starred chats first; older ones follow while the computer is idle.`);
    location.hash = "#settings/chats";
  });
}

sourcesEl.addEventListener("click", (e) => {
  const row = (e.target as HTMLElement).closest<HTMLElement>(".src");
  if (!row) return;
  const id = row.dataset.source!;
  state.sources = state.sources.includes(id) ? state.sources.filter((s) => s !== id) : [...state.sources, id];
  sourcesEl.querySelectorAll<HTMLElement>(".src").forEach((r) => r.classList.toggle("off", state.sources.length > 0 && !state.sources.includes(r.dataset.source!)));
  state.lastQuery = "";
  runSearch();
});

async function addSources() {
  const list = await pickFolders(() => { adding++; refreshStatus(); });
  adding = 0;
  if (!list.length) return 0;
  $("#indexing-summary").textContent = list.length === 1
    ? `Recall is reading ${shortPath(list[0])}.`
    : `Recall is reading ${list.length} folders.`;
  await refreshStatus();
  if (!localStorage.getItem("recall.indexing-notice")) { indexingDlg.showModal(); localStorage.setItem("recall.indexing-notice", "1"); }
  else toast(list.length === 1 ? "Adding folder" : `Adding ${list.length} folders`);
  return list.length;
}

const addMenu = $<HTMLDivElement>("#add-menu");
function closeAddMenu() { addMenu.classList.remove("on"); }
$("#add-source").addEventListener("click", (e) => {
  e.stopPropagation();
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
  addMenu.style.top = `${r.bottom + 6}px`; addMenu.style.left = `${r.left}px`;
  addMenu.classList.toggle("on");
});
document.addEventListener("mousedown", (e) => { if (!addMenu.contains(e.target as Node)) closeAddMenu(); });
addMenu.addEventListener("click", async (e) => {
  const row = (e.target as HTMLElement).closest<HTMLElement>("[data-add]");
  if (!row) return;
  closeAddMenu();
  const what = row.dataset.add!;
  if (what === "folders") { await addSources(); return; }
  await connectChat(what);
});

async function connectChat(platform: string) {
  try {
    await api.connectPlatform(platform);
    toast(`Sign in to ${PLATFORM_LABEL[platform]} in the window that opened. Recall never sees your password.`);
  } catch (e) { toast(`Could not open the sign-in window. ${e}`); }
}

document.addEventListener("recall:add-folders", () => { addSources(); });
document.addEventListener("recall:connect", (e) => connectChat((e as CustomEvent<string>).detail));

// ---- feedback ----

const fb = $<HTMLDivElement>("#feedback");
$("#feedback-btn").addEventListener("click", () => { fb.classList.toggle("on"); if (fb.classList.contains("on")) $<HTMLTextAreaElement>("#fb-text").focus(); });
$("#fb-close").addEventListener("click", () => fb.classList.remove("on"));
$("#fb-send").addEventListener("click", async () => {
  const ta = $<HTMLTextAreaElement>("#fb-text");
  const text = ta.value.trim();
  if (!text) return;
  try { await api.sendFeedback(text); ta.value = ""; fb.classList.remove("on"); toast("Feedback sent."); }
  catch (e) { toast(`Could not send. ${e}`); }
});
$<HTMLTextAreaElement>("#fb-text").addEventListener("input", (e) => { $<HTMLButtonElement>("#fb-send").disabled = !(e.target as HTMLTextAreaElement).value.trim(); });

// ---- boot ----
route();
(function poll() { refreshStatus().finally(() => setTimeout(poll, wasIndexing || adding ? 700 : 2000)); })();
