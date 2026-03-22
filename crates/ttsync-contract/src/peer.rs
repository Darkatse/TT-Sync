use serde::{Deserialize, Serialize};

/// Unique identifier for a device.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DeviceId(String);

impl DeviceId {
    pub fn new(raw: String) -> Result<Self, DeviceIdError> {
        validate_uuid(&raw)?;
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for DeviceId {
    type Error = DeviceIdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<DeviceId> for String {
    fn from(value: DeviceId) -> Self {
        value.0
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceIdError {
    #[error("device id must be a UUID")]
    NotUuid,
}

fn validate_uuid(value: &str) -> Result<(), DeviceIdError> {
    // 8-4-4-4-12 hex with hyphens.
    if value.len() != 36 {
        return Err(DeviceIdError::NotUuid);
    }

    for (idx, ch) in value.chars().enumerate() {
        match idx {
            8 | 13 | 18 | 23 => {
                if ch != '-' {
                    return Err(DeviceIdError::NotUuid);
                }
            }
            _ => {
                if !ch.is_ascii_hexdigit() {
                    return Err(DeviceIdError::NotUuid);
                }
            }
        }
    }
    Ok(())
}

/// Permissions granted to a paired peer.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub mirror_delete: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            read: true,
            write: false,
            mirror_delete: false,
        }
    }
}

/// A registered peer device with its granted capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerGrant {
    pub device_id: DeviceId,
    pub device_name: String,
    /// Ed25519 public key bytes.
    pub public_key: Vec<u8>,
    pub permissions: Permissions,
    pub paired_at_ms: u64,
    pub last_sync_ms: Option<u64>,
}
