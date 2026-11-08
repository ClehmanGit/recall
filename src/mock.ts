// Browser-only mock of the Tauri commands, so `npm run web` shows the UI with
// sample data outside the desktop shell. Never loaded inside Tauri.

type Handler = (args: any) => any;

const now = Date.now();
const day = 86_400_000;
const iso = (ms: number) => new Date(ms).toISOString();

const items = [
  { id: 1, kind: "chat", source: "claude", ident: "claude:1", title: "Abiogenesis research overview", location: "Claude", uri: "https://claude.ai/chat/1", modified_at: iso(now - 110 * day), size_label: "41 turns", size_num: 41, missing: false, starred: true, indexed_at: iso(now - 3 * 3600_000),
    hits: [
      { chunk_id: 1, locator: "turn 1", anchor: "m1", score: .89, text: "## Where abiogenesis research stands\n\nThe question is how chemistry became biology. There are three live camps, and the disagreement is mostly about energy source and compartmentalisation.\n\n- **Alkaline vents**, a sustained proton gradient from serpentinization\n- **Warm pond**, wet-dry cycling concentrates reactants\n- **Mineral surfaces**, clays template the first polymers\n\nWhether membranes came before metabolism or after is still open." },
      { chunk_id: 2, locator: "turn 7", anchor: "m7", score: .81, text: "The protocell question is really about whether vesicle growth can be driven by a gradient alone, or whether it needs a catalytic surface first." },
      { chunk_id: 3, locator: "turn 12", anchor: "m12", score: .74, text: "Lane's argument against the warm pond is that it has no sustained disequilibrium, so there is nothing to drive the chemistry uphill." },
      { chunk_id: 4, locator: "turn 19", anchor: "m19", score: .66, text: "Serpentinization produces hydrogen continuously, which is why alkaline vents are the favoured candidate." },
      { chunk_id: 5, locator: "turn 23", anchor: "m23", score: .61, text: "If you want the simulation to mean anything, the proton gradient has to be an input, not an assumption." },
      { chunk_id: 11, locator: "turn 27", anchor: "m27", score: .58, text: "Wet-dry cycling concentrates nucleotides, which is the warm pond camp's strongest abiogenesis result." },
      { chunk_id: 12, locator: "turn 31", anchor: "m31", score: .55, text: "Mineral surfaces may have templated the first polymers before any membrane existed." },
      { chunk_id: 13, locator: "turn 36", anchor: "m36", score: .52, text: "A protocell that divides without a gene is still a long way from abiogenesis in the full sense." },
      { chunk_id: 14, locator: "turn 40", anchor: "m40", score: .49, text: "Summary: the vent hypothesis explains energy; the pond hypothesis explains concentration." },
    ], total_hits: 9 },
  { id: 2, kind: "pdf", source: "C:/Users/you/Downloads", ident: "f:2", title: "Beyond the Warm Little Pond.pdf", location: "~/Downloads", uri: "file:///C:/Users/you/Downloads/Beyond the Warm Little Pond.pdf", modified_at: iso(now - 22 * day), size_label: "14 pages", size_num: 14, missing: false, starred: false, indexed_at: iso(now - 40 * 60_000),
    hits: [
      { chunk_id: 6, locator: "p.3", anchor: "3", score: .84, text: "2.1 ENERGY AND COMPARTMENT\n\nThe biophysical mechanics of abiogenesis depend on a persistent proton gradient across a mineral membrane, maintained by the chemistry of the vent itself rather than by any process inside the compartment." },
      { chunk_id: 7, locator: "p.9", anchor: "9", score: .72, text: "Vesicle formation under alkaline conditions was observed at pH 9.2 in the presence of fatty acids of mixed chain length." },
    ], total_hits: 2 },
  { id: 3, kind: "code", source: "C:/Users/you/Projects", ident: "f:3", title: "protocell_field.py", location: "~/Projects/protocell", uri: "file:///C:/Users/you/Projects/protocell/protocell_field.py", modified_at: iso(now - 70 * day), size_label: "312 lines", size_num: 312, missing: false, starred: true, indexed_at: iso(now - 2 * day),
    hits: [{ chunk_id: 8, locator: "L44", anchor: "44", score: .71, text: "# abiogenesis sim: gradient-driven vesicle growth, see Lane 2015\ndef step(field, dt):\n    grad = np.gradient(field.protons)\n    field.vesicles += dt * k * grad\n    return field" }], total_hits: 1 },
  { id: 4, kind: "image", source: "C:/Users/you/Downloads", ident: "f:4", title: "vent_diagram.png", location: "~/Downloads", uri: "file:///C:/Users/you/Downloads/vent_diagram.png", modified_at: iso(now - 20 * day), size_label: "1.2 MB", size_num: 1258291, missing: false, starred: false, indexed_at: iso(now - 25 * 60_000),
    hits: [{ chunk_id: 9, locator: "name", anchor: null, score: .63, text: "vent diagram" }], total_hits: 1 },
  { id: 5, kind: "spreadsheet", source: "C:/Users/you/Projects", ident: "f:5", title: "glossary_terms.xlsx", location: "~/Projects/protocell", uri: "file:///C:/Users/you/Projects/protocell/glossary_terms.xlsx", modified_at: iso(now - 1 * day), size_label: "4,420 rows", size_num: 4420, missing: false, starred: false, indexed_at: iso(now - 8 * 3600_000),
    hits: [{ chunk_id: 10, locator: "Sheet1", anchor: "Sheet1", score: .58, text: "id | term | parent | status\nT-0412 | abiogenesis | origins | merged\nT-0413 | prebiotic chemistry | origins | kept\nT-0414 | protocell | origins | kept" }], total_hits: 1 },
  { id: 6, kind: "chat", source: "gemini", ident: "gemini:6", title: "Block universe and the present", location: "Gemini", uri: "https://gemini.google.com/app/6", modified_at: iso(now - 83 * day), size_label: "36 turns", size_num: 36, missing: false, starred: true, indexed_at: iso(now - 5 * day), hits: [], total_hits: 0 },
  { id: 7, kind: "chat", source: "claude", ident: "claude:7", title: "Protocell lab notebook", location: "Claude", uri: "https://claude.ai/chat/7", modified_at: iso(now - 9 * day), size_label: "120 turns", size_num: 120, missing: false, starred: false, indexed_at: iso(now - 9 * day),
    hits: Array.from({ length: 34 }, (_, k) => ({ chunk_id: 200 + k, locator: `turn ${3 + k * 3}`, anchor: `n${k}`, score: .57 - k * .006, text: `Notebook entry ${k + 1}: abiogenesis run ${k + 1}, gradient held at ${(0.4 + k * 0.05).toFixed(2)} for ${12 + k} hours.` })), total_hits: 34 },
];

