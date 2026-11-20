//! `benchlens watch` and `benchlens serve --inbox` as processes: they stop cleanly on SIGINT / SIGTERM
//! (releasing the inbox lock), and a second watcher on the same inbox is refused while the first runs.
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn workdir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("watch-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn benchlens(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_benchlens"));
    c.arg("--db")
        .arg(dir.join("benchlens.db"))
        .env_remove("BENCHLENS_INBOX")
        .env_remove("BENCHLENS_INBOX_REGISTRY")
        .env_remove("BENCHLENS_DB");
    c
}

/// Start `watch` and wait until it says it is watching; returns the child and its stderr lines so far plus
/// a receiver for the rest.
fn start_watch(dir: &Path) -> (Child, Vec<String>, mpsc::Receiver<String>) {
    let mut cmd = benchlens(dir);
    cmd.args(["watch", "--interval", "1", "--inbox"])
        .arg(dir.join("inbox"));
    start(cmd, "watching ")
}

/// Start `serve --inbox` on a free loopback port and wait until the inbox watcher runs.
fn start_serve(dir: &Path) -> (Child, Vec<String>, mpsc::Receiver<String>) {
    let mut cmd = benchlens(dir);
    cmd.current_dir(dir)
        .env_remove("BENCHLENS_CONFIG")
        .env_remove("BENCHLENS_LISTEN")
        .env_remove("BENCHLENS_TOKEN")
        .env_remove("BENCHLENS_READ_PUBLIC")
        .args([
            "serve",
            "--listen",
            "127.0.0.1:0",
            "--inbox-interval",
            "1",
            "--inbox",
        ])
        .arg(dir.join("inbox"));
    start(cmd, "watching inbox ")
}

fn start(mut cmd: Command, ready: &str) -> (Child, Vec<String>, mpsc::Receiver<String>) {
    let mut child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut seen = Vec::new();
    loop {
        let Ok(line) = rx.recv_timeout(Duration::from_secs(20)) else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("no {ready:?} line: {seen:?}");
        };
        let is_ready = line.starts_with(ready);
        seen.push(line);
        if is_ready {
            return (child, seen, rx);
        }
    }
}

fn wait(child: &mut Child, timeout: Duration) -> std::process::ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            return st;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            panic!("did not exit within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn signal(child: &Child, sig: &str) {
    let st = Command::new("kill")
        .args([sig, &child.id().to_string()])
        .status()
        .unwrap();
    assert!(st.success());
}

fn stops_cleanly_on(sig: &str, name: &str) {
    let dir = workdir(name);
    let (mut child, _, rx) = start_watch(&dir);
    // let it finish at least one scan, so the signal arrives between scans
    std::thread::sleep(Duration::from_millis(300));
    signal(&child, sig);
    let status = wait(&mut child, Duration::from_secs(10));
    let rest: Vec<String> = rx.try_iter().collect();
    assert!(status.success(), "{status:?}: {rest:?}");
    assert!(
        rest.iter().any(|l| l.starts_with("stopped watching ")),
        "{rest:?}"
    );
    // the lock is released and its record emptied
    let lock = std::fs::read_to_string(dir.join("inbox/.benchlens.lock")).unwrap();
    assert_eq!(lock, "");
}

#[test]
fn watch_stops_cleanly_on_sigint() {
    stops_cleanly_on("-INT", "sigint");
}

#[test]
fn watch_stops_cleanly_on_sigterm() {
    stops_cleanly_on("-TERM", "sigterm");
}

#[test]
fn second_watch_on_the_same_inbox_is_refused() {
    let dir = workdir("second");
    let (mut first, _, _rx) = start_watch(&dir);
    let out = benchlens(&dir)
        .args(["watch", "--once", "--inbox"])
        .arg(dir.join("inbox"))
        .output()
        .unwrap();
    signal(&first, "-INT");
    let status = wait(&mut first, Duration::from_secs(10));
    assert!(status.success());
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(&format!("already watched by pid {}", first.id())),
        "{err}"
    );
}

fn serve_stops_cleanly_on(sig: &str, name: &str) {
    let dir = workdir(name);
    let (mut child, _, rx) = start_serve(&dir);
    std::thread::sleep(Duration::from_millis(300));
    signal(&child, sig);
    // well inside the 10 s `docker stop` waits before it kills
    let status = wait(&mut child, Duration::from_secs(5));
    let rest: Vec<String> = rx.try_iter().collect();
    assert!(status.success(), "{status:?}: {rest:?}");
    let lock = std::fs::read_to_string(dir.join("inbox/.benchlens.lock")).unwrap();
    assert_eq!(lock, "", "the inbox thread released its lock");
}

#[test]
fn serve_stops_cleanly_on_sigint() {
    serve_stops_cleanly_on("-INT", "serve-sigint");
}

#[test]
fn serve_stops_cleanly_on_sigterm() {
    serve_stops_cleanly_on("-TERM", "serve-sigterm");
}
