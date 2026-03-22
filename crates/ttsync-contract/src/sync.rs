use serde::{Deserialize, Serialize};

/// Synchronization mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMode {
    /// Only transfer changed/new files. Never delete.
    Incremental,
    /// Transfer changed/new files, then delete files not present on the source side.
    Mirror,
}

impl Default for SyncMode {
    fn default() -> Self {
        Self::Incremental
    }
}

/// Scope profile identifier. Determines which paths are included in a sync operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScopeProfileId {
    /// Exact equivalent of the v1 LAN Sync whitelist.
    CompatibleMinimal,
    /// Full TauriTavern user content.
    Default,
}

impl Default for ScopeProfileId {
    fn default() -> Self {
        Self::Default
    }
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
