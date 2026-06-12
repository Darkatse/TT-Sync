use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer, SigningKey};
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::error::SyncError;

pub fn random_base64url(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn sha256_base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(bytes))
}

pub fn device_pubkey_b64url(ed25519_seed_b64url: &str) -> Result<String, SyncError> {
    let seed = decode_seed(ed25519_seed_b64url)?;
    let signing = SigningKey::from_bytes(&seed);
    Ok(URL_SAFE_NO_PAD.encode(signing.verifying_key().to_bytes()))
}

pub fn sign_ed25519_b64url(ed25519_seed_b64url: &str, message: &[u8]) -> Result<String, SyncError> {
    let seed = decode_seed(ed25519_seed_b64url)?;
    let signing = SigningKey::from_bytes(&seed);
    let signature: Signature = signing.sign(message);
    Ok(URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

fn decode_seed(ed25519_seed_b64url: &str) -> Result<[u8; 32], SyncError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(ed25519_seed_b64url.as_bytes())
        .map_err(|e| SyncError::InvalidData(e.to_string()))?;

    bytes
        .try_into()
        .map_err(|_| SyncError::InvalidData("ed25519 seed must be 32 bytes".into()))
}

#[cfg(test)]
mod tests {
    use super::{device_pubkey_b64url, sign_ed25519_b64url};

    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    #[test]
    fn seed_helpers_require_32_byte_seed() {
        let seed = URL_SAFE_NO_PAD.encode([7u8; 32]);
        assert!(device_pubkey_b64url(&seed).is_ok());
        assert!(sign_ed25519_b64url(&seed, b"message").is_ok());

        let short = URL_SAFE_NO_PAD.encode([7u8; 31]);
        assert!(device_pubkey_b64url(&short).is_err());
        assert!(sign_ed25519_b64url(&short, b"message").is_err());
    }
}
