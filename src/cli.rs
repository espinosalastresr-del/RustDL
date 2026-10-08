//! CLI definition with clap.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "rustdl",
    version,
    about = "Resilient download manager optimized for unstable connections and iSH/iOS",
    long_about = "rustdl prioritizes integrity, resume, and progress preservation over raw speed.\n\
A poor connection should only make a download take longer — never force you to lose progress."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// URL to download (shorthand when no subcommand)
    #[arg(global = false)]
    pub url: Option<String>,

    /// Output directory
    #[arg(long, short = 'o', env = "RUSTDL_DOWNLOAD_DIR")]
    pub output_dir: Option<PathBuf>,

    /// Output filename
    #[arg(long, short = 'O')]
    pub output: Option<String>,

    /// Network profile: resilient | stable | fast
    #[arg(long, env = "RUSTDL_PROFILE")]
    pub profile: Option<String>,

    /// Safe mode (max resilience)
    #[arg(long)]
    pub safe: bool,

    /// Max retries (use "infinite" for unlimited)
    #[arg(long, env = "RUSTDL_RETRIES")]
    pub retries: Option<String>,

    /// Max retry delay in seconds
    #[arg(long)]
    pub max_retry_delay: Option<u64>,

    /// Connect timeout seconds
    #[arg(long)]
    pub connect_timeout: Option<u64>,

    /// Idle timeout seconds (no data received)
    #[arg(long)]
    pub idle_timeout: Option<u64>,

    /// Parallel connections (default 1 for resilient)
    #[arg(long)]
    pub connections: Option<u32>,

    /// Rate limit (e.g. 500K, 2M)
    #[arg(long, env = "RUSTDL_LIMIT_RATE")]
    pub limit_rate: Option<String>,

    /// Checkpoint every N seconds
    #[arg(long)]
    pub checkpoint_interval: Option<u64>,

    /// Checkpoint every N bytes
    #[arg(long)]
    pub checkpoint_size: Option<String>,

    /// Force resume even if metadata changed
    #[arg(long)]
    pub force_resume: bool,

    /// Restart download from scratch
    #[arg(long)]
    pub restart: bool,

    /// Overwrite existing final file
    #[arg(long)]
    pub overwrite: bool,

    /// Assume yes for prompts
    #[arg(long, short = 'y')]
    pub yes: bool,

    /// Quiet output
    #[arg(long, short = 'q')]
    pub quiet: bool,

    /// Silent (no progress)
    #[arg(long)]
    pub silent: bool,

    /// JSON progress lines
    #[arg(long)]
    pub json: bool,

    /// Verbose logging
    #[arg(long, short = 'v')]
    pub verbose: bool,

    /// Log level
    #[arg(long, default_value = "info")]
    pub log_level: String,

    /// SHA-256 expected hash
    #[arg(long)]
    pub sha256: Option<String>,

    /// SHA-512 expected hash
    #[arg(long)]
    pub sha512: Option<String>,

    /// SHA-1 expected hash
    #[arg(long)]
    pub sha1: Option<String>,

    /// MD5 expected hash
    #[arg(long)]
    pub md5: Option<String>,

    /// Custom header (repeatable): "Name: Value"
    #[arg(long = "header", short = 'H')]
    pub headers: Vec<String>,

    /// User-Agent
    #[arg(long)]
    pub user_agent: Option<String>,

    /// Disable TLS verification (INSECURE)
    #[arg(long)]
    pub insecure: bool,

    /// Data saver mode
    #[arg(long)]
    pub data_saver: bool,

    /// Max redirects
    #[arg(long)]
    pub max_redirects: Option<u32>,

    /// Basic auth user:password
    #[arg(long = "basic-auth")]
    pub basic_auth: Option<String>,

    /// Bearer token
    #[arg(long)]
    pub bearer: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Download a URL
    Download { url: String, },
    /// List incomplete / known downloads
    List,
    /// Add URL to queue
    Add {
        url: String,
        #[arg(long, short = 'O')]
        output: Option<String>,
    },
    /// Start queued downloads
    Start,
    /// Resume incomplete download(s)
    Resume {
        /// Download id (optional — resumes all if omitted)
        id: Option<String>,
    },
    /// Pause is cooperative via Ctrl+C; marks state
    Pause { id: Option<String>, },
    /// Retry a failed download
    Retry { id: String, },
    /// Remove download state / queue item
    Remove { id: String, },
    /// Show info about a download
    Info { id: String, },
    /// Show history
    History,
    /// Verify file checksum
    Verify {
        file: PathBuf,
        #[arg(long)]
        sha256: Option<String>,
        #[arg(long)]
        sha512: Option<String>,
        #[arg(long)]
        sha1: Option<String>,
        #[arg(long)]
        md5: Option<String>,
    },
    /// Show or edit config
    Config {
        #[command(subcommand)]
        action: Option<ConfigCmd>,
    },
    /// Generate shell completions
    Completion {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCmd {
    Show,
    Path,
    Init,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Elvish,
    PowerShell,
}

impl Cli {
    pub fn parse_headers(&self) -> Vec<(String, String)> {
        self.headers
            .iter()
            .filter_map(|h| {
                let mut parts = h.splitn(2, ':');
                let k = parts.next()?.trim().to_string();
                let v = parts.next()?.trim().to_string();
                if k.is_empty() {
                    None
                } else {
                    Some((k, v))
                }
            })
            .collect()
    }
}
