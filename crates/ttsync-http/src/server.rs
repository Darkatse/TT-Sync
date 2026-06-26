//! axum-based HTTP server for the TT-Sync v2 protocol.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::HeaderMap;
use axum::routing::{get, post};
use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DATASET_SCOPE_FEATURE_V1};
use ttsync_contract::status::StatusResponse;
use ttsync_core::bundle::{FEATURE_BUNDLE_V1, FEATURE_ZSTD_V1};
use ttsync_core::dataset::{
    supported_dataset_ids, supported_profile_ids, tauri_tavern_default_selection,
};
use ttsync_core::error::SyncError;
use ttsync_core::ports::{ManifestStore, PeerStore};
use ttsync_core::session::SessionManager;

use crate::pairing_store::PairingTokenStore;
use crate::tls::TlsProvider;

mod auth;
mod bundle;
mod error;
mod handlers;
mod plans;

pub use auth::AuthenticatedPeer;

use auth::authenticate_peer;
use plans::PlanStore;

const SYNC_PLAN_BODY_LIMIT_BYTES: usize = 32 * 1024 * 1024;

/// Shared state accessible by all route handlers.
pub struct ServerState<M, P> {
    pub server_device_id: ttsync_contract::peer::DeviceId,
    pub server_device_name: String,
    pub manifest_store: Arc<M>,
    pub peer_store: Arc<P>,
    pub session_manager: Arc<SessionManager>,
    pub status: StatusResponse,
    plans: PlanStore,
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
            status: default_status_response(),
            plans: PlanStore::default(),
        }
    }

    pub fn with_status(mut self, status: StatusResponse) -> Self {
        self.status = status;
        self
    }

    pub async fn authenticate_headers(
        &self,
        headers: &HeaderMap,
    ) -> Result<AuthenticatedPeer, SyncError>
    where
        M: ManifestStore + 'static,
        P: PeerStore + 'static,
    {
        authenticate_peer(self, headers)
            .await
            .map_err(|error| error.0)
    }
}

/// State for the standard one-token pairing endpoint.
pub struct PairingState<M, P> {
    pub(crate) shared: Arc<ServerState<M, P>>,
    pub(crate) pairing_store: PairingTokenStore,
}

impl<M, P> PairingState<M, P> {
    pub fn new(shared: Arc<ServerState<M, P>>, pairing_store: PairingTokenStore) -> Self {
        Self {
            shared,
            pairing_store,
        }
    }
}

/// Handle to a running TT-Sync server.
pub struct ServerHandle {
    pub addr: SocketAddr,
    handle: axum_server::Handle<SocketAddr>,
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
    pairing_store: PairingTokenStore,
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

    let app = build_router(state, pairing_store);

    let handle = axum_server::Handle::<SocketAddr>::new();
    let mut server = axum_server::from_tcp_rustls(listener, tls_config)
        .map_err(|e| SyncError::Io(e.to_string()))?
        .handle(handle.clone());
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

pub fn build_router<M, P>(state: Arc<ServerState<M, P>>, pairing_store: PairingTokenStore) -> Router
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    build_transfer_router(state.clone()).merge(build_pairing_router(Arc::new(PairingState::new(
        state,
        pairing_store,
    ))))
}

pub fn build_transfer_router<M, P>(state: Arc<ServerState<M, P>>) -> Router
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    Router::new()
        .route("/v2/status", get(handlers::status::<M, P>))
        .route("/v2/session/open", post(handlers::session_open::<M, P>))
        .route(
            "/v2/sync/pull-plan",
            post(handlers::pull_plan::<M, P>).layer(sync_plan_body_limit()),
        )
        .route(
            "/v2/sync/push-plan",
            post(handlers::push_plan::<M, P>).layer(sync_plan_body_limit()),
        )
        .route(
            "/v2/plans/{plan_id}/files/{path_b64}",
            get(handlers::download::<M, P>).put(handlers::upload::<M, P>),
        )
        .route(
            "/v2/plans/{plan_id}/bundle",
            get(handlers::bundle_download::<M, P>).put(handlers::bundle_upload::<M, P>),
        )
        .route("/v2/plans/{plan_id}/commit", post(handlers::commit::<M, P>))
        .with_state(state)
}

pub fn build_pairing_router<M, P>(state: Arc<PairingState<M, P>>) -> Router
where
    M: ManifestStore + 'static,
    P: PeerStore + 'static,
{
    Router::new()
        .route("/v2/pair/complete", post(handlers::pair_complete::<M, P>))
        .with_state(state)
}

fn sync_plan_body_limit() -> DefaultBodyLimit {
    DefaultBodyLimit::max(SYNC_PLAN_BODY_LIMIT_BYTES)
}

