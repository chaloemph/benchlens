//! Settings of `benchlens serve`: command-line flags, `BENCHLENS_*` environment variables and an optional
//! `benchlens.toml`, in that order of precedence, then the defaults.
//!
//! `allowed_hosts` has one more, lowest-priority source: `BENCHLENS_DEFAULT_ALLOWED_HOSTS`, which the Docker
//! image sets to `localhost` (F26). So the order for it is flag > `BENCHLENS_ALLOWED_HOSTS` > `allowed_hosts`
//! in the file > `BENCHLENS_DEFAULT_ALLOWED_HOSTS` > none, and a mounted benchlens.toml replaces the image
//! default. An empty list counts as not set, except that `BENCHLENS_ALLOWED_HOSTS=` (set, but empty) also
//! drops the default list: the opt-out of 0.1.2 (`docker run -e BENCHLENS_ALLOWED_HOSTS=`) still turns the
//! check off when no file lists names.
//!
//! ```toml
//! db = "/data/benchlens.db"
//! listen = "0.0.0.0:8080"
//! inbox = "/data/inbox"        # off when not set
//! inbox_interval = 5           # seconds
//! registry = "/data/registry.tsv"   # tags for inbox imports and uploads (old name: inbox_registry)
//! inbox_tags = { set = "poc" } # tags for every inbox import (win over the registry)
//! token = "at least 16 characters"
//! read_public = true           # false: reads need the token too
//! allowed_hosts = ["bench.example.com"]   # Host names served besides localhost and IP addresses
//! max_upload_mb = 256
//! ```
//!
//! The token never appears in `Debug` output or in messages. [`Settings::resolve`] refuses a token shorter
//! than [`MIN_TOKEN_LEN`] and `read_public = false` without a token.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The config file looked for in the current folder when `--config` / `BENCHLENS_CONFIG` are not given.
pub const DEFAULT_CONFIG_FILE: &str = "benchlens.toml";
pub const DEFAULT_DB: &str = "benchlens.db";
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8080";
pub const DEFAULT_INBOX_INTERVAL_SECS: u64 = 5;
pub const DEFAULT_MAX_UPLOAD_MB: u64 = 256;
/// Largest `max_upload_mb` accepted (core refuses archives holding more than 512 MiB anyway).
pub const MAX_UPLOAD_MB_LIMIT: u64 = 4096;
/// Shortest token accepted.
pub const MIN_TOKEN_LEN: usize = 16;

/// The write (and, with `read_public = false`, read) token. Its `Debug` output is redacted and it has no
/// `Display`, so it cannot end up in a log line by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn new(s: impl Into<String>) -> Token {
        Token(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

/// One source of settings (flags, environment or file); `None` means "not given here".
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub db: Option<PathBuf>,
    pub listen: Option<String>,
    pub inbox: Option<PathBuf>,
    pub inbox_interval: Option<u64>,
    /// Registry TSV for inbox imports and uploads.
    pub registry: Option<PathBuf>,
    /// The former name of `registry`, still accepted.
    pub inbox_registry: Option<PathBuf>,
    pub inbox_tags: Option<BTreeMap<String, String>>,
    pub token: Option<Token>,
    pub read_public: Option<bool>,
    pub max_upload_mb: Option<u64>,
    /// `Host` names served besides `localhost` and IP addresses (DNS rebinding guard).
    pub allowed_hosts: Option<Vec<String>>,
    /// `BENCHLENS_DEFAULT_ALLOWED_HOSTS`: the list used when no other source gives one (the Docker image sets
    /// it to `localhost`). Environment only; not a key of benchlens.toml.
    #[serde(skip)]
    pub default_allowed_hosts: Option<Vec<String>>,
}

/// Where the effective `allowed_hosts` came from, so the Host 403 and the start-up message can name the
/// setting that would work (F25). The configured names themselves are never shown in a 403.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HostsSource {
    /// No list anywhere.
    #[default]
    Unset,
    /// A command-line flag (no such `serve` flag today; kept for programmatic callers).
    Flag,
    /// `BENCHLENS_ALLOWED_HOSTS`.
    Env,
    /// `allowed_hosts` in the config file.
    File,
    /// `BENCHLENS_DEFAULT_ALLOWED_HOSTS` (the Docker image default).
    DefaultEnv,
}

impl HostsSource {
    /// The setting, as shown at start-up.
    pub fn describe(self) -> &'static str {
        match self {
            HostsSource::Unset => "none",
            HostsSource::Flag => "the command line",
            HostsSource::Env => "BENCHLENS_ALLOWED_HOSTS",
            HostsSource::File => "allowed_hosts in the config file",
            HostsSource::DefaultEnv => "BENCHLENS_DEFAULT_ALLOWED_HOSTS",
        }
    }
}

impl<'de> Deserialize<'de> for Token {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(d).map(Token)
    }
}

