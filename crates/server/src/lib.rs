//! benchlens HTTP server: the JSON API of `docs/api.md` and the web UI in `web/`, built into the binary.
//!
//! T0 skeleton: `/api/health` and the static files work; every other endpoint answers 501 until task A
//! (read endpoints) and C2 (upload, auth) fill them in. Keep the routes in [`router`] in step with docs/api.md.

use axum::extract::State;
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
    let store = st.store.clone();
    let runs = tokio::task::spawn_blocking(move || store.lock().expect("store lock").run_count())
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))??;
    Ok(Json(Health {
        benchlens: env!("CARGO_PKG_VERSION"),
        db_layout: DB_VERSION,
        runs,
    }))
}

macro_rules! todo_endpoint {
    ($name:ident, $what:literal) => {
        async fn $name() -> ApiError {
            ApiError::not_implemented($what)
        }
    };
}
todo_endpoint!(list_runs, "GET /api/runs (task A)");
todo_endpoint!(get_run, "GET /api/runs/{id} (task A)");
todo_endpoint!(get_run_file, "GET /api/runs/{id}/files/{name} (task A)");
todo_endpoint!(patch_tags, "PATCH /api/runs/{id}/tags (task A, auth in C2)");
todo_endpoint!(delete_run, "DELETE /api/runs/{id} (task A, auth in C2)");
todo_endpoint!(upload_run, "POST /api/runs (task C2)");
todo_endpoint!(list_groups, "GET /api/groups (task A)");
todo_endpoint!(compare, "GET /api/compare (task A)");
todo_endpoint!(values, "GET /api/values (task A)");
todo_endpoint!(metrics, "GET /api/metrics (task A)");

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
        .route("/runs", get(list_runs).post(upload_run))
        .route("/runs/{id}", get(get_run).delete(delete_run))
        .route("/runs/{id}/files/{name}", get(get_run_file))
        .route("/runs/{id}/tags", patch(patch_tags))
        .route("/groups", get(list_groups))
        .route("/compare", get(compare))
        .route("/values", get(values))
        .route("/metrics", get(metrics))
        .fallback(api_not_found);
    Router::new()
        .nest("/api", api)
        .fallback(static_file)
        .with_state(state)
}

/// Open the database and serve until Ctrl-C.
pub async fn serve(cfg: ServeConfig) -> anyhow::Result<()> {
    let store = Store::open(&cfg.db)?;
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
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
