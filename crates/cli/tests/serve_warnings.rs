//! F27: `benchlens serve` as a process prints its start-up warnings on stderr. With a token on a non-loopback
//! listener and no Host list, the Host check is off: the server warns (without printing the token); with a list
//! it does not.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const TOKEN: &str = "f27-0123456789abcdef";

fn workdir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("serve-warnings-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Kills the server if the test panics before it stopped it (a `Child` is not killed on drop).
struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Wait until the server answers `GET /api/health` on `port`: then the graceful-shutdown future (and with it
/// the SIGTERM handler) is being polled, so SIGTERM stops it cleanly instead of killing it (review).
fn wait_serving(port: u16) {
    let start = Instant::now();
    loop {
        let answered = TcpStream::connect(("127.0.0.1", port))
            .ok()
            .and_then(|mut s| {
                s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
                s.write_all(
                    b"GET /api/health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
                )
                .ok()?;
                let mut buf = String::new();
                s.read_to_string(&mut buf).ok()?;
                buf.starts_with("HTTP/1.1 200").then_some(())
            });
        if answered.is_some() {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "not serving on port {port}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Run `serve --listen 0.0.0.0:0` with a token and `env`, then stop it with SIGTERM once it has printed its
/// start-up lines and answers requests; returns every stderr line up to the exit ("benchlens stopping" comes
/// after the warnings).
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
    let mut server = Server(
        cmd.stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stderr = server.0.stderr.take().unwrap();
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
    let mut port = None;
    loop {
        let Ok(line) = rx.recv_timeout(Duration::from_secs(20)) else {
            panic!("no start-up lines: {lines:?}");
        };
        // "benchlens X on http://0.0.0.0:PORT (database ...)": the port actually bound
        if let Some(rest) = line.split_once(" on http://0.0.0.0:").map(|(_, r)| r) {
            port = rest.split(' ').next().and_then(|p| p.parse::<u16>().ok());
        }
        let ready = line.starts_with("Host names served: ");
        lines.push(line);
        if ready {
            break;
        }
    }
    wait_serving(port.unwrap_or_else(|| panic!("no port: {lines:?}")));
    let st = Command::new("kill")
        .args(["-TERM", &server.0.id().to_string()])
        .status()
        .unwrap();
    assert!(st.success());
    let start = Instant::now();
    while server.0.try_wait().unwrap().is_none() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "did not stop: {lines:?}"
        );
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
