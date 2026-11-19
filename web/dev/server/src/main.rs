//! Development stand-in for the benchlens HTTP API v1 (docs/api.md), so the web UI can be built and checked
//! before the real server (crates/server, task A) answers every endpoint. Every response comes from
//! benchlens-core on a real database; nothing is invented here.
//!
//!   cargo run --manifest-path web/dev/server/Cargo.toml -- --db benchlens.db [--listen 127.0.0.1:8090]
//!
//! Env: BENCHLENS_DEV_TOKEN=secret makes writes need `Authorization: Bearer secret` (to try the 401 prompt);
//! BENCHLENS_DEV_DELAY_MS=800 slows every API answer (to see loading states).

use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use axum::{Json, Router};
use benchlens_core::compare;
use benchlens_core::store::{valid_tag_key, Store, DB_VERSION};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct St {
    store: Arc<Mutex<Store>>,
    web: PathBuf,
    token: Option<String>,
    delay_ms: u64,
}

type Res = Result<Response, Response>;

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

fn core_err(e: benchlens_core::Error) -> Response {
    use benchlens_core::Error as E;
    let status = match &e {
        E::NotFound(_) => StatusCode::NOT_FOUND,
        E::Contract(_) | E::UnsafeArchive { .. } => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    err(status, e.to_string())
}

fn ok<T: serde::Serialize>(v: T) -> Res {
    Ok(Json(v).into_response())
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query(q: &Option<String>) -> Vec<(String, String)> {
    q.as_deref()
        .unwrap_or("")
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (pct_decode(k), pct_decode(v))
        })
        .collect()
}

fn first(q: &[(String, String)], key: &str) -> Option<String> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

async fn pause(st: &St) {
    if st.delay_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(st.delay_ms)).await;
    }
}

fn check_token(st: &St, h: &HeaderMap) -> Result<(), Response> {
    let Some(tok) = &st.token else { return Ok(()) };
    let got = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    if got == Some(tok.as_str()) {
        Ok(())
    } else {
        Err(err(StatusCode::UNAUTHORIZED, "a valid token is needed to change data"))
    }
}

async fn health(State(st): State<St>) -> Res {
    pause(&st).await;
    let s = st.store.lock().unwrap();
    let runs = s.run_count().map_err(core_err)?;
    ok(json!({ "benchlens": "dev", "db_layout": DB_VERSION, "runs": runs }))
}

async fn list_runs(State(st): State<St>, RawQuery(q): RawQuery) -> Res {
    pause(&st).await;
    let q = query(&q);
    let runs = st.store.lock().unwrap().list_runs().map_err(core_err)?;
    let mut out = vec![];
    'run: for r in runs {
        for (k, v) in &q {
            let keep = match k.as_str() {
                "tag" => {
                    let (tk, tv) = v.split_once(':').unwrap_or((v, ""));
                    r.tags.get(tk).map(|x| x == tv).unwrap_or(false)
                }
                "arch" => &r.arch == v,
                "group" => &r.group == v,
                "q" => {
                    let n = v.to_lowercase();
                    [&r.label, &r.folder, &r.group]
                        .iter()
                        .any(|s| s.to_lowercase().contains(&n))
                        || r.tags.values().any(|s| s.to_lowercase().contains(&n))
                }
                _ => true,
            };
            if !keep {
                continue 'run;
            }
        }
        out.push(r);
    }
    ok(out)
}

async fn get_run(State(st): State<St>, Path(id): Path<String>) -> Res {
    pause(&st).await;
    let s = st.store.lock().unwrap();
    let id = s.find_run(&id).map_err(core_err)?;
    ok(s.run_detail(&id).map_err(core_err)?)
}

async fn get_file(State(st): State<St>, Path((id, name)): Path<(String, String)>) -> Res {
    pause(&st).await;
    let s = st.store.lock().unwrap();
    let id = s.find_run(&id).map_err(core_err)?;
    let data = s.file(&id, &name).map_err(core_err)?;
    let ct = if name.ends_with(".json") { "application/json" } else { "text/plain; charset=utf-8" };
    Ok(([(header::CONTENT_TYPE, ct)], data).into_response())
}

async fn patch_tags(State(st): State<St>, Path(id): Path<String>, h: HeaderMap, body: Bytes) -> Res {
    pause(&st).await;
    check_token(&st, &h)?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| err(StatusCode::BAD_REQUEST, format!("body is not JSON: {e}")))?;
    let obj = v.as_object().ok_or_else(|| err(StatusCode::BAD_REQUEST, "body must be a JSON object"))?;
    let mut tags = BTreeMap::new();
    for (k, v) in obj {
        if !valid_tag_key(k) {
            return Err(err(StatusCode::BAD_REQUEST, format!("bad tag key {k:?}")));
        }
        let v = v.as_str().ok_or_else(|| err(StatusCode::BAD_REQUEST, format!("tag {k}: value must be a string")))?;
        if v.contains('\n') || v.len() > 1024 {
            return Err(err(StatusCode::BAD_REQUEST, format!("tag {k}: one line, at most 1024 bytes")));
        }
        tags.insert(k.clone(), v.to_string());
    }
    let s = st.store.lock().unwrap();
    let id = s.find_run(&id).map_err(core_err)?;
    s.set_tags(&id, &tags).map_err(core_err)?;
    ok(s.run_summary(&id).map_err(core_err)?)
}

