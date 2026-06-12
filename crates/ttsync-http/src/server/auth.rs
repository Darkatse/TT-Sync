use axum::http::{HeaderMap, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ttsync_contract::path::SyncPath;
use ttsync_contract::peer::{DeviceId, PeerGrant};
use ttsync_contract::session::SessionToken;
use ttsync_contract::sync::SyncMode;
use ttsync_core::error::SyncError;
use ttsync_core::ports::{ManifestStore, PeerStore};

use super::ServerState;
use super::error::ApiError;

pub struct AuthenticatedPeer {
    pub device_id: DeviceId,
    pub grant: PeerGrant,
}

pub(super) async fn authenticate_peer<M, P>(
    state: &ServerState<M, P>,
    headers: &HeaderMap,
) -> Result<AuthenticatedPeer, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let token = bearer_token(headers)?;
    let device_id = state
        .session_manager
        .validate_session(&SessionToken(token))
        .map_err(ApiError)?;

    let grant = match state.peer_store.get_peer(&device_id).await {
        Ok(grant) => grant,
        Err(SyncError::NotFound(_)) => {
            return Err(SyncError::Unauthorized("unknown peer".into()).into());
        }
        Err(e) => return Err(e.into()),
    };

    Ok(AuthenticatedPeer { device_id, grant })
}

pub(super) fn bearer_token(headers: &HeaderMap) -> Result<String, SyncError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .ok_or_else(|| SyncError::Unauthorized("missing Authorization header".into()))?;
    let value = value
        .to_str()
        .map_err(|_| SyncError::Unauthorized("invalid Authorization header".into()))?;

    let token = value
        .strip_prefix("Bearer ")
        .ok_or_else(|| SyncError::Unauthorized("invalid Authorization scheme".into()))?;

    if token.is_empty() {
        return Err(SyncError::Unauthorized("empty session token".into()));
    }

    Ok(token.to_owned())
}

pub(super) fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, SyncError> {
    headers
        .get(name)
        .ok_or_else(|| SyncError::InvalidData(format!("missing header: {}", name)))?
        .to_str()
        .map_err(|_| SyncError::InvalidData(format!("invalid header: {}", name)))
}

pub(super) fn ensure_mode_allowed(grant: &PeerGrant, mode: SyncMode) -> Result<(), SyncError> {
    if mode == SyncMode::Mirror && !grant.permissions.mirror_delete {
        return Err(SyncError::Unauthorized("mirror delete not granted".into()));
    }
    Ok(())
}

pub(super) fn decode_sync_path_b64(value: &str) -> Result<SyncPath, SyncError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| SyncError::InvalidData("invalid path encoding".into()))?;
    let text =
        String::from_utf8(bytes).map_err(|_| SyncError::InvalidData("non-UTF-8 path".into()))?;
    SyncPath::new(text).map_err(|e| SyncError::InvalidData(e.to_string()))
}
