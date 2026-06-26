//! reqwest-based HTTP client for the TT-Sync v2 protocol.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::{Body, Client, Response};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};
use ttsync_contract::canonical::CanonicalRequest;
use ttsync_contract::dataset::{
    DATASET_POLICY_VERSION, DATASET_SCOPE_FEATURE_V1, DatasetSelection,
};
use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::pair::{PairCompleteRequest, PairCompleteResponse};
use ttsync_contract::path::SyncPath;
use ttsync_contract::peer::DeviceId;
use ttsync_contract::plan::{CommitResponse, PlanId, PullPlanRequest, PushPlanRequest, SyncPlan};
use ttsync_contract::session::{
    HEADER_DEVICE_ID, HEADER_NONCE, HEADER_SIGNATURE, HEADER_TIMESTAMP_MS, SessionOpenRequest,
    SessionOpenResponse, SessionToken,
};
use ttsync_contract::status::StatusResponse;
use ttsync_contract::sync::SyncMode;
use ttsync_core::bundle::BUNDLE_CONTENT_TYPE;
use ttsync_core::crypto::{random_base64url, sha256_base64url, sign_ed25519_b64url};
use ttsync_core::error::SyncError;
use url::Url;

/// A configured v2 sync client.
#[derive(Clone)]
pub struct SyncClient {
    http: Client,
    base_url: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SyncClientHttpVersion {
    #[default]
    Default,
    Http1Only,
}

#[derive(Debug, Clone, Default)]
pub struct SyncClientOptions {
    pub spki_sha256: Option<String>,
    pub user_agent: Option<String>,
    pub http_version: SyncClientHttpVersion,
}

impl SyncClient {
    /// Create a new client targeting the given base URL.
    ///
    /// `spki_sha256`: the expected SPKI hash for certificate pinning.
    pub fn new(base_url: String, spki_sha256: Option<String>) -> Result<Self, SyncError> {
        Self::with_options(
            base_url,
            SyncClientOptions {
                spki_sha256,
                ..SyncClientOptions::default()
            },
        )
    }

    pub fn with_options(base_url: String, options: SyncClientOptions) -> Result<Self, SyncError> {
        let parsed = Url::parse(&base_url).map_err(|e| SyncError::InvalidData(e.to_string()))?;
        validate_base_url(&parsed)?;

        let builder = reqwest::Client::builder();
        let builder = match options.spki_sha256 {
            Some(expected) => {
                let mut config = build_pinned_rustls_config(&expected)?;
                if options.http_version == SyncClientHttpVersion::Http1Only {
                    config.alpn_protocols = http1_only_alpn_protocols();
                }
                builder.use_preconfigured_tls(config)
            }
            None => builder,
        };
        let builder = match options.http_version {
            SyncClientHttpVersion::Default => builder,
            SyncClientHttpVersion::Http1Only => builder.http1_only(),
        };
        let builder = match options.user_agent {
            Some(user_agent) => builder.user_agent(user_agent),
            None => builder,
        };

        let http = builder
            .build()
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        Ok(Self { http, base_url })
    }

    pub fn http(&self) -> &Client {
        &self.http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Build an absolute endpoint URL under this client's validated base URL.
    ///
    /// This is intended for protocol extensions that share the TT-Sync transport
    /// contract but are not part of the core `/v2/*` route set.
    pub fn endpoint_url(&self, path: &str) -> Result<Url, SyncError> {
        endpoint_url(&self.base_url, path)
    }

    pub async fn pair_complete(
        &self,
        token: &str,
        request: &PairCompleteRequest,
    ) -> Result<PairCompleteResponse, SyncError> {
        let url = pair_complete_url(&self.base_url, token)?;
        let response = self
            .http
            .post(url)
            .json(request)
            .send()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        let response = ensure_success(response, "TT-Sync pairing failed").await?;
        response
            .json::<PairCompleteResponse>()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))
    }

    pub async fn status(&self) -> Result<StatusResponse, SyncError> {
        let url = endpoint_url(&self.base_url, "/v2/status")?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        let response = ensure_success(response, "TT-Sync status failed").await?;
        let status = response
            .json::<StatusResponse>()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        if !status.ok {
            return Err(SyncError::Internal(
                "TT-Sync status returned ok=false".into(),
            ));
        }

        Ok(status)
    }

