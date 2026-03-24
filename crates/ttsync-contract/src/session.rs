use serde::{Deserialize, Serialize};
use crate::peer::Permissions;

pub const HEADER_DEVICE_ID: &str = "TT-Device-Id";
pub const HEADER_TIMESTAMP_MS: &str = "TT-Timestamp-Ms";
pub const HEADER_NONCE: &str = "TT-Nonce";
pub const HEADER_SIGNATURE: &str = "TT-Signature";

/// Short-lived bearer token for authenticated requests after session open.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionToken(pub String);

impl SessionToken {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Request to open a new session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOpenRequest {
    pub device_id: crate::peer::DeviceId,
}

/// Response to a successful session open.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionOpenResponse {
    pub session_token: SessionToken,
    pub expires_at_ms: u64,
    pub granted_permissions: Permissions,
}
