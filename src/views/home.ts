// Home: what the results area shows with an empty query. One list with a switch
// between starred and recently added, so starring something visibly files it, and
// the same Line / Regular / Large sizes as search, remembered per side.
// Clicking a row or card opens the quick view; star, hide and open come from ../actions.

import { api, type Chunk, type Item, type RecentItem } from "../api";
import { esc, badge, fmtDate, rowMeta, listHead, metaLine, hitBody, skeleton, STAR_SVG } from "../ui";
import { itemAttrs, actionButtons, onItemChange, fadeOut } from "../actions";
import { openViewer } from "./viewer";
import { renderWelcome } from "./welcome";

const shown = new Map<number, Item>();
type Tab = "starred" | "recent";
type Size = "line" | "regular" | "large";
type Sort = "added" | "name" | "kind" | "size";
const SORTS: [Sort, string][] = [["added", "Newest"], ["name", "Name"], ["kind", "Type"], ["size", "Size"]];
/** Recently added grows by STEP rows per "Show more", up to CAP; past that, the timeline has everything. */
const STEP = 20;
const CAP = 100;
/** Passages a Large card shows; Regular shows the first one, closed. */
const LARGE_PASSAGES = 3;

const CLOCK_SVG = `<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>`;
const SIZES: [Size, string, string][] = [
  ["line", "Line", `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M4 6h16M4 12h16M4 18h16"/></svg>`],
  ["regular", "Regular", `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="4" y="4.5" width="16" height="6" rx="1.5"/><rect x="4" y="13.5" width="16" height="6" rx="1.5"/></svg>`],
  ["large", "Large", `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="4" y="4" width="16" height="16" rx="2"/><path d="M8 9.5h8M8 14h5"/></svg>`],
];
const DEFAULT_SIZE: Record<Tab, Size> = { starred: "regular", recent: "line" };

function stored(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return null; }
}
function store(key: string, value: string) {
  try { localStorage.setItem(key, value); } catch { /* the choice just isn't remembered */ }
}
function storedSize(tab: Tab): Size {
  const s = stored(`recall.homeSize.${tab}`);
  return s === "line" || s === "regular" || s === "large" ? s : DEFAULT_SIZE[tab];
}

const saved = stored("recall.homeTab");
const state = {
  tab: (saved === "starred" || saved === "recent" ? saved : null) as Tab | null, // null: decided on first draw
  size: { starred: storedSize("starred"), recent: storedSize("recent") } as Record<Tab, Size>,
  sort: "added" as Sort,
  limit: STEP,
};
let lastRoot: HTMLElement | null = null;
let lastSources: string[] = [];
/** First passages per item, so redraws and size changes do not ask again. */
const passages = new Map<number, Chunk[]>();

/** Newest means newest added for recent items, newest modified for starred ones. */
function sortItems<T extends Item>(list: T[], sort: Sort, when: (it: T) => string): T[] {
  const by = {
    added: (a: T, b: T) => Date.parse(when(b)) - Date.parse(when(a)),
    name: (a: T, b: T) => a.title.localeCompare(b.title, undefined, { sensitivity: "base", numeric: true }),
    kind: (a: T, b: T) => a.kind.localeCompare(b.kind) || Date.parse(when(b)) - Date.parse(when(a)),
    size: (a: T, b: T) => b.size_num - a.size_num,
  }[sort];
  return [...list].sort(by);
}

/** Fetched once at the Large count; Regular uses the first of them. */
async function loadPassages(list: Item[]) {
  await Promise.all(list.filter((it) => !passages.has(it.id)).map(async (it) => {
    try { passages.set(it.id, await api.itemChunks(it.id, LARGE_PASSAGES)); } catch { passages.set(it.id, []); }
  }));
}

function rowsHtml<T extends Item>(list: T[], when: (it: T) => string, dateLabel: string): string {
  return `<div class="list cols">${listHead(dateLabel)}${list.map((it) => `<div class="row" ${itemAttrs(it)}>
      ${badge(it.kind, it.source, it.title, 24)}
      <span class="rt">${esc(it.title)}</span>
      ${rowMeta(it, when(it))}
      ${actionButtons(it.starred)}
    </div>`).join("")}</div>`;
}

function cardsHtml(list: Item[], size: Size, quiet: boolean): string {
  const large = size === "large";
  return `<div class="home-cards">${list.map((it, i) => {
    const ps = (passages.get(it.id) ?? []).slice(0, large ? LARGE_PASSAGES : 1);
    const hits = ps.map((p, j) => `<div class="hit ${large && j === 0 ? "open" : ""}" data-chunk="${p.id}" data-anchor="${esc(p.anchor ?? "")}" data-text="${esc(p.text)}">
        <span class="loc">${esc(p.locator)}</span>${hitBody(it, p.text, large && j === 0, p.anchor)}</div>`).join("");
    return `<article class="card home-card ${large ? "" : "regular"}" style="--i:${quiet ? 0 : i}" ${itemAttrs(it)} data-source="${esc(it.source)}">
      <div class="card-head">
        ${badge(it.kind, it.source, it.title)}
        <div class="card-title"><a href="#" class="title">${esc(it.title)}</a>${metaLine(it)}</div>
        ${actionButtons(it.starred)}
      </div>
      ${hits ? `<div class="hits">${hits}</div>` : ""}
    </article>`;
  }).join("")}</div>`;
}

