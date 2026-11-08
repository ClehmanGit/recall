//! Localhost bridge. The browser extension POSTs conversations here. Bound to
//! 127.0.0.1 only, and every request must carry the per-install token the app
//! shows in Settings, so another local process can't feed the index.

use crate::db::{self, Db};
use crate::ingest::{ChatPayload, Job, Progress};
use std::sync::atomic::Ordering;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

pub const PORT: u16 = 47617;

#[derive(Clone)]
struct AppState {
    db: Db,
    tx: Sender<Job>,
    token: Arc<String>,
    progress: Progress,
}

#[derive(Deserialize)]
struct Batch {
    conversations: Vec<ChatPayload>,
    /// the newest updated_at the extension saw for this platform; stored as the sync cursor
    cursor: Option<String>,
    platform: String,
}

#[derive(Serialize)]
struct Ack {
    accepted: usize,
    cursor: Option<String>,
}

pub fn spawn(db: Db, tx: Sender<Job>, token: String, progress: Progress) {
    let state = AppState { db, tx, token: Arc::new(token), progress };
    std::thread::Builder::new()
        .name("bridge".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("tokio");
            rt.block_on(async move {
                let app = Router::new()
                    .route("/health", get(health))
                    .route("/cursor/:platform", get(cursor))
                    .route("/ingest", post(ingest))
                    .with_state(state);
                let addr = std::net::SocketAddr::from(([127, 0, 0, 1], PORT));
                let listener = tokio::net::TcpListener::bind(addr).await.expect("bind bridge");
                info!("bridge listening on {addr}");
                axum::serve(listener, app).await.expect("serve bridge");
            });
        })
        .expect("spawn bridge");
}

fn authed(state: &AppState, headers: &HeaderMap) -> bool {
    headers
        .get("x-recall-token")
        .and_then(|v| v.to_str().ok())
        .map(|t| t == state.token.as_str())
        .unwrap_or(false)
}

async fn health() -> &'static str {
    "ok"
}

async fn cursor(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(platform): axum::extract::Path<String>,
) -> Result<Json<Option<String>>, StatusCode> {
    if !authed(&state, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    db::get_cursor(&state.db, &platform).map(Json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// Queues a batch of conversations for one platform and advances its cursor.
/// Shared by the HTTP bridge (extension) and the in-app sign-in windows.
pub fn accept(db: &Db, tx: &Sender<Job>, progress: &Progress, platform: &str, conversations: Vec<ChatPayload>, cursor: Option<&str>) -> usize {
    // a platform switched off in settings is acknowledged and dropped, so the
    // sender keeps its cursor and nothing is indexed
    let enabled: std::collections::HashMap<String, bool> =
        db::setting_get(db, "platform_enabled").ok().flatten().unwrap_or_default();
    if enabled.get(platform).copied() == Some(false) {
        return 0;
    }
    let n = conversations.len();
    progress.queued.fetch_add(n, Ordering::Relaxed);
    for c in conversations {
        let _ = tx.send(Job::Chat(c));
    }
    if let Some(cur) = cursor {
        let _ = db::set_cursor(db, platform, cur);
    }
    let _ = tx.send(Job::Save);
    n
}

async fn ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(batch): Json<Batch>,
) -> Result<Json<Ack>, StatusCode> {
    if !authed(&state, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let n = accept(&state.db, &state.tx, &state.progress, &batch.platform, batch.conversations, batch.cursor.as_deref());
    Ok(Json(Ack { accepted: n, cursor: batch.cursor }))
}
