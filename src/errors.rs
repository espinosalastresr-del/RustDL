//! Typed errors for rustdl.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Connection timeout after {0:?}")]
    Timeout(std::time::Duration),

    #[error("HTTP error {status}: {message}")]
    Http { status: u16, message: String },

    #[error("Resume not supported by server")]
    ResumeUnsupported,

    #[error("Remote file appears to have changed (ETag/Last-Modified mismatch)")]
    RemoteChanged {
        old_etag: Option<String>,
        new_etag: Option<String>,
    },

    #[error("Not enough disk space: required {required}, available {available}")]
    DiskFull { required: u64, available: u64 },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Download incomplete: expected {expected} bytes, got {actual}")]
    Incomplete { expected: u64, actual: u64 },

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Download cancelled")]
    Cancelled,

    #[error("Download already active for this file (lock held)")]
    AlreadyActive,

    #[error("File already exists: {0}")]
    FileExists(PathBuf),

    #[error("Invalid range response from server")]
    InvalidRange,

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP client error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

impl DownloadError {
    /// Whether this error is typically retryable.
    pub fn is_retryable(&self) -> bool {
        match self {
            DownloadError::Network(_)
            | DownloadError::Timeout(_)
            | DownloadError::Reqwest(_) => true,
            DownloadError::Http { status, .. } => matches!(
                *status,
                408 | 425 | 429 | 500 | 502 | 503 | 504
            ),
            DownloadError::Io(e) => {
                matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                )
            }
            _ => false,
        }
    }

    /// Extract Retry-After seconds if present in an HTTP error context.
    pub fn retry_after_secs(&self) -> Option<u64> {
        None // Populated at call site when 429 is received
    }
}

pub type Result<T> = std::result::Result<T, DownloadError>;
