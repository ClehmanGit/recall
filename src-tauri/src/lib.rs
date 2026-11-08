//! Tauri shell: wires the window to recall-core.

mod connect;
mod open;

use recall_core::{db, feedback, ingest, rules::Rules, search, Engine};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{Manager, State};

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Set at build time: `RECALL_FEEDBACK_URL=https://... npm run build`. Empty hides the button.
const FEEDBACK_URL: &str = match option_env!("RECALL_FEEDBACK_URL") {
    Some(u) => u,
    None => "",
};

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------- status ----------

#[derive(Serialize)]
pub struct Status {
    pub version: String,
    pub chunks: i64,
    pub items: i64,
    pub embedder_ready: bool,
    pub model: String,
    pub sources: Vec<db::SourceRow>,
    pub platforms: Vec<(String, i64)>,
    pub platform_enabled: std::collections::HashMap<String, bool>,
    pub queued: usize,
    pub done: usize,
    pub current: String,
    pub feedback_enabled: bool,
    pub connections: std::collections::HashMap<String, connect::Connection>,
    /// one plain line per chat app for Settings
    pub connection_lines: std::collections::HashMap<String, String>,
}

#[tauri::command]
async fn status(app: State<'_, Engine>, conns: State<'_, connect::Connections>) -> Result<Status, String> {
    let prog = app.ingest.progress.snapshot();
    let model = app.ingest.embedder.lock().as_ref().map(|e| e.cfg.name.clone()).unwrap_or_default();
    Ok(Status {
        version: VERSION.into(),
        chunks: db::chunk_count(&app.db).map_err(err)?,
        items: db::item_count(&app.db).map_err(err)?,
        embedder_ready: app.ingest.embedder.lock().is_some(),
        model,
        sources: db::list_sources(&app.db).map_err(err)?,
        platforms: db::platform_counts(&app.db).map_err(err)?,
        platform_enabled: db::setting_get(&app.db, "platform_enabled").map_err(err)?.unwrap_or_default(),
        queued: prog.0,
        done: prog.1,
        current: prog.2,
        feedback_enabled: !FEEDBACK_URL.is_empty(),
        connections: conns.snapshot(),
        connection_lines: conns.lines(),
    })
}

// ---------- search and home ----------

#[tauri::command]
async fn search(app: State<'_, Engine>, query: search::Query) -> Result<search::Results, String> {
    search::run(&app.db, &app.index, &app.ingest.embedder, &query).map_err(err)
}

#[tauri::command]
async fn recent_queries(app: State<'_, Engine>) -> Result<Vec<(String, i64)>, String> {
    db::recent_queries(&app.db, 8).map_err(err)
}

#[tauri::command]
async fn clear_recent_queries(app: State<'_, Engine>) -> Result<(), String> {
    db::clear_recent_queries(&app.db).map_err(err)
}

#[tauri::command]
async fn remove_recent_query(app: State<'_, Engine>, query: String) -> Result<(), String> {
    db::remove_recent_query(&app.db, &query).map_err(err)
}

#[tauri::command]
async fn starred(app: State<'_, Engine>) -> Result<Vec<db::Item>, String> {
    db::starred(&app.db, 12).map_err(err)
}

#[derive(Serialize)]
pub struct RecentItem {
    #[serde(flatten)]
    pub item: db::Item,
    pub indexed_at: String,
}

#[tauri::command]
async fn recently_indexed(app: State<'_, Engine>, limit: Option<usize>) -> Result<Vec<RecentItem>, String> {
    Ok(db::recently_indexed(&app.db, limit.unwrap_or(20).min(500))
        .map_err(err)?
        .into_iter()
        .map(|(item, indexed_at)| RecentItem { item, indexed_at })
        .collect())
}

#[tauri::command]
async fn browse(app: State<'_, Engine>, kinds: Vec<String>, limit: usize, offset: Option<usize>, oldest: Option<bool>) -> Result<Vec<db::Item>, String> {
    db::browse(&app.db, &kinds, limit.min(500), offset.unwrap_or(0), oldest.unwrap_or(false)).map_err(err)
}

#[tauri::command]
async fn item_chunks(app: State<'_, Engine>, item_id: i64, limit: usize) -> Result<Vec<db::Chunk>, String> {
    db::item_chunks(&app.db, item_id, limit.min(200)).map_err(err)
}

#[tauri::command]
async fn set_star(app: State<'_, Engine>, item_id: i64, on: bool) -> Result<(), String> {
    db::set_star(&app.db, item_id, on).map_err(err)
}

