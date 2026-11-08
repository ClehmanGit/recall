// Settings: sources, what to skip, chat apps, model, storage, about. Switches and pickers, no typing.

import { api, type Rules, type Status } from "../api";
import { $, esc, fmtBytes, fmtDate, shortPath, toast, FOLDER_SVG, PLATFORM_LABEL, PLATFORM_MARK, assetUrl } from "../ui";
import { pickFolders } from "./search";

const root = $<HTMLDivElement>("#settings");
let rules: Rules;

const KINDS: [string, string][] = [["pdf", "PDFs"], ["document", "Word documents"], ["spreadsheet", "Spreadsheets"], ["text", "Text and notes"], ["code", "Code"], ["image", "Images"]];
const SIZES = [16, 32, 64, 128, 256, 0];

/** The app's own line for a chat app, plus when it last backed up. */
export function connectionLine(s: Status, p: string): string {
  const c = s.connections[p];
  const line = s.connection_lines?.[p] ?? "Not connected";
  return c?.state === "connected" && c.last_sync ? `${line} · Last backup ${fmtDate(c.last_sync)}` : line;
}

function sw(id: string, on: boolean, label: string): string {
  return `<div class="row"><span class="rt">${label}</span><button class="switch ${on ? "on" : ""}" id="${id}" role="switch" aria-checked="${on}" aria-label="${esc(label)}"><i></i></button></div>`;
}

