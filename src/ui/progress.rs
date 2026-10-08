//! Live terminal download monitor with safe pause handling.

use crate::downloader::engine::{human_bytes, ProgressSnapshot};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Terminal,
};
use std::{
    io::{self, Stdout},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::watch;

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let _ = execute!(io::stdout(), LeaveAlternateScreen);
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

pub fn spawn_progress_ui(
    mut rx: watch::Receiver<ProgressSnapshot>,
    quiet: bool,
    silent: bool,
    json: bool,
    cancel: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if silent {
            return;
        }
        if json {
            while rx.changed().await.is_ok() {
                let p = rx.borrow().clone();
                println!(
                    "{}",
                    serde_json::json!({
                        "downloaded": p.downloaded,
                        "total": p.total,
                        "speed": p.speed,
                        "avg_speed": p.avg_speed,
                        "retries": p.retries,
                        "status": p.status,
                    })
                );
            }
            return;
        }
        if quiet {
            return;
        }

        let mut terminal = match TerminalGuard::new() {
            Ok(guard) => guard,
            Err(_) => {
                // Fall back to a simple progress line if the terminal cannot be controlled.
                while rx.changed().await.is_ok() {
                    print_progress_line(&rx.borrow().clone());
                }
                eprintln!();
                return;
            }
        };

        let mut latest = rx.borrow().clone();
        let mut disconnected = false;
        loop {
            if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind == KeyEventKind::Press
                        && matches!(key.code, KeyCode::Char('p') | KeyCode::Esc)
                    {
                        cancel.store(true, Ordering::SeqCst);
                        latest.status = "pausing — saving progress".into();
                    }
                }
            }

            match rx.has_changed() {
                Ok(true) => latest = rx.borrow_and_update().clone(),
                Ok(false) => {}
                Err(_) => disconnected = true,
            }

            let p = latest.clone();
            let _ = terminal.terminal.draw(|frame| {
                let area = frame.size();
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(2)
                    .constraints([
                        Constraint::Length(3),
                        Constraint::Length(3),
                        Constraint::Length(5),
                        Constraint::Min(4),
                        Constraint::Length(3),
                    ])
                    .split(area);

                let title = Paragraph::new(Line::from(vec![
                    Span::styled(" RustDL ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::raw(" / DOWNLOAD MONITOR"),
                ]))
                .block(Block::default().borders(Borders::ALL).title(" Transfer "));
                frame.render_widget(title, chunks[0]);

                let (ratio, label) = match p.total {
                    Some(total) if total > 0 => {
                        let ratio = (p.downloaded as f64 / total as f64).clamp(0.0, 1.0);
                        (ratio, format!("{:.1}%  ·  {} / {}", ratio * 100.0, human_bytes(p.downloaded), human_bytes(total)))
                    }
                    _ => (0.0, format!("{} downloaded  ·  total size unknown", human_bytes(p.downloaded))),
                };
                let gauge = Gauge::default()
                    .block(Block::default().borders(Borders::ALL).title(" Progress "))
                    .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
                    .ratio(ratio)
                    .label(label);
                frame.render_widget(gauge, chunks[1]);

                let eta = match p.total {
                    Some(total) if p.avg_speed > 0.0 => format_duration(total.saturating_sub(p.downloaded) as f64 / p.avg_speed),
                    _ => "—".into(),
                };
                let metrics = vec![
                    Line::from(vec![Span::styled("Current speed  ", Style::default().fg(Color::Gray)), Span::styled(format!("{}/s", human_bytes(p.speed as u64)), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]),
                    Line::from(vec![Span::styled("Average speed  ", Style::default().fg(Color::Gray)), Span::raw(format!("{}/s", human_bytes(p.avg_speed as u64)))]),
                    Line::from(vec![Span::styled("Time remaining ", Style::default().fg(Color::Gray)), Span::raw(eta)]),
                    Line::from(vec![Span::styled("Retries        ", Style::default().fg(Color::Gray)), Span::raw(p.retries.to_string())]),
                ];
                frame.render_widget(Paragraph::new(metrics).block(Block::default().borders(Borders::ALL).title(" Network ")), chunks[2]);

                let status = Paragraph::new(vec![
                    Line::from(vec![Span::styled("Status: ", Style::default().fg(Color::Gray)), Span::styled(p.status.clone(), Style::default().fg(if p.status.contains("pausing") { Color::Yellow } else { Color::Green }).add_modifier(Modifier::BOLD))]),
                    Line::from("Progress is checkpointed by the download engine; pausing preserves the partial file."),
                ])
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).title(" Activity "));
                frame.render_widget(status, chunks[3]);

                let help = Paragraph::new(" [P] Pause safely    [Esc] Pause and return    Keep this terminal open ")
                    .style(Style::default().fg(Color::Yellow))
                    .block(Block::default().borders(Borders::ALL));
                frame.render_widget(help, chunks[4]);
            });

            if disconnected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
}

fn format_duration(secs: f64) -> String {
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
