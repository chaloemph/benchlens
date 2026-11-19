//! Write the input of the wasm `compare` for every pair in tests/golden/compare.json, taken from the fixtures
//! imported with benchlens-core exactly as `benchlens compare` would, plus the native Comparison JSON.
//! Used by tests/wasm/golden.mjs (scripts/test-wasm.sh):
//!
//!     cargo run -p benchlens-wasm --example golden_inputs -- OUT.json

use benchlens_core::compare;
use benchlens_core::import::{import_path, ImportOptions};
use benchlens_core::store::Store;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: golden_inputs OUT.json");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let store = Store::open_in_memory().unwrap();
    for ver in ["v0.2.1", "v0.2.3"] {
        let reports = import_path(
            &store,
            &root.join("fixtures").join(ver),
            &ImportOptions::default(),
        )
        .unwrap();
        assert!(reports.iter().all(|r| r.run_id.is_some()), "{reports:?}");
    }
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("golden/compare.json")).unwrap())
            .unwrap();
    let pairs: BTreeSet<(String, String)> = golden["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["base"].as_str().unwrap().to_string(),
                r["other"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let mut cases = Vec::new();
    for (base, other) in &pairs {
        let (bs, bv) = compare::load(&store, base).unwrap();
        let (os, ov) = compare::load(&store, other).unwrap();
        let input = json!({
            "base": {"side": bs, "values": bv},
            "others": [{"side": os, "values": ov}],
        });
        let native = compare::compare(&store, base, std::slice::from_ref(other)).unwrap();
        cases.push(json!({
            "base": base,
            "other": other,
            "input": input,
            "native": serde_json::to_string(&native).unwrap(),
        }));
    }
    std::fs::write(
        &out,
        serde_json::to_string(&json!({ "cases": cases })).unwrap(),
    )
    .unwrap();
    eprintln!("{} pairs written to {out}", cases.len());
}
