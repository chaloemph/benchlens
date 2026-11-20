//! Who may read and write.
//!
//! - A token is configured: every write needs `Authorization: Bearer <token>`; with `read_public = false`,
//!   every `/api` read does too, except `/api/health`. The static UI files are always served, so the browser
//!   can load the page and its token dialog. A missing or wrong token is 401 with `WWW-Authenticate: Bearer`.
//! - No token: writes are open only when the server listens on a loopback address (local use); on any other
//!   address they are refused with 403. Reads are open (a token is required for `read_public = false`).
//!   Open writes from a web page of another origin are refused too (403): a browser sends a multipart POST
//!   cross-site without asking first, so any page the local user visits could otherwise upload.
//!
//! - Host check (DNS rebinding): without a token, or on a loopback listener (with or without a token), a
//!   request is served only when its `Host` names this machine in a way a web page cannot rebind:
//!   `localhost`, an IP address (`127.0.0.1`, `[::1]`, `192.168.1.5`, ...) or one of the configured
//!   `allowed_hosts`, with any port. A page on `http://evil.example:8080` whose name the attacker re-points at
//!   127.0.0.1 sends `Host: evil.example:8080` and is refused (403), for the API and the static files alike;
//!   with a token it could not write, but it could read what `read_public` makes public (F19). On a
//!   non-loopback listener with a token any `Host` is served, unless `allowed_hosts` is set, which then
//!   applies in the same way. A reverse proxy on the same machine that passes its own public name as `Host`
//!   needs that name in `allowed_hosts`. A request without `Host` (HTTP/1.0 tools) is served: browsers
//!   always send one.
//! - Reverse proxy: open writes (loopback, no token) are refused when the request carries `Forwarded`,
//!   `X-Forwarded-For` or `X-Forwarded-Host`: a proxy on the same machine would otherwise turn "local" into
//!   "anyone who reaches the proxy". Serve behind a proxy with a token.
//!
//! The token is compared in constant time (SHA-256 of both sides, then `subtle`), so neither its content nor
//! its length leaks through timing.

use crate::{ApiError, AppState};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use subtle::ConstantTimeEq;

pub use crate::config::Token;

/// The access rules of a running server.
#[derive(Debug, Clone)]
pub struct Access {
    token: Option<Token>,
    read_public: bool,
    /// Without a token, may anyone write? True when listening on loopback only.
    open_writes: bool,
    /// Listening on a loopback address: the Host check applies also with a token.
    loopback: bool,
    /// Extra `Host` names served besides `localhost` and IP addresses (lower case, no port).
    allowed_hosts: Vec<String>,
}

impl Default for Access {
    /// Local use: no token, reads and writes open.
    fn default() -> Self {
        Access::local()
    }
}

impl Access {
    /// No token, everything open: a server listening on loopback.
    pub fn local() -> Access {
        Access {
            token: None,
            read_public: true,
            open_writes: true,
            loopback: true,
            allowed_hosts: Vec::new(),
        }
    }

    /// No token on a non-loopback address: reads open, writes refused (403).
    pub fn public_read_only() -> Access {
        Access {
            token: None,
            read_public: true,
            open_writes: false,
            loopback: false,
            allowed_hosts: Vec::new(),
        }
    }

    /// Writes need `token`; reads too unless `read_public`. These are the rules of a non-loopback listener
    /// (any `Host` served unless `allowed_hosts` is set); see [`on_loopback`](Access::on_loopback).
    pub fn with_token(token: Token, read_public: bool) -> Access {
        Access {
            token: Some(token),
            read_public,
            open_writes: false,
            loopback: false,
            allowed_hosts: Vec::new(),
        }
    }

    /// The server listens on a loopback address: the Host check applies also with a token, so a DNS
    /// rebinding page cannot read public data from 127.0.0.1.
    pub fn on_loopback(mut self) -> Access {
        self.loopback = true;
        self
    }

    /// Also serve these `Host` names (see [`check_host`](Access::check_host)); names are matched without
    /// case and without a trailing dot.
    pub fn with_allowed_hosts(mut self, hosts: impl IntoIterator<Item = String>) -> Access {
        self.allowed_hosts = hosts.into_iter().map(|h| normalize_name(&h)).collect();
        self
    }

