//! Core download engine — streaming, checkpoints, resume, retries, multi-segment.

use crate::config::Config;
use crate::downloader::range::{parse_content_range, range_header, range_header_segment};
use crate::downloader::request::build_client;
use crate::downloader::resume::{decide_resume, remote_changed_error, ResumeDecision};
use crate::downloader::retry::Backoff;
use crate::downloader::segments::plan_segments;
use crate::errors::{DownloadError, Result};
use crate::metadata::http::{probe, validate_partial, RemoteMeta};
use crate::storage::filesystem::{
    available_space, ensure_parent_dir, file_size, filename_from_url, part_path, sanitize_filename,
};
use crate::storage::locks::DownloadLock;
use crate::storage::state::{append_history, DownloadState, DownloadStatus, SegmentState};
use crate::storage::{atomic_rename, atomic_write_string};
use crate::verification::checksum::{verify_file, HashAlgo};
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE};
use reqwest::Client;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::{watch, Mutex};
use tracing::{debug, error, info, warn};

pub struct DownloadOptions {
    pub url: String,
    pub output_dir: PathBuf,
    pub output_name: Option<String>,
    pub force_resume: bool,
    pub restart: bool,
    pub overwrite: bool,
    pub sha256: Option<String>,
    pub sha512: Option<String>,
    pub sha1: Option<String>,
    pub md5: Option<String>,
    pub headers: Vec<(String, String)>,
    pub basic_auth: Option<(String, String)>,
    pub bearer: Option<String>,
    pub yes: bool,
    pub quiet: bool,
    pub silent: bool,
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct ProgressSnapshot {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: f64,
    pub avg_speed: f64,
    pub retries: u32,
    pub status: String,
}

pub struct Engine {
    pub config: Config,
    client: Client,
}

impl Engine {
    pub fn new(config: Config) -> Result<Self> {
        let client = build_client(&config)?;
        Ok(Self { config, client })
    }

