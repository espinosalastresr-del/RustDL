//! rustdl CLI — thin frontend over the `rustdl` library.

use clap::{CommandFactory, Parser};
use rustdl::config::{parse_size, Config, Profile};
use rustdl::downloader::engine::{human_bytes, DownloadOptions, Engine, ProgressSnapshot};
use rustdl::errors::{DownloadError, Result};
use rustdl::queue::{Queue, QueueStatus};
use rustdl::storage::state::find_incomplete;
use rustdl::ui;
use rustdl::verification::checksum::{hash_file, verify_file, HashAlgo};
use rustdl::{logging, DownloadStatus};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::watch;
use tracing::error;

mod cli;
use cli::{Cli, Commands, ConfigCmd, Shell};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let log_level = if cli.verbose {
        "debug"
    } else {
        cli.log_level.as_str()
    };
    logging::init(log_level, cli.json);

    if let Err(e) = run(cli).await {
        if !matches!(e, DownloadError::Cancelled) {
            eprintln!("Error: {}", e);
            error!("{}", e);
            std::process::exit(1);
        }
        std::process::exit(130);
    }
}

async fn run(mut cli: Cli) -> Result<()> {
    Config::ensure_dirs()?;
    let mut cfg = Config::load()?;

    if cli.safe {
        cfg.apply_safe_mode();
    } else if let Some(ref p) = cli.profile {
        cfg.apply_profile(p.parse()?);
    }

    if let Some(ref d) = cli.output_dir {
        cfg.download_dir = d.clone();
    }
    if let Some(ref r) = cli.retries {
        if r.eq_ignore_ascii_case("infinite") || r == "0" {
            cfg.retries.max_retries = None;
        } else {
            cfg.retries.max_retries = Some(
                r.parse()
                    .map_err(|_| DownloadError::Config(format!("Invalid retries: {}", r)))?,
            );
        }
    }
    if let Some(d) = cli.max_retry_delay {
        cfg.retries.max_delay_secs = d;
    }
    if let Some(t) = cli.connect_timeout {
        cfg.timeouts.connect_secs = t;
    }
    if let Some(t) = cli.idle_timeout {
        cfg.timeouts.idle_secs = t;
    }
    if let Some(c) = cli.connections {
        cfg.connections = c.max(1);
    }
    if let Some(ref lr) = cli.limit_rate {
        cfg.limit_rate = Some(parse_size(lr)?);
    }
    if let Some(i) = cli.checkpoint_interval {
        cfg.checkpoint_interval_secs = i;
    }
    if let Some(ref s) = cli.checkpoint_size {
        cfg.checkpoint_size_bytes = parse_size(s)?;
    }
    if let Some(ref ua) = cli.user_agent {
        cfg.user_agent = ua.clone();
    }
    if cli.insecure {
        eprintln!("WARNING: TLS verification disabled (--insecure)");
        cfg.verify_tls = false;
    }
    if cli.data_saver {
        cfg.data_saver = true;
        cfg.connections = 1;
    }
    if let Some(m) = cli.max_redirects {
        cfg.max_redirects = m;
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_c = cancel.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        eprintln!("\nInterrupted — saving progress...");
        cancel_c.store(true, Ordering::SeqCst);
    });

    let command = cli.command.take();
    match command {
        Some(Commands::Download { url }) => {
            do_download(&cfg, &cli, &url, cancel).await?;
        }
        Some(Commands::List) => {
            let items = find_incomplete(&cfg.download_dir)?;
            if items.is_empty() {
                println!("No incomplete downloads.");
            } else {
                for s in items {
                    println!(
                        "{}\t{}\t{:.1}%\t{}\t{:?}",
                        s.id,
                        s.filename,
                        s.progress_pct(),
                        human_bytes(s.downloaded),
                        s.status
                    );
                }
            }
        }
        Some(Commands::Add { url, output }) => {
            let mut q = Queue::load()?;
            let item = q.add(&url, output);
            q.save()?;
            println!("Added {} ({})", item.id, item.url);
        }
        Some(Commands::Start) => {
            let mut q = Queue::load()?;
            let pending: Vec<_> = q
                .items
                .iter()
                .filter(|i| i.status == QueueStatus::Queued)
                .cloned()
                .collect();
            if pending.is_empty() {
                println!("Queue empty.");
                return Ok(());
            }
            for item in pending {
                println!("Starting {}...", item.id);
                if let Some(qi) = q.items.iter_mut().find(|i| i.id == item.id) {
                    qi.status = QueueStatus::Active;
                }
                q.save()?;
                match do_download(&cfg, &cli, &item.url, cancel.clone()).await {
                    Ok(_) => {
                        if let Some(qi) = q.items.iter_mut().find(|i| i.id == item.id) {
                            qi.status = QueueStatus::Done;
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed {}: {}", item.id, e);
                        if let Some(qi) = q.items.iter_mut().find(|i| i.id == item.id) {
                            qi.status = QueueStatus::Failed;
                        }
                    }
                }
                q.save()?;
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
            }
        }
        Some(Commands::Resume { id }) => {
            let items = find_incomplete(&cfg.download_dir)?;
            let targets: Vec<_> = if let Some(ref id) = id {
                items.into_iter().filter(|s| &s.id == id).collect()
            } else {
                items
            };
            if targets.is_empty() {
                println!("Nothing to resume.");
                return Ok(());
            }
            for s in targets {
                if s.url.is_empty() {
                    eprintln!("Skip {}: no URL in state", s.id);
                    continue;
                }
                println!("Resuming {} ({})...", s.id, s.filename);
                do_download(&cfg, &cli, &s.url, cancel.clone()).await?;
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
            }
        }
        Some(Commands::Pause { id: _ }) => {
            println!("Use Ctrl+C to pause an active download. Progress is saved automatically.");
        }
        Some(Commands::Retry { id }) => {
            let items = find_incomplete(&cfg.download_dir)?;
            let s = items
                .into_iter()
                .find(|s| s.id == id)
                .ok_or_else(|| DownloadError::NotFound(id))?;
            do_download(&cfg, &cli, &s.url, cancel).await?;
        }
        Some(Commands::Remove { id }) => {
            let mut q = Queue::load()?;
            let _ = q.remove(&id);
            q.save()?;
            let items = find_incomplete(&cfg.download_dir)?;
            for s in items {
                if s.id == id {
                    let _ = std::fs::remove_file(&s.part_path);
                    let _ = std::fs::remove_file(s.state_file_path());
                    println!("Removed {}", id);
                    return Ok(());
                }
            }
            println!("Removed queue item {} (if present)", id);
        }
        Some(Commands::Info { id }) => {
            let items = find_incomplete(&cfg.download_dir)?;
            let s = items
                .into_iter()
                .find(|s| s.id == id)
                .ok_or_else(|| DownloadError::NotFound(id))?;
            println!("{}", serde_json::to_string_pretty(&s)?);
        }
        Some(Commands::History) => {
            let path = Config::history_path();
            if !path.exists() {
                println!("No history.");
                return Ok(());
            }
            let content = std::fs::read_to_string(&path)?;
            for line in content.lines().rev().take(50) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    println!(
                        "{}\t{}\t{:?}",
                        v["id"].as_str().unwrap_or("?"),
                        v["filename"].as_str().unwrap_or("?"),
                        v["status"]
                    );
                }
            }
        }
        Some(Commands::Verify {
            file,
            sha256,
            sha512,
            sha1,
            md5,
        }) => {
            let has_any = sha256.is_some() || sha512.is_some() || sha1.is_some() || md5.is_some();
            if let Some(h) = sha256 {
                verify_file(&file, HashAlgo::Sha256, &h)?;
                println!("SHA-256 OK");
            }
            if let Some(h) = sha512 {
                verify_file(&file, HashAlgo::Sha512, &h)?;
                println!("SHA-512 OK");
            }
            if let Some(h) = sha1 {
                verify_file(&file, HashAlgo::Sha1, &h)?;
                println!("SHA-1 OK");
            }
            if let Some(h) = md5 {
                verify_file(&file, HashAlgo::Md5, &h)?;
                println!("MD5 OK");
            }
            if !has_any {
                let h = hash_file(&file, HashAlgo::Sha256)?;
                println!("SHA-256: {}", h);
            }
        }
        Some(Commands::Config { action }) => match action.unwrap_or(ConfigCmd::Show) {
            ConfigCmd::Show => println!("{}", toml::to_string_pretty(&cfg).unwrap()),
            ConfigCmd::Path => println!("{}", Config::config_path().display()),
            ConfigCmd::Init => {
                cfg.save()?;
                println!("Wrote {}", Config::config_path().display());
            }
        },
        Some(Commands::Completion { shell }) => {
            let mut cmd = Cli::command();
            let sh = match shell {
                Shell::Bash => clap_complete::Shell::Bash,
                Shell::Zsh => clap_complete::Shell::Zsh,
                Shell::Fish => clap_complete::Shell::Fish,
                Shell::Elvish => clap_complete::Shell::Elvish,
                Shell::PowerShell => clap_complete::Shell::PowerShell,
            };
            clap_complete::generate(sh, &mut cmd, "rustdl", &mut std::io::stdout());
        }
        None => {
            if let Some(ref url) = cli.url {
                do_download(&cfg, &cli, url, cancel).await?;
            } else {
                let mut cmd = Cli::command();
                cmd.print_help().ok();
                println!();
            }
        }
    }
    Ok(())
}