export async function renderSettings(params: URLSearchParams = new URLSearchParams()) {
  const [s, r, excluded, data] = await Promise.all([api.status(), api.getRules(), api.listExcluded(), api.dataInfo()]);
  rules = r;
  if (params.get("folder")) { const f = params.get("folder")!; if (!r.skipped_folders.includes(f)) r.skipped_folders = [...r.skipped_folders, f]; }

  root.innerHTML = `
    <div class="settings-head"><h1>Settings</h1><span class="hint">Changes are saved as you make them.</span></div>
    <div class="settings-grid">
      <div class="col">
        <section id="s-sources" class="home-sec">
          <div class="section-row"><span class="section">FOLDERS</span></div>
          <div class="list">${s.sources.map((src) => `<div class="row src-row ${src.enabled ? "" : "off"}" data-path="${esc(src.path)}">
              ${FOLDER_SVG}
              <span class="rt"><span>${esc(shortPath(src.path))}<span class="cnt">${src.count.toLocaleString()} items${src.enabled ? "" : " · Paused"}</span></span><small title="${esc(src.path)}">${esc(src.path)}</small></span>
              <span class="row-actions">
                <button class="mini-btn toggle">${src.enabled ? "Pause" : "Resume"}</button>
                <button class="mini-btn resync">Refresh</button>
                <button class="mini-btn remove">Remove</button>
              </span></div>`).join("") || `<div class="hint pad">No folders yet.</div>`}</div>
          <div class="btn-row"><button class="primary" id="s-add">Add folder</button><button class="ghost" id="s-resync">Refresh all</button></div>
        </section>

        <section id="s-platforms" class="home-sec">
          <div class="section-row"><span class="section">CHAT APPS</span></div>
          <div class="list">${["claude", "gemini", "chatgpt"].map((p) => {
            const n = s.platforms.find(([k]) => k === p)?.[1] ?? 0;
            const on = s.platform_enabled[p] !== false;
            const c = s.connections[p] ?? { state: "never", last_sync: null, last_error: null, synced: 0 };
            const action = c.state === "connected" || c.state === "syncing"
              ? `<button class="mini-btn sync">Back up now</button><button class="mini-btn disconnect">Disconnect</button>`
              : c.state === "signed_out" ? `<button class="mini-btn connect">Sign in again</button><button class="mini-btn disconnect">Disconnect</button>`
              : `<button class="mini-btn connect">Connect</button>`;
            return `<div class="row plat-row" data-platform="${p}">
              <img src="${assetUrl(PLATFORM_MARK[p])}" alt="" width="18" height="18">
              <span class="rt"><span>${PLATFORM_LABEL[p]}<span class="cnt">${n.toLocaleString()} conversations</span></span><small class="plat-line">${esc(connectionLine(s, p))}</small></span>
              <span class="row-actions">${action}</span>
              <button class="switch ${on ? "on" : ""}" role="switch" aria-checked="${on}" aria-label="Include ${PLATFORM_LABEL[p]}" title="Include in search"><i></i></button></div>`;
          }).join("")}</div>
          <div class="hint">Connect opens the app's own sign-in page in a window inside Recall. You sign in there, the same way you always do; Recall never sees your password. The window then closes and Recall copies your recent and starred conversations to this computer first, then checks for new ones every fifteen minutes. Older conversations are copied a few at a time while the computer is idle or at night, so they do not slow you down. Nothing is sent anywhere else. Closing the window keeps Recall running in the tray, next to the clock, so this carries on; quit from the tray icon.</div>
        </section>

        <section id="s-excluded" class="home-sec">
          <div class="section-row"><span class="section">HIDDEN ITEMS</span><span class="n">${excluded.length || ""}</span></div>
          <div class="list">${excluded.map((x) => `<div class="row" data-ident="${esc(x.ident)}">
              <span class="rt"><span>${esc(x.label.split(/[\\/]/).pop() || x.label)}</span><small>${esc(x.label)}</small></span>
              <span class="rm">${fmtDate(x.at)}</span>
              <span class="row-actions"><button class="mini-btn restore">Show again</button></span></div>`).join("") || `<div class="hint pad">Nothing hidden. Use Hide on any result.</div>`}</div>
        </section>
      </div>

      <div class="col">
        <section id="s-rules" class="home-sec">
          <div class="section-row"><span class="section">WHAT TO SKIP</span></div>
          <div class="list">
            ${sw("r-projects", r.skip_code_projects, "Code projects (folders with .git or a build file)")}
            ${sw("r-dev", r.skip_dev_folders, "Program and build folders")}
            ${sw("r-hidden", r.skip_hidden, "Hidden files and folders")}
            ${sw("r-packages", r.skip_packages, "Installers and archives")}
            ${sw("r-media", r.skip_media, "Video and audio")}
            ${sw("r-textless", r.skip_textless_images, "Pictures with no readable text")}
            <div class="row"><span class="rt">Files larger than</span>
              <select id="r-max" class="select">${SIZES.map((mb) => `<option value="${mb}" ${mb === r.max_mb ? "selected" : ""}>${mb ? `${mb} MB` : "No limit"}</option>`).join("")}</select></div>
          </div>

          <div class="section-row"><span class="section">SKIP THESE FOLDERS</span></div>
          <div class="list">${r.skipped_folders.map((f) => `<div class="row" data-skip="${esc(f)}">${FOLDER_SVG}<span class="rt"><span>${esc(shortPath(f))}</span><small>${esc(f)}</small></span><span class="row-actions"><button class="mini-btn unskip">Remove</button></span></div>`).join("")}
            <button class="row add-row" id="r-skip-add">+ Choose a folder</button></div>

          <div class="section-row"><span class="section">WHAT TO INCLUDE</span></div>
          <div class="list">
            <div class="row kinds"><span class="rt">File types</span>
              <span class="chips">${KINDS.map(([k, l]) => `<button class="chip small ${!r.only_kinds.length || r.only_kinds.includes(k) ? "active" : ""}" data-kind="${k}">${l}</button>`).join("")}</span></div>
            ${sw("r-ocr", r.ocr, "Read text inside images")}
            ${sw("r-links", r.follow_links, "Follow shortcuts to other folders")}
          </div>
          <div class="hint" id="r-preview">—</div>
        </section>

        <section id="s-model" class="home-sec">
          <div class="section-row"><span class="section">SEARCH ENGINE</span></div>
          <div class="panel kv">
            <div><span>Model</span><span>${esc(s.model || "Loading")}</span></div>
            <div><span>Runs</span><span>On this computer</span></div>
            <div><span>Indexed</span><span>${s.items.toLocaleString()} items · ${s.chunks.toLocaleString()} passages</span></div>
          </div>
        </section>

        <section id="s-data" class="home-sec">
          <div class="section-row"><span class="section">STORAGE</span></div>
          <div class="panel kv">
            <div><span>Location</span><span><code>${esc(data.dir)}</code></span></div>
            <div><span>Size</span><span>${fmtBytes(data.db_bytes + data.index_bytes)}</span></div>
            <div class="btn-row"><button class="ghost danger" id="s-reset">Rebuild index</button></div>
          </div>
        </section>

        <section id="s-about" class="home-sec">
          <div class="section-row"><span class="section">PRIVACY</span></div>
          <div class="panel hint-panel">Everything stays on this computer. Recall makes no connections to the internet. ${s.feedback_enabled ? "Feedback is sent only when you press Send, and contains only what you wrote." : ""}<br><br>Recall ${esc(s.version)}</div>
        </section>
      </div>
    </div>`;

  wire(s);
  preview();
  if (params.get("folder")) { $("#s-rules").scrollIntoView({ block: "start" }); save(); }
  if (location.hash.startsWith("#settings/chats")) $("#s-platforms").scrollIntoView({ block: "start" });
}

async function save() {
  await api.setRules(rules);
  preview();
}