/** `quiet` redraws in place: no skeleton, no fade, scroll position kept. */
export async function renderHome(root: HTMLElement, sources: string[] = [], quiet = false) {
  lastRoot = root;
  lastSources = sources;
  if (!quiet && !root.querySelector(".home-sec")) root.innerHTML = skeleton("Loading your library");
  const inScope = (it: Item) => !sources.length || sources.includes(it.source);
  const [starredAll, recentAll] = await Promise.all([api.starred(), api.recentlyIndexed(state.limit + 1)]);
  if (!starredAll.length && !recentAll.length && !sources.length) {
    // nothing indexed yet: the home screen is the Getting started guide
    const s = await api.status();
    if (!s.items) { shown.clear(); await renderWelcome(root, false, s); return; }
  }
  const starred = starredAll.filter(inScope);
  const recentRaw = recentAll.filter(inScope);
  // Starred comes first; someone who has starred nothing yet starts on Recently added
  const tab: Tab = state.tab ?? (starred.length ? "starred" : "recent");
  const size = state.size[tab];
  shown.clear();
  for (const it of [...starred, ...recentRaw]) shown.set(it.id, it);

  let list: Item[], body: string, meta = "", foot = `<a class="link" href="#timeline">All items in Timeline</a>`;
  if (tab === "starred") {
    list = sortItems(starred, state.sort, (it) => it.modified_at);
    if (size !== "line") await loadPassages(list);
    body = !list.length
      ? `<div class="hint">Nothing starred yet. Star anything with the ☆ on a result or row and it collects here.</div>`
      : size === "line" ? rowsHtml(list, (it) => it.modified_at, "Modified") : cardsHtml(list, size, quiet);
  } else {
    const recent = sortItems(recentRaw.slice(0, state.limit), state.sort, (it: RecentItem) => it.indexed_at);
    list = recent;
    if (size !== "line") await loadPassages(list);
    body = !recent.length
      ? `<div class="hint">New items appear here as they are added.</div>`
      : size === "line" ? rowsHtml(recent, (it) => it.indexed_at, "Added") : cardsHtml(recent, size, quiet);
    meta = recentRaw.length ? `Last ${fmtDate(recentRaw[0].indexed_at)}` : "";
    if (recentAll.length > state.limit && state.limit < CAP) foot = `<button class="link" id="recent-more">Show ${STEP} more</button>${foot}`;
  }

  const tabBtn = (t: Tab, icon: string, label: string) =>
    `<button data-tab="${t}" class="tab-${t} ${tab === t ? "on" : ""}" aria-pressed="${tab === t}">${icon}<span>${label}</span></button>`;
  const sizeBtns = SIZES.map(([s, label, svg]) =>
    `<button data-size="${s}" class="${s === size ? "on" : ""}" title="${label}" aria-label="${label}" aria-pressed="${s === size}">${svg}</button>`).join("");
  if (!quiet) { root.classList.remove("swap"); void root.offsetWidth; }
  root.innerHTML = `
    <section class="home-sec">
      <div class="section-row">
        <span class="seg home-tabs" role="group" aria-label="Show">${tabBtn("starred", STAR_SVG, `Starred${starred.length ? ` <b>${starred.length}</b>` : ""}`)}${tabBtn("recent", CLOCK_SVG, "Recently added")}</span>
        <span class="n">${meta}</span>
        <span class="spacer"></span>
        <label class="sort">Sort <select id="home-sort">${SORTS.map(([v, l]) => `<option value="${v}" ${v === state.sort ? "selected" : ""}>${l}</option>`).join("")}</select></label>
        <span class="seg density" role="group" aria-label="Size">${sizeBtns}</span>
      </div>
      ${body}
      <div class="list-foot">${foot}</div>
    </section>`;
  if (!quiet) root.classList.add("swap");

  root.querySelectorAll<HTMLButtonElement>(".home-tabs button").forEach((b) => b.onclick = () => {
    if (b.dataset.tab === tab) return;
    state.tab = b.dataset.tab as Tab;
    store("recall.homeTab", state.tab);
    renderHome(root, sources, true);
  });
  root.querySelectorAll<HTMLButtonElement>(".density button").forEach((b) => b.onclick = () => {
    if (b.dataset.size === size) return;
    state.size[tab] = b.dataset.size as Size;
    store(`recall.homeSize.${tab}`, state.size[tab]);
    renderHome(root, sources, true);
  });
  root.querySelector<HTMLSelectElement>("#home-sort")!.onchange = (e) => { state.sort = (e.target as HTMLSelectElement).value as Sort; renderHome(root, sources, true); };
  root.querySelector<HTMLButtonElement>("#recent-more")?.addEventListener("click", async (e) => {
    const b = e.currentTarget as HTMLButtonElement;
    b.disabled = true; b.textContent = "Loading";
    state.limit = Math.min(state.limit + STEP, CAP);
    await renderHome(root, sources, true);
  });
}

function homeOnScreen(): boolean {
  return !!lastRoot?.querySelector(".home-sec") && !lastRoot.closest(".hidden");
}

// hiding takes the row or card away; unstarring takes it off the Starred side;
// the redraw updates the Starred count either way
onItemChange(async (c) => {
  if (!homeOnScreen() || !lastRoot) return;
  const onStarred = !!lastRoot.querySelector(".home-tabs .tab-starred.on");
  if (c.hidden === true || (c.starred === false && onStarred)) {
    const el = lastRoot.querySelector<HTMLElement>(`.row[data-id="${c.id}"], .home-card[data-id="${c.id}"]`);
    if (el) await fadeOut(el);
  }
  renderHome(lastRoot, lastSources, true);
});

// a row, or a card outside its title, passages and buttons, opens the quick view;
// the title opens the file and passages expand, as on search cards
document.addEventListener("click", (e) => {
  const t = e.target as HTMLElement;
  const el = t.closest<HTMLElement>("#results .row[data-id], #results .home-card[data-id]");
  if (!el || (el.classList.contains("home-card") && t.closest(".title, .hit, .acts, button, a"))) return;
  const it = shown.get(Number(el.dataset.id));
  if (it) openViewer(it);
});
