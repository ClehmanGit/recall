// Quick view: one item in a dialog. Image or PDF shown in place where the
// webview can render it, passages from the index underneath, and the two ways
// out: open in the program Windows uses for that file, or show it in its folder.

import { openPath, openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, type Item } from "../api";
import { $, esc, badge, metaLine, hitBody, toast } from "../ui";
import { itemAttrs, actionButtons, onItemChange } from "../actions";

const dlg = $<HTMLDialogElement>("#viewer");
$("#v-body").tabIndex = -1;
let current: Item | null = null;

const MOCK = () => (window as any).__RECALL_MOCK__ === true;

function filePath(uri: string): string {
  return uri.startsWith("file:///") ? decodeURIComponent(uri.slice(8)) : "";
}

function assetUrlFor(path: string): string {
  return `http://asset.localhost/${encodeURI(path.replace(/\\/g, "/"))}`;
}

export async function openNative(it: Pick<Item, "uri" | "kind">, anchor?: string) {
  try {
    if (it.uri.startsWith("http")) { await openUrl(anchor ? `${it.uri}#${anchor}` : it.uri); return; }
    const path = filePath(it.uri);
    if (it.kind === "pdf" && anchor) { await openUrl(`file:///${path}#page=${anchor}`); return; }
    await openPath(path);
  } catch (e) {
    toast(`Could not open. ${e}`);
  }
}

export async function openViewer(it: Item) {
  current = it;
  $("#v-badge").innerHTML = badge(it.kind, it.source, it.title, 32);
  $("#v-name").textContent = it.title;
  $("#v-meta").outerHTML = metaLine(it).replace('class="meta"', 'class="meta" id="v-meta"');
  const path = filePath(it.uri);
  $("#v-path").textContent = path || it.uri;
  $("#v-reveal").classList.toggle("hidden", !path);
  $("#v-open").textContent = it.kind === "chat" ? "Open conversation" : "Open";
  // star and hide, the same buttons as everywhere else; Open has its own button below
  $("#v-acts").outerHTML = `<span id="v-acts" class="v-acts" ${itemAttrs(it)}>${actionButtons(it.starred, false)}</span>`;

  const body = $("#v-body");
  body.innerHTML = `<div class="hint">Loading</div>`;
  if (!dlg.open) dlg.showModal();
  body.focus();

  let preview = "";
  if (it.kind === "image" && path) {
    preview = MOCK()
      ? `<div class="preview"><div class="ph">Image preview shows here in the app.</div></div>`
      : `<div class="preview"><img src="${esc(assetUrlFor(path))}" alt=""></div>`;
  } else if (it.kind === "pdf" && path) {
    preview = MOCK()
      ? `<div class="preview"><div class="ph">The PDF shows here in the app.</div></div>`
      : `<div class="preview"><iframe src="${esc(assetUrlFor(path))}" title="${esc(it.title)}"></iframe></div>`;
  }

  // when the file itself is shown, the passages add nothing
  if (preview) { body.innerHTML = preview; body.scrollTop = 0; return; }

  let chunks: { locator: string; text: string; anchor: string | null }[] = [];
  try { chunks = await api.itemChunks(it.id, 40); } catch (e) { toast(`${e}`); }
  if (current !== it) return;
  const passages = chunks.map((h) => `<div class="hit open" data-anchor="${esc(h.anchor ?? "")}"><span class="loc">${esc(h.locator)}</span>${hitBody(it, h.text, true, h.anchor)}</div>`).join("");
  body.innerHTML = passages || `<div class="hint">No text has been read from this item yet.</div>`;
  body.scrollTop = 0;
}

$("#v-close").addEventListener("click", () => dlg.close());
onItemChange((c) => {
  if (!current || c.id !== current.id) return;
  if (c.starred !== undefined) current.starred = c.starred;
  if (c.hidden === true) dlg.close();
});
dlg.addEventListener("click", (e) => { if (e.target === dlg) dlg.close(); });
dlg.addEventListener("close", () => { current = null; $("#v-body").innerHTML = ""; });
$("#v-open").addEventListener("click", () => { if (current) openNative(current); });
$("#v-reveal").addEventListener("click", async () => {
  if (!current) return;
  try { await revealItemInDir(filePath(current.uri)); } catch (e) { toast(`Could not show the folder. ${e}`); }
});
$("#v-body").addEventListener("click", (e) => {
  const hit = (e.target as HTMLElement).closest<HTMLElement>(".hit");
  if (hit && current && current.kind !== "image") openNative(current, hit.dataset.anchor || undefined);
});
