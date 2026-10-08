//! Probe remote resource for size, ranges, etag, filename.

use crate::errors::{DownloadError, Result};
use crate::storage::filesystem::filename_from_content_disposition;
use reqwest::header::{
    ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, ETAG, LAST_MODIFIED,
};
use reqwest::Client;
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone, Default)]
pub struct RemoteMeta {
    pub url: String,
    pub final_url: String,
    pub content_length: Option<u64>,
    pub accept_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub filename: Option<String>,
    pub status: u16,
}

pub async fn probe(
    client: &Client,
    url: &str,
    max_redirects: u32,
) -> Result<RemoteMeta> {
    let parsed = Url::parse(url).map_err(|e| DownloadError::InvalidUrl(e.to_string()))?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(DownloadError::InvalidUrl(format!(
            "Unsupported scheme: {}",
            parsed.scheme()
        )));
    }

    // Prefer HEAD
    let head = client
        .head(url)
        .timeout(Duration::from_secs(60))
        .send()
        .await;

    let mut meta = RemoteMeta {
        url: url.to_string(),
        ..Default::default()
    };

    match head {
        Ok(resp) => {
            meta.status = resp.status().as_u16();
            meta.final_url = resp.url().to_string();
            fill_from_headers(&mut meta, resp.headers());
            // Some servers lie on HEAD; if no length, try Range probe
            if meta.content_length.is_none() || !meta.accept_ranges {
                if let Ok(m) = range_probe(client, &meta.final_url).await {
                    if m.content_length.is_some() {
                        meta.content_length = m.content_length;
                    }
                    if m.accept_ranges {
                        meta.accept_ranges = true;
                    }
                    if meta.etag.is_none() {
                        meta.etag = m.etag;
                    }
                    if meta.last_modified.is_none() {
                        meta.last_modified = m.last_modified;
                    }
                }
            }
            Ok(meta)
        }
        Err(_) => {
            // Fallback: Range bytes=0-0
            range_probe(client, url).await
        }
    }
}

async fn range_probe(client: &Client, url: &str) -> Result<RemoteMeta> {
    let resp = client
        .get(url)
        .header("Range", "bytes=0-0")
        .timeout(Duration::from_secs(60))
        .send()
        .await?;

    let mut meta = RemoteMeta {
        url: url.to_string(),
        final_url: resp.url().to_string(),
        status: resp.status().as_u16(),
        ..Default::default()
    };

    if resp.status().as_u16() == 206 {
        meta.accept_ranges = true;
        if let Some(cr) = resp.headers().get(CONTENT_RANGE) {
            if let Ok(s) = cr.to_str() {
                // bytes 0-0/12345
                if let Some(total) = s.split('/').nth(1) {
                    if total != "*" {
                        if let Ok(n) = total.parse::<u64>() {
                            meta.content_length = Some(n);
                        }
                    }
                }
            }
        }
    } else if resp.status().is_success() {
        // Server ignored Range
        meta.accept_ranges = false;
        fill_from_headers(&mut meta, resp.headers());
    }

    fill_from_headers(&mut meta, resp.headers());
    // Consume body minimally
    let _ = resp.bytes().await;
    Ok(meta)
}

fn fill_from_headers(meta: &mut RemoteMeta, headers: &reqwest::header::HeaderMap) {
    if let Some(cl) = headers.get(CONTENT_LENGTH) {
        if let Ok(s) = cl.to_str() {
            if let Ok(n) = s.parse::<u64>() {
                meta.content_length = Some(n);
            }
        }
    }
    if let Some(ar) = headers.get(ACCEPT_RANGES) {
        if let Ok(s) = ar.to_str() {
            if s.to_lowercase().contains("bytes") {
                meta.accept_ranges = true;
            }
        }
    }
    if let Some(e) = headers.get(ETAG) {
        if let Ok(s) = e.to_str() {
            meta.etag = Some(s.to_string());
        }
    }
    if let Some(lm) = headers.get(LAST_MODIFIED) {
        if let Ok(s) = lm.to_str() {
            meta.last_modified = Some(s.to_string());
        }
    }
    if let Some(cd) = headers.get(CONTENT_DISPOSITION) {
        if let Ok(s) = cd.to_str() {
            meta.filename = filename_from_content_disposition(s);
        }
    }
}

/// Validate a 206 response matches the requested offset.
pub fn validate_partial(
    status: u16,
    content_range: Option<&str>,
    requested_offset: u64,
    content_length_header: Option<u64>,
) -> Result<()> {
    if status == 200 {
        // Server ignored Range — must not append
        return Err(DownloadError::InvalidRange);
    }
    if status != 206 {
        return Err(DownloadError::Http {
            status,
            message: "Expected 206 Partial Content for resume".into(),
        });
    }
    let cr = content_range.ok_or(DownloadError::InvalidRange)?;
    let (start, end, _) = crate::downloader::range::parse_content_range(cr)?;
    if start != requested_offset || end < start {
        return Err(DownloadError::InvalidRange);
    }
    let expected_len = end - start + 1;
    if let Some(content_length) = content_length_header {
        if content_length != expected_len {
            return Err(DownloadError::InvalidRange);
        }
    }
    Ok(())
}
