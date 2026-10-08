//! Interactive terminal panel for the human-friendly RustDL frontend.

use crate::config::{Config, Profile};
use crate::errors::{DownloadError, Result};
use crate::queue::{Queue, QueueStatus};
use crate::storage::state::{find_incomplete, DownloadState};
use crossterm::{event::{self, Event, KeyCode, KeyEvent, KeyModifiers}, execute, terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen}};
use ratatui::{backend::CrosstermBackend, layout::{Constraint, Direction, Layout, Rect}, style::{Color, Modifier, Style}, text::{Line, Span}, widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table, TableState, Wrap}, Terminal};
use std::{io::{self, Stdout}, path::PathBuf, time::Duration};

#[derive(Debug)]
pub enum Action {
    NewDownload { url: String, output: Option<String> },
    Resume(String),
    ResumeAll,
    Retry(String),
    Remove(String),
    Info(String),
    QueueAdd { url: String, output: Option<String> },
    QueueStart,
    Verify(PathBuf),
    History,
    Settings,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen { Dashboard, Downloads, NewDownload, QueueAdd, Verify, Settings, History, Help, Info, ConfirmRemove }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field { Url, Output, Path }

struct App {
    screen: Screen,
    selected: usize,
    field: Field,
    url: String,
    output: String,
    path: String,
    message: String,
    selected_id: Option<String>,
    settings_selected: usize,
    settings_dirty: bool,
    states: Vec<DownloadState>,
    queue: Queue,
    history: Vec<String>,
}

impl App {
    fn load(cfg: &Config) -> Result<Self> {
        Ok(Self {
            screen: Screen::Dashboard, selected: 0, field: Field::Url,
            url: String::new(), output: String::new(), path: String::new(),
            message: String::new(), selected_id: None, settings_selected: 0,
            settings_dirty: false, states: find_incomplete(&cfg.download_dir)?,
            queue: Queue::load()?, history: load_history()?,
        })
    }
    fn selected_state(&self) -> Option<&DownloadState> { self.states.get(self.selected) }
    fn move_selection(&mut self, delta: i32) {
        if self.states.is_empty() { self.selected = 0; return; }
        if delta < 0 { self.selected = self.selected.saturating_sub(delta.unsigned_abs() as usize); }
        else { self.selected = (self.selected + delta as usize).min(self.states.len() - 1); }
    }
    fn reset_form(&mut self, screen: Screen, field: Field) {
        self.screen = screen; self.field = field;
        self.url.clear(); self.output.clear(); self.path.clear(); self.message.clear();
    }
}

pub fn run(cfg: &mut Config) -> Result<Action> {
    let app = &mut App::load(cfg)?;
    let mut terminal = TerminalGuard::new()?;
    loop {
        terminal.draw(|f| render(f, app, cfg))?;
        if !event::poll(Duration::from_millis(150)).map_err(DownloadError::Io)? { continue; }
        let Event::Key(key) = event::read().map_err(DownloadError::Io)? else { continue; };
        if let Some(action) = handle_key(app, cfg, key)? { return Ok(action); }
    }
}

fn handle_key(app: &mut App, cfg: &mut Config, key: KeyEvent) -> Result<Option<Action>> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') { return Ok(Some(Action::Exit)); }
    match app.screen {
        Screen::Dashboard => match key.code {
            KeyCode::Char('n') => app.reset_form(Screen::NewDownload, Field::Url),
            KeyCode::Char('d') | KeyCode::Enter => app.screen = Screen::Downloads,
            KeyCode::Char('q') => app.reset_form(Screen::QueueAdd, Field::Url),
            KeyCode::Char('s') => app.screen = Screen::Settings,
            KeyCode::Char('h') => app.screen = Screen::History,
            KeyCode::Char('v') => app.reset_form(Screen::Verify, Field::Path),
            KeyCode::F(1) | KeyCode::Char('?') => app.screen = Screen::Help,
            KeyCode::Char('x') | KeyCode::Esc => return Ok(Some(Action::Exit)),
            _ => {}
        },
        Screen::Downloads => match key.code {
            KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
            KeyCode::Esc | KeyCode::Char('b') => app.screen = Screen::Dashboard,
            KeyCode::Char('n') => app.reset_form(Screen::NewDownload, Field::Url),
            KeyCode::Char('q') => app.reset_form(Screen::QueueAdd, Field::Url),
            KeyCode::Char('a') => return Ok(Some(Action::ResumeAll)),
            KeyCode::Char('r') | KeyCode::Enter => if let Some(s)=app.selected_state() { return Ok(Some(Action::Resume(s.id.clone()))); },
            KeyCode::Char('t') => if let Some(s)=app.selected_state() { return Ok(Some(Action::Retry(s.id.clone()))); },
            KeyCode::Char('d') => if let Some(s)=app.selected_state() { app.selected_id=Some(s.id.clone()); app.screen=Screen::ConfirmRemove; },
            KeyCode::Char('i') => if app.selected_state().is_some() { app.screen=Screen::Info; },
            KeyCode::F(1) | KeyCode::Char('?') => app.screen=Screen::Help,
            _ => {}
        },
        Screen::NewDownload | Screen::QueueAdd => {
            match key.code {
                KeyCode::Esc => app.screen=Screen::Dashboard,
                KeyCode::Tab | KeyCode::Down => app.field=match app.field { Field::Url=>Field::Output, _=>Field::Url },
                KeyCode::BackTab | KeyCode::Up => app.field=match app.field { Field::Url=>Field::Output, _=>Field::Url },
                KeyCode::Backspace => active_text(app).pop(),
                KeyCode::Char(c) => active_text(app).push(c),
                KeyCode::Enter => {
                    if !is_http_url(&app.url) { app.message="Enter a valid HTTP/HTTPS URL.".into(); }
                    else {
                        let url=app.url.trim().to_string(); let output=non_empty(&app.output);
                        let action=if app.screen==Screen::QueueAdd { Action::QueueAdd{url,output} } else { Action::NewDownload{url,output} };
                        return Ok(Some(action));
                    }
                }
                _=>{}
            }
        }
        Screen::Verify => match key.code {
            KeyCode::Esc=>app.screen=Screen::Dashboard,
            KeyCode::Backspace=>{app.path.pop();},
            KeyCode::Char(c)=>app.path.push(c),
            KeyCode::Enter=>if !app.path.trim().is_empty(){return Ok(Some(Action::Verify(PathBuf::from(app.path.trim()))));},
            _=>{}
        },
        Screen::Settings => match key.code {
            KeyCode::Esc=>{ if app.settings_dirty { cfg.save()?; app.settings_dirty=false; } app.screen=Screen::Dashboard; },
            KeyCode::Up|KeyCode::Char('k')=>app.settings_selected=app.settings_selected.saturating_sub(1),
            KeyCode::Down|KeyCode::Char('j')=>app.settings_selected=(app.settings_selected+1).min(4),
            KeyCode::Enter|KeyCode::Left|KeyCode::Right=>match app.settings_selected {
                0=>{cfg.profile=match cfg.profile{Profile::Resilient=>Profile::Stable,Profile::Stable=>Profile::Fast,Profile::Fast=>Profile::Resilient};cfg.apply_profile(cfg.profile);app.settings_dirty=true;},
                1=>{cfg.data_saver=!cfg.data_saver;if cfg.data_saver{cfg.connections=1;}app.settings_dirty=true;},
                2=>{cfg.connections=if cfg.connections>=16{1}else{cfg.connections+1};if cfg.data_saver{cfg.connections=1;}app.settings_dirty=true;},
                3=>{cfg.download_dir=dirs::home_dir().unwrap_or_else(||PathBuf::from(".")).join("Downloads");app.settings_dirty=true;},
                4=>{cfg.save()?;app.settings_dirty=false;},
                _=>{}
            },
            _=>{}
        },
        Screen::History=>if matches!(key.code,KeyCode::Esc|KeyCode::Char('q')){app.screen=Screen::Dashboard},
        Screen::Help=>if key.code!=KeyCode::F(1){app.screen=Screen::Dashboard},
        Screen::Info=>if key.code!=KeyCode::F(1){app.screen=Screen::Downloads},
        Screen::ConfirmRemove=>match key.code {
            KeyCode::Char('y')|KeyCode::Char('Y')|KeyCode::Enter=>if let Some(id)=app.selected_id.take(){app.screen=Screen::Downloads;return Ok(Some(Action::Remove(id)));},
            _=>app.screen=Screen::Downloads,
        }
    }
    Ok(None)
}

