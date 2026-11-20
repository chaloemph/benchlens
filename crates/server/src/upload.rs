//! `POST /api/runs`: upload a `.tar` / `.tar.gz` of result folders (multipart field `file`, repeated
//! `tag` = `KEY=VALUE`) and import it with the same code as `benchlens import`, source `upload`.
//!
//! The file is streamed into a fresh folder under the system temp dir and counted while it arrives: one byte
//! over the limit ends the request with 413. The folder is removed on every path (it is a `TempDir`, dropped
//! when the request ends, also when the client goes away). Core reads the archive without extracting it and
//! refuses unsafe entries (400).

use crate::params::bad_request;
use crate::write::{WriteAccess, MAX_TAGS_PER_REQUEST};
use crate::{ApiError, AppState};
use axum::extract::multipart::{Field, MultipartError, MultipartRejection};
use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::Json;
use benchlens_core::import::{import_path, ImportOptions, ImportReport};
use benchlens_core::store::{self, MAX_TAG_VALUE};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

/// Room for the multipart framing and the `tag` fields on top of the file itself.
pub const MULTIPART_OVERHEAD: usize = 256 * 1024;
/// Longest `tag` field accepted (key, `=`, value of at most 1024 bytes).
const MAX_TAG_FIELD: usize = 64 + 1 + MAX_TAG_VALUE;

fn too_large(max: u64) -> ApiError {
    ApiError::new(
        StatusCode::PAYLOAD_TOO_LARGE,
        format!("upload larger than {} MiB", max.div_ceil(1024 * 1024)),
    )
}

fn from_multipart(e: MultipartError, max: u64) -> ApiError {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        too_large(max)
    } else {
        bad_request("bad multipart body").with_detail(e.body_text())
    }
}

/// A file name safe to use inside our temp folder; it also names a result folder that sits at the root of
/// the archive, as with `benchlens import name.tar.gz`.
fn safe_file_name(given: Option<&str>) -> String {
    let base = given.unwrap_or("").rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(128)
        .collect();
    let cleaned = cleaned.trim_start_matches('.');
    if cleaned.is_empty() {
        "upload.tar.gz".into()
    } else {
        cleaned.to_string()
    }
}

/// Read a small text field, refusing more than `max` bytes.
async fn small_text(field: &mut Field<'_>, max: usize, limit: u64) -> Result<String, ApiError> {
    let mut buf = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|e| from_multipart(e, limit))? {
        if buf.len() + chunk.len() > max {
            return Err(bad_request(format!(
                "a tag field is longer than {max} bytes"
            )));
        }
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf).map_err(|_| bad_request("a tag field is not UTF-8"))
}

/// `KEY=VALUE` with the store's rules for keys and values (an empty value removes the tag).
fn parse_tag(t: &str) -> Result<(String, String), ApiError> {
    store::parse_tag(t).map_err(|e| bad_request(e.to_string()))
}

/// Stream the `file` field into `path`, at most `max` bytes. Returns the size.
async fn save_file(field: &mut Field<'_>, path: &PathBuf, max: u64) -> Result<u64, ApiError> {
    let mut out = tokio::fs::File::create(path)
        .await
        .map_err(|e| ApiError::internal(format!("cannot create a temporary file: {e}")))?;
    let mut size = 0u64;
    while let Some(chunk) = field.chunk().await.map_err(|e| from_multipart(e, max))? {
        size += chunk.len() as u64;
        if size > max {
            return Err(too_large(max));
        }
        out.write_all(&chunk)
            .await
            .map_err(|e| ApiError::internal(format!("cannot write the temporary file: {e}")))?;
    }
    out.flush()
        .await
        .map_err(|e| ApiError::internal(format!("cannot write the temporary file: {e}")))?;
    Ok(size)
}

