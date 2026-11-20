//! benchlens HTTP server: the JSON API of `docs/api.md` and the web UI in `web/`, built into the binary.
//!
//! Read endpoints live in [`read`], tag editing and delete in [`write`], upload in [`upload`], the static HTML
//! export in [`export`]. Who may read and write is decided in [`auth`] (every write handler takes a
//! [`write::WriteAccess`]; reads, the export included, go through [`auth::read_guard`]); settings come from
//! [`config`]. Keep the routes in [`router`] in step with docs/api.md.

pub mod auth;
pub mod config;
pub mod export;
mod params;
pub mod read;
pub mod upload;
pub mod write;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use benchlens_core::store::{Store, DB_VERSION};
use rust_embed::RustEmbed;
use serde::Serialize;
use std::sync::{Arc, Mutex};

/// The web UI, compiled into the binary. `web/dev/` (development tools) and the READMEs are left out.
#[derive(RustEmbed)]
#[folder = "../../web/"]
#[exclude = "dev/*"]
#[exclude = "*.md"]
struct Web;

/// How to run the server: see [`config::Settings`] (flags, environment, `benchlens.toml`).
pub use config::Settings as ServeConfig;

/// Shared state of every handler. rusqlite connections are not `Sync`, so the store sits behind a mutex;
/// handlers should do their database work in `spawn_blocking`.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Mutex<Store>>,
    /// Who may read and write.
    pub access: Arc<auth::Access>,
    /// Largest file accepted by `POST /api/runs`, in bytes.
    pub max_upload_bytes: u64,
    /// Registry TSV applied to uploads (form tags win over it), read at every upload.
    pub registry: Option<std::path::PathBuf>,
}

impl AppState {
    /// Local use: no token, reads and writes open, the default upload limit.
    pub fn new(store: Store) -> AppState {
        AppState {
            store: Arc::new(Mutex::new(store)),
            access: Arc::new(auth::Access::local()),
            max_upload_bytes: config::DEFAULT_MAX_UPLOAD_MB * 1024 * 1024,
            registry: None,
        }
    }
    pub fn with_registry(mut self, registry: Option<std::path::PathBuf>) -> AppState {
        self.registry = registry;
        self
    }
    pub fn with_access(mut self, access: auth::Access) -> AppState {
        self.access = Arc::new(access);
        self
    }
    pub fn with_max_upload_bytes(mut self, bytes: u64) -> AppState {
        self.max_upload_bytes = bytes;
        self
    }
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
            E::Contract(_) | E::UnsafeArchive { .. } | E::Ambiguous(_) | E::BadReference(_) => {
                StatusCode::BAD_REQUEST
            }
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
    let upload_limit = usize::try_from(state.max_upload_bytes)
        .unwrap_or(usize::MAX)
        .saturating_add(upload::MULTIPART_OVERHEAD);
    let api = Router::new()
        .route(
            "/runs",
            get(read::list_runs)
                .merge(post(upload::upload_run).layer(DefaultBodyLimit::max(upload_limit))),
        )
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
        .route("/export.html", get(export::export_html))
        // everything above needs the token when read_public = false; health and unknown paths do not
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::read_guard,
        ))
        .route("/health", get(health))
        .fallback(api_not_found);
    Router::new()
        .nest("/api", api)
        .fallback(static_file)
        // the Host check (DNS rebinding) covers every path, the static files included
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::host_guard,
        ))
        .with_state(state)
}

