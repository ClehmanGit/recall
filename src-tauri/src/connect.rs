//! In-app sign-in for chat apps. Each platform gets its own webview window that
//! shows the site's own sign-in page. Once signed in, the window is hidden and
//! kept for backups; `connect/reader.js` runs inside it and hands conversations
//! to the commands below. Cookies live in the app's WebView2 profile on this
//! machine, and the window only ever loads the platform's own site.
//!
//! Two kinds of backup, both run by the reader:
//! - quick: starred chats and the most recent ones, on sign-in and every fifteen
//!   minutes, so new chats are searchable within seconds;
//! - full: everything else, slowly, only while the computer is idle or at night.
//! Both read only chats Recall does not hold yet or that changed (`connect_known`).

use parking_lot::Mutex;
use recall_core::{bridge, db, ingest::ChatPayload, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing::{debug, info, warn};

const READER: &str = include_str!("../connect/reader.js");

/// Quick backup (starred and recent chats) this often.
const QUICK_EVERY: Duration = Duration::from_secs(15 * 60);
/// How often the scheduler looks at the clock and the idle time.
const TICK: Duration = Duration::from_secs(30);
/// Keyboard and mouse idle time before a full backup may run.
const IDLE_FOR_FULL: u64 = 5 * 60;
/// Chats read per full-backup run; the scheduler starts the next run while still idle.
const FULL_RUN: usize = 50;
/// Hours (local time) when a full backup may run even if the computer is in use.
const NIGHT: std::ops::Range<u32> = 1..6;
/// After a site answers 429 (too many requests), wait this long.
const PAUSE_AFTER_REFUSAL: Duration = Duration::from_secs(30 * 60);

pub const PLATFORMS: [&str; 3] = ["claude", "gemini", "chatgpt"];

fn start_url(platform: &str) -> &'static str {
    match platform {
        "claude" => "https://claude.ai/new",
        "gemini" => "https://gemini.google.com/app",
        _ => "https://chatgpt.com/",
    }
}

fn label(platform: &str) -> String {
    format!("connect-{platform}")
}

fn name(platform: &str) -> &'static str {
    match platform {
        "claude" => "Claude",
        "gemini" => "Gemini",
        _ => "ChatGPT",
    }
}

fn title(platform: &str) -> String {
    format!("Sign in to {} · Recall", name(platform))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Connection {
    /// "never" | "signing_in" | "connected" | "signed_out" | "syncing"
    pub state: String,
    pub last_sync: Option<String>,
    pub last_error: Option<String>,
    pub synced: usize,
    /// "quick" | "full" while a backup runs
    #[serde(default)]
    pub pass: Option<String>,
    /// "listing" | "reading" while a backup runs
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub done: usize,
    #[serde(default)]
    pub todo: usize,
    /// chats the site lists, from the last backup that read the whole list
    #[serde(default)]
    pub total: Option<usize>,
    /// chats a full backup still has to read
    #[serde(default)]
    pub remaining: Option<usize>,
    #[serde(default)]
    pub last_quick: Option<String>,
    /// local date (YYYY-MM-DD) of the last full backup that finished
    #[serde(default)]
    pub full_done_on: Option<String>,
    /// RFC3339; no backups before this, because the site asked to slow down
    #[serde(default)]
    pub paused_until: Option<String>,
}

fn paused(c: &Connection) -> bool {
    c.paused_until.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()).is_some_and(|t| t > chrono::Utc::now())
}

fn thousands(v: usize) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 { out.push(','); }
        out.push(ch);
    }
    out
}

/// One line in plain words for Settings.
pub fn status_line(platform: &str, c: &Connection) -> String {
    let n = thousands;
    let app = name(platform);
    match (c.state.as_str(), c.pass.as_deref(), c.stage.as_deref()) {
        ("never", ..) | ("", ..) => "Not connected".into(),
        ("signing_in", ..) => "Waiting for you to sign in".into(),
        ("signed_out", ..) => format!("Signed out of {app}. Sign in again to keep backing up."),
        (_, Some(_), Some("listing")) => "Signed in. Getting the list of chats…".into(),
        (_, Some("quick"), _) => format!("Reading {} of {} recent and starred chats", n(c.done), n(c.todo)),
        (_, Some("full"), _) => format!("Backing up older chats: {} of {} in this run. Runs while the computer is idle.", n(c.done), n(c.todo)),
        _ => {
            let mut s = match (c.total, c.remaining) {
                (Some(0), _) => "Signed in. No saved chats found.".to_string(),
                (Some(t), Some(0)) => format!("All {} chats backed up", n(t)),
                (Some(t), Some(r)) => format!("Recent and starred chats backed up. {} of {} older chats left; Recall reads them while the computer is idle or at night.", n(r), n(t)),
                _ if c.last_sync.is_some() => "Recent and starred chats backed up. Older chats follow while the computer is idle or at night.".to_string(),
                _ => "Connected".to_string(),
            };
            if paused(c) {
                let until = c.paused_until.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string()).unwrap_or_default();
                s.push_str(&format!(" {app} asked Recall to slow down; the next backup starts after {until}."));
            } else if let Some(e) = &c.last_error {
                s.push_str(&format!(" Last backup had a problem: {e}"));
            }
            s
        }
    }
}

