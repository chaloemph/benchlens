//! Writes docs/api-examples/*.json: the JSON shapes of API v1, produced by the real code on the test
//! fixtures (long lists cut to a few entries). Run from the repo root: cargo run -p benchlens-core --example api_examples

use benchlens_core::compare;
use benchlens_core::import::ImportOptions;
use benchlens_core::store::Store;
use benchlens_core::testing::{import_fixtures, poc_registry};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

fn cut(mut v: Value, keep: usize) -> Value {
    if let Value::Array(a) = &mut v {
        let n = a.len();
        a.truncate(keep);
        if n > keep {
            a.push(json!(format!("... {} more", n - keep)));
        }
    }
    v
}

fn write(name: &str, v: &Value) {
    let p = Path::new("docs/api-examples").join(format!("{name}.json"));
    fs::write(&p, serde_json::to_string_pretty(v).unwrap() + "\n").unwrap();
    println!("wrote {}", p.display());
}

fn main() {
    let store = Store::open_in_memory().unwrap();
    let opts = ImportOptions {
        registry: Some(poc_registry()),
        ..Default::default()
    };
    let imported = import_fixtures(&store, &opts);
    write(
        "import-report",
        &cut(serde_json::to_value(&imported).unwrap(), 2),
    );

    let runs = store.list_runs().unwrap();
    write("runs", &cut(serde_json::to_value(&runs).unwrap(), 2));

    let r2 = runs.iter().find(|r| r.label == "x86-high-r2").unwrap();
    let mut detail = serde_json::to_value(store.run_detail(&r2.run_id).unwrap()).unwrap();
    // meta (an object, meta.json as stored) and stages (a string) are shown as they are; hardware.json is cut
    // to its cpu part (docs/api.md says so)
    if let Some(hw) = detail["hardware"].as_object_mut() {
        hw.retain(|k, _| k == "cpu" || k == "schema");
        hw.insert(
            "...".into(),
            json!("memory, nics, disks, ... as in hardware.json"),
        );
    }
    write("run-detail", &detail);

    write(
        "groups",
        &serde_json::to_value(store.list_groups().unwrap()).unwrap(),
    );

    let c = compare::compare(
        &store,
        "group:x86-high",
        &["group:arm64-high".to_string(), "x86-std-r2".to_string()],
    )
    .unwrap();
    let mut cv = serde_json::to_value(&c).unwrap();
    cv["rows"] = cut(cv["rows"].take(), 3);
    write("compare", &cv);

    let (side, values) = compare::load(&store, "group:x86-high").unwrap();
    let values: Vec<_> = values
        .into_iter()
        .filter(|v| v.id == "cpu.7z.rating.max")
        .collect();
    write(
        "values",
        &json!({ "side": side, "id": "cpu.7z.rating.max", "values": values }),
    );

    let m = store.metrics("0.2.3").unwrap();
    write("metrics", &cut(serde_json::to_value(&m).unwrap(), 3));

    write(
        "health",
        &json!({ "benchlens": env!("CARGO_PKG_VERSION"), "db_layout": benchlens_core::store::DB_VERSION, "runs": runs.len() }),
    );
    write(
        "error",
        &json!({ "error": "no run matches \"nope\" (try `benchlens runs`)" }),
    );
}
