//! Professional interactive terminal UI for RustDL.

use crate::config::{Config, Profile};
use crate::errors::{DownloadError, Result};
use crate::queue::{Queue, QueueStatus};
use crate::storage::state::{find_incomplete, DownloadState};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, TableState, Wrap,
    },
    Terminal,
};
use std::{
    io::{self, Stdout},
    path::PathBuf,
    time::Duration,
};

#[derive(Debug)]
pub enum Action {
    NewDownload { url: String, output: Option<String> },
    Resume(String),
    ResumeAll,
    Retry(String),
    Remove(String),
    Info(String),
    QueueAdd { url: String, output: Option<String> },
    QueueRemove(String),
    QueueStart,
    Verify(PathBuf),
    History,
    Settings,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    Dashboard,
    Downloads,
    NewDownload,
    Queue,
    QueueAdd,
    Verify,
    Settings,
    History,
    Help,
    Info,
    ConfirmRemove,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Url,
    Output,
}

struct App {
    screen: Screen,
    selected: usize,
    field: Field,
    url: String,
    output: String,
    path: String,
    message: String,
    selected_id: Option<String>,
    queue_selected: usize,
    settings_selected: usize,
    settings_dirty: bool,
    states: Vec<DownloadState>,
    queue: Queue,
    history: Vec<String>,
}

impl App {
    fn load(cfg: &Config) -> Result<Self> {
        Ok(Self {
            screen: Screen::Dashboard,
            selected: 0,
            field: Field::Url,
            url: String::new(),
            output: String::new(),
            path: String::new(),
            message: String::new(),
            selected_id: None,
            queue_selected: 0,
            settings_selected: 0,
            settings_dirty: false,
            states: find_incomplete(&cfg.download_dir)?,
            queue: Queue::load()?,
            history: load_history()?,
        })
    }

    fn selected_state(&self) -> Option<&DownloadState> {
        self.states.get(self.selected)
    }

    fn move_selection(&mut self, delta: i32) {
        if self.states.is_empty() {
            self.selected = 0;
            return;
        }

        if delta < 0 {
            self.selected = self.selected.saturating_sub(delta.unsigned_abs() as usize);
        } else {
            self.selected = (self.selected + delta as usize).min(self.states.len() - 1);
        }
    }

    fn reset_form(&mut self, screen: Screen) {
        self.screen = screen;
        self.field = Field::Url;
        self.url.clear();
        self.output.clear();
        self.path.clear();
        self.message.clear();
    }
}

pub fn run(cfg: &mut Config) -> Result<Action> {
    let mut app = App::load(cfg)?;
    let mut terminal = TerminalGuard::new()?;

    loop {
        terminal.draw(|frame| render(frame, &app, cfg))?;

        if !event::poll(Duration::from_millis(150)).map_err(DownloadError::Io)? {
            continue;
        }

        let Event::Key(key) = event::read().map_err(DownloadError::Io)? else {
            continue;
        };

        if let Some(action) = handle_key(&mut app, cfg, key)? {
            return Ok(action);
        }
    }
}

fn handle_key(app: &mut App, cfg: &mut Config, key: KeyEvent) -> Result<Option<Action>> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(Some(Action::Exit));
    }

    match app.screen {
        Screen::Dashboard => dashboard_key(app, key),
        Screen::Downloads => downloads_key(app, key),
        Screen::Queue => queue_key(app, key),
        Screen::NewDownload | Screen::QueueAdd => form_key(app, key),
        Screen::Verify => verify_key(app, key),
        Screen::Settings => settings_key(app, cfg, key),
        Screen::History => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                app.screen = Screen::Dashboard;
            }
            Ok(None)
        }
        Screen::Help => {
            app.screen = Screen::Dashboard;
            Ok(None)
        }
        Screen::Info => {
            if key.code != KeyCode::F(1) {
                app.screen = Screen::Downloads;
            }
            Ok(None)
        }
        Screen::ConfirmRemove => confirm_remove_key(app, key),
    }
}