    /// The rules for these settings. `read_public = false` without a token is refused earlier, in
    /// [`crate::config::Settings::resolve`]; here it falls back to open reads.
    pub fn for_listen(
        token: Option<Token>,
        read_public: bool,
        listen: std::net::SocketAddr,
    ) -> Access {
        match token {
            Some(t) if listen.ip().is_loopback() => {
                Access::with_token(t, read_public).on_loopback()
            }
            Some(t) => Access::with_token(t, read_public),
            None if listen.ip().is_loopback() => Access::local(),
            None => Access::public_read_only(),
        }
    }

    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Is the `Host` of a request checked at all? Not only with a token on a non-loopback listener and no
    /// `allowed_hosts`.
    fn checks_host(&self) -> bool {
        self.token.is_none() || self.loopback || !self.allowed_hosts.is_empty()
    }

    /// The `Host` names served, for the start-up message: `any`, or `localhost, IP addresses, <allowed_hosts>`.
    pub fn hosts_served(&self) -> String {
        if !self.checks_host() {
            return "any".to_string();
        }
        let mut names = vec!["localhost".to_string(), "IP addresses".to_string()];
        // `localhost` in allowed_hosts only turns the check on (Docker with a token); do not list it twice
        names.extend(
            self.allowed_hosts
                .iter()
                .filter(|h| *h != "localhost")
                .cloned(),
        );
        names.join(", ")
    }

    /// May this request write?
    pub fn check_write(&self, headers: &HeaderMap) -> Result<(), Denied> {
        match &self.token {
            Some(t) => check_bearer(t, headers),
            None if self.open_writes => not_forwarded(headers).and_then(|()| same_origin(headers)),
            None => Err(Denied(ApiError::new(
                StatusCode::FORBIDDEN,
                "writes are turned off: this server listens on a non-loopback address without a token",
            )
            .with_detail("start it with BENCHLENS_TOKEN (or `token` in benchlens.toml) to allow upload, tags and delete"))),
        }
    }

    /// May this request be served at all, given its `Host` (or HTTP/2 `:authority`)? See the module docs.
    pub fn check_host(&self, headers: &HeaderMap, uri: &Uri) -> Result<(), Denied> {
        if !self.checks_host() {
            return Ok(());
        }
        // Every name the request gives must pass: each `Host` header and the authority of the request target
        // (HTTP/2 `:authority`, or an HTTP/1.1 absolute-form target such as `GET http://name/ HTTP/1.1`), so a
        // request cannot pass with one name and be routed or cached by another.
        let names = headers
            .get_all(header::HOST)
            .iter()
            .map(|h| h.to_str().unwrap_or("\u{fffd}").to_string())
            .chain(uri.authority().map(|a| a.as_str().to_string()));
        for given in names {
            let name = host_name(&given);
            if name == "localhost"
                || name.parse::<IpAddr>().is_ok()
                || self.allowed_hosts.contains(&name)
            {
                continue;
            }
            return Err(self.host_refused(&given));
        }
        Ok(())
    }

    fn host_refused(&self, given: &str) -> Denied {
        let shown: String = given.chars().take(100).collect();
        let why = if self.loopback {
            "on a loopback address only localhost, IP addresses and allowed_hosts are served (DNS rebinding guard)"
        } else if self.token.is_some() {
            "it is not in allowed_hosts"
        } else {
            "without a token only localhost, IP addresses and allowed_hosts are served (DNS rebinding guard)"
        };
        // Name exactly what to set when the name could go into allowed_hosts; a token helps only on a
        // non-loopback listener.
        let name = host_name(given);
        let what = if is_plain_name(&name) {
            // the configured names are not shown: this answer goes to whoever sent the request
            format!(
                "to serve this name (for example behind a reverse proxy on this machine that passes its public \
                 name as Host), set allowed_hosts = [\"{name}\"] in benchlens.toml or \
                 BENCHLENS_ALLOWED_HOSTS={name} (keep any names already listed)"
            )
        } else {
            "to serve a name, add it to allowed_hosts in benchlens.toml or BENCHLENS_ALLOWED_HOSTS (comma list)"
                .to_string()
        };
        let detail = if self.token.is_none() && !self.loopback {
            format!("{what}, or set a token")
        } else {
            what
        };
        Denied(
            ApiError::new(
                StatusCode::FORBIDDEN,
                format!("Host {shown:?} refused: {why}"),
            )
            .with_detail(detail),
        )
    }

