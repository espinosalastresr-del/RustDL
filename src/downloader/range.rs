//! HTTP Range helpers.

use crate::errors::{DownloadError, Result};

pub fn range_header(offset: u64) -> String {
    format!("bytes={}-", offset)
}

pub fn range_header_segment(start: u64, end: u64) -> String {
    format!("bytes={}-{}", start, end)
}

pub fn parse_content_range(header: &str) -> Result<(u64, u64, Option<u64>)> {
    // bytes START-END/TOTAL or bytes START-END/*
    let s = header.trim();
    let s = s
        .strip_prefix("bytes ")
        .or_else(|| s.strip_prefix("bytes="))
        .unwrap_or(s);
    let mut parts = s.split('/');
    let range = parts.next().ok_or(DownloadError::InvalidRange)?;
    let total = parts.next();
    let mut se = range.split('-');
    let start: u64 = se
        .next()
        .ok_or(DownloadError::InvalidRange)?
        .parse()
        .map_err(|_| DownloadError::InvalidRange)?;
    let end: u64 = se
        .next()
        .ok_or(DownloadError::InvalidRange)?
        .parse()
        .map_err(|_| DownloadError::InvalidRange)?;
    let total = match total {
        Some("*") | None => None,
        Some(t) => Some(t.parse().map_err(|_| DownloadError::InvalidRange)?),
    };
    Ok((start, end, total))
}
