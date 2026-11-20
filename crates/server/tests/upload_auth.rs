//! Upload (`POST /api/runs`) and token auth, through the router (no network).

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use benchlens_core::store::Store;
use benchlens_core::testing::{fixture_store, fixtures};
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
    www_authenticate: Option<String>,
    body: String,
}

impl Res {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }
    fn assert_error(&self, status: StatusCode) -> String {
        assert_eq!(self.status, status, "body: {}", self.body);
        self.json()["error"]
            .as_str()
            .expect("error message")
            .to_string()
    }
}

async fn call(
    state: &AppState,
    method: Method,
    path: &str,
    auth: Option<&str>,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> Res {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(a) = auth {
        req = req.header(header::AUTHORIZATION, a);
    }
    if let Some(ct) = content_type {
        req = req.header(header::CONTENT_TYPE, ct);
    }
    let res = router(state.clone())
        .oneshot(req.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let www_authenticate = res
        .headers()
        .get(header::WWW_AUTHENTICATE)
        .map(|v| v.to_str().unwrap().to_string());
    let body = res.into_body().collect().await.unwrap().to_bytes();
    Res {
        status,
        www_authenticate,
        body: String::from_utf8_lossy(&body).to_string(),
    }
}

/// A tar.gz of one fixture result folder, as `tar -czf FOLDER.tar.gz FOLDER` would make it.
fn fixture_tar_gz() -> Vec<u8> {
    let dir = fixtures().join("v0.2.3").join(FOLDER);
    let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), flate2::Compression::default()));
    tar.append_dir_all(FOLDER, &dir).unwrap();
    tar.into_inner().unwrap().finish().unwrap()
}

/// A tar holding one entry whose path climbs out with `..` (written into the header by hand: the tar crate
/// refuses to build such a path).
fn dotdot_tar() -> Vec<u8> {
    let data = b"{}";
    let mut h = tar::Header::new_old();
    let name = b"../evil/meta.json";
    h.as_old_mut().name[..name.len()].copy_from_slice(name);
    h.set_size(data.len() as u64);
    h.set_mode(0o644);
    h.set_entry_type(tar::EntryType::Regular);
    h.set_cksum();
    let mut out = h.as_bytes().to_vec();
    out.extend_from_slice(data);
    out.resize(512 * 2, 0);
    out.extend_from_slice(&[0u8; 1024]);
    out
}

/// A multipart/form-data body: `file` (when given) and `tag` fields.
fn multipart(file: Option<(&str, &[u8])>, tags: &[&str]) -> Vec<u8> {
    let mut b = Vec::new();
    for t in tags {
        b.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"tag\"\r\n\r\n{t}\r\n")
                .as_bytes(),
        );
    }
    if let Some((name, data)) = file {
        b.extend_from_slice(
            format!(
                "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\n\
                 Content-Type: application/gzip\r\n\r\n"
            )
            .as_bytes(),
        );
        b.extend_from_slice(data);
        b.extend_from_slice(b"\r\n");
    }
    b.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    b
}

fn mp_type() -> String {
    format!("multipart/form-data; boundary={BOUNDARY}")
}

async fn upload(state: &AppState, auth: Option<&str>, body: Vec<u8>) -> Res {
    call(
        state,
        Method::POST,
        "/api/runs",
        auth,
        Some(&mp_type()),
        body,
    )
    .await
}

fn bearer() -> String {
    format!("Bearer {TOKEN}")
}

fn empty_state() -> AppState {
    AppState::new(Store::open_in_memory().unwrap())
}

#[tokio::test]
async fn upload_imports_tags_and_is_idempotent() {
    let st = empty_state();
    let gz = fixture_tar_gz();
    let body = multipart(Some(("x86-std-r2.tar.gz", &gz)), &["set=poc", "round=7"]);
    let r = upload(&st, None, body.clone()).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let reports = r.json();
    let reports = reports.as_array().unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0]["folder"], FOLDER);
    assert_eq!(reports[0]["outcome"], "imported");
    let run_id = reports[0]["run_id"].as_str().unwrap().to_string();

    let runs = call(&st, Method::GET, "/api/runs", None, None, vec![])
        .await
        .json();
    let run = &runs.as_array().unwrap()[0];
    assert_eq!(run["run_id"], run_id.as_str());
    assert_eq!(run["source"], "upload");
    assert_eq!(run["tags"]["set"], "poc");
    assert_eq!(run["tags"]["round"], "7");
    assert!(run["values"].as_i64().unwrap() > 0);

    // the same archive again: already present, tags applied again, nothing added
    let r = upload(
        &st,
        None,
        multipart(Some(("again.tar.gz", &gz)), &["round=8"]),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.json()[0]["outcome"], "already_present");
    assert_eq!(r.json()[0]["run_id"], run_id.as_str());
    let runs = call(&st, Method::GET, "/api/runs", None, None, vec![])
        .await
        .json();
    assert_eq!(runs.as_array().unwrap().len(), 1);
    assert_eq!(runs[0]["tags"]["round"], "8");
}