/// Persisted under settings key `connections`; mirrored here for quick reads.
/// `accepted` holds chats handed to the ingest queue this session that may not
/// be indexed yet, so a following backup does not read them again.
pub struct Connections(pub Mutex<HashMap<String, Connection>>, Mutex<HashMap<String, HashMap<String, String>>>);

impl Connections {
    pub fn load(engine: &Engine) -> Self {
        let mut map: HashMap<String, Connection> = db::setting_get(&engine.db, "connections").ok().flatten().unwrap_or_default();
        for c in map.values_mut() {
            // a backup or sign-in that was running when Recall closed is over
            if c.state == "syncing" || c.state == "signing_in" {
                c.state = if c.last_sync.is_some() { "connected".into() } else { "never".into() };
            }
            c.pass = None;
            c.stage = None;
        }
        Self(Mutex::new(map), Mutex::new(HashMap::new()))
    }
    fn update(&self, engine: &Engine, platform: &str, f: impl FnOnce(&mut Connection)) {
        let mut m = self.0.lock();
        let c = m.entry(platform.to_string()).or_default();
        f(c);
        let _ = db::setting_set(&engine.db, "connections", &*m);
    }
    fn get(&self, platform: &str) -> Connection {
        self.0.lock().get(platform).cloned().unwrap_or_default()
    }
    pub fn snapshot(&self) -> HashMap<String, Connection> {
        self.0.lock().clone()
    }
    pub fn lines(&self) -> HashMap<String, String> {
        let m = self.0.lock();
        PLATFORMS.iter().map(|p| (p.to_string(), status_line(p, &m.get(*p).cloned().unwrap_or_default()))).collect()
    }
}

fn valid(platform: &str) -> Result<(), String> {
    if PLATFORMS.contains(&platform) { Ok(()) } else { Err(format!("unknown platform {platform}")) }
}

/// Creates the platform's window if it does not exist. Hidden unless `show`.
fn ensure_window(app: &AppHandle, platform: &str, show: bool) -> Result<tauri::WebviewWindow, String> {
    if let Some(w) = app.get_webview_window(&label(&platform)) {
        if show { let _ = w.show(); let _ = w.set_focus(); }
        return Ok(w);
    }
    let url: tauri::Url = start_url(platform).parse().map_err(|e| format!("{e}"))?;
    let p = platform.to_string();
    info!("{platform}: opening sign-in window (visible={show})");
    WebviewWindowBuilder::new(app, label(platform), WebviewUrl::External(url))
        .title(title(platform))
        .inner_size(1040.0, 780.0)
        .min_inner_size(720.0, 560.0)
        .visible(show)
        .initialization_script(READER)
        // origin and path only: query strings can carry sign-in tokens
        .on_page_load(move |_w, e| debug!("{p}: page {:?} {}{}", e.event(), e.url().origin().ascii_serialization(), e.url().path()))
        .build()
        .map_err(|e| { warn!("{platform}: sign-in window failed: {e}"); e.to_string() })
}

// ---------- commands called from the main window ----------

/// Opens the sign-in window for a platform. If a hidden window already exists it
/// is shown, so a signed-out session can be signed back in.
#[tauri::command]
pub async fn connect_platform(app: AppHandle, engine: tauri::State<'_, Engine>, conns: tauri::State<'_, Connections>, platform: String) -> Result<(), String> {
    valid(&platform)?;
    info!("{platform}: connect");
    conns.update(&engine, &platform, |c| { c.state = "signing_in".into(); c.last_error = None; });
    let w = ensure_window(&app, &platform, true)?;
    let _ = w.eval("window.__recall && window.__recall.check()");
    Ok(())
}

