use serde::{Deserialize, Serialize};

use crate::peer::DeviceId;

/// Response body for `GET /v2/status`.
///
/// The core v2 fields are shared by remote TT-Sync hubs and peer-to-peer
/// transports. Topology-specific identity fields are optional so LAN peers can
/// advertise their local device identity without changing the base protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub ok: bool,
    pub protocol: String,
    pub server: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub dataset_policy_version: Option<u32>,
    #[serde(default)]
    pub supported_dataset_ids: Vec<String>,
    #[serde(default)]
    pub supported_profile_ids: Vec<String>,
    #[serde(default)]
    pub default_dataset_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<DeviceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spki_sha256: Option<String>,
}