#[tokio::test]
async fn upload_same_values_as_cli_import() {
    // the uploaded run equals the one `benchlens import` makes from the folder
    let st = empty_state();
    let gz = fixture_tar_gz();
    let r = upload(&st, None, multipart(Some(("a.tar.gz", &gz)), &[])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let cli = fixture_store(false);
    let want = cli
        .list_runs()
        .unwrap()
        .into_iter()
        .find(|r| r.folder == FOLDER)
        .unwrap();
    assert_eq!(r.json()[0]["run_id"], want.run_id.as_str());
    let got = call(&st, Method::GET, "/api/runs", None, None, vec![])
        .await
        .json();
    assert_eq!(got[0]["values"], want.values);
}

#[tokio::test]
async fn upload_over_the_limit_is_413() {
    let gz = fixture_tar_gz();
    assert!(gz.len() > 2048, "fixture archive is {} bytes", gz.len());
    let st = empty_state().with_max_upload_bytes(2048);
    let r = upload(&st, None, multipart(Some(("big.tar.gz", &gz)), &[])).await;
    r.assert_error(StatusCode::PAYLOAD_TOO_LARGE);
    // a body far over the limit is cut off by the body limit as well
    let huge = vec![0u8; 2 * 1024 * 1024];
    let r = upload(&st, None, multipart(Some(("huge.tar", &huge)), &[])).await;
    r.assert_error(StatusCode::PAYLOAD_TOO_LARGE);
    let runs = call(&st, Method::GET, "/api/runs", None, None, vec![])
        .await
        .json();
    assert_eq!(runs.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn unsafe_and_broken_uploads_are_400() {
    let st = empty_state();
    let msg = upload(&st, None, multipart(Some(("evil.tar", &dotdot_tar())), &[]))
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    assert!(msg.contains("unsafe"), "{msg}");

    let msg = upload(
        &st,
        None,
        multipart(
            Some(("notes.tar.gz", b"just some text, not an archive")),
            &[],
        ),
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST);
    assert!(!msg.is_empty());
    let noise: Vec<u8> = (0..4096u32)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect();
    upload(&st, None, multipart(Some(("noise.tar", &noise)), &[]))
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    // gzip header, then garbage
    let mut badgz = vec![0x1f, 0x8b];
    badgz.extend_from_slice(&noise);
    upload(&st, None, multipart(Some(("bad.tar.gz", &badgz)), &[]))
        .await
        .assert_error(StatusCode::BAD_REQUEST);

    upload(&st, None, multipart(Some(("empty.tar.gz", b"")), &[]))
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    upload(&st, None, multipart(None, &["set=poc"]))
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    // not multipart at all
    call(
        &st,
        Method::POST,
        "/api/runs",
        None,
        Some("application/json"),
        b"{}".to_vec(),
    )
    .await
    .assert_error(StatusCode::BAD_REQUEST);
    let runs = call(&st, Method::GET, "/api/runs", None, None, vec![])
        .await
        .json();
    assert_eq!(runs.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn bad_tags_and_fields_are_400_and_import_nothing() {
    let st = empty_state();
    let gz = fixture_tar_gz();
    for tags in [&["bad key=1"][..], &["novalue"], &["k=line\rbreak"]] {
        upload(&st, None, multipart(Some(("a.tar.gz", &gz)), tags))
            .await
            .assert_error(StatusCode::BAD_REQUEST);
    }
    let many: Vec<String> = (0..65).map(|i| format!("k{i}=v")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    upload(&st, None, multipart(Some(("a.tar.gz", &gz)), &many))
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    let mut two = multipart(Some(("a.tar.gz", &gz)), &[]);
    two.truncate(two.len() - format!("--{BOUNDARY}--\r\n").len());
    two.extend_from_slice(&multipart(Some(("b.tar.gz", &gz)), &[]));
    upload(&st, None, two)
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    let runs = call(&st, Method::GET, "/api/runs", None, None, vec![])
        .await
        .json();
    assert_eq!(runs.as_array().unwrap().len(), 0);
}

fn token_state(read_public: bool) -> AppState {
    AppState::new(fixture_store(true))
        .with_access(Access::with_token(Token::new(TOKEN), read_public))
}

async fn some_run(st: &AppState, auth: Option<&str>) -> String {
    let runs = call(st, Method::GET, "/api/runs", auth, None, vec![])
        .await
        .json();
    runs[0]["run_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn writes_need_the_token() {
    let st = token_state(true);
    let id = some_run(&st, None).await;
    let tags_path = format!("/api/runs/{id}/tags");
    let run_path = format!("/api/runs/{id}");
    let gz = fixture_tar_gz();
    for auth in [
        None,
        Some("Bearer wrong-token-0123456789"),
        Some("Basic dGVzdA=="),
    ] {
        let r = call(
            &st,
            Method::PATCH,
            &tags_path,
            auth,
            Some("application/json"),
            br#"{"c2check":"x"}"#.to_vec(),
        )
        .await;
        r.assert_error(StatusCode::UNAUTHORIZED);
        assert_eq!(
            r.www_authenticate.as_deref(),
            Some("Bearer realm=\"benchlens\"")
        );
        let r = call(&st, Method::DELETE, &run_path, auth, None, vec![]).await;
        r.assert_error(StatusCode::UNAUTHORIZED);
        assert!(r.www_authenticate.is_some());
        let r = upload(&st, auth, multipart(Some(("a.tar.gz", &gz)), &[])).await;
        r.assert_error(StatusCode::UNAUTHORIZED);
        assert!(r.www_authenticate.is_some());
    }
    // nothing changed
    let runs = call(&st, Method::GET, "/api/runs", None, None, vec![])
        .await
        .json();
    assert!(runs
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["tags"].get("c2check").is_none()));

    let ok = bearer();
    let r = call(
        &st,
        Method::PATCH,
        &tags_path,
        Some(&ok),
        Some("application/json"),
        br#"{"c2check":"x"}"#.to_vec(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.json()["tags"]["c2check"], "x");
    let r = upload(&st, Some(&ok), multipart(Some(("a.tar.gz", &gz)), &[])).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.json()[0]["outcome"], "already_present");
    let r = call(&st, Method::DELETE, &run_path, Some(&ok), None, vec![]).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.body);
}

#[tokio::test]
async fn private_reads_need_the_token_except_health_and_ui() {
    let st = token_state(false);
    for path in [
        "/api/runs",
        "/api/groups",
        "/api/metrics?version=0.2.3",
        "/api/compare?base=a&other=b",
        "/api/values?ref=a&id=b",
        "/api/export.html?base=group:x86-high&other=group:arm64-high",
    ] {
        let r = call(&st, Method::GET, path, None, None, vec![]).await;
        r.assert_error(StatusCode::UNAUTHORIZED);
        assert!(r.www_authenticate.is_some(), "{path}");
        let r = call(
            &st,
            Method::GET,
            path,
            Some("Bearer nope-nope-nope-nope"),
            None,
            vec![],
        )
        .await;
        r.assert_error(StatusCode::UNAUTHORIZED);
    }
    let r = call(&st, Method::GET, "/api/health", None, None, vec![]).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    for path in ["/", "/index.html", "/js/app.js"] {
        let r = call(&st, Method::GET, path, None, None, vec![]).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
    }
    let ok = bearer();
    let id = some_run(&st, Some(&ok)).await;
    let r = call(
        &st,
        Method::GET,
        &format!("/api/runs/{id}"),
        Some(&ok),
        None,
        vec![],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // the export carries every value of its sides: same guard as the other reads
    let r = call(
        &st,
        Method::GET,
        "/api/export.html?base=group:x86-high&other=group:arm64-high",
        Some(&ok),
        None,
        vec![],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("id=\"benchlens-data\""));
    let r = call(
        &st,
        Method::GET,
        &format!("/api/runs/{id}/files/meta.json"),
        None,
        None,
        vec![],
    )
    .await;
    r.assert_error(StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn public_reads_stay_open_with_a_token() {
    let st = token_state(true);
    let r = call(&st, Method::GET, "/api/runs", None, None, vec![]).await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn no_token_on_open_address_refuses_writes() {
    let st = AppState::new(fixture_store(true)).with_access(Access::public_read_only());
    let id = some_run(&st, None).await;
    let r = call(
        &st,
        Method::PATCH,
        &format!("/api/runs/{id}/tags"),
        None,
        Some("application/json"),
        br#"{"c2check":"x"}"#.to_vec(),
    )
    .await;
    r.assert_error(StatusCode::FORBIDDEN);
    assert!(r.www_authenticate.is_none());
    call(
        &st,
        Method::DELETE,
        &format!("/api/runs/{id}"),
        None,
        None,
        vec![],
    )
    .await
    .assert_error(StatusCode::FORBIDDEN);
    upload(
        &st,
        None,
        multipart(Some(("a.tar.gz", &fixture_tar_gz())), &[]),
    )
    .await
    .assert_error(StatusCode::FORBIDDEN);
    // a token header does not help when none is configured
    call(
        &st,
        Method::DELETE,
        &format!("/api/runs/{id}"),
        Some(&bearer()),
        None,
        vec![],
    )
    .await
    .assert_error(StatusCode::FORBIDDEN);
    let r = call(&st, Method::GET, "/api/runs", None, None, vec![]).await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn loopback_without_token_keeps_writes_open() {
    let lo = "127.0.0.1:8080".parse().unwrap();
    let st = AppState::new(fixture_store(true)).with_access(Access::for_listen(None, true, lo));
    let id = some_run(&st, None).await;
    let r = call(
        &st,
        Method::DELETE,
        &format!("/api/runs/{id}"),
        None,
        None,
        vec![],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
}