    pub async fn download(
        &self,
        opts: DownloadOptions,
        progress_tx: Option<watch::Sender<ProgressSnapshot>>,
        cancel: Arc<AtomicBool>,
    ) -> Result<DownloadState> {
        Config::ensure_dirs()?;
        std::fs::create_dir_all(&opts.output_dir)?;

        info!("Probing {}", opts.url);
        let remote = probe(&self.client, &opts.url, self.config.max_redirects).await?;
        debug!(
            "Remote: size={:?} ranges={} etag={:?}",
            remote.content_length, remote.accept_ranges, remote.etag
        );

        let filename = opts
            .output_name
            .clone()
            .or(remote.filename.clone())
            .unwrap_or_else(|| {
                filename_from_url(
                    &url::Url::parse(&remote.final_url)
                        .unwrap_or_else(|_| url::Url::parse(&opts.url).unwrap()),
                )
            });
        let filename = sanitize_filename(&filename);
        let output_path = opts.output_dir.join(&filename);
        let part = part_path(&output_path);

        if output_path.exists() && !opts.overwrite && !opts.restart {
            if !part.exists() {
                return Err(DownloadError::FileExists(output_path));
            }
        }

        if let Some(total) = remote.content_length {
            let avail = available_space(&opts.output_dir)?;
            let already = if part.exists() {
                file_size(&part).unwrap_or(0)
            } else {
                0
            };
            let needed = total.saturating_sub(already);
            if needed > avail && avail != u64::MAX {
                return Err(DownloadError::DiskFull {
                    required: needed,
                    available: avail,
                });
            }
        }

        let lock_key = output_path.to_string_lossy();
        let _lock = DownloadLock::try_acquire(&Config::locks_dir(), &lock_key)?;

        let mut state = if part.exists() && !opts.restart {
            match DownloadState::load_for_part(&part) {
                Ok(mut s) => {
                    s.sync_downloaded_from_disk()?;
                    s
                }
                Err(_) => {
                    let size = file_size(&part).unwrap_or(0);
                    let mut s =
                        DownloadState::new(&opts.url, &filename, output_path.clone(), part.clone());
                    s.downloaded = size;
                    s
                }
            }
        } else {
            if opts.restart && part.exists() {
                let _ = std::fs::remove_file(&part);
                let sp = crate::storage::filesystem::state_path_for_part(&part);
                let _ = std::fs::remove_file(&sp);
            }
            DownloadState::new(&opts.url, &filename, output_path.clone(), part.clone())
        };

        state.url = opts.url.clone();
        state.final_url = Some(remote.final_url.clone());
        state.total_size = remote.content_length.or(state.total_size);
        state.accept_ranges = remote.accept_ranges;
        if state.etag.is_none() {
            state.etag = remote.etag.clone();
        }
        if state.last_modified.is_none() {
            state.last_modified = remote.last_modified.clone();
        }
        state.sha256 = opts.sha256.clone().or(state.sha256);
        state.sha512 = opts.sha512.clone().or(state.sha512);
        state.sha1 = opts.sha1.clone().or(state.sha1);
        state.md5 = opts.md5.clone().or(state.md5);

        let decision = decide_resume(&state, &remote, opts.force_resume, opts.restart);
        match decision {
            ResumeDecision::Abort => return Err(remote_changed_error(&state, &remote)),
            ResumeDecision::Restart => {
                if part.exists() {
                    let _ = std::fs::remove_file(&part);
                }
                state.downloaded = 0;
                state.segments.clear();
                state.etag = remote.etag.clone();
                state.last_modified = remote.last_modified.clone();
                state.total_size = remote.content_length;
            }
            ResumeDecision::Resume => {
                if state.downloaded > 0 {
                    info!(
                        "Resuming from {} bytes ({:.1}%)",
                        state.downloaded,
                        state.progress_pct()
                    );
                }
            }
        }

        if state.downloaded > 0 && !opts.yes && !opts.silent && !opts.quiet {
            eprintln!(
                "Partial download found.\nDownloaded: {} Expected: {}\nProgress: {:.1}%\nResume? [Y/n]",
                human_bytes(state.downloaded),
                state
                    .total_size
                    .map(human_bytes)
                    .unwrap_or_else(|| "unknown".into()),
                state.progress_pct()
            );
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            if matches!(line.trim().to_lowercase().as_str(), "n" | "no") {
                return Err(DownloadError::Cancelled);
            }
        }

        state.status = DownloadStatus::Downloading;
        state.save()?;

        // Multi-connection only when safe: known size + Range + connections > 1
        let use_multi = self.config.connections > 1
            && remote.accept_ranges
            && state.total_size.unwrap_or(0) > 0
            && !self.config.data_saver;

        let result = if use_multi {
            info!(
                "Using {} parallel connections",
                self.config.connections
            );
            self.download_multi(&mut state, &remote, &opts, progress_tx, cancel)
                .await
        } else {
            self.download_single(&mut state, &remote, &opts, progress_tx, cancel)
                .await
        };

        match result {
            Ok(()) => {
                let actual = file_size(&state.part_path)?;
                if let Some(expected) = state.total_size {
                    if actual != expected {
                        state.status = DownloadStatus::Failed;
                        state.error = Some(format!(
                            "Incomplete: expected {}, got {}",
                            expected, actual
                        ));
                        state.save()?;
                        return Err(DownloadError::Incomplete { expected, actual });
                    }
                }

                state.status = DownloadStatus::Verifying;
                state.save()?;
                if let Some(ref h) = state.sha256 {
                    verify_file(&state.part_path, HashAlgo::Sha256, h)?;
                }
                if let Some(ref h) = state.sha512 {
                    verify_file(&state.part_path, HashAlgo::Sha512, h)?;
                }
                if let Some(ref h) = state.sha1 {
                    verify_file(&state.part_path, HashAlgo::Sha1, h)?;
                }
                if let Some(ref h) = state.md5 {
                    verify_file(&state.part_path, HashAlgo::Md5, h)?;
                }

                if state.output_path.exists() && opts.overwrite {
                    let _ = std::fs::remove_file(&state.output_path);
                }
                atomic_rename(&state.part_path, &state.output_path)?;
                let _ = std::fs::remove_file(state.state_file_path());

                state.status = DownloadStatus::Completed;
                state.downloaded = actual;
                state.save_to_state_dir()?;
                append_history(&state)?;
                info!("Download complete: {}", state.output_path.display());
                Ok(state)
            }
            Err(e) => {
                state.status = if matches!(e, DownloadError::Cancelled) {
                    DownloadStatus::Paused
                } else {
                    DownloadStatus::Failed
                };
                state.error = Some(e.to_string());
                let _ = state.save();
                Err(e)
            }
        }
    }

