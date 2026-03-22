use serde::{Deserialize, Serialize};

use crate::path::SyncPath;

/// A single entry in a v2 manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntryV2 {
    pub path: SyncPath,
    pub size_bytes: u64,
    pub modified_ms: u64,
    /// Optional content hash (BLAKE3, base64url). Only present when verify mode is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

/// A complete file manifest for the TT-Sync v2 dataset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestV2 {
    pub entries: Vec<ManifestEntryV2>,
}
