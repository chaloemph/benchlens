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
//! The token is compared in constant time (SHA-256 of both sides, then `subtle`), so neither its content nor
//! its length leaks through timing.

use crate::{ApiError, AppState};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub use crate::config::Token;

/// The access rules of a running server.
#[derive(Debug, Clone)]
pub struct Access {
    token: Option<Token>,
    read_public: bool,
    /// Without a token, may anyone write? True when listening on loopback only.
    open_writes: bool,
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
        }
    }

    /// No token on a non-loopback address: reads open, writes refused (403).
    pub fn public_read_only() -> Access {
        Access {
            token: None,
            read_public: true,
            open_writes: false,
        }
    }

    /// Writes need `token`; reads too unless `read_public`.
    pub fn with_token(token: Token, read_public: bool) -> Access {
        Access {
            token: Some(token),
            read_public,
            open_writes: false,
        }
    }

    /// The rules for these settings. `read_public = false` without a token is refused earlier, in
    /// [`crate::config::Settings::resolve`]; here it falls back to open reads.
    pub fn for_listen(
        token: Option<Token>,
        read_public: bool,
        listen: std::net::SocketAddr,
    ) -> Access {
        match token {
            Some(t) => Access::with_token(t, read_public),
            None if listen.ip().is_loopback() => Access::local(),
            None => Access::public_read_only(),
        }
    }

    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// May this request write?
    pub fn check_write(&self, headers: &HeaderMap) -> Result<(), Denied> {
        match &self.token {
            Some(t) => check_bearer(t, headers),
            None if self.open_writes => same_origin(headers),
            None => Err(Denied(ApiError::new(
                StatusCode::FORBIDDEN,
                "writes are turned off: this server listens on a non-loopback address without a token",
            )
            .with_detail("start it with BENCHLENS_TOKEN (or `token` in benchlens.toml) to allow upload, tags and delete"))),
        }
    }

    /// May this request read `/api` data?
    pub fn check_read(&self, headers: &HeaderMap) -> Result<(), Denied> {
        match &self.token {
            Some(t) if !self.read_public => check_bearer(t, headers),
            _ => Ok(()),
        }
    }
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
