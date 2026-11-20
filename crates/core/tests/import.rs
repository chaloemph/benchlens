//! Importing: the contract checks, archive safety, duplicates, tags and stage exclusion.

mod common;
use benchlens_core::import::{import_path, read_tar, ImportOptions, Outcome, Registry};
use benchlens_core::store::Store;
use benchlens_core::testing::import_fixtures;
use benchlens_core::Error;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const R2: &str = "v0.2.3/x86-high-r2-20261119-145026";

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
}

fn tar_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for (path, data) in entries {
        let mut h = tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, path, *data).unwrap();
    }
    b.into_inner().unwrap()
}

#[test]
fn import_twice_does_not_duplicate() {
    let store = Store::open_in_memory().unwrap();
    let p = common::fixture(R2);
    let a = import_path(&store, &p, &ImportOptions::default()).unwrap();
    assert_eq!(a[0].outcome, Outcome::Imported);
    let b = import_path(&store, &p, &ImportOptions::default()).unwrap();
    assert_eq!(b[0].outcome, Outcome::AlreadyPresent);
    assert_eq!(a[0].run_id, b[0].run_id);
    let n: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn unknown_schema_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("future-run");
    copy_dir(&common::fixture(R2), &dir);
    let meta = fs::read_to_string(dir.join("meta.json"))
        .unwrap()
        .replace("\"schema\": 1", "\"schema\": 2");
    assert!(meta.contains("\"schema\": 2"), "fixture layout changed");
    fs::write(dir.join("meta.json"), meta).unwrap();
    let store = Store::open_in_memory().unwrap();
    let r = import_path(&store, &dir, &ImportOptions::default()).unwrap();
    assert_eq!(r[0].outcome, Outcome::Failed);
    assert!(
        r[0].message.contains("schema 2 is not supported"),
        "{}",
        r[0].message
    );
}

#[test]
fn missing_files_are_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("half");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("meta.json"), "{\"schema\": 1}").unwrap();
    let store = Store::open_in_memory().unwrap();
    let r = import_path(&store, &dir, &ImportOptions::default()).unwrap();
    assert_eq!(r[0].outcome, Outcome::Failed);
    assert!(r[0].message.contains("results.tsv"));
}

#[test]
fn archive_with_parent_path_is_refused() {
    // tar::Builder refuses `..`, so write the name bytes into the header directly, as a hostile archive would
    let mut h = tar::Header::new_old();
    let name = b"../evil/meta.json";
    h.as_old_mut().name[..name.len()].copy_from_slice(name);
    h.set_size(2);
    h.set_mode(0o644);
    h.set_cksum();
    let mut t = h.as_bytes().to_vec();
    t.extend_from_slice(b"{}");
    t.resize(512 * 2, 0);
    t.extend_from_slice(&[0u8; 1024]);
    match read_tar(&t[..], "x") {
        Err(Error::UnsafeArchive { .. }) => {}
        other => panic!("expected refusal, got {other:?}"),
    }
}

#[test]
fn archive_with_symlink_is_refused() {
    let mut b = tar::Builder::new(Vec::new());
    let mut h = tar::Header::new_gnu();
    h.set_entry_type(tar::EntryType::Symlink);
    h.set_size(0);
    b.append_link(&mut h, "run/meta.json", "/etc/passwd")
        .unwrap();
    let t = b.into_inner().unwrap();
    assert!(matches!(
        read_tar(&t[..], "x"),
        Err(Error::UnsafeArchive { .. })
    ));
}

#[test]
fn archive_round_trip_matches_folder() {
    let dir = common::fixture(R2);
    let mut entries = Vec::new();
    for name in ["meta.json", "results.tsv", "status.tsv", "hardware.json"] {
        entries.push((
            format!("bench-results/x86-high-r2-20261119-145026/{name}"),
            fs::read(dir.join(name)).unwrap(),
        ));
    }
    entries.push((
        "bench-results/x86-high-r2-20261119-145026/run.log".into(),
        b"ignored".to_vec(),
    ));
    let refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(p, d)| (p.as_str(), d.as_slice()))
        .collect();
    let tmp = tempfile::tempdir().unwrap();
    let tgz = tmp.path().join("run.tar.gz");
    let mut gz =
        flate2::write::GzEncoder::new(fs::File::create(&tgz).unwrap(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &tar_of(&refs)).unwrap();
    gz.finish().unwrap();

    let store = Store::open_in_memory().unwrap();
    let from_tar = import_path(&store, &tgz, &ImportOptions::default()).unwrap();
    assert_eq!(from_tar[0].outcome, Outcome::Imported, "{from_tar:?}");
    assert_eq!(from_tar[0].folder, "x86-high-r2-20261119-145026");
    let from_dir = import_path(&store, &dir, &ImportOptions::default()).unwrap();
    assert_eq!(
        from_dir[0].outcome,
        Outcome::AlreadyPresent,
        "same run by another route"
    );
    let files: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .unwrap();
    assert_eq!(files, 4, "run.log is not kept");
    assert_eq!(
        store.list_runs().unwrap()[0].kept_files,
        ["hardware.json", "meta.json", "results.tsv", "status.tsv"],
        "kept file names, sorted"
    );
}

#[test]
fn tags_registry_and_exclude() {
    let store = Store::open_in_memory().unwrap();
    let reg = Registry::parse(
        "run\tgroup\tround\texclude\nx86-high-r2-20261119-145026\tpoc-x86-high\t2\tweb,net\n",
    )
    .unwrap();
    let mut cli = BTreeMap::new();
    cli.insert("round".to_string(), "two".to_string()); // the command line wins over the registry
    let opts = ImportOptions {
        registry: Some(reg),
        tags: cli,
        ..Default::default()
    };
    let r = import_path(&store, &common::fixture(R2), &opts).unwrap();
    let id = r[0].run_id.clone().unwrap();
    let runs = store.list_runs().unwrap();
    assert_eq!(runs[0].group, "poc-x86-high");
    assert_eq!(runs[0].tags["round"], "two");
    let sections: Vec<String> = store
        .values(&[id])
        .unwrap()
        .into_iter()
        .map(|v| v.section)
        .collect();
    assert!(
        !sections.iter().any(|s| s == "web" || s == "net"),
        "excluded stages left out"
    );
    assert!(sections.iter().any(|s| s == "pg"));
}

#[test]
fn auto_group_joins_rounds_of_one_machine() {
    let store = Store::open_in_memory().unwrap();
    import_fixtures(&store, &ImportOptions::default());
    let runs = store.list_runs().unwrap();
    let group_of = |label: &str| {
        runs.iter()
            .find(|r| r.label == label)
            .unwrap()
            .group
            .clone()
    };
    assert_eq!(group_of("x86-high"), group_of("x86-high-r2"));
    assert_eq!(group_of("x86-high"), group_of("x86-high-web"));
    assert_ne!(group_of("x86-high"), group_of("arm64-high"));
    assert_ne!(group_of("x86-high"), group_of("x86-std-r2"));
    assert!(
        group_of("arm64-high").contains("HiSilicon"),
        "{}",
        group_of("arm64-high")
    );
}

#[test]
fn bad_tag_key_is_refused() {
    let store = Store::open_in_memory().unwrap();
    let mut tags = BTreeMap::new();
    tags.insert("bad key!".to_string(), "x".to_string());
    let r = import_path(
        &store,
        &common::fixture(R2),
        &ImportOptions {
            tags,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r[0].outcome, Outcome::Failed);
    let n: i64 = store
        .conn()
        .query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0, "nothing half-written");
}
