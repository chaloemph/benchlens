//! The static HTML export (`GET /api/export.html`, `benchlens export html`) against the test fixtures.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use benchlens_core::compare;
use benchlens_core::store::Store;
use benchlens_core::testing::{fixture_store, repo_root};
use benchlens_server::export::{base64, collect, render_html, CSP};
use benchlens_server::{router, AppState};
use http_body_util::BodyExt;
use serde_json::Value;
use std::collections::BTreeMap;
use tower::ServiceExt;

const X86: &str = "group:x86-high";
const ARM: &str = "group:arm64-high";
const EVIL: &str = "</script><img src=x onerror=alert(1)><!--<script>\u{2028}\u{2029}&amp;";

struct Res {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

async fn get(state: &AppState, path: &str) -> Res {
    let req = Request::builder().uri(path).body(Body::empty()).unwrap();
    let res = router(state.clone()).oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let body = res.into_body().collect().await.unwrap().to_bytes();
    Res {
        status,
        headers,
        body: String::from_utf8(body.to_vec()).unwrap(),
    }
}

fn state(store: Store) -> AppState {
    AppState::new(store)
}

/// The text of the one element `<script ... id="ID">...</script>`.
fn script_by_id<'a>(html: &'a str, id: &str) -> &'a str {
    let open = format!("id=\"{id}\">");
    let start = html.find(&open).unwrap_or_else(|| panic!("no #{id}")) + open.len();
    let len = html[start..].find("</script>").unwrap();
    &html[start..start + len]
}

fn data_of(html: &str) -> Value {
    serde_json::from_str(script_by_id(html, "benchlens-data")).expect("inlined data is JSON")
}

/// Every place where an element ends or a comment starts, as the HTML tokenizer would see them.
fn closers(html: &str) -> (usize, usize, usize) {
    let l = html.to_ascii_lowercase();
    (
        l.matches("</script").count(),
        l.matches("<script").count(),
        l.matches("<!--").count(),
    )
}

/// The page as `benchlens export html` writes it (same code as the endpoint).
fn export(store: &Store, base: &str, others: &[&str], title: Option<&str>) -> String {
    let others: Vec<String> = others.iter().map(|s| s.to_string()).collect();
    render_html(&collect(store, base, &others, "test.db", title).unwrap()).unwrap()
}