    /// May this request read `/api` data?
    pub fn check_read(&self, headers: &HeaderMap) -> Result<(), Denied> {
        match &self.token {
            Some(t) if !self.read_public => check_bearer(t, headers),
            _ => Ok(()),
        }
    }
}

/// Without a token, a write must not come through a reverse proxy: on the same machine it would look local.
fn not_forwarded(headers: &HeaderMap) -> Result<(), Denied> {
    match ["forwarded", "x-forwarded-for", "x-forwarded-host"]
        .into_iter()
        .find(|h| headers.contains_key(*h))
    {
        None => Ok(()),
        Some(h) => Err(Denied(
            ApiError::new(
                StatusCode::FORBIDDEN,
                "write refused: it came through a reverse proxy and this server has no token",
            )
            .with_detail(format!(
                "the request carries {h}; set a token (BENCHLENS_TOKEN or `token` in benchlens.toml) when \
                 serving behind a proxy"
            )),
        )),
    }
}

/// The name of a `Host` value, without port, brackets of an IPv6 address, case or trailing dot.
fn host_name(host: &str) -> String {
    let host = host.trim();
    let name = if let Some(rest) = host.strip_prefix('[') {
        // `[v6]` or `[v6]:port`; anything else is not a name (refused)
        match rest.split_once(']') {
            Some((v6, "")) => v6,
            Some((v6, port)) if port.starts_with(':') => v6,
            _ => return String::new(),
        }
    } else {
        host.rsplit_once(':').map_or(host, |(n, _)| n)
    };
    normalize_name(name)
}

/// Could this (normalized) name be put into `allowed_hosts` as it is? Letters, digits, `-`, `_` and dots.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn normalize_name(name: &str) -> String {
    name.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Without a token, a write must not come from a page of another origin: a browser marks such a request with
/// `Origin` (and `Sec-Fetch-Site: cross-site`), tools such as curl send neither.
fn same_origin(headers: &HeaderMap) -> Result<(), Denied> {
    let cross = |why: &str| {
        Err(Denied(
            ApiError::new(
                StatusCode::FORBIDDEN,
                "write refused: it comes from a web page of another origin",
            )
            .with_detail(why.to_string()),
        ))
    };
    if headers
        .get("sec-fetch-site")
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"cross-site"))
    {
        return cross("Sec-Fetch-Site: cross-site");
    }
    let Some(origin) = headers.get(header::ORIGIN) else {
        return Ok(());
    };
    let host = headers.get(header::HOST).map(|h| h.as_bytes());
    // Origin is scheme://host[:port]; it must name the Host this request was sent to
    let origin_host = origin
        .as_bytes()
        .splitn(2, |b| *b == b':')
        .nth(1)
        .and_then(|rest| rest.strip_prefix(b"//"));
    match (origin_host, host) {
        (Some(o), Some(h)) if o.eq_ignore_ascii_case(h) => Ok(()),
        _ => cross("the Origin header does not match Host"),
    }
}

fn digest(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}

/// The token of an `Authorization: Bearer <token>` header (scheme case-insensitive), if there is one.
fn bearer(headers: &HeaderMap) -> Option<&[u8]> {
    let v = headers.get(header::AUTHORIZATION)?.as_bytes();
    let (scheme, rest) = v.split_at(v.iter().position(|b| *b == b' ')?);
    if !scheme.eq_ignore_ascii_case(b"bearer") {
        return None;
    }
    let rest = rest.trim_ascii();
    (!rest.is_empty()).then_some(rest)
}

fn check_bearer(token: &Token, headers: &HeaderMap) -> Result<(), Denied> {
    let Some(given) = bearer(headers) else {
        return Err(Denied(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "a token is needed: send Authorization: Bearer <token>",
        )));
    };
    let ok: bool = digest(given)
        .ct_eq(&digest(token.expose().as_bytes()))
        .into();
    if ok {
        Ok(())
    } else {
        Err(Denied(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "the token is wrong",
        )))
    }
}

/// A refused request: the JSON error, plus `WWW-Authenticate: Bearer` on 401.
#[derive(Debug)]
pub struct Denied(pub ApiError);

