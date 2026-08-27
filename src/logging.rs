//! Logging setup with optional file under ~/.rustdl/logs/

use crate::config::Config;
use std::fs::{self, OpenOptions};
use std::io::Write;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

pub fn init(level: &str, json: bool) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let _ = fs::create_dir_all(Config::logs_dir());
    rotate_log_if_needed();

    if json {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json().with_writer(std::io::stderr))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().with_target(false).with_writer(std::io::stderr))
            .init();
    }
}

fn rotate_log_if_needed() {
    let path = Config::logs_dir().join("rustdl.log");
    if let Ok(meta) = path.metadata() {
        if meta.len() > MAX_LOG_BYTES {
            let bak = Config::logs_dir().join("rustdl.log.1");
            let _ = fs::rename(&path, &bak);
        }
    }
}

/// Append a line to the log file (best-effort).
pub fn append_file_log(line: &str) {
    let path = Config::logs_dir().join("rustdl.log");
    let _ = fs::create_dir_all(Config::logs_dir());
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{}", line);
    }
}
