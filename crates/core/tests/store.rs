//! References (group subsets, ambiguous runs), the one-metric values query and the layout 1 -> 2 migration.

use benchlens_core::compare::{self, Reference};
use benchlens_core::import::ImportOptions;
use benchlens_core::store::{Store, DB_VERSION};
use benchlens_core::testing::{fixture_store, import_fixtures, poc_registry};
use benchlens_core::Error;
use std::collections::BTreeSet;

fn labels(store: &Store, run_ids: &[String]) -> BTreeSet<String> {
    run_ids
        .iter()
        .map(|id| store.run_names(id).unwrap().0)
        .collect()
}

/// ValueRow and Side have no PartialEq (benchlens-rules); compare what the API would send.
fn json<T: serde::Serialize>(x: &T) -> serde_json::Value {
    serde_json::to_value(x).unwrap()
}

fn set(xs: &[&str]) -> BTreeSet<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parse_references() {
    assert_eq!(
        Reference::parse("x86-high").unwrap(),
        Reference::Run("x86-high")
    );
    assert_eq!(
        Reference::parse("group:x86-high").unwrap(),
        Reference::Group {
            name: "x86-high",
            tag: None
        }
    );
    assert_eq!(
        Reference::parse("group:x86-high@round=1").unwrap(),
        Reference::Group {
            name: "x86-high",
            tag: Some(("round", "1"))
        }
    );
    // the value is everything after the first `=`, matched as it is
    assert_eq!(
        Reference::parse("group:g@note=a=b %_").unwrap(),
        Reference::Group {
            name: "g",
            tag: Some(("note", "a=b %_"))
        }
    );
    for bad in [
        "",
        " ",
        "group:",
        "group: ",
        "group:@round=1",
        "group:g@",
        "group:g@round",
        "group:g@round=",
        "group:g@=1",
        "group:g@ro und=1",
        "group:g@round=1@set=x",
        "group:g@round=1@",
    ] {
        match Reference::parse(bad) {
            Err(Error::BadReference(_)) => {}
            other => panic!("{bad:?}: {other:?}"),
        }
    }
}

#[test]
fn group_subset_pools_runs_with_the_tag() {
    let store = fixture_store(true);
    let r1 = compare::resolve(&store, "group:x86-high@round=1").unwrap();
    assert_eq!(r1.reference, "group:x86-high@round=1");
    assert_eq!(r1.label, "x86-high · round=1");
    assert_eq!(
        labels(&store, &r1.run_ids),
        set(&["x86-high", "x86-high-web"])
    );
    let r2 = compare::resolve(&store, "group:x86-high@round=2").unwrap();
    assert_eq!(labels(&store, &r2.run_ids), set(&["x86-high-r2"]));
    let all = compare::resolve(&store, "group:x86-high").unwrap();
    assert_eq!(all.run_ids.len(), 3);
    assert_eq!(all.label, "x86-high");

    // the subset's values are the group's values of those runs
    let (_, v1) = compare::load(&store, "group:x86-high@round=1").unwrap();
    let direct = store.values(&r1.run_ids).unwrap();
    assert_eq!(json(&v1), json(&direct));

    let c = compare::compare(
        &store,
        "group:x86-high@round=1",
        &["group:x86-high@round=2".to_string()],
    )
    .unwrap();
    assert_eq!(c.base.run_ids, r1.run_ids);
    assert!(!c.rows.is_empty());
}