    pub async fn open_session(
        &self,
        device_id: &DeviceId,
        ed25519_seed_b64url: &str,
    ) -> Result<SessionOpenResponse, SyncError> {
        let url = endpoint_url(&self.base_url, "/v2/session/open")?;
        let now_ms = now_ms()?;
        let nonce = random_base64url(12);
        let request = SessionOpenRequest {
            device_id: device_id.clone(),
        };
        let body = serde_json::to_vec(&request).map_err(|e| SyncError::Internal(e.to_string()))?;
        let body_hash = sha256_base64url(&body);

        let canonical = CanonicalRequest::new(
            device_id.clone(),
            now_ms,
            nonce.clone(),
            "POST".to_owned(),
            "/v2/session/open".to_owned(),
            body_hash,
        )
        .map_err(|e| SyncError::InvalidData(e.to_string()))?;
        let signature = sign_ed25519_b64url(ed25519_seed_b64url, &canonical.to_bytes())?;

        let response = self
            .http
            .post(url)
            .header(HEADER_DEVICE_ID, device_id.as_str())
            .header(HEADER_TIMESTAMP_MS, now_ms.to_string())
            .header(HEADER_NONCE, nonce)
            .header(HEADER_SIGNATURE, signature)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        let response = ensure_success(response, "TT-Sync session open failed").await?;
        response
            .json::<SessionOpenResponse>()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))
    }

    pub async fn pull_plan(
        &self,
        session_token: &SessionToken,
        mode: SyncMode,
        selection: DatasetSelection,
        target_manifest: ManifestV2,
    ) -> Result<SyncPlan, SyncError> {
        let url = endpoint_url(&self.base_url, "/v2/sync/pull-plan")?;
        let request = PullPlanRequest {
            mode,
            selection,
            target_manifest,
        };
        let body = serde_json::to_vec(&request).map_err(|e| SyncError::Internal(e.to_string()))?;

        let response = self
            .http
            .post(url)
            .header(
                reqwest::header::AUTHORIZATION,
                bearer_auth_value(session_token),
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        let response = ensure_success(response, "TT-Sync pull plan failed").await?;
        response
            .json::<SyncPlan>()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))
    }

    pub async fn push_plan(
        &self,
        session_token: &SessionToken,
        mode: SyncMode,
        selection: DatasetSelection,
        source_manifest: ManifestV2,
    ) -> Result<SyncPlan, SyncError> {
        let url = endpoint_url(&self.base_url, "/v2/sync/push-plan")?;
        let request = PushPlanRequest {
            mode,
            selection,
            source_manifest,
        };
        let body = serde_json::to_vec(&request).map_err(|e| SyncError::Internal(e.to_string()))?;

        let response = self
            .http
            .post(url)
            .header(
                reqwest::header::AUTHORIZATION,
                bearer_auth_value(session_token),
            )
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        let response = ensure_success(response, "TT-Sync push plan failed").await?;
        response
            .json::<SyncPlan>()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))
    }

    pub async fn download_file(
        &self,
        session_token: &SessionToken,
        plan_id: &PlanId,
        path: &SyncPath,
    ) -> Result<Response, SyncError> {
        let path_b64 = URL_SAFE_NO_PAD.encode(path.as_str().as_bytes());
        let url = file_url(&self.base_url, plan_id, &path_b64)?;
        let response = self
            .http
            .get(url)
            .header(
                reqwest::header::AUTHORIZATION,
                bearer_auth_value(session_token),
            )
            .send()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        ensure_success(response, "TT-Sync file download failed").await
    }

    pub async fn download_bundle(
        &self,
        session_token: &SessionToken,
        plan_id: &PlanId,
        accept_zstd: bool,
    ) -> Result<Response, SyncError> {
        let url = bundle_url(&self.base_url, plan_id)?;
        let mut request = self
            .http
            .get(url)
            .header(
                reqwest::header::AUTHORIZATION,
                bearer_auth_value(session_token),
            )
            .header(reqwest::header::ACCEPT, BUNDLE_CONTENT_TYPE);

        if accept_zstd {
            request = request.header(reqwest::header::ACCEPT_ENCODING, "zstd");
        }

        let response = request
            .send()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        ensure_success(response, "TT-Sync bundle download failed").await
    }

    pub async fn upload_file(
        &self,
        session_token: &SessionToken,
        plan_id: &PlanId,
        path: &SyncPath,
        body: Body,
    ) -> Result<(), SyncError> {
        let path_b64 = URL_SAFE_NO_PAD.encode(path.as_str().as_bytes());
        let url = file_url(&self.base_url, plan_id, &path_b64)?;
        let response = self
            .http
            .put(url)
            .header(
                reqwest::header::AUTHORIZATION,
                bearer_auth_value(session_token),
            )
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(body)
            .send()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        let response = ensure_success(response, "TT-Sync file upload failed").await?;
        response
            .bytes()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        Ok(())
    }

    pub async fn upload_bundle(
        &self,
        session_token: &SessionToken,
        plan_id: &PlanId,
        body: Body,
        content_encoding_zstd: bool,
    ) -> Result<(), SyncError> {
        let url = bundle_url(&self.base_url, plan_id)?;
        let mut request = self
            .http
            .put(url)
            .header(
                reqwest::header::AUTHORIZATION,
                bearer_auth_value(session_token),
            )
            .header(reqwest::header::CONTENT_TYPE, BUNDLE_CONTENT_TYPE);

        if content_encoding_zstd {
            request = request.header(reqwest::header::CONTENT_ENCODING, "zstd");
        }

        let response = request
            .body(body)
            .send()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        let response = ensure_success(response, "TT-Sync bundle upload failed").await?;
        response
            .bytes()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        Ok(())
    }

    pub async fn commit(
        &self,
        session_token: &SessionToken,
        plan_id: &PlanId,
    ) -> Result<CommitResponse, SyncError> {
        let url = commit_url(&self.base_url, plan_id)?;
        let response = self
            .http
            .post(url)
            .header(
                reqwest::header::AUTHORIZATION,
                bearer_auth_value(session_token),
            )
            .send()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))?;

        let response = ensure_success(response, "TT-Sync commit failed").await?;
        response
            .json::<CommitResponse>()
            .await
            .map_err(|e| SyncError::Internal(e.to_string()))
    }
}