fn active_text(app:&mut App)->&mut String{match app.field{Field::Url=>&mut app.url,Field::Output|Field::Path=>&mut app.output}}
fn non_empty(s:&str)->Option<String>{let s=s.trim();if s.is_empty(){None}else{Some(s.to_string())}}

fn render(f:&mut ratatui::Frame, app:&App, cfg:&Config){
    let area=f.size();
    f.render_widget(Block::default().title(" RUSTDL • Reliable Downloads ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)),area);
    let inner=Rect::new(area.x+1,area.y+1,area.width.saturating_sub(2),area.height.saturating_sub(2));
    match app.screen{
        Screen::Dashboard=>dashboard(f,inner,app,cfg), Screen::Downloads=>downloads(f,inner,app),
        Screen::NewDownload|Screen::QueueAdd=>form(f,inner,app,false), Screen::Verify=>form(f,inner,app,true),
        Screen::Settings=>settings(f,inner,app,cfg), Screen::History=>history(f,inner,app),
        Screen::Help=>{dashboard(f,inner,app,cfg);help(f,area)}, Screen::Info=>{downloads(f,inner,app);info(f,area,app)},
        Screen::ConfirmRemove=>{downloads(f,inner,app);confirm(f,area,app)}
    }
}

fn dashboard(f:&mut ratatui::Frame,area:Rect,app:&App,cfg:&Config){
    let chunks=Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(5),Constraint::Min(7),Constraint::Length(2)]).split(area);
    let queued=app.queue.items.iter().filter(|x|x.status==QueueStatus::Queued).count();
    let overview=Paragraph::new(vec![Line::from(vec![Span::styled(" INCOMPLETE ",Style::default().fg(Color::Yellow)),Span::raw(app.states.len().to_string()),Span::raw("   "),Span::styled("QUEUE ",Style::default().fg(Color::Cyan)),Span::raw(queued.to_string())]),Line::from(format!(" {} • {} • {} connections",cfg.download_dir.display(),cfg.profile,if cfg.data_saver{"data saver"}else{"normal"}))]).block(Block::default().borders(Borders::ALL).title(" Overview "));
    f.render_widget(overview,chunks[0]);
    let items=vec![" [Enter] Downloads manager"," [n] New download"," [q] Add to queue"," [s] Settings"," [h] History"," [v] Verify checksum"," [?] Help"," [x] Exit"].into_iter().map(ListItem::new).collect::<Vec<_>>();
    f.render_widget(List::new(items).block(Block::default().borders(Borders::ALL).title(" Quick actions ")),chunks[1]);
    f.render_widget(Paragraph::new(" arrows/jk navigate • Enter select • F1 help • Ctrl+C exit").style(Style::default().fg(Color::DarkGray)),chunks[2]);
}