#[test]
fn group_subset_matches_exactly() {
    let store = fixture_store(true);
    // values are not patterns and not trimmed
    for sel in [
        "round=%", "round=_", "round= 1", "round=1 ", "ROUND=1", "round=01",
    ] {
        match compare::resolve(&store, &format!("group:x86-high@{sel}")) {
            Err(Error::NotFound(msg)) => {
                assert!(msg.contains("none of the 3 runs"), "{sel}: {msg}")
            }
            other => panic!("{sel}: {other:?}"),
        }
    }
    match compare::resolve(&store, "group:nope@round=1") {
        Err(Error::NotFound(msg)) => assert!(msg.contains("no runs in group"), "{msg}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn ambiguous_reference() {
    let store = fixture_store(true);
    store
        .conn()
        .execute(
            "UPDATE runs SET label = 'twin' WHERE label IN ('x86-high-r2', 'x86-high-web')",
            [],
        )
        .unwrap();
    match store.find_run("twin") {
        Err(Error::Ambiguous(msg)) => assert!(msg.contains("matches 2 runs"), "{msg}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        compare::resolve(&store, "twin"),
        Err(Error::Ambiguous(_))
    ));
    assert!(matches!(store.find_run("nope"), Err(Error::NotFound(_))));
}

#[test]
fn metric_values_equal_the_filtered_side() {
    let store = fixture_store(true);
    for reference in [
        "group:x86-high",
        "group:x86-high@round=1",
        "group:arm64-high",
        "x86-std-r2",
    ] {
        let (side, all) = compare::load(&store, reference).unwrap();
        let ids: BTreeSet<&str> = all.iter().map(|v| v.id.as_str()).collect();
        assert!(ids.len() > 10, "{reference}");
        for id in ids.iter().copied().chain(["no.such.metric"]) {
            let (side2, one) = compare::load_metric(&store, reference, id).unwrap();
            assert_eq!(json(&side2), json(&side));
            let expected: Vec<_> = all.iter().filter(|v| v.id == id).cloned().collect();
            assert_eq!(json(&one), json(&expected), "{reference} {id}");
        }
    }
}

/// `v_runs` and `v_values` of layout 1 as they were created (v_stats did not change).
const V1_VIEWS: &str = r#"
DROP VIEW IF EXISTS v_stats;
DROP VIEW IF EXISTS v_values;
DROP VIEW IF EXISTS v_runs;
CREATE VIEW IF NOT EXISTS v_runs AS
SELECT r.*,
       COALESCE((SELECT t.value FROM tags t WHERE t.run_id = r.run_id AND t.key = 'group'), r.auto_group) AS grp,
       EXISTS (SELECT 1 FROM metrics m WHERE m.version = ltrim(r.vmbench_version, 'v')) AS catalog_known
FROM runs r;
CREATE VIEW IF NOT EXISTS v_values AS
SELECT v.run_id, r.label, r.grp, r.arch, r.nproc, r.ram_gb, r.cpu_model, r.vmbench_version,
       v.id, COALESCE(m.name, v.metric) AS name,
       substr(v.id, 1, instr(v.id || '.', '.') - 1) AS section,
       CASE WHEN m.unit IS NULL OR m.unit = 'varies' THEN v.unit ELSE m.unit END AS unit,
       m.direction, m.tool, v.round, v.value
FROM results v
JOIN v_runs r ON r.run_id = v.run_id
LEFT JOIN metrics m ON m.version = ltrim(r.vmbench_version, 'v') AND m.id = v.id
WHERE substr(v.id, 1, instr(v.id || '.', '.') - 1) NOT IN (SELECT f.stage FROM failed_stages f WHERE f.run_id = v.run_id)
  AND (',' || replace(COALESCE((SELECT t.value FROM tags t WHERE t.run_id = v.run_id AND t.key = 'exclude'), ''), ' ', '') || ',')
      NOT LIKE ('%,' || substr(v.id, 1, instr(v.id || '.', '.') - 1) || ',%');
CREATE VIEW IF NOT EXISTS v_stats AS
SELECT run_id, label, grp, id, name, section, unit, direction,
       COUNT(*) AS n, AVG(value) AS mean, MIN(value) AS min, MAX(value) AS max
FROM v_values GROUP BY run_id, id;
PRAGMA user_version = 1;
"#;

/// Sections of the values that count for a run, through `v_values`.
fn sections(store: &Store, run_id: &str) -> BTreeSet<String> {
    store
        .values(&[run_id.to_string()])
        .unwrap()
        .into_iter()
        .map(|v| v.section)
        .collect()
}

#[test]
fn layout_1_database_is_migrated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v1.db");
    let run_id;
    {
        // a layout 1 file: the tables (unchanged since) with the fixtures, and the views of layout 1
        let store = Store::open(&path).unwrap();
        import_fixtures(
            &store,
            &ImportOptions {
                registry: Some(poc_registry()),
                ..Default::default()
            },
        );
        run_id = store.find_run("x86-std-r2").unwrap();
        let c = store.conn();
        c.execute_batch(V1_VIEWS).unwrap();
        // sections with LIKE wildcards in their names, and an exclude list that names neither of them
        for (id, metric) in [("w_b.x", "x"), ("%.y", "y"), ("pg.z", "z")] {
            c.execute(
                "INSERT INTO results (run_id, id, metric, round, value, unit) VALUES (?1, ?2, ?3, 1, 1.0, 'n')",
                [run_id.as_str(), id, metric],
            )
            .unwrap();
        }
        c.execute(
            "INSERT INTO tags (run_id, key, value) VALUES (?1, 'exclude', 'wxb, pg')",
            [run_id.as_str()],
        )
        .unwrap();
        // layout 1 gets it wrong: `w_b` matches `wxb` and `%` matches anything
        let s = sections(&store, &run_id);
        assert!(!s.contains("w_b") && !s.contains("%"), "{s:?}");
        assert!(!s.contains("pg"));
        let v: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
    }
    let runs_before = 7;
    let store = Store::open(&path).unwrap();
    let v: i64 = store
        .conn()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(v, DB_VERSION);
    assert_eq!(store.run_count().unwrap(), runs_before, "data untouched");
    let s = sections(&store, &run_id);
    assert!(s.contains("w_b") && s.contains("%"), "{s:?}");
    assert!(!s.contains("pg"), "listed stages still left out");
    assert!(s.contains("cpu"));
    // the registry's exclude=web on round 1 still works after the migration
    let x86_high = store.find_run("x86-high").unwrap();
    let s = sections(&store, &x86_high);
    assert!(!s.contains("web") && s.contains("cpu"), "{s:?}");
    drop(store);
    // opening again is a no-op
    let store = Store::open(&path).unwrap();
    assert_eq!(store.run_count().unwrap(), runs_before);
}

#[test]
fn exclude_matches_whole_names_only() {
    let store = fixture_store(false);
    let run_id = store.find_run("x86-std-r2").unwrap();
    let c = store.conn();
    for (id, metric) in [
        ("w_b.x", "x"),
        ("wxb.x", "x"),
        ("%.y", "y"),
        ("pg_extra.z", "z"),
    ] {
        c.execute(
            "INSERT INTO results (run_id, id, metric, round, value, unit) VALUES (?1, ?2, ?3, 1, 1.0, 'n')",
            [run_id.as_str(), id, metric],
        )
        .unwrap();
    }
    let mut tags = std::collections::BTreeMap::new();
    tags.insert("exclude".to_string(), "wxb , pg".to_string());
    store.set_tags(&run_id, &tags).unwrap();
    let s = sections(&store, &run_id);
    assert!(!s.contains("wxb") && !s.contains("pg"), "{s:?}");
    assert!(
        s.contains("w_b") && s.contains("%") && s.contains("pg_extra"),
        "{s:?}"
    );
    // a `%` in the list is a name too, not a wildcard
    tags.insert("exclude".to_string(), "%".to_string());
    store.set_tags(&run_id, &tags).unwrap();
    let s = sections(&store, &run_id);
    assert!(
        !s.contains("%") && s.contains("pg") && s.contains("wxb"),
        "{s:?}"
    );
}

#[test]
fn newer_layout_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.db");
    drop(Store::open(&path).unwrap());
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", DB_VERSION + 1)
            .unwrap();
    }
    match Store::open(&path) {
        Err(Error::Contract(msg)) => assert!(msg.contains("newer"), "{msg}"),
        Err(e) => panic!("{e}"),
        Ok(_) => panic!("opened a newer layout"),
    }
}

