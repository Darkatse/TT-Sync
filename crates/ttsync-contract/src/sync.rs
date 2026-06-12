use serde::{Deserialize, Serialize};

/// Synchronization mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMode {
    /// Only transfer changed/new files. Never delete.
    #[default]
    Incremental,
    /// Transfer changed/new files, then delete files not present on the source side.
    Mirror,
}

/// Current phase of a sync operation, used for progress reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncPhase {
    Scanning,
    Diffing,
    Downloading,
    Uploading,
    Deleting,
}
