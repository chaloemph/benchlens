//! API v1 against the test fixtures, through the router (no network).

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use benchlens_core::compare;
use benchlens_core::import::{import_path, ImportOptions, Registry};
use benchlens_core::store::Store;
use benchlens_server::{router, AppState};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_store() -> Store {
    let store = Store::open_in_memory().unwrap();
    let text = std::fs::read_to_string(root().join("examples/poc-2026-11/registry.tsv")).unwrap();
    let opts = ImportOptions {
        registry: Some(Registry::parse(&text).unwrap()),
        ..Default::default()
    };
    for ver in ["v0.2.1", "v0.2.3"] {
        import_path(&store, &root().join("tests/fixtures").join(ver), &opts).unwrap();
    }
    store
}

struct App {
    state: AppState,
}

struct Res {
    status: StatusCode,
    content_type: String,
    body: String,
}

impl Res {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", self.body))
    }
    /// An error body: `{"error": "..."}` with the given status.
    fn assert_error(&self, status: StatusCode) -> String {
        assert_eq!(self.status, status, "body: {}", self.body);
        let v = self.json();
        let msg = v["error"].as_str().expect("error message").to_string();
        assert!(!msg.is_empty());
        msg
    }
}

impl App {
    fn new() -> App {
        App {
            state: AppState {
                store: Arc::new(Mutex::new(fixture_store())),
            },
        }
    }