impl Layer {
    /// The `BENCHLENS_*` variables, read through `var` (so tests need not touch the process environment):
    /// `BENCHLENS_DB`, `_LISTEN`, `_INBOX`, `_INBOX_INTERVAL`, `_REGISTRY` (or the older `_INBOX_REGISTRY`),
    /// `_TOKEN`, `_READ_PUBLIC`, `_MAX_UPLOAD_MB`, `_ALLOWED_HOSTS` (comma list), `_DEFAULT_ALLOWED_HOSTS`
    /// (comma list, lowest priority). An empty variable counts as not set; an `_ALLOWED_HOSTS` without any name
    /// (empty, or only blanks and commas) counts as not set too and also drops `_DEFAULT_ALLOWED_HOSTS` (the
    /// opt-out of the Docker image default, as in 0.1.2).
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Result<Layer> {
        let get = |k: &str| var(k).filter(|v| !v.is_empty());
        // BENCHLENS_ALLOWED_HOSTS set without any name (empty, or only blanks, commas, dots: review) counts as
        // not set and drops the default list, so `BENCHLENS_ALLOWED_HOSTS=" "` behaves like the documented
        // opt-out `BENCHLENS_ALLOWED_HOSTS=` instead of also hiding a config file's list
        let hosts = var("BENCHLENS_ALLOWED_HOSTS");
        let hosts_blank = hosts.as_deref().is_some_and(|v| {
            v.split(',')
                .all(|h| h.trim().trim_end_matches('.').is_empty())
        });
        let num = |k: &str| -> Result<Option<u64>> {
            get(k)
                .map(|v| {
                    v.trim()
                        .parse::<u64>()
                        .with_context(|| format!("{k}: expected a whole number, got {v:?}"))
                })
                .transpose()
        };
        Ok(Layer {
            db: get("BENCHLENS_DB").map(PathBuf::from),
            listen: get("BENCHLENS_LISTEN"),
            inbox: get("BENCHLENS_INBOX").map(PathBuf::from),
            inbox_interval: num("BENCHLENS_INBOX_INTERVAL")?,
            registry: get("BENCHLENS_REGISTRY").map(PathBuf::from),
            inbox_registry: get("BENCHLENS_INBOX_REGISTRY").map(PathBuf::from),
            inbox_tags: None,
            token: get("BENCHLENS_TOKEN").map(Token),
            read_public: get("BENCHLENS_READ_PUBLIC")
                .map(|v| parse_bool("BENCHLENS_READ_PUBLIC", &v))
                .transpose()?,
            max_upload_mb: num("BENCHLENS_MAX_UPLOAD_MB")?,
            allowed_hosts: hosts.filter(|_| !hosts_blank).map(|v| comma_list(&v)),
            default_allowed_hosts: if hosts_blank {
                None
            } else {
                get("BENCHLENS_DEFAULT_ALLOWED_HOSTS").map(|v| comma_list(&v))
            },
        })
    }

    /// Parse the text of a `benchlens.toml`; unknown keys are an error (a typo would otherwise be ignored).
    pub fn from_toml(text: &str) -> Result<Layer> {
        let mut layer: Layer =
            toml::from_str(text).map_err(|e| anyhow::anyhow!("{}", e.message()))?;
        if layer.token.as_ref().is_some_and(|t| t.0.is_empty()) {
            layer.token = None;
        }
        if layer.registry.is_some() && layer.inbox_registry.is_some() {
            bail!("give `registry` or its old name `inbox_registry`, not both");
        }
        Ok(layer)
    }

    /// Read a config file. `explicit`: the path was given with `--config` / `BENCHLENS_CONFIG`, so it must
    /// exist; otherwise a missing `./benchlens.toml` is simply no layer.
    pub fn from_file(path: &Path, explicit: bool) -> Result<Layer> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                Layer::from_toml(&text).with_context(|| format!("config {}", path.display()))
            }
            Err(e) if !explicit && e.kind() == std::io::ErrorKind::NotFound => Ok(Layer::default()),
            Err(e) => Err(e).with_context(|| format!("reading config {}", path.display())),
        }
    }
}

fn comma_list(v: &str) -> Vec<String> {
    v.split(',').map(str::to_string).collect()
}

fn parse_bool(what: &str, v: &str) -> Result<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("{what}: expected true or false, got {v:?}"),
    }
}

/// Clean up `allowed_hosts`: trimmed, lower case, empty entries dropped; a scheme, path or port is an error.
fn check_hosts(list: Vec<String>) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for h in list {
        let h = h.trim().trim_end_matches('.').to_ascii_lowercase();
        if h.is_empty() {
            continue;
        }
        // IP addresses are always served, so only names make sense here
        if h.contains(['/', ':', '[', ']']) || h.chars().any(char::is_whitespace) {
            bail!("allowed_hosts: {h:?} is not a host name; give names such as bench.example.com, without scheme or port (IP addresses are always served)");
        }
        out.push(h);
    }
    Ok(out)
}

