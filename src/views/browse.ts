// Gallery: visual items as tiles, with kind filter and sort. Timeline: everything grouped by
// month and day, with a month rail for jumping. Both page through the whole library from the
// engine, newest or oldest first. Clicking opens the quick view; star, hide and open come from ../actions.

import { api, type Item } from "../api";
import { esc, badge, metaLine, rowMeta, listHead, skeleton } from "../ui";
import { itemAttrs, actionButtons, onItemChange, fadeOut } from "../actions";
import { openViewer } from "./viewer";

const VISUAL = ["image", "pdf", "document", "spreadsheet"];
const KIND_CHIPS: [string, string][] = [["", "All"], ["image", "Images"], ["pdf", "PDFs"], ["document", "Documents"], ["spreadsheet", "Sheets"]];
const TL_CHIPS: [string, string][] = [["", "All"], ["chat", "Chats"], ["document,pdf,text,spreadsheet", "Documents"], ["code", "Code"], ["image", "Images"]];

type Sort = "newest" | "oldest" | "name" | "size" | "kind";
const SORTS: [Sort, string][] = [["newest", "Newest"], ["oldest", "Oldest"], ["name", "Name"], ["size", "Size"], ["kind", "Type"]];
/** Items per page from the engine. Name, size and type sorts work on one larger page instead. */
const PAGE = 200;
const SORTED_PAGE = 500;

/** `capped`: a name, size or type sort filled its one page, so it covers only the newest items. */
interface Paged { kind: string; sort: Sort; items: Item[]; done: boolean; capped: boolean }
const gallery: Paged = { kind: "", sort: "newest", items: [], done: false, capped: false };
const timeline: Paged = { kind: "", sort: "newest", items: [], done: false, capped: false };
const shown = new Map<number, Item>();

function sortItems(list: Item[], sort: Sort): Item[] {
  const by = {
    newest: (a: Item, b: Item) => Date.parse(b.modified_at) - Date.parse(a.modified_at),
    oldest: (a: Item, b: Item) => Date.parse(a.modified_at) - Date.parse(b.modified_at),
    name: (a: Item, b: Item) => a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true }),
    size: (a: Item, b: Item) => b.size_num - a.size_num,
    kind: (a: Item, b: Item) => a.kind.localeCompare(b.kind) || a.title.localeCompare(b.title),
  }[sort];
  return [...list].sort(by);
}

/** Load the next page into `p`. Date sorts page from the engine; the others take one larger page. */
async function loadPage(p: Paged, kinds: string[]): Promise<void> {
  const byDate = p.sort === "newest" || p.sort === "oldest";
  const size = byDate ? PAGE : SORTED_PAGE;
  const page = await api.browse(kinds, size, byDate ? p.items.length : 0, p.sort === "oldest");
  const have = new Set(p.items.map((i) => i.id));
  p.items = byDate ? [...p.items, ...page.filter((i) => !have.has(i.id))] : page;
  p.done = !byDate || page.length < size;
  p.capped = !byDate && page.length >= size;
  for (const it of page) shown.set(it.id, it);
}

function reset(p: Paged) { p.items = []; p.done = false; p.capped = false; }

function tools(chips: [string, string][], p: Paged, sorts: [Sort, string][]): string {
  const n = p.items.length;
  return `<div class="browse-tools">
    ${chips.map(([k, l]) => `<button class="chip small ${k === p.kind ? "active" : ""}" data-kind="${k}">${l}</button>`).join("")}
    <span class="spacer"></span>
    <span class="hint" ${p.capped ? `title="Only the newest ${n.toLocaleString()} are sorted this way. Newest and Oldest page through everything."` : ""}>${p.capped ? "Newest " : ""}${n.toLocaleString()}${p.done ? "" : "+"} items</span>
    <label class="sort">Sort <select class="sort-sel">${sorts.map(([v, l]) => `<option value="${v}" ${v === p.sort ? "selected" : ""}>${l}</option>`).join("")}</select></label>
  </div>`;
}

function moreButton(p: Paged): string {
  if (p.done) return "";
  return `<div class="list-foot center"><button class="ghost page-more">${p.sort === "oldest" ? "Show newer" : "Show older"}</button></div>`;
}