pub fn ensure_dataset_scope_v1(status: &StatusResponse) -> Result<(), SyncError> {
    if !status
        .features
        .iter()
        .any(|feature| feature == DATASET_SCOPE_FEATURE_V1)
    {
        return Err(SyncError::InvalidData(
            "TT-Sync server does not support DatasetPolicy; upgrade TT-Sync server".into(),
        ));
    }

    let Some(version) = status.dataset_policy_version else {
        return Err(SyncError::InvalidData(
            "TT-Sync server did not report dataset policy version".into(),
        ));
    };

    if version != DATASET_POLICY_VERSION {
        return Err(SyncError::InvalidData(format!(
            "Unsupported TT-Sync dataset policy version: {}",
            version
        )));
    }

    Ok(())
}

fn pair_complete_url(base_url: &str, token: &str) -> Result<Url, SyncError> {
    let mut url = endpoint_url(base_url, "/v2/pair/complete")?;
    url.query_pairs_mut().append_pair("token", token);
    Ok(url)
}

fn file_url(base_url: &str, plan_id: &PlanId, path_b64: &str) -> Result<Url, SyncError> {
    endpoint_url(
        base_url,
        &format!("/v2/plans/{}/files/{}", plan_id.0, path_b64),
    )
}

fn bundle_url(base_url: &str, plan_id: &PlanId) -> Result<Url, SyncError> {
    endpoint_url(base_url, &format!("/v2/plans/{}/bundle", plan_id.0))
}

fn commit_url(base_url: &str, plan_id: &PlanId) -> Result<Url, SyncError> {
    endpoint_url(base_url, &format!("/v2/plans/{}/commit", plan_id.0))
}

fn endpoint_url(base_url: &str, path: &str) -> Result<Url, SyncError> {
    let mut url = Url::parse(base_url).map_err(|e| SyncError::InvalidData(e.to_string()))?;
    validate_base_url(&url)?;

    url.set_path(path);
    url.set_query(None);
    Ok(url)
}

fn validate_base_url(url: &Url) -> Result<(), SyncError> {
    if url.scheme() != "https" {
        return Err(SyncError::InvalidData(
            "TT-Sync base_url must be https".into(),
        ));
    }

    if url.host_str().is_none() {
        return Err(SyncError::InvalidData(
            "TT-Sync base_url must include a host".into(),
        ));
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err(SyncError::InvalidData(
            "TT-Sync base_url must not include credentials".into(),
        ));
    }

    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(SyncError::InvalidData(
            "TT-Sync base_url must not include path, query, or fragment".into(),
        ));
    }

    Ok(())
}