    async fn call(&self, method: Method, path: &str, body: Option<String>) -> Res {
        let mut req = Request::builder().method(method).uri(path);
        if body.is_some() {
            req = req.header(header::CONTENT_TYPE, "application/json");
        }
        let req = req
            .body(body.map(Body::from).unwrap_or_else(Body::empty))
            .unwrap();
        let res = router(self.state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let content_type = res
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        Res {
            status,
            content_type,
            body: String::from_utf8_lossy(&body).to_string(),
        }
    }

    async fn get(&self, path: &str) -> Res {
        self.call(Method::GET, path, None).await
    }

    async fn get_json(&self, path: &str) -> Value {
        let r = self.get(path).await;
        assert_eq!(r.status, StatusCode::OK, "{path}: {}", r.body);
        r.json()
    }

    async fn patch(&self, path: &str, body: &str) -> Res {
        self.call(Method::PATCH, path, Some(body.to_string())).await
    }

    /// The run id of the run with this label.
    async fn id_of(&self, label: &str) -> String {
        let runs = self.get_json("/api/runs").await;
        runs.as_array()
            .unwrap()
            .iter()
            .find(|r| r["label"] == label)
            .unwrap_or_else(|| panic!("no run {label}"))["run_id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn with_store<T>(&self, f: impl FnOnce(&Store) -> T) -> T {
        f(&self.state.store.lock().unwrap())
    }
}

/// What the server would send for `x`, read back the way the test reads responses (serde_json's float parsing
/// is not exact, so compare text-parsed values with text-parsed values).
fn as_sent<T: serde::Serialize>(x: &T) -> Value {
    serde_json::from_str(&serde_json::to_string(x).unwrap()).unwrap()
}

fn labels(v: &Value) -> Vec<String> {
    let mut l: Vec<String> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["label"].as_str().unwrap().to_string())
        .collect();
    l.sort();
    l
}

#[tokio::test]
async fn runs_list_everything_oldest_first() {
    let app = App::new();
    let runs = app.get_json("/api/runs").await;
    let a = runs.as_array().unwrap();
    assert_eq!(a.len(), 7);
    let starts: Vec<&str> = a.iter().map(|r| r["start_utc"].as_str().unwrap()).collect();
    let mut sorted = starts.clone();
    sorted.sort();
    assert_eq!(starts, sorted);
    let r = &a[0];
    for key in [
        "run_id",
        "folder",
        "label",
        "group",
        "arch",
        "nproc",
        "ram_gb",
        "cpu_model",
        "vmbench_version",
        "start_utc",
        "source",
        "catalog_known",
        "failed_stages",
        "tags",
        "values",
    ] {
        assert!(r.get(key).is_some(), "RunSummary has {key}");
    }
    // same as the store
    let direct = app.with_store(|s| as_sent(&s.list_runs().unwrap()));
    assert_eq!(runs, direct);
}

#[tokio::test]
async fn runs_filters() {
    let app = App::new();
    let v = app.get_json("/api/runs?arch=aarch64").await;
    assert_eq!(
        labels(&v),
        ["arm64-high", "arm64-high-r2", "arm64-high-web"]
    );
    let v = app.get_json("/api/runs?group=x86-std").await;
    assert_eq!(labels(&v), ["x86-std-r2"]);
    let v = app.get_json("/api/runs?tag=set:poc-x86").await;
    assert_eq!(labels(&v).len(), 4);
    // repeated tag filters all must match
    let v = app
        .get_json("/api/runs?tag=set:poc-x86&tag=size:high")
        .await;
    assert_eq!(labels(&v), ["x86-high", "x86-high-r2", "x86-high-web"]);
    // filters combine
    let v = app
        .get_json("/api/runs?tag=size:high&arch=x86_64&q=WEB")
        .await;
    // x86-high matches through its note tag ("... rerun in x86-high-web")
    assert_eq!(labels(&v), ["x86-high", "x86-high-web"]);
    // q looks into tag values too, case-insensitive
    let v = app.get_json("/api/runs?q=RERUN").await;
    assert!(!labels(&v).is_empty());
    let v = app.get_json("/api/runs?q=r2").await;
    assert_eq!(labels(&v), ["arm64-high-r2", "x86-high-r2", "x86-std-r2"]);
    // nothing matches: empty list, not an error
    let v = app.get_json("/api/runs?arch=riscv64").await;
    assert_eq!(v, json!([]));
    let v = app.get_json("/api/runs?tag=set:nope").await;
    assert_eq!(v, json!([]));
    // empty filter values are ignored
    let v = app.get_json("/api/runs?arch=&q=").await;
    assert_eq!(v.as_array().unwrap().len(), 7);
}

#[tokio::test]
async fn runs_bad_filters() {
    let app = App::new();
    for path in [
        "/api/runs?tag=nocolon",
        "/api/runs?tag=bad%20key:x",
        "/api/runs?tag=:x",
        "/api/runs?color=red",
        "/api/runs?arch=a&arch=b",
        "/api/runs?q=%0Aline",
    ] {
        app.get(path).await.assert_error(StatusCode::BAD_REQUEST);
    }
    let long = "x".repeat(2000);
    app.get(&format!("/api/runs?q={long}"))
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    let many: Vec<String> = (0..40).map(|i| format!("tag=k:{i}")).collect();
    app.get(&format!("/api/runs?{}", many.join("&")))
        .await
        .assert_error(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_detail_by_id_prefix_and_label() {
    let app = App::new();
    let id = app.id_of("x86-high-r2").await;
    let v = app.get_json(&format!("/api/runs/{id}")).await;
    assert_eq!(v["run_id"], id.as_str());
    assert_eq!(v["label"], "x86-high-r2");
    assert_eq!(v["hardware"]["cpu"]["model"], "INTEL(R) XEON(R) GOLD 6526Y");
    assert_eq!(v["meta"]["schema"], 1);
    for key in [
        "stages",
        "end_utc",
        "imported_at",
        "pg_profile",
        "web_cpus",
        "tags",
        "group",
    ] {
        assert!(v.get(key).is_some(), "RunDetail has {key}");
    }
    let files: Vec<&str> = v["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert!(files.contains(&"meta.json") && files.contains(&"results.tsv"));
    // prefix and label give the same run
    assert_eq!(app.get_json(&format!("/api/runs/{}", &id[..6])).await, v);
    assert_eq!(app.get_json("/api/runs/x86-high-r2").await, v);
}

#[tokio::test]
async fn run_detail_not_found_and_bad() {
    let app = App::new();
    let msg = app
        .get("/api/runs/nope")
        .await
        .assert_error(StatusCode::NOT_FOUND);
    assert!(msg.contains("nope"));
    app.get("/api/runs/0000000000000000")
        .await
        .assert_error(StatusCode::NOT_FOUND);
    // a group is not a run
    app.get("/api/runs/group:x86-high")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn run_files() {
    let app = App::new();
    let id = app.id_of("x86-high-r2").await;
    let r = app.get(&format!("/api/runs/{id}/files/results.tsv")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.content_type, "text/plain; charset=utf-8");
    let stored = std::fs::read_to_string(
        root().join("tests/fixtures/v0.2.3/x86-high-r2-20261119-145026/results.tsv"),
    )
    .unwrap();
    assert_eq!(r.body, stored);

    let r = app.get(&format!("/api/runs/{id}/files/meta.json")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.content_type, "application/json");
    assert_eq!(r.json()["schema"], 1);

    // not kept for this run, not a kept name at all, and an unknown run
    app.get(&format!("/api/runs/{id}/files/report.md"))
        .await
        .assert_error(StatusCode::NOT_FOUND);
    app.get(&format!("/api/runs/{id}/files/secret.txt"))
        .await
        .assert_error(StatusCode::NOT_FOUND);
    app.get(&format!("/api/runs/{id}/files/..%2Fmeta.json"))
        .await
        .assert_error(StatusCode::NOT_FOUND);
    app.get("/api/runs/nope/files/meta.json")
        .await
        .assert_error(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn groups() {
    let app = App::new();
    let v = app.get_json("/api/groups").await;
    let names: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["group"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["arm64-high", "x86-high", "x86-std"]);
    assert_eq!(v[1]["runs"], 3);
    let direct = app.with_store(|s| as_sent(&s.list_groups().unwrap()));
    assert_eq!(v, direct);
}

#[tokio::test]
async fn compare_equals_core() {
    let app = App::new();
    let v = app
        .get_json("/api/compare?base=group:x86-high&other=group:arm64-high&other=x86-std-r2")
        .await;
    let direct = app.with_store(|s| {
        let c = compare::compare(
            s,
            "group:x86-high",
            &["group:arm64-high".to_string(), "x86-std-r2".to_string()],
        )
        .unwrap();
        as_sent(&c)
    });
    assert_eq!(v, direct);
    assert_eq!(v["others"].as_array().unwrap().len(), 2);
    assert_eq!(v["tallies"].as_array().unwrap().len(), 2);
    assert!(v["rows"].as_array().unwrap().len() > 10);

    // run against run, by id prefix
    let a = app.id_of("x86-high-r2").await;
    let b = app.id_of("arm64-high-r2").await;
    let v = app
        .get_json(&format!("/api/compare?base={}&other={}", &a[..8], b))
        .await;
    let direct = app
        .with_store(|s| as_sent(&compare::compare(s, &a[..8], std::slice::from_ref(&b)).unwrap()));
    assert_eq!(v, direct);
}

#[tokio::test]
async fn compare_section_keeps_tallies() {
    let app = App::new();
    let all = app
        .get_json("/api/compare?base=group:x86-high&other=group:arm64-high")
        .await;
    let cpu = app
        .get_json("/api/compare?base=group:x86-high&other=group:arm64-high&section=cpu")
        .await;
    let rows = cpu["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r["section"] == "cpu"));
    let n_cpu = all["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["section"] == "cpu")
        .count();
    assert_eq!(rows.len(), n_cpu);
    assert!(rows.len() < all["rows"].as_array().unwrap().len());
    assert_eq!(cpu["tallies"], all["tallies"]);
}

#[tokio::test]
async fn compare_errors() {
    let app = App::new();
    let msg = app
        .get("/api/compare?base=group:x86-high")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    assert!(msg.contains("other"));
    app.get("/api/compare?other=group:x86-high")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    app.get("/api/compare?base=group:&other=x86-std-r2")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    app.get("/api/compare?base=group:x86-high&other=x86-std-r2&section=c%20pu")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    app.get("/api/compare?base=group:x86-high&other=x86-std-r2&extra=1")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    // unknown run, unknown group
    app.get("/api/compare?base=group:x86-high&other=nope")
        .await
        .assert_error(StatusCode::NOT_FOUND);
    app.get("/api/compare?base=group:nope&other=x86-std-r2")
        .await
        .assert_error(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ambiguous_reference_is_refused() {
    let app = App::new();
    // give two runs the same label so that the label matches both
    app.with_store(|s| {
        s.conn()
            .execute(
                "UPDATE runs SET label = 'twin' WHERE label IN ('x86-high-r2', 'x86-high-web')",
                [],
            )
            .unwrap();
    });
    let msg = app
        .get("/api/compare?base=twin&other=x86-std-r2")
        .await
        .assert_error(StatusCode::NOT_FOUND);
    assert!(msg.contains("matches 2 runs"), "{msg}");
    app.get("/api/runs/twin")
        .await
        .assert_error(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn values_of_one_metric() {
    let app = App::new();
    let v = app
        .get_json("/api/values?ref=group:x86-high&id=cpu.7z.rating.max")
        .await;
    assert_eq!(v["id"], "cpu.7z.rating.max");
    assert_eq!(v["side"]["reference"], "group:x86-high");
    assert_eq!(v["side"]["run_ids"].as_array().unwrap().len(), 3);
    let rows = v["values"].as_array().unwrap();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| r["id"] == "cpu.7z.rating.max"));
    let direct = app.with_store(|s| {
        let (_, vals) = compare::load(s, "group:x86-high").unwrap();
        let vals: Vec<_> = vals
            .into_iter()
            .filter(|r| r.id == "cpu.7z.rating.max")
            .collect();
        as_sent(&vals)
    });
    assert_eq!(v["values"], direct);

    // a metric with no values on that side: empty list
    let v = app
        .get_json("/api/values?ref=x86-std-r2&id=no.such.metric")
        .await;
    assert_eq!(v["values"], json!([]));

    app.get("/api/values?ref=group:x86-high")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    app.get("/api/values?id=cpu.7z.rating.max")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    app.get("/api/values?ref=x86-std-r2&id=a%20b")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    app.get("/api/values?ref=nope&id=cpu.7z.rating.max")
        .await
        .assert_error(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn metrics_table() {
    let app = App::new();
    let v = app.get_json("/api/metrics?version=0.2.3").await;
    let a = v.as_array().unwrap();
    assert!(a.len() > 50);
    assert!(a.iter().any(|m| m["id"] == "cpu.7z.rating.max"));
    for key in ["id", "unit", "direction", "tool", "label", "description"] {
        assert!(a[0].get(key).is_some(), "Metric has {key}");
    }
    // a leading v is accepted, as vmbench writes versions
    assert_eq!(app.get_json("/api/metrics?version=v0.2.3").await, v);
    app.get("/api/metrics?version=9.9.9")
        .await
        .assert_error(StatusCode::NOT_FOUND);
    app.get("/api/metrics")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    app.get("/api/metrics?version=0.2.3%27%3B")
        .await
        .assert_error(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tag_edit_moves_run_to_another_group() {
    let app = App::new();
    let id = app.id_of("x86-std-r2").await;
    let r = app
        .patch(
            &format!("/api/runs/{id}/tags"),
            r#"{"group": "x86-std-new", "owner": "lab 2"}"#,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let v = r.json();
    assert_eq!(v["run_id"], id.as_str());
    assert_eq!(v["group"], "x86-std-new");
    assert_eq!(v["tags"]["owner"], "lab 2");

    let groups = app.get_json("/api/groups").await;
    let names: Vec<&str> = groups
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["group"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["arm64-high", "x86-high", "x86-std-new"]);
    let v = app.get_json("/api/runs?tag=owner:lab%202").await;
    assert_eq!(labels(&v), ["x86-std-r2"]);

    // an empty value removes the tag
    let r = app
        .patch(&format!("/api/runs/{id}/tags"), r#"{"owner": ""}"#)
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.json()["tags"].get("owner").is_none());
}

#[tokio::test]
async fn tag_edit_errors() {
    let app = App::new();
    let id = app.id_of("x86-std-r2").await;
    let path = format!("/api/runs/{id}/tags");
    app.patch("/api/runs/nope/tags", r#"{"a": "b"}"#)
        .await
        .assert_error(StatusCode::NOT_FOUND);
    for body in [
        r#"{"bad key": "x"}"#,
        r#"{"a": "two\nlines"}"#,
        r#"{"a": 1}"#,
        r#"["a"]"#,
        "not json",
    ] {
        app.patch(&path, body)
            .await
            .assert_error(StatusCode::BAD_REQUEST);
    }
    let long = format!(r#"{{"a": "{}"}}"#, "x".repeat(1025));
    app.patch(&path, &long)
        .await
        .assert_error(StatusCode::BAD_REQUEST);
    let huge = format!(r#"{{"a": "{}"}}"#, "x".repeat(70 * 1024));
    app.patch(&path, &huge)
        .await
        .assert_error(StatusCode::PAYLOAD_TOO_LARGE);
    // nothing changed
    let v = app.get_json(&format!("/api/runs/{id}")).await;
    assert!(v["tags"].get("a").is_none());
}

#[tokio::test]
async fn delete_run() {
    let app = App::new();
    let id = app.id_of("x86-std-r2").await;
    let r = app
        .call(Method::DELETE, &format!("/api/runs/{id}"), None)
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(r.body.is_empty());
    app.get(&format!("/api/runs/{id}"))
        .await
        .assert_error(StatusCode::NOT_FOUND);
    assert_eq!(app.get_json("/api/runs").await.as_array().unwrap().len(), 6);
    let groups = app.get_json("/api/groups").await;
    assert!(groups
        .as_array()
        .unwrap()
        .iter()
        .all(|g| g["group"] != "x86-std"));
    assert_eq!(app.get_json("/api/health").await["runs"], 6);
    // values are gone too
    let n: i64 = app.with_store(|s| {
        s.conn()
            .query_row(
                "SELECT COUNT(*) FROM results WHERE run_id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap()
    });
    assert_eq!(n, 0);
    // again: 404
    app.call(Method::DELETE, &format!("/api/runs/{id}"), None)
        .await
        .assert_error(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn upload_is_left_to_c2() {
    let app = App::new();
    let r = app.call(Method::POST, "/api/runs", None).await;
    assert_eq!(r.status, StatusCode::NOT_IMPLEMENTED);
}
