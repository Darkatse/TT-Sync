//! axum-based HTTP server for the TT-Sync v2 protocol.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use futures_util::TryStreamExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::io::{ReaderStream, StreamReader};
use uuid::Uuid;

use ttsync_contract::canonical::CanonicalRequest;
use ttsync_contract::pair::{PairCompleteRequest, PairCompleteResponse};
use ttsync_contract::path::SyncPath;
use ttsync_contract::plan::{CommitResponse, PlanId, PullPlanRequest, PushPlanRequest, SyncPlan};
use ttsync_contract::session::{
    SessionOpenRequest, SessionOpenResponse, SessionToken, HEADER_DEVICE_ID, HEADER_NONCE,
    HEADER_SIGNATURE, HEADER_TIMESTAMP_MS,
};
use ttsync_contract::sync::{ScopeProfileId, SyncMode};
use ttsync_core::error::SyncError;
use ttsync_core::pairing::{complete_pairing, PairingSession};
use ttsync_core::plan::compute_plan;
use ttsync_core::ports::{ManifestStore, PeerStore};
use ttsync_core::session::SessionManager;

use crate::tls::TlsProvider;

/// Shared state accessible by all route handlers.
pub struct ServerState<M, P> {
    pub server_device_id: ttsync_contract::peer::DeviceId,
    pub server_device_name: String,
    pub manifest_store: Arc<M>,
    pub peer_store: Arc<P>,
    pub session_manager: Arc<SessionManager>,
    pairing: std::sync::Mutex<HashMap<String, PairingSession>>,
    plans: std::sync::Mutex<HashMap<String, PlanRecord>>,
}

impl<M, P> ServerState<M, P> {
    pub fn new(
        server_device_id: ttsync_contract::peer::DeviceId,
        server_device_name: String,
        manifest_store: Arc<M>,
        peer_store: Arc<P>,
        session_manager: Arc<SessionManager>,
    ) -> Self {
        Self {
            server_device_id,
            server_device_name,
            manifest_store,
            peer_store,
            session_manager,
            pairing: std::sync::Mutex::new(HashMap::new()),
            plans: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn insert_pairing_session(&self, session: PairingSession) {
        let now_ms = now_ms().expect("system time must be valid");
        let mut map = self.pairing.lock().expect("pairing mutex poisoned");
        map.retain(|_, s| s.expires_at_ms > now_ms);
        map.insert(session.token.clone(), session);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanDirection {
    Pull,
    Push,
}

#[derive(Debug, Clone)]
struct PlanRecord {
    direction: PlanDirection,
    device_id: ttsync_contract::peer::DeviceId,
    mode: SyncMode,
    plan: SyncPlan,
    created_at_ms: u64,
}

/// Handle to a running TT-Sync server.
pub struct ServerHandle {
    pub addr: SocketAddr,
    handle: axum_server::Handle,
    _task: tokio::task::JoinHandle<()>,
}

impl ServerHandle {
    /// Initiate graceful shutdown.
    pub fn shutdown(self) {
        self.handle.graceful_shutdown(Some(Duration::from_secs(5)));
    }
}

/// Spawn the v2 HTTP server on the given address.
pub async fn spawn_server<M, P>(
    addr: SocketAddr,
    tls: Arc<dyn TlsProvider>,
    state: Arc<ServerState<M, P>>,
) -> Result<ServerHandle, SyncError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let server_config = tls.server_config()?;
    let tls_config = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server_config));

    let listener = std::net::TcpListener::bind(addr).map_err(|e| SyncError::Io(e.to_string()))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| SyncError::Io(e.to_string()))?;
    let addr = listener
        .local_addr()
        .map_err(|e| SyncError::Io(e.to_string()))?;

    let app = Router::new()
        .route("/v2/status", get(handle_status));

    let app = app
        .route("/v2/pair/complete", post(handle_pair_complete::<M, P>))
        .route("/v2/session/open", post(handle_session_open::<M, P>))
        .route("/v2/sync/pull-plan", post(handle_pull_plan::<M, P>))
        .route("/v2/sync/push-plan", post(handle_push_plan::<M, P>))
        .route(
            "/v2/plans/{plan_id}/files/{path_b64}",
            get(handle_download::<M, P>).put(handle_upload::<M, P>),
        )
        .route("/v2/plans/{plan_id}/commit", post(handle_commit::<M, P>))
        .with_state(state);