/// A read-only reader (Grafana on a `:ro` mount) can open the database after benchlens closed it: `-wal` and
/// `-shm` stay (SQLITE_FCNTL_PERSIST_WAL), so the reader need not create them.
#[test]
fn read_only_reader_works_after_benchlens_closed_the_database() {
    use rusqlite::{Connection, OpenFlags};
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("benchlens.db");
    let runs = {
        let store = Store::open(&db).unwrap();
        import_fixtures(&store, &ImportOptions::default());
        store.list_runs().unwrap().len()
    };
    assert!(runs > 0);
    for ext in ["db-wal", "db-shm"] {
        assert!(
            dir.path().join(format!("benchlens.{ext}")).exists(),
            "{ext} kept"
        );
    }
    // the WAL was checkpointed on close: the database file alone has every run
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let ro = |p: &std::path::Path, mode| {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap()
        };
        for f in ["benchlens.db", "benchlens.db-wal", "benchlens.db-shm"] {
            ro(&dir.path().join(f), 0o444);
        }
        ro(dir.path(), 0o555);
    }
    let uri = format!("file:{}?mode=ro", db.display());
    let conn = Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    let n: usize = conn
        .query_row("SELECT COUNT(*) FROM v_runs", [], |r| r.get(0))
        .unwrap();
    let values: i64 = conn
        .query_row("SELECT COUNT(*) FROM v_stats", [], |r| r.get(0))
        .unwrap();
    drop(conn);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(n, runs);
    assert!(values > 0);
}