/// Runs a quick backup now in the platform's window, creating it hidden if needed.
#[tauri::command]
pub async fn sync_platform(app: AppHandle, engine: tauri::State<'_, Engine>, conns: tauri::State<'_, Connections>, platform: String) -> Result<(), String> {
    valid(&platform)?;
    conns.update(&engine, &platform, |c| c.paused_until = None);
    let w = ensure_window(&app, &platform, false)?;
    w.eval("window.__recall && window.__recall.quick()").map_err(|e| e.to_string())
}

/// Closes the window, clears its cookies, and forgets the connection. Indexed
/// conversations stay until hidden or the index is rebuilt.
#[tauri::command]
pub async fn disconnect_platform(app: AppHandle, engine: tauri::State<'_, Engine>, conns: tauri::State<'_, Connections>, platform: String) -> Result<(), String> {
    valid(&platform)?;
    if let Some(w) = app.get_webview_window(&label(&platform)) {
        let _ = w.clear_all_browsing_data();
        let _ = w.close();
    }
    conns.update(&engine, &platform, |c| *c = Connection { state: "never".into(), ..Default::default() });
    Ok(())
}

// ---------- commands called from inside a sign-in window ----------

/// Kept for the browser extension's shape; the sign-in windows use `connect_known`.
#[tauri::command]
pub async fn connect_cursor(engine: tauri::State<'_, Engine>, platform: String) -> Result<Option<String>, String> {
    valid(&platform)?;
    db::get_cursor(&engine.db, &platform).map_err(|e| e.to_string())
}

/// Which chats Recall already holds for a platform: id -> updated_at.
#[tauri::command]
pub async fn connect_known(engine: tauri::State<'_, Engine>, conns: tauri::State<'_, Connections>, platform: String) -> Result<HashMap<String, String>, String> {
    valid(&platform)?;
    let mut known = db::chat_versions(&engine.db, &platform).map_err(|e| e.to_string())?;
    if let Some(queued) = conns.1.lock().get(&platform) {
        for (id, at) in queued {
            known.entry(id.clone()).or_insert_with(|| at.clone());
        }
    }
    debug!("{platform}: backup asks what is held ({} chats)", known.len());
    Ok(known)
}

#[tauri::command]
pub async fn connect_ingest(engine: tauri::State<'_, Engine>, conns: tauri::State<'_, Connections>, platform: String, conversations: Vec<ChatPayload>, cursor: Option<String>) -> Result<usize, String> {
    valid(&platform)?;
    let held = db::chat_versions(&engine.db, &platform).unwrap_or_default();
    let mut new = 0;
    {
        let mut queued = conns.1.lock();
        let q = queued.entry(platform.clone()).or_default();
        for c in &conversations {
            if !held.contains_key(&c.id) && !q.contains_key(&c.id) { new += 1; }
            q.insert(c.id.clone(), c.updated_at.clone());
        }
    }
    // keep "N left" current between full listings
    conns.update(&engine, &platform, |c| { if let Some(left) = c.remaining.as_mut() { *left = left.saturating_sub(new); } });
    info!("{platform}: received {} chats ({new} new)", conversations.len());
    Ok(bridge::accept(&engine.db, &engine.ingest.tx, &engine.ingest.progress, &platform, conversations, cursor.as_deref()))
}

#[tauri::command]
pub async fn connect_progress(conns: tauri::State<'_, Connections>, platform: String, pass: String, stage: String, done: usize, total: usize) -> Result<(), String> {
    valid(&platform)?;
    // kept in memory only; the state is saved with the next report
    let mut m = conns.0.lock();
    let c = m.entry(platform).or_default();
    c.state = "syncing".into();
    c.pass = Some(pass);
    c.stage = Some(stage);
    c.done = done;
    c.todo = total;
    Ok(())
}

