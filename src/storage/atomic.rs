//! Atomic file writes to avoid corrupt state on crash.

use crate::errors::{DownloadError, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Write content atomically: write to .tmp then rename.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = tmp_path(path);
    {
        let mut f = File::create(&tmp)?;
        f.write_all(content)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn atomic_write_string(path: &Path, content: &str) -> Result<()> {
    atomic_write(path, content.as_bytes())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_owned();
    p.push(".tmp");
    PathBuf::from(p)
}

/// Atomically rename part file to final name when possible.
pub fn atomic_rename(from: &Path, to: &Path) -> Result<()> {
    if to.exists() {
        return Err(DownloadError::FileExists(to.to_path_buf()));
    }
    fs::rename(from, to).map_err(|e| {
        DownloadError::Storage(format!(
            "Failed to rename {} -> {}: {}",
            from.display(),
            to.display(),
            e
        ))
    })
}