async function pageMore(root: HTMLElement, p: Paged, kinds: string[], render: (root: HTMLElement, quiet: boolean) => Promise<void>) {
  const b = root.querySelector<HTMLButtonElement>(".page-more");
  if (b) { b.disabled = true; b.textContent = "Loading"; }
  await loadPage(p, kinds);
  await render(root, true);
}

// ---- gallery ----

function tile(it: Item, i: number): string {
  const path = it.uri.startsWith("file:///") ? it.uri.replace(/^file:\/\/\//, "") : "";
  const thumb = it.kind === "image" && path && !(window as any).__RECALL_MOCK__
    ? `<img src="${esc(toAsset(path))}" alt="" loading="lazy">`
    : badge(it.kind, it.source, it.title, 32);
  return `<article class="tile" style="--i:${Math.min(i, 24)}" ${itemAttrs(it)}>
    <div class="thumb">${thumb}${actionButtons(it.starred)}</div>
    <div class="tbody"><span class="title" title="${esc(it.title)}">${esc(it.title)}</span>${metaLine(it, 0)}</div>
  </article>`;
}

// Tauri serves local files to the webview through the asset protocol; the mock just shows a badge.
function toAsset(path: string): string {
  return `http://asset.localhost/${encodeURI(path.replace(/\\/g, "/"))}`;
}

const galleryKinds = () => (gallery.kind ? [gallery.kind] : VISUAL);

export async function renderGallery(root: HTMLElement, quiet = false) {
  if (!quiet) {
    if (!root.querySelector(".gallery-grid")) root.innerHTML = `<div class="browse-head"><h1>Gallery</h1></div>${skeleton("Loading")}`;
    reset(gallery);
    await loadPage(gallery, galleryKinds());
  }
  const items = sortItems(gallery.items, gallery.sort);
  root.innerHTML = `<div class="browse-head"><h1>Gallery</h1></div>
    ${tools(KIND_CHIPS, gallery, SORTS)}
    ${items.length ? `<div class="gallery-grid">${items.map((it, i) => tile(it, quiet ? 0 : i)).join("")}</div>${moreButton(gallery)}` : `<div class="hint">Images and documents appear here.</div>`}`;
  root.querySelectorAll<HTMLButtonElement>(".chip").forEach((b) => b.onclick = () => { gallery.kind = b.dataset.kind!; renderGallery(root); });
  root.querySelector<HTMLSelectElement>(".sort-sel")!.onchange = (e) => { gallery.sort = (e.target as HTMLSelectElement).value as Sort; renderGallery(root); };
  root.querySelector<HTMLButtonElement>(".page-more")?.addEventListener("click", () => pageMore(root, gallery, galleryKinds(), renderGallery));
}

// ---- timeline ----

const timelineKinds = () => (timeline.kind ? timeline.kind.split(",") : []);

export async function renderTimeline(root: HTMLElement, quiet = false) {
  if (!quiet) {
    if (!root.querySelector(".tl")) root.innerHTML = `<div class="browse-head"><h1>Timeline</h1></div>${skeleton("Loading")}`;
    reset(timeline);
    await loadPage(timeline, timelineKinds());
  }
  const items = sortItems(timeline.items, timeline.sort);

  // month → day → items, in the order the sort produced
  const months = new Map<string, { label: string; year: string; days: Map<string, Item[]> }>();
  for (const it of items) {
    const d = new Date(it.modified_at);
    const mk = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
    const m = months.get(mk) ?? months.set(mk, { label: d.toLocaleDateString(undefined, { month: "long", year: "numeric" }), year: String(d.getFullYear()), days: new Map() }).get(mk)!;
    const dk = d.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
    (m.days.get(dk) ?? m.days.set(dk, []).get(dk)!).push(it);
  }
  const monthCount = (m: { days: Map<string, Item[]> }) => Array.from(m.days.values()).reduce((a, l) => a + l.length, 0);

  let lastYear = "";
  const rail = Array.from(months).map(([mk, m]) => {
    const yr = m.year !== lastYear ? `<div class="yr">${m.year}</div>` : "";
    lastYear = m.year;
    return `${yr}<button data-month="${mk}"><span>${m.label.replace(/ \d{4}$/, "")}</span><span class="n">${monthCount(m)}</span></button>`;
  }).join("");

  const body = Array.from(months).map(([mk, m]) => `<section class="month" id="m-${mk}">
      <h2>${esc(m.label)}<span class="n">${monthCount(m)}</span></h2>
      ${Array.from(m.days).map(([day, list]) => `<section class="day">
        <div class="section-row"><span class="section">${esc(day.toUpperCase())}</span><span class="n">${list.length}</span></div>
        <div class="list cols">${list.map((it) => `<div class="row" ${itemAttrs(it)}>
          ${badge(it.kind, it.source, it.title, 24)}<span class="rt">${esc(it.title)}</span>${rowMeta(it, it.modified_at, true)}${actionButtons(it.starred)}</div>`).join("")}</div>
      </section>`).join("")}
    </section>`).join("");

  const top = root.scrollTop;
  root.innerHTML = `<div class="browse-head"><h1>Timeline</h1></div>
    ${tools(TL_CHIPS, timeline, SORTS.filter(([v]) => v === "newest" || v === "oldest"))}
    ${items.length ? `<div class="tl"><div class="tl-body"><div class="tl-sticky"><div class="list cols tl-head">${listHead("Time")}</div></div>${body}${moreButton(timeline)}</div><nav class="rail">${rail}</nav></div>` : `<div class="hint">Items appear here by date.</div>`}`;
  if (quiet) root.scrollTop = top;

  root.querySelectorAll<HTMLButtonElement>(".chip").forEach((b) => b.onclick = () => { timeline.kind = b.dataset.kind!; renderTimeline(root); });
  root.querySelector<HTMLSelectElement>(".sort-sel")!.onchange = (e) => { timeline.sort = (e.target as HTMLSelectElement).value as Sort; renderTimeline(root); };
  root.querySelector<HTMLButtonElement>(".page-more")?.addEventListener("click", () => pageMore(root, timeline, timelineKinds(), renderTimeline));
  const railBtns = Array.from(root.querySelectorAll<HTMLButtonElement>(".rail button"));
  railBtns.forEach((b) => b.onclick = () => root.querySelector(`#m-${b.dataset.month}`)?.scrollIntoView({ behavior: "smooth", block: "start" }));
  // highlight the month in view
  const sections = Array.from(root.querySelectorAll<HTMLElement>(".month"));
  const mark = () => {
    const edge = root.getBoundingClientRect().top + 110; // below the sticky column header
    let cur = sections[0];
    for (const s of sections) if (s.getBoundingClientRect().top <= edge) cur = s;
    railBtns.forEach((b) => b.classList.toggle("on", !!cur && `m-${b.dataset.month}` === cur.id));
  };
  root.onscroll = mark;
  mark();
}

// ---- keep both views in step with stars and hides made anywhere ----

onItemChange(async (c) => {
  for (const p of [gallery, timeline]) {
    const it = p.items.find((x) => x.id === c.id);
    if (it && c.starred !== undefined) it.starred = c.starred;
    if (c.hidden === true) p.items = p.items.filter((x) => x.id !== c.id);
  }
  if (c.hidden === false) {
    // Undo: reload what was already showing, so the item is back in place if the engine has it again
    for (const [p, sel, render, kinds] of [[gallery, "#gallery", renderGallery, galleryKinds], [timeline, "#timeline", renderTimeline, timelineKinds]] as const) {
      const root = document.querySelector<HTMLElement>(sel);
      if (!root?.firstElementChild) continue;
      const want = p.items.length;
      reset(p);
      do await loadPage(p, kinds()); while (!p.done && p.items.length < want);
      render(root, true);
    }
    return;
  }
  if (c.hidden !== true) return;
  for (const [sel, render] of [["#gallery", renderGallery], ["#timeline", renderTimeline]] as const) {
    const root = document.querySelector<HTMLElement>(sel);
    const el = root?.querySelector<HTMLElement>(`[data-id="${c.id}"]`);
    if (!root || !el) continue;
    await fadeOut(el);
    if (!root.closest(".hidden")) render(root, true); // counts and empty days follow
  }
});

document.addEventListener("click", (e) => {
  const t = e.target as HTMLElement;
  const el = t.closest<HTMLElement>(".tile[data-id], #timeline .row[data-id]");
  if (!el) return;
  const it = shown.get(Number(el.dataset.id));
  if (it) openViewer(it);
});