pub fn default_status_response() -> StatusResponse {
    StatusResponse {
        ok: true,
        protocol: "v2".to_owned(),
        server: "tt-sync".to_owned(),
        features: vec![
            FEATURE_BUNDLE_V1.to_owned(),
            FEATURE_ZSTD_V1.to_owned(),
            DATASET_SCOPE_FEATURE_V1.to_owned(),
        ],
        dataset_policy_version: Some(DATASET_POLICY_VERSION),
        supported_dataset_ids: supported_dataset_ids(),
        supported_profile_ids: supported_profile_ids(),
        default_dataset_ids: tauri_tavern_default_selection().dataset_ids,
        device_id: None,
        device_name: None,
        spki_sha256: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode, header};
    use tower::ServiceExt;
    use ttsync_contract::manifest::ManifestV2;
    use ttsync_contract::path::SyncPath;
    use ttsync_contract::peer::{DeviceId, PeerGrant};
    use ttsync_core::dataset::ResolvedDatasetPolicy;
    use ttsync_core::session::SessionManagerConfig;
    use uuid::Uuid;

    #[derive(Debug)]
    struct UnusedManifestStore;

    impl ManifestStore for UnusedManifestStore {
        async fn scan(&self, _policy: ResolvedDatasetPolicy) -> Result<ManifestV2, SyncError> {
            Err(SyncError::Internal(
                "manifest store should not be used".into(),
            ))
        }

        async fn read_file(
            &self,
            _path: &SyncPath,
        ) -> Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, SyncError> {
            Err(SyncError::Internal(
                "manifest store should not be used".into(),
            ))
        }

        async fn write_file(
            &self,
            _path: &SyncPath,
            _data: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
            _modified_ms: u64,
        ) -> Result<(), SyncError> {
            Err(SyncError::Internal(
                "manifest store should not be used".into(),
            ))
        }

        async fn delete_file(&self, _path: &SyncPath) -> Result<(), SyncError> {
            Err(SyncError::Internal(
                "manifest store should not be used".into(),
            ))
        }
    }

    #[derive(Debug)]
    struct UnusedPeerStore;

    impl PeerStore for UnusedPeerStore {
        async fn get_peer(&self, _device_id: &DeviceId) -> Result<PeerGrant, SyncError> {
            Err(SyncError::Internal("peer store should not be used".into()))
        }

        async fn save_peer(&self, _grant: PeerGrant) -> Result<(), SyncError> {
            Err(SyncError::Internal("peer store should not be used".into()))
        }

        async fn remove_peer(&self, _device_id: &DeviceId) -> Result<(), SyncError> {
            Err(SyncError::Internal("peer store should not be used".into()))
        }

        async fn list_peers(&self) -> Result<Vec<PeerGrant>, SyncError> {
            Err(SyncError::Internal("peer store should not be used".into()))
        }
    }

    fn test_state() -> Arc<ServerState<UnusedManifestStore, UnusedPeerStore>> {
        Arc::new(ServerState::new(
            DeviceId::new(Uuid::new_v4().to_string()).expect("valid device id"),
            "TT-Sync Test".to_owned(),
            Arc::new(UnusedManifestStore),
            Arc::new(UnusedPeerStore),
            Arc::new(SessionManager::new(SessionManagerConfig::default())),
        ))
    }

    fn test_pairing_store() -> PairingTokenStore {
        let state_dir = std::env::temp_dir().join(format!("ttsync-http-test-{}", Uuid::new_v4()));
        PairingTokenStore::from_state_dir(state_dir)
    }

    fn pull_plan_body_at_least(min_size: usize) -> String {
        let prefix = r#"{"mode":"Incremental","selection":{"policy_version":1,"dataset_ids":["chat.character.history"]},"target_manifest":{"entries":[{"path":"default-user/chats/"#;
        let suffix = r#".json","size_bytes":1,"modified_ms":1}]}}"#;
        let filler_len = min_size.saturating_sub(prefix.len() + suffix.len());
        let body = format!("{prefix}{}{suffix}", "x".repeat(filler_len));
        assert!(body.len() >= min_size);
        body
    }

    fn push_plan_body_at_least(min_size: usize) -> String {
        let prefix = r#"{"mode":"Incremental","selection":{"policy_version":1,"dataset_ids":["chat.character.history"]},"source_manifest":{"entries":[{"path":"default-user/chats/"#;
        let suffix = r#".json","size_bytes":1,"modified_ms":1}]}}"#;
        let filler_len = min_size.saturating_sub(prefix.len() + suffix.len());
        let body = format!("{prefix}{}{suffix}", "x".repeat(filler_len));
        assert!(body.len() >= min_size);
        body
    }

    #[tokio::test]
    async fn plan_routes_reject_missing_selection_before_auth() {
        assert_eq!(
            post_plan(
                "/v2/sync/pull-plan",
                r#"{"mode":"Incremental","target_manifest":{"entries":[]}}"#.to_owned(),
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            post_plan(
                "/v2/sync/push-plan",
                r#"{"mode":"Incremental","source_manifest":{"entries":[]}}"#.to_owned(),
            )
            .await,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    async fn post_plan(path: &str, body: String) -> StatusCode {
        let app = build_transfer_router(test_state());
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
    async fn status_reports_datasets_and_profiles_separately() {
        let app = build_router(test_state(), test_pairing_store());
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v2/status")
                    .body(Body::empty())
                    .expect("valid request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read response body");
        let value: serde_json::Value =
            serde_json::from_slice(&body).expect("status response is JSON");

        assert!(
            value["supported_dataset_ids"]
                .as_array()
                .expect("dataset ids array")
                .iter()
                .any(|id| id == "agent.profiles")
        );
        assert!(
            value["supported_profile_ids"]
                .as_array()
                .expect("profile ids array")
                .iter()
                .any(|id| id == "tauritavern.default")
        );
        assert!(value.get("device_id").is_none());
        assert!(value.get("device_name").is_none());
        assert!(value.get("spki_sha256").is_none());
    }

    #[tokio::test]
    async fn transfer_router_does_not_expose_standard_pairing_route() {
        let app = build_transfer_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v2/pair/complete?token=test")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"device_id":"550e8400-e29b-41d4-a716-446655440000","device_name":"Peer","device_pubkey":"x"}"#,
                    ))
                    .expect("valid request"),
            )
            .await
            .expect("router response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
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
