//! Resume logic: metadata comparison, force options.

use crate::errors::DownloadError;
use crate::metadata::RemoteMeta;
use crate::storage::state::DownloadState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeDecision {
    Resume,
    Restart,
    Abort,
}

pub fn decide_resume(
    state: &DownloadState,
    remote: &RemoteMeta,
    force_resume: bool,
    restart: bool,
) -> ResumeDecision {
    if restart {
        return ResumeDecision::Restart;
    }

    // No local progress
    if state.downloaded == 0 || !state.part_path.exists() {
        return ResumeDecision::Resume; // start fresh effectively
    }

    // Server doesn't support range
    if !remote.accept_ranges && state.downloaded > 0 {
        if force_resume {
            return ResumeDecision::Resume; // will likely fail validation
        }
        return ResumeDecision::Abort;
    }

    // ETag / Last-Modified changed
    let etag_changed = matches!((&state.etag, &remote.etag), (Some(a), Some(b)) if a != b);
    let lm_changed = matches!((&state.last_modified, &remote.last_modified), (Some(a), Some(b)) if a != b);
    let size_changed = matches!((state.total_size, remote.content_length), (Some(a), Some(b)) if a != b);

    if etag_changed || lm_changed || size_changed {
        if force_resume {
            return ResumeDecision::Resume;
        }
        return ResumeDecision::Abort;
    }

    ResumeDecision::Resume
}

pub fn remote_changed_error(state: &DownloadState, remote: &RemoteMeta) -> DownloadError {
    DownloadError::RemoteChanged {
        old_etag: state.etag.clone(),
        new_etag: remote.etag.clone(),
    }
}
