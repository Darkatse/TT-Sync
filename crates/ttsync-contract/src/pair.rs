use serde::{Deserialize, Serialize};

use crate::peer::DeviceId;

/// Structured pair URI for v2 protocol.
///
/// Format: `tauritavern://tt-sync/pair?v=2&url=<url>&token=<token>&exp=<exp>&spki=<spki>`
#[derive(Debug, Clone)]
pub struct PairUri {
    pub url: String,
    pub token: String,
    pub expires_at_ms: u64,
    pub spki_sha256: String,
}

impl PairUri {
    /// Render as the canonical `tauritavern://` URI string.
    pub fn to_uri_string(&self) -> String {
        format!(
            "tauritavern://tt-sync/pair?v=2&url={}&token={}&exp={}&spki={}",
            urlencoded(&self.url),
            urlencoded(&self.token),
            self.expires_at_ms,
            urlencoded(&self.spki_sha256),
        )
    }

    pub fn parse_uri_string(input: &str) -> Result<Self, PairUriError> {
        let (base, query) = input.split_once('?').ok_or(PairUriError::InvalidFormat)?;
        if base != "tauritavern://tt-sync/pair" {
            return Err(PairUriError::InvalidFormat);
        }

        let mut version: Option<String> = None;
        let mut url: Option<String> = None;
        let mut token: Option<String> = None;
        let mut exp: Option<String> = None;
        let mut spki: Option<String> = None;

        for part in query.split('&') {
            let (key, value) = part.split_once('=').ok_or(PairUriError::InvalidFormat)?;
            let value = percent_decode(value)?;
            match key {
                "v" => version = Some(value),
                "url" => url = Some(value),
                "token" => token = Some(value),
                "exp" => exp = Some(value),
                "spki" => spki = Some(value),
                _ => {}
            }
        }

        if version.as_deref() != Some("2") {
            return Err(PairUriError::UnsupportedVersion);
        }

        let expires_at_ms = exp
            .ok_or(PairUriError::MissingField("exp"))?
            .parse::<u64>()
            .map_err(|_| PairUriError::InvalidExp)?;

        Ok(Self {
            url: url.ok_or(PairUriError::MissingField("url"))?,
            token: token.ok_or(PairUriError::MissingField("token"))?,
            expires_at_ms,
            spki_sha256: spki.ok_or(PairUriError::MissingField("spki"))?,
        })
    }
}

/// Request body for `POST /v2/pair/complete`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairCompleteRequest {
    pub device_id: DeviceId,
    pub device_name: String,
    /// Ed25519 public key, base64url encoded.
    pub device_pubkey: String,
}

/// Response body for `POST /v2/pair/complete`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairCompleteResponse {
    pub server_device_id: DeviceId,
    pub server_device_name: String,
    pub granted_permissions: crate::peer::Permissions,
}

#[derive(Debug, thiserror::Error)]
pub enum PairUriError {
    #[error("pair URI has invalid format")]
    InvalidFormat,
    #[error("pair URI is missing field: {0}")]
    MissingField(&'static str),
    #[error("pair URI has unsupported version")]
    UnsupportedVersion,
    #[error("pair URI has invalid exp")]
    InvalidExp,
    #[error("pair URI has invalid percent-encoding")]
    InvalidPercentEncoding,
}

/// Minimal percent-encoding for URI query values.
fn urlencoded(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push('%');
                result.push_str(&format!("{:02X}", byte));
            }
        }
    }
    result
}

fn percent_decode(value: &str) -> Result<String, PairUriError> {
    let mut out = Vec::with_capacity(value.len());
    let mut bytes = value.as_bytes().iter().copied();
    while let Some(b) = bytes.next() {
        match b {
            b'%' => {
                let hi = bytes.next().ok_or(PairUriError::InvalidPercentEncoding)?;
                let lo = bytes.next().ok_or(PairUriError::InvalidPercentEncoding)?;
                let hi = from_hex(hi).ok_or(PairUriError::InvalidPercentEncoding)?;
                let lo = from_hex(lo).ok_or(PairUriError::InvalidPercentEncoding)?;
                out.push((hi << 4) | lo);
            }
            _ => out.push(b),
        }
    }

    String::from_utf8(out).map_err(|_| PairUriError::InvalidPercentEncoding)
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::PairUri;

    #[test]
    fn pair_uri_renders_correctly() {
        let uri = PairUri {
            url: "https://sync.example.com:8443".to_owned(),
            token: "abc123".to_owned(),
            expires_at_ms: 1712345678901,
            spki_sha256: "dGVzdA".to_owned(),
        };
        let rendered = uri.to_uri_string();
        assert!(rendered.starts_with("tauritavern://tt-sync/pair?v=2&"));
        assert!(rendered.contains("token=abc123"));
        assert!(rendered.contains("spki=dGVzdA"));
    }

    #[test]
    fn pair_uri_parses_roundtrip() {
        let uri = PairUri {
            url: "https://sync.example.com:8443".to_owned(),
            token: "abc123".to_owned(),
            expires_at_ms: 1712345678901,
            spki_sha256: "dGVzdA".to_owned(),
        };
        let rendered = uri.to_uri_string();
        let parsed = PairUri::parse_uri_string(&rendered).unwrap();
        assert_eq!(parsed.url, uri.url);
        assert_eq!(parsed.token, uri.token);
        assert_eq!(parsed.expires_at_ms, uri.expires_at_ms);
        assert_eq!(parsed.spki_sha256, uri.spki_sha256);
    }
}