fn downloads(f:&mut ratatui::Frame,area:Rect,app:&App){
    let rows=app.states.iter().map(|s|Row::new(vec![Cell::from(s.id.clone()),Cell::from(truncate(&s.filename,34)),Cell::from(format!("{:.1}%",s.progress_pct())),Cell::from(format!("{:?}",s.status))]));
    let table=Table::new(rows,[Constraint::Length(8),Constraint::Min(20),Constraint::Length(9),Constraint::Length(13)]).header(Row::new(vec!["ID","FILE","PROGRESS","STATUS"]).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))).block(Block::default().borders(Borders::ALL).title(" Downloads ")).highlight_style(Style::default().bg(Color::DarkGray)).highlight_symbol("› ");
    let mut ts=TableState::default();if !app.states.is_empty(){ts.select(Some(app.selected));} f.render_stateful_widget(table,area,&mut ts);
    f.render_widget(Paragraph::new(" r resume • t retry • d delete • i info • a all • n new • q queue • Esc back").style(Style::default().fg(Color::DarkGray)),Rect::new(area.x+2,area.y+area.height.saturating_sub(2),area.width.saturating_sub(4),1));
}

fn form(f:&mut ratatui::Frame,area:Rect,app:&App,verify:bool){
    let title=if verify{" Verify file "}else if app.screen==Screen::QueueAdd{" Add to queue "}else{" New download "};
    let lines=if verify{vec![Line::from(vec![Span::styled("File ",Style::default().fg(Color::Cyan)),Span::raw(format!("{}▌",app.path))]),Line::from(""),Line::from("Enter confirm • Esc cancel")]}else{vec![Line::from(vec![Span::styled("URL      ",Style::default().fg(Color::Cyan)),Span::raw(if app.field==Field::Url{format!("{}▌",app.url)}else{app.url.clone()})]),Line::from(vec![Span::styled("Filename ",Style::default().fg(Color::Cyan)),Span::raw(if app.field==Field::Output{format!("{}▌",app.output)}else{app.output.clone()})]),Line::from(""),Line::from("Tab/↑↓ switch • Enter confirm • Esc cancel")]};
    f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(title)).wrap(Wrap{trim:false}),centered(area,64,12));
    if !app.message.is_empty(){f.render_widget(Paragraph::new(app.message.as_str()).style(Style::default().fg(Color::Yellow)),Rect::new(area.x+3,area.y+area.height.saturating_sub(3),area.width.saturating_sub(6),1));}
}

