//! Terminal progress display.

use crate::downloader::engine::{human_bytes, ProgressSnapshot};
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;
use tokio::sync::watch;

pub fn spawn_progress_ui(
    mut rx: watch::Receiver<ProgressSnapshot>,
    quiet: bool,
    silent: bool,
    json: bool,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if silent {
            return;
        }
        if json {
            while rx.changed().await.is_ok() {
                let p = rx.borrow().clone();
                let obj = serde_json::json!({
                    "downloaded": p.downloaded,
                    "total": p.total,
                    "speed": p.speed,
                    "avg_speed": p.avg_speed,
                    "retries": p.retries,
                    "status": p.status,
                });
                println!("{}", obj);
            }
            return;
        }
        if quiet {
            return;
        }

        let pb = ProgressBar::new(0);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg}\n{bar:40.cyan/blue} {percent}%\n{bytes}/{total_bytes}  Speed: {bytes_per_sec}  ETA: {eta}  Retries: {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("█░"),
        );

        loop {
            tokio::select! {
                changed = rx.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let p = rx.borrow().clone();
                    if let Some(total) = p.total {
                        pb.set_length(total);
                    }
                    pb.set_position(p.downloaded);
                    let eta = if p.avg_speed > 0.0 {
                        if let Some(t) = p.total {
                            let left = t.saturating_sub(p.downloaded) as f64;
                            let secs = left / p.avg_speed;
                            format_eta(secs)
                        } else {
                            "—".into()
                        }
                    } else {
                        "—".into()
                    };
                    pb.set_message(format!(
                        "Speed: {}/s  Avg: {}/s  ETA: {}  Retries: {}",
                        human_bytes(p.speed as u64),
                        human_bytes(p.avg_speed as u64),
                        eta,
                        p.retries
                    ));
                }
                _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            }
        }
        pb.finish_and_clear();
    })
}

fn format_eta(secs: f64) -> String {
    if !secs.is_finite() || secs < 0.0 {
        return "—".into();
    }
    let s = secs as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{}h {}m", h, m)
    } else if m > 0 {
        format!("{}m {}s", m, sec)
    } else {
        format!("{}s", sec)
    }
}

pub fn print_progress_line(p: &ProgressSnapshot) {
    let pct = match p.total {
        Some(t) if t > 0 => (p.downloaded as f64 / t as f64) * 100.0,
        _ => 0.0,
    };
    let bar_len = 20usize;
    let filled = ((pct / 100.0) * bar_len as f64) as usize;
    let bar: String = std::iter::repeat('█')
        .take(filled.min(bar_len))
        .chain(std::iter::repeat('░').take(bar_len.saturating_sub(filled)))
        .collect();
    eprint!(
        "\r{} {:.1}%  {} / {}  Speed: {}/s  Retries: {}    ",
        bar,
        pct,
        human_bytes(p.downloaded),
        p.total.map(human_bytes).unwrap_or_else(|| "?".into()),
        human_bytes(p.speed as u64),
        p.retries
    );
}