// filler for the timeline and gallery: spread across the last seven months
const FILL: [string, string, string, string][] = [
  ["pdf", "Dishwasher manual.pdf", "~/Downloads/Manuals", "44 pages"],
  ["spreadsheet", "Household budget 2026.xlsx", "~/Downloads", "919 rows"],
  ["document", "Book club notes.docx", "~/Documents/Notes", "4,085 words"],
  ["image", "whiteboard_oct.jpg", "~/Pictures/Phone", "2.4 MB"],
  ["code", "results_merge.py", "~/Projects/protocell", "188 lines"],
  ["chat", "Sourdough starter help", "Claude", "58 turns"],
  ["pdf", "Home insurance policy.pdf", "~/Documents/House", "5 pages"],
  ["image", "vent_chimney_sem.png", "~/Downloads", "840 KB"],
  ["document", "Cover letter v3.docx", "~/Documents/Letters", "2,210 words"],
  ["chat", "Owl logo ideas", "Gemini", "12 turns"],
  ["spreadsheet", "Reading list.xlsx", "~/Documents", "273 rows"],
  ["text", "field_notes.md", "~/Projects/protocell", "96 lines"],
  ["image", "owl_mascot_final.png", "~/Projects/recall/assets", "1.1 MB"],
  ["chat", "Recall v2 architecture", "ChatGPT", "22 turns"],
  ["pdf", "Beyond the Warm Little Pond (annotated).pdf", "~/Documents/Papers", "14 pages"],
  ["code", "watch.rs", "~/Projects/recall/core/src", "140 lines"],
];
FILL.forEach(([kind, title, location, size_label], i) => {
  const src = kind === "chat" ? location.toLowerCase() : location.startsWith("~/Projects") ? "C:/Users/you/Projects" : "C:/Users/you/Downloads";
  const uri = kind === "chat" ? `https://${src}.example/${i}` : `file:///C:/Users/you/${location.replace("~/", "")}/${title}`;
  const age = 3 + i * 13 + (i % 3) * 5;
  items.push({ id: 100 + i, kind, source: src, ident: `fill:${i}`, title, location, uri, modified_at: iso(now - age * day), size_label, size_num: 1000 * (i + 1), missing: false, starred: false, indexed_at: iso(now - age * day), hits: [], total_hits: 0 } as any);
});

