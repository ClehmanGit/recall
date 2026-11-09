// Search view: field with recents dropdown, filters, result cards. Star, hide and open on a card
// are handled by ../actions like everywhere else; this file grows cards and keeps them in sync.

import { api, type Card, type Hit, type Item } from "../api";
import { openNative, openViewer } from "./viewer";
import { $, esc, badge, metaLine, hitBody, toast, shortPath, fmtDate, PLATFORM_LABEL } from "../ui";
import { itemAttrs, actionButtons, onItemChange, fadeOut } from "../actions";
import { renderHome } from "./home";

const q = $<HTMLInputElement>("#q");
const results = $<HTMLDivElement>("#results");
const count = $<HTMLSpanElement>("#count");
const recents = $<HTMLDivElement>("#recents");
const scope = $<HTMLSpanElement>("#scope");
const densityEl = $<HTMLSpanElement>("#density");

export const state = { kinds: [] as string[], sources: [] as string[], lastQuery: "" };
let debounce: number | undefined;
let recentIndex = -1;
const cards = new Map<number, Card>();
/** The results as last drawn, so a size change redraws without searching again. */
let shownCards: Card[] = [];

/** Result size, like a file explorer's view: one row each, head and best passage, or the full card. */
type Density = "line" | "regular" | "large";
let density: Density = (() => {
  try { const d = localStorage.getItem("recall.density"); return d === "line" || d === "regular" ? d : "large"; } catch { return "large"; }
})();

/** Passages a card adds per "Show more", and the most it shows before pointing to the quick view. */
const STEP = 10;
const CAP = 25;

// ---- rendering ----

function renderHit(c: Card, h: Hit, open: boolean, enter = false): string {
  return `<div class="hit ${open ? "open" : ""} ${enter ? "enter" : ""}" data-chunk="${h.chunk_id}" data-anchor="${esc(h.anchor ?? "")}" data-text="${esc(h.text)}">
    <span class="loc">${esc(h.locator)}</span>${hitBody(c, h.text, open, h.anchor)}</div>`;
}

function moreButton(total: number, shown: number): string {
  const left = total - shown;
  if (left <= 0) return "";
  if (shown >= CAP) return `<button class="more all">Quick view · ${left} more</button>`;
  return `<button class="more">Show ${Math.min(left, STEP, CAP - shown)} more</button>`;
}

