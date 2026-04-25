//! axum-based HTTP server for the TT-Sync v2 protocol.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::header;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures_util::TryStreamExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, ReadBuf};
use tokio_util::io::{ReaderStream, StreamReader};
use uuid::Uuid;

use async_compression::tokio::bufread::{ZstdDecoder, ZstdEncoder};

use ttsync_contract::canonical::CanonicalRequest;
use ttsync_contract::pair::{PairCompleteRequest, PairCompleteResponse};
use ttsync_contract::path::SyncPath;
use ttsync_contract::plan::{CommitResponse, PlanId, PullPlanRequest, PushPlanRequest, SyncPlan};
use ttsync_contract::session::{
    HEADER_DEVICE_ID, HEADER_NONCE, HEADER_SIGNATURE, HEADER_TIMESTAMP_MS, SessionOpenRequest,
    SessionOpenResponse, SessionToken,
};
use ttsync_contract::sync::SyncMode;
use ttsync_core::error::SyncError;
use ttsync_core::pairing::complete_pairing;
use ttsync_core::plan::compute_plan;
use ttsync_core::ports::{ManifestStore, PeerStore};
use ttsync_core::session::SessionManager;

use crate::pairing_store::PairingTokenStore;
use crate::tls::TlsProvider;

const SYNC_PLAN_BODY_LIMIT_BYTES: usize = 32 * 1024 * 1024;

/// Shared state accessible by all route handlers.
pub struct ServerState<M, P> {
    pub server_device_id: ttsync_contract::peer::DeviceId,
    pub server_device_name: String,
    pub manifest_store: Arc<M>,
    pub peer_store: Arc<P>,
    pub session_manager: Arc<SessionManager>,
    pub pairing_store: PairingTokenStore,
    plans: std::sync::Mutex<HashMap<String, PlanRecord>>,
}

