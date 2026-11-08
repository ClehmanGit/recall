//! Tauri shell: wires the window to recall-core.

mod connect;

use recall_core::{bridge, db, feedback, ingest, rules::Rules, search, Engine};
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
    pub vectors: usize,
    pub embedder_ready: bool,
    pub model: String,
    pub dim: usize,
    pub bridge_port: u16,
    pub token: String,
    pub sources: Vec<db::SourceRow>,
    pub platforms: Vec<(String, i64)>,
    pub platform_enabled: std::collections::HashMap<String, bool>,
    pub queued: usize,
    pub done: usize,
    pub current: String,
    pub feedback_enabled: bool,
    pub data_dir: String,
    pub connections: std::collections::HashMap<String, connect::Connection>,
    /// one plain line per chat app for Settings
    pub connection_lines: std::collections::HashMap<String, String>,
}

#[tauri::command]
async fn status(app: State<'_, Engine>, conns: State<'_, connect::Connections>) -> Result<Status, String> {
    let prog = app.ingest.progress.snapshot();
    let (model, dim) = {
        let g = app.ingest.embedder.lock();
        match g.as_ref() {
            Some(e) => (e.cfg.name.clone(), e.dim),
            None => (String::new(), 0),
        }
    };
    Ok(Status {
        version: VERSION.into(),
        chunks: db::chunk_count(&app.db).map_err(err)?,
        items: db::item_count(&app.db).map_err(err)?,
        vectors: app.index.read().len(),
        embedder_ready: app.ingest.embedder.lock().is_some(),
        model,
        dim,
        bridge_port: bridge::PORT,
        token: app.token.clone(),
        sources: db::list_sources(&app.db).map_err(err)?,
        platforms: db::platform_counts(&app.db).map_err(err)?,
        platform_enabled: db::setting_get(&app.db, "platform_enabled").map_err(err)?.unwrap_or_default(),
        queued: prog.0,
        done: prog.1,
        current: prog.2,
        feedback_enabled: !FEEDBACK_URL.is_empty(),
        data_dir: app.data_dir.display().to_string(),
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
    let item = db::exclude_item(&app.db, item_id).map_err(err)?;
    let _ = app.ingest.tx.send(ingest::Job::Save);
    Ok(item)
}

/// Undo for exclude: drops the exclusion and re-queues the file (or waits for the next chat sync).
#[tauri::command]
async fn unexclude(app: State<'_, Engine>, ident: String, uri: String, source: String) -> Result<(), String> {
    db::unexclude(&app.db, &ident).map_err(err)?;
    if let Some(p) = uri.strip_prefix("file:///") {
        let path = PathBuf::from(p);
        if path.exists() {
            let _ = app.ingest.tx.send(ingest::Job::File { path, source });
        }
    }
    Ok(())
}

#[tauri::command]
async fn list_excluded(app: State<'_, Engine>) -> Result<Vec<db::ExcludedRow>, String> {
    db::list_excluded(&app.db).map_err(err)
}

// ---------- sources and rules ----------

#[tauri::command]
async fn add_source(app: State<'_, Engine>, path: String) -> Result<(), String> {
    db::add_source(&app.db, &path).map_err(err)?;
    app.watch.lock().add_root(Path::new(&path)).map_err(err)?;
    app.ingest.tx.send(ingest::Job::SweepSource(path)).map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn remove_source(app: State<'_, Engine>, path: String) -> Result<(), String> {
    app.watch.lock().remove_root(Path::new(&path)).map_err(err)?;
    db::remove_source(&app.db, &path).map_err(err)
}

#[tauri::command]
async fn set_source_enabled(app: State<'_, Engine>, path: String, enabled: bool) -> Result<(), String> {
    db::set_source_enabled(&app.db, &path, enabled).map_err(err)?;
    let mut w = app.watch.lock();
    if enabled {
        w.add_root(Path::new(&path)).map_err(err)?;
        let _ = app.ingest.tx.send(ingest::Job::SweepSource(path));
    } else {
        w.remove_root(Path::new(&path)).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
async fn resync_source(app: State<'_, Engine>, path: String) -> Result<(), String> {
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
    resync_everything(&app)
}

// ---------- feedback ----------

#[tauri::command]
async fn send_feedback(text: String) -> Result<(), String> {
    feedback::send(FEEDBACK_URL, &text, VERSION).map_err(err)
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
            app.manage(engine);
            app.manage(conns);
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
            connect::connect_platform,
            connect::sync_platform,
            connect::disconnect_platform,
            connect::connect_cursor,
            connect::connect_known,
            connect::connect_ingest,
            connect::connect_progress,
            connect::connect_report
        ])
        .run(tauri::generate_context!())
        .expect("error while running Recall");
}
