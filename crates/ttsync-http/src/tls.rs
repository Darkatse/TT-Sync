//! TLS configuration for TT-Sync.
//!
//! MVP: `SelfManagedTls` — generates a long-term TLS key + self-signed cert via rcgen,
//! computes the SPKI SHA-256 hash for certificate pinning.

use std::io::BufReader;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rcgen::CertifiedKey;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use ttsync_core::error::SyncError;

/// TLS deployment mode abstraction.
pub trait TlsProvider: Send + Sync {
    /// Get the SPKI SHA-256 hash of the server's TLS public key.
    fn spki_sha256(&self) -> &str;

    /// Build a rustls ServerConfig for this TLS mode.
    fn server_config(&self) -> Result<rustls::ServerConfig, SyncError>;
}

/// Self-managed TLS: TT-Sync generates its own key + self-signed certificate.
pub struct SelfManagedTls {
    spki_sha256: String,
    cert_chain: Vec<CertificateDer<'static>>,
    private_key: PrivateKeyDer<'static>,
}

impl SelfManagedTls {
    /// Load existing or generate new TLS identity from the state directory.
    pub fn load_or_create(state_dir: &Path) -> Result<Self, SyncError> {
        let tls_dir = state_dir.join("tls");
        std::fs::create_dir_all(&tls_dir).map_err(|e| SyncError::Io(e.to_string()))?;

        let key_path = tls_dir.join("key.pem");
        let cert_path = tls_dir.join("cert.pem");

        if key_path.exists() && cert_path.exists() {
            let key_pem = std::fs::read(&key_path).map_err(|e| SyncError::Io(e.to_string()))?;
            let cert_pem = std::fs::read(&cert_path).map_err(|e| SyncError::Io(e.to_string()))?;

            let cert_chain = parse_cert_chain_pem(&cert_pem)?;
            let private_key = parse_private_key_pem(&key_pem)?;
            let spki_sha256 = spki_sha256_from_cert(&cert_chain[0])?;

            return Ok(Self {
                spki_sha256,
                cert_chain,
                private_key,
            });
        }

        let CertifiedKey { cert, key_pair } =
            rcgen::generate_simple_self_signed(["tt-sync".to_owned(), "localhost".to_owned()])
                .map_err(|e| SyncError::Internal(e.to_string()))?;

        let key_pem = key_pair.serialize_pem();
        let cert_pem = cert.pem();

        write_atomic_text(&key_path, &key_pem)?;
        write_atomic_text(&cert_path, &cert_pem)?;

        let spki_sha256 = {
            let digest = Sha256::digest(key_pair.public_key_der());
            URL_SAFE_NO_PAD.encode(digest)
        };

        let cert_chain = vec![cert.der().clone()];
        let private_key = PrivatePkcs8KeyDer::from(key_pair.serialize_der()).into();

        Ok(Self {
            spki_sha256,
            cert_chain,
            private_key,
        })
    }
}

impl TlsProvider for SelfManagedTls {
    fn spki_sha256(&self) -> &str {
        &self.spki_sha256
    }

    fn server_config(&self) -> Result<rustls::ServerConfig, SyncError> {
        let mut config =
            rustls::ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
                .with_no_client_auth()
                .with_single_cert(self.cert_chain.clone(), self.private_key.clone_key())
                .map_err(|e| SyncError::Internal(e.to_string()))?;

        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(config)
    }
}

fn parse_cert_chain_pem(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, SyncError> {
    let mut reader = BufReader::new(pem);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| SyncError::InvalidData(e.to_string()))?;

    if certs.is_empty() {
        return Err(SyncError::InvalidData(
            "no certificates found in cert.pem".into(),
        ));
    }

    Ok(certs)
}

fn parse_private_key_pem(pem: &[u8]) -> Result<PrivateKeyDer<'static>, SyncError> {
    let mut reader = BufReader::new(pem);
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| SyncError::InvalidData(e.to_string()))?
        .ok_or_else(|| SyncError::InvalidData("no private key found in key.pem".into()))
}

fn spki_sha256_from_cert(cert: &CertificateDer<'static>) -> Result<String, SyncError> {
    let parsed = rustls::server::ParsedCertificate::try_from(cert)
        .map_err(|e| SyncError::InvalidData(e.to_string()))?;
    let spki = parsed.subject_public_key_info();
    let digest = Sha256::digest(spki.as_ref());
    Ok(URL_SAFE_NO_PAD.encode(digest))
}

fn write_atomic_text(path: &PathBuf, content: &str) -> Result<(), SyncError> {
    let tmp = atomic_tmp_path(path);
    std::fs::write(&tmp, content).map_err(|e| SyncError::Io(e.to_string()))?;
    rename_overwrite(&tmp, path)
}

fn atomic_tmp_path(path: &Path) -> PathBuf {
    match path.extension() {
        Some(ext) if !ext.is_empty() => {
            let mut tmp_ext = ext.to_os_string();
            tmp_ext.push(".ttsync.tmp");
            path.with_extension(tmp_ext)
        }
        _ => path.with_extension("ttsync.tmp"),
    }
}

fn rename_overwrite(from: &Path, to: &Path) -> Result<(), SyncError> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(to);
            std::fs::rename(from, to).map_err(|e| SyncError::Io(e.to_string()))
        }
        Err(e) => Err(SyncError::Io(e.to_string())),
    }
}