impl<M, P> ServerState<M, P> {
    pub fn new(
        server_device_id: ttsync_contract::peer::DeviceId,
        server_device_name: String,
        manifest_store: Arc<M>,
        peer_store: Arc<P>,
        session_manager: Arc<SessionManager>,
        pairing_store: PairingTokenStore,
    ) -> Self {
        Self {
            server_device_id,
            server_device_name,
            manifest_store,
            peer_store,
            session_manager,
            pairing_store,
            plans: std::sync::Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanDirection {
    Pull,
    Push,
}

#[derive(Debug, Clone, Copy)]
struct TransferMeta {
    size_bytes: u64,
    modified_ms: u64,
}

#[derive(Debug)]
struct PlanRecord {
    direction: PlanDirection,
    device_id: ttsync_contract::peer::DeviceId,
    mode: SyncMode,
    transfer: HashMap<SyncPath, TransferMeta>,
    delete: Vec<SyncPath>,
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

    let app = build_router(state);

    let handle = axum_server::Handle::new();
    let mut server = axum_server::from_tcp_rustls(listener, tls_config).handle(handle.clone());
    server
        .http_builder()
        .http2()
        .max_concurrent_streams(Some(256))
        .initial_connection_window_size(Some(4 * 1024 * 1024))
        .initial_stream_window_size(Some(1024 * 1024));

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

fn build_router<M, P>(state: Arc<ServerState<M, P>>) -> Router
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    Router::new()
        .route("/v2/status", get(handle_status))
        .route("/v2/pair/complete", post(handle_pair_complete::<M, P>))
        .route("/v2/session/open", post(handle_session_open::<M, P>))
        .route(
            "/v2/sync/pull-plan",
            post(handle_pull_plan::<M, P>).layer(sync_plan_body_limit()),
        )
        .route(
            "/v2/sync/push-plan",
            post(handle_push_plan::<M, P>).layer(sync_plan_body_limit()),
        )
        .route(
            "/v2/plans/{plan_id}/files/{path_b64}",
            get(handle_download::<M, P>).put(handle_upload::<M, P>),
        )
        .route(
            "/v2/plans/{plan_id}/bundle",
            get(handle_bundle_download::<M, P>).put(handle_bundle_upload::<M, P>),
        )
        .route("/v2/plans/{plan_id}/commit", post(handle_commit::<M, P>))
        .with_state(state)
}

fn sync_plan_body_limit() -> DefaultBodyLimit {
    DefaultBodyLimit::max(SYNC_PLAN_BODY_LIMIT_BYTES)
}

async fn handle_status() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "protocol": "v2",
        "server": "tt-sync",
        "features": ["bundle_v1", "zstd_v1"],
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
    let session = state.pairing_store.take(&query.token, now_ms)?;

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
    let device_id = header_str(&headers, HEADER_DEVICE_ID).and_then(|s| {
        ttsync_contract::peer::DeviceId::new(s.to_owned())
            .map_err(|e| SyncError::InvalidData(e.to_string()))
    })?;

    let timestamp_ms = header_str(&headers, HEADER_TIMESTAMP_MS)?
        .parse::<u64>()
        .map_err(|_| SyncError::InvalidData("invalid TT-Timestamp-Ms".into()))?;

    let nonce = header_str(&headers, HEADER_NONCE)?.to_owned();
    let signature_b64 = header_str(&headers, HEADER_SIGNATURE)?;
    let signature = URL_SAFE_NO_PAD
        .decode(signature_b64)
        .map_err(|_| SyncError::InvalidData("invalid TT-Signature".into()))?;

    let request_body = serde_json::from_slice::<SessionOpenRequest>(&body)
        .map_err(|e| SyncError::InvalidData(e.to_string()))?;
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
        Err(SyncError::NotFound(_)) => {
            return Err(SyncError::Unauthorized("unknown peer".into()).into());
        }
        Err(e) => return Err(e.into()),
    };

    let response = state.session_manager.open_session(
        &device_id,
        &signature,
        &canonical_bytes,
        &grant.public_key,
        grant.permissions,
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
    ensure_mode_allowed(&peer.grant, request.mode)?;
    if !peer.grant.permissions.read {
        return Err(SyncError::Unauthorized("read not granted".into()).into());
    }

    let source_manifest = state.manifest_store.scan().await?;
    let plan_id = PlanId(Uuid::new_v4().to_string());
    let plan = compute_plan(
        plan_id,
        &source_manifest,
        &request.target_manifest,
        request.mode,
    );

    let response = plan.clone();
    insert_plan(
        &state,
        peer.device_id,
        PlanDirection::Pull,
        request.mode,
        plan,
    )?;

    Ok(Json(response))
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
    ensure_mode_allowed(&peer.grant, request.mode)?;
    if !peer.grant.permissions.write {
        return Err(SyncError::Unauthorized("write not granted".into()).into());
    }

    let target_manifest = state.manifest_store.scan().await?;
    let plan_id = PlanId(Uuid::new_v4().to_string());
    let plan = compute_plan(
        plan_id,
        &request.source_manifest,
        &target_manifest,
        request.mode,
    );

    let response = plan.clone();
    insert_plan(
        &state,
        peer.device_id,
        PlanDirection::Push,
        request.mode,
        plan,
    )?;

    Ok(Json(response))
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

    let (direction, meta) = get_plan_transfer_meta(&state, &plan_id, &peer.device_id, &sync_path)?;
    if direction != PlanDirection::Pull {
        return Err(SyncError::Unauthorized("plan is not a pull plan".into()).into());
    }
    if !peer.grant.permissions.read {
        return Err(SyncError::Unauthorized("read not granted".into()).into());
    }

    let reader = state.manifest_store.read_file(&sync_path).await?;
    let stream = ReaderStream::with_capacity(reader, 64 * 1024);
    let body = Body::from_stream(stream);

    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&meta.size_bytes.to_string())
            .map_err(|e| SyncError::Internal(e.to_string()))?,
    );
    response.headers_mut().insert(
        header::HeaderName::from_static("tt-modified-ms"),
        HeaderValue::from_str(&meta.modified_ms.to_string())
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

    let (direction, meta) = get_plan_transfer_meta(&state, &plan_id, &peer.device_id, &sync_path)?;
    if direction != PlanDirection::Push {
        return Err(SyncError::Unauthorized("plan is not a push plan".into()).into());
    }
    if !peer.grant.permissions.write {
        return Err(SyncError::Unauthorized("write not granted".into()).into());
    }

    let stream = body
        .into_data_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    let mut reader = StreamReader::new(stream);

    state
        .manifest_store
        .write_file(&sync_path, &mut reader, meta.modified_ms)
        .await?;

    Ok(Json(json!({ "ok": true })))
}

const BUNDLE_CONTENT_TYPE: &str = "application/x-ttsync-bundle";
const MAX_BUNDLE_PATH_LEN: u32 = 16 * 1024;

async fn handle_bundle_download<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
) -> Result<Response, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let peer = authenticate_peer(&state, &headers).await?;

    let snapshot = get_plan_snapshot(&state, &plan_id, &peer.device_id)?;
    if snapshot.direction != PlanDirection::Pull {
        return Err(SyncError::Unauthorized("plan is not a pull plan".into()).into());
    }
    if !peer.grant.permissions.read {
        return Err(SyncError::Unauthorized("read not granted".into()).into());
    }

    let wants_zstd = accepts_zstd(&headers);

    let mut transfer = snapshot
        .transfer
        .into_iter()
        .collect::<Vec<(SyncPath, TransferMeta)>>();
    transfer.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));

    let (reader, writer) = tokio::io::duplex(64 * 1024);
    let manifest_store = state.manifest_store.clone();
    tokio::spawn(async move {
        let result = write_bundle_download(manifest_store, transfer, writer).await;
        if let Err(e) = result {
            tracing::warn!("bundle download stream ended early: {}", e);
        }
    });

    let reader: Box<dyn AsyncRead + Send + Unpin> = if wants_zstd {
        Box::new(ZstdEncoder::new(BufReader::new(reader)))
    } else {
        Box::new(reader)
    };

    let stream = ReaderStream::with_capacity(reader, 64 * 1024);
    let body = Body::from_stream(stream);

    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(BUNDLE_CONTENT_TYPE),
    );
    if wants_zstd {
        response
            .headers_mut()
            .insert(header::CONTENT_ENCODING, HeaderValue::from_static("zstd"));
    }

    Ok(response)
}