    let handle = axum_server::Handle::new();
    let server = axum_server::from_tcp_rustls(listener, tls_config).handle(handle.clone());

    let task = tokio::spawn(async move {
        if let Err(e) = server.serve(app.into_make_service()).await {
            tracing::error!("TT-Sync server failed: {}", e);
        }
    });

    Ok(ServerHandle {
        addr,
        handle,
        _task: task,
    })
}

async fn handle_status() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "protocol": "v2",
        "server": "tt-sync",
    }))
}

#[derive(Debug)]
struct ApiError(SyncError);

impl From<SyncError> for ApiError {
    fn from(value: SyncError) -> Self {
        Self(value)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self.0 {
            SyncError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            SyncError::InvalidData(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            SyncError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            SyncError::Io(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
            SyncError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };

        (
            status,
            Json(json!({
                "ok": false,
                "error": message,
            })),
        )
            .into_response()
    }
}

#[derive(serde::Deserialize)]
struct PairQuery {
    token: String,
}

async fn handle_pair_complete<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    Query(query): Query<PairQuery>,
    Json(request): Json<PairCompleteRequest>,
) -> Result<Json<PairCompleteResponse>, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let now_ms = now_ms()?;
    let session = {
        let mut sessions = state.pairing.lock().expect("pairing mutex poisoned");
        sessions.retain(|_, s| s.expires_at_ms > now_ms);
        sessions
            .remove(&query.token)
            .ok_or_else(|| SyncError::Unauthorized("invalid pair token".into()))?
    };

    let (grant, response) = complete_pairing(
        &session,
        &request,
        &state.server_device_id,
        &state.server_device_name,
    )?;

    state.peer_store.save_peer(grant).await?;
    Ok(Json(response))
}

async fn handle_session_open<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<SessionOpenResponse>, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let device_id = header_str(&headers, HEADER_DEVICE_ID)
        .and_then(|s| ttsync_contract::peer::DeviceId::new(s.to_owned()).map_err(|e| SyncError::InvalidData(e.to_string())))?;

    let timestamp_ms = header_str(&headers, HEADER_TIMESTAMP_MS)?
        .parse::<u64>()
        .map_err(|_| SyncError::InvalidData("invalid TT-Timestamp-Ms".into()))?;

    let nonce = header_str(&headers, HEADER_NONCE)?.to_owned();
    let signature_b64 = header_str(&headers, HEADER_SIGNATURE)?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature_b64)
        .map_err(|_| SyncError::InvalidData("invalid TT-Signature".into()))?;

    let request_body =
        serde_json::from_slice::<SessionOpenRequest>(&body).map_err(|e| SyncError::InvalidData(e.to_string()))?;
    if request_body.device_id != device_id {
        return Err(SyncError::Unauthorized("device_id mismatch".into()).into());
    }

    let body_hash = {
        let digest = Sha256::digest(&body);
        URL_SAFE_NO_PAD.encode(digest)
    };

    let path_and_query = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or(uri.path())
        .to_owned();

    let canonical = CanonicalRequest::new(
        device_id.clone(),
        timestamp_ms,
        nonce,
        method.as_str().to_owned(),
        path_and_query,
        body_hash,
    )
    .map_err(|e| SyncError::InvalidData(e.to_string()))?;
    let canonical_bytes = canonical.to_bytes();

    let grant = match state.peer_store.get_peer(&device_id).await {
        Ok(grant) => grant,
        Err(SyncError::NotFound(_)) => return Err(SyncError::Unauthorized("unknown peer".into()).into()),
        Err(e) => return Err(e.into()),
    };

    let response = state.session_manager.open_session(
        &device_id,
        &signature,
        &canonical_bytes,
        &grant.public_key,
    )?;

    Ok(Json(response))
}

async fn handle_pull_plan<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    headers: HeaderMap,
    Json(request): Json<PullPlanRequest>,
) -> Result<Json<SyncPlan>, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let peer = authenticate_peer(&state, &headers).await?;
    ensure_profile_allowed(peer.grant.profile, request.profile)?;
    ensure_mode_allowed(&peer.grant, request.mode)?;
    if !peer.grant.permissions.read {
        return Err(SyncError::Unauthorized("read not granted".into()).into());
    }

    let source_manifest = state.manifest_store.scan(&request.profile).await?;
    let plan_id = PlanId(Uuid::new_v4().to_string());
    let plan = compute_plan(plan_id, &source_manifest, &request.target_manifest, request.mode);

    insert_plan(
        &state,
        peer.device_id,
        PlanDirection::Pull,
        request.mode,
        plan.clone(),
    )?;

    Ok(Json(plan))
}

