//! benchlens HTTP server: the JSON API of `docs/api.md` and the web UI in `web/`, built into the binary.
//!
//! Read endpoints live in [`read`], tag editing and delete in [`write`] (their token check goes in
//! [`write::WriteAccess`]). Upload (`POST /api/runs`) still answers 501 until task C2. Keep the routes in
//! [`router`] in step with docs/api.md.

mod params;
pub mod read;
pub mod write;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use axum::{Json, Router};
use benchlens_core::store::{Store, DB_VERSION};
use rust_embed::RustEmbed;
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// The web UI, compiled into the binary.
#[derive(RustEmbed)]
#[folder = "../../web/"]
struct Web;

/// How to run the server.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub db: PathBuf,
    pub listen: SocketAddr,
    /// Inbox folder to watch and import from (task C1); `None` leaves the inbox off.
    pub inbox: Option<PathBuf>,
    /// Time between two inbox scans.
    pub inbox_interval: std::time::Duration,
}

/// Shared state of every handler. rusqlite connections are not `Sync`, so the store sits behind a mutex;
/// handlers should do their database work in `spawn_blocking`.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Mutex<Store>>,
}

/// The JSON error body of every failed request: `{"error": "...", "detail": "..."}`.
#[derive(Debug, Serialize)]
pub struct ApiError {
    #[serde(skip)]
    pub status: StatusCode,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ApiError {
    pub fn new(status: StatusCode, error: impl Into<String>) -> Self {
        ApiError {
            status,
            error: error.into(),
            detail: None,
        }
    }
    pub fn internal(error: impl Into<String>) -> Self {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error)
    }
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
    pub fn not_implemented(what: &str) -> Self {
        ApiError {
            status: StatusCode::NOT_IMPLEMENTED,
            error: "not implemented yet".into(),
            detail: Some(what.into()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(&self)).into_response()
    }
}

impl From<benchlens_core::Error> for ApiError {
    fn from(e: benchlens_core::Error) -> Self {
        use benchlens_core::Error as E;
        let status = match &e {
            E::NotFound(_) => StatusCode::NOT_FOUND,
            E::Contract(_) | E::UnsafeArchive { .. } => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError::new(status, e.to_string())
    }
}

#[derive(Serialize)]
pub struct Health {
    pub benchlens: &'static str,
    pub db_layout: i64,
    pub runs: i64,
}

async fn health(State(st): State<AppState>) -> Result<Json<Health>, ApiError> {
    let runs = params::db(&st, |s| Ok(s.run_count()?)).await?;
    Ok(Json(Health {
        benchlens: env!("CARGO_PKG_VERSION"),
        db_layout: DB_VERSION,
        runs,
    }))
}

/// `POST /api/runs`: task C2 builds it in upload.rs (and takes a [`write::WriteAccess`] like the other writes).
async fn upload_run() -> ApiError {
    ApiError::not_implemented("POST /api/runs (task C2)")
}

async fn api_not_found(uri: Uri) -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, format!("no endpoint {}", uri.path()))
}

/// Serve a file of the embedded web UI; unknown paths get index.html so links into the UI work.
async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Web::get(path).or_else(|| Web::get("index.html")) {
        Some(file) => {
            let mime = file.metadata.mimetype().to_string();
            ([(header::CONTENT_TYPE, mime)], file.data.into_owned()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "web UI not built into this binary").into_response(),
    }
}

/// Every route of API v1 (docs/api.md) plus the web UI.
pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/runs", get(read::list_runs).post(upload_run))
        .route("/runs/{id}", get(read::get_run).delete(write::delete_run))
        .route("/runs/{id}/files/{name}", get(read::get_run_file))
        .route(
            "/runs/{id}/tags",
            patch(write::patch_tags).layer(DefaultBodyLimit::max(write::MAX_TAGS_BODY)),
        )
        .route("/groups", get(read::list_groups))
        .route("/compare", get(read::compare))
        .route("/values", get(read::values))
        .route("/metrics", get(read::metrics))
        .fallback(api_not_found);
    Router::new()
        .nest("/api", api)
        .fallback(static_file)
        .with_state(state)
}

/// Open the database and serve until Ctrl-C.
pub async fn serve(cfg: ServeConfig) -> anyhow::Result<()> {
    let store = Store::open(&cfg.db)?;
    let inbox = start_inbox(&cfg)?;
    let state = AppState {
        store: Arc::new(Mutex::new(store)),
    };
    let listener = tokio::net::TcpListener::bind(cfg.listen).await?;
    eprintln!(
        "benchlens {} on http://{} (database {})",
        env!("CARGO_PKG_VERSION"),
        listener.local_addr()?,
        cfg.db.display()
    );
    let served = axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    if let Some((stop, thread)) = inbox {
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = tokio::task::spawn_blocking(move || thread.join()).await;
    }
    served?;
    Ok(())
}

type InboxThread = (
    Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
);

/// Run the inbox watcher of `cfg.inbox` in its own thread with its own connection to the database.
fn start_inbox(cfg: &ServeConfig) -> anyhow::Result<Option<InboxThread>> {
    use benchlens_core::inbox::{watch, InboxOptions};
    let Some(dir) = cfg.inbox.clone() else {
        return Ok(None);
    };
    let store = Store::open(&cfg.db)?;
    let opts = InboxOptions {
        dir,
        interval: cfg.inbox_interval,
    };
    eprintln!(
        "watching inbox {} every {}s",
        opts.dir.display(),
        opts.interval.as_secs_f64()
    );
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("benchlens-inbox".into())
        .spawn(move || watch(&store, &opts, &flag, |ev| eprintln!("inbox: {ev}")))?;
    Ok(Some((stop, thread)))
}