async fn handle_bundle_upload<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
    body: Body,
) -> Result<Json<serde_json::Value>, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let peer = authenticate_peer(&state, &headers).await?;

    let snapshot = get_plan_snapshot(&state, &plan_id, &peer.device_id)?;
    if snapshot.direction != PlanDirection::Push {
        return Err(SyncError::Unauthorized("plan is not a push plan".into()).into());
    }
    if !peer.grant.permissions.write {
        return Err(SyncError::Unauthorized("write not granted".into()).into());
    }

    let encoding = request_content_encoding(&headers)?;

    let stream = body
        .into_data_stream()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
    let reader = StreamReader::new(stream);

    let mut reader: Box<dyn AsyncRead + Send + Unpin> = match encoding {
        BundleContentEncoding::Identity => Box::new(reader),
        BundleContentEncoding::Zstd => Box::new(ZstdDecoder::new(BufReader::new(reader))),
    };

    let mut remaining = snapshot.transfer;
    let files_total = remaining.len();
    let mut files_written = 0usize;

    loop {
        let path_len = read_u32_be(&mut reader).await?;
        if path_len == 0 {
            break;
        }
        if path_len > MAX_BUNDLE_PATH_LEN {
            return Err(SyncError::InvalidData(format!(
                "bundle path too long: {} bytes",
                path_len
            ))
            .into());
        }

        let mut path_bytes = vec![0u8; path_len as usize];
        reader
            .read_exact(&mut path_bytes)
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;

        let path_text = String::from_utf8(path_bytes)
            .map_err(|_| SyncError::InvalidData("non-UTF-8 path".into()))?;
        let sync_path =
            SyncPath::new(path_text).map_err(|e| SyncError::InvalidData(e.to_string()))?;

        let meta = remaining
            .remove(&sync_path)
            .ok_or_else(|| SyncError::NotFound("file not in plan".into()))?;

        let mut exact = ExactSizeReader::new(&mut reader, meta.size_bytes);
        state
            .manifest_store
            .write_file(&sync_path, &mut exact, meta.modified_ms)
            .await?;

        files_written += 1;
    }

    if !remaining.is_empty() {
        return Err(SyncError::InvalidData(format!(
            "bundle ended early: {}/{} files received",
            files_written, files_total
        ))
        .into());
    }

    Ok(Json(json!({ "ok": true, "files_written": files_written })))
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
        for path in &record.delete {
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
        Err(SyncError::NotFound(_)) => {
            return Err(SyncError::Unauthorized("unknown peer".into()).into());
        }
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

fn ensure_mode_allowed(
    grant: &ttsync_contract::peer::PeerGrant,
    mode: SyncMode,
) -> Result<(), SyncError> {
    if mode == SyncMode::Mirror && !grant.permissions.mirror_delete {
        return Err(SyncError::Unauthorized("mirror delete not granted".into()));
    }
    Ok(())
}

fn decode_sync_path_b64(value: &str) -> Result<SyncPath, SyncError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| SyncError::InvalidData("invalid path encoding".into()))?;
    let text =
        String::from_utf8(bytes).map_err(|_| SyncError::InvalidData("non-UTF-8 path".into()))?;
    SyncPath::new(text).map_err(|e| SyncError::InvalidData(e.to_string()))
}