async fn delete_run(State(st): State<St>, Path(id): Path<String>, h: HeaderMap) -> Res {
    pause(&st).await;
    check_token(&st, &h)?;
    let s = st.store.lock().unwrap();
    let id = s.find_run(&id).map_err(core_err)?;
    s.delete_run(&id).map_err(core_err)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn groups(State(st): State<St>) -> Res {
    pause(&st).await;
    ok(st.store.lock().unwrap().list_groups().map_err(core_err)?)
}

async fn compare_h(State(st): State<St>, RawQuery(q): RawQuery) -> Res {
    pause(&st).await;
    let q = query(&q);
    let base = first(&q, "base").ok_or_else(|| err(StatusCode::BAD_REQUEST, "base is required"))?;
    let others: Vec<String> = q.iter().filter(|(k, _)| k == "other").map(|(_, v)| v.clone()).collect();
    if others.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "at least one other is required"));
    }
    let s = st.store.lock().unwrap();
    let mut c = compare::compare(&s, &base, &others).map_err(core_err)?;
    if let Some(sec) = first(&q, "section").filter(|x| !x.is_empty()) {
        c.rows.retain(|r| r.section == sec);
    }
    ok(c)
}

async fn values(State(st): State<St>, RawQuery(q): RawQuery) -> Res {
    pause(&st).await;
    let q = query(&q);
    let r = first(&q, "ref").ok_or_else(|| err(StatusCode::BAD_REQUEST, "ref is required"))?;
    let id = first(&q, "id").ok_or_else(|| err(StatusCode::BAD_REQUEST, "id is required"))?;
    let s = st.store.lock().unwrap();
    let (side, vals) = compare::load(&s, &r).map_err(core_err)?;
    let vals: Vec<_> = vals.into_iter().filter(|v| v.id == id).collect();
    ok(json!({ "side": side, "id": id, "values": vals }))
}

async fn metrics(State(st): State<St>, RawQuery(q): RawQuery) -> Res {
    pause(&st).await;
    let q = query(&q);
    let v = first(&q, "version").unwrap_or_default();
    ok(st.store.lock().unwrap().metrics(&v).map_err(core_err)?)
}

async fn api_404(uri: Uri) -> Response {
    err(StatusCode::NOT_FOUND, format!("no endpoint {}", uri.path()))
}

/// Static files from web/ on disk (no caching, so edits show on reload). Unknown paths get index.html.
async fn static_file(State(st): State<St>, uri: Uri) -> Response {
    let rel = uri.path().trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let safe = !rel.split('/').any(|p| p == ".." || p.starts_with('.')) && !rel.starts_with("dev/");
    let mut path = st.web.join(if safe { rel } else { "index.html" });
    if !path.is_file() {
        path = st.web.join("index.html");
    }
    let ct = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        _ => "application/octet-stream",
    };
    match std::fs::read(&path) {
        Ok(b) => ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "no-store")], b).into_response(),
        Err(e) => err(StatusCode::NOT_FOUND, e.to_string()),
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let db = arg("--db").unwrap_or_else(|| "benchlens.db".into());
    let listen = arg("--listen").unwrap_or_else(|| "127.0.0.1:8090".into());
    let web = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let store = Store::open(std::path::Path::new(&db)).expect("open database");
    let st = St {
        store: Arc::new(Mutex::new(store)),
        web: web.canonicalize().expect("web folder"),
        token: std::env::var("BENCHLENS_DEV_TOKEN").ok().filter(|t| !t.is_empty()),
        delay_ms: std::env::var("BENCHLENS_DEV_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0),
    };
    let api = Router::new()
        .route("/health", get(health))
        .route("/runs", get(list_runs))
        .route("/runs/{id}", get(get_run).delete(delete_run))
        .route("/runs/{id}/files/{name}", get(get_file))
        .route("/runs/{id}/tags", patch(patch_tags))
        .route("/groups", get(groups))
        .route("/compare", get(compare_h))
        .route("/values", get(values))
        .route("/metrics", get(metrics))
        .fallback(api_404);
    let app = Router::new().nest("/api", api).fallback(static_file).with_state(st);
    let l = tokio::net::TcpListener::bind(&listen).await.expect("bind");
    eprintln!("benchlens web dev server on http://{listen} (database {db})");
    axum::serve(l, app).await.unwrap();
}