pub async fn upload_run(
    _w: WriteAccess,
    State(st): State<AppState>,
    multipart: Result<Multipart, MultipartRejection>,
) -> Result<Json<Vec<ImportReport>>, ApiError> {
    let max = st.max_upload_bytes;
    let mut mp = multipart.map_err(|e| {
        bad_request("send multipart/form-data with a `file` field").with_detail(e.body_text())
    })?;
    let dir = tempfile::Builder::new()
        .prefix("benchlens-upload-")
        .tempdir()
        .map_err(|e| ApiError::internal(format!("cannot create a temporary folder: {e}")))?;
    let mut file: Option<(PathBuf, u64)> = None;
    let mut tags = BTreeMap::new();
    let mut tag_fields = 0usize;
    while let Some(mut field) = mp.next_field().await.map_err(|e| from_multipart(e, max))? {
        match field.name() {
            Some("file") => {
                if file.is_some() {
                    return Err(bad_request("one `file` per upload"));
                }
                let path = dir.path().join(safe_file_name(field.file_name()));
                let size = save_file(&mut field, &path, max).await?;
                file = Some((path, size));
            }
            Some("tag") => {
                tag_fields += 1;
                if tag_fields > MAX_TAGS_PER_REQUEST {
                    return Err(bad_request(format!(
                        "at most {MAX_TAGS_PER_REQUEST} tags per upload"
                    )));
                }
                let text = small_text(&mut field, MAX_TAG_FIELD, max).await?;
                let (k, v) = parse_tag(&text)?;
                tags.insert(k, v);
            }
            other => {
                return Err(bad_request(format!(
                    "unknown form field {:?} (expected `file` and `tag`)",
                    other.unwrap_or("")
                )))
            }
        }
    }
    let Some((path, size)) = file else {
        return Err(bad_request("no `file` field: attach a .tar or .tar.gz"));
    };
    if size == 0 {
        return Err(bad_request("the uploaded file is empty"));
    }
    let opts = ImportOptions {
        source: "upload".into(),
        tags,
        registry: None,
        dry_run: false,
    };
    let store = st.store.clone();
    let reports = tokio::task::spawn_blocking(move || {
        let _keep = dir; // removed when this closure ends, whatever happens
        let guard = store
            .lock()
            .map_err(|_| ApiError::internal("the database lock is poisoned; restart benchlens"))?;
        import_path(&guard, &path, &opts).map_err(import_error)
    })
    .await
    .map_err(|e| ApiError::internal(format!("import task failed: {e}")))??;
    Ok(Json(reports))
}

/// What a refused archive answers: unsafe entries, no result folder, and unreadable tar/gzip data are the
/// client's (400); a database problem is ours (500).
fn import_error(e: benchlens_core::Error) -> ApiError {
    use benchlens_core::Error as E;
    match e {
        // the tar reader quotes raw header bytes in some messages: keep the readable part only
        E::Io(io) => {
            let detail: String = io
                .to_string()
                .chars()
                .take_while(|c| c.is_ascii_graphic() || *c == ' ')
                .take(160)
                .collect();
            bad_request("not a readable .tar or .tar.gz archive").with_detail(detail.trim_end())
        }
        other => ApiError::from(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(safe_file_name(Some("x86-std-1.tar.gz")), "x86-std-1.tar.gz");
        assert_eq!(safe_file_name(Some("../../etc/passwd")), "passwd");
        assert_eq!(safe_file_name(Some("C:\\a\\b c.tgz")), "bc.tgz");
        assert_eq!(safe_file_name(Some("..")), "upload.tar.gz");
        assert_eq!(safe_file_name(Some(".hidden")), "hidden");
        assert_eq!(safe_file_name(None), "upload.tar.gz");
    }

    #[test]
    fn tags() {
        assert_eq!(
            parse_tag(" round = 2 ").unwrap(),
            ("round".into(), "2".into())
        );
        assert_eq!(parse_tag("note=").unwrap(), ("note".into(), "".into()));
        assert!(parse_tag("novalue").is_err());
        assert!(parse_tag("bad key=1").is_err());
        assert!(parse_tag("=1").is_err());
    }
}