fn accepts_zstd(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(header::ACCEPT_ENCODING) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };

    value.split(',').any(|part| {
        let name = part.split(';').next().unwrap_or_default().trim();
        name.eq_ignore_ascii_case("zstd")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundleContentEncoding {
    Identity,
    Zstd,
}

fn request_content_encoding(headers: &HeaderMap) -> Result<BundleContentEncoding, SyncError> {
    let Some(value) = headers.get(header::CONTENT_ENCODING) else {
        return Ok(BundleContentEncoding::Identity);
    };

    let value = value
        .to_str()
        .map_err(|_| SyncError::InvalidData("invalid Content-Encoding header".into()))?
        .trim();

    if value.eq_ignore_ascii_case("identity") {
        Ok(BundleContentEncoding::Identity)
    } else if value.eq_ignore_ascii_case("zstd") {
        Ok(BundleContentEncoding::Zstd)
    } else {
        Err(SyncError::InvalidData(format!(
            "unsupported Content-Encoding: {}",
            value
        )))
    }
}

async fn read_u32_be<R>(reader: &mut R) -> Result<u32, SyncError>
where
    R: AsyncRead + Unpin,
{
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;
    Ok(u32::from_be_bytes(buf))
}

async fn write_bundle_download<M>(
    manifest_store: Arc<M>,
    transfer: Vec<(SyncPath, TransferMeta)>,
    mut out: tokio::io::DuplexStream,
) -> Result<(), SyncError>
where
    M: ManifestStore + 'static,
{
    for (path, meta) in transfer {
        let path_bytes = path.as_str().as_bytes();
        let path_len = u32::try_from(path_bytes.len())
            .map_err(|_| SyncError::InvalidData("bundle path is too long to encode".into()))?;
        if path_len > MAX_BUNDLE_PATH_LEN {
            return Err(SyncError::InvalidData(format!(
                "bundle path is too long to encode: {} bytes",
                path_len
            )));
        }

        out.write_all(&path_len.to_be_bytes())
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;
        out.write_all(path_bytes)
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;

        let mut reader = manifest_store.read_file(&path).await?;
        copy_exact(&mut reader, &mut out, meta.size_bytes).await?;
    }

    out.write_all(&0u32.to_be_bytes())
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;

    Ok(())
}

async fn copy_exact<R, W>(
    reader: &mut R,
    writer: &mut W,
    mut remaining: u64,
) -> Result<(), SyncError>
where
    R: AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let to_read = (buffer.len() as u64).min(remaining) as usize;
        let read = reader
            .read(&mut buffer[..to_read])
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;
        if read == 0 {
            return Err(SyncError::Io("unexpected EOF in bundle stream".into()));
        }
        writer
            .write_all(&buffer[..read])
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;
        remaining -= read as u64;
    }
    Ok(())
}

struct ExactSizeReader<R> {
    inner: R,
    remaining: u64,
}

impl<R> ExactSizeReader<R> {
    fn new(inner: R, size_bytes: u64) -> Self {
        Self {
            inner,
            remaining: size_bytes,
        }
    }
}