fn dashboard_key(app: &mut App, key: KeyEvent) -> Result<Option<Action>> {
    match key.code {
        KeyCode::Char('n') => app.reset_form(Screen::NewDownload),
        KeyCode::Char('d') | KeyCode::Enter => app.screen = Screen::Downloads,
        KeyCode::Char('q') => app.screen = Screen::Queue,
        KeyCode::Char('a') => app.reset_form(Screen::QueueAdd),
        KeyCode::Char('s') => app.screen = Screen::Settings,
        KeyCode::Char('h') => app.screen = Screen::History,
        KeyCode::Char('v') => {
            app.screen = Screen::Verify;
            app.path.clear();
        }
        KeyCode::F(1) | KeyCode::Char('?') => app.screen = Screen::Help,
        KeyCode::Char('x') | KeyCode::Esc => return Ok(Some(Action::Exit)),
        _ => {}
    }

    Ok(None)
}

fn downloads_key(app: &mut App, key: KeyEvent) -> Result<Option<Action>> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
        KeyCode::Esc | KeyCode::Char('b') => app.screen = Screen::Dashboard,
        KeyCode::Char('n') => app.reset_form(Screen::NewDownload),
        KeyCode::Char('q') => app.screen = Screen::Queue,
        KeyCode::Char('a') => return Ok(Some(Action::ResumeAll)),
        KeyCode::Char('r') | KeyCode::Enter => {
            if let Some(state) = app.selected_state() {
                return Ok(Some(Action::Resume(state.id.clone())));
            }
        }
        KeyCode::Char('t') => {
            if let Some(state) = app.selected_state() {
                return Ok(Some(Action::Retry(state.id.clone())));
            }
        }
        KeyCode::Char('d') => {
            if let Some(state) = app.selected_state() {
                app.selected_id = Some(state.id.clone());
                app.screen = Screen::ConfirmRemove;
            }
        }
        KeyCode::Char('i') => {
            if app.selected_state().is_some() {
                app.screen = Screen::Info;
            }
        }
        KeyCode::F(1) | KeyCode::Char('?') => app.screen = Screen::Help,
        _ => {}
    }

    Ok(None)
}

fn queue_key(app: &mut App, key: KeyEvent) -> Result<Option<Action>> {
    let len = app.queue.items.len();
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.queue_selected = app.queue_selected.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if len > 0 {
                app.queue_selected = (app.queue_selected + 1).min(len - 1);
            }
        }
        KeyCode::Esc | KeyCode::Char('b') => app.screen = Screen::Dashboard,
        KeyCode::Char('a') => app.reset_form(Screen::QueueAdd),
        KeyCode::Char('s') | KeyCode::Enter => {
            if len > 0 {
                return Ok(Some(Action::QueueStart));
            }
        }
        KeyCode::Char('d') => {
            if let Some(item) = app.queue.items.get(app.queue_selected) {
                return Ok(Some(Action::QueueRemove(item.id.clone())));
            }
        }
        KeyCode::F(1) | KeyCode::Char('?') => app.screen = Screen::Help,
        _ => {}
    }
    Ok(None)
}

fn form_key(app: &mut App, key: KeyEvent) -> Result<Option<Action>> {
    match key.code {
        KeyCode::Esc => app.screen = Screen::Dashboard,
        KeyCode::Tab | KeyCode::Down => {
            app.field = match app.field {
                Field::Url => Field::Output,
                Field::Output => Field::Url,
            };
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.field = match app.field {
                Field::Url => Field::Output,
                Field::Output => Field::Url,
            };
        }
        KeyCode::Backspace => {
            active_text(app).pop();
        }
        KeyCode::Char(c) => active_text(app).push(c),
        KeyCode::Enter => {
            if !is_http_url(&app.url) {
                app.message = "Enter a valid HTTP/HTTPS URL.".into();
            } else {
                let url = app.url.trim().to_string();
                let output = non_empty(&app.output);

                return Ok(Some(match app.screen {
                    Screen::QueueAdd => Action::QueueAdd { url, output },
                    _ => Action::NewDownload { url, output },
                }));
            }
        }
        _ => {}
    }

    Ok(None)
}

fn verify_key(app: &mut App, key: KeyEvent) -> Result<Option<Action>> {
    match key.code {
        KeyCode::Esc => app.screen = Screen::Dashboard,
        KeyCode::Backspace => {
            app.path.pop();
        }
        KeyCode::Char(c) => app.path.push(c),
        KeyCode::Enter => {
            if app.path.trim().is_empty() {
                app.message = "Enter a file path.".into();
            } else {
                return Ok(Some(Action::Verify(PathBuf::from(app.path.trim()))));
            }
        }
        _ => {}
    }

    Ok(None)
}

