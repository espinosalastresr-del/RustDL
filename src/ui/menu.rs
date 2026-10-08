//! Interactive terminal panel for the human-friendly RustDL frontend.

use crate::config::{Config, Profile};
use crate::errors::{DownloadError, Result};
use crate::storage::state::find_incomplete;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug)]
pub enum Action {
    NewDownload { url: String, output: Option<String> },
    ResumeAll,
    List,
    Verify(PathBuf),
    History,
    Settings,
    Exit,
}

pub fn run(cfg: &mut Config) -> Result<Action> {
    clear_screen();
    print_banner();

    loop {
        let incomplete = find_incomplete(&cfg.download_dir)?;
        println!();
        println!("  Downloads: {} incomplete", incomplete.len());
        println!("  Location : {}", cfg.download_dir.display());
        println!("  Profile  : {}", cfg.profile);
        println!(
            "  Mode     : {}",
            if cfg.data_saver {
                "Data saver"
            } else {
                "Normal"
            }
        );
        println!();
        println!("  ┌──────────────────────────────────────────────┐");
        println!("  │  1  New download                             │");
        println!("  │  2  Resume downloads                         │");
        println!("  │  3  Downloads & queue                        │");
        println!("  │  4  Verify a file                            │");
        println!("  │  5  Settings                                 │");
        println!("  │  6  History                                  │");
        println!("  │  0  Exit                                     │");
        println!("  └──────────────────────────────────────────────┘");
        println!();

        match prompt("  Select an option [1-6, 0]: ")? {
            Some(choice) => match choice.as_str() {
                "1" => return new_download(cfg),
                "2" => return Ok(Action::ResumeAll),
                "3" => return Ok(Action::List),
                "4" => return verify_file(),
                "5" => {
                    settings(cfg)?;
                    clear_screen();
                    print_banner();
                }
                "6" => return Ok(Action::History),
                "0" => return Ok(Action::Exit),
                _ => {
                    println!("  Invalid option. Choose a number from 0 to 6.");
                    pause()?;
                    clear_screen();
                    print_banner();
                }
            },
            None => return Ok(Action::Exit),
        }
    }
}

fn new_download(cfg: &Config) -> Result<Action> {
    clear_screen();
    print_section("NEW DOWNLOAD");
    println!("  Files are saved to: {}", cfg.download_dir.display());
    println!();

    let url = loop {
        match prompt("  URL: ")? {
            Some(value) if is_http_url(&value) => break value,
            Some(_) => {
                println!("  Please enter a valid http:// or https:// URL.");
            }
            None => return Ok(Action::Exit),
        }
    };

    let output = prompt("  Filename (Enter = automatic): ")?;
    let output = output.filter(|value| !value.is_empty());

    println!();
    println!("  Current profile: {}", cfg.profile);
    println!("  1) Resilient  — maximum recovery, lowest resource use");
    println!("  2) Stable     — balanced reliability and speed");
    println!("  3) Fast       — more parallelism, higher bandwidth use");
    println!("  Enter to keep current profile.");
    if let Some(choice) = prompt("  Profile [1-3]: ")? {
        match choice.as_str() {
            "1" => cfg.apply_profile(Profile::Resilient),
            "2" => cfg.apply_profile(Profile::Stable),
            "3" => cfg.apply_profile(Profile::Fast),
            _ => println!("  Keeping current profile."),
        }
    }

    println!();
    println!(
        "  Data saver is currently {}.",
        if cfg.data_saver { "ON" } else { "OFF" }
    );
    if let Some(choice) = prompt("  Toggle data saver? [y/N]: ")? {
        if matches!(choice.to_ascii_lowercase().as_str(), "y" | "yes") {
            cfg.data_saver = !cfg.data_saver;
            if cfg.data_saver {
                cfg.connections = 1;
            }
        }
    }

    Ok(Action::NewDownload { url, output })
}

fn verify_file() -> Result<Action> {
    clear_screen();
    print_section("VERIFY FILE");
    println!("  SHA-256 is used when no expected checksum is supplied.");
    println!();
    match prompt("  File path: ")? {
        Some(path) if !path.is_empty() => Ok(Action::Verify(PathBuf::from(path))),
        _ => Ok(Action::Exit),
    }
}

fn settings(cfg: &mut Config) -> Result<()> {
    loop {
        clear_screen();
        print_section("SETTINGS");
        println!("  1) Network profile     : {}", cfg.profile);
        println!("  2) Data saver          : {}", on_off(cfg.data_saver));
        println!("  3) Download directory  : {}", cfg.download_dir.display());
        println!("  4) Parallel connections: {}", cfg.connections);
        println!("  5) Save settings");
        println!("  0) Back");
        println!();

        match prompt("  Select [0-5]: ")? {
            Some(choice) => match choice.as_str() {
                "1" => {
                    if let Some(value) = prompt("  Profile [1=resilient, 2=stable, 3=fast]: ")? {
                        match value.as_str() {
                            "1" => cfg.apply_profile(Profile::Resilient),
                            "2" => cfg.apply_profile(Profile::Stable),
                            "3" => cfg.apply_profile(Profile::Fast),
                            _ => println!("  Invalid profile."),
                        }
                        pause()?;
                    }
                }
                "2" => {
                    cfg.data_saver = !cfg.data_saver;
                    if cfg.data_saver {
                        cfg.connections = 1;
                    }
                }
                "3" => {
                    if let Some(value) = prompt("  Directory: ")? {
                        if !value.is_empty() {
                            cfg.download_dir = PathBuf::from(value);
                        }
                    }
                }
                "4" => {
                    if cfg.data_saver {
                        println!("  Data saver keeps parallel connections at 1.");
                        pause()?;
                    } else if let Some(value) = prompt("  Connections [1-16]: ")? {
                        match value.parse::<u32>() {
                            Ok(n) if (1..=16).contains(&n) => cfg.connections = n,
                            _ => println!("  Enter a number from 1 to 16."),
                        }
                        pause()?;
                    }
                }
                "5" => {
                    cfg.save()?;
                    println!("  Settings saved to {}", Config::config_path().display());
                    pause()?;
                }
                "0" => return Ok(()),
                _ => {
                    println!("  Invalid option.");
                    pause()?;
                }
            },
            None => return Ok(()),
        }
    }
}

fn is_http_url(value: &str) -> bool {
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
        assert!(!is_http_url("https:///missing-host"));
    }
}
