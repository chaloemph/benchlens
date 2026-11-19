//! The inbox: stability between scans, moves to done/ and failed/, archives, duplicates, names, links.

mod common;
use benchlens_core::import::Outcome;
use benchlens_core::inbox::{Inbox, InboxEvent, InboxOptions};
use benchlens_core::store::Store;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const STD: &str = "v0.2.3/x86-std-r2-20261119-103253";
const HIGH: &str = "v0.2.3/x86-high-r2-20261119-145026";

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
}

fn setup() -> (tempfile::TempDir, InboxOptions, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let opts = InboxOptions::new(tmp.path().join("inbox"));
    fs::create_dir_all(&opts.dir).unwrap();
    (tmp, opts, Store::open_in_memory().unwrap())
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

fn done(evs: &[InboxEvent]) -> Vec<&InboxEvent> {
    evs.iter()
        .filter(|e| matches!(e, InboxEvent::Done { .. }))
        .collect()
}

fn processed(evs: &[InboxEvent]) -> usize {
    evs.iter()
        .filter(|e| matches!(e, InboxEvent::Done { .. } | InboxEvent::Failed { .. }))
        .count()
}

fn tgz_of(path: &Path, runs: &[&str]) {
    let mut b = tar::Builder::new(Vec::new());
    for rel in runs {
        let dir = common::fixture(rel);
        let folder = dir.file_name().unwrap().to_string_lossy().to_string();
        for e in fs::read_dir(&dir).unwrap() {
            let e = e.unwrap();
            let data = fs::read(e.path()).unwrap();
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            let name = format!("bench-results/{folder}/{}", e.file_name().to_string_lossy());
            b.append_data(&mut h, name, &data[..]).unwrap();
        }
    }
    let mut gz =
        flate2::write::GzEncoder::new(fs::File::create(path).unwrap(), flate2::Compression::fast());
    gz.write_all(&b.into_inner().unwrap()).unwrap();
    gz.finish().unwrap();
}

#[test]
fn folder_is_imported_after_two_scans_and_moved_to_done() {
    let (_tmp, opts, store) = setup();
    let mut inbox = Inbox::new();
    copy_dir(
        &common::fixture(STD),
        &opts.dir.join("x86-std-r2-20261119-103253"),
    );

    let first = inbox.scan_once(&store, &opts);
    assert!(
        matches!(&first[..], [InboxEvent::Waiting { .. }]),
        "{first:?}"
    );
    assert_eq!(store.run_count().unwrap(), 0);

    let second = inbox.scan_once(&store, &opts);
    let d = done(&second);
    assert_eq!(d.len(), 1, "{second:?}");
    let InboxEvent::Done {
        moved_to, reports, ..
    } = d[0]
    else {
        unreachable!()
    };
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].outcome, Outcome::Imported);
    assert!(moved_to.starts_with(opts.dir.join("done")));
    assert!(moved_to.join("meta.json").is_file());
    assert_eq!(store.run_count().unwrap(), 1);
    assert_eq!(store.list_runs().unwrap()[0].source, "inbox");
    assert_eq!(names(&opts.dir), ["done", "failed"]);

    // nothing left to do
    assert!(inbox.scan_once(&store, &opts).is_empty());
    assert_eq!(store.run_count().unwrap(), 1);
}

#[test]
fn growing_file_waits_until_stable() {
    let (_tmp, opts, store) = setup();
    let mut inbox = Inbox::new();
    let full = opts.dir.join("full.tar.gz");
    tgz_of(&full, &[STD]);
    let bytes = fs::read(&full).unwrap();
    fs::remove_file(&full).unwrap();

    let path = opts.dir.join("run.tar.gz");
    let half = bytes.len() / 2;
    fs::write(&path, &bytes[..half]).unwrap();
    let evs = inbox.scan_once(&store, &opts);
    assert_eq!(processed(&evs), 0);

    // still being written: grows between scans
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&bytes[half..half + 10])
        .unwrap();
    assert_eq!(processed(&inbox.scan_once(&store, &opts)), 0);
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&bytes[half + 10..])
        .unwrap();
    assert_eq!(processed(&inbox.scan_once(&store, &opts)), 0);
    assert_eq!(store.run_count().unwrap(), 0);
    assert!(path.is_file());

    // unchanged since the last scan: imported now
    let evs = inbox.scan_once(&store, &opts);
    assert_eq!(done(&evs).len(), 1, "{evs:?}");
    assert_eq!(store.run_count().unwrap(), 1);
}

