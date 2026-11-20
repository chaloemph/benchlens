//! Settings of `benchlens serve`: command-line flags, `BENCHLENS_*` environment variables and an optional
//! `benchlens.toml`, in that order of precedence, then the defaults.
//!
//! ```toml
//! db = "/data/benchlens.db"
//! listen = "0.0.0.0:8080"
//! inbox = "/data/inbox"        # off when not set
//! inbox_interval = 5           # seconds
//! token = "at least 16 characters"
//! read_public = true           # false: reads need the token too
//! max_upload_mb = 256
//! ```
//!
//! The token never appears in `Debug` output or in messages. [`Settings::resolve`] refuses a token shorter
//! than [`MIN_TOKEN_LEN`] and `read_public = false` without a token.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fmt;
use std::net::SocketAddr;
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
    pub token: Option<Token>,
    pub read_public: Option<bool>,
    pub max_upload_mb: Option<u64>,
}

impl<'de> Deserialize<'de> for Token {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        String::deserialize(d).map(Token)
    }
}

impl Layer {
    /// The `BENCHLENS_*` variables, read through `var` (so tests need not touch the process environment):
    /// `BENCHLENS_DB`, `_LISTEN`, `_INBOX`, `_INBOX_INTERVAL`, `_TOKEN`, `_READ_PUBLIC`, `_MAX_UPLOAD_MB`.
    /// An empty variable counts as not set.
    pub fn from_env(var: impl Fn(&str) -> Option<String>) -> Result<Layer> {
        let get = |k: &str| var(k).filter(|v| !v.is_empty());
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
            token: get("BENCHLENS_TOKEN").map(Token),
            read_public: get("BENCHLENS_READ_PUBLIC")
                .map(|v| parse_bool("BENCHLENS_READ_PUBLIC", &v))
                .transpose()?,
            max_upload_mb: num("BENCHLENS_MAX_UPLOAD_MB")?,
        })
    }

    /// Parse the text of a `benchlens.toml`; unknown keys are an error (a typo would otherwise be ignored).
    pub fn from_toml(text: &str) -> Result<Layer> {
        let mut layer: Layer =
            toml::from_str(text).map_err(|e| anyhow::anyhow!("{}", e.message()))?;
        if layer.token.as_ref().is_some_and(|t| t.0.is_empty()) {
            layer.token = None;
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

fn parse_bool(what: &str, v: &str) -> Result<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => bail!("{what}: expected true or false, got {v:?}"),
    }
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
    pub token: Option<Token>,
    pub read_public: bool,
    pub max_upload_bytes: u64,
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
        Ok(Settings {
            db: pick!(db).unwrap_or_else(|| PathBuf::from(DEFAULT_DB)),
            listen,
            inbox: pick!(inbox),
            inbox_interval: Duration::from_secs(secs.max(1)),
            token,
            read_public,
            max_upload_bytes: mb * 1024 * 1024,
        })
    }

    /// A warning to print at start-up, if any: serving other machines without a token.
    pub fn warning(&self) -> Option<String> {
        if self.token.is_none() && !self.listen.ip().is_loopback() {
            Some(format!(
                "warning: listening on {} without a token: anyone who can reach it can read, and writes \
                 (upload, tags, delete) are refused; set BENCHLENS_TOKEN to allow them",
                self.listen
            ))
        } else {
            None
        }
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
        assert!(s.warning().is_none());
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
        assert!(s.warning().unwrap().contains("without a token"));
        let l = Layer {
            listen: Some("0.0.0.0:8080".into()),
            token: Some(Token::new(TOKEN)),
            ..Layer::default()
        };
        let s = Settings::resolve(&l, &Layer::default(), &Layer::default()).unwrap();
        assert!(s.warning().is_none());
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
