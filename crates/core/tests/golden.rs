//! benchlens must judge every metric exactly as `vmbench report compare` does.
//! tests/golden/compare.json is written by tools/golden.py from vmbench's own report.py.

mod common;
use benchlens_core::compare;
use benchlens_core::import::ImportOptions;
use benchlens_core::store::Store;
use benchlens_core::testing::import_fixtures;
use serde_json::Value;
use std::collections::BTreeMap;

#[test]
fn verdicts_match_vmbench_report_compare() {
    let store = Store::open_in_memory().unwrap();
    let reports = import_fixtures(&store, &ImportOptions::default());
    assert!(reports.iter().all(|r| r.run_id.is_some()), "{reports:?}");
    let golden: Value = serde_json::from_str(
        &std::fs::read_to_string(common::fixtures().join("../golden/compare.json")).unwrap(),
    )
    .unwrap();
    // group the golden rows by pair
    let mut pairs: BTreeMap<(String, String), Vec<&Value>> = BTreeMap::new();
    for r in golden["rows"].as_array().unwrap() {
        pairs
            .entry((
                r["base"].as_str().unwrap().into(),
                r["other"].as_str().unwrap().into(),
            ))
            .or_default()
            .push(r);
    }
    let mut checked = 0;
    for ((base, other), rows) in &pairs {
        let c = compare::compare(&store, base, std::slice::from_ref(other)).unwrap();
        let ours: BTreeMap<&str, &compare::Row> =
            c.rows.iter().map(|r| (r.id.as_str(), r)).collect();
        let judged: Vec<&&Value> = rows.iter().filter(|r| !r["verdict"].is_null()).collect();
        for g in &judged {
            let id = g["id"].as_str().unwrap();
            let row = ours
                .get(id)
                .unwrap_or_else(|| panic!("{base} vs {other}: {id} missing"));
            let gv = g["verdict"].as_i64().unwrap() as i8;
            assert_eq!(row.verdict.as_i8(), gv, "{base} vs {other}: {id} verdict");
            match (g["diff"].as_f64(), row.diff) {
                (Some(a), Some(b)) => assert!(
                    (a - b).abs() < 1e-9 * a.abs().max(1.0),
                    "{id}: diff {a} vs {b}"
                ),
                (None, None) => {}
                (a, b) => panic!("{id}: diff {a:?} vs {b:?}"),
            }
            assert!(
                (g["median_base"].as_f64().unwrap() - row.base).abs()
                    < 1e-9 * row.base.abs().max(1.0),
                "{id} base median"
            );
            checked += 1;
        }
        // nothing judged by benchlens that vmbench did not judge
        assert_eq!(c.rows.len(), judged.len(), "{base} vs {other}: row count");
    }
    assert!(checked > 300, "only {checked} verdicts checked");
}