impl IntoResponse for Denied {
    fn into_response(self) -> Response {
        let challenge = self.0.status == StatusCode::UNAUTHORIZED;
        let mut res = self.0.into_response();
        if challenge {
            res.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer realm=\"benchlens\""),
            );
        }
        res
    }
}

/// Middleware on every route, the static files included: refuses a `Host` that may come from DNS rebinding.
pub async fn host_guard(State(st): State<AppState>, req: Request, next: Next) -> Response {
    match st.access.check_host(req.headers(), req.uri()) {
        Ok(()) => next.run(req).await,
        Err(d) => d.into_response(),
    }
}

/// Middleware on the `/api` read routes: refuses reads without the token when `read_public = false`.
pub async fn read_guard(State(st): State<AppState>, req: Request, next: Next) -> Response {
    match st.access.check_read(req.headers()) {
        Ok(()) => next.run(req).await,
        Err(d) => d.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(auth: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, HeaderValue::from_str(auth).unwrap());
        h
    }

    const T: &str = "0123456789abcdef";

    #[test]
    fn bearer_parsing() {
        let a = Access::with_token(Token::new(T), true);
        assert!(a.check_write(&headers(&format!("Bearer {T}"))).is_ok());
        assert!(a.check_write(&headers(&format!("bearer  {T} "))).is_ok());
        assert!(a.check_write(&headers(&format!("Basic {T}"))).is_err());
        assert!(a.check_write(&headers("Bearer")).is_err());
        assert!(a.check_write(&headers("Bearer ")).is_err());
        assert!(a.check_write(&headers(&format!("Bearer {T}x"))).is_err());
        assert!(a.check_write(&HeaderMap::new()).is_err());
        // reads are public here
        assert!(a.check_read(&HeaderMap::new()).is_ok());
    }

    #[test]
    fn modes() {
        let lo = "127.0.0.1:8080".parse().unwrap();
        let any = "0.0.0.0:8080".parse().unwrap();
        assert!(Access::for_listen(None, true, lo)
            .check_write(&HeaderMap::new())
            .is_ok());
        let e = Access::for_listen(None, true, any)
            .check_write(&HeaderMap::new())
            .unwrap_err();
        assert_eq!(e.0.status, StatusCode::FORBIDDEN);
        let private = Access::for_listen(Some(Token::new(T)), false, lo);
        let e = private.check_read(&HeaderMap::new()).unwrap_err();
        assert_eq!(e.0.status, StatusCode::UNAUTHORIZED);
        assert!(private.check_read(&headers(&format!("Bearer {T}"))).is_ok());
    }

    #[test]
    fn open_writes_refuse_other_origins() {
        let a = Access::local();
        let with = |pairs: &[(&'static str, &'static str)]| {
            let mut h = HeaderMap::new();
            for (k, v) in pairs {
                h.insert(*k, HeaderValue::from_static(v));
            }
            h
        };
        // curl and the like: no Origin
        assert!(a.check_write(&HeaderMap::new()).is_ok());
        // the UI itself
        assert!(a
            .check_write(&with(&[
                ("host", "127.0.0.1:8080"),
                ("origin", "http://127.0.0.1:8080"),
                ("sec-fetch-site", "same-origin"),
            ]))
            .is_ok());
        for bad in [
            with(&[
                ("host", "127.0.0.1:8080"),
                ("origin", "https://evil.example"),
            ]),
            with(&[("host", "127.0.0.1:8080"), ("origin", "null")]),
            with(&[("host", "127.0.0.1:8080"), ("sec-fetch-site", "cross-site")]),
            with(&[("origin", "http://127.0.0.1:8080")]),
        ] {
            let e = a.check_write(&bad).unwrap_err();
            assert_eq!(e.0.status, StatusCode::FORBIDDEN, "{bad:?}");
        }
        // with a token the bearer decides (a browser never adds it on its own)
        let t = Access::with_token(Token::new(T), true);
        assert!(t
            .check_write(&with(&[
                ("host", "127.0.0.1:8080"),
                ("origin", "https://evil.example"),
                ("authorization", "Bearer 0123456789abcdef"),
            ]))
            .is_ok());
    }

    fn host(h: &'static str) -> HeaderMap {
        let mut m = HeaderMap::new();
        m.insert(header::HOST, HeaderValue::from_static(h));
        m
    }

    #[test]
    fn host_check_without_token() {
        let a = Access::local();
        let uri = Uri::from_static("/api/runs");
        for ok in [
            "localhost",
            "localhost:8080",
            "LocalHost.:8080",
            "127.0.0.1",
            "127.0.0.1:8080",
            "[::1]",
            "[::1]:8080",
            "192.168.1.5:8080",
        ] {
            assert!(a.check_host(&host(ok), &uri).is_ok(), "{ok}");
        }
        for bad in [
            "evil.example",
            "evil.example:8080",
            "localhost.evil.example",
            "127.0.0.1.nip.io:8080",
            "bench.lan",
            "",
        ] {
            let e = a.check_host(&host(bad), &uri).unwrap_err();
            assert_eq!(e.0.status, StatusCode::FORBIDDEN, "{bad}");
            assert!(e.0.detail.unwrap().contains("allowed_hosts"));
        }
        // no Host: not a browser
        assert!(a.check_host(&HeaderMap::new(), &uri).is_ok());
        // other spellings of a loopback or IP address are still addresses (cannot be rebound)
        for ok in [
            "LOCALHOST",
            "127.0.0.1.:8080",
            "[::ffff:127.0.0.1]:8080",
            "[0:0:0:0:0:0:0:1]",
            "localhost:",
        ] {
            assert!(a.check_host(&host(ok), &uri).is_ok(), "{ok}");
        }
        // names that only look like an address, userinfo, odd ports and IPv6 without brackets are refused
        for bad in [
            "2130706433",
            "0x7f.1",
            "user@localhost",
            "evil.example:80@127.0.0.1",
            "evil.example@127.0.0.1",
            "127.0.0.1:8080:evil.example",
            "::1",
            "[::1",
            "[::1]evil.example",
            "localhost evil.example",
            "*.localhost",
            "app.localhost",
        ] {
            assert!(a.check_host(&host(bad), &uri).is_err(), "{bad}");
        }
        // every name must pass: a second Host header, or an absolute-form target naming another host
        let mut two = host("localhost:8080");
        two.append(header::HOST, HeaderValue::from_static("evil.example"));
        assert!(a.check_host(&two, &uri).is_err());
        let absolute = Uri::from_static("http://evil.example:8080/api/runs");
        assert!(a.check_host(&host("localhost:8080"), &absolute).is_err());
        let absolute = Uri::from_static("http://127.0.0.1:8080/api/runs");
        assert!(a.check_host(&host("localhost:8080"), &absolute).is_ok());
        // HTTP/2 :authority
        let h2 = Uri::from_static("http://evil.example:8080/");
        assert!(a.check_host(&HeaderMap::new(), &h2).is_err());
        // allowed_hosts
        let a = Access::public_read_only().with_allowed_hosts(["Bench.LAN".to_string()]);
        assert!(a.check_host(&host("bench.lan:8080"), &uri).is_ok());
        assert!(a.check_host(&host("bench.lan."), &uri).is_ok());
        assert!(a.check_host(&host("other.lan"), &uri).is_err());
        assert!(a.check_host(&host("localhost:8080"), &uri).is_ok());
    }

    #[test]
    fn host_check_with_token() {
        let uri = Uri::from_static("/");
        let a = Access::with_token(Token::new(T), true);
        assert!(a.check_host(&host("evil.example:8080"), &uri).is_ok());
        let a = a.with_allowed_hosts(["bench.example.com".to_string()]);
        assert!(a.check_host(&host("bench.example.com"), &uri).is_ok());
        assert!(a.check_host(&host("127.0.0.1:8080"), &uri).is_ok());
        let e = a.check_host(&host("evil.example"), &uri).unwrap_err();
        assert!(e.0.error.contains("allowed_hosts"), "{}", e.0.error);
    }

    #[test]
    fn host_check_with_token_on_loopback() {
        let uri = Uri::from_static("/api/runs");
        let lo: std::net::SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let lo6: std::net::SocketAddr = "[::1]:8080".parse().unwrap();
        for listen in [lo, lo6] {
            let a = Access::for_listen(Some(Token::new(T)), true, listen);
            for ok in [
                "localhost:8080",
                "127.0.0.1:8080",
                "[::1]:8080",
                "192.168.1.5",
            ] {
                assert!(a.check_host(&host(ok), &uri).is_ok(), "{ok}");
            }
            let e = a.check_host(&host("evil.example:8080"), &uri).unwrap_err();
            assert_eq!(e.0.status, StatusCode::FORBIDDEN);
            assert!(e.0.error.contains("loopback"), "{}", e.0.error);
            // says exactly what to set, and no longer suggests a token (it has one)
            let detail = e.0.detail.unwrap();
            assert!(
                detail.contains(r#"allowed_hosts = ["evil.example"]"#),
                "{detail}"
            );
            assert!(
                detail.contains("BENCHLENS_ALLOWED_HOSTS=evil.example"),
                "{detail}"
            );
            assert!(!detail.contains("set a token"), "{detail}");
            assert_eq!(a.hosts_served(), "localhost, IP addresses");
            // allowed_hosts adds names
            let a = a.with_allowed_hosts(["bench.example.com".to_string()]);
            assert!(a.check_host(&host("bench.example.com"), &uri).is_ok());
            assert!(a.check_host(&host("evil.example"), &uri).is_err());
            // a private server too
            let p = Access::for_listen(Some(Token::new(T)), false, listen);
            assert!(p.check_host(&host("evil.example"), &uri).is_err());
        }
        // a name that cannot go into allowed_hosts is not echoed as a setting
        let a = Access::for_listen(Some(Token::new(T)), true, lo);
        let e = a.check_host(&host("user@evil.example"), &uri).unwrap_err();
        assert!(!e.0.detail.unwrap().contains("user@"));
        // non-loopback with a token: any Host, as before
        let any: std::net::SocketAddr = "0.0.0.0:8080".parse().unwrap();
        let a = Access::for_listen(Some(Token::new(T)), true, any);
        assert!(a.check_host(&host("evil.example:8080"), &uri).is_ok());
        assert_eq!(a.hosts_served(), "any");
        let lan: std::net::SocketAddr = "192.168.1.5:8080".parse().unwrap();
        let a = Access::for_listen(Some(Token::new(T)), true, lan);
        assert!(a.check_host(&host("bench.lan"), &uri).is_ok());
        // Docker (0.0.0.0 in the container) with a token: allowed_hosts = ["localhost"] turns the check on
        let a = Access::for_listen(Some(Token::new(T)), true, any)
            .with_allowed_hosts(["localhost".to_string()]);
        assert!(a.check_host(&host("localhost:8080"), &uri).is_ok());
        assert!(a.check_host(&host("127.0.0.1:8080"), &uri).is_ok());
        assert!(a.check_host(&host("evil.example:8080"), &uri).is_err());
        assert_eq!(a.hosts_served(), "localhost, IP addresses");
        // without a token, non-loopback: a token is still a way out
        let a = Access::for_listen(None, true, any);
        let e = a.check_host(&host("bench.lan"), &uri).unwrap_err();
        assert!(e.0.detail.unwrap().contains("set a token"));
    }

    #[test]
    fn open_writes_refuse_proxied_requests() {
        let a = Access::local();
        for h in ["forwarded", "x-forwarded-for", "x-forwarded-host"] {
            let mut m = host("127.0.0.1:8080");
            m.insert(h, HeaderValue::from_static("for=192.0.2.1"));
            let e = a.check_write(&m).unwrap_err();
            assert_eq!(e.0.status, StatusCode::FORBIDDEN);
            assert!(e.0.detail.unwrap().contains("token"));
        }
        // with a token the proxy is fine
        let t = Access::with_token(Token::new(T), true);
        let mut m = headers(&format!("Bearer {T}"));
        m.insert("x-forwarded-for", HeaderValue::from_static("192.0.2.1"));
        assert!(t.check_write(&m).is_ok());
    }

    #[test]
    fn denied_carries_challenge_only_on_401() {
        let r = Denied(ApiError::new(StatusCode::UNAUTHORIZED, "x")).into_response();
        assert_eq!(
            r.headers()[header::WWW_AUTHENTICATE],
            "Bearer realm=\"benchlens\""
        );
        let r = Denied(ApiError::new(StatusCode::FORBIDDEN, "x")).into_response();
        assert!(r.headers().get(header::WWW_AUTHENTICATE).is_none());
    }
}