fn settings_key(app: &mut App, cfg: &mut Config, key: KeyEvent) -> Result<Option<Action>> {
    match key.code {
        KeyCode::Esc => {
            if app.settings_dirty {
                cfg.save()?;
                app.settings_dirty = false;
            }
            app.screen = Screen::Dashboard;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.settings_selected = app.settings_selected.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.settings_selected = (app.settings_selected + 1).min(4);
        }
        KeyCode::Enter | KeyCode::Left | KeyCode::Right => match app.settings_selected {
            0 => {
                cfg.profile = match cfg.profile {
                    Profile::Resilient => Profile::Stable,
                    Profile::Stable => Profile::Fast,
                    Profile::Fast => Profile::Resilient,
                };
                cfg.apply_profile(cfg.profile);
                app.settings_dirty = true;
            }
            1 => {
                cfg.data_saver = !cfg.data_saver;
                if cfg.data_saver {
                    cfg.connections = 1;
                }
                app.settings_dirty = true;
            }
            2 => {
                cfg.connections = if cfg.connections >= 16 {
                    1
                } else {
                    cfg.connections + 1
                };
                if cfg.data_saver {
                    cfg.connections = 1;
                }
                app.settings_dirty = true;
            }
            3 => {
                cfg.download_dir = dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("Downloads");
                app.settings_dirty = true;
            }
            4 => {
                cfg.save()?;
                app.settings_dirty = false;
            }
            _ => {}
        },
        _ => {}
    }

    Ok(None)
}

fn confirm_remove_key(app: &mut App, key: KeyEvent) -> Result<Option<Action>> {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
            if let Some(id) = app.selected_id.take() {
                app.screen = Screen::Downloads;
                return Ok(Some(Action::Remove(id)));
            }
        }
        _ => {
            app.screen = Screen::Downloads;
        }
    }

    Ok(None)
}

fn active_text(app: &mut App) -> &mut String {
    match app.field {
        Field::Url => &mut app.url,
        Field::Output => &mut app.output,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn render(frame: &mut ratatui::Frame, app: &App, cfg: &Config) {
    let area = frame.size();
    let outer = Block::default()
        .title(" RUSTDL • Reliable Downloads ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    frame.render_widget(outer, area);

    let inner = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );

    match app.screen {
        Screen::Dashboard => render_dashboard(frame, inner, app, cfg),
        Screen::Downloads => render_downloads(frame, inner, app),
        Screen::Queue => render_queue(frame, inner, app),
        Screen::NewDownload | Screen::QueueAdd => render_form(frame, inner, app, false),
        Screen::Verify => render_form(frame, inner, app, true),
        Screen::Settings => render_settings(frame, inner, app, cfg),
        Screen::History => render_history(frame, inner, app),
        Screen::Help => {
            render_dashboard(frame, inner, app, cfg);
            render_help(frame, area);
        }
        Screen::Info => {
            render_downloads(frame, inner, app);
            render_info(frame, area, app);
        }
        Screen::ConfirmRemove => {
            render_downloads(frame, inner, app);
            render_confirm(frame, area, app);
        }
    }
}

fn render_dashboard(frame: &mut ratatui::Frame, area: Rect, app: &App, cfg: &Config) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(7),
            Constraint::Length(2),
        ])
        .split(area);

    let queued = app
        .queue
        .items
        .iter()
        .filter(|item| item.status == QueueStatus::Queued)
        .count();

    let overview = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" INCOMPLETE ", Style::default().fg(Color::Yellow)),
            Span::raw(app.states.len().to_string()),
            Span::raw("   "),
            Span::styled("QUEUE ", Style::default().fg(Color::Cyan)),
            Span::raw(queued.to_string()),
        ]),
        Line::from(format!(
            " {} • {} • {} connections",
            cfg.download_dir.display(),
            cfg.profile,
            if cfg.data_saver {
                "data saver"
            } else {
                "normal"
            }
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Overview "));
    frame.render_widget(overview, chunks[0]);

    let items = vec![
        ListItem::new(" [Enter] Downloads manager"),
        ListItem::new(" [n] New download"),
        ListItem::new(" [q] Add to queue"),
        ListItem::new(" [s] Settings"),
        ListItem::new(" [h] History"),
        ListItem::new(" [v] Verify checksum"),
        ListItem::new(" [?] Help"),
        ListItem::new(" [x] Exit"),
    ];

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Quick actions "),
        ),
        chunks[1],
    );

    frame.render_widget(
        Paragraph::new(" shortcuts • F1 help • Ctrl+C exit")
            .style(Style::default().fg(Color::DarkGray)),
        chunks[2],
    );
}