    fn apply_auth(
        &self,
        mut req: reqwest::RequestBuilder,
        opts: &DownloadOptions,
    ) -> reqwest::RequestBuilder {
        for (k, v) in &opts.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        if let Some((user, pass)) = &opts.basic_auth {
            req = req.basic_auth(user, Some(pass));
        }
        if let Some(token) = &opts.bearer {
            req = req.bearer_auth(token);
        }
        req
    }

    async fn download_single(
        &self,
        state: &mut DownloadState,
        remote: &RemoteMeta,
        opts: &DownloadOptions,
        progress_tx: Option<watch::Sender<ProgressSnapshot>>,
        cancel: Arc<AtomicBool>,
    ) -> Result<()> {
        let mut backoff = Backoff::new(self.config.retries.clone());
        let rate_limit = self.config.limit_rate;
        let idle_timeout = self.config.idle_timeout();
        let checkpoint_interval = Duration::from_secs(self.config.checkpoint_interval_secs);
        let checkpoint_size = self.config.checkpoint_size_bytes;

        ensure_parent_dir(&state.part_path)?;

        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(DownloadError::Cancelled);
            }

            let offset = if state.part_path.exists() {
                file_size(&state.part_path)?
            } else {
                0
            };
            state.downloaded = offset;

            let url = state.final_url.as_ref().unwrap_or(&state.url);
            let mut req = self.client.get(url);
            req = self.apply_auth(req, opts);

            if offset > 0 {
                if !remote.accept_ranges && !opts.force_resume {
                    return Err(DownloadError::ResumeUnsupported);
                }
                req = req.header("Range", range_header(offset));
                if let Some(v) = state.etag.as_deref().or(state.last_modified.as_deref()) {
                    req = req.header("If-Range", v);
                }
            }

