//! Persistent download queue.

use crate::errors::{DownloadError, Result};
use crate::storage::atomic::atomic_write_string;
use crate::storage::state::DownloadState;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Queue {
    pub items: Vec<QueueItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: String,
    pub url: String,
    pub output_name: Option<String>,
    pub added_at: chrono::DateTime<chrono::Utc>,
    pub status: QueueStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueStatus {
    Queued,
    Active,
    Done,
    Failed,
}

impl Queue {
    fn path() -> PathBuf {
        crate::config::Config::rustdl_dir().join("queue.json")
    }

    pub fn load() -> Result<Self> {
        let path = Self::path();
        if path.exists() {
            let s = std::fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&s)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        atomic_write_string(&Self::path(), &json)
    }

    pub fn add(&mut self, url: &str, output_name: Option<String>) -> QueueItem {
        let item = QueueItem {
            id: uuid::Uuid::new_v4().to_string()[..6].to_string(),
            url: url.to_string(),
            output_name,
            added_at: chrono::Utc::now(),
            status: QueueStatus::Queued,
        };
        self.items.push(item.clone());
        item
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        if self.items.len() == before {
            return Err(DownloadError::NotFound(format!("Queue item {}", id)));
        }
        Ok(())
    }

    pub fn list(&self) -> &[QueueItem] {
        &self.items
    }
}
