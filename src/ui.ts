// Shared pieces: escaping, marks, badges, dates, a small markdown renderer, toast.

import type { Card, Item } from "./api";

export const $ = <T extends HTMLElement>(sel: string, root: ParentNode = document) => root.querySelector(sel) as T;

export function esc(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c] as string));
}

// Marks: platform and language tiles use the real marks from ./assets, formats use lettered badges.
export const PLATFORM_MARK: Record<string, string> = { claude: "claude.svg", gemini: "gemini.svg", chatgpt: "openai.svg" };
export const PLATFORM_LABEL: Record<string, string> = { claude: "Claude", gemini: "Gemini", chatgpt: "ChatGPT" };
const LANG_MARK: Record<string, string> = { py: "python.svg", ts: "typescript.svg", js: "javascript.svg", rs: "rust.svg", go: "go.svg" };
export const FOLDER_SVG = `<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>`;
export const STAR_SVG = `<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z"/></svg>`;
export const OPEN_SVG = `<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 17L17 7M9 7h8v8"/></svg>`;
export const EXCLUDE_SVG = `<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="8"/><path d="M6.5 6.5l11 11"/></svg>`;

export function assetUrl(name: string): string {
  return `./assets/${name}`;
}

export function extOf(title: string): string {
  const i = title.lastIndexOf(".");
  return i > 0 ? title.slice(i + 1).toLowerCase() : "";
}

export function badge(kind: string, source: string, title: string, size = 32): string {
  const img = size >= 32 ? 20 : 15;
  if (kind === "chat") {
    const m = PLATFORM_MARK[source];
    return m
      ? `<span class="badge tile" style="--b:${size}px"><img src="${assetUrl(m)}" alt="" style="width:${img}px;height:${img}px"></span>`
      : `<span class="badge code" style="--b:${size}px">${esc(source.slice(0, 2).toUpperCase())}</span>`;
  }
  const ext = extOf(title);
  if (kind === "code" && LANG_MARK[ext]) {
    return `<span class="badge tile" style="--b:${size}px"><img src="${assetUrl(LANG_MARK[ext])}" alt="" style="width:${img}px;height:${img}px"></span>`;
  }
  const label = kind === "pdf" ? "PDF" : ext.slice(0, 4).toUpperCase() || kind.slice(0, 3).toUpperCase();
  return `<span class="badge ${kind}" style="--b:${size}px">${label}</span>`;
}

export function fmtDate(iso: string): string {
  const d = new Date(iso);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  if (sameDay) return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  const opts: Intl.DateTimeFormatOptions = d.getFullYear() === now.getFullYear() ? { month: "short", day: "numeric" } : { month: "short", day: "numeric", year: "numeric" };
  return d.toLocaleDateString(undefined, opts);
}

