use std::fmt;

use crate::peer::{DeviceId, DeviceIdError};

pub const CANONICAL_REQUEST_PREFIX: &str = "TT-SYNC-V2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalRequest {
    pub device_id: DeviceId,
    pub timestamp_ms: u64,
    pub nonce: String,
    pub method: String,
    pub path_and_query: String,
    pub body_sha256_base64url: String,
}

impl CanonicalRequest {
    pub fn new(
        device_id: DeviceId,
        timestamp_ms: u64,
        nonce: String,
        method: String,
        path_and_query: String,
        body_sha256_base64url: String,
    ) -> Result<Self, CanonicalRequestError> {
        validate_field("nonce", &nonce)?;
        validate_field("method", &method)?;
        validate_field("path_and_query", &path_and_query)?;
        validate_field("body_sha256_base64url", &body_sha256_base64url)?;

        Ok(Self {
            device_id,
            timestamp_ms,
            nonce,
            method,
            path_and_query,
            body_sha256_base64url,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }

    pub fn parse_bytes(input: &[u8]) -> Result<Self, CanonicalRequestError> {
        let text = std::str::from_utf8(input).map_err(|_| CanonicalRequestError::NonUtf8)?;
        Self::parse_str(text)
    }

    pub fn parse_str(input: &str) -> Result<Self, CanonicalRequestError> {
        let input = input.strip_suffix('\n').unwrap_or(input);
        let parts: Vec<&str> = input.split('\n').collect();
        if parts.len() != 7 {
            return Err(CanonicalRequestError::InvalidFormat);
        }

        if parts[0] != CANONICAL_REQUEST_PREFIX {
            return Err(CanonicalRequestError::InvalidPrefix);
        }

        let device_id =
            DeviceId::new(parts[1].to_owned()).map_err(CanonicalRequestError::DeviceId)?;
        let timestamp_ms = parts[2]
            .parse::<u64>()
            .map_err(|_| CanonicalRequestError::InvalidTimestamp)?;

        Self::new(
            device_id,
            timestamp_ms,
            parts[3].to_owned(),
            parts[4].to_owned(),
            parts[5].to_owned(),
            parts[6].to_owned(),
        )
    }
}

impl fmt::Display for CanonicalRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{prefix}\n{device_id}\n{timestamp}\n{nonce}\n{method}\n{path_and_query}\n{body_hash}",
            prefix = CANONICAL_REQUEST_PREFIX,
            device_id = self.device_id,
            timestamp = self.timestamp_ms,
            nonce = self.nonce,
            method = self.method,
            path_and_query = self.path_and_query,
            body_hash = self.body_sha256_base64url,
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CanonicalRequestError {
    #[error("canonical request is not UTF-8")]
    NonUtf8,

    #[error("canonical request has invalid format")]
    InvalidFormat,

    #[error("canonical request has invalid prefix")]
    InvalidPrefix,

    #[error("canonical request has invalid device id: {0}")]
    DeviceId(DeviceIdError),

    #[error("canonical request has invalid timestamp")]
    InvalidTimestamp,

    #[error("canonical request has invalid field {field}")]
    InvalidField { field: &'static str },
}

fn validate_field(field: &'static str, value: &str) -> Result<(), CanonicalRequestError> {
    if value.contains('\n') || value.contains('\r') {
        return Err(CanonicalRequestError::InvalidField { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CANONICAL_REQUEST_PREFIX, CanonicalRequest};
    use crate::peer::DeviceId;

    #[test]
    fn roundtrip_render_and_parse() {
        let req = CanonicalRequest::new(
            DeviceId::new("550e8400-e29b-41d4-a716-446655440000".into()).unwrap(),
            1712345678901,
            "nonce".into(),
            "POST".into(),
            "/v2/session/open".into(),
            "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU".into(),
        )
        .unwrap();

        let text = req.to_string();
        assert!(text.starts_with(CANONICAL_REQUEST_PREFIX));

        let parsed = CanonicalRequest::parse_str(&text).unwrap();
        assert_eq!(parsed, req);
    }
}