impl<R> AsyncRead for ExactSizeReader<R>
where
    R: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.remaining == 0 {
            return Poll::Ready(Ok(()));
        }

        let max = (self.remaining as usize).min(buf.remaining());
        if max == 0 {
            return Poll::Ready(Ok(()));
        }

        let dst = buf.initialize_unfilled_to(max);
        let mut limited = ReadBuf::new(dst);
        match Pin::new(&mut self.inner).poll_read(cx, &mut limited) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => {
                let read = limited.filled().len();
                if read == 0 {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "bundle file stream ended early",
                    )));
                }

                buf.advance(read);
                self.remaining -= read as u64;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
        }
    }
}

#[derive(Debug, Clone)]
struct PlanSnapshot {
    direction: PlanDirection,
    transfer: HashMap<SyncPath, TransferMeta>,
}

fn get_plan_snapshot<M, P>(
    state: &ServerState<M, P>,
    plan_id: &str,
    device_id: &ttsync_contract::peer::DeviceId,
) -> Result<PlanSnapshot, SyncError> {
    let now_ms = now_ms()?;
    let mut plans = state.plans.lock().expect("plans mutex poisoned");
    plans.retain(|_, record| record.created_at_ms + 30 * 60 * 1000 > now_ms);

    let record = plans
        .get(plan_id)
        .ok_or_else(|| SyncError::NotFound("plan not found".into()))?;

    if &record.device_id != device_id {
        return Err(SyncError::Unauthorized(
            "plan does not belong to this peer".into(),
        ));
    }

    Ok(PlanSnapshot {
        direction: record.direction,
        transfer: record.transfer.clone(),
    })
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

    let SyncPlan {
        plan_id: PlanId(plan_id),
        transfer,
        delete,
        files_total: _,
        bytes_total: _,
    } = plan;

    let transfer = transfer
        .into_iter()
        .map(|entry| {
            (
                entry.path,
                TransferMeta {
                    size_bytes: entry.size_bytes,
                    modified_ms: entry.modified_ms,
                },
            )
        })
        .collect::<HashMap<_, _>>();

    plans.insert(
        plan_id,
        PlanRecord {
            direction,
            device_id,
            mode,
            transfer,
            delete,
            created_at_ms: now_ms,
        },
    );
    Ok(())
}

fn get_plan_transfer_meta<M, P>(
    state: &ServerState<M, P>,
    plan_id: &str,
    device_id: &ttsync_contract::peer::DeviceId,
    sync_path: &SyncPath,
) -> Result<(PlanDirection, TransferMeta), SyncError> {
    let now_ms = now_ms()?;
    let mut plans = state.plans.lock().expect("plans mutex poisoned");
    plans.retain(|_, record| record.created_at_ms + 30 * 60 * 1000 > now_ms);

    let record = plans
        .get(plan_id)
        .ok_or_else(|| SyncError::NotFound("plan not found".into()))?;

    if &record.device_id != device_id {
        return Err(SyncError::Unauthorized(
            "plan does not belong to this peer".into(),
        ));
    }

    let meta = record
        .transfer
        .get(sync_path)
        .ok_or_else(|| SyncError::NotFound("file not in plan".into()))?;

    Ok((record.direction, *meta))
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
        return Err(SyncError::Unauthorized(
            "plan does not belong to this peer".into(),
        ));
    }

    Ok(record)
}

