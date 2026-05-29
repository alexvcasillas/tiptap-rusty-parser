//! Error types.

use thiserror::Error;

/// Errors produced while parsing or serializing a document.
#[derive(Debug, Error)]
pub enum ParseError {
    /// Underlying `serde_json` (de)serialization failure.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// I/O failure while reading from a reader.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Crate result alias.
pub type Result<T> = std::result::Result<T, ParseError>;