fn settings(f:&mut ratatui::Frame,area:Rect,app:&App,cfg:&Config){
    let vals=vec![format!("Network profile     {}",cfg.profile),format!("Data saver          {}",if cfg.data_saver{"ON"}else{"OFF"}),format!("Connections         {}",cfg.connections),format!("Download directory  {}",cfg.download_dir.display()),"Save configuration".into()];
    let list=List::new(vals.into_iter().map(ListItem::new).collect::<Vec<_>>()).block(Block::default().borders(Borders::ALL).title(" Settings ")).highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)).highlight_symbol("› ");
    let mut ls=ratatui::widgets::ListState::default();ls.select(Some(app.settings_selected));f.render_stateful_widget(list,centered(area,72,14),&mut ls);
    f.render_widget(Paragraph::new(" ↑↓/jk select • ←→/Enter change • Esc save & back").style(Style::default().fg(Color::DarkGray)),Rect::new(area.x+2,area.y+area.height.saturating_sub(2),area.width.saturating_sub(4),1));
}

fn history(f:&mut ratatui::Frame,area:Rect,app:&App){
    let text=app.history.iter().take(30).filter_map(|l|serde_json::from_str::<serde_json::Value>(l).ok()).map(|v|format!("{}  {}  {:?}",v["id"].as_str().unwrap_or("?"),v["filename"].as_str().unwrap_or("?"),v["status"])).collect::<Vec<_>>().join("\n");
    let text=if text.is_empty(){"No download history yet.".into()}else{text};
    f.render_widget(Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" History ")).wrap(Wrap{trim:true}),area);
}

fn help(f:&mut ratatui::Frame,area:Rect){
    let text="n New  • d Downloads  • q Queue  • s Settings  • h History  • v Verify\nr Resume  • t Retry  • a Resume all  • i Info  • x Exit\nj/k or arrows Navigate  • Enter Confirm  • Esc Back\nF1 or ? Help";
    let r=centered(area,62,12);f.render_widget(Clear,r);f.render_widget(Paragraph::new(text).block(Block::default().borders(Borders::ALL).title(" Help ")).wrap(Wrap{trim:false}),r);
}

fn info(f:&mut ratatui::Frame,area:Rect,app:&App){
    if let Some(s)=app.selected_state(){let r=centered(area,88,24);f.render_widget(Clear,r);f.render_widget(Paragraph::new(serde_json::to_string_pretty(s).unwrap_or_default()).block(Block::default().borders(Borders::ALL).title(" Download details ")).wrap(Wrap{trim:false}),r);}
}
fn confirm(f:&mut ratatui::Frame,area:Rect,app:&App){let r=centered(area,58,8);f.render_widget(Clear,r);f.render_widget(Paragraph::new(format!("Delete partial download {}?\n\ny = delete • any other key = cancel",app.selected_id.as_deref().unwrap_or("?"))).block(Block::default().borders(Borders::ALL).title(" Confirm ")).wrap(Wrap{trim:true}),r);}
fn centered(a:Rect,w:u16,h:u16)->Rect{let w=w.min(a.width.saturating_sub(2)).max(20);let h=h.min(a.height.saturating_sub(2)).max(6);Rect::new(a.x+a.width.saturating_sub(w)/2,a.y+a.height.saturating_sub(h)/2,w,h)}
fn truncate(s:&str,max:usize)->String{if s.chars().count()<=max{s.into()}else{let mut x=s.chars().take(max.saturating_sub(1)).collect::<String>();x.push('…');x}}
fn load_history()->Result<Vec<String>>{let p=Config::history_path();if !p.exists(){return Ok(Vec::new())}Ok(std::fs::read_to_string(p)?.lines().rev().take(100).map(str::to_owned).collect())}

