// Star, hide and open for one item, the same everywhere an item is shown: search cards, home,
// timeline rows, gallery tiles and the quick view. An element that shows an item carries
// itemAttrs(); its buttons come from actionButtons(). One delegated handler serves them all, and
// every change is announced as a `recall:item` event so each view showing that item updates in place.

import { api } from "./api";
import { esc, toast, STAR_SVG, OPEN_SVG, EXCLUDE_SVG } from "./ui";
import { openNative } from "./views/viewer";

export interface ItemRef { id: number; kind: string; uri: string; title: string }
export interface ItemChange { id: number; starred?: boolean; hidden?: boolean }

export function itemAttrs(it: ItemRef): string {
  return `data-id="${it.id}" data-kind="${esc(it.kind)}" data-uri="${esc(it.uri)}" data-title="${esc(it.title)}"`;
}

export function actionButtons(starred: boolean, open = true): string {
  const label = starred ? "Unstar" : "Star";
  return `<span class="acts">
    <button class="act star ${starred ? "on" : ""}" aria-label="${label}" title="${label}">${STAR_SVG}</button>
    <button class="act exclude" aria-label="Hide" title="Hide">${EXCLUDE_SVG}</button>
    ${open ? `<button class="act open" aria-label="Open" title="Open">${OPEN_SVG}</button>` : ""}
  </span>`;
}

export function announce(change: ItemChange) {
  document.dispatchEvent(new CustomEvent<ItemChange>("recall:item", { detail: change }));
}

export function onItemChange(fn: (c: ItemChange) => void) {
  document.addEventListener("recall:item", (e) => fn((e as CustomEvent<ItemChange>).detail));
}

function refOf(host: HTMLElement): ItemRef {
  return { id: Number(host.dataset.id), kind: host.dataset.kind ?? "", uri: host.dataset.uri ?? "", title: host.dataset.title ?? "" };
}

// ---- one handler for every action button; capture phase so a row's own click (quick view) never fires too ----
document.addEventListener("click", async (e) => {
  const act = (e.target as HTMLElement).closest<HTMLButtonElement>(".act");
  const host = act?.closest<HTMLElement>("[data-id][data-uri]");
  if (!act || !host) return;
  e.preventDefault();
  e.stopPropagation();
  const it = refOf(host);
  if (act.classList.contains("star")) {
    const on = !act.classList.contains("on");
    paintStars(it.id, on);
    try { await api.setStar(it.id, on); announce({ id: it.id, starred: on }); }
    catch (err) { paintStars(it.id, !on); toast(`Could not ${on ? "star" : "unstar"}. ${err}`); }
  } else if (act.classList.contains("exclude")) {
    openHideMenu(act, it);
  } else if (act.classList.contains("open")) {
    await openNative(it);
  }
}, true);

/** Every star button for this item, wherever it is on screen. */
export function paintStars(id: number, on: boolean) {
  document.querySelectorAll<HTMLElement>(`[data-id="${id}"][data-uri] .act.star`).forEach((b) => {
    b.classList.toggle("on", on);
    b.setAttribute("aria-label", on ? "Unstar" : "Star");
    b.title = on ? "Unstar" : "Star";
  });
}

/** Fade an element out and remove it; resolves when it is gone. */
export function fadeOut(el: HTMLElement): Promise<void> {
  el.style.transition = "opacity var(--t-base) var(--ease), transform var(--t-base) var(--ease)";
  el.style.opacity = "0";
  el.style.transform = "translateY(-4px)";
  return new Promise((r) => setTimeout(() => { el.remove(); r(); }, 200));
}

// ---- hide menu ----

let openMenu: { el: HTMLElement; close: () => void } | null = null;

export function closeHideMenu() { openMenu?.close(); }

export function openHideMenu(anchor: HTMLElement, it: ItemRef) {
  closeHideMenu();
  const noun = it.kind === "chat" ? "conversation" : "file";
  const menu = document.createElement("div");
  menu.className = "menu hide-menu";
  menu.setAttribute("role", "menu");
  menu.innerHTML = `
    <button class="menu-row primary-row" role="menuitem"><span>Hide this ${noun}</span><small>Leaves Recall. The ${noun} itself is not changed.</small></button>
    <button class="menu-row link-row" role="menuitem"><span>Choose what to skip</span>${OPEN_SVG}</button>`;
  // inside the quick view the menu must live in the dialog: a modal dialog makes the rest of the page inert
  (anchor.closest("dialog") ?? document.body).appendChild(menu);
  place(menu, anchor);
  requestAnimationFrame(() => menu.classList.add("on"));

  const close = () => {
    if (openMenu?.el !== menu) return;
    openMenu = null;
    menu.classList.remove("on");
    setTimeout(() => menu.remove(), 200);
    document.removeEventListener("mousedown", onDoc, true);
    document.removeEventListener("keydown", onKey, true);
    document.removeEventListener("scroll", close, true);
    window.removeEventListener("resize", close);
  };
  const onDoc = (e: MouseEvent) => { if (!menu.contains(e.target as Node)) close(); };
  const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); e.preventDefault(); close(); anchor.focus(); } };
  openMenu = { el: menu, close };
  setTimeout(() => {
    document.addEventListener("mousedown", onDoc, true);
    document.addEventListener("keydown", onKey, true);
    document.addEventListener("scroll", close, true); // a fixed menu would drift away from its button
    window.addEventListener("resize", close);
  }, 0);

  (menu.querySelector(".primary-row") as HTMLButtonElement).onclick = async () => {
    close();
    let item;
    try { item = await api.excludeItem(it.id); } catch (err) { toast(`Could not hide. ${err}`); return; }
    announce({ id: it.id, hidden: true });
    if (item) {
      // the engine deletes a hidden item; Undo re-reads a file right away, a conversation returns with the next backup
      toast(`Hidden: ${item.title}`, { label: "Undo", run: async () => {
        await api.unexclude(item.ident);
        announce({ id: it.id, hidden: false });
        toast(item.kind === "chat" ? "Undone. The conversation returns with the next backup." : "Undone. The file returns once it is read again.");
      } });
    }
  };
  (menu.querySelector(".link-row") as HTMLButtonElement).onclick = () => {
    close();
    anchor.closest("dialog")?.close();
    const folder = it.uri.startsWith("file:///") ? decodeURIComponent(it.uri.slice(8)).replace(/\/[^/]*$/, "") : "";
    location.hash = folder ? `#settings/rules?folder=${encodeURIComponent(folder)}` : "#settings/rules";
  };
}

/** Below the button when it fits, above it otherwise; right edges aligned; always inside the window. */
function place(menu: HTMLElement, anchor: HTMLElement) {
  const r = anchor.getBoundingClientRect();
  const w = menu.offsetWidth, h = menu.offsetHeight, m = 8;
  const below = r.bottom + 6 + h <= window.innerHeight - m;
  menu.style.top = `${below ? r.bottom + 6 : Math.max(m, r.top - 6 - h)}px`;
  menu.style.left = `${Math.max(m, Math.min(r.right - w, window.innerWidth - w - m))}px`;
  menu.style.transformOrigin = below ? "top right" : "bottom right";
}
