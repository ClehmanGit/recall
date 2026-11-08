//! Opening an item in its own program, or showing it in its folder.
//!
//! The window never hands the system a path or a link. It names an item by id,
//! and the app opens it only when the item is a supported file inside a folder
//! chosen in Settings, or a chat on one of the chat sites. Code and script files
//! open for editing, never to run: on Windows, opening a .js or .py file can run it.

use recall_core::{db, extract, Engine};
use std::path::{Path, PathBuf};
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::real_path;

const CHAT_HOSTS: &[&str] = &["claude.ai", "chatgpt.com", "gemini.google.com"];

enum Target {
    File { path: PathBuf, kind: String },
    Chat(tauri::Url),
}

fn target(app: &Engine, item_id: i64) -> Result<Target, String> {
    let it = db::get_item(&app.db, item_id)
        .map_err(|e| e.to_string())?
        .ok_or("That item is no longer in Recall.")?;
    if let Some(rest) = it.uri.strip_prefix("file:///") {
        let path = real_path(rest);
        if !path.is_file() {
            return Err("That file is no longer there.".into());
        }
        if !extract::supported(&path) || !in_chosen_folder(app, &path) {
            return Err("Recall only opens files from the folders it searches.".into());
        }
        return Ok(Target::File { path, kind: it.kind });
    }
    let url: tauri::Url = it.uri.parse().map_err(|_| "That link is not valid.")?;
    let host_ok = url.host_str().is_some_and(|h| CHAT_HOSTS.iter().any(|c| h == *c || h.ends_with(&format!(".{c}"))));
    if url.scheme() == "https" && host_ok {
        Ok(Target::Chat(url))
    } else {
        Err("Recall only opens links to Claude, ChatGPT and Gemini.".into())
    }
}

fn in_chosen_folder(app: &Engine, path: &Path) -> bool {
    db::list_sources(&app.db)
        .unwrap_or_default()
        .iter()
        .filter(|s| s.enabled)
        .any(|s| path.starts_with(real_path(&s.path)))
}

/// A page number or a message id: letters, digits, `-`, `_` and `:` only.
fn clean_anchor(anchor: Option<String>) -> Option<String> {
    anchor.filter(|a| !a.is_empty() && a.len() <= 128 && a.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')))
}

#[tauri::command]
pub async fn open_item(handle: tauri::AppHandle, app: State<'_, Engine>, item_id: i64, anchor: Option<String>) -> Result<(), String> {
    let anchor = clean_anchor(anchor);
    match target(&app, item_id)? {
        Target::Chat(mut url) => {
            if anchor.is_some() {
                url.set_fragment(anchor.as_deref());
            }
            handle.opener().open_url(url.as_str(), None::<&str>).map_err(|e| e.to_string())
        }
        Target::File { path, kind } if kind == "code" => edit(&path),
        Target::File { path, kind } => {
            // a PDF opens at the matching page where the reader supports it
            if kind == "pdf" {
                if let Some(page) = anchor.filter(|a| a.chars().all(|c| c.is_ascii_digit())) {
                    if let Ok(mut url) = tauri::Url::from_file_path(dunce(&path)) {
                        url.set_fragment(Some(&format!("page={page}")));
                        return handle.opener().open_url(url.as_str(), None::<&str>).map_err(|e| e.to_string());
                    }
                }
            }
            handle.opener().open_path(dunce(&path).to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
        }
    }
}

#[tauri::command]
pub async fn reveal_item(handle: tauri::AppHandle, app: State<'_, Engine>, item_id: i64) -> Result<(), String> {
    match target(&app, item_id)? {
        Target::File { path, .. } => handle.opener().reveal_item_in_dir(dunce(&path)).map_err(|e| e.to_string()),
        Target::Chat(_) => Err("A chat has no folder to show.".into()),
    }
}

/// `C:\x` rather than `\\?\C:\x`, which some programs cannot open.
fn dunce(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// Opens with the file type's Edit action (Notepad for scripts, unless the
/// person chose an editor), and Notepad when the type has none.
#[cfg(windows)]
fn edit(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let path = dunce(path);
    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let verb = wide("edit".as_ref());
    let file = wide(path.as_os_str());
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call
    let code = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) } as isize;
    if code > 32 {
        return Ok(());
    }
    std::process::Command::new("notepad.exe").arg(&path).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn edit(path: &Path) -> Result<(), String> {
    Err(format!("Opening {} for editing is only built for Windows.", path.display()))
}
