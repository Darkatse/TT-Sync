use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_compression::tokio::bufread::{ZstdDecoder, ZstdEncoder};
use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures_util::TryStreamExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};
use tokio_util::io::{ReaderStream, StreamReader};
use ttsync_contract::canonical::CanonicalRequest;
use ttsync_contract::pair::{PairCompleteRequest, PairCompleteResponse};
use ttsync_contract::path::SyncPath;
use ttsync_contract::plan::{CommitResponse, PlanId, PullPlanRequest, PushPlanRequest, SyncPlan};
use ttsync_contract::session::{
    HEADER_DEVICE_ID, HEADER_NONCE, HEADER_SIGNATURE, HEADER_TIMESTAMP_MS, SessionOpenRequest,
    SessionOpenResponse,
};
use ttsync_contract::status::StatusResponse;
use ttsync_contract::sync::SyncMode;
use ttsync_core::dataset::ResolvedDatasetPolicy;
use ttsync_core::error::SyncError;
use ttsync_core::pairing::complete_pairing;
use ttsync_core::plan::compute_plan_for_policy;
use ttsync_core::ports::{ManifestStore, PeerStore};
use uuid::Uuid;

use super::ServerState;
use super::auth::{authenticate_peer, decode_sync_path_b64, ensure_mode_allowed, header_str};
use super::bundle::{
    BUNDLE_CONTENT_TYPE, BundleContentEncoding, ExactSizeReader, MAX_BUNDLE_PATH_LEN, accepts_zstd,
    read_u32_be, request_content_encoding, write_bundle_download,
};
use super::error::ApiError;
use super::plans::{PlanDirection, TransferMeta};

#[derive(serde::Deserialize)]
pub(super) struct PairQuery {
    token: String,
}

pub(super) async fn status<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
) -> Json<StatusResponse>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    Json(state.status.clone())
}

pub(super) async fn pair_complete<M, P>(
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

pub(super) async fn session_open<M, P>(
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

pub(super) async fn pull_plan<M, P>(
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

    let policy = ResolvedDatasetPolicy::from_selection(&request.selection)?;
    let source_manifest = state.manifest_store.scan(policy.clone()).await?;
    let plan_id = PlanId(Uuid::new_v4().to_string());
    let plan = compute_plan_for_policy(
        plan_id,
        &source_manifest,
        &request.target_manifest,
        request.mode,
        &policy,
    )?;

    let response = plan.clone();
    state
        .plans
        .insert(peer.device_id, PlanDirection::Pull, request.mode, plan)?;

    Ok(Json(response))
}

pub(super) async fn push_plan<M, P>(
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

    let policy = ResolvedDatasetPolicy::from_selection(&request.selection)?;
    let target_manifest = state.manifest_store.scan(policy.clone()).await?;
    let plan_id = PlanId(Uuid::new_v4().to_string());
    let plan = compute_plan_for_policy(
        plan_id,
        &request.source_manifest,
        &target_manifest,
        request.mode,
        &policy,
    )?;

    let response = plan.clone();
    state
        .plans
        .insert(peer.device_id, PlanDirection::Push, request.mode, plan)?;

    Ok(Json(response))
}

pub(super) async fn download<M, P>(
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

    let (direction, meta) = state
        .plans
        .transfer_meta(&plan_id, &peer.device_id, &sync_path)?;
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

pub(super) async fn upload<M, P>(
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

    let (direction, meta) = state
        .plans
        .transfer_meta(&plan_id, &peer.device_id, &sync_path)?;
    if direction != PlanDirection::Push {
        return Err(SyncError::Unauthorized("plan is not a push plan".into()).into());
    }
    if !peer.grant.permissions.write {
        return Err(SyncError::Unauthorized("write not granted".into()).into());
    }

    let stream = body.into_data_stream().map_err(std::io::Error::other);
    let mut reader = StreamReader::new(stream);

    state
        .manifest_store
        .write_file(&sync_path, &mut reader, meta.modified_ms)
        .await?;

    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn bundle_download<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
) -> Result<Response, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let peer = authenticate_peer(&state, &headers).await?;

    let snapshot = state.plans.snapshot(&plan_id, &peer.device_id)?;
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

pub(super) async fn bundle_upload<M, P>(
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

    let snapshot = state.plans.snapshot(&plan_id, &peer.device_id)?;
    if snapshot.direction != PlanDirection::Push {
        return Err(SyncError::Unauthorized("plan is not a push plan".into()).into());
    }
    if !peer.grant.permissions.write {
        return Err(SyncError::Unauthorized("write not granted".into()).into());
    }

    let encoding = request_content_encoding(&headers)?;

    let stream = body.into_data_stream().map_err(std::io::Error::other);
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

pub(super) async fn commit<M, P>(
    State(state): State<Arc<ServerState<M, P>>>,
    headers: HeaderMap,
    Path(plan_id): Path<String>,
) -> Result<Json<CommitResponse>, ApiError>
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    let peer = authenticate_peer(&state, &headers).await?;

    let record = state.plans.take_if(&plan_id, &peer.device_id, |record| {
        if record.direction != PlanDirection::Push {
            return Err(SyncError::Unauthorized("plan is not a push plan".into()));
        }
        if !peer.grant.permissions.write {
            return Err(SyncError::Unauthorized("write not granted".into()));
        }
        if record.mode == SyncMode::Mirror && !peer.grant.permissions.mirror_delete {
            return Err(SyncError::Unauthorized("mirror delete not granted".into()));
        }
        Ok(())
    })?;

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

fn now_ms() -> Result<u64, SyncError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    Ok(duration.as_millis() as u64)
}
