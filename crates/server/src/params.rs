//! Shared pieces of the handlers: running store work off the async threads, and checking what clients send
//! (query strings, path segments, references) before it reaches the store.

use crate::{ApiError, AppState};
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use benchlens_core::compare::Reference;
use benchlens_core::store::Store;

/// Longest query value, path segment or reference accepted.
pub const MAX_PARAM_BYTES: usize = 1024;
/// Most repeated `tag=` or `other=` parameters accepted in one request.
pub const MAX_REPEAT: usize = 32;

/// Run `f` on the store in a blocking thread (rusqlite is not `Sync`; the store sits behind a mutex).
pub async fn db<T, F>(st: &AppState, f: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce(&Store) -> Result<T, ApiError> + Send + 'static,
{
    let store = st.store.clone();
    tokio::task::spawn_blocking(move || {
        let guard = store
            .lock()
            .map_err(|_| ApiError::internal("the database lock is poisoned; restart benchlens"))?;
        f(&guard)
    })
    .await
    .map_err(|e| ApiError::internal(format!("database task failed: {e}")))?
}

pub fn bad_request(msg: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, msg)
}

pub fn from_query_rejection(e: QueryRejection) -> ApiError {
    bad_request("bad query string").with_detail(e.body_text())
}

pub fn from_path_rejection(e: PathRejection) -> ApiError {
    bad_request("bad path").with_detail(e.body_text())
}

/// Body problems are 400, except a body over the limit (413).
pub fn from_json_rejection(e: JsonRejection) -> ApiError {
    let status = if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        StatusCode::PAYLOAD_TOO_LARGE
    } else {
        StatusCode::BAD_REQUEST
    };
    let msg = if status == StatusCode::PAYLOAD_TOO_LARGE {
        "request body too large"
    } else {
        "bad JSON body"
    };
    ApiError::new(status, msg).with_detail(e.body_text())
}

/// A query string as `(key, value)` pairs, every key one of `allowed`, every value of sane length.
pub fn check_query(pairs: &[(String, String)], allowed: &[&str]) -> Result<(), ApiError> {
    for (k, v) in pairs {
        if !allowed.contains(&k.as_str()) {
            return Err(bad_request(format!(
                "unknown query parameter {k:?} (expected: {})",
                allowed.join(", ")
            )));
        }
        check_text(k, v)?;
    }
    for key in allowed {
        if pairs.iter().filter(|(k, _)| k == key).count() > MAX_REPEAT {
            return Err(bad_request(format!(
                "at most {MAX_REPEAT} {key:?} parameters"
            )));
        }
    }
    Ok(())
}

/// One line of at most [`MAX_PARAM_BYTES`] with no control characters.
pub fn check_text(what: &str, v: &str) -> Result<(), ApiError> {
    if v.len() > MAX_PARAM_BYTES {
        return Err(bad_request(format!(
            "{what}: at most {MAX_PARAM_BYTES} bytes"
        )));
    }
    if v.chars().any(char::is_control) {
        return Err(bad_request(format!(
            "{what}: control characters not allowed"
        )));
    }
    Ok(())
}

/// The single value of `key`; an error when it is given more than once.
pub fn single<'a>(pairs: &'a [(String, String)], key: &str) -> Result<Option<&'a str>, ApiError> {
    let mut it = pairs.iter().filter(|(k, _)| k == key);
    let first = it.next().map(|(_, v)| v.as_str());
    if it.next().is_some() {
        return Err(bad_request(format!("{key:?} given more than once")));
    }
    Ok(first)
}

/// Like [`single`], but the parameter must be there and not empty.
pub fn required<'a>(pairs: &'a [(String, String)], key: &str) -> Result<&'a str, ApiError> {
    match single(pairs, key)? {
        Some(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(bad_request(format!("missing query parameter {key:?}"))),
    }
}

/// Every value of a repeatable parameter.
pub fn all<'a>(pairs: &'a [(String, String)], key: &str) -> Vec<&'a str> {
    pairs
        .iter()
        .filter(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .collect()
}

/// A comparison side (REF): run id, id prefix, folder, label, `group:NAME` or `group:NAME@KEY=VALUE`, written
/// the way core reads it ([`Reference::parse`]).
pub fn check_ref(what: &str, r: &str) -> Result<(), ApiError> {
    check_text(what, r)?;
    Reference::parse(r).map_err(|e| bad_request(format!("{what}: {e}")))?;
    Ok(())
}

/// A metric id, section or version: letters, digits and `. _ - + :`.
pub fn check_word(what: &str, v: &str) -> Result<(), ApiError> {
    if v.is_empty()
        || v.len() > 128
        || !v
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+' | ':'))
    {
        return Err(bad_request(format!(
            "{what}: use 1-128 letters, digits, . _ - + or :"
        )));
    }
    Ok(())
}