            let resp = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    let err = DownloadError::from(e);
                    let delay = backoff.next_delay().ok_or(err)?;
                    warn!(
                        "Connection error. Progress preserved ({}). Retrying in {:?}...",
                        human_bytes(offset),
                        delay
                    );
                    state.retries = backoff.attempt();
                    tokio::time::sleep(delay).await;
                    continue;
                }
            };

            let status = resp.status().as_u16();
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok());

            if status == 429 || (500..600).contains(&status) || status == 408 || status == 425 {
                let err = DownloadError::Http {
                    status,
                    message: resp.status().canonical_reason().unwrap_or("").into(),
                };
                let delay = backoff
                    .next_delay_with_retry_after(retry_after)
                    .ok_or(err)?;
                warn!(
                    "HTTP {}. Progress preserved. Retrying in {:?}...",
                    status, delay
                );
                state.retries = backoff.attempt();
                let _ = state.save();
                tokio::time::sleep(delay).await;
                continue;
            }

            if matches!(status, 400 | 401 | 403 | 404 | 410) {
                return Err(DownloadError::Http {
                    status,
                    message: resp.status().canonical_reason().unwrap_or("").into(),
                });
            }

            if offset > 0 {
                let cr = resp
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let cl = resp
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok());
                if let Err(e) = validate_partial(status, cr.as_deref(), offset, cl) {
                    error!("Invalid range response — refusing to corrupt partial file");
                    return Err(e);
                }
                if let Some(ref crh) = cr {
                    if let Ok((_, _, total)) = parse_content_range(crh) {
                        if let Some(t) = total {
                            state.total_size = Some(t);
                        }
                    }
                }
            } else if status == 200 {
                if let Some(cl) = resp
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok())
                {
                    state.total_size = Some(cl);
                }
            }

            let mut file = if offset > 0 {
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .append(true)
                    .open(&state.part_path)
                    .await?
            } else {
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&state.part_path)
                    .await?
            };

            let mut stream = resp.bytes_stream();
            let mut last_checkpoint = Instant::now();
            let mut bytes_since_checkpoint: u64 = 0;
            let mut last_data = Instant::now();
            let start_time = Instant::now();
            let start_offset = offset;
            let downloaded_atomic = Arc::new(AtomicU64::new(offset));
            let mut window_bytes: u64 = 0;
            let mut window_start = Instant::now();

            loop {
                if cancel.load(Ordering::SeqCst) {
                    file.flush().await?;
                    state.downloaded = file_size(&state.part_path)?;
                    state.save()?;
                    return Err(DownloadError::Cancelled);
                }

                if last_data.elapsed() > idle_timeout {
                    warn!("Idle timeout — no data for {:?}", idle_timeout);
                    file.flush().await?;
                    state.downloaded = file_size(&state.part_path)?;
                    state.save()?;
                    let delay = backoff
                        .next_delay()
                        .ok_or(DownloadError::Timeout(idle_timeout))?;
                    warn!("Retrying in {:?}...", delay);
                    state.retries = backoff.attempt();
                    tokio::time::sleep(delay).await;
                    break;
                }

                let next = tokio::time::timeout(Duration::from_secs(5), stream.next()).await;
                match next {
                    Ok(Some(Ok(chunk))) => {
                        last_data = Instant::now();
                        let chunk: Bytes = chunk;
                        if chunk.is_empty() {
                            continue;
                        }

                        if let Some(limit) = rate_limit {
                            let elapsed = window_start.elapsed().as_secs_f64().max(0.001);
                            let rate = (window_bytes as f64) / elapsed;
                            if rate > limit as f64 {
                                let excess = window_bytes.saturating_sub(limit);
                                let sleep_secs = excess as f64 / limit as f64;
                                tokio::time::sleep(Duration::from_secs_f64(sleep_secs.min(1.0)))
                                    .await;
                                window_bytes = 0;
                                window_start = Instant::now();
                            }
                        }

                        file.write_all(&chunk).await?;
                        let n = chunk.len() as u64;
                        window_bytes += n;
                        bytes_since_checkpoint += n;
                        let new_dl = downloaded_atomic.fetch_add(n, Ordering::SeqCst) + n;
                        state.downloaded = new_dl;

                        if let Some(ref tx) = progress_tx {
                            let elapsed = start_time.elapsed().as_secs_f64().max(0.001);
                            let avg = (new_dl - start_offset) as f64 / elapsed;
                            let inst_elapsed = window_start.elapsed().as_secs_f64().max(0.001);
                            let inst = window_bytes as f64 / inst_elapsed;
                            let _ = tx.send(ProgressSnapshot {
                                downloaded: new_dl,
                                total: state.total_size,
                                speed: inst,
                                avg_speed: avg,
                                retries: state.retries,
                                status: "downloading".into(),
                            });
                        }

                        if last_checkpoint.elapsed() >= checkpoint_interval
                            || bytes_since_checkpoint >= checkpoint_size
                        {
                            file.flush().await?;
                            state.downloaded = file_size(&state.part_path)?;
                            state.save()?;
                            last_checkpoint = Instant::now();
                            bytes_since_checkpoint = 0;
                            debug!("Checkpoint at {}", state.downloaded);
                        }
                    }
                    Ok(Some(Err(e))) => {
                        warn!("Stream error: {}", e);
                        file.flush().await?;
                        state.downloaded = file_size(&state.part_path)?;
                        state.save()?;
                        let delay = backoff
                            .next_delay()
                            .ok_or(DownloadError::Network(e.to_string()))?;
                        warn!(
                            "Progress preserved ({}). Retrying in {:?}...",
                            human_bytes(state.downloaded),
                            delay
                        );
                        state.retries = backoff.attempt();
                        tokio::time::sleep(delay).await;
                        break;
                    }
                    Ok(None) => {
                        file.flush().await?;
                        state.downloaded = file_size(&state.part_path)?;
                        state.save()?;
                        if let Some(total) = state.total_size {
                            if state.downloaded >= total {
                                return Ok(());
                            }
                            warn!(
                                "Connection closed early ({}/{}). Retrying...",
                                state.downloaded, total
                            );
                            let delay = backoff.next_delay().unwrap_or(Duration::from_secs(5));
                            state.retries = backoff.attempt();
                            tokio::time::sleep(delay).await;
                            break;
                        }
                        return Ok(());
                    }
                    Err(_) => continue,
                }
            }
        }
    }

    /// Parallel segmented download. Only incomplete segments are resumed.
    async fn download_multi(
        &self,
        state: &mut DownloadState,
        remote: &RemoteMeta,
        opts: &DownloadOptions,
        progress_tx: Option<watch::Sender<ProgressSnapshot>>,
        cancel: Arc<AtomicBool>,
    ) -> Result<()> {
        let total = state
            .total_size
            .or(remote.content_length)
            .ok_or_else(|| DownloadError::Other("Size unknown for multi-connection".into()))?;

        if state.segments.is_empty() {
            state.segments = plan_segments(total, self.config.connections);
        }

        // Never use preallocation as a progress signal. A preallocated/sparse
        // file can have the final length while many ranges are still missing.
        ensure_parent_dir(&state.part_path)?;
        {
            let _f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .open(&state.part_path)?;
        }

        // Segment metadata is the source of truth for multi-connection progress.
        state.downloaded = state.segments.iter().map(|s| s.downloaded).sum();
        let file = Arc::new(Mutex::new(
            tokio::fs::OpenOptions::new()
                .write(true)
                .read(true)
                .open(&state.part_path)
                .await?,
        ));

        let global_downloaded = Arc::new(AtomicU64::new(
            state
                .segments
                .iter()
                .map(|s| s.downloaded)
                .sum::<u64>(),
        ));
        // Do not derive progress from file_size(): ranged writes may create holes.

        let start_time = Instant::now();
        let client = self.client.clone();
        let url = state.final_url.clone().unwrap_or_else(|| state.url.clone());
        let headers = opts.headers.clone();
        let basic = opts.basic_auth.clone();
        let bearer = opts.bearer.clone();
        let idle_timeout = self.config.idle_timeout();
        let retry_cfg = self.config.retries.clone();
        let rate_limit = self.config.limit_rate;
        let validator = state.etag.clone().or_else(|| state.last_modified.clone());

        let mut handles = Vec::new();
        let segs: Vec<SegmentState> = state.segments.clone();

        for seg in segs {
            if seg.completed {
                continue;
            }
            let file = file.clone();
            let client = client.clone();
            let url = url.clone();
            let headers = headers.clone();
            let basic = basic.clone();
            let bearer = bearer.clone();
            let cancel = cancel.clone();
            let global_downloaded = global_downloaded.clone();
            let retry_cfg = retry_cfg.clone();
            let validator = validator.clone();
            let mut seg = seg;
                        handles.push(tokio::spawn(async move {
                let mut backoff = Backoff::new(retry_cfg);
                loop {
                    if cancel.load(Ordering::SeqCst) {
                        return Err(DownloadError::Cancelled);
                    }
                    let cur = seg.start + seg.downloaded;
                    if cur > seg.end {
                        seg.completed = true;
                        return Ok(seg);
                    }

                    let mut req = client.get(&url);
                    for (k, v) in &headers {
                        req = req.header(k.as_str(), v.as_str());
                    }
                    if let Some((u, p)) = &basic {
                        req = req.basic_auth(u, Some(p));
                    }
                    if let Some(t) = &bearer {
                        req = req.bearer_auth(t);
                    }
                    req = req.header("Range", range_header_segment(cur, seg.end));
                    if let Some(v) = validator.as_deref() {
                        req = req.header("If-Range", v);
                    }

                    let resp = match req.send().await {
                        Ok(r) => r,
                        Err(e) => {
                            let delay = backoff
                                .next_delay()
                                .ok_or(DownloadError::Network(e.to_string()))?;
                            tokio::time::sleep(delay).await;
                            continue;
                        }
                    };
                    let status = resp.status().as_u16();
                    if status == 429 || (500..600).contains(&status) {
                        let ra = resp
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse().ok());
                        let delay = backoff
                            .next_delay_with_retry_after(ra)
                            .unwrap_or(Duration::from_secs(5));
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    if status != 206 {
                        return Err(DownloadError::Http {
                            status,
                            message: format!("segment {} requires 206 Partial Content", seg.index),
                        });
                    }

                    let cr = resp
                        .headers()
                        .get(CONTENT_RANGE)
                        .and_then(|v| v.to_str().ok());
                    let cl = resp
                        .headers()
                        .get(CONTENT_LENGTH)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok());
                    validate_partial(status, cr, cur, cl)?;
                    let (range_start, range_end, range_total) =
                        parse_content_range(cr.ok_or(DownloadError::InvalidRange)?)?;
                    if range_start != cur || range_end > seg.end {
                        return Err(DownloadError::InvalidRange);
                    }
                    if let Some(t) = range_total {
                        if t != total {
                            return Err(DownloadError::InvalidRange);
                        }
                    }

                    let mut stream = resp.bytes_stream();
                    let mut last_data = Instant::now();
                    let mut window_bytes: u64 = 0;
                    let mut window_start = Instant::now();

                    loop {
                        if cancel.load(Ordering::SeqCst) {
                            return Err(DownloadError::Cancelled);
                        }
                        if last_data.elapsed() > idle_timeout {
                            let delay = backoff.next_delay().unwrap_or(Duration::from_secs(3));
                            tokio::time::sleep(delay).await;
                            break;
                        }
                        match tokio::time::timeout(Duration::from_secs(5), stream.next()).await {
                            Ok(Some(Ok(chunk))) => {
                                last_data = Instant::now();
                                if chunk.is_empty() {
                                    continue;
                                }
                                if let Some(limit) = rate_limit {
                                    let elapsed = window_start.elapsed().as_secs_f64().max(0.001);
                                    if (window_bytes as f64) / elapsed > limit as f64 {
                                        tokio::time::sleep(Duration::from_millis(50)).await;
                                        window_bytes = 0;
                                        window_start = Instant::now();
                                    }
                                }
                                let n = chunk.len() as u64;
                                let write_pos = seg.start + seg.downloaded;
                                {
                                    let mut f = file.lock().await;
                                    f.seek(SeekFrom::Start(write_pos)).await?;
                                    f.write_all(&chunk).await?;
                                }
                                seg.downloaded += n;
                                window_bytes += n;
                                let _g = global_downloaded.fetch_add(n, Ordering::SeqCst) + n;
                                if seg.start + seg.downloaded > seg.end {
                                    seg.downloaded = seg.end - seg.start + 1;
                                    seg.completed = true;
                                    return Ok(seg);
                                }
                            }
                            Ok(Some(Err(_))) | Ok(None) => break,
                            Err(_) => continue,
                        }
                    }
                }
            }));
        }

        let mut updated = state.segments.clone();
        for h in handles {
            match h.await {
                Ok(Ok(seg)) => {
                    if let Some(s) = updated.iter_mut().find(|s| s.index == seg.index) {
                        *s = seg;
                    }
                    // Persist each completed segment before waiting on the next
                    // worker so a later failure never discards earlier progress.
                    state.segments = updated.clone();
                    state.downloaded = state.segments.iter().map(|s| s.downloaded).sum();
                    {
                        let mut f = file.lock().await;
                        f.sync_data().await?;
                    }
                    state.save()?;
                }
                Ok(Err(e)) => {
                    state.segments = updated;
                    state.downloaded = global_downloaded.load(Ordering::SeqCst);
                    state.save()?;
                    return Err(e);
                }
                Err(e) => {
                    return Err(DownloadError::Other(format!("segment task: {}", e)));
                }
            }
        }

        state.segments = updated;
        state.downloaded = global_downloaded.load(Ordering::SeqCst);
        // Truncate to total if needed
        {
            let mut f = file.lock().await;
            f.flush().await?;
        }
        let meta_len = file_size(&state.part_path)?;
        if meta_len > total {
            let f = std::fs::OpenOptions::new().write(true).open(&state.part_path)?;
            f.set_len(total)?;
        }
        state.downloaded = total.min(file_size(&state.part_path)?);
        state.save()?;

        if state.segments.iter().all(|s| s.completed) {
            Ok(())
        } else {
            // Retry incomplete via single-connection path from current size
            // Recompute downloaded as contiguous prefix for single resume fallback
            warn!("Some segments incomplete; falling back to single-connection resume");
            // Only the prefix covered by fully completed segments plus the
            // contiguous bytes of the first incomplete segment is safe to keep.
            let mut sorted = state.segments.clone();
            sorted.sort_by_key(|s| s.start);
            let mut contiguous = 0u64;
            for seg in sorted {
                if seg.start != contiguous {
                    break;
                }
                if seg.completed {
                    contiguous = seg.end.saturating_add(1);
                } else {
                    contiguous = seg.start.saturating_add(seg.downloaded);
                    break;
                }
            }
            let f = std::fs::OpenOptions::new().write(true).open(&state.part_path)?;
            f.set_len(contiguous)?;
            state.downloaded = contiguous;
            state.segments.clear();
            state.save()?;
            self.download_single(state, remote, opts, progress_tx, cancel)
                .await
        }
    }
}

impl DownloadState {
    fn save_to_state_dir(&self) -> Result<()> {
        let path = Config::state_dir().join(format!("{}.json", self.id));
        let json = serde_json::to_string_pretty(self)?;
        atomic_write_string(&path, &json)
    }
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", n, UNITS[0])
    } else {
        format!("{:.2} {}", v, UNITS[i])
    }
}