fn render_downloads(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let rows = app.states.iter().map(|state| {
        Row::new(vec![
            Cell::from(state.id.clone()),
            Cell::from(truncate(&state.filename, 34)),
            Cell::from(format!("{:.1}%", state.progress_pct())),
            Cell::from(format!("{:?}", state.status)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Min(20),
            Constraint::Length(9),
            Constraint::Length(13),
        ],
    )
    .header(
        Row::new(vec!["ID", "FILE", "PROGRESS", "STATUS"]).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().borders(Borders::ALL).title(" Downloads "))
    .highlight_style(Style::default().bg(Color::DarkGray))
    .highlight_symbol("› ");

    let mut table_state = TableState::default();
    if !app.states.is_empty() {
        table_state.select(Some(app.selected));
    }
    frame.render_stateful_widget(table, area, &mut table_state);

    frame.render_widget(
        Paragraph::new(
            " r resume • t retry • d delete • i info • a all • n new • q queue • Esc back",
        )
        .style(Style::default().fg(Color::DarkGray)),
        Rect::new(
            area.x + 2,
            area.y + area.height.saturating_sub(2),
            area.width.saturating_sub(4),
            1,
        ),
    );
}

fn render_queue(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let rows = app.queue.items.iter().map(|item| {
        Row::new(vec![
            Cell::from(item.id.clone()),
            Cell::from(truncate(&item.url, 42)),
            Cell::from(format!("{:?}", item.status)),
        ])
    });

    let table = Table::new(
        rows,
        [
            Constraint::Length(8),
            Constraint::Min(30),
            Constraint::Length(12),
        ],
    )
    .header(
        Row::new(vec!["ID", "URL", "STATUS"]).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::default().borders(Borders::ALL).title(" Queue "))
    .highlight_style(Style::default().bg(Color::DarkGray))
    .highlight_symbol("› ");

    let mut state = TableState::default();
    if !app.queue.items.is_empty() {
        state.select(Some(app.queue_selected.min(app.queue.items.len() - 1)));
    }
    frame.render_stateful_widget(table, area, &mut state);
    frame.render_widget(
        Paragraph::new(" s/Enter start • a add • d remove • Esc back")
            .style(Style::default().fg(Color::DarkGray)),
        Rect::new(
            area.x + 2,
            area.y + area.height.saturating_sub(2),
            area.width.saturating_sub(4),
            1,
        ),
    );
}

fn render_form(frame: &mut ratatui::Frame, area: Rect, app: &App, verify: bool) {
    let title = if verify {
        " Verify file "
    } else if app.screen == Screen::QueueAdd {
        " Add to queue "
    } else {
        " New download "
    };

    let lines = if verify {
        vec![
            Line::from(vec![
                Span::styled("File ", Style::default().fg(Color::Cyan)),
                Span::raw(format!("{}▌", app.path)),
            ]),
            Line::from(""),
            Line::from("Enter confirm • Esc cancel"),
        ]
    } else {
        vec![
            Line::from(vec![
                Span::styled("URL      ", Style::default().fg(Color::Cyan)),
                Span::raw(if app.field == Field::Url {
                    format!("{}▌", app.url)
                } else {
                    app.url.clone()
                }),
            ]),
            Line::from(vec![
                Span::styled("Filename ", Style::default().fg(Color::Cyan)),
                Span::raw(if app.field == Field::Output {
                    format!("{}▌", app.output)
                } else {
                    app.output.clone()
                }),
            ]),
            Line::from(""),
            Line::from("Tab/↑↓ switch • Enter confirm • Esc cancel"),
        ]
    };

    let box_area = centered(area, 64, 12);
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false }),
        box_area,
    );

    if !app.message.is_empty() {
        frame.render_widget(
            Paragraph::new(app.message.as_str()).style(Style::default().fg(Color::Yellow)),
            Rect::new(
                area.x + 3,
                area.y + area.height.saturating_sub(3),
                area.width.saturating_sub(6),
                1,
            ),
        );
    }
}

