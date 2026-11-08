// Every command gets an `allow-<command>` permission so the capabilities can say
// which window may call it: the main window gets all of them, the sign-in
// windows only the three the reader script needs.
fn main() {
    let attributes = tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
        "status",
        "search",
        "recent_queries",
        "clear_recent_queries",
        "remove_recent_query",
        "starred",
        "recently_indexed",
        "browse",
        "item_chunks",
        "set_star",
        "exclude_item",
        "unexclude",
        "list_excluded",
        "pick_folders",
        "add_source",
        "remove_source",
        "set_source_enabled",
        "resync_source",
        "resync_all",
        "get_rules",
        "set_rules",
        "preview_rules",
        "set_platform_enabled",
        "data_info",
        "reset_index",
        "send_feedback",
        "open_item",
        "reveal_item",
        "connect_platform",
        "sync_platform",
        "disconnect_platform",
        "connect_known",
        "connect_ingest",
        "connect_progress",
        "connect_report",
    ]));
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
