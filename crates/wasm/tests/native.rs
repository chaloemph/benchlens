//! compare_json (what the wasm export runs) gives the same JSON as benchlens-core's compare.

use benchlens_core::compare;
use benchlens_core::import::{import_path, ImportOptions};
use benchlens_core::store::Store;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn compare_json_matches_core_compare() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let store = Store::open_in_memory().unwrap();
    for ver in ["v0.2.1", "v0.2.3"] {
        import_path(&store, &fixtures.join(ver), &ImportOptions::default()).unwrap();
    }
    let runs = store.list_runs().unwrap();
    assert!(runs.len() >= 3);
    let base = runs[0].run_id.clone();
    let others: Vec<String> = runs[1..3].iter().map(|r| r.run_id.clone()).collect();
    let (bs, bv) = compare::load(&store, &base).unwrap();
    let sides: Vec<_> = others
        .iter()
        .map(|o| {
            let (s, v) = compare::load(&store, o).unwrap();
            json!({"side": s, "values": v})
        })
        .collect();
    let input = json!({"base": {"side": bs, "values": bv}, "others": sides}).to_string();
    let ours = benchlens_wasm::compare_json(&input).unwrap();
    let native = serde_json::to_string(&compare::compare(&store, &base, &others).unwrap()).unwrap();
    assert_eq!(ours, native);
}

#[test]
fn bad_input_is_an_error() {
    assert!(benchlens_wasm::compare_json("{").is_err());
    let no_others =
        r#"{"base":{"side":{"reference":"a","label":"a","run_ids":[]},"values":[]},"others":[]}"#;
    assert!(benchlens_wasm::compare_json(no_others).is_err());
}
