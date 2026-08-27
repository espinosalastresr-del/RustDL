//! Integration / resilience tests with a local HTTP mock server.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Minimal HTTP server supporting GET/HEAD, Range, and fault injection.
struct MockServer {
    addr: String,
    hits: Arc<AtomicU32>,
}

impl MockServer {
    fn spawn(body: &'static [u8], fail_first: u32) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let hits = Arc::new(AtomicU32::new(0));
        let hits_c = hits.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let n = hits_c.fetch_add(1, Ordering::SeqCst);
                let _ = handle(stream, body, n < fail_first);
            }
        });
        // Give server a moment
        thread::sleep(Duration::from_millis(50));
        Self { addr, hits }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}/{}", self.addr, path.trim_start_matches('/'))
    }
}

fn handle(mut stream: TcpStream, body: &[u8], fail: bool) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let is_head = req.starts_with("HEAD ");
    let range = req.lines().find(|l| l.to_lowercase().starts_with("range:"));

    if fail {
        write!(stream, "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n")?;
        return Ok(());
    }

    if let Some(rline) = range {
        // bytes=START-END or bytes=START-
        let spec = rline.split(':').nth(1).unwrap_or("").trim();
        let spec = spec.trim_start_matches("bytes=");
        let mut parts = spec.split('-');
        let start: usize = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let end: usize = parts
            .next()
            .filter(|s| !s.is_empty())
            .and_then(|s| s.parse().ok())
            .unwrap_or(body.len() - 1);
        let end = end.min(body.len() - 1);
        let slice = &body[start..=end];
        write!(
            stream,
            "HTTP/1.1 206 Partial Content\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {}-{}/{}\r\nContent-Length: {}\r\n\r\n",
            start,
            end,
            body.len(),
            slice.len()
        )?;
        if !is_head {
            stream.write_all(slice)?;
        }
    } else {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )?;
        if !is_head {
            stream.write_all(body)?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn download_full_file() {
    let body: &'static [u8] = b"Hello, resilience test payload!!!";
    let server = MockServer::spawn(body, 0);
    let dir = {
        let d = std::env::temp_dir().join(format!("rustdl-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    };

    let mut cfg = rustdl::Config::default();
    cfg.download_dir = dir.as_path().to_path_buf();
    cfg.retries.max_retries = Some(3);
    cfg.timeouts.idle_secs = 10;
    cfg.connections = 1;

    let engine = rustdl::Engine::new(cfg).unwrap();
    let opts = rustdl::DownloadOptions {
        url: server.url("file.bin"),
        output_dir: dir.as_path().to_path_buf(),
        output_name: Some("file.bin".into()),
        force_resume: false,
        restart: false,
        overwrite: true,
        sha256: None,
        sha512: None,
        sha1: None,
        md5: None,
        headers: vec![],
        basic_auth: None,
        bearer: None,
        yes: true,
        quiet: true,
        silent: true,
        json: false,
    };
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let state = engine.download(opts, None, cancel).await.expect("download");
    assert_eq!(state.status, rustdl::DownloadStatus::Completed);
    let data = std::fs::read(dir.as_path().join("file.bin")).unwrap();
    assert_eq!(data, body);
}

#[tokio::test]
async fn retries_on_503_then_succeeds() {
    let body: &'static [u8] = b"after-failures";
    let server = MockServer::spawn(body, 2); // first 2 requests fail
    let dir = {
        let d = std::env::temp_dir().join(format!("rustdl-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    };

    let mut cfg = rustdl::Config::default();
    cfg.download_dir = dir.as_path().to_path_buf();
    cfg.retries.max_retries = Some(10);
    cfg.retries.initial_delay_ms = 50;
    cfg.retries.max_delay_secs = 1;
    cfg.timeouts.idle_secs = 10;

    let engine = rustdl::Engine::new(cfg).unwrap();
    let opts = rustdl::DownloadOptions {
        url: server.url("retry.bin"),
        output_dir: dir.as_path().to_path_buf(),
        output_name: Some("retry.bin".into()),
        force_resume: false,
        restart: false,
        overwrite: true,
        sha256: None,
        sha512: None,
        sha1: None,
        md5: None,
        headers: vec![],
        basic_auth: None,
        bearer: None,
        yes: true,
        quiet: true,
        silent: true,
        json: false,
    };
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let state = engine.download(opts, None, cancel).await.expect("retry download");
    assert_eq!(state.status, rustdl::DownloadStatus::Completed);
    assert!(server.hits.load(Ordering::SeqCst) >= 3);
    assert_eq!(std::fs::read(dir.as_path().join("retry.bin")).unwrap(), body);
}

#[tokio::test]
async fn resume_after_partial() {
    let body: &'static [u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let server = MockServer::spawn(body, 0);
    let dir = {
        let d = std::env::temp_dir().join(format!("rustdl-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    };
    let part = dir.as_path().join("big.bin.part");
    // Pre-write first 10 bytes as partial
    std::fs::write(&part, &body[..10]).unwrap();

    let mut cfg = rustdl::Config::default();
    cfg.download_dir = dir.as_path().to_path_buf();
    cfg.retries.max_retries = Some(5);
    cfg.timeouts.idle_secs = 15;

    let engine = rustdl::Engine::new(cfg).unwrap();
    let opts = rustdl::DownloadOptions {
        url: server.url("big.bin"),
        output_dir: dir.as_path().to_path_buf(),
        output_name: Some("big.bin".into()),
        force_resume: false,
        restart: false,
        overwrite: true,
        sha256: None,
        sha512: None,
        sha1: None,
        md5: None,
        headers: vec![],
        basic_auth: None,
        bearer: None,
        yes: true,
        quiet: true,
        silent: true,
        json: false,
    };
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let state = engine.download(opts, None, cancel).await.expect("resume");
    assert_eq!(state.status, rustdl::DownloadStatus::Completed);
    assert_eq!(std::fs::read(dir.as_path().join("big.bin")).unwrap(), body);
}

#[test]
fn backoff_and_segments_unit() {
    use rustdl::downloader::retry::Backoff;
    use rustdl::config::RetryConfig;
    use rustdl::downloader::segments::plan_segments;

    let cfg = RetryConfig {
        max_retries: Some(3),
        initial_delay_ms: 10,
        max_delay_secs: 1,
        jitter: false,
    };
    let mut b = Backoff::new(cfg);
    assert!(b.next_delay().is_some());
    assert!(b.next_delay().is_some());
    assert!(b.next_delay().is_some());
    assert!(b.next_delay().is_none());

    let segs = plan_segments(1000, 4);
    assert_eq!(segs.len(), 4);
    assert_eq!(segs[0].start, 0);
    assert_eq!(segs[3].end, 999);
}

#[test]
fn checksum_roundtrip() {
    use rustdl::verification::checksum::{hash_file, verify_file, HashAlgo};
    let dir = {
        let d = std::env::temp_dir().join(format!("rustdl-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    };
    let f = dir.as_path().join("x.bin");
    std::fs::write(&f, b"abc").unwrap();
    let h = hash_file(&f, HashAlgo::Sha256).unwrap();
    verify_file(&f, HashAlgo::Sha256, &h).unwrap();
    assert!(verify_file(&f, HashAlgo::Sha256, "deadbeef").is_err());
}

#[test]
fn sanitize_and_parse_size() {
    use rustdl::config::parse_size;
    use rustdl::storage::filesystem::sanitize_filename;
    assert_eq!(sanitize_filename("../etc/passwd"), "passwd");
    assert_eq!(parse_size("500K").unwrap(), 500 * 1024);
    assert_eq!(parse_size("2M").unwrap(), 2 * 1024 * 1024);
}