fn render_settings(frame: &mut ratatui::Frame, area: Rect, app: &App, cfg: &Config) {
    let values = vec![
        format!("Network profile     {}", cfg.profile),
        format!(
            "Data saver          {}",
            if cfg.data_saver { "ON" } else { "OFF" }
        ),
        format!("Connections         {}", cfg.connections),
        format!("Download directory  {}", cfg.download_dir.display()),
        "Save configuration".to_string(),
    ];

    let list = List::new(values.into_iter().map(ListItem::new).collect::<Vec<_>>())
        .block(Block::default().borders(Borders::ALL).title(" Settings "))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");

    let mut list_state = ratatui::widgets::ListState::default();
    list_state.select(Some(app.settings_selected));
    frame.render_stateful_widget(list, centered(area, 72, 14), &mut list_state);

    frame.render_widget(
        Paragraph::new(" ↑↓/jk select • ←→/Enter change • Esc save & back")
            .style(Style::default().fg(Color::DarkGray)),
        Rect::new(
            area.x + 2,
            area.y + area.height.saturating_sub(2),
            area.width.saturating_sub(4),
            1,
        ),
    );
}

fn render_history(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let text = app
        .history
        .iter()
        .take(30)
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .map(|value| {
            format!(
                "{}  {}  {:?}",
                value["id"].as_str().unwrap_or("?"),
                value["filename"].as_str().unwrap_or("?"),
                value["status"]
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let text = if text.is_empty() {
        "No download history yet.".to_string()
    } else {
        text
    };

    frame.render_widget(
        Paragraph::new(text)
            .block(Block::default().borders(Borders::ALL).title(" History "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_help(frame: &mut ratatui::Frame, area: Rect) {
    let help = "n New • d Downloads • q Queue • s Settings • h History • v Verify\nr Resume • t Retry • a Resume all • i Info • x Exit\nj/k or arrows Navigate • Enter Confirm • Esc Back\nF1 or ? Help";

    let popup = centered(area, 62, 12);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(help)
            .block(Block::default().borders(Borders::ALL).title(" Help "))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_info(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let Some(state) = app.selected_state() else {
        return;
    };

    let popup = centered(area, 88, 24);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(serde_json::to_string_pretty(state).unwrap_or_default())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Download details "),
            )
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_confirm(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let popup = centered(area, 58, 8);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(format!(
            "Delete partial download {}?\n\ny = delete • any other key = cancel",
            app.selected_id.as_deref().unwrap_or("?")
        ))
        .block(Block::default().borders(Borders::ALL).title(" Confirm "))
        .wrap(Wrap { trim: true }),
        popup,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(20);
    let height = height.min(area.height.saturating_sub(2)).max(6);

    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let mut result = value
            .chars()
            .take(max.saturating_sub(1))
            .collect::<String>();
        result.push('…');
        result
    }
}

fn load_history() -> Result<Vec<String>> {
    let path = Config::history_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    Ok(std::fs::read_to_string(path)?
        .lines()
        .rev()
        .take(100)
        .map(str::to_owned)
        .collect())
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode().map_err(DownloadError::Io)?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen).map_err(DownloadError::Io)?;

        Ok(Self {
            terminal: Terminal::new(CrosstermBackend::new(stdout)).map_err(DownloadError::Io)?,
        })
    }

    fn draw<F>(&mut self, draw: F) -> Result<()>
    where
        F: FnOnce(&mut ratatui::Frame),
    {
        self.terminal
            .draw(draw)
            .map(|_| ())
            .map_err(DownloadError::Io)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
}

fn is_http_url(value: &str) -> bool {
    match url::Url::parse(value.trim()) {
        Ok(parsed) => matches!(parsed.scheme(), "http" | "https") && parsed.host().is_some(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_http_url;

    #[test]
    fn accepts_http_and_https() {
        assert!(is_http_url("https://example.com/file"));
        assert!(is_http_url("http://example.com/file"));
    }

    #[test]
    fn rejects_invalid() {
        assert!(!is_http_url("ftp://example.com/file"));
        assert!(!is_http_url("not-a-url"));
        assert!(!is_http_url("https://"));
    }
}
