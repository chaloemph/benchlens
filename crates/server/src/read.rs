//! Read endpoints of API v1 (docs/api.md): runs, files, groups, compare, values, metrics.

use crate::params::{
    all, bad_request, check_query, check_ref, check_text, check_word, db, from_path_rejection,
    from_query_rejection, required, single,
};
use crate::{ApiError, AppState};
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use benchlens_core::compare::{self, Comparison, Side, ValueRow};
use benchlens_core::store::{valid_tag_key, GroupSummary, RunDetail, RunSummary};
use serde::Serialize;

type Pairs = Result<Query<Vec<(String, String)>>, QueryRejection>;

fn pairs(q: Pairs) -> Result<Vec<(String, String)>, ApiError> {
    q.map(|Query(p)| p).map_err(from_query_rejection)
}

/// Files a run may keep (docs/contract.md); anything else is 404 without asking the store.
const KEPT_FILES: &[&str] = &[
    "meta.json",
    "results.tsv",
    "status.tsv",
    "hardware.json",
    "report.md",
    "benchlens-tags.json",
];

/// Filters of `GET /api/runs`; all must match.
#[derive(Debug, Default)]
struct RunFilter {
    tags: Vec<(String, String)>,
    arch: Option<String>,
    group: Option<String>,
    q: Option<String>,
}

impl RunFilter {
    fn parse(p: &[(String, String)]) -> Result<RunFilter, ApiError> {
        check_query(p, &["tag", "arch", "group", "q"])?;
        let mut tags = Vec::new();
        for t in all(p, "tag") {
            let (k, v) = t
                .split_once(':')
                .ok_or_else(|| bad_request(format!("tag filter {t:?}: use tag=KEY:VALUE")))?;
            if !valid_tag_key(k) {
                return Err(bad_request(format!(
                    "tag filter {t:?}: the key must be 1-64 letters, digits, _ - or ."
                )));
            }
            tags.push((k.to_string(), v.to_string()));
        }
        let opt = |key: &str| -> Result<Option<String>, ApiError> {
            Ok(single(p, key)?
                .filter(|v| !v.is_empty())
                .map(str::to_string))
        };
        Ok(RunFilter {
            tags,
            arch: opt("arch")?,
            group: opt("group")?,
            q: opt("q")?.map(|q| q.to_lowercase()),
        })
    }

    fn matches(&self, r: &RunSummary) -> bool {
        self.tags
            .iter()
            .all(|(k, v)| r.tags.get(k).is_some_and(|x| x == v))
            && self.arch.as_ref().is_none_or(|a| &r.arch == a)
            && self.group.as_ref().is_none_or(|g| &r.group == g)
            && self.q.as_ref().is_none_or(|q| {
                [&r.label, &r.folder, &r.group]
                    .into_iter()
                    .chain(r.tags.values())
                    .any(|s| s.to_lowercase().contains(q.as_str()))
            })
    }
}

pub async fn list_runs(
    State(st): State<AppState>,
    q: Pairs,
) -> Result<Json<Vec<RunSummary>>, ApiError> {
    let filter = RunFilter::parse(&pairs(q)?)?;
    let runs = db(&st, |s| Ok(s.list_runs()?)).await?;
    Ok(Json(
        runs.into_iter().filter(|r| filter.matches(r)).collect(),
    ))
}

/// A run in a path: id, unique id prefix (4+), folder or label.
pub fn check_run_ref(id: &str) -> Result<(), ApiError> {
    check_text("run", id)?;
    if id.trim().is_empty() || id.starts_with("group:") {
        return Err(bad_request(format!("{id:?} is not a run reference")));
    }
    Ok(())
}

pub async fn get_run(
    State(st): State<AppState>,
    id: Result<Path<String>, PathRejection>,
) -> Result<Json<RunDetail>, ApiError> {
    let Path(id) = id.map_err(from_path_rejection)?;
    check_run_ref(&id)?;
    let detail = db(&st, move |s| {
        let run_id = s.find_run(&id)?;
        Ok(s.run_detail(&run_id)?)
    })
    .await?;
    Ok(Json(detail))
}

pub async fn get_run_file(
    State(st): State<AppState>,
    p: Result<Path<(String, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let Path((id, name)) = p.map_err(from_path_rejection)?;
    check_run_ref(&id)?;
    if !KEPT_FILES.contains(&name.as_str()) {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            format!("no file {name:?} (kept files: {})", KEPT_FILES.join(", ")),
        ));
    }
    let content_type = if name.ends_with(".json") {
        "application/json"
    } else {
        "text/plain; charset=utf-8"
    };
    let bytes = db(&st, move |s| {
        let run_id = s.find_run(&id)?;
        Ok(s.file(&run_id, &name)?)
    })
    .await?;
    Ok(([(header::CONTENT_TYPE, content_type)], bytes).into_response())
}

pub async fn list_groups(State(st): State<AppState>) -> Result<Json<Vec<GroupSummary>>, ApiError> {
    Ok(Json(db(&st, |s| Ok(s.list_groups()?)).await?))
}

pub async fn compare(State(st): State<AppState>, q: Pairs) -> Result<Json<Comparison>, ApiError> {
    let p = pairs(q)?;
    check_query(&p, &["base", "other", "section"])?;
    let base = required(&p, "base")?.to_string();
    check_ref("base", &base)?;
    let others: Vec<String> = all(&p, "other").into_iter().map(str::to_string).collect();
    if others.is_empty() {
        return Err(bad_request(
            "nothing to compare the baseline with: give at least one other=REF",
        ));
    }
    for o in &others {
        check_ref("other", o)?;
    }
    let section = single(&p, "section")?
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(s) = &section {
        check_word("section", s)?;
    }
    let mut c = db(&st, move |s| Ok(compare::compare(s, &base, &others)?)).await?;
    if let Some(s) = section {
        c.rows.retain(|r| r.section == s);
    }
    Ok(Json(c))
}

#[derive(Serialize)]
pub struct Values {
    pub side: Side,
    pub id: String,
    pub values: Vec<ValueRow>,
}

pub async fn values(State(st): State<AppState>, q: Pairs) -> Result<Json<Values>, ApiError> {
    let p = pairs(q)?;
    check_query(&p, &["ref", "id"])?;
    let reference = required(&p, "ref")?.to_string();
    check_ref("ref", &reference)?;
    let id = required(&p, "id")?.to_string();
    check_word("id", &id)?;
    let metric = id.clone();
    let (side, values) = db(&st, move |s| {
        Ok(compare::load_metric(s, &reference, &metric)?)
    })
    .await?;
    Ok(Json(Values { side, id, values }))
}

pub async fn metrics(
    State(st): State<AppState>,
    q: Pairs,
) -> Result<Json<Vec<benchlens_core::catalog::Metric>>, ApiError> {
    let p = pairs(q)?;
    check_query(&p, &["version"])?;
    let version = required(&p, "version")?.to_string();
    check_word("version", &version)?;
    Ok(Json(db(&st, move |s| Ok(s.metrics(&version)?)).await?))
}
