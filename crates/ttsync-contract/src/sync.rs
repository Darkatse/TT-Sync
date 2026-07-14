use serde::{Deserialize, Serialize};

pub const OVERWRITE_POLICY_FEATURE_V1: &str = "overwrite_policy_v1";

/// Synchronization mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMode {
    /// Only transfer changed/new files. Never delete.
    #[default]
    Incremental,
    /// Transfer changed/new files, then delete files not present on the source side.
    Mirror,
}

/// Policy deciding whether a plan may replicate over a target copy whose
/// `modified_ms` is strictly newer than the source's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverwritePolicy {
    /// Transfer every `(size_bytes, modified_ms)` mismatch. The source is
    /// authoritative and the target becomes an exact copy of it.
    #[default]
    Exact,
    /// Preserve target copies whose `modified_ms` is strictly newer than the
    /// source's, so a stale source cannot revert the target's latest write.
    /// Relies on reasonably synchronized device clocks.
    PreferNewer,
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