pub fn validate_https_origin(value: &str) -> Result<(), SyncError> {
    let url = Url::parse(value).map_err(|e| SyncError::InvalidData(e.to_string()))?;
    validate_base_url(&url)
}

pub fn bearer_auth_value(session_token: &SessionToken) -> String {
    format!("Bearer {}", session_token.as_str())
}

pub async fn ensure_success(response: Response, context: &str) -> Result<Response, SyncError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    let message = format!("{} ({}): {}", context, status, body);

    Err(response_error(status, message))
}

fn response_error(status: reqwest::StatusCode, message: String) -> SyncError {
    match status.as_u16() {
        400 | 413 => SyncError::InvalidData(message),
        401 | 403 => SyncError::Unauthorized(message),
        404 => SyncError::NotFound(message),
        _ => SyncError::Internal(message),
    }
}

fn now_ms() -> Result<u64, SyncError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| SyncError::Internal(e.to_string()))?;
    Ok(duration.as_millis() as u64)
}

pub fn build_pinned_rustls_config(
    expected_spki_sha256: &str,
) -> Result<rustls::ClientConfig, SyncError> {
    let signature_verifier = build_signature_verifier()?;

    let verifier = SpkiPinVerifier {
        expected_spki_sha256: expected_spki_sha256.to_owned(),
        signature_verifier,
    };

    let mut config =
        rustls::ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();

    config.alpn_protocols = default_alpn_protocols();
    Ok(config)
}

fn build_signature_verifier() -> Result<Arc<rustls::client::WebPkiServerVerifier>, SyncError> {
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    rustls::client::WebPkiServerVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|e| SyncError::Internal(e.to_string()))
}

fn default_alpn_protocols() -> Vec<Vec<u8>> {
    if cfg!(target_os = "android") {
        http1_only_alpn_protocols()
    } else {
        vec![b"h2".to_vec(), b"http/1.1".to_vec()]
    }
}

fn http1_only_alpn_protocols() -> Vec<Vec<u8>> {
    vec![b"http/1.1".to_vec()]
}

#[derive(Debug)]
struct SpkiPinVerifier {
    expected_spki_sha256: String,
    signature_verifier: Arc<rustls::client::WebPkiServerVerifier>,
}

impl ServerCertVerifier for SpkiPinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let parsed = rustls::server::ParsedCertificate::try_from(end_entity)?;
        let spki = parsed.subject_public_key_info();
        let digest = Sha256::digest(spki.as_ref());
        let actual = URL_SAFE_NO_PAD.encode(digest);

        if actual == self.expected_spki_sha256 {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.signature_verifier
            .verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.signature_verifier
            .verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.signature_verifier.supported_verify_schemes()
    }
}

