//! Error types and result aliases.

use camino::Utf8PathBuf;

/// Crate-wide result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Crate-wide domain errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid path for {kind}: {path} ({reason})")]
    InvalidPath {
        kind: &'static str,
        path: Utf8PathBuf,
        reason: &'static str,
    },
    #[error("invalid value for {option}: {value}; expected {expected}")]
    InvalidOptionValue {
        option: &'static str,
        value: String,
        expected: &'static str,
    },
    #[error("validation error: {message}")]
    Validation { message: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Csv(#[from] csv::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}