// volume, so the timeline and gallery have more than one page to show
const VOL: [string, string, string][] = [["document", "docx", "~/Documents/Notes"], ["pdf", "pdf", "~/Downloads"], ["spreadsheet", "xlsx", "~/Documents/House"], ["image", "png", "~/Pictures/Phone"], ["code", "py", "~/Projects/protocell"], ["text", "md", "~/Projects/recall"]];
for (let i = 0; i < 280; i++) {
  const [kind, ext, location] = VOL[i % VOL.length];
  const src = location.startsWith("~/Projects") ? "C:/Users/you/Projects" : "C:/Users/you/Downloads";
  const title = `${["meeting notes", "draft", "export", "scan", "worklog", "summary"][i % 6]} ${String(i + 1).padStart(3, "0")}.${ext}`;
  const at = now - (150 + i * 2.1) * day - (i % 9) * 3600_000;
  items.push({ id: 1000 + i, kind, source: src, ident: `vol:${i}`, title, location, uri: `file:///C:/Users/you/${location.slice(2)}/${title}`, modified_at: iso(at), size_label: `${(i % 40) + 2} pages`, size_num: (i % 40) + 2, missing: false, starred: false, indexed_at: iso(at), hits: [], total_hits: 0 } as any);
}

let queued = 0, done = 0, current = "";
let sources = [{ path: "C:/Users/you/Downloads", enabled: true, count: 2041 }, { path: "C:/Users/you/Projects", enabled: true, count: 6733 }];
let recent: [string, number][] = [["abiogenesis", 4], ["dishwasher warranty", 31], ["houseplant watering", 2]];
let excluded: { ident: string; label: string; kind: string; at: string }[] = [];
let rules = { skip_dev_folders: true, skip_hidden: true, skip_packages: true, skip_media: true, skip_code_projects: true, skip_textless_images: true, max_mb: 64, follow_links: false, skipped_folders: ["C:/Users/you/Downloads/installers"], only_kinds: [] as string[] };
let platformEnabled: Record<string, boolean> = {};
let connections: Record<string, any> = {
  claude: { state: "connected", last_sync: iso(now - 9 * 60_000), last_error: null },
  gemini: { state: "signed_out", last_sync: iso(now - 2 * day), last_error: null },
  chatgpt: { state: "never", last_sync: null, last_error: null },
};

/** `?empty` starts with a new, empty library, to see the first-run screens. */
if (new URLSearchParams(location.search).has("empty")) {
  items.splice(0);
  sources = [];
  recent = [];
  for (const c of Object.values(connections)) Object.assign(c, { state: "never", last_sync: null });
}

