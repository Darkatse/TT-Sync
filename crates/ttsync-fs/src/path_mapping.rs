//! Path mapping between wire format (SyncPath) and local file system (PathBuf).

use std::path::{Path, PathBuf};

use ttsync_contract::path::SyncPath;

/// The kind of data root the server is hosting.
#[derive(Debug, Clone, Copy)]
pub enum RootKind {
    /// The data root IS `data/` (ST-compatible). Wire paths map 1:1.
    DataRoot,
    /// The data root is a user handle directory (e.g., `data/default-user`).
    /// Wire paths starting with `default-user/` map to the root directly.
    UserRoot,
}

/// Resolve a wire-format SyncPath to an absolute local file path.
pub fn resolve_to_local(data_root: &Path, root_kind: RootKind, sync_path: &SyncPath) -> PathBuf {
    match root_kind {
        RootKind::DataRoot => join_segments(data_root, sync_path.as_str()),
        RootKind::UserRoot => {
            if let Some(rest) = sync_path.as_str().strip_prefix("default-user/") {
                join_segments(data_root, rest)
            } else {
                let base = data_root.parent().unwrap_or(data_root);
                join_segments(base, sync_path.as_str())
            }
        }
    }
}

fn join_segments(base: &Path, rel: &str) -> PathBuf {
    let mut full_path = PathBuf::from(base);
    for segment in rel.split('/') {
        full_path.push(segment);
    }
    full_path
}
