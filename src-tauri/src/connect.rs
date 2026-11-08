//! In-app sign-in for chat apps. Each platform gets its own webview window that
//! shows the site's own sign-in page. Once signed in, the window is hidden and
//! kept for backups; `connect/reader.js` runs inside it and hands conversations
//! to the commands below. Cookies live in the app's WebView2 profile on this
//! machine. The window opens on the platform's own site and goes only to
//! https pages (sign-in can pass through Google, Microsoft or Apple); the
//! reader's commands work only on the chat sites themselves (capabilities/connect.json).
//!
//! Two kinds of backup, both run by the reader:
//! - quick: starred chats and the most recent ones, on sign-in and every fifteen
//!   minutes, so new chats are searchable within seconds;
//! - full: everything else, slowly, only while the computer is idle or at night.
//! Both read only chats Recall does not hold yet or that changed (`connect_known`).

use parking_lot::Mutex;
use recall_core::{db, ingest::{ChatPayload, Job}, Engine};
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

/// The reader's commands answer only the platform's own window, so a page on
/// one chat site cannot read or add another site's chats.
fn own(window: &tauri::WebviewWindow, platform: &str) -> Result<(), String> {
    valid(platform)?;
    if window.label() == label(platform) { Ok(()) } else { Err(format!("{} may not act for {platform}", window.label())) }
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
        // nothing but https: no file:, no custom schemes, no plain http
        .on_navigation(|url| url.scheme() == "https" || url.as_str() == "about:blank")
        // a hidden window must never save files to the computer
        .on_download(|_, _| false)
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
    sign_out(&app, &platform);
    if let Some(w) = app.get_webview_window(&label(&platform)) {
        let _ = w.close();
    }
    conns.update(&engine, &platform, |c| *c = Connection { state: "never".into(), ..Default::default() });
    Ok(())
}

/// Deletes the platform's sign-in cookies, and only its own: the windows share
/// one browser profile, so clearing everything would sign out the other apps
/// too. Works whether or not the platform's window is open.
fn sign_out(app: &AppHandle, platform: &str) {
    let domains: &[&str] = match platform {
        "claude" => &["claude.ai", "anthropic.com"],
        "gemini" => &["google.com"],
        _ => &["chatgpt.com", "openai.com"],
    };
    let Some(w) = app.get_webview_window(&label(platform)).or_else(|| app.get_webview_window("main")) else { return };
    match w.cookies() {
        Ok(all) => {
            let mut n = 0;
            for c in all {
                let d = c.domain().unwrap_or("").trim_start_matches('.').to_string();
                if domains.iter().any(|x| d == *x || d.ends_with(&format!(".{x}"))) {
                    if w.delete_cookie(c).is_ok() { n += 1; }
                }
            }
            info!("{platform}: signed out ({n} cookies removed)");
        }
        Err(e) => warn!("{platform}: could not read cookies to sign out: {e}"),
    }
}

// ---------- commands called from inside a sign-in window ----------


/// Which chats Recall already holds for a platform: id -> updated_at.
#[tauri::command]
pub async fn connect_known(window: tauri::WebviewWindow, engine: tauri::State<'_, Engine>, conns: tauri::State<'_, Connections>, platform: String) -> Result<HashMap<String, String>, String> {
    own(&window, &platform)?;
    let mut known = db::chat_versions(&engine.db, &platform).map_err(|e| e.to_string())?;
    if let Some(queued) = conns.1.lock().get(&platform) {
        for (id, at) in queued {
            known.entry(id.clone()).or_insert_with(|| at.clone());
        }
    }
    debug!("{platform}: backup asks what is held ({} chats)", known.len());
    Ok(known)
}

/// Longest title, message and batch the app accepts from a page.
const MAX_TITLE: usize = 300;
const MAX_TEXT: usize = 200_000;
const MAX_MESSAGES: usize = 5_000;
const MAX_BATCH: usize = 500;

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'))
}

fn cut(s: &mut String, max: usize) {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) { end -= 1; }
        s.truncate(end);
    }
}

/// Chats as the page sent them, made safe to store: the platform is the
/// checked one, the link is built here from the id, ids are plain, dates
/// parse, and nothing is unboundedly long. Anything else is dropped.
fn clean(platform: &str, conversations: Vec<ChatPayload>) -> Vec<ChatPayload> {
    let mut out = Vec::new();
    for mut c in conversations.into_iter().take(MAX_BATCH) {
        if !id_ok(&c.id) || (platform != "gemini" && c.id.contains(':')) {
            warn!("{platform}: dropped a chat with an unexpected id");
            continue;
        }
        c.platform = platform.to_string();
        c.url = match platform {
            "claude" => format!("https://claude.ai/chat/{}", c.id),
            "gemini" => format!("https://gemini.google.com/app/{}", c.id.trim_start_matches("c_")),
            _ => format!("https://chatgpt.com/c/{}", c.id),
        };
        c.updated_at = chrono::DateTime::parse_from_rfc3339(&c.updated_at)
            .map(|d| d.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now())
            .to_rfc3339();
        c.title = c.title.chars().filter(|ch| !ch.is_control()).collect();
        cut(&mut c.title, MAX_TITLE);
        c.messages.truncate(MAX_MESSAGES);
        for m in &mut c.messages {
            cut(&mut m.text, MAX_TEXT);
            if m.id.as_deref().is_some_and(|id| !id_ok(id)) { m.id = None; }
        }
        out.push(c);
    }
    out
}

/// A page's error message, short and without ids, for the log and Settings.
fn tidy_error(e: Option<String>) -> Option<String> {
    e.map(|e| {
        let mut words: Vec<String> = e
            .split_whitespace()
            .map(|w| if w.chars().filter(|c| c.is_ascii_hexdigit()).count() >= 16 { "…".to_string() } else { w.to_string() })
            .collect();
        words.dedup();
        let mut s = words.join(" ");
        cut(&mut s, 200);
        s
    })
}

#[tauri::command]
pub async fn connect_ingest(window: tauri::WebviewWindow, engine: tauri::State<'_, Engine>, conns: tauri::State<'_, Connections>, platform: String, conversations: Vec<ChatPayload>) -> Result<usize, String> {
    own(&window, &platform)?;
    // a platform switched off in Settings is acknowledged and dropped
    let enabled: HashMap<String, bool> = db::setting_get(&engine.db, "platform_enabled").ok().flatten().unwrap_or_default();
    if enabled.get(&platform).copied() == Some(false) {
        return Ok(0);
    }
    let conversations = clean(&platform, conversations);
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
    let n = conversations.len();
    engine.ingest.progress.queued.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    for c in conversations {
        let _ = engine.ingest.tx.send(Job::Chat(c));
    }
    let _ = engine.ingest.tx.send(Job::Save);
    Ok(n)
}

#[tauri::command]
pub async fn connect_progress(window: tauri::WebviewWindow, conns: tauri::State<'_, Connections>, platform: String, pass: String, stage: String, done: usize, total: usize) -> Result<(), String> {
    own(&window, &platform)?;
    let (mut pass, mut stage) = (pass, stage);
    cut(&mut pass, 16);
    cut(&mut stage, 32);
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
    window: tauri::WebviewWindow,
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
    own(&window, &platform)?;
    let error = tidy_error(error);
    let pass = pass.map(|mut p| { cut(&mut p, 16); p });
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