/// Excludes one item. Returns it so the UI can offer Undo.
#[tauri::command]
async fn exclude_item(app: State<'_, Engine>, item_id: i64) -> Result<Option<db::Item>, String> {
    let (item, chunks) = db::exclude_item(&app.db, item_id).map_err(err)?;
    app.index.write().forget(&chunks);
    db::shrink(&app.db);
    let _ = app.ingest.tx.send(ingest::Job::Save);
    Ok(item)
}

/// Undo for exclude: drops the exclusion and, for a file still inside a chosen
/// folder, reads it again now (a chat returns with the next backup).
#[tauri::command]
async fn unexclude(app: State<'_, Engine>, ident: String) -> Result<(), String> {
    let row = db::list_excluded(&app.db).map_err(err)?.into_iter().find(|x| x.ident == ident);
    db::unexclude(&app.db, &ident).map_err(err)?;
    if let Some(row) = row.filter(|r| r.kind != "chat") {
        let path = real_path(&row.label);
        if let Some(source) = chosen_folder_of(&app, &path) {
            if path.is_file() {
                let _ = app.ingest.tx.send(ingest::Job::File { path, source });
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn list_excluded(app: State<'_, Engine>) -> Result<Vec<db::ExcludedRow>, String> {
    db::list_excluded(&app.db).map_err(err)
}

// ---------- sources and rules ----------

// The window shows image thumbnails and quick-view previews by reading files
// through Tauri's asset protocol. Its scope starts empty (tauri.conf.json) and
// holds only the folders chosen in Settings, so the window can read those
// files and nothing else on the computer.

fn real_path(path: &str) -> PathBuf {
    // the protocol checks the file's real, on-disk path, so match its casing
    std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path))
}

fn allow_previews(handle: &tauri::AppHandle, path: &str) {
    if let Err(e) = handle.asset_protocol_scope().allow_directory(real_path(path), true) {
        tracing::warn!("could not allow previews for {path}: {e}");
    }
}

/// Tauri can add to the scope but not take anything back; it can only forbid a
/// folder, and that lasts until Recall restarts, even if the folder is added
/// again (its previews come back after a restart). A folder that overlaps one
/// still chosen is left alone so the other keeps its previews.
fn forbid_previews(handle: &tauri::AppHandle, app: &Engine, path: &str) {
    let gone = real_path(path);
    let overlaps = db::list_sources(&app.db)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.enabled && s.path != path)
        .map(|s| real_path(&s.path))
        .any(|kept| kept.starts_with(&gone) || gone.starts_with(&kept));
    if overlaps {
        return;
    }
    if let Err(e) = handle.asset_protocol_scope().forbid_directory(gone, true) {
        tracing::warn!("could not forbid previews for {path}: {e}");
    }
}

/// The enabled chosen folder that holds `path`, as stored.
fn chosen_folder_of(app: &Engine, path: &Path) -> Option<String> {
    db::list_sources(&app.db)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.enabled)
        .find(|s| path.starts_with(real_path(&s.path)))
        .map(|s| s.path)
}

fn is_source(app: &Engine, path: &str) -> Result<(), String> {
    if db::list_sources(&app.db).map_err(err)?.iter().any(|s| s.path == path) {
        Ok(())
    } else {
        Err("That folder is not one Recall searches.".into())
    }
}

/// Folders the person picked in the system dialog and has not added yet. Only
/// these can become sources, so nothing but the person's own choice decides
/// what Recall reads.
#[derive(Default)]
struct Picked(parking_lot::Mutex<std::collections::HashSet<String>>);

/// Opens the system folder picker. For `purpose` "source" the folders are
/// remembered for `add_source`; for "skip" they only fill the skip list.
#[tauri::command]
async fn pick_folders(handle: tauri::AppHandle, picked: State<'_, Picked>, purpose: String) -> Result<Vec<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let title = if purpose == "skip" { "Folders to skip" } else { "Choose folders for Recall" };
    let h = handle.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || h.dialog().file().set_title(title).blocking_pick_folders())
        .await
        .map_err(err)?
        .unwrap_or_default();
    let paths: Vec<String> = chosen.into_iter().filter_map(|p| p.into_path().ok()).map(|p| p.display().to_string()).collect();
    if purpose == "source" {
        picked.0.lock().extend(paths.iter().cloned());
    }
    Ok(paths)
}