/// Open the database and serve until Ctrl-C or SIGTERM. The token is never printed.
pub async fn serve(cfg: ServeConfig) -> anyhow::Result<()> {
    let store = Store::open(&cfg.db)?;
    let access = auth::Access::for_listen(cfg.token.clone(), cfg.read_public, cfg.listen)
        .with_allowed_hosts(cfg.allowed_hosts.clone());
    let access_hosts = access.hosts_served();
    let state = AppState::new(store)
        .with_access(access)
        .with_max_upload_bytes(cfg.max_upload_bytes)
        .with_registry(cfg.registry.clone());
    let listener = tokio::net::TcpListener::bind(cfg.listen).await?;
    let inbox = start_inbox(&cfg)?;
    eprintln!(
        "benchlens {} on http://{} (database {})",
        env!("CARGO_PKG_VERSION"),
        listener.local_addr()?,
        cfg.db.display()
    );
    let reads = if !cfg.read_public {
        "reads need the token"
    } else {
        "reads open"
    };
    let writes = if cfg.token.is_some() {
        "writes need the token"
    } else if config::is_loopback(cfg.listen.ip()) {
        "writes open (loopback, no token)"
    } else {
        "writes refused (no token)"
    };
    eprintln!(
        "{reads}; {writes}; uploads up to {} MiB",
        cfg.max_upload_bytes / (1024 * 1024)
    );
    eprintln!("Host names served: {access_hosts}");
    if let Some(r) = &cfg.registry {
        eprintln!("registry {} (inbox imports and uploads)", r.display());
    }
    if let Some(w) = cfg.warning() {
        eprintln!("{w}");
    }
    let served = axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await;
    eprintln!("benchlens stopping");
    if let Some((stop, thread)) = inbox {
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = tokio::task::spawn_blocking(move || thread.join()).await;
    }
    served?;
    Ok(())
}

/// Resolves on Ctrl-C, or on SIGTERM on unix (what `docker stop` and systemd send), so the server stops
/// gracefully either way; the inbox thread is stopped after it.
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

type InboxThread = (
    Arc<std::sync::atomic::AtomicBool>,
    std::thread::JoinHandle<()>,
);

/// Get an inbox ready to watch, the same way for `benchlens watch` and `serve --inbox`: refuse a registry that
/// does not parse now (later edits are checked on every scan), create the folder, take its lock (warning when
/// a stale lock of a watcher that did not stop cleanly is taken over). Hold the lock while watching.
pub fn prepare_inbox(
    opts: &benchlens_core::inbox::InboxOptions,
) -> anyhow::Result<benchlens_core::inbox::InboxLock> {
    use anyhow::Context;
    use benchlens_core::import::Registry;
    if let Some(p) = &opts.registry {
        let text =
            std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
        Registry::parse(&text).with_context(|| format!("registry {}", p.display()))?;
    }
    std::fs::create_dir_all(&opts.dir)
        .with_context(|| format!("creating inbox {}", opts.dir.display()))?;
    let lock = benchlens_core::inbox::InboxLock::acquire(&opts.dir)?;
    if let Some(old) = lock.taken_over() {
        eprintln!(
            "warning: took over the stale inbox lock of {old} ({})",
            lock.path().display()
        );
    }
    Ok(lock)
}

/// Run the inbox watcher of `cfg.inbox` in its own thread with its own connection to the database. Fails
/// when another watcher holds the inbox lock or the registry does not parse.
fn start_inbox(cfg: &ServeConfig) -> anyhow::Result<Option<InboxThread>> {
    use benchlens_core::inbox::{watch, InboxOptions};
    let Some(dir) = cfg.inbox.clone() else {
        return Ok(None);
    };
    let store = Store::open(&cfg.db)?;
    let opts = InboxOptions {
        interval: cfg.inbox_interval,
        tags: cfg.inbox_tags.clone(),
        registry: cfg.registry.clone(),
        ..InboxOptions::new(dir)
    };
    let lock = prepare_inbox(&opts)?;
    eprintln!(
        "watching inbox {} every {}s",
        opts.dir.display(),
        opts.interval.as_secs_f64()
    );
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::Builder::new()
        .name("benchlens-inbox".into())
        .spawn(move || {
            watch(&store, &opts, &flag, |ev| eprintln!("inbox: {ev}"));
            drop(lock);
        })?;
    Ok(Some((stop, thread)))
}
