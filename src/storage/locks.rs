//! Cross-process locks to prevent concurrent downloads of the same file.

use crate::errors::{DownloadError, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

pub struct DownloadLock {
    _file: File,
    path: PathBuf,
}

impl DownloadLock {
    pub fn try_acquire(lock_dir: &Path, key: &str) -> Result<Self> {
        std::fs::create_dir_all(lock_dir)?;
        let path = lock_dir.join(format!("{}.lock", sanitize_key(key)));
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&path)?;
        file.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                DownloadError::AlreadyActive
            } else {
                DownloadError::Storage(format!("Lock error: {}", e))
            }
        })?;
        Ok(Self { _file: file, path })
    }
}

impl Drop for DownloadLock {
    fn drop(&mut self) {
        let _ = self._file.unlock();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn sanitize_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