/// Is this a loopback address? `127.0.0.0/8`, `::1`, and the IPv4-mapped form of a loopback address
/// (`::ffff:127.0.0.1`), which `Ipv6Addr::is_loopback` does not count (F24).
pub fn is_loopback(ip: IpAddr) -> bool {
    ip.to_canonical().is_loopback()
}

/// Where the config file is: the `--config` / `BENCHLENS_CONFIG` value if any (must exist), else
/// `./benchlens.toml` when present.
pub fn config_path(flag_or_env: Option<PathBuf>) -> (PathBuf, bool) {
    match flag_or_env {
        Some(p) => (p, true),
        None => (PathBuf::from(DEFAULT_CONFIG_FILE), false),
    }
}

/// The settings `benchlens serve` runs with, after precedence, defaults and checks.
#[derive(Debug, Clone)]
pub struct Settings {
    pub db: PathBuf,
    pub listen: SocketAddr,
    pub inbox: Option<PathBuf>,
    pub inbox_interval: Duration,
    /// Registry TSV for inbox imports (read again when it changes) and uploads (read at every upload).
    pub registry: Option<PathBuf>,
    /// Tags for every run the inbox imports (they win over the registry).
    pub inbox_tags: BTreeMap<String, String>,
    pub token: Option<Token>,
    pub read_public: bool,
    pub max_upload_bytes: u64,
    /// `Host` names served besides `localhost` and IP addresses: lower case, no port.
    pub allowed_hosts: Vec<String>,
    /// Where `allowed_hosts` came from.
    pub allowed_hosts_source: HostsSource,
    /// The config file's `allowed_hosts` when a flag or `BENCHLENS_ALLOWED_HOSTS` hides it with another
    /// list, for a start-up warning (the image default never hides the file).
    pub file_hosts_ignored: Option<Vec<String>>,
}

impl Settings {
    /// Merge the layers (flags win over the environment, the environment over the file) and check the result.
    pub fn resolve(flags: &Layer, env: &Layer, file: &Layer) -> Result<Settings> {
        let layers = [flags, env, file];
        macro_rules! pick {
            ($f:ident) => {
                layers.iter().find_map(|l| l.$f.clone())
            };
        }
        let listen_text = pick!(listen).unwrap_or_else(|| DEFAULT_LISTEN.to_string());
        let listen: SocketAddr = listen_text.trim().parse().with_context(|| {
            format!("listen: {listen_text:?} is not an address such as 127.0.0.1:8080")
        })?;
        let token = pick!(token).filter(|t| !t.0.is_empty());
        if let Some(t) = &token {
            if t.0.chars().count() < MIN_TOKEN_LEN {
                bail!("the token is too short: use at least {MIN_TOKEN_LEN} characters (e.g. `openssl rand -hex 32`)");
            }
            if !t.0.bytes().all(|b| b.is_ascii_graphic()) {
                bail!("the token may only hold visible ASCII characters (no spaces)");
            }
        }
        let read_public = pick!(read_public).unwrap_or(true);
        if !read_public && token.is_none() {
            bail!(
                "read_public = false needs a token (BENCHLENS_TOKEN or `token` in benchlens.toml)"
            );
        }
        let mb = pick!(max_upload_mb).unwrap_or(DEFAULT_MAX_UPLOAD_MB);
        if mb == 0 || mb > MAX_UPLOAD_MB_LIMIT {
            bail!("max_upload_mb: use 1 to {MAX_UPLOAD_MB_LIMIT}");
        }
        let secs = pick!(inbox_interval).unwrap_or(DEFAULT_INBOX_INTERVAL_SECS);
        let inbox_tags = pick!(inbox_tags).unwrap_or_default();
        for (k, v) in &inbox_tags {
            benchlens_core::store::check_tag(k, v).context("inbox_tags")?;
        }
        // flag > BENCHLENS_ALLOWED_HOSTS > file > BENCHLENS_DEFAULT_ALLOWED_HOSTS; an empty file or default
        // list counts as not set (an empty flag or variable list is kept: it turns the check off, as before)
        let (allowed_hosts, allowed_hosts_source) = if let Some(l) = &flags.allowed_hosts {
            (check_hosts(l.clone())?, HostsSource::Flag)
        } else if let Some(l) = &env.allowed_hosts {
            (check_hosts(l.clone())?, HostsSource::Env)
        } else {
            let from_file = check_hosts(file.allowed_hosts.clone().unwrap_or_default())?;
            let from_default = check_hosts(env.default_allowed_hosts.clone().unwrap_or_default())
                .context("BENCHLENS_DEFAULT_ALLOWED_HOSTS")?;
            if !from_file.is_empty() {
                (from_file, HostsSource::File)
            } else if !from_default.is_empty() {
                (from_default, HostsSource::DefaultEnv)
            } else {
                (Vec::new(), HostsSource::Unset)
            }
        };
        // a flag or BENCHLENS_ALLOWED_HOSTS beats the file; say so when that hides a different list there
        let file_hosts_ignored = match &file.allowed_hosts {
            Some(f) if matches!(allowed_hosts_source, HostsSource::Flag | HostsSource::Env) => {
                let f_clean = check_hosts(f.clone()).unwrap_or_else(|_| f.clone());
                (!f_clean.is_empty() && f_clean != allowed_hosts).then_some(f_clean)
            }
            _ => None,
        };
        // within one layer `registry` wins over its old name; a flag of either name beats the environment
        let registry = layers
            .iter()
            .find_map(|l| l.registry.clone().or_else(|| l.inbox_registry.clone()));
        Ok(Settings {
            db: pick!(db).unwrap_or_else(|| PathBuf::from(DEFAULT_DB)),
            listen,
            inbox: pick!(inbox),
            inbox_interval: Duration::from_secs(secs.max(1)),
            registry,
            inbox_tags,
            token,
            read_public,
            max_upload_bytes: mb * 1024 * 1024,
            allowed_hosts,
            allowed_hosts_source,
            file_hosts_ignored,
        })
    }