#[test]
fn page_inlines_data_wasm_and_code() {
    let store = fixture_store(true);
    let html = export(&store, X86, &[ARM], None);
    assert!(html.starts_with("<!doctype html>"));
    // nothing is loaded from anywhere
    assert!(!html.contains(" src=\""), "a script or image loads a file");
    assert!(!html.contains("<link"), "a stylesheet loads a file");
    assert!(html.contains(&format!(
        "<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">"
    )));
    // the WASM build, base64
    let wasm = std::fs::read(repo_root().join("web/wasm/benchlens_wasm_bg.wasm")).unwrap();
    assert_eq!(script_by_id(&html, "benchlens-wasm"), base64(&wasm));
    // the live UI's views and notes, the CSS, d3
    for needle in [
        "id=\"view-compare\"",
        "id=\"view-metric\"",
        "Each cell is the difference from the baseline",
        "--good:",
        "d3js.org",
        "__benchlens_modules[\"js/compare.js\"]",
        "__benchlens_modules[\"export/offline.js\"]",
    ] {
        assert!(html.contains(needle), "{needle}");
    }
    assert!(html.contains("<title>x86-high vs arm64-high · benchlens</title>"));

    // the data: every side as core::compare::load gives it, the runs and groups of those sides
    let d = data_of(&html);
    assert_eq!(d["format"], 1);
    assert_eq!(d["benchlens"], env!("CARGO_PKG_VERSION"));
    assert_eq!(d["source"], "test.db");
    assert_eq!(
        (d["base"].as_str(), d["others"][0].as_str()),
        (Some(X86), Some(ARM))
    );
    let mut run_ids = Vec::new();
    for (i, r) in [X86, ARM].iter().enumerate() {
        let (side, values) = compare::load(&store, r).unwrap();
        let s = &d["sides"][i];
        assert_eq!(s["side"]["reference"], *r);
        assert_eq!(s["side"]["run_ids"], serde_json::json!(side.run_ids));
        assert_eq!(
            s["values"],
            serde_json::to_value(&values).unwrap(),
            "values of {r}"
        );
        run_ids.extend(side.run_ids);
    }
    let runs: Vec<&str> = d["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["run_id"].as_str().unwrap())
        .collect();
    assert_eq!(runs, run_ids);
    for r in d["runs"].as_array().unwrap() {
        assert!(r["hardware"].is_object(), "hardware of {}", r["label"]);
        assert_eq!(r["files"], serde_json::json!([]));
    }
    let groups: Vec<&str> = d["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["group"].as_str().unwrap())
        .collect();
    assert_eq!(groups, ["arm64-high", "x86-high"]);
    // no unrelated runs: the whole fixture set has more
    assert!(store.list_runs().unwrap().len() > runs.len());
}

#[test]
fn subset_sides_carry_their_group() {
    let store = fixture_store(true);
    let html = export(
        &store,
        "group:x86-high@round=1",
        &["group:x86-high@round=2"],
        None,
    );
    let d = data_of(&html);
    let groups: Vec<&str> = d["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["group"].as_str().unwrap())
        .collect();
    assert_eq!(groups, ["x86-high"]);
    assert_eq!(d["sides"][0]["side"]["label"], "x86-high · round=1");
}

/// What the page computes (the inlined sides through the verdict rule of the WASM crate, natively) is byte for
/// byte what `benchlens compare --json` computes.
#[test]
fn inlined_sides_give_the_cli_comparison() {
    let store = fixture_store(true);
    for (base, others) in [
        (X86, vec![ARM]),
        (ARM, vec![X86]),
        ("group:x86-std", vec![X86, ARM]),
    ] {
        let html = export(&store, base, &others, None);
        let d = data_of(&html);
        let side = |s: &Value| serde_json::json!({"side": s["side"], "values": s["values"]});
        let sides = d["sides"].as_array().unwrap();
        let input = serde_json::json!({
            "base": side(&sides[0]),
            "others": sides[1..].iter().map(side).collect::<Vec<_>>(),
        });
        let page = benchlens_wasm::compare_json(&input.to_string()).unwrap();
        let others: Vec<String> = others.iter().map(|s| s.to_string()).collect();
        let cli = serde_json::to_string(&compare::compare(&store, base, &others).unwrap()).unwrap();
        assert_eq!(page, cli, "{base} vs {others:?}");
    }
}

#[test]
fn hostile_labels_and_titles_cannot_break_out() {
    let store = fixture_store(true);
    let arm = store.find_run("arm64-high-r2").unwrap();
    store
        .set_tags(
            &arm,
            &BTreeMap::from([
                ("group".to_string(), EVIL.to_string()),
                ("note".to_string(), EVIL.to_string()),
            ]),
        )
        .unwrap();
    let evil_ref = format!("group:{EVIL}");
    let clean = export(&store, X86, &[ARM], None);
    let html = export(&store, X86, &[&evil_ref], Some(EVIL));
    // the hostile text adds no element end, no script start and no comment anywhere in the page
    assert_eq!(closers(&html), closers(&clean));
    let data = script_by_id(&html, "benchlens-data");
    assert!(!data.contains(['<', '>', '&', '\u{2028}', '\u{2029}']));
    // and comes back unchanged
    let d = data_of(&html);
    assert_eq!(d["title"], EVIL);
    assert_eq!(d["sides"][1]["side"]["label"], EVIL);
    assert_eq!(d["others"][0], evil_ref.as_str());
    let run = d["runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["run_id"] == arm.as_str())
        .unwrap();
    assert_eq!(run["tags"]["note"], EVIL);
    assert_eq!(run["group"], EVIL);
    // the title element holds escaped text only
    assert!(html.contains(
        "<title>&lt;/script&gt;&lt;img src=x onerror=alert(1)&gt;&lt;!--&lt;script&gt;\u{2028}\u{2029}&amp;amp; · benchlens</title>"
    ));
}

#[tokio::test]
async fn endpoint_downloads_the_same_page() {
    let st = state(fixture_store(true));
    let r = get(
        &st,
        "/api/export.html?base=group:x86-high&other=group:arm64-high&title=POC%20report",
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let h = |k: header::HeaderName| r.headers.get(k).unwrap().to_str().unwrap().to_string();
    assert_eq!(h(header::CONTENT_TYPE), "text/html; charset=utf-8");
    assert_eq!(
        h(header::CONTENT_DISPOSITION),
        "attachment; filename=\"benchlens-x86-high-vs-arm64-high.html\""
    );
    assert_eq!(h(header::CONTENT_SECURITY_POLICY), CSP);
    assert_eq!(h(header::X_CONTENT_TYPE_OPTIONS), "nosniff");
    let d = data_of(&r.body);
    assert_eq!(d["title"], "POC report");
    assert_eq!(d["source"], "benchlens server");
    // same page as the CLI apart from the time and source
    let cli = {
        let store = st.store.lock().unwrap();
        export(&store, X86, &[ARM], Some("POC report"))
    };
    let mut cli_d = data_of(&cli);
    cli_d["exported_at"] = d["exported_at"].clone();
    cli_d["source"] = d["source"].clone();
    assert_eq!(cli_d, d);
    let strip = |s: &str| s[..s.find("id=\"benchlens-data\"").unwrap()].to_string();
    assert_eq!(strip(&cli), strip(&r.body));
}

#[tokio::test]
async fn endpoint_errors() {
    let st = state(fixture_store(true));
    for (q, status) in [
        ("other=group:x86-high", StatusCode::BAD_REQUEST),
        ("base=group:x86-high", StatusCode::BAD_REQUEST),
        (
            "base=group:x86-high&other=group:arm64-high&x=1",
            StatusCode::BAD_REQUEST,
        ),
        (
            "base=group:x86-high&base=group:x86-med&other=group:arm64-high",
            StatusCode::BAD_REQUEST,
        ),
        (
            "base=group:&other=group:arm64-high",
            StatusCode::BAD_REQUEST,
        ),
        (
            "base=group:x86-high&other=group:arm64-high&title=a%0Ab",
            StatusCode::BAD_REQUEST,
        ),
        (
            "base=group:x86-high&other=group:nope",
            StatusCode::NOT_FOUND,
        ),
        ("base=nope&other=group:arm64-high", StatusCode::NOT_FOUND),
    ] {
        let r = get(&st, &format!("/api/export.html?{q}")).await;
        assert_eq!(r.status, status, "{q}: {}", r.body);
        let v: Value = serde_json::from_str(&r.body).unwrap();
        assert!(v["error"].as_str().is_some_and(|e| !e.is_empty()), "{q}");
    }
    let many: String = (0..33).map(|_| "&other=group:arm64-high").collect();
    let r = get(&st, &format!("/api/export.html?base=group:x86-high{many}")).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}
