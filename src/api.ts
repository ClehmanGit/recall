import { invoke } from "@tauri-apps/api/core";

export interface Hit { chunk_id: number; locator: string; text: string; anchor: string | null; score: number }
export interface Card {
  item_id: number; kind: string; source: string; title: string; location: string; uri: string;
  modified_at: string; size_label: string; starred: boolean; score: number; hits: Hit[]; total_hits: number;
}
export interface Results { cards: Card[]; total_hits: number; took_ms: number }
export interface Item {
  id: number; kind: string; source: string; ident: string; title: string; location: string; uri: string;
  modified_at: string; size_label: string; size_num: number; missing: boolean; starred: boolean;
}
export interface RecentItem extends Item { indexed_at: string }
export interface SourceRow { path: string; enabled: boolean; count: number }
export interface Status {
  version: string; chunks: number; items: number; vectors: number; embedder_ready: boolean; model: string; dim: number;
  sources: SourceRow[]; platforms: [string, number][];
  platform_enabled: Record<string, boolean>; queued: number; done: number; current: string;
  feedback_enabled: boolean; data_dir: string;
  connections: Record<string, Connection>;
  /** one plain line per chat app, written by the app */
  connection_lines: Record<string, string>;
}
export interface Connection { state: "never" | "signing_in" | "connected" | "signed_out" | "syncing"; last_sync: string | null; last_error: string | null; synced: number }
export interface Rules {
  skip_dev_folders: boolean; skip_hidden: boolean; skip_packages: boolean; skip_media: boolean;
  skip_code_projects: boolean; skip_textless_images: boolean;
  max_mb: number; follow_links: boolean; ocr: boolean; skipped_folders: string[]; only_kinds: string[];
}
export interface ExcludedRow { ident: string; label: string; kind: string; at: string }
export interface Chunk { id: number; item_id: number; ord: number; locator: string; text: string; anchor: string | null }
export interface DataInfo { dir: string; db_bytes: number; index_bytes: number }

export const api = {
  status: () => invoke<Status>("status"),
  search: (q: string, kinds: string[], sources: string[]) => invoke<Results>("search", { query: { q, kinds, sources, limit: 30 } }),
  /** One card's passages for the same query, up to `hits` (the engine caps it at 25). */
  cardHits: (q: string, kinds: string[], sources: string[], item: number, hits: number) =>
    invoke<Results>("search", { query: { q, kinds, sources, limit: 1, item, hits } }),
  recentQueries: () => invoke<[string, number][]>("recent_queries"),
  clearRecentQueries: () => invoke<void>("clear_recent_queries"),
  removeRecentQuery: (query: string) => invoke<void>("remove_recent_query", { query }),
  starred: () => invoke<Item[]>("starred"),
  recentlyIndexed: (limit = 20) => invoke<RecentItem[]>("recently_indexed", { limit }),
  /** A page of items by modified date, newest first unless `oldest`. */
  browse: (kinds: string[], limit = 200, offset = 0, oldest = false) => invoke<Item[]>("browse", { kinds, limit, offset, oldest }),
  itemChunks: (itemId: number, limit = 40) => invoke<Chunk[]>("item_chunks", { itemId, limit }),
  openItem: (itemId: number, anchor?: string) => invoke<void>("open_item", { itemId, anchor: anchor ?? null }),
  revealItem: (itemId: number) => invoke<void>("reveal_item", { itemId }),
  setStar: (itemId: number, on: boolean) => invoke<void>("set_star", { itemId, on }),
  excludeItem: (itemId: number) => invoke<Item | null>("exclude_item", { itemId }),
  unexclude: (ident: string, uri: string, source: string) => invoke<void>("unexclude", { ident, uri, source }),
  listExcluded: () => invoke<ExcludedRow[]>("list_excluded"),
  addSource: (path: string) => invoke<void>("add_source", { path }),
  removeSource: (path: string) => invoke<void>("remove_source", { path }),
  setSourceEnabled: (path: string, enabled: boolean) => invoke<void>("set_source_enabled", { path, enabled }),
  resyncSource: (path: string) => invoke<void>("resync_source", { path }),
  resyncAll: () => invoke<void>("resync_all"),
  getRules: () => invoke<Rules>("get_rules"),
  setRules: (rules: Rules) => invoke<void>("set_rules", { rules }),
  previewRules: (rules: Rules) => invoke<{ matched: number; excluded: number }>("preview_rules", { rules }),
  setPlatformEnabled: (platform: string, enabled: boolean) => invoke<void>("set_platform_enabled", { platform, enabled }),
  dataInfo: () => invoke<DataInfo>("data_info"),
  resetIndex: () => invoke<void>("reset_index"),
  sendFeedback: (text: string) => invoke<void>("send_feedback", { text }),
  connectPlatform: (platform: string) => invoke<void>("connect_platform", { platform }),
  syncPlatform: (platform: string) => invoke<void>("sync_platform", { platform }),
  disconnectPlatform: (platform: string) => invoke<void>("disconnect_platform", { platform }),
};
