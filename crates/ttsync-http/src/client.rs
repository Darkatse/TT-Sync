//! reqwest-based HTTP client for the TT-Sync v2 protocol.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rcgen::CertifiedKey;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};
use ttsync_core::error::SyncError;

/// A configured v2 sync client.
pub struct SyncClient {
    http: reqwest::Client,
    base_url: String,
}

impl SyncClient {
    /// Create a new client targeting the given base URL.
    ///
    /// `spki_sha256`: the expected SPKI hash for certificate pinning.
    pub fn new(base_url: String, spki_sha256: Option<String>) -> Result<Self, SyncError> {
        let builder = reqwest::Client::builder();
        let client = match spki_sha256 {
            Some(expected) => {
                let config = build_pinned_rustls_config(&expected)?;
                builder
                    .use_preconfigured_tls(Some(config))
                    .build()
                    .map_err(|e| SyncError::Internal(e.to_string()))?
            }
            None => builder
                .build()
                .map_err(|e| SyncError::Internal(e.to_string()))?,
        };

        Ok(Self {
            http: client,
            base_url,
        })
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

fn build_pinned_rustls_config(
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

    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

fn build_signature_verifier() -> Result<Arc<rustls::client::WebPkiServerVerifier>, SyncError> {
    let CertifiedKey { cert, .. } =
        rcgen::generate_simple_self_signed(["ttsync-dummy-root".to_owned()])
            .map_err(|e| SyncError::Internal(e.to_string()))?;

    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(cert))
        .map_err(|e| SyncError::Internal(e.to_string()))?;

    rustls::client::WebPkiServerVerifier::builder(Arc::new(roots))
        .build()
        .map_err(|e| SyncError::Internal(e.to_string()))
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
