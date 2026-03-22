//! Pairing use-case: generate tokens, validate pair requests, register peers.
//!
//! Actual implementation will be filled in during feature development.
//! This module defines the public interface and placeholder functions.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ttsync_contract::pair::{PairCompleteRequest, PairCompleteResponse, PairUri};
use ttsync_contract::peer::{DeviceId, PeerGrant, Permissions};
use ttsync_contract::sync::ScopeProfileId;

use crate::error::SyncError;

/// Configuration for a pairing session.
pub struct PairingConfig {
    pub profile: ScopeProfileId,
    pub permissions: Permissions,
    pub expires_in_secs: u64,
}

/// An active pairing session on the server side.
pub struct PairingSession {
    pub token: String,
    pub config: PairingConfig,
    pub expires_at_ms: u64,
}

/// Generate a new pairing session with a one-time token.
pub fn create_pairing_session(
    public_base_url: &str,
    spki_sha256: &str,
    config: PairingConfig,
) -> Result<(PairingSession, PairUri), SyncError> {
    let token = generate_one_time_token();
    let now_ms = now_ms()?;
    let expires_at_ms = now_ms + (config.expires_in_secs * 1000);

    let session = PairingSession {
        token: token.clone(),
        config,
        expires_at_ms,
    };

    let uri = PairUri {
        url: public_base_url.to_owned(),
        token,
        expires_at_ms,
        spki_sha256: spki_sha256.to_owned(),
    };

    Ok((session, uri))
}

/// Validate an incoming pair-complete request and register the peer.
pub fn complete_pairing(
    session: &PairingSession,
    request: &PairCompleteRequest,
    server_device_id: &DeviceId,
    server_device_name: &str,
) -> Result<(PeerGrant, PairCompleteResponse), SyncError> {
    let now_ms = now_ms()?;
    if now_ms > session.expires_at_ms {
        return Err(SyncError::Unauthorized("pair token expired".into()));
    }

    let public_key = URL_SAFE_NO_PAD
        .decode(&request.device_pubkey)
        .map_err(|_| SyncError::InvalidData("invalid device_pubkey".into()))?;
    if public_key.len() != 32 {
        return Err(SyncError::InvalidData("invalid device_pubkey length".into()));
    }

    let grant = PeerGrant {
        device_id: request.device_id.clone(),
        device_name: request.device_name.clone(),
        public_key,
        profile: session.config.profile,
        permissions: session.config.permissions,
        paired_at_ms: now_ms,
        last_sync_ms: None,
    };

    let response = PairCompleteResponse {
        server_device_id: server_device_id.clone(),
        server_device_name: server_device_name.to_owned(),
        granted_profile: session.config.profile,
        granted_permissions: session.config.permissions,
    };

    Ok((grant, response))
}

fn generate_one_time_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

fn now_ms() -> Result<u64, SyncError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    Ok(duration.as_millis() as u64)
}