fn now_ms() -> Result<u64, SyncError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    Ok(duration.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::http::Request;
    use tower::ServiceExt;
    use ttsync_contract::manifest::ManifestV2;
    use ttsync_contract::peer::{DeviceId, PeerGrant};
    use ttsync_core::session::SessionManagerConfig;

    #[derive(Debug)]
    struct UnusedManifestStore;

    impl ManifestStore for UnusedManifestStore {
        fn scan(&self) -> impl std::future::Future<Output = Result<ManifestV2, SyncError>> + Send {
            async {
                Err(SyncError::Internal(
                    "manifest store should not be used".into(),
                ))
            }
        }

        fn read_file(
            &self,
            _path: &SyncPath,
        ) -> impl std::future::Future<
            Output = Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, SyncError>,
        > + Send {
            async {
                Err::<Box<dyn tokio::io::AsyncRead + Send + Unpin>, _>(SyncError::Internal(
                    "manifest store should not be used".into(),
                ))
            }
        }

        fn write_file(
            &self,
            _path: &SyncPath,
            _data: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
            _modified_ms: u64,
        ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
            async {
                Err(SyncError::Internal(
                    "manifest store should not be used".into(),
                ))
            }
        }

        fn delete_file(
            &self,
            _path: &SyncPath,
        ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
            async {
                Err(SyncError::Internal(
                    "manifest store should not be used".into(),
                ))
            }
        }
    }

    #[derive(Debug)]
    struct UnusedPeerStore;

    impl PeerStore for UnusedPeerStore {
        fn get_peer(
            &self,
            _device_id: &DeviceId,
        ) -> impl std::future::Future<Output = Result<PeerGrant, SyncError>> + Send {
            async { Err(SyncError::Internal("peer store should not be used".into())) }
        }

        fn save_peer(
            &self,
            _grant: PeerGrant,
        ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
            async { Err(SyncError::Internal("peer store should not be used".into())) }
        }

        fn remove_peer(
            &self,
            _device_id: &DeviceId,
        ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
            async { Err(SyncError::Internal("peer store should not be used".into())) }
        }

        fn list_peers(
            &self,
        ) -> impl std::future::Future<Output = Result<Vec<PeerGrant>, SyncError>> + Send {
            async { Err(SyncError::Internal("peer store should not be used".into())) }
        }
    }

    fn test_state() -> Arc<ServerState<UnusedManifestStore, UnusedPeerStore>> {
        let state_dir = std::env::temp_dir().join(format!("ttsync-http-test-{}", Uuid::new_v4()));

        Arc::new(ServerState::new(
            DeviceId::new(Uuid::new_v4().to_string()).expect("valid device id"),
            "TT-Sync Test".to_owned(),
            Arc::new(UnusedManifestStore),
            Arc::new(UnusedPeerStore),
            Arc::new(SessionManager::new(SessionManagerConfig::default())),
            PairingTokenStore::from_state_dir(state_dir),
        ))
    }

    fn pull_plan_body_at_least(min_size: usize) -> String {
        let prefix =
            r#"{"mode":"Incremental","target_manifest":{"entries":[{"path":"default-user/chats/"#;
        let suffix = r#".json","size_bytes":1,"modified_ms":1}]}}"#;
        let filler_len = min_size.saturating_sub(prefix.len() + suffix.len());
        let body = format!("{prefix}{}{suffix}", "x".repeat(filler_len));
        assert!(body.len() >= min_size);
        body
    }

    fn push_plan_body_at_least(min_size: usize) -> String {
        let prefix =
            r#"{"mode":"Incremental","source_manifest":{"entries":[{"path":"default-user/chats/"#;
        let suffix = r#".json","size_bytes":1,"modified_ms":1}]}}"#;
        let filler_len = min_size.saturating_sub(prefix.len() + suffix.len());
        let body = format!("{prefix}{}{suffix}", "x".repeat(filler_len));
        assert!(body.len() >= min_size);
        body
    }

    async fn post_plan(path: &str, body: String) -> StatusCode {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .expect("valid request"),
            )
            .await
            .expect("router response");

        response.status()
    }

    #[tokio::test]
    async fn plan_routes_accept_manifests_above_axum_default_limit() {
        const AXUM_DEFAULT_BODY_LIMIT_BYTES: usize = 2_097_152;
        let body_size = AXUM_DEFAULT_BODY_LIMIT_BYTES + 4096;

        assert_eq!(
            post_plan("/v2/sync/pull-plan", pull_plan_body_at_least(body_size)).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            post_plan("/v2/sync/push-plan", push_plan_body_at_least(body_size)).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn plan_routes_reject_manifests_above_explicit_limit() {
        assert_eq!(
            post_plan(
                "/v2/sync/pull-plan",
                pull_plan_body_at_least(SYNC_PLAN_BODY_LIMIT_BYTES + 1),
            )
            .await,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}
