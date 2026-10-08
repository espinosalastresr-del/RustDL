//! Filesystem helpers: space check, sanitize names, ensure dirs.

use crate::errors::Result;
use std::path::{Path, PathBuf};

/// Sanitize filename: strip path components, remove dangerous chars.
pub fn sanitize_filename(name: &str) -> String {
    let name_owned = name.replace('\\', "/");
    let base = name_owned
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .trim()
        .trim_start_matches('.');

    let mut out = String::new();
    for c in base.chars() {
        if c.is_control() || "<>:\"|?*".contains(c) {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    if out.is_empty() || out == "." || out == ".." {
        out = "download".into();
    }
    // Limit length
    if out.len() > 200 {
        out.truncate(200);
    }
    out
}

/// Extract filename from URL path.
pub fn filename_from_url(url: &url::Url) -> String {
    let path = url.path();
    let name = path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("download");
    let decoded = urlencoding_decode(name);
    sanitize_filename(&decoded)
}

fn urlencoding_decode(s: &str) -> String {
    // Simple percent-decode
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push((h * 16 + l) as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Available disk space in bytes for the given path's filesystem.
pub fn available_space(path: &Path) -> Result<u64> {
    // Walk up to existing ancestor
    let mut p = path.to_path_buf();
    while !p.exists() {
        if let Some(parent) = p.parent() {
            p = parent.to_path_buf();
        } else {
            break;
        }
    }
    #[cfg(unix)]
    {
        // Prefer statvfs via libc if available; fallback to a conservative check
        match fs2::available_space(&p) {
            Ok(s) => Ok(s),
            Err(e) => {
                tracing::warn!("Could not query free space: {}", e);
                Ok(u64::MAX) // don't block if we can't check
            }
        }
    }
    #[cfg(not(unix))]
    {
        match fs2::available_space(&p) {
            Ok(s) => Ok(s),
            Err(_) => Ok(u64::MAX),
        }
    }
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn file_size(path: &Path) -> Result<u64> {
    Ok(std::fs::metadata(path)?.len())
}

/// Parse Content-Disposition filename.
pub fn filename_from_content_disposition(header: &str) -> Option<String> {
    // filename*=UTF-8''encoded or filename="..."
    if let Some(idx) = header.find("filename*=") {
        let rest = &header[idx + 10..];
        let rest = rest.trim_start_matches(|c: char| c == '"' || c.is_whitespace());
        // skip charset''
        if let Some(pos) = rest.find("''") {
            let encoded = rest[pos + 2..]
                .trim_end_matches('"')
                .trim_end_matches(';')
                .trim();
            return Some(sanitize_filename(&urlencoding_decode(encoded)));
        }
    }
    if let Some(idx) = header.to_lowercase().find("filename=") {
        let rest = &header[idx + 9..];
        let rest = rest.trim_start_matches(|c: char| c == '"' || c.is_whitespace());
        let name = rest
            .split(['"', ';', '\n'])
            .next()
            .unwrap_or("")
            .trim();
        if !name.is_empty() {
            return Some(sanitize_filename(name));
        }
    }
    None
}

pub fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

pub fn state_path_for_part(part: &Path) -> PathBuf {
    let mut s = part.as_os_str().to_owned();
    s.push(".json");
    PathBuf::from(s)
}