    /// Warnings to print at start-up: serving other machines without a token, serving them with a token but
    /// no Host check while reads are open, and a config file's `allowed_hosts` hidden by a flag or
    /// `BENCHLENS_ALLOWED_HOSTS`.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.token.is_none() && !is_loopback(self.listen.ip()) {
            out.push(format!(
                "warning: listening on {} without a token: anyone who can reach it can read, and writes \
                 (upload, tags, delete) are refused; set BENCHLENS_TOKEN to allow them",
                self.listen
            ));
        }
        // F27: the Host check is off (a token, a non-loopback listener, no allowed_hosts: asked of
        // `Access::checks_host` itself, so the two cannot drift) and reads are open, so a DNS rebinding page can
        // read the public data. With read_public = false only /api/health (version, run count) and the static
        // UI files are readable without the token: not warned about (docs/reviews/m7-f27.md)
        if self.read_public && !crate::auth::Access::for_settings(self).checks_host() {
            out.push(format!(
                "warning: listening on {} with no Host check: any Host name is served, so a DNS rebinding web \
                 page can read the public data (writes still need the token); set allowed_hosts in \
                 benchlens.toml or BENCHLENS_ALLOWED_HOSTS=name to turn the check on, or read_public = false \
                 to keep reads behind the token",
                self.listen
            ));
        }
        if let Some(f) = &self.file_hosts_ignored {
            let shown = |l: &[String]| {
                if l.is_empty() {
                    "none".to_string()
                } else {
                    l.join(",")
                }
            };
            let winner = match self.allowed_hosts_source {
                HostsSource::Flag => "the command line",
                _ => "BENCHLENS_ALLOWED_HOSTS",
            };
            out.push(format!(
                "warning: allowed_hosts in the config file ({}) is ignored: {winner} ({}) wins over the \
                 file; put the names in {winner}, or remove it to use the file's list",
                shown(f),
                shown(&self.allowed_hosts)
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TOKEN: &str = "0123456789abcdef";

    fn env(pairs: &[(&str, &str)]) -> Layer {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Layer::from_env(|k| m.get(k).cloned()).unwrap()
    }

    #[test]
    fn defaults() {
        let s = Settings::resolve(&Layer::default(), &Layer::default(), &Layer::default()).unwrap();
        assert_eq!(s.db, PathBuf::from("benchlens.db"));
        assert_eq!(s.listen, "127.0.0.1:8080".parse().unwrap());
        assert_eq!(s.inbox, None);
        assert_eq!(s.inbox_interval, Duration::from_secs(5));
        assert!(s.token.is_none());
        assert!(s.read_public);
        assert_eq!(s.max_upload_bytes, 256 * 1024 * 1024);
        assert!(s.warnings().is_empty());
    }

    #[test]
    fn flags_beat_env_beat_file() {
        let file = Layer::from_toml(
            "db = \"file.db\"\nlisten = \"127.0.0.1:1\"\ninbox = \"in-file\"\ninbox_interval = 7\n\
             token = \"file-token-0123456789\"\nread_public = false\nmax_upload_mb = 10\n",
        )
        .unwrap();
        let envl = env(&[
            ("BENCHLENS_DB", "env.db"),
            ("BENCHLENS_LISTEN", "127.0.0.1:2"),
            ("BENCHLENS_MAX_UPLOAD_MB", "20"),
        ]);
        let flags = Layer {
            db: Some("flag.db".into()),
            ..Layer::default()
        };
        let s = Settings::resolve(&flags, &envl, &file).unwrap();
        assert_eq!(s.db, PathBuf::from("flag.db"));
        assert_eq!(s.listen.port(), 2);
        assert_eq!(s.max_upload_bytes, 20 * 1024 * 1024);
        assert_eq!(s.inbox, Some(PathBuf::from("in-file")));
        assert_eq!(s.inbox_interval, Duration::from_secs(7));
        assert_eq!(s.token.unwrap().expose(), "file-token-0123456789");
        assert!(!s.read_public);
    }

    #[test]
    fn env_token_wins_over_file_and_empty_env_is_unset() {
        let file = Layer::from_toml(&format!("token = \"{TOKEN}-file\"")).unwrap();
        let s = Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_TOKEN", TOKEN)]),
            &file,
        )
        .unwrap();
        assert_eq!(s.token.unwrap().expose(), TOKEN);
        let s =
            Settings::resolve(&Layer::default(), &env(&[("BENCHLENS_TOKEN", "")]), &file).unwrap();
        assert_eq!(s.token.unwrap().expose(), format!("{TOKEN}-file"));
    }

    #[test]
    fn refuses_short_token() {
        let e = Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_TOKEN", "short")]),
            &Layer::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("too short"), "{e}");
        assert!(!e.contains("short\""), "the token must not be echoed: {e}");
        let fifteen = &TOKEN[..15];
        assert!(Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_TOKEN", fifteen)]),
            &Layer::default()
        )
        .is_err());
        assert!(Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_TOKEN", TOKEN)]),
            &Layer::default()
        )
        .is_ok());
    }

    #[test]
    fn refuses_token_with_spaces() {
        let l = Layer {
            token: Some(Token::new("0123456789 abcdef")),
            ..Layer::default()
        };
        assert!(Settings::resolve(&l, &Layer::default(), &Layer::default()).is_err());
    }

    #[test]
    fn refuses_private_reads_without_token() {
        let e = Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_READ_PUBLIC", "false")]),
            &Layer::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("needs a token"), "{e}");
        let s = Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_READ_PUBLIC", "no"), ("BENCHLENS_TOKEN", TOKEN)]),
            &Layer::default(),
        )
        .unwrap();
        assert!(!s.read_public);
    }

    #[test]
    fn bad_values_are_errors() {
        assert!(Layer::from_toml("tokn = \"x\"").is_err(), "unknown key");
        assert!(Layer::from_toml("read_public = \"maybe\"").is_err());
        assert!(
            Layer::from_env(|k| (k == "BENCHLENS_READ_PUBLIC").then(|| "maybe".into())).is_err()
        );
        assert!(
            Layer::from_env(|k| (k == "BENCHLENS_MAX_UPLOAD_MB").then(|| "lots".into())).is_err()
        );
        let l = Layer {
            listen: Some("localhost".into()),
            ..Layer::default()
        };
        assert!(Settings::resolve(&l, &Layer::default(), &Layer::default()).is_err());
        let l = Layer {
            max_upload_mb: Some(0),
            ..Layer::default()
        };
        assert!(Settings::resolve(&l, &Layer::default(), &Layer::default()).is_err());
    }

    #[test]
    fn warns_on_open_address_without_token() {
        let l = Layer {
            listen: Some("0.0.0.0:8080".into()),
            ..Layer::default()
        };
        let s = Settings::resolve(&l, &Layer::default(), &Layer::default()).unwrap();
        assert!(s.warnings()[0].contains("without a token"));
        let l = Layer {
            listen: Some("0.0.0.0:8080".into()),
            token: Some(Token::new(TOKEN)),
            allowed_hosts: Some(vec!["bench.lan".into()]),
            ..Layer::default()
        };
        let s = Settings::resolve(&l, &Layer::default(), &Layer::default()).unwrap();
        assert!(s.warnings().is_empty());
    }

    /// F27: a token on a non-loopback listener with no Host list turns the Host check off; say so at start-up.
    #[test]
    fn warns_when_the_host_check_is_off() {
        let off = |w: &[String]| w.iter().any(|l| l.contains("with no Host check"));
        let resolve = |e: &[(&str, &str)], f: &str| {
            Settings::resolve(&Layer::default(), &env(e), &Layer::from_toml(f).unwrap()).unwrap()
        };
        let open = ("BENCHLENS_LISTEN", "0.0.0.0:8080");
        let tok = ("BENCHLENS_TOKEN", TOKEN);

        // token, non-loopback, no list: warn, naming both fixes, never the token
        let w = resolve(&[open, tok], "").warnings();
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(
            w[0],
            "warning: listening on 0.0.0.0:8080 with no Host check: any Host name is served, so a DNS \
             rebinding web page can read the public data (writes still need the token); set allowed_hosts \
             in benchlens.toml or BENCHLENS_ALLOWED_HOSTS=name to turn the check on, or read_public = false \
             to keep reads behind the token"
        );
        assert!(!w[0].contains(TOKEN));
        // same with IPv6 and a LAN address
        for addr in ["[::]:8080", "192.168.1.5:8080", "[::ffff:192.168.1.5]:8080"] {
            let w = resolve(&[("BENCHLENS_LISTEN", addr), tok], "").warnings();
            assert!(off(&w), "{addr}: {w:?}");
        }

        // the documented opt-out BENCHLENS_ALLOWED_HOSTS= (and a blank value) with the image default and no
        // file list: warn
        for opt_out in ["", " ", ","] {
            let e = [
                open,
                tok,
                ("BENCHLENS_DEFAULT_ALLOWED_HOSTS", "localhost"),
                ("BENCHLENS_ALLOWED_HOSTS", opt_out),
            ];
            let s = resolve(&e, "");
            assert_eq!(s.allowed_hosts_source, HostsSource::Unset);
            assert!(off(&s.warnings()), "{opt_out:?}: {:?}", s.warnings());
            // an empty file list counts as not set: still off
            assert!(off(&resolve(&e, "allowed_hosts = []").warnings()));
            // a file list turns the check on, and its names are not printed
            let w = resolve(&e, "allowed_hosts = [\"bench.lan\"]").warnings();
            assert!(w.is_empty(), "{w:?}");
        }

        // token, non-loopback, a list from any source: quiet (configured names never printed)
        for (e, f) in [
            (
                vec![open, tok, ("BENCHLENS_ALLOWED_HOSTS", "bench.lan")],
                "",
            ),
            (vec![open, tok], "allowed_hosts = [\"bench.lan\"]"),
            // the image default (Docker with no extra env)
            (
                vec![open, tok, ("BENCHLENS_DEFAULT_ALLOWED_HOSTS", "localhost")],
                "",
            ),
        ] {
            let w = resolve(&e, f).warnings();
            assert!(w.is_empty(), "{e:?} {f:?}: {w:?}");
        }
        let flags = Layer {
            allowed_hosts: Some(vec!["flag.lan".into()]),
            ..Layer::default()
        };
        let s = Settings::resolve(&flags, &env(&[open, tok]), &Layer::default()).unwrap();
        assert!(s.warnings().is_empty(), "{:?}", s.warnings());

        // token on a loopback listener: the check is on there anyway
        for addr in ["127.0.0.1:8080", "[::1]:8080", "[::ffff:127.0.0.1]:8080"] {
            let w = resolve(&[("BENCHLENS_LISTEN", addr), tok], "").warnings();
            assert!(w.is_empty(), "{addr}: {w:?}");
        }

        // no token: only the old warning, not this one
        let w = resolve(&[open, ("BENCHLENS_ALLOWED_HOSTS", "")], "").warnings();
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("without a token"), "{w:?}");

        // read_public = false: nothing is readable without the token, so nothing to warn about
        let w = resolve(&[open, tok, ("BENCHLENS_READ_PUBLIC", "false")], "").warnings();
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn inbox_registry_and_tags() {
        let file = Layer::from_toml(
            "inbox_registry = \"file.tsv\"\ninbox_tags = { set = \"poc\", round = \"2\" }\n",
        )
        .unwrap();
        let s = Settings::resolve(&Layer::default(), &Layer::default(), &file).unwrap();
        assert_eq!(s.registry, Some(PathBuf::from("file.tsv")));
        assert_eq!(s.inbox_tags.get("set").map(String::as_str), Some("poc"));
        let s = Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_INBOX_REGISTRY", "env.tsv")]),
            &file,
        )
        .unwrap();
        assert_eq!(s.registry, Some(PathBuf::from("env.tsv")));
        let flags = Layer {
            inbox_tags: Some(BTreeMap::from([("set".to_string(), "flag".to_string())])),
            ..Layer::default()
        };
        let s = Settings::resolve(&flags, &Layer::default(), &file).unwrap();
        assert_eq!(s.inbox_tags.len(), 1, "flags replace the file's tags");
        let bad = Layer::from_toml("inbox_tags = { \"bad key\" = \"x\" }").unwrap();
        assert!(Settings::resolve(&Layer::default(), &Layer::default(), &bad).is_err());
    }

    #[test]
    fn registry_and_its_old_name() {
        let file = Layer::from_toml("registry = \"new.tsv\"\n").unwrap();
        let s = Settings::resolve(&Layer::default(), &Layer::default(), &file).unwrap();
        assert_eq!(s.registry, Some(PathBuf::from("new.tsv")));
        assert!(Layer::from_toml("registry = \"a.tsv\"\ninbox_registry = \"b.tsv\"\n").is_err());
        // the environment beats the file, whichever name each uses
        let s = Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_INBOX_REGISTRY", "old-env.tsv")]),
            &file,
        )
        .unwrap();
        assert_eq!(s.registry, Some(PathBuf::from("old-env.tsv")));
        let s = Settings::resolve(
            &Layer::default(),
            &env(&[
                ("BENCHLENS_INBOX_REGISTRY", "old-env.tsv"),
                ("BENCHLENS_REGISTRY", "env.tsv"),
            ]),
            &file,
        )
        .unwrap();
        assert_eq!(s.registry, Some(PathBuf::from("env.tsv")));
    }

    #[test]
    fn mapped_loopback_counts_as_loopback() {
        for ip in [
            "127.0.0.1",
            "127.9.8.7",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:127.1.2.3",
        ] {
            assert!(is_loopback(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "0.0.0.0",
            "::",
            "192.168.1.5",
            "::ffff:192.168.1.5",
            "::ffff:0.0.0.0",
        ] {
            assert!(!is_loopback(ip.parse().unwrap()), "{ip}");
        }
        // no "listening without a token" warning on a mapped loopback address
        let s = Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_LISTEN", "[::ffff:127.0.0.1]:8080")]),
            &Layer::default(),
        )
        .unwrap();
        assert!(s.warnings().is_empty());
        let s = Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_LISTEN", "[::ffff:192.168.1.5]:8080")]),
            &Layer::default(),
        )
        .unwrap();
        assert!(!s.warnings().is_empty());
    }

    #[test]
    fn allowed_hosts() {
        let s = Settings::resolve(
            &Layer::default(),
            &env(&[("BENCHLENS_ALLOWED_HOSTS", " Bench.LAN, ,bench.example.com.")]),
            &Layer::default(),
        )
        .unwrap();
        assert_eq!(s.allowed_hosts, ["bench.lan", "bench.example.com"]);
        let file = Layer::from_toml("allowed_hosts = [\"a.example\"]").unwrap();
        let s = Settings::resolve(&Layer::default(), &Layer::default(), &file).unwrap();
        assert_eq!(s.allowed_hosts, ["a.example"]);
        assert!(s.warnings().is_empty());
        for bad in [
            "http://a.example",
            "a.example:8080",
            "a.example/x",
            "a b",
            "[::1]",
        ] {
            let l = Layer {
                allowed_hosts: Some(vec![bad.to_string()]),
                ..Layer::default()
            };
            assert!(
                Settings::resolve(&l, &Layer::default(), &Layer::default()).is_err(),
                "{bad}"
            );
        }
    }

    /// F23 review, F26: an explicit BENCHLENS_ALLOWED_HOSTS beats `allowed_hosts` in a mounted benchlens.toml;
    /// the server says so at start-up instead of silently ignoring the file.
    #[test]
    fn env_hiding_file_hosts_warns() {
        let file = Layer::from_toml("allowed_hosts = [\"Bench.LAN\"]").unwrap();
        let docker = env(&[
            ("BENCHLENS_LISTEN", "0.0.0.0:8080"),
            ("BENCHLENS_TOKEN", TOKEN),
            ("BENCHLENS_DEFAULT_ALLOWED_HOSTS", "localhost"),
            ("BENCHLENS_ALLOWED_HOSTS", "localhost"),
        ]);
        let s = Settings::resolve(&Layer::default(), &docker, &file).unwrap();
        assert_eq!(s.allowed_hosts, ["localhost"]);
        assert_eq!(s.allowed_hosts_source, HostsSource::Env);
        let w = s.warnings();
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("allowed_hosts in the config file (bench.lan) is ignored"),
            "{}",
            w[0]
        );
        assert!(
            w[0].contains("BENCHLENS_ALLOWED_HOSTS (localhost) wins"),
            "{}",
            w[0]
        );
        assert!(!w[0].contains("Docker image"), "{}", w[0]);
        assert!(!w[0].contains(TOKEN));
        // a flag hides the file too, and the warning names it
        let flags = Layer {
            allowed_hosts: Some(vec!["flag.lan".into()]),
            ..Layer::default()
        };
        let s = Settings::resolve(&flags, &docker, &file).unwrap();
        assert_eq!(s.allowed_hosts_source, HostsSource::Flag);
        assert!(
            s.warnings()[0].contains("the command line (flag.lan) wins"),
            "{:?}",
            s.warnings()
        );
        // the same list in both (after clean-up): nothing is hidden
        let same = env(&[("BENCHLENS_ALLOWED_HOSTS", "bench.lan")]);
        let s = Settings::resolve(&Layer::default(), &same, &file).unwrap();
        assert!(s.warnings().is_empty());
        // an empty list in the file counts as not set: nothing to hide
        let empty = Layer::from_toml("allowed_hosts = []").unwrap();
        let s = Settings::resolve(&Layer::default(), &docker, &empty).unwrap();
        assert!(s.warnings().is_empty(), "{:?}", s.warnings());
    }

    /// F26: the image default (BENCHLENS_DEFAULT_ALLOWED_HOSTS) has the lowest priority, so a mounted
    /// benchlens.toml replaces it, with no warning.
    #[test]
    fn hosts_precedence_for_every_source() {
        let resolve = |flags: &Layer, e: &[(&str, &str)], f: &str| {
            let file = Layer::from_toml(f).unwrap();
            Settings::resolve(flags, &env(e), &file).unwrap()
        };
        let none = Layer::default();
        let flag = Layer {
            allowed_hosts: Some(vec!["flag.lan".into()]),
            ..Layer::default()
        };
        let file = "allowed_hosts = [\"file.lan\"]";
        let dflt = ("BENCHLENS_DEFAULT_ALLOWED_HOSTS", "localhost");
        let envh = ("BENCHLENS_ALLOWED_HOSTS", "env.lan");
        /// flags, environment, file text, expected list, expected source
        type Case<'a> = (
            &'a Layer,
            &'a [(&'a str, &'a str)],
            &'a str,
            &'a [&'a str],
            HostsSource,
        );
        let cases: &[Case] = &[
            (&none, &[], "", &[], HostsSource::Unset),
            (&none, &[dflt], "", &["localhost"], HostsSource::DefaultEnv),
            (&none, &[dflt], file, &["file.lan"], HostsSource::File),
            (&none, &[], file, &["file.lan"], HostsSource::File),
            (&none, &[dflt, envh], file, &["env.lan"], HostsSource::Env),
            (&none, &[envh], "", &["env.lan"], HostsSource::Env),
            (&flag, &[dflt, envh], file, &["flag.lan"], HostsSource::Flag),
            (&flag, &[], "", &["flag.lan"], HostsSource::Flag),
            // an empty file list does not replace the default
            (
                &none,
                &[dflt],
                "allowed_hosts = [\" \"]",
                &["localhost"],
                HostsSource::DefaultEnv,
            ),
            // an empty default counts as not set
            (
                &none,
                &[("BENCHLENS_DEFAULT_ALLOWED_HOSTS", "")],
                "",
                &[],
                HostsSource::Unset,
            ),
            // BENCHLENS_ALLOWED_HOSTS= (empty) is "not set" for itself but drops the default: the opt-out
            // of the Docker image default, as in 0.1.2 ...
            (
                &none,
                &[dflt, ("BENCHLENS_ALLOWED_HOSTS", "")],
                "",
                &[],
                HostsSource::Unset,
            ),
            // ... and the file's list is used, as in 0.1.2
            (
                &none,
                &[dflt, ("BENCHLENS_ALLOWED_HOSTS", "")],
                file,
                &["file.lan"],
                HostsSource::File,
            ),
            // review: a value without any name (blanks, commas) is the same opt-out, and does not hide the
            // file's list (in 0.1.2 it turned the check off and ignored the file)
            (
                &none,
                &[dflt, ("BENCHLENS_ALLOWED_HOSTS", " , ")],
                "",
                &[],
                HostsSource::Unset,
            ),
            (
                &none,
                &[dflt, ("BENCHLENS_ALLOWED_HOSTS", " ")],
                file,
                &["file.lan"],
                HostsSource::File,
            ),
            // names are cleaned up: case, blanks, trailing dot, empty entries
            (
                &none,
                &[("BENCHLENS_DEFAULT_ALLOWED_HOSTS", " LocalHost. ,")],
                "",
                &["localhost"],
                HostsSource::DefaultEnv,
            ),
        ];
        for (i, (flags, e, f, want, src)) in cases.iter().enumerate() {
            let s = resolve(flags, e, f);
            assert_eq!(s.allowed_hosts, *want, "case {i}");
            assert_eq!(s.allowed_hosts_source, *src, "case {i}");
            // only a flag or BENCHLENS_ALLOWED_HOSTS hiding the file's list is warned about
            let hidden = matches!(src, HostsSource::Flag | HostsSource::Env) && *f == file;
            assert_eq!(
                !s.warnings().is_empty(),
                hidden,
                "case {i}: {:?}",
                s.warnings()
            );
        }
        // the default list is checked like the others
        let e = Settings::resolve(
            &none,
            &env(&[("BENCHLENS_DEFAULT_ALLOWED_HOSTS", "http://x")]),
            &Layer::default(),
        )
        .unwrap_err();
        assert!(
            format!("{e:#}").contains("BENCHLENS_DEFAULT_ALLOWED_HOSTS"),
            "{e:#}"
        );
        // a bad file list is an error unless a flag or the variable replaces it (as before)
        assert!(Settings::resolve(
            &none,
            &env(&[dflt]),
            &Layer::from_toml("allowed_hosts = [\"a:1\"]").unwrap()
        )
        .is_err());
        assert!(Settings::resolve(
            &none,
            &env(&[envh]),
            &Layer::from_toml("allowed_hosts = [\"a:1\"]").unwrap()
        )
        .is_ok());
        // not a key of the file
        assert!(Layer::from_toml("default_allowed_hosts = [\"x\"]").is_err());
    }

    #[test]
    fn token_is_redacted_in_debug() {
        let l = Layer {
            token: Some(Token::new(TOKEN)),
            ..Layer::default()
        };
        let s = Settings::resolve(&l, &Layer::default(), &Layer::default()).unwrap();
        assert!(!format!("{s:?}").contains(TOKEN));
        assert!(!format!("{l:?}").contains(TOKEN));
    }

    #[test]
    fn missing_default_file_is_fine_missing_explicit_file_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("benchlens.toml");
        assert!(Layer::from_file(&p, false).unwrap().db.is_none());
        assert!(Layer::from_file(&p, true).is_err());
        std::fs::write(&p, "db = \"x.db\"\n").unwrap();
        assert_eq!(
            Layer::from_file(&p, true).unwrap().db,
            Some(PathBuf::from("x.db"))
        );
    }
}