#[tauri::command]
async fn add_source(handle: tauri::AppHandle, app: State<'_, Engine>, picked: State<'_, Picked>, path: String) -> Result<(), String> {
    if !picked.0.lock().remove(&path) {
        return Err("Choose the folder with Add folders.".into());
    }
    db::add_source(&app.db, &path).map_err(err)?;
    allow_previews(&handle, &path);
    app.watch.lock().add_root(Path::new(&path)).map_err(err)?;
    app.ingest.tx.send(ingest::Job::SweepSource(path)).map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn remove_source(handle: tauri::AppHandle, app: State<'_, Engine>, path: String) -> Result<(), String> {
    is_source(&app, &path)?;
    app.watch.lock().remove_root(Path::new(&path)).map_err(err)?;
    let chunks = db::remove_source(&app.db, &path).map_err(err)?;
    app.index.write().forget(&chunks);
    db::shrink(&app.db);
    let _ = app.ingest.tx.send(ingest::Job::Save);
    forbid_previews(&handle, &app, &path);
    Ok(())
}

#[tauri::command]
async fn set_source_enabled(handle: tauri::AppHandle, app: State<'_, Engine>, path: String, enabled: bool) -> Result<(), String> {
    is_source(&app, &path)?;
    db::set_source_enabled(&app.db, &path, enabled).map_err(err)?;
    if enabled {
        allow_previews(&handle, &path);
        app.watch.lock().add_root(Path::new(&path)).map_err(err)?;
        let _ = app.ingest.tx.send(ingest::Job::SweepSource(path));
    } else {
        app.watch.lock().remove_root(Path::new(&path)).map_err(err)?;
        forbid_previews(&handle, &app, &path);
    }
    Ok(())
}

#[tauri::command]
async fn resync_source(app: State<'_, Engine>, path: String) -> Result<(), String> {
    is_source(&app, &path)?;
    app.ingest.tx.send(ingest::Job::SweepSource(path)).map_err(err)
}

fn resync_everything(app: &Engine) -> Result<(), String> {
    for s in db::list_sources(&app.db).map_err(err)? {
        if s.enabled {
            let _ = app.ingest.tx.send(ingest::Job::SweepSource(s.path));
        }
    }
    Ok(())
}

#[tauri::command]
async fn resync_all(app: State<'_, Engine>) -> Result<(), String> {
    resync_everything(&app)
}

#[tauri::command]
async fn get_rules(app: State<'_, Engine>) -> Result<Rules, String> {
    Ok(ingest::load_rules(&app.db))
}

/// Saves rules and re-sweeps every enabled source so removals take effect.
#[tauri::command]
async fn set_rules(app: State<'_, Engine>, rules: Rules) -> Result<(), String> {
    db::setting_set(&app.db, "rules", &rules).map_err(err)?;
    resync_everything(&app)
}

#[derive(Serialize)]
pub struct RulesPreview {
    pub matched: usize,
    pub excluded: usize,
}

#[tauri::command]
async fn preview_rules(app: State<'_, Engine>, rules: Rules) -> Result<RulesPreview, String> {
    let (matched, excluded) = ingest::preview(&app.db, &rules);
    Ok(RulesPreview { matched, excluded })
}

#[tauri::command]
async fn set_platform_enabled(app: State<'_, Engine>, platform: String, enabled: bool) -> Result<(), String> {
    let mut map: std::collections::HashMap<String, bool> = db::setting_get(&app.db, "platform_enabled").map_err(err)?.unwrap_or_default();
    map.insert(platform, enabled);
    db::setting_set(&app.db, "platform_enabled", &map).map_err(err)
}

// ---------- data ----------

#[derive(Serialize)]
pub struct DataInfo {
    pub dir: String,
    pub db_bytes: u64,
    pub index_bytes: u64,
}

#[tauri::command]
async fn data_info(app: State<'_, Engine>) -> Result<DataInfo, String> {
    let size = |p: PathBuf| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let index_bytes = std::fs::read_dir(&app.data_dir)
        .map(|d| d.filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".usearch")).map(|e| e.metadata().map(|m| m.len()).unwrap_or(0)).sum())
        .unwrap_or(0);
    Ok(DataInfo { dir: app.data_dir.display().to_string(), db_bytes: size(app.data_dir.join("recall.sqlite")), index_bytes })
}

/// Clears items and chunks, keeps sources, rules, exclusions. Re-sweeps.
#[tauri::command]
async fn reset_index(app: State<'_, Engine>) -> Result<(), String> {
    db::clear_index(&app.db).map_err(err)?;
    app.index.write().clear().map_err(err)?;
    let _ = app.ingest.tx.send(ingest::Job::Save);
    resync_everything(&app)
}

// ---------- feedback ----------