/** Time of day, for lists already grouped by day. */
export function fmtTime(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${Math.round(n / 1024)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export function shortPath(p: string): string {
  const parts = p.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts.length > 2 ? `…/${parts.slice(-2).join("/")}` : parts.join("/");
}

const CHEV = `<svg class="chev" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M9 6l6 6-6 6"/></svg>`;
const CHAT_SVG = `<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>`;

/// Where an item lives, as the last folders of its path. Full path stays in the title attribute.
export function crumbs(it: Pick<Item, "location" | "kind" | "source">, depth = 2): string {
  if (it.kind === "chat") {
    const m = PLATFORM_MARK[it.source];
    return `<span class="where" title="${esc(it.location)}"><span class="crumb">${m ? `<img src="${assetUrl(m)}" alt="">` : CHAT_SVG}${esc(it.location)}</span></span>`;
  }
  const parts = it.location.replace(/\\/g, "/").split("/").filter((p) => p && p !== "~");
  const tail = parts.slice(-depth);
  const lead = depth > 1 && parts.length > depth ? `<span class="crumb dim">…</span>${CHEV}` : "";
  return `<span class="where" title="${esc(it.location)}">${lead}${tail.map((p) => `<span class="crumb">${FOLDER_SVG}${esc(p)}</span>`).join(CHEV)}</span>`;
}

export function metaLine(it: Pick<Item, "location" | "modified_at" | "size_label" | "kind" | "source">, depth = 2): string {
  const where = depth > 0 ? `${crumbs(it, depth)}<span class="sep">/</span>` : "";
  return `<div class="meta">${where}<span>${fmtDate(it.modified_at)}</span><span class="sep">/</span><span>${esc(it.size_label)}</span></div>`;
}

export const SPIN = `<i class="spin" aria-hidden="true"></i>`;
export function listHead(dateLabel: string): string {
  return `<div class="list-head" aria-hidden="true"><span></span><span>Name</span><span class="h-loc">Location</span><span>${dateLabel}</span><span class="h-size">Size</span><span></span></div>`;
}
export function skeleton(note: string): string {
  return `<div class="skel"><div class="skel-note">${SPIN}<span>${esc(note)}</span></div><div class="bar" style="width:22%"></div><div class="block"></div><div class="block"></div><div class="block"></div><div class="block"></div></div>`;
}

/// Row meta for lists: folders, date, size in fixed columns so rows line up.
export function rowMeta(it: Pick<Item, "location" | "size_label" | "kind" | "source">, when: string, time = false): string {
  return `<span class="rm"><span class="rloc">${crumbs(it)}</span><span class="when">${time ? fmtTime(when) : fmtDate(when)}</span><span class="size">${esc(it.size_label)}</span></span>`;
}

/// Short meta for small cards: where it lives and when, nothing that has to be cut off.
export function shortMeta(it: Pick<Item, "location" | "modified_at" | "kind" | "source">): string {
  return `<div class="meta">${crumbs(it, 1)}<span class="sep">/</span><span>${fmtDate(it.modified_at)}</span></div>`;
}

// ---- markdown, the subset chunks carry: headings, bold, italic, code spans, lists, paragraphs ----
export function md(src: string): string {
  const lines = src.split("\n");
  const out: string[] = [];
  let para: string[] = [];
  let list: string[] | null = null;
  let ordered = false;
  const flushPara = () => { if (para.length) { out.push(`<p>${inline(para.join(" "))}</p>`); para = []; } };
  const flushList = () => { if (list) { out.push(`<${ordered ? "ol" : "ul"}>${list.map((l) => `<li>${inline(l)}</li>`).join("")}</${ordered ? "ol" : "ul"}>`); list = null; } };
  for (const raw of lines) {
    const line = raw.trimEnd();
    const h = /^(#{1,6})\s+(.*)$/.exec(line);
    const li = /^\s*(?:[-*•]|\d+[.)])\s+(.*)$/.exec(line);
    if (!line.trim()) { flushPara(); flushList(); continue; }
    if (h) { flushPara(); flushList(); out.push(`<h4>${inline(h[2])}</h4>`); continue; }
    if (li) { flushPara(); if (!list) { list = []; ordered = /^\s*\d/.test(line); } list.push(li[1]); continue; }
    flushList();
    para.push(line.trim());
  }
  flushPara(); flushList();
  return out.join("");
}

function inline(s: string): string {
  let t = esc(s);
  t = t.replace(/`([^`]+)`/g, "<code>$1</code>");
  t = t.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  t = t.replace(/(^|[^*])\*([^*]+)\*(?!\*)/g, "$1<em>$2</em>");
  return t;
}

// ---- toast with optional action ----
let toastTimer: number | undefined;
export function toast(msg: string, action?: { label: string; run: () => void }) {
  const el = $("#toast");
  el.innerHTML = esc(msg) + (action ? ` <button class="toast-action">${esc(action.label)}</button>` : "");
  if (action) $(".toast-action", el).onclick = () => { action.run(); el.classList.remove("on"); };
  el.classList.add("on");
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => el.classList.remove("on"), action ? 6000 : 2600);
}

// ---- card body: one shape per kind ----
export function hitBody(c: Pick<Card, "kind" | "title">, text: string, open: boolean, anchor: string | null = null): string {
  if (!open) {
    const first = text.split("\n").find((l) => l.trim()) ?? "";
    const cls = c.kind === "code" ? "mono" : c.kind === "pdf" || c.kind === "document" ? "serif" : "";
    return `<span class="t one ${cls}">${esc(first.replace(/^#+\s*/, ""))}</span>`;
  }
  switch (c.kind) {
    case "chat":
      return `<div class="t bubble">${md(text)}</div>`;
    case "pdf":
    case "document":
    case "text":
      return `<div class="t paper">${md(text)}</div>`;
    case "code": {
      const start = Number(anchor) || 1;
      const rows = text.split("\n").slice(0, 12).map((l, i) => `<div class="ln"><span>${start + i}</span><span>${esc(l) || " "}</span></div>`).join("");
      return `<div class="t codeblock">${rows}</div>`;
    }
    case "spreadsheet": {
      const rows = text.split("\n").slice(0, 8).map((l) => `<div class="tr">${l.split(" | ").map((cell) => `<span>${esc(cell)}</span>`).join("")}</div>`).join("");
      return `<div class="t sheet">${rows}</div>`;
    }
    case "image":
      return `<div class="t tags">${text.split(/[,\n]/).map((w) => w.trim()).filter(Boolean).slice(0, 12).map((w) => `<span>${esc(w)}</span>`).join("")}</div>`;
    default:
      return `<div class="t paper">${md(text)}</div>`;
  }
}
