//! Multi-connection segment planning and recovery.

use crate::storage::state::SegmentState;

/// Plan segments for a file of known size with N connections.
pub fn plan_segments(total: u64, connections: u32) -> Vec<SegmentState> {
    let n = connections.max(1) as u64;
    if total == 0 {
        return vec![SegmentState {
            index: 0,
            start: 0,
            end: 0,
            downloaded: 0,
            completed: false,
        }];
    }
    let chunk = total / n;
    let mut segs = Vec::new();
    for i in 0..n {
        let start = i * chunk;
        let end = if i == n - 1 {
            total - 1
        } else {
            (i + 1) * chunk - 1
        };
        segs.push(SegmentState {
            index: i as u32,
            start,
            end,
            downloaded: 0,
            completed: false,
        });
    }
    segs
}

/// Only incomplete segments need resume.
pub fn incomplete_segments(segs: &[SegmentState]) -> Vec<&SegmentState> {
    segs.iter().filter(|s| !s.completed).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_four() {
        let segs = plan_segments(1000, 4);
        assert_eq!(segs.len(), 4);
        assert_eq!(segs[0].start, 0);
        assert_eq!(segs[3].end, 999);
    }
}
