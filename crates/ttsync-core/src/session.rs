//! Session management: open/validate sessions with Ed25519 signatures.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};
use ttsync_contract::canonical::CanonicalRequest;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::peer::Permissions;
use ttsync_contract::session::{SessionOpenResponse, SessionToken};

use crate::error::SyncError;

#[derive(Debug, Clone)]
pub struct SessionManagerConfig {
    /// Maximum allowed absolute difference between client timestamp and server clock.
    pub time_window_ms: u64,
    /// How long a session token is valid for.
    pub session_ttl_ms: u64,
    /// How long a nonce is kept for replay prevention.
    pub nonce_ttl_ms: u64,
}

impl Default for SessionManagerConfig {
    fn default() -> Self {
        Self {
            time_window_ms: 90_000,
            session_ttl_ms: 20 * 60 * 1000,
            nonce_ttl_ms: 3 * 60 * 1000,
        }
    }
}

pub struct SessionManager {
    config: SessionManagerConfig,
    sessions: Mutex<HashMap<String, SessionRecord>>,
    nonces: Mutex<HashMap<String, u64>>,
}

struct SessionRecord {
    device_id: DeviceId,
    expires_at_ms: u64,
}

impl SessionManager {
    pub fn new(config: SessionManagerConfig) -> Self {
        Self {
            config,
            sessions: Mutex::new(HashMap::new()),
            nonces: Mutex::new(HashMap::new()),
        }
    }

    /// Validate an Ed25519-signed session open request and issue a session token.
    pub fn open_session(
        &self,
        device_id: &DeviceId,
        signature: &[u8],
        canonical_request: &[u8],
        device_public_key: &[u8],
        granted_permissions: Permissions,
    ) -> Result<SessionOpenResponse, SyncError> {
        let canonical = CanonicalRequest::parse_bytes(canonical_request)
            .map_err(|e| SyncError::InvalidData(e.to_string()))?;
        if &canonical.device_id != device_id {
            return Err(SyncError::Unauthorized("device_id mismatch".into()));
        }

        let now_ms = now_ms()?;
        if canonical.timestamp_ms.abs_diff(now_ms) > self.config.time_window_ms {
            return Err(SyncError::Unauthorized("timestamp out of window".into()));
        }

        verify_ed25519_signature(signature, canonical_request, device_public_key)?;
        self.record_nonce(device_id, &canonical.nonce, now_ms)?;

        let token = generate_session_token();
        let expires_at_ms = now_ms + self.config.session_ttl_ms;
        {
            let mut sessions = self.sessions.lock().expect("session mutex poisoned");
            sessions.retain(|_, record| record.expires_at_ms > now_ms);
            sessions.insert(
                token.clone(),
                SessionRecord {
                    device_id: device_id.clone(),
                    expires_at_ms,
                },
            );
        }

        Ok(SessionOpenResponse {
            session_token: SessionToken(token),
            expires_at_ms,
            granted_permissions,
        })
    }

    /// Validate a session token and return the associated device ID.
    pub fn validate_session(&self, token: &SessionToken) -> Result<DeviceId, SyncError> {
        let now_ms = now_ms()?;
        let mut sessions = self.sessions.lock().expect("session mutex poisoned");
        sessions.retain(|_, record| record.expires_at_ms > now_ms);

        let record = sessions
            .get(token.as_str())
            .ok_or_else(|| SyncError::Unauthorized("invalid session token".into()))?;

        Ok(record.device_id.clone())
    }

    fn record_nonce(
        &self,
        device_id: &DeviceId,
        nonce: &str,
        now_ms: u64,
    ) -> Result<(), SyncError> {
        let key = format!("{}|{}", device_id.as_str(), nonce);
        let mut nonces = self.nonces.lock().expect("nonce mutex poisoned");
        nonces.retain(|_, expires_at| *expires_at > now_ms);

        if nonces.contains_key(&key) {
            return Err(SyncError::Unauthorized("replayed nonce".into()));
        }

        nonces.insert(key, now_ms + self.config.nonce_ttl_ms);
        Ok(())
    }
}

fn verify_ed25519_signature(
    signature: &[u8],
    message: &[u8],
    device_public_key: &[u8],
) -> Result<(), SyncError> {
    let public_key: [u8; 32] = device_public_key
        .try_into()
        .map_err(|_| SyncError::InvalidData("invalid device public key".into()))?;
    let verifying_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| SyncError::InvalidData("invalid device public key".into()))?;

    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| SyncError::InvalidData("invalid signature".into()))?;
    let signature = Signature::from_bytes(&signature);

    verifying_key
        .verify_strict(message, &signature)
        .map_err(|_| SyncError::Unauthorized("invalid signature".into()))
}

fn generate_session_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

fn now_ms() -> Result<u64, SyncError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    Ok(duration.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::{SessionManager, SessionManagerConfig};
    use ttsync_contract::canonical::CanonicalRequest;
    use ttsync_contract::peer::DeviceId;

    #[test]
    fn opens_and_validates_session() {
        let manager = SessionManager::new(SessionManagerConfig {
            time_window_ms: u64::MAX,
            ..Default::default()
        });

        let device_id = DeviceId::new("550e8400-e29b-41d4-a716-446655440000".into()).unwrap();
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let verifying = signing.verifying_key();

        let canonical = CanonicalRequest::new(
            device_id.clone(),
            1,
            "nonce".into(),
            "POST".into(),
            "/v2/session/open".into(),
            "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU".into(),
        )
        .unwrap();
        let canonical_bytes = canonical.to_bytes();
        let signature = signing.sign(&canonical_bytes);
        let signature_bytes = signature.to_bytes();

        let response = manager
            .open_session(
                &device_id,
                &signature_bytes,
                &canonical_bytes,
                &verifying.to_bytes(),
                ttsync_contract::peer::Permissions::default(),
            )
            .unwrap();

        let validated = manager.validate_session(&response.session_token).unwrap();
        assert_eq!(validated, device_id);
    }
}
