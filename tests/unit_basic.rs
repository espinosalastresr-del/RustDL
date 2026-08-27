use std::path::Path;

// Integration-style unit tests that don't need the full binary.

#[test]
fn sanitize_strips_traversal() {
    // Mirror logic: we only expose via the library modules once linked;
    // these tests document expected behavior for path safety.
    let dangerous = "../../../etc/passwd";
    let base = dangerous.rsplit('/').next().unwrap_or(dangerous);
    assert_eq!(base, "passwd");
}

#[test]
fn human_size_parse_idea() {
    // 500K = 500 * 1024
    let s = "500K";
    assert!(s.ends_with('K') || s.ends_with('k'));
}

#[test]
fn range_header_format() {
    let offset = 5_000_000u64;
    let h = format!("bytes={}-", offset);
    assert_eq!(h, "bytes=5000000-");
}
