//! Server hardening through the router (no network): the Host check against DNS rebinding (F14), open writes
//! refused through a reverse proxy (F15) and the server's registry applied to uploads (F16).

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use benchlens_core::store::Store;
use benchlens_core::testing::fixtures;
use benchlens_server::auth::{Access, Token};
use benchlens_server::{router, AppState};
use flate2::write::GzEncoder;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

const TOKEN: &str = "test-token-0123456789";
const BOUNDARY: &str = "benchlens-test-boundary";
const FOLDER: &str = "x86-std-r2-20261119-103253";

struct Res {
    status: StatusCode,
    body: String,
}

impl Res {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }
}

async fn send(state: &AppState, req: Request<Body>) -> Res {
    let res = router(state.clone()).oneshot(req).await.unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    Res {
        status,
        body: String::from_utf8_lossy(&body).to_string(),
    }
}

async fn get(state: &AppState, path: &str, host: &str) -> Res {
    let req = Request::get(path)
        .header(header::HOST, host)
        .body(Body::empty())
        .unwrap();
    send(state, req).await
}

fn fixture_tar_gz() -> Vec<u8> {
    let dir = fixtures().join("v0.2.3").join(FOLDER);
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), flate2::Compression::default()));
    tar.append_dir_all(FOLDER, &dir).unwrap();
    tar.into_inner().unwrap().finish().unwrap()
}

fn multipart(file: &[u8], tags: &[&str]) -> Vec<u8> {
    let mut b = Vec::new();
    for t in tags {
        b.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"tag\"\r\n\r\n{t}\r\n")
                .as_bytes(),
        );
    }
    b.extend_from_slice(
        format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.tar.gz\"\r\n\
             Content-Type: application/gzip\r\n\r\n"
        )
        .as_bytes(),
    );
    b.extend_from_slice(file);
    b.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    b
}

fn upload_req(extra: &[(&str, &str)], tags: &[&str]) -> Request<Body> {
    let mut r = Request::post("/api/runs").header(
        header::CONTENT_TYPE,
        format!("multipart/form-data; boundary={BOUNDARY}"),
    );
    for (k, v) in extra {
        r = r.header(*k, *v);
    }
    r.body(Body::from(multipart(&fixture_tar_gz(), tags)))
        .unwrap()
}

fn local() -> AppState {
    AppState::new(Store::open_in_memory().unwrap())
}

fn with_access(access: Access) -> AppState {
    local().with_access(access)
}

#[tokio::test]
async fn rebinding_host_is_refused_without_token() {
    let st = local();
    for path in ["/api/health", "/api/runs", "/", "/index.html", "/compare"] {
        for host in [
            "evil.example",
            "evil.example:8080",
            "localhost.evil.example:8080",
        ] {
            let r = get(&st, path, host).await;
            assert_eq!(r.status, StatusCode::FORBIDDEN, "{path} {host}: {}", r.body);
            let j = r.json();
            assert!(j["error"].as_str().unwrap().contains("refused"), "{j}");
            assert!(
                j["detail"].as_str().unwrap().contains("allowed_hosts"),
                "{j}"
            );
        }
    }
    // writes too, before anything is read
    let r = send(&st, upload_req(&[("host", "evil.example:8080")], &[])).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(st.store.lock().unwrap().run_count().unwrap(), 0);
}

#[tokio::test]
async fn loopback_hosts_are_served_with_and_without_port() {
    let st = local();
    for host in [
        "localhost",
        "localhost:8080",
        "127.0.0.1",
        "127.0.0.1:8080",
        "[::1]",
        "[::1]:8080",
        "192.168.1.5:8080",
    ] {
        for path in ["/api/health", "/"] {
            let r = get(&st, path, host).await;
            assert_eq!(r.status, StatusCode::OK, "{path} {host}: {}", r.body);
        }
    }
    let r = send(&st, upload_req(&[("host", "localhost:8080")], &[])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

#[tokio::test]
async fn allowed_hosts_are_served() {
    let st = with_access(Access::local().with_allowed_hosts(["bench.lan".to_string()]));
    assert_eq!(get(&st, "/", "bench.lan:8080").await.status, StatusCode::OK);
    assert_eq!(
        get(&st, "/api/runs", "BENCH.LAN").await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&st, "/api/runs", "other.lan").await.status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn token_mode_serves_any_host_unless_allowed_hosts_is_set() {
    let st = with_access(Access::with_token(Token::new(TOKEN), true));
    assert_eq!(
        get(&st, "/api/runs", "evil.example:8080").await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&st, "/", "bench.example.com").await.status,
        StatusCode::OK
    );
    let st = with_access(
        Access::with_token(Token::new(TOKEN), true)
            .with_allowed_hosts(["bench.example.com".to_string()]),
    );
    assert_eq!(
        get(&st, "/", "bench.example.com").await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&st, "/api/health", "127.0.0.1:8080").await.status,
        StatusCode::OK
    );
    assert_eq!(
        get(&st, "/", "evil.example").await.status,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn open_writes_through_a_proxy_are_refused() {
    let st = local();
    for (h, v) in [
        ("x-forwarded-for", "192.0.2.7"),
        ("x-forwarded-host", "bench.example.com"),
        ("forwarded", "for=192.0.2.7;proto=https"),
    ] {
        let r = send(&st, upload_req(&[("host", "127.0.0.1:8080"), (h, v)], &[])).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{h}: {}", r.body);
        assert!(r.json()["detail"].as_str().unwrap().contains("token"));
        let del = Request::builder()
            .method(Method::DELETE)
            .uri("/api/runs/0123456789abcdef")
            .header(h, v)
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&st, del).await.status, StatusCode::FORBIDDEN, "{h}");
        // reads through the proxy stay open
        let read = Request::get("/api/runs")
            .header(h, v)
            .body(Body::empty())
            .unwrap();
        assert_eq!(send(&st, read).await.status, StatusCode::OK, "{h}");
    }
    assert_eq!(st.store.lock().unwrap().run_count().unwrap(), 0);
    // with a token, a proxy is fine
    let st = with_access(Access::with_token(Token::new(TOKEN), true));
    let auth = format!("Bearer {TOKEN}");
    let r = send(
        &st,
        upload_req(
            &[
                ("x-forwarded-for", "192.0.2.7"),
                ("authorization", auth.as_str()),
            ],
            &[],
        ),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

#[tokio::test]
async fn uploads_get_the_server_registry_form_tags_win() {
    let dir = tempfile::tempdir().unwrap();
    let reg = dir.path().join("registry.tsv");
    std::fs::write(
        &reg,
        format!("run\tgroup\tnote\tround\n{FOLDER}\tfrom-registry\tregistry note\t3\n"),
    )
    .unwrap();
    let st = local().with_registry(Some(reg.clone()));
    let r = send(&st, upload_req(&[], &["note=form note"])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let runs = get(&st, "/api/runs", "localhost").await.json();
    let tags = &runs[0]["tags"];
    assert_eq!(tags["group"], "from-registry");
    assert_eq!(tags["round"], "3");
    assert_eq!(tags["note"], "form note", "the form's tags win");

    // a registry that does not parse refuses the upload instead of importing untagged
    std::fs::write(&reg, "no run column\n").unwrap();
    let st = local().with_registry(Some(reg));
    let r = send(&st, upload_req(&[], &[])).await;
    assert_eq!(r.status, StatusCode::INTERNAL_SERVER_ERROR, "{}", r.body);
    assert!(r.json()["error"].as_str().unwrap().contains("registry"));
    assert_eq!(st.store.lock().unwrap().run_count().unwrap(), 0);
}
