//! F27: `benchlens serve` as a process prints its start-up warnings on stderr. With a token on a non-loopback
//! listener and no Host list, the Host check is off: the server warns (without printing the token); with a list
//! it does not.
#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const TOKEN: &str = "f27-0123456789abcdef";

fn workdir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("serve-warnings-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `serve --listen 0.0.0.0:0` with a token and `env`, then stop it with SIGTERM once it has printed its
/// start-up lines; returns every stderr line up to the exit ("benchlens stopping" comes after the warnings).
fn serve_stderr(name: &str, env: &[(&str, &str)]) -> Vec<String> {
    let dir = workdir(name);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_benchlens"));
    cmd.current_dir(&dir)
        .arg("--db")
        .arg(dir.join("benchlens.db"));
    for k in [
        "BENCHLENS_DB",
        "BENCHLENS_CONFIG",
        "BENCHLENS_LISTEN",
        "BENCHLENS_INBOX",
        "BENCHLENS_INBOX_REGISTRY",
        "BENCHLENS_REGISTRY",
        "BENCHLENS_READ_PUBLIC",
        "BENCHLENS_ALLOWED_HOSTS",
        "BENCHLENS_DEFAULT_ALLOWED_HOSTS",
    ] {
        cmd.env_remove(k);
    }
    cmd.env("BENCHLENS_TOKEN", TOKEN)
        .envs(env.iter().copied())
        .args(["serve", "--listen", "0.0.0.0:0"]);
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
    let mut lines = Vec::new();
    // wait for the start-up line about Host names (the warnings follow it), then stop the server
    loop {
        let Ok(line) = rx.recv_timeout(Duration::from_secs(20)) else {
            let _ = child.kill();
            let _ = child.wait();
            panic!("no start-up lines: {lines:?}");
        };
        let ready = line.starts_with("Host names served: ");
        lines.push(line);
        if ready {
            break;
        }
    }
    let st = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(st.success());
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("did not stop: {lines:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // the reader thread ends at EOF, once the process is gone
    lines.extend(rx.iter());
    assert!(lines.iter().any(|l| l == "benchlens stopping"), "{lines:?}");
    lines
}

fn host_check_warnings(lines: &[String]) -> Vec<&String> {
    lines
        .iter()
        .filter(|l| l.starts_with("warning: ") && l.contains("with no Host check"))
        .collect()
}

#[test]
fn token_without_host_list_warns() {
    let lines = serve_stderr("off", &[]);
    assert!(
        lines.contains(&"Host names served: any".to_string()),
        "{lines:?}"
    );
    let w = host_check_warnings(&lines);
    assert_eq!(w.len(), 1, "{lines:?}");
    assert!(
        w[0].starts_with(
            "warning: listening on 0.0.0.0:0 with no Host check: any Host name is served"
        ),
        "{}",
        w[0]
    );
    assert!(w[0].contains("BENCHLENS_ALLOWED_HOSTS=name"), "{}", w[0]);
    assert!(w[0].contains("read_public = false"), "{}", w[0]);
    assert!(!lines.iter().any(|l| l.contains(TOKEN)), "{lines:?}");
    assert!(
        !lines.iter().any(|l| l.contains("without a token")),
        "{lines:?}"
    );
}

#[test]
fn docker_opt_out_warns() {
    // the image's environment with the documented opt-out and no config file
    let lines = serve_stderr(
        "opt-out",
        &[
            ("BENCHLENS_DEFAULT_ALLOWED_HOSTS", "localhost"),
            ("BENCHLENS_ALLOWED_HOSTS", ""),
        ],
    );
    assert_eq!(host_check_warnings(&lines).len(), 1, "{lines:?}");
}

#[test]
fn host_list_or_image_default_is_quiet() {
    for (name, env) in [
        ("env", ("BENCHLENS_ALLOWED_HOSTS", "bench.lan")),
        ("default", ("BENCHLENS_DEFAULT_ALLOWED_HOSTS", "localhost")),
    ] {
        let lines = serve_stderr(name, &[env]);
        assert!(
            !lines.iter().any(|l| l.starts_with("warning: ")),
            "{name}: {lines:?}"
        );
    }
}
