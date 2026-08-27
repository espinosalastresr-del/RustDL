//! Configuration: CLI > env > config file > defaults.

use crate::errors::{DownloadError, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    #[default]
    Resilient,
    Stable,
    Fast,
}

impl std::str::FromStr for Profile {
    type Err = DownloadError;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "resilient" | "safe" => Ok(Profile::Resilient),
            "stable" => Ok(Profile::Stable),
            "fast" => Ok(Profile::Fast),
            _ => Err(DownloadError::Config(format!("Unknown profile: {}", s))),
        }
    }
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Profile::Resilient => write!(f, "resilient"),
            Profile::Stable => write!(f, "stable"),
            Profile::Fast => write!(f, "fast"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub download_dir: PathBuf,
    pub profile: Profile,
    pub retries: RetryConfig,
    pub timeouts: TimeoutConfig,
    pub connections: u32,
    pub checkpoint_interval_secs: u64,
    pub checkpoint_size_bytes: u64,
    pub max_redirects: u32,
    pub limit_rate: Option<u64>,
    pub log_level: String,
    pub user_agent: String,
    pub verify_tls: bool,
    pub data_saver: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// None = infinite
    pub max_retries: Option<u32>,
    pub initial_delay_ms: u64,
    pub max_delay_secs: u64,
    pub jitter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    pub connect_secs: u64,
    pub idle_secs: u64,
    pub request_secs: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            download_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Downloads"),
            profile: Profile::Resilient,
            retries: RetryConfig::default(),
            timeouts: TimeoutConfig::default(),
            connections: 1,
            checkpoint_interval_secs: 5,
            checkpoint_size_bytes: 10 * 1024 * 1024, // 10 MB
            max_redirects: 20,
            limit_rate: None,
            log_level: "info".into(),
            user_agent: format!("rustdl/{}", env!("CARGO_PKG_VERSION")),
            verify_tls: true,
            data_saver: false,
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: None, // infinite for resilient
            initial_delay_ms: 3000,
            max_delay_secs: 120,
            jitter: true,
        }
    }
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect_secs: 30,
            idle_secs: 120,
            request_secs: None,
        }
    }
}

impl Config {
    pub fn rustdl_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rustdl")
    }

    pub fn config_path() -> PathBuf {
        Self::rustdl_dir().join("config.toml")
    }

    pub fn state_dir() -> PathBuf {
        Self::rustdl_dir().join("state")
    }

    pub fn logs_dir() -> PathBuf {
        Self::rustdl_dir().join("logs")
    }

    pub fn locks_dir() -> PathBuf {
        Self::rustdl_dir().join("locks")
    }

    pub fn history_path() -> PathBuf {
        Self::rustdl_dir().join("history.jsonl")
    }

    pub fn ensure_dirs() -> Result<()> {
        let base = Self::rustdl_dir();
        for d in [
            &base,
            &Self::state_dir(),
            &Self::logs_dir(),
            &Self::locks_dir(),
            &base.join("cache"),
        ] {
            std::fs::create_dir_all(d).map_err(|e| {
                DownloadError::Storage(format!("Failed to create {}: {}", d.display(), e))
            })?;
        }
        Ok(())
    }

    pub fn load() -> Result<Self> {
        Self::ensure_dirs()?;
        let path = Self::config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let cfg: Config = toml::from_str(&content)
                .map_err(|e| DownloadError::Config(format!("Parse error: {}", e)))?;
            Ok(cfg)
        } else {
            let cfg = Config::default();
            cfg.save()?;
            Ok(cfg)
        }
    }

    pub fn save(&self) -> Result<()> {
        Self::ensure_dirs()?;
        let content = toml::to_string_pretty(self)
            .map_err(|e| DownloadError::Config(format!("Serialize error: {}", e)))?;
        let path = Self::config_path();
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Apply profile defaults (can be overridden by explicit CLI later).
    pub fn apply_profile(&mut self, profile: Profile) {
        self.profile = profile;
        match profile {
            Profile::Resilient => {
                self.connections = 1;
                self.retries.max_retries = None;
                self.retries.initial_delay_ms = 3000;
                self.retries.max_delay_secs = 120;
                self.timeouts.idle_secs = 180;
                self.timeouts.connect_secs = 45;
                self.checkpoint_interval_secs = 5;
                self.checkpoint_size_bytes = 5 * 1024 * 1024;
                self.data_saver = true;
            }
            Profile::Stable => {
                self.connections = 1;
                self.retries.max_retries = Some(50);
                self.retries.initial_delay_ms = 2000;
                self.retries.max_delay_secs = 60;
                self.timeouts.idle_secs = 90;
                self.timeouts.connect_secs = 30;
                self.checkpoint_interval_secs = 10;
                self.checkpoint_size_bytes = 10 * 1024 * 1024;
            }
            Profile::Fast => {
                self.connections = 4;
                self.retries.max_retries = Some(10);
                self.retries.initial_delay_ms = 1000;
                self.retries.max_delay_secs = 30;
                self.timeouts.idle_secs = 30;
                self.timeouts.connect_secs = 15;
                self.checkpoint_interval_secs = 15;
                self.checkpoint_size_bytes = 32 * 1024 * 1024;
            }
        }
    }

    pub fn apply_safe_mode(&mut self) {
        self.apply_profile(Profile::Resilient);
        self.connections = 1;
        self.retries.max_retries = None;
        self.data_saver = true;
    }

    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.timeouts.connect_secs)
    }

    pub fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.timeouts.idle_secs)
    }
}

/// Parse human-readable size: 500K, 2M, 1G, etc.
pub fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim().to_uppercase();
    let (num_str, mult) = if s.ends_with('G') {
        (&s[..s.len() - 1], 1024u64 * 1024 * 1024)
    } else if s.ends_with('M') {
        (&s[..s.len() - 1], 1024 * 1024)
    } else if s.ends_with('K') {
        (&s[..s.len() - 1], 1024)
    } else if s.ends_with('B') {
        (&s[..s.len() - 1], 1)
    } else {
        (s.as_str(), 1)
    };
    let n: u64 = num_str
        .parse()
        .map_err(|_| DownloadError::Config(format!("Invalid size: {}", s)))?;
    Ok(n.saturating_mul(mult))
}
