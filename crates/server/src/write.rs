//! Write endpoints of API v1: tag editing and delete (upload is in [`crate::upload`]).
//!
//! Every write handler takes a [`WriteAccess`] argument; that extractor is the one place where the token
//! check happens.

use crate::auth::Denied;
use crate::params::{db, from_json_rejection, from_path_rejection};
use crate::read::check_run_ref;
use crate::{ApiError, AppState};
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::Json;
use benchlens_core::store::RunSummary;
use std::collections::BTreeMap;

/// Largest body accepted by `PATCH /api/runs/{id}/tags`.
pub const MAX_TAGS_BODY: usize = 64 * 1024;
/// Most tags changed by one request.
pub const MAX_TAGS_PER_REQUEST: usize = 64;

/// Proof that the request may write: the configured token was sent, or the server runs without a token on
/// loopback (see [`crate::auth`]). Add it as an argument to every write handler; extractors that read only
/// the head run before the body is read, so a refused upload is not received.
pub struct WriteAccess;

impl FromRequestParts<AppState> for WriteAccess {
    type Rejection = Denied;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        state.access.check_write(&parts.headers)?;
        Ok(WriteAccess)
    }
}

pub async fn patch_tags(
    _w: WriteAccess,
    State(st): State<AppState>,
    id: Result<Path<String>, PathRejection>,
    body: Result<Json<BTreeMap<String, String>>, JsonRejection>,
) -> Result<Json<RunSummary>, ApiError> {
    let Path(id) = id.map_err(from_path_rejection)?;
    check_run_ref(&id)?;
    let Json(tags) = body.map_err(from_json_rejection)?;
    if tags.len() > MAX_TAGS_PER_REQUEST {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("at most {MAX_TAGS_PER_REQUEST} tags per request"),
        ));
    }
    // key and value rules are checked by the store (Error::Contract -> 400)
    let summary = db(&st, move |s| {
        let run_id = s.find_run(&id)?;
        s.set_tags(&run_id, &tags)?;
        Ok(s.run_summary(&run_id)?)
    })
    .await?;
    Ok(Json(summary))
}

pub async fn delete_run(
    _w: WriteAccess,
    State(st): State<AppState>,
    id: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id.map_err(from_path_rejection)?;
    check_run_ref(&id)?;
    db(&st, move |s| {
        let run_id = s.find_run(&id)?;
        if !s.delete_run(&run_id)? {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                format!("no run {run_id}"),
            ));
        }
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}