async fn handle_push_plan<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    headers: HeaderMap,
    Json(request): Json<PushPlanRequest>,
) -> Result<Json<SyncPlan>, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let peer = authenticate_peer(&state, &headers).await?;
    ensure_profile_allowed(peer.grant.profile, request.profile)?;
    ensure_mode_allowed(&peer.grant, request.mode)?;
    if !peer.grant.permissions.write {
        return Err(SyncError::Unauthorized("write not granted".into()).into());
    }

    let target_manifest = state.manifest_store.scan(&request.profile).await?;
    let plan_id = PlanId(Uuid::new_v4().to_string());
    let plan = compute_plan(plan_id, &request.source_manifest, &target_manifest, request.mode);

    insert_plan(
        &state,
        peer.device_id,
        PlanDirection::Push,
        request.mode,
        plan.clone(),
    )?;

    Ok(Json(plan))
}

async fn handle_download<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    headers: HeaderMap,
    Path((plan_id, path_b64)): Path<(String, String)>,
) -> Result<Response, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let peer = authenticate_peer(&state, &headers).await?;
    let sync_path = decode_sync_path_b64(&path_b64)?;

    let record = get_plan(&state, &plan_id, &peer.device_id)?;
    if record.direction != PlanDirection::Pull {
        return Err(SyncError::Unauthorized("plan is not a pull plan".into()).into());
    }
    if !peer.grant.permissions.read {
        return Err(SyncError::Unauthorized("read not granted".into()).into());
    }

    let entry = record
        .plan
        .transfer
        .iter()
        .find(|e| e.path == sync_path)
        .ok_or_else(|| SyncError::NotFound("file not in plan".into()))?
        .clone();

    let reader = state.manifest_store.read_file(&entry.path).await?;
    let stream = ReaderStream::new(reader);
    let body = Body::from_stream(stream);

    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&entry.size_bytes.to_string())
            .map_err(|e| SyncError::Internal(e.to_string()))?,
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("tt-modified-ms"),
        HeaderValue::from_str(&entry.modified_ms.to_string())
            .map_err(|e| SyncError::Internal(e.to_string()))?,
    );

    Ok(response)
}

async fn handle_upload<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    headers: HeaderMap,
    Path((plan_id, path_b64)): Path<(String, String)>,
    body: Body,
) -> Result<Json<serde_json::Value>, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let peer = authenticate_peer(&state, &headers).await?;
    let sync_path = decode_sync_path_b64(&path_b64)?;

    let record = get_plan(&state, &plan_id, &peer.device_id)?;
    if record.direction != PlanDirection::Push {
        return Err(SyncError::Unauthorized("plan is not a push plan".into()).into());
    }
    if !peer.grant.permissions.write {
        return Err(SyncError::Unauthorized("write not granted".into()).into());
    }

    let entry = record
        .plan
        .transfer
        .iter()
        .find(|e| e.path == sync_path)
        .ok_or_else(|| SyncError::NotFound("file not in plan".into()))?
        .clone();

    let stream = body
        .into_data_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    let mut reader = StreamReader::new(stream);

    state
        .manifest_store
        .write_file(&entry.path, &mut reader, entry.modified_ms)
        .await?;

    Ok(Json(json!({ "ok": true })))
}

async fn handle_commit<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
) -> Result<Json<CommitResponse>, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let peer = authenticate_peer(&state, &headers).await?;

    let record = take_plan(&state, &plan_id, &peer.device_id)?;
    if record.direction != PlanDirection::Push {
        return Err(SyncError::Unauthorized("plan is not a push plan".into()).into());
    }
    if !peer.grant.permissions.write {
        return Err(SyncError::Unauthorized("write not granted".into()).into());
    }
    if record.mode == SyncMode::Mirror && !peer.grant.permissions.mirror_delete {
        return Err(SyncError::Unauthorized("mirror delete not granted".into()).into());
    }

    if record.mode == SyncMode::Mirror {
        for path in &record.plan.delete {
            state.manifest_store.delete_file(path).await?;
        }
    }

    let now_ms = now_ms()?;
    let mut grant = peer.grant;
    grant.last_sync_ms = Some(now_ms);
    state.peer_store.save_peer(grant).await?;

    Ok(Json(CommitResponse { ok: true }))
}