#[test]
fn broken_folder_goes_to_failed_with_reason() {
    let (_tmp, opts, store) = setup();
    let mut inbox = Inbox::new();
    let dir = opts.dir.join("half");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("meta.json"), "{\"schema\": 1}").unwrap();
    // a stray archive that is not one
    fs::write(opts.dir.join("junk.tar"), b"this is not a tar file at all").unwrap();

    inbox.scan_once(&store, &opts);
    let evs = inbox.scan_once(&store, &opts);
    let failed: Vec<_> = evs
        .iter()
        .filter_map(|e| match e {
            InboxEvent::Failed {
                name,
                moved_to,
                reason_file,
                ..
            } => Some((name.clone(), moved_to.clone(), reason_file.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(failed.len(), 2, "{evs:?}");
    for (name, moved_to, reason_file) in &failed {
        assert!(moved_to.exists(), "{name} moved");
        assert_eq!(
            reason_file,
            &PathBuf::from(format!("{}.reason.txt", moved_to.display()))
        );
        let reason = fs::read_to_string(reason_file).unwrap();
        assert!(!reason.trim().is_empty());
        if name == "half" {
            assert!(reason.contains("results.tsv"), "{reason}");
            assert!(
                !reason.contains(&format!("{name}: {name}:")),
                "folder name repeated: {reason}"
            );
        }
    }
    assert_eq!(names(&opts.dir), ["done", "failed"]);
    assert_eq!(names(&opts.dir.join("failed")).len(), 4);
    assert_eq!(store.run_count().unwrap(), 0);
}

#[test]
fn archive_with_two_runs_imports_both() {
    let (_tmp, opts, store) = setup();
    let mut inbox = Inbox::new();
    tgz_of(&opts.dir.join("both.tgz"), &[STD, HIGH]);
    inbox.scan_once(&store, &opts);
    let evs = inbox.scan_once(&store, &opts);
    let d = done(&evs);
    assert_eq!(d.len(), 1, "{evs:?}");
    let InboxEvent::Done { reports, .. } = d[0] else {
        unreachable!()
    };
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|r| r.outcome == Outcome::Imported));
    assert_eq!(store.run_count().unwrap(), 2);
    let moved = names(&opts.dir.join("done"));
    assert_eq!(moved.len(), 1);
    assert!(moved[0].ends_with("-both.tgz"), "{moved:?}");
}

#[test]
fn same_run_twice_goes_to_done_as_already_present() {
    let (_tmp, opts, store) = setup();
    let mut inbox = Inbox::new();
    let name = "x86-std-r2-20261119-103253";
    copy_dir(&common::fixture(STD), &opts.dir.join(name));
    inbox.scan_once(&store, &opts);
    assert_eq!(done(&inbox.scan_once(&store, &opts)).len(), 1);

    // dropped again under the same name, in the same second: must not overwrite the first
    copy_dir(&common::fixture(STD), &opts.dir.join(name));
    inbox.scan_once(&store, &opts);
    let evs = inbox.scan_once(&store, &opts);
    let d = done(&evs);
    assert_eq!(d.len(), 1, "{evs:?}");
    let InboxEvent::Done { reports, .. } = d[0] else {
        unreachable!()
    };
    assert_eq!(reports[0].outcome, Outcome::AlreadyPresent);
    assert_eq!(store.run_count().unwrap(), 1);
    let moved = names(&opts.dir.join("done"));
    assert_eq!(moved.len(), 2, "{moved:?}");
    for m in &moved {
        assert!(opts.dir.join("done").join(m).join("meta.json").is_file());
    }
}

#[test]
fn names_are_never_overwritten() {
    let (_tmp, opts, store) = setup();
    let mut inbox = Inbox::new();
    // the failed reason file for the first choice of name already exists
    let failed = opts.dir.join("failed");
    fs::create_dir_all(&failed).unwrap();
    let mut bad = Vec::new();
    for i in 0..3 {
        let dir = opts.dir.join(format!("bad{i}"));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("meta.json"), "{\"schema\": 1}").unwrap();
        bad.push(dir);
    }
    inbox.scan_once(&store, &opts);
    inbox.scan_once(&store, &opts);
    let first = names(&failed);
    assert_eq!(first.len(), 6, "{first:?}");

    // drop the same names again and make every obvious target name taken
    for d in &bad {
        fs::create_dir(d).unwrap();
        fs::write(d.join("meta.json"), "{\"schema\": 1}").unwrap();
    }
    inbox.scan_once(&store, &opts);
    inbox.scan_once(&store, &opts);
    let second = names(&failed);
    assert_eq!(second.len(), 12, "{second:?}");
    for f in &first {
        assert!(second.contains(f));
    }
    // every moved folder still has its meta.json
    for f in second.iter().filter(|f| !f.ends_with(".reason.txt")) {
        assert!(failed.join(f).join("meta.json").is_file(), "{f}");
        assert!(second.contains(&format!("{f}.reason.txt")), "{f}");
    }
}

#[cfg(unix)]
#[test]
fn symlinks_dotfiles_and_partials_are_ignored() {
    let (tmp, opts, store) = setup();
    let mut inbox = Inbox::new();
    let outside = tmp.path().join("outside");
    copy_dir(&common::fixture(STD), &outside);
    std::os::unix::fs::symlink(&outside, opts.dir.join("link")).unwrap();
    copy_dir(&common::fixture(HIGH), &opts.dir.join(".hidden"));
    tgz_of(&opts.dir.join("upload.tar.gz.part"), &[STD]);
    fs::write(opts.dir.join("notes.txt"), "hello").unwrap();

    for _ in 0..3 {
        assert!(inbox.scan_once(&store, &opts).is_empty());
    }
    assert_eq!(store.run_count().unwrap(), 0);
    assert!(outside.join("meta.json").is_file());
    assert!(fs::symlink_metadata(opts.dir.join("link"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(names(&opts.dir.join("done")).is_empty());
}