#[cfg(test)]
mod tests {
    use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DATASET_SCOPE_FEATURE_V1};
    use ttsync_contract::status::StatusResponse;

    use super::{
        SyncClient, ensure_dataset_scope_v1, http1_only_alpn_protocols, response_error,
        validate_https_origin,
    };
    use ttsync_core::error::SyncError;

    fn status(features: Vec<String>, version: Option<u32>) -> StatusResponse {
        StatusResponse {
            ok: true,
            protocol: "v2".to_owned(),
            server: "tt-sync".to_owned(),
            features,
            dataset_policy_version: version,
            supported_dataset_ids: vec![],
            supported_profile_ids: vec![],
            default_dataset_ids: vec![],
            device_id: None,
            device_name: None,
            spki_sha256: None,
        }
    }

    #[test]
    fn response_error_maps_client_statuses() {
        assert!(matches!(
            response_error(
                reqwest::StatusCode::PAYLOAD_TOO_LARGE,
                "too large".to_owned()
            ),
            SyncError::InvalidData(_)
        ));
        assert!(matches!(
            response_error(reqwest::StatusCode::UNAUTHORIZED, "nope".to_owned()),
            SyncError::Unauthorized(_)
        ));
        assert!(matches!(
            response_error(reqwest::StatusCode::NOT_FOUND, "missing".to_owned()),
            SyncError::NotFound(_)
        ));
    }

    #[test]
    fn client_base_url_must_be_https_origin() {
        assert!(SyncClient::new("https://example.test/".to_owned(), None).is_ok());
        assert!(matches!(
            SyncClient::new("http://example.test/".to_owned(), None),
            Err(SyncError::InvalidData(_))
        ));
        assert!(matches!(
            SyncClient::new("https://example.test/tt-sync".to_owned(), None),
            Err(SyncError::InvalidData(_))
        ));
        assert!(matches!(
            SyncClient::new("https://example.test/?x=1".to_owned(), None),
            Err(SyncError::InvalidData(_))
        ));
        assert!(matches!(
            SyncClient::new("https://example.test/#x".to_owned(), None),
            Err(SyncError::InvalidData(_))
        ));
        assert!(matches!(
            SyncClient::new("https://user@example.test/".to_owned(), None),
            Err(SyncError::InvalidData(_))
        ));
    }

    #[test]
    fn shared_origin_validator_matches_client_contract() {
        validate_https_origin("https://example.test:8443").expect("valid origin");
        assert!(matches!(
            validate_https_origin("https://example.test/sync"),
            Err(SyncError::InvalidData(_))
        ));
        assert!(matches!(
            validate_https_origin("https://user@example.test"),
            Err(SyncError::InvalidData(_))
        ));
    }

    #[test]
    fn http1_policy_uses_http1_only_alpn() {
        assert_eq!(http1_only_alpn_protocols(), vec![b"http/1.1".to_vec()]);
    }

    #[test]
    fn status_requires_dataset_scope_feature() {
        let status = status(vec![], Some(DATASET_POLICY_VERSION));
        assert!(matches!(
            ensure_dataset_scope_v1(&status),
            Err(SyncError::InvalidData(_))
        ));
    }

    #[test]
    fn status_accepts_matching_dataset_policy_version() {
        let status = status(
            vec![DATASET_SCOPE_FEATURE_V1.to_owned()],
            Some(DATASET_POLICY_VERSION),
        );
        ensure_dataset_scope_v1(&status).expect("status should be accepted");
    }
}