function firstLine(text: string): string {
  return (text.split("\n").find((l) => l.trim()) ?? "").replace(/^#+\s*/, "");
}

function renderLine(c: Card, i: number): string {
  return `<article class="card line" style="--i:${i}" ${itemAttrs({ id: c.item_id, kind: c.kind, uri: c.uri, title: c.title })} data-source="${esc(c.source)}">
    ${badge(c.kind, c.source, c.title, 24)}
    <span class="lt">${esc(c.title)}</span>
    <span class="snip">${esc(c.hits[0] ? firstLine(c.hits[0].text) : "")}</span>
    <span class="when">${fmtDate(c.modified_at)}</span>
    ${actionButtons(c.starred)}
    <span class="score" title="Match strength">${c.score.toFixed(2)}</span>
  </article>`;
}

export function renderCard(c: Card, i: number): string {
  if (density === "line") return renderLine(c, i);
  const regular = density === "regular";
  const list = regular ? c.hits.slice(0, 1) : c.hits;
  const hits = list.map((h, j) => renderHit(c, h, !regular && j === 0)).join("");
  return `<article class="card ${regular ? "regular" : ""}" style="--i:${i}" ${itemAttrs({ id: c.item_id, kind: c.kind, uri: c.uri, title: c.title })} data-source="${esc(c.source)}" data-total="${c.total_hits}">
    <div class="card-head">
      ${badge(c.kind, c.source, c.title)}
      <div class="card-title">
        <a href="#" class="title">${esc(c.title)}</a>
        ${metaLine(c)}
      </div>
      ${actionButtons(c.starred)}
      <span class="score" title="Match strength">${c.score.toFixed(2)}</span>
    </div>
    <div class="hits">${hits}${moreButton(c.total_hits, list.length)}</div>
  </article>`;
}

function setResults(html: string) {
  results.classList.remove("swap");
  void results.offsetWidth;
  results.innerHTML = html;
  results.classList.add("swap");
}

function sourceName(id: string): string {
  return PLATFORM_LABEL[id] ?? shortPath(id).replace(/^…\//, "");
}

/** The sidebar's source selection narrows search; say so where the filters are, with a way out. */
export function renderScope() {
  if (!state.sources.length) { scope.innerHTML = ""; return; }
  const names = state.sources.map(sourceName);
  const label = names.length > 2 ? `${names.slice(0, 2).join(", ")} +${names.length - 2}` : names.join(", ");
  scope.innerHTML = `<button class="chip scope" title="Show every source">Only ${esc(label)}<span aria-hidden="true">✕</span></button>`;
}
scope.addEventListener("click", () => {
  state.sources = [];
  document.querySelectorAll<HTMLElement>("#sources .src").forEach((r) => r.classList.remove("off"));
  state.lastQuery = "";
  runSearch();
});

export async function runSearch() {
  renderScope();
  const text = q.value.trim();
  densityEl.hidden = !text;
  if (!text) { state.lastQuery = ""; count.textContent = ""; shownCards = []; await renderHome(results, state.sources); return; }
  const r = await api.search(text, state.kinds, state.sources);
  if (q.value.trim() !== text) return;
  count.textContent = `${r.cards.length} results · ${r.total_hits} hits · ${r.took_ms} ms`;
  const changed = text !== state.lastQuery;
  state.lastQuery = text;
  cards.clear();
  for (const c of r.cards) cards.set(c.item_id, c);
  shownCards = r.cards;
  drawCards(changed);
}

function drawCards(animate: boolean) {
  const body = shownCards.map((c, i) => renderCard(c, animate ? i : 0)).join("");
  const html = !shownCards.length ? `<div class="empty">Nothing found</div>` : density === "line" ? `<div class="list lines">${body}</div>` : body;
  if (animate) setResults(html); else results.innerHTML = html;
}

function paintDensity() {
  densityEl.querySelectorAll<HTMLButtonElement>("button").forEach((b) => {
    b.classList.toggle("on", b.dataset.d === density);
    b.setAttribute("aria-pressed", String(b.dataset.d === density));
  });
}
paintDensity();
densityEl.addEventListener("click", (e) => {
  const b = (e.target as HTMLElement).closest<HTMLButtonElement>("button[data-d]");
  if (!b || b.dataset.d === density) return;
  density = b.dataset.d as Density;
  try { localStorage.setItem("recall.density", density); } catch { /* the choice just isn't remembered */ }
  paintDensity();
  drawCards(true);
});

// ---- recents dropdown: only on focus with an empty field ----

async function showRecents() {
  if (q.value.trim()) { hideRecents(); return; }
  const rows = await api.recentQueries();
  if (!rows.length) { hideRecents(); return; }
  recentIndex = -1;
  recents.innerHTML = `<div class="section">RECENT</div>${rows.map(([r, n]) => `<button class="recent-row" data-q="${esc(r)}">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>
      <span class="rq">${esc(r)}</span><span class="rn">${n} results</span>
      <span class="recent-x" role="button" aria-label="Remove ${esc(r)} from recent searches" title="Remove">×</span></button>`).join("")}
    <div class="recent-foot"><span>↑↓ move · ↵ search · del remove · esc close</span><button class="recent-clear">Clear all</button></div>`;
  recents.classList.add("on");
}
function hideRecents() { recents.classList.remove("on"); recentIndex = -1; }

/** Takes one search off the list and keeps the list open, with the same row picked. */
async function removeRecent(row: HTMLButtonElement) {
  const keep = recentIndex;
  await api.removeRecentQuery(row.dataset.q!);
  await showRecents();
  const rows = recents.querySelectorAll<HTMLButtonElement>(".recent-row");
  if (keep >= 0 && rows.length) {
    recentIndex = Math.min(keep, rows.length - 1);
    rows[recentIndex].classList.add("sel");
  }
}

recents.addEventListener("mousedown", (e) => e.preventDefault()); // keep input focus
recents.addEventListener("click", async (e) => {
  const t = e.target as HTMLElement;
  const row = t.closest<HTMLButtonElement>(".recent-row");
  if (row && t.closest(".recent-x")) { await removeRecent(row); return; }
  if (row) { q.value = row.dataset.q!; hideRecents(); runSearch(); return; }
  if (t.closest(".recent-clear")) { await api.clearRecentQueries(); hideRecents(); }
});

q.addEventListener("focus", showRecents);
q.addEventListener("click", () => { if (!recents.classList.contains("on")) showRecents(); }); // already focused, e.g. after Escape
q.addEventListener("blur", () => setTimeout(hideRecents, 120));
q.addEventListener("input", () => {
  hideRecents();
  clearTimeout(debounce);
  debounce = window.setTimeout(runSearch, 120);
});
q.addEventListener("keydown", (e) => {
  const rows = Array.from(recents.querySelectorAll<HTMLButtonElement>(".recent-row"));
  if (recents.classList.contains("on") && rows.length) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      recentIndex = (recentIndex + (e.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length;
      rows.forEach((r, i) => r.classList.toggle("sel", i === recentIndex));
      return;
    }
    if (e.key === "Enter" && recentIndex >= 0) { e.preventDefault(); q.value = rows[recentIndex].dataset.q!; hideRecents(); runSearch(); return; }
    if (e.key === "Delete" && recentIndex >= 0) { e.preventDefault(); removeRecent(rows[recentIndex]); return; }
  }
  if (e.key === "Escape") {
    if (recents.classList.contains("on")) hideRecents();
    else if (q.value) { q.value = ""; runSearch(); }
  }
});

function asItem(c: Card): Item {
  return { id: c.item_id, kind: c.kind, source: c.source, ident: "", title: c.title, location: c.location, uri: c.uri,
    modified_at: c.modified_at, size_label: c.size_label, size_num: 0, missing: false, starred: c.starred };
}

// ---- card interactions (star, hide and open buttons are handled in ../actions) ----

results.addEventListener("click", async (e) => {
  const t = e.target as HTMLElement;
  const card = t.closest<HTMLElement>(".card");
  if (!card) return;
  const id = Number(card.dataset.id);
  const kind = card.dataset.kind!;

  if (card.classList.contains("line")) { const k = cards.get(id); if (k) openViewer(asItem(k)); return; }
  if (t.closest(".title")) { e.preventDefault(); await openNative({ id }); return; }

  const more = t.closest<HTMLButtonElement>(".more");
  if (more) { await growCard(card, more, id); return; }

  const hit = t.closest<HTMLElement>(".hit");
  if (hit) {
    if (hit.classList.contains("open")) { await openNative({ id }, hit.dataset.anchor || undefined); return; }
    const c = { kind, title: card.dataset.title! };
    card.querySelectorAll<HTMLElement>(".hit.open").forEach((h) => {
      h.classList.remove("open");
      h.querySelector(".t")!.outerHTML = hitBody(c, h.dataset.text!, false, h.dataset.anchor || null);
    });
    hit.classList.add("open");
    hit.querySelector(".t")!.outerHTML = hitBody(c, hit.dataset.text!, true, hit.dataset.anchor || null);
  }
});

/** Show the card's next passages for the same query; past CAP, the quick view shows the whole item. */
async function growCard(card: HTMLElement, more: HTMLButtonElement, id: number) {
  const known = cards.get(id);
  if (more.classList.contains("all")) { if (known) openViewer(asItem(known)); return; }
  const shown = card.querySelectorAll(".hit").length;
  const label = more.textContent;
  more.disabled = true;
  more.textContent = "Loading";
  try {
    const r = await api.cardHits(state.lastQuery, state.kinds, state.sources, id, Math.min(shown + STEP, CAP));
    const c = r.cards[0];
    const have = new Set(Array.from(card.querySelectorAll<HTMLElement>(".hit")).map((h) => h.dataset.chunk));
    const fresh = c ? c.hits.filter((h) => !have.has(String(h.chunk_id))) : [];
    if (!c || !fresh.length) { more.remove(); return; } // the index changed underneath; nothing more to add
    more.insertAdjacentHTML("beforebegin", fresh.map((h) => renderHit(c, h, false, true)).join(""));
    card.dataset.total = String(c.total_hits);
    const next = moreButton(c.total_hits, shown + fresh.length);
    if (next) more.outerHTML = next; else more.remove();
  } catch (err) {
    more.disabled = false;
    more.textContent = label;
    toast(`Could not load more. ${err}`);
  }
}

// a card hidden anywhere leaves the results; star buttons are already painted by ../actions
onItemChange((c) => {
  const card = results.querySelector<HTMLElement>(`.card[data-id="${c.id}"]`);
  const known = cards.get(c.id);
  if (known && c.starred !== undefined) known.starred = c.starred;
  if (c.hidden === true) shownCards = shownCards.filter((x) => x.item_id !== c.id);
  if (c.hidden === true && card) fadeOut(card);
  if (c.hidden === false && state.lastQuery) runSearch();
});

// ---- filters ----

document.querySelectorAll<HTMLButtonElement>(".filters .chip[data-kind]").forEach((b) =>
  b.addEventListener("click", () => {
    document.querySelectorAll(".filters .chip[data-kind]").forEach((x) => x.classList.remove("active"));
    b.classList.add("active");
    state.kinds = b.dataset.kind ? b.dataset.kind.split(",") : [];
    state.lastQuery = "";
    runSearch();
  }),
);

/// `onPicked` fires the moment folders are chosen, before they are registered, so the UI can show it is busy.
export async function pickFolders(onPicked?: () => void): Promise<string[]> {
  const list = await api.pickFolders("source");
  if (list.length) onPicked?.();
  for (const p of list) await api.addSource(p);
  return list;
}
