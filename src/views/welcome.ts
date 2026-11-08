// Getting started: what Recall does and how to set it up, in four steps that
// show whether they are done. It is the home screen while the library is empty
// and stays one click away in the sidebar after that.
// Adding folders and connecting chat apps live in main.ts; this asks for them by event.

import { api, type Status } from "../api";
import { esc, shortPath, assetUrl, PLATFORMS, PLATFORM_MARK, PLATFORM_LABEL } from "../ui";

const CHECK = `<svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>`;

/** What the steps show, so a redraw only happens when one of them changes. */
let drawn = "";

function connected(s: Status, p: string): boolean {
  const st = s.connections[p]?.state;
  return st === "connected" || st === "syncing";
}

function step(n: number, done: boolean, title: string, body: string): string {
  return `<section class="guide-step ${done ? "done" : ""}">
    <span class="guide-n" aria-hidden="true">${done ? CHECK : n}</span>
    <div class="guide-body"><h2>${title}${done ? `<span class="sr"> (done)</span>` : ""}</h2>${body}</div>
  </section>`;
}

function guideHtml(s: Status, standalone: boolean): string {
  const folders = s.sources.filter((x) => x.enabled);
  const chats = PLATFORMS;
  const anyChat = chats.some((p) => connected(s, p));

  const folderBody = `
    <p>Pick the folders you keep things in, such as Documents or Downloads. Recall reads them once, then keeps up as files change. Reading a large folder takes a while, and your computer may feel slower until it finishes.</p>
    <p class="guide-note">Recall leaves out code projects, program folders and pictures with no text, so results stay about your documents. Change that in Settings, What to skip.</p>
    <div class="guide-actions">
      <button class="${folders.length ? "ghost" : "primary"}" data-guide="folders">${folders.length ? "Add another folder" : "Choose folders"}</button>
      ${folders.length ? `<span class="guide-status">${folders.length === 1 ? "Searching" : `Searching ${folders.length} folders:`} ${folders.map((f) => esc(shortPath(f.path))).join(", ")}</span>` : ""}
    </div>`;

  const chatBody = `
    <p>Optional. Sign in to Claude, ChatGPT or Gemini in a window inside Recall, the same way you always do; Recall never sees your password. The window closes by itself, your recent and starred chats are copied first, and older ones follow while the computer is idle or at night.</p>
    <div class="guide-actions">${chats.map((p) => connected(s, p)
      ? `<span class="guide-chip on"><img src="${assetUrl(PLATFORM_MARK[p])}" alt="" width="16" height="16">${PLATFORM_LABEL[p]}${CHECK}</span>`
      : `<button class="ghost guide-chip" data-guide="connect" data-platform="${p}"><img src="${assetUrl(PLATFORM_MARK[p])}" alt="" width="16" height="16">${PLATFORM_LABEL[p]}</button>`).join("")}
    </div>`;

  const searchBody = `
    <p>Type what you remember, in your own words, such as <i>the lease renewal letter</i> or <i>notes on the kitchen quote</i>. Recall finds what matches by meaning, not only by exact words, and shows the passages that match.</p>
    <ul class="guide-list">
      <li><kbd>Ctrl K</kbd> jumps to search from anywhere.</li>
      <li>Star anything to keep it on the home screen.</li>
      <li>Hide anything you never want to see in results again. Settings lists what you hid.</li>
      <li>Line, Regular and Large change how much each result shows.</li>
    </ul>`;

  const knowBody = `
    <ul class="guide-list">
      <li>Nothing leaves this computer. Your files stay where they are and are never changed.</li>
      <li>Closing the window keeps Recall running in the tray, next to the clock, so it stays up to date. Quit from the tray icon.</li>
      <li>This page is always under Getting started in the sidebar.</li>
    </ul>`;

  return `<div class="guide ${standalone ? "standalone" : ""}">
    <header class="guide-head">
      <img src="${assetUrl("owl.png")}" alt="" width="44" height="44">
      <div><h1>Getting started</h1><p>Recall searches your files and chats by what they say, not just their names.</p></div>
    </header>
    ${step(1, folders.length > 0, "Choose folders to search", folderBody)}
    ${step(2, anyChat, "Connect your chat apps", chatBody)}
    ${step(3, false, "Search", searchBody)}
    ${step(4, false, "Good to know", knowBody)}
    ${standalone ? `<div class="guide-foot"><a href="#" class="primary" id="guide-start">Start searching</a></div>` : ""}
  </div>`;
}

/**
 * Draws the guide into `root`. `standalone` is the Getting started page; otherwise it
 * is the empty home screen. Pass a status to skip fetching one (the poll has it).
 */
export async function renderWelcome(root: HTMLElement, standalone: boolean, status?: Status) {
  const s = status ?? await api.status();
  const sig = JSON.stringify([standalone, s.sources.map((x) => [x.path, x.enabled]), Object.entries(s.connections).map(([p, c]) => [p, c.state])]);
  if (root.querySelector(".guide") && sig === drawn) return;
  drawn = sig;
  root.innerHTML = guideHtml(s, standalone);
}

/** True when the guide is on screen, so the status poll keeps its steps current. */
export function welcomeOnScreen(): HTMLElement | null {
  const g = document.querySelector<HTMLElement>(".guide");
  return g && !g.closest(".hidden") ? g.parentElement : null;
}

document.addEventListener("click", (e) => {
  const t = e.target as HTMLElement;
  const b = t.closest<HTMLElement>("[data-guide]");
  if (b?.dataset.guide === "folders") document.dispatchEvent(new CustomEvent("recall:add-folders"));
  if (b?.dataset.guide === "connect") document.dispatchEvent(new CustomEvent("recall:connect", { detail: b.dataset.platform }));
  if (t.closest("#guide-start")) {
    e.preventDefault();
    location.hash = "";
    // after the route shows the search view; a hidden field cannot take focus
    setTimeout(() => document.querySelector<HTMLInputElement>("#q")?.focus(), 50);
  }
});