async fn do_download(cfg: &Config, cli: &Cli, url: &str, cancel: Arc<AtomicBool>) -> Result<()> {
    let engine = Engine::new(cfg.clone())?;
    let basic_auth = cli.basic_auth.as_ref().and_then(|s| {
        let mut p = s.splitn(2, ':');
        Some((p.next()?.to_string(), p.next().unwrap_or("").to_string()))
    });
    let opts = DownloadOptions {
        url: url.to_string(),
        output_dir: cfg.download_dir.clone(),
        output_name: cli.output.clone(),
        force_resume: cli.force_resume,
        restart: cli.restart,
        overwrite: cli.overwrite,
        sha256: cli.sha256.clone(),
        sha512: cli.sha512.clone(),
        sha1: cli.sha1.clone(),
        md5: cli.md5.clone(),
        headers: cli.parse_headers(),
        basic_auth,
        bearer: cli.bearer.clone(),
        yes: cli.yes,
        quiet: cli.quiet,
        silent: cli.silent,
        json: cli.json,
    };

    let (tx, rx) = watch::channel(ProgressSnapshot {
        downloaded: 0,
        total: None,
        speed: 0.0,
        avg_speed: 0.0,
        retries: 0,
        status: "starting".into(),
    });

    let _ui = ui::progress::spawn_progress_ui(rx, cli.quiet, cli.silent, cli.json);
    let result = engine.download(opts, Some(tx), cancel).await;

    match result {
        Ok(state) => {
            if !cli.silent && !cli.quiet {
                println!(
                    "\nCompleted: {} ({})",
                    state.output_path.display(),
                    human_bytes(state.downloaded)
                );
            }
            let _ = state.status;
            Ok(())
        }
        Err(e) => Err(e),
    }
}
