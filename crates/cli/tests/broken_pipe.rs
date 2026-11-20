//! Printing commands piped into a reader that goes away (`benchlens compare ... | head`) end quietly, killed
//! by SIGPIPE (the shell shows status 141) instead of panicking with "failed printing to stdout".
#![cfg(unix)]

use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn benchlens(db: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_benchlens"));
    c.arg("--db").arg(db).env_remove("BENCHLENS_DB");
    c
}

/// Run `args` with stdout a pipe whose reader is already closed: the first write fails (EPIPE).
fn run_into_closed_pipe(db: &Path, args: &[&str]) -> (std::process::ExitStatus, String) {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let out = benchlens(db)
        .args(args)
        .stdout(writer)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    (
        out.status,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Run `args`, read a little of stdout, then close it (what `head -c 16` does); returns (status, stderr).
fn run_closing_early(db: &Path, args: &[&str]) -> (std::process::ExitStatus, String) {
    let mut child = benchlens(db)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = child.stdout.take().unwrap();
    let _ = out.read(&mut [0u8; 16]);
    drop(out);
    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    (child.wait().unwrap(), err)
}

#[test]
fn printing_commands_end_quietly_on_a_closed_pipe() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("broken-pipe");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("benchlens.db");
    let ok = benchlens(&db)
        .arg("import")
        .arg(repo().join("tests/fixtures/v0.2.3"))
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(ok.success());
    let cases: &[&[&str]] = &[
        &["compare", "x86-std-r2", "x86-high-r2"],
        &["compare", "x86-std-r2", "x86-high-r2", "--json"],
        &["runs"],
        &["runs", "--json"],
        &["--help"],
    ];
    for args in cases {
        let (status, err) = run_into_closed_pipe(&db, args);
        assert!(!err.contains("panicked"), "{args:?}: {err}");
        assert!(!err.contains("Broken pipe"), "{args:?}: {err}");
        // killed by SIGPIPE (13), as `head` expects; never a panic (101) or an error status
        assert_eq!(status.signal(), Some(13), "{args:?}: {status:?} {err}");
        // closed after a few bytes: SIGPIPE, or success when everything fit into the pipe in time
        let (status, err) = run_closing_early(&db, args);
        assert!(!err.contains("panicked"), "{args:?}: {err}");
        assert!(
            status.success() || status.signal() == Some(13),
            "{args:?}: {status:?} {err}"
        );
    }
}