/// The window reports whether the session is signed in and how a backup went.
/// Signed in: hide the window and keep it for the next backup; if the person just
/// signed in, bring the main window forward so they see what happens next.
/// Signed out: leave it visible so the person can sign in.
#[tauri::command]
pub async fn connect_report(
    app: AppHandle,
    engine: tauri::State<'_, Engine>,
    conns: tauri::State<'_, Connections>,
    platform: String,
    signed_in: bool,
    synced: usize,
    error: Option<String>,
    pass: Option<String>,
    total: Option<usize>,
    remaining: Option<usize>,
) -> Result<(), String> {
    valid(&platform)?;
    info!("{platform}: report signed_in={signed_in} pass={pass:?} synced={synced} total={total:?} remaining={remaining:?} error={error:?}");
    let w = app.get_webview_window(&label(&platform));
    if signed_in {
        let was = conns.get(&platform).state;
        if let Some(w) = &w { let _ = w.hide(); }
        if was == "signing_in" {
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.show();
                let _ = main.unminimize();
                let _ = main.set_focus();
            }
            let _ = app.emit_to("main", "connect-signed-in", &platform);
        }
        conns.update(&engine, &platform, |c| {
            if c.state != "syncing" || pass.is_some() { c.state = "connected".into(); }
            if pass.is_some() {
                c.pass = None;
                c.stage = None;
                c.last_error = error.clone();
                c.last_sync = Some(chrono::Utc::now().to_rfc3339());
                c.synced = synced;
                if total.is_some() { c.total = total; }
                if remaining.is_some() { c.remaining = remaining; }
                if pass.as_deref() == Some("quick") { c.last_quick = c.last_sync.clone(); }
                if pass.as_deref() == Some("full") && remaining == Some(0) {
                    c.full_done_on = Some(chrono::Local::now().format("%Y-%m-%d").to_string());
                }
                if error.as_deref().is_some_and(|e| e.contains("429")) {
                    c.paused_until = Some((chrono::Utc::now() + PAUSE_AFTER_REFUSAL).to_rfc3339());
                }
            }
        });
    } else {
        conns.update(&engine, &platform, |c| {
            // still waiting for the person to sign in: keep saying so
            c.state = match c.state.as_str() { "never" => "never", "signing_in" => "signing_in", _ => "signed_out" }.into();
            c.pass = None;
            c.stage = None;
            c.last_error = error.clone();
        });
        if let Some(w) = &w {
            // only pop the window if the person asked to connect; a background
            // backup that finds itself signed out stays quiet and Settings says so
            if conns.get(&platform).state == "signing_in" { let _ = w.show(); }
        }
    }
    Ok(())
}

// ---------- scheduling ----------

/// Seconds since the last keyboard or mouse input anywhere on the computer.
#[cfg(windows)]
fn idle_secs() -> u64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: info is a valid LASTINPUTINFO with cbSize set
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return 0;
    }
    (unsafe { GetTickCount() }.wrapping_sub(info.dwTime) / 1000) as u64
}

#[cfg(not(windows))]
fn idle_secs() -> u64 {
    0
}

fn night() -> bool {
    use chrono::Timelike;
    NIGHT.contains(&chrono::Local::now().hour())
}

fn needs_full(c: &Connection) -> bool {
    match c.remaining {
        None => true,
        Some(r) if r > 0 => true,
        // all read: re-check once a night for older chats that changed
        Some(_) => night() && c.full_done_on.as_deref() != Some(chrono::Local::now().format("%Y-%m-%d").to_string().as_str()),
    }
}

/// Every thirty seconds: a quick backup for each connected platform when one is
/// due, and a full-backup run when the computer is idle or it is night. Runs on
/// its own thread: creating a window from the main thread itself would wait on
/// the main thread and hang the app on Windows.
pub fn start_scheduler(app: AppHandle) {
    std::thread::Builder::new()
        .name("connect-sync".into())
        .spawn(move || loop {
            std::thread::sleep(TICK);
            let conns = app.state::<Connections>();
            let engine = app.state::<Engine>();
            let enabled: HashMap<String, bool> = db::setting_get(&engine.db, "platform_enabled").ok().flatten().unwrap_or_default();
            for p in PLATFORMS {
                let c = conns.get(p);
                if !matches!(c.state.as_str(), "connected" | "syncing") || c.pass.is_some() || paused(&c) || enabled.get(p).copied() == Some(false) {
                    continue;
                }
                let quick_due = c.last_quick.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                    .map_or(true, |t| chrono::Utc::now().signed_duration_since(t).to_std().unwrap_or_default() >= QUICK_EVERY);
                let script = if quick_due {
                    "window.__recall && window.__recall.quick()".to_string()
                } else if needs_full(&c) && (idle_secs() >= IDLE_FOR_FULL || night()) {
                    format!("window.__recall && window.__recall.full({FULL_RUN})")
                } else {
                    continue;
                };
                if let Ok(w) = ensure_window(&app, p, false) {
                    let _ = w.eval(&script);
                }
            }
        })
        .expect("spawn connect scheduler");
}