struct PeerContext {
    device_id: ttsync_contract::peer::DeviceId,
    grant: ttsync_contract::peer::PeerGrant,
}

async fn authenticate_peer<M, P>(
    state: &ServerState<M, P>,
    headers: &HeaderMap,
) -> Result<PeerContext, ApiError>
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
        Err(SyncError::NotFound(_)) => return Err(SyncError::Unauthorized("unknown peer".into()).into()),
        Err(e) => return Err(e.into()),
    };

    Ok(PeerContext { device_id, grant })
}

fn bearer_token(headers: &HeaderMap) -> Result<String, SyncError> {
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

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, SyncError> {
    headers
        .get(name)
        .ok_or_else(|| SyncError::InvalidData(format!("missing header: {}", name)))?
        .to_str()
        .map_err(|_| SyncError::InvalidData(format!("invalid header: {}", name)))
}

fn ensure_profile_allowed(granted: ScopeProfileId, requested: ScopeProfileId) -> Result<(), SyncError> {
    match (granted, requested) {
        (ScopeProfileId::Default, _) => Ok(()),
        (ScopeProfileId::CompatibleMinimal, ScopeProfileId::CompatibleMinimal) => Ok(()),
        _ => Err(SyncError::Unauthorized("profile not granted".into())),
    }
}

fn ensure_mode_allowed(grant: &ttsync_contract::peer::PeerGrant, mode: SyncMode) -> Result<(), SyncError> {
    if mode == SyncMode::Mirror && !grant.permissions.mirror_delete {
        return Err(SyncError::Unauthorized("mirror delete not granted".into()));
    }
    Ok(())
}

fn decode_sync_path_b64(value: &str) -> Result<SyncPath, SyncError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| SyncError::InvalidData("invalid path encoding".into()))?;
    let text = String::from_utf8(bytes).map_err(|_| SyncError::InvalidData("non-UTF-8 path".into()))?;
    SyncPath::new(text).map_err(|e| SyncError::InvalidData(e.to_string()))
}

fn insert_plan<M, P>(
    state: &ServerState<M, P>,
    device_id: ttsync_contract::peer::DeviceId,
    direction: PlanDirection,
    mode: SyncMode,
    plan: SyncPlan,
) -> Result<(), SyncError> {
    let now_ms = now_ms()?;
    let mut plans = state.plans.lock().expect("plans mutex poisoned");
    plans.retain(|_, record| record.created_at_ms + 30 * 60 * 1000 > now_ms);

    plans.insert(
        plan.plan_id.0.clone(),
        PlanRecord {
            direction,
            device_id,
            mode,
            plan,
            created_at_ms: now_ms,
        },
    );
    Ok(())
}

fn get_plan<M, P>(
    state: &ServerState<M, P>,
    plan_id: &str,
    device_id: &ttsync_contract::peer::DeviceId,
) -> Result<PlanRecord, SyncError> {
    let now_ms = now_ms()?;
    let mut plans = state.plans.lock().expect("plans mutex poisoned");
    plans.retain(|_, record| record.created_at_ms + 30 * 60 * 1000 > now_ms);

    let record = plans
        .get(plan_id)
        .ok_or_else(|| SyncError::NotFound("plan not found".into()))?;

    if &record.device_id != device_id {
        return Err(SyncError::Unauthorized("plan does not belong to this peer".into()));
    }

    Ok(record.clone())
}

fn take_plan<M, P>(
    state: &ServerState<M, P>,
    plan_id: &str,
    device_id: &ttsync_contract::peer::DeviceId,
) -> Result<PlanRecord, SyncError> {
    let now_ms = now_ms()?;
    let mut plans = state.plans.lock().expect("plans mutex poisoned");
    plans.retain(|_, record| record.created_at_ms + 30 * 60 * 1000 > now_ms);

    let record = plans
        .remove(plan_id)
        .ok_or_else(|| SyncError::NotFound("plan not found".into()))?;

    if &record.device_id != device_id {
        return Err(SyncError::Unauthorized("plan does not belong to this peer".into()));
    }

    Ok(record)
}

fn now_ms() -> Result<u64, SyncError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    Ok(duration.as_millis() as u64)
}
