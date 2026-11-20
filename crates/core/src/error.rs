use std::fmt;

/// Everything that can go wrong in benchlens-core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The folder or archive does not hold a vmbench result benchlens can read.
    #[error("{0}")]
    Contract(String),
    /// An archive entry that could write outside the import folder, or is too big.
    #[error("unsafe archive entry {path}: {why}")]
    UnsafeArchive { path: String, why: String },
    /// A lookup (run, group, catalog) that found nothing.
    #[error("{0}")]
    NotFound(String),
    /// A reference that matches more than one run.
    #[error("{0}")]
    Ambiguous(String),
    /// A reference that is not written the way a REF is (docs/api.md), such as `group:NAME@KEY` with no value.
    #[error("{0}")]
    BadReference(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Csv(#[from] csv::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn contract(msg: impl fmt::Display) -> Self {
        Error::Contract(msg.to_string())
    }
}