#[cfg(all(test, feature = "server"))]
mod integration_tests {
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use reqwest::Body;
    use tokio::io::AsyncReadExt;
    use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DatasetSelection};
    use ttsync_contract::manifest::{ManifestEntryV2, ManifestV2};
    use ttsync_contract::pair::PairCompleteRequest;
    use ttsync_contract::path::SyncPath;
    use ttsync_contract::peer::{DeviceId, PeerGrant, Permissions};
    use ttsync_contract::sync::SyncMode;
    use ttsync_core::crypto::device_pubkey_b64url;
    use ttsync_core::dataset::ResolvedDatasetPolicy;
    use ttsync_core::error::SyncError;
    use ttsync_core::pairing::{PairingConfig, create_pairing_session};
    use ttsync_core::ports::{ManifestStore, PeerStore};
    use ttsync_core::session::{SessionManager, SessionManagerConfig};

    use super::SyncClient;
    use crate::pairing_store::PairingTokenStore;
    use crate::server::{ServerState, spawn_server};
    use crate::tls::{SelfManagedTls, TlsProvider};

    fn chat_selection() -> DatasetSelection {
        DatasetSelection::new(
            DATASET_POLICY_VERSION,
            vec!["chat.character.history".to_owned()],
        )
    }

    #[derive(Debug, Clone)]
    struct MemoryFile {
        bytes: Vec<u8>,
        modified_ms: u64,
    }

    #[derive(Debug, Default)]
    struct MemoryManifestStore {
        files: Mutex<HashMap<SyncPath, MemoryFile>>,
    }

    impl MemoryManifestStore {
        fn insert(&self, path: &str, bytes: &[u8], modified_ms: u64) {
            self.files.lock().expect("files mutex").insert(
                SyncPath::new(path.to_owned()).expect("valid sync path"),
                MemoryFile {
                    bytes: bytes.to_vec(),
                    modified_ms,
                },
            );
        }

        fn bytes(&self, path: &str) -> Vec<u8> {
            self.files
                .lock()
                .expect("files mutex")
                .get(&SyncPath::new(path.to_owned()).expect("valid sync path"))
                .expect("file exists")
                .bytes
                .clone()
        }
    }

    impl ManifestStore for MemoryManifestStore {
        fn scan(
            &self,
            policy: ResolvedDatasetPolicy,
        ) -> impl std::future::Future<Output = Result<ManifestV2, SyncError>> + Send {
            let mut entries = self
                .files
                .lock()
                .expect("files mutex")
                .iter()
                .filter(|(path, _)| policy.contains_path(path.as_str()))
                .map(|(path, file)| ManifestEntryV2 {
                    path: path.clone(),
                    size_bytes: file.bytes.len() as u64,
                    modified_ms: file.modified_ms,
                    content_hash: None,
                })
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));

            async move { Ok(ManifestV2 { entries }) }
        }

        fn read_file(
            &self,
            path: &SyncPath,
        ) -> impl std::future::Future<
            Output = Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, SyncError>,
        > + Send {
            let bytes = self
                .files
                .lock()
                .expect("files mutex")
                .get(path)
                .map(|file| file.bytes.clone());

            async move {
                let bytes = bytes.ok_or_else(|| SyncError::NotFound("file not found".into()))?;
                Ok(Box::new(Cursor::new(bytes)) as Box<dyn tokio::io::AsyncRead + Send + Unpin>)
            }
        }

        fn write_file(
            &self,
            path: &SyncPath,
            data: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
            modified_ms: u64,
        ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
            let files = &self.files;
            let path = path.clone();

            async move {
                let mut bytes = Vec::new();
                data.read_to_end(&mut bytes)
                    .await
                    .map_err(|e| SyncError::Io(e.to_string()))?;
                files
                    .lock()
                    .expect("files mutex")
                    .insert(path, MemoryFile { bytes, modified_ms });
                Ok(())
            }
        }

        fn delete_file(
            &self,
            path: &SyncPath,
        ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
            let path = path.clone();
            async move {
                self.files.lock().expect("files mutex").remove(&path);
                Ok(())
            }
        }
    }

    #[derive(Debug, Default)]
    struct MemoryPeerStore {
        peers: Mutex<HashMap<DeviceId, PeerGrant>>,
    }

    impl MemoryPeerStore {
        fn peer(&self, device_id: &DeviceId) -> PeerGrant {
            self.peers
                .lock()
                .expect("peers mutex")
                .get(device_id)
                .expect("peer exists")
                .clone()
        }
    }

    impl PeerStore for MemoryPeerStore {
        fn get_peer(
            &self,
            device_id: &DeviceId,
        ) -> impl std::future::Future<Output = Result<PeerGrant, SyncError>> + Send {
            let grant = self
                .peers
                .lock()
                .expect("peers mutex")
                .get(device_id)
                .cloned();

            async move { grant.ok_or_else(|| SyncError::NotFound("peer not found".into())) }
        }

        async fn save_peer(&self, grant: PeerGrant) -> Result<(), SyncError> {
            self.peers
                .lock()
                .expect("peers mutex")
                .insert(grant.device_id.clone(), grant);
            Ok(())
        }

        fn remove_peer(
            &self,
            device_id: &DeviceId,
        ) -> impl std::future::Future<Output = Result<(), SyncError>> + Send {
            let device_id = device_id.clone();
            async move {
                self.peers.lock().expect("peers mutex").remove(&device_id);
                Ok(())
            }
        }

        fn list_peers(
            &self,
        ) -> impl std::future::Future<Output = Result<Vec<PeerGrant>, SyncError>> + Send {
            let peers = self
                .peers
                .lock()
                .expect("peers mutex")
                .values()
                .cloned()
                .collect::<Vec<_>>();

            async move { Ok(peers) }
        }
    }

    #[tokio::test]
    async fn sync_client_completes_pair_session_pull_push_and_commit() {
        let state_dir = unique_temp_dir();
        let tls = SelfManagedTls::load_or_create(&state_dir).expect("TLS identity");
        let spki_sha256 = tls.spki_sha256().to_owned();
        let pairing_store = PairingTokenStore::from_state_dir(state_dir.clone());

        let manifest_store = Arc::new(MemoryManifestStore::default());
        manifest_store.insert("default-user/chats/server.jsonl", b"server", 1234);
        let peer_store = Arc::new(MemoryPeerStore::default());

        let server_device_id = DeviceId::new("00000000-0000-4000-8000-000000000010".to_owned())
            .expect("valid device id");
        let state = Arc::new(ServerState::new(
            server_device_id.clone(),
            "TT-Sync Test Server".to_owned(),
            manifest_store.clone(),
            peer_store.clone(),
            Arc::new(SessionManager::new(SessionManagerConfig::default())),
        ));

        let handle = spawn_server(
            "127.0.0.1:0".parse::<SocketAddr>().expect("valid addr"),
            Arc::new(tls),
            state,
            pairing_store.clone(),
        )
        .await
        .expect("spawn server");
        let base_url = format!("https://127.0.0.1:{}", handle.addr.port());

        let (pairing_session, _) = create_pairing_session(
            &base_url,
            &spki_sha256,
            PairingConfig {
                permissions: Permissions {
                    read: true,
                    write: true,
                    mirror_delete: true,
                },
                expires_in_secs: 60,
            },
        )
        .expect("pairing session");
        pairing_store
            .insert(&pairing_session)
            .expect("store pairing token");

        let client = SyncClient::new(base_url, Some(spki_sha256)).expect("client");
        let client_device_id = DeviceId::new("00000000-0000-4000-8000-000000000020".to_owned())
            .expect("valid device id");
        let client_seed = URL_SAFE_NO_PAD.encode([9u8; 32]);
        let pair_response = client
            .pair_complete(
                &pairing_session.token,
                &PairCompleteRequest {
                    device_id: client_device_id.clone(),
                    device_name: "TT-Sync Test Client".to_owned(),
                    device_pubkey: device_pubkey_b64url(&client_seed).expect("public key"),
                },
            )
            .await
            .expect("pair complete");
        assert_eq!(pair_response.server_device_id, server_device_id);

        let session = client
            .open_session(&client_device_id, &client_seed)
            .await
            .expect("open session");
        let selection = chat_selection();

        let pull_plan = client
            .pull_plan(
                &session.session_token,
                SyncMode::Incremental,
                selection.clone(),
                ManifestV2 { entries: vec![] },
            )
            .await
            .expect("pull plan");
        assert_eq!(pull_plan.transfer.len(), 1);

        let download = client
            .download_file(
                &session.session_token,
                &pull_plan.plan_id,
                &pull_plan.transfer[0].path,
            )
            .await
            .expect("download file");
        assert_eq!(
            download.bytes().await.expect("body bytes").as_ref(),
            b"server"
        );

        let pushed_path =
            SyncPath::new("default-user/chats/client.jsonl".to_owned()).expect("valid sync path");
        let push_plan = client
            .push_plan(
                &session.session_token,
                SyncMode::Incremental,
                selection,
                ManifestV2 {
                    entries: vec![ManifestEntryV2 {
                        path: pushed_path.clone(),
                        size_bytes: 6,
                        modified_ms: 2345,
                        content_hash: None,
                    }],
                },
            )
            .await
            .expect("push plan");
        assert_eq!(push_plan.transfer.len(), 1);

        let upload_error = client
            .upload_file(
                &session.session_token,
                &push_plan.plan_id,
                &pushed_path,
                Body::from("client-extra"),
            )
            .await
            .expect_err("oversized upload");
        assert!(matches!(upload_error, SyncError::InvalidData(_)));

        client
            .upload_file(
                &session.session_token,
                &push_plan.plan_id,
                &pushed_path,
                Body::from("client"),
            )
            .await
            .expect("upload file");
        let commit = client
            .commit(&session.session_token, &push_plan.plan_id)
            .await
            .expect("commit");
        assert!(commit.ok);
        assert_eq!(manifest_store.bytes(pushed_path.as_str()), b"client");
        assert!(peer_store.peer(&client_device_id).last_sync_ms.is_some());

        let empty_push_plan = client
            .push_plan(
                &session.session_token,
                SyncMode::Incremental,
                chat_selection(),
                ManifestV2 {
                    entries: vec![
                        ManifestEntryV2 {
                            path: SyncPath::new("default-user/chats/server.jsonl".to_owned())
                                .expect("valid sync path"),
                            size_bytes: 6,
                            modified_ms: 1234,
                            content_hash: None,
                        },
                        ManifestEntryV2 {
                            path: pushed_path.clone(),
                            size_bytes: 6,
                            modified_ms: 2345,
                            content_hash: None,
                        },
                    ],
                },
            )
            .await
            .expect("empty push plan");
        assert!(empty_push_plan.transfer.is_empty());

        let bundle_error = client
            .upload_bundle(
                &session.session_token,
                &empty_push_plan.plan_id,
                Body::from(vec![0, 0, 0, 0, b'x']),
                false,
            )
            .await
            .expect_err("trailing bundle byte");
        assert!(matches!(bundle_error, SyncError::InvalidData(_)));

        handle.shutdown();
        let _ = std::fs::remove_dir_all(state_dir);
    }

    fn unique_temp_dir() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("ttsync-http-client-e2e-{now}"))
    }
}