#[tauri::command]
async fn send_feedback(text: String) -> Result<(), String> {
    // a blocking request: keep it off the async runtime's threads
    tauri::async_runtime::spawn_blocking(move || feedback::send(FEEDBACK_URL, &text, VERSION))
        .await
        .map_err(err)?
        .map_err(err)
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Tray icon: click to open Recall, or the menu's Open and Quit. Quit is the
/// only way out once the window is closed.
fn tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    let open = MenuItem::with_id(app, "open", "Open Recall", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Recall", true, None::<&str>)?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Recall")
        .menu(&Menu::with_items(app, &[&open, &quit])?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// The configured 1280x820 is taller than many laptop screens once the taskbar
/// is taken off. Shrink to fit the free part of the screen the window opens on,
/// with a small margin, and center it there.
fn fit_to_screen(w: &tauri::WebviewWindow) {
    let (Ok(Some(monitor)), Ok(outer), Ok(inner)) = (w.current_monitor(), w.outer_size(), w.inner_size()) else { return };
    let area = monitor.work_area();
    let margin = (24.0 * monitor.scale_factor()) as u32;
    // set_size sizes the content; the title bar and frame come on top of it
    let (frame_w, frame_h) = (outer.width.saturating_sub(inner.width), outer.height.saturating_sub(inner.height));
    let width = outer.width.min(area.size.width.saturating_sub(margin * 2));
    let height = outer.height.min(area.size.height.saturating_sub(margin * 2));
    if (width, height) != (outer.width, outer.height) {
        let _ = w.set_size(tauri::PhysicalSize::new(width.saturating_sub(frame_w), height.saturating_sub(frame_h)));
    }
    let x = area.position.x + (area.size.width.saturating_sub(width) / 2) as i32;
    let y = area.position.y + (area.size.height.saturating_sub(height) / 2) as i32;
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
}

pub fn run() {
    // RUST_LOG when set, otherwise info. Also written to recall.log in the data
    // folder, since the installed app has no console. Appended, so a second launch
    // (which only brings the first forward) cannot wipe it; started over past 2 MB.
    use tracing_subscriber::fmt::writer::MakeWriterExt;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("recall_core=info,recall_lib=info"));
    let data_dir = recall_core::data_dir();
    let _ = std::fs::create_dir_all(&data_dir);
    let log = tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false);
    let log_path = data_dir.join("recall.log");
    let big = std::fs::metadata(&log_path).map(|m| m.len() > 2 * 1024 * 1024).unwrap_or(false);
    match std::fs::OpenOptions::new().create(true).write(true).append(!big).truncate(big).open(&log_path) {
        Ok(file) => log.with_writer(std::io::stdout.and(std::sync::Mutex::new(file))).init(),
        Err(_) => log.init(),
    }

    tauri::Builder::default()
        // first: a second launch brings the running Recall forward and exits
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // closing the window keeps Recall in the tray, so chat backups keep running
        .on_window_event(|w, e| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                if w.label() == "main" {
                    api.prevent_close();
                    let _ = w.hide();
                }
            }
        })
        .setup(|app| {
            tray(app)?;
            let model_dir = app.path().resource_dir().map(|r| r.join("models")).unwrap_or_else(|_| PathBuf::from("models"));
            let engine = recall_core::boot(model_dir)?;
            let conns = connect::Connections::load(&engine);
            for s in db::list_sources(&engine.db).unwrap_or_default() {
                if s.enabled {
                    allow_previews(app.handle(), &s.path);
                }
            }
            app.manage(engine);
            app.manage(conns);
            app.manage(Picked::default());
            connect::start_scheduler(app.handle().clone());
            // the window starts hidden (tauri.conf.json) so it appears already sized
            if let Some(w) = app.get_webview_window("main") {
                fit_to_screen(&w);
                let _ = w.show();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            status,
            search,
            recent_queries,
            clear_recent_queries,
            remove_recent_query,
            starred,
            recently_indexed,
            browse,
            item_chunks,
            set_star,
            exclude_item,
            unexclude,
            list_excluded,
            pick_folders,
            add_source,
            remove_source,
            set_source_enabled,
            resync_source,
            resync_all,
            get_rules,
            set_rules,
            preview_rules,
            set_platform_enabled,
            data_info,
            reset_index,
            send_feedback,
            open::open_item,
            open::reveal_item,
            connect::connect_platform,
            connect::sync_platform,
            connect::disconnect_platform,
            connect::connect_known,
            connect::connect_ingest,
            connect::connect_progress,
            connect::connect_report
        ])
        .run(tauri::generate_context!())
        .expect("error while running Recall");
}
