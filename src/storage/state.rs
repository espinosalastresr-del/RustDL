//! Download state persistence (checkpoints).

use crate::errors::{DownloadError, Result};
use crate::storage::atomic::atomic_write_string;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Paused,
    Completed,
    Failed,
    Verifying,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentState {
    pub index: u32,
    pub start: u64,
    pub end: u64, // inclusive end, or total-1
    pub downloaded: u64,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    pub id: String,
    pub url: String,
    pub final_url: Option<String>,
    pub filename: String,
    pub output_path: PathBuf,
    pub part_path: PathBuf,
    pub status: DownloadStatus,
    pub downloaded: u64,
    pub total_size: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub accept_ranges: bool,
    pub sha256: Option<String>,
    pub sha512: Option<String>,
    pub sha1: Option<String>,
    pub md5: Option<String>,
    pub segments: Vec<SegmentState>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub retries: u32,
    pub error: Option<String>,
}

impl DownloadState {
    pub fn new(url: &str, filename: &str, output_path: PathBuf, part_path: PathBuf) -> Self {
        let now = Utc::now();
        let id = short_id();
        Self {
            id,
            url: url.to_string(),
            final_url: None,
            filename: filename.to_string(),
            output_path,
            part_path,
            status: DownloadStatus::Pending,
            downloaded: 0,
            total_size: None,
            etag: None,
            last_modified: None,
            accept_ranges: false,
            sha256: None,
            sha512: None,
            sha1: None,
            md5: None,
            segments: vec![],
            created_at: now,
            updated_at: now,
            retries: 0,
            error: None,
        }
    }

    pub fn state_file_path(&self) -> PathBuf {
        crate::storage::filesystem::state_path_for_part(&self.part_path)
    }

    pub fn save(&self) -> Result<()> {
        let mut s = self.clone();
        s.updated_at = Utc::now();
        let json = serde_json::to_string_pretty(&s)?;
        atomic_write_string(&self.state_file_path(), &json)
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn load_for_part(part: &Path) -> Result<Self> {
        let p = crate::storage::filesystem::state_path_for_part(part);
        Self::load(&p)
    }

    pub fn progress_pct(&self) -> f64 {
        match self.total_size {
            Some(t) if t > 0 => (self.downloaded as f64 / t as f64) * 100.0,
            _ => 0.0,
        }
    }

    pub fn sync_downloaded_from_disk(&mut self) -> Result<()> {
        if self.part_path.exists() {
            self.downloaded = crate::storage::filesystem::file_size(&self.part_path)?;
        }
        Ok(())
    }
}

fn short_id() -> String {
    Uuid::new_v4().to_string()[..6].to_string()
}

/// Scan download dir / state dir for incomplete downloads.
pub fn find_incomplete(download_dir: &Path) -> Result<Vec<DownloadState>> {
    let mut out = Vec::new();
    if !download_dir.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(download_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("part") {
            // Prefer sibling .part.json
            if let Ok(state) = DownloadState::load_for_part(&path) {
                if state.status != DownloadStatus::Completed {
                    out.push(state);
                }
            } else {
                // Reconstruct minimal state
                let filename = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                // strip .part from stem if double
                let final_name = filename.trim_end_matches(".part");
                let output = path.with_file_name(final_name);
                let size = crate::storage::filesystem::file_size(&path).unwrap_or(0);
                let mut st = DownloadState::new("", final_name, output, path.clone());
                st.downloaded = size;
                st.status = DownloadStatus::Paused;
                out.push(st);
            }
        }
    }
    // Also scan global state dir
    let state_dir = crate::config::Config::state_dir();
    if state_dir.exists() {
        for entry in std::fs::read_dir(&state_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(state) = DownloadState::load(&path) {
                    if state.status != DownloadStatus::Completed
                        && !out.iter().any(|s| s.id == state.id)
                    {
                        out.push(state);
                    }
                }
            }
        }
    }
    Ok(out)
}

pub fn append_history(state: &DownloadState) -> Result<()> {
    use std::io::Write;
    let path = crate::config::Config::history_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let line = serde_json::to_string(state)?;
    writeln!(f, "{}", line)?;
    Ok(())
}