struct TerminalGuard{terminal:Terminal<CrosstermBackend<Stdout>>}
impl TerminalGuard{fn new()->Result<Self>{enable_raw_mode().map_err(DownloadError::Io)?;let mut out=io::stdout();execute!(out,EnterAlternateScreen).map_err(DownloadError::Io)?;Ok(Self{terminal:Terminal::new(CrosstermBackend::new(out)).map_err(DownloadError::Io)?})}fn draw<F>(&mut self,f:F)->Result<()> where F:FnOnce(&mut ratatui::Frame){self.terminal.draw(f).map(|_|()).map_err(DownloadError::Io)}}
impl Drop for TerminalGuard{fn drop(&mut self){let _=disable_raw_mode();let _=execute!(self.terminal.backend_mut(),LeaveAlternateScreen);let _=self.terminal.show_cursor();}}

fn is_http_url(value:&str)->bool{match url::Url::parse(value.trim()){Ok(p)=>matches!(p.scheme(),"http"|"https")&&p.host().is_some(),Err(_)=>false}}

#[cfg(test)]
mod tests{use super::is_http_url;#[test]fn accepts_http_and_https(){assert!(is_http_url("https://example.com/file"));assert!(is_http_url("http://example.com/file"));}#[test]fn rejects_invalid(){assert!(!is_http_url("ftp://example.com/file"));assert!(!is_http_url("not-a-url"));assert!(!is_http_url("https://"));}}(value: &str) -> bool {
    match url::Url::parse(value) {
        Ok(parsed) => matches!(parsed.scheme(), "http" | "https") && parsed.host().is_some(),
        Err(_) => false,
    }
}

fn prompt(label: &str) -> Result<Option<String>> {
    print!("{}", label);
    io::stdout().flush().map_err(DownloadError::Io)?;
    let mut input = String::new();
    let read = io::stdin()
        .read_line(&mut input)
        .map_err(DownloadError::Io)?;
    if read == 0 {
        return Ok(None);
    }
    Ok(Some(input.trim().to_string()))
}

fn pause() -> Result<()> {
    let _ = prompt("  Press Enter to continue...")?;
    Ok(())
}

fn clear_screen() {
    if std::env::var("TERM").map(|v| v != "dumb").unwrap_or(true) {
        print!("\x1b[2J\x1b[H");
        let _ = io::stdout().flush();
    }
}

fn print_banner() {
    println!("  ╔══════════════════════════════════════════════╗");
    println!("  ║                 R U S T D L                  ║");
    println!("  ║      Reliable downloads for bad networks     ║");
    println!("  ╚══════════════════════════════════════════════╝");
}

fn print_section(title: &str) {
    println!("  ┌──────────────────────────────────────────────┐");
    println!("  │ {:<44} │", title);
    println!("  └──────────────────────────────────────────────┘");
    println!();
}

fn on_off(value: bool) -> &'static str {
    if value {
        "ON"
    } else {
        "OFF"
    }
}

#[cfg(test)]
mod tests {
    use super::is_http_url;

    #[test]
    fn accepts_http_and_https_urls() {
        assert!(is_http_url("https://example.com/file.zip"));
        assert!(is_http_url("http://example.com/file.zip"));
    }

    #[test]
    fn rejects_non_http_urls() {
        assert!(!is_http_url("ftp://example.com/file.zip"));
        assert!(!is_http_url("not-a-url"));
        assert!(!is_http_url("https://"));
    }
}