const handlers: Record<string, Handler> = {
  status: () => ({
    version: "2.1.1", chunks: 18402 + done * 7, items: items.length + Math.floor(done / 2), embedder_ready: true, model: "bge-small-en-v1.5",
    sources, platforms: items.length ? [["claude", 1284], ["gemini", 312], ["chatgpt", 97]] : [],
    platform_enabled: platformEnabled, queued, done, current, feedback_enabled: true, connections,
  }),
  search: ({ query }) => {
    const t = (query.q as string).toLowerCase();
    const kinds: string[] = query.kinds ?? [];
    const cards = items
      .filter((c) => c.hits.length && !excluded.some((x) => x.ident === c.ident))
      .filter((c) => query.item == null || c.id === query.item)
      .filter((c) => !kinds.length || kinds.includes(c.kind))
      .filter((c) => !(query.sources ?? []).length || query.sources.includes(c.source))
      .filter((c) => t.length < 3 || c.title.toLowerCase().includes(t) || c.hits.some((h) => h.text.toLowerCase().includes(t)))
      .map((c) => ({ item_id: c.id, kind: c.kind, source: c.source, title: c.title, location: c.location, uri: c.uri, modified_at: c.modified_at, size_label: c.size_label, starred: c.starred, score: c.hits[0].score, hits: c.hits.slice(0, Math.min(query.hits ?? 5, 25)), total_hits: c.hits.length }));
    if (query.item != null) return { cards, total_hits: cards.reduce((n, c) => n + c.total_hits, 0), took_ms: 4 };
    recent = ([[query.q, cards.length] as [string, number]].concat(recent.filter(([r]) => r !== query.q))).slice(0, 8);
    return { cards, total_hits: cards.reduce((n, c) => n + c.total_hits, 0), took_ms: 12 };
  },
  recent_queries: () => recent,
  clear_recent_queries: () => { recent = []; },
  remove_recent_query: ({ query }) => { recent = recent.filter(([q]) => q !== query); },
  starred: () => items.filter((i) => i.starred && !excluded.some((x) => x.ident === i.ident)),
  recently_indexed: ({ limit }) => items.filter((i) => !excluded.some((x) => x.ident === i.ident)).sort((a, b) => Date.parse(b.indexed_at) - Date.parse(a.indexed_at)).slice(0, limit ?? 20),
  browse: ({ kinds, limit, offset, oldest }) => items
    .filter((i) => (!kinds.length || kinds.includes(i.kind)) && !excluded.some((x) => x.ident === i.ident))
    .sort((a, b) => (oldest ? 1 : -1) * (Date.parse(a.modified_at) - Date.parse(b.modified_at)))
    .slice(offset ?? 0, (offset ?? 0) + (limit ?? 200)),
  item_chunks: ({ itemId }) => (items.find((i) => i.id === itemId)?.hits ?? []).map((h, ord) => ({ id: h.chunk_id, item_id: itemId, ord, locator: h.locator, text: h.text, anchor: h.anchor })),
  set_star: ({ itemId, on }) => { const i = items.find((x) => x.id === itemId); if (i) i.starred = on; },
  exclude_item: ({ itemId }) => { const i = items.find((x) => x.id === itemId); if (!i) return null; excluded.push({ ident: i.ident, label: i.kind === "chat" ? i.title : i.uri.replace("file:///", ""), kind: i.kind, at: iso(now) }); return i; },
  unexclude: ({ ident }) => { excluded = excluded.filter((x) => x.ident !== ident); },
  list_excluded: () => excluded,
  add_source: ({ path }) => {
    sources.push({ path, enabled: true, count: 0 });
    queued += 240; current = "starting";
    const names = ["book_club_notes.docx", "dishwasher_manual.pdf", "field_notes.md", "readme.txt", "household_budget.xlsx", "photo_rename.py"];
    const tick = setInterval(() => {
      done = Math.min(queued, done + 3);
      current = names[done % names.length];
      sources[sources.length - 1].count = Math.floor(done / sources.length);
      if (done >= queued) { current = ""; clearInterval(tick); }
    }, 120);
  },
  remove_source: ({ path }) => { sources = sources.filter((s) => s.path !== path); },
  set_source_enabled: ({ path, enabled }) => { const s = sources.find((x) => x.path === path); if (s) s.enabled = enabled; },
  resync_source: () => undefined,
  resync_all: () => undefined,
  get_rules: () => rules,
  set_rules: ({ rules: r }) => { rules = r; },
  preview_rules: ({ rules: r }) => { const skipped = (r.skip_dev_folders ? 900 : 0) + (r.skip_hidden ? 140 : 0) + (r.skip_packages ? 60 : 0) + (r.skip_media ? 80 : 0) + r.skipped_folders.length * 25 + (r.only_kinds.length ? 400 : 0); return { matched: 9982 - skipped, excluded: skipped }; },
  set_platform_enabled: ({ platform, enabled }) => { platformEnabled[platform] = enabled; },
  data_info: () => ({ dir: "C:/Users/you/AppData/Local/Recall", db_bytes: 184_000_000, index_bytes: 56_000_000 }),
  reset_index: () => undefined,
  connect_platform: ({ platform }) => { connections[platform] = { ...connections[platform], state: "signing_in" }; setTimeout(() => { connections[platform] = { state: "connected", last_sync: iso(Date.now()), last_error: null }; }, 2500); },
  sync_platform: ({ platform }) => { connections[platform] = { ...connections[platform], state: "syncing" }; setTimeout(() => { connections[platform] = { state: "connected", last_sync: iso(Date.now()), last_error: null }; }, 1800); },
  disconnect_platform: ({ platform }) => { connections[platform] = { state: "never", last_sync: null, last_error: null }; },
  send_feedback: ({ text }) => { console.info("feedback:", text); },
  // app events (sign-in finished): nothing fires in the browser, but listening must not throw
  "plugin:event|listen": () => 0,
  "plugin:event|unlisten": () => undefined,
  pick_folders: () => ["C:/Users/you/Documents"],
  open_item: ({ itemId }) => { const i = items.find((x) => x.id === itemId); if (i?.uri.startsWith("https://")) window.open(i.uri, "_blank"); else console.info("open", i?.uri); },
  reveal_item: ({ itemId }) => { console.info("reveal", items.find((x) => x.id === itemId)?.uri); },
};

export function installMock() {
  (window as any).__RECALL_MOCK__ = true;
  (window as any).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: any) => {
      const h = handlers[cmd];
      if (!h) throw new Error(`mock: no handler for ${cmd}`);
      await new Promise((r) => setTimeout(r, 30));
      return h(args ?? {});
    },
    transformCallback: (cb: Function) => cb,
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    plugins: {},
  };
}
