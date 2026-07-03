//! Error types and result aliases.

/// Crate-wide result type.
pub type Result<T> = std::result::Result<T, Error>;

/// Crate-wide error placeholder.
#[derive(Debug, thiserror::Error)]
#[error("corrodeql error")]
pub struct Error;