async function preview() {
  const el = $("#r-preview");
  const p = await api.previewRules(rules);
  el.innerHTML = `<b>${p.matched.toLocaleString()}</b> files will be indexed · <b>${p.excluded.toLocaleString()}</b> skipped`;
}

function bindSwitch(id: string, key: keyof Rules) {
  const b = $<HTMLButtonElement>(`#${id}`);
  b.onclick = () => {
    const on = !b.classList.contains("on");
    b.classList.toggle("on", on); b.setAttribute("aria-checked", String(on));
    (rules as any)[key] = on;
    save();
  };
}

function wire(s: Status) {
  $("#s-add").onclick = async () => { const n = await pickFolders(); if (n.length) { toast(n.length === 1 ? "Indexing started" : `Indexing ${n.length} folders`); renderSettings(); } };
  $("#s-resync").onclick = async () => { await api.resyncAll(); toast("Refreshing all folders"); };
  $("#s-reset").onclick = async () => { if (confirm("Rebuild the index? Folders, settings and hidden items are kept. This takes a while.")) { await api.resetIndex(); toast("Rebuilding"); } };

  root.querySelectorAll<HTMLElement>(".src-row").forEach((row) => {
    const path = row.dataset.path!;
    $(".toggle", row).onclick = async () => { await api.setSourceEnabled(path, row.classList.contains("off")); renderSettings(); };
    $(".resync", row).onclick = async () => { await api.resyncSource(path); toast("Refreshing"); };
    $(".remove", row).onclick = async () => { if (confirm(`Remove ${shortPath(path)} from Recall? Your files are not affected.`)) { await api.removeSource(path); renderSettings(); } };
  });
  root.querySelectorAll<HTMLElement>(".plat-row").forEach((row) => {
    const p = row.dataset.platform!;
    const b = $<HTMLButtonElement>(".switch", row);
    b.onclick = async () => { const on = !b.classList.contains("on"); b.classList.toggle("on", on); b.setAttribute("aria-checked", String(on)); await api.setPlatformEnabled(p, on); };
    row.querySelector<HTMLButtonElement>(".connect")?.addEventListener("click", async () => { await api.connectPlatform(p); toast(`Sign in to ${PLATFORM_LABEL[p]} in the window that opened`); renderSettings(); });
    row.querySelector<HTMLButtonElement>(".sync")?.addEventListener("click", async () => { await api.syncPlatform(p); toast("Backing up recent and starred chats"); renderSettings(); });
    row.querySelector<HTMLButtonElement>(".disconnect")?.addEventListener("click", async () => { if (confirm(`Disconnect ${PLATFORM_LABEL[p]}? Recall signs out of it. Conversations already indexed stay until you hide them or rebuild the index.`)) { await api.disconnectPlatform(p); renderSettings(); } });
  });
  root.querySelectorAll<HTMLElement>("#s-excluded .row[data-ident]").forEach((row) => {
    $(".restore", row).onclick = async () => { await api.unexclude(row.dataset.ident!); renderSettings(); toast("Shown again on next refresh"); };
  });

  bindSwitch("r-projects", "skip_code_projects");
  bindSwitch("r-textless", "skip_textless_images");
  bindSwitch("r-dev", "skip_dev_folders");
  bindSwitch("r-hidden", "skip_hidden");
  bindSwitch("r-packages", "skip_packages");
  bindSwitch("r-media", "skip_media");
  bindSwitch("r-ocr", "ocr");
  bindSwitch("r-links", "follow_links");
  $<HTMLSelectElement>("#r-max").onchange = (e) => { rules.max_mb = Number((e.target as HTMLSelectElement).value); save(); };

  $("#r-skip-add").onclick = async () => {
    const list = await api.pickFolders("skip");
    for (const f of list) if (!rules.skipped_folders.includes(f)) rules.skipped_folders.push(f);
    if (list.length) { await save(); renderSettings(); }
  };
  root.querySelectorAll<HTMLElement>(".row[data-skip]").forEach((row) => {
    $(".unskip", row).onclick = async () => { rules.skipped_folders = rules.skipped_folders.filter((f) => f !== row.dataset.skip); await save(); renderSettings(); };
  });

  root.querySelectorAll<HTMLButtonElement>(".chips .chip").forEach((c) => {
    c.onclick = () => {
      const all = Array.from(root.querySelectorAll<HTMLButtonElement>(".chips .chip"));
      c.classList.toggle("active");
      const on = all.filter((x) => x.classList.contains("active")).map((x) => x.dataset.kind!);
      if (!on.length) { c.classList.add("active"); return; } // at least one type stays on
      rules.only_kinds = on.length === all.length ? [] : on;
      save();
    };
  });
}
