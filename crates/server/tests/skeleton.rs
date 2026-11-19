use axum::body::Body;
use axum::http::{Request, StatusCode};
use benchlens_core::store::Store;
use benchlens_server::{router, AppState};
use http_body_util::BodyExt;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn app() -> axum::Router {
    router(AppState {
        store: Arc::new(Mutex::new(Store::open_in_memory().unwrap())),
    })
}

async fn get(path: &str) -> (StatusCode, String) {
    let res = app()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&body).to_string())
}

#[tokio::test]
async fn health_answers() {
    let (s, body) = get("/api/health").await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["runs"], 0);
    assert_eq!(v["db_layout"], 1);
}

#[tokio::test]
async fn web_ui_is_served() {
    let (s, body) = get("/").await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("benchlens"));
    let (s, _) = get("/some/deep/link").await;
    assert_eq!(s, StatusCode::OK, "unknown UI paths get index.html");
}

#[tokio::test]
async fn unfinished_endpoints_say_so() {
    let (s, body) = get("/api/runs").await;
    assert_eq!(s, StatusCode::NOT_IMPLEMENTED);
    assert!(body.contains("task A"));
    let (s, _) = get("/api/nope").await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
