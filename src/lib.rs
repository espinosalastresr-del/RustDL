//! rustdl-core — resilient download engine library.
//!
//! The CLI binary (`rustdl`) is a thin frontend over this crate.
//! Future TUI / daemon / local API can reuse the same engine.

pub mod config;
pub mod downloader;
pub mod errors;
pub mod logging;
pub mod metadata;
pub mod queue;
pub mod storage;
pub mod ui;
pub mod verification;

pub use config::{Config, Profile};
pub use downloader::engine::{human_bytes, DownloadOptions, Engine, ProgressSnapshot};
pub use errors::{DownloadError, Result};
pub use storage::state::{DownloadState, DownloadStatus};
