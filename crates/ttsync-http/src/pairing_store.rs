use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use ttsync_contract::peer::Permissions;
use ttsync_core::error::SyncError;
use ttsync_core::pairing::{PairingConfig, PairingSession};

#[derive(Debug, Clone)]
pub struct PairingTokenStore {
    dir: PathBuf,
}

impl PairingTokenStore {
    pub fn from_state_dir(state_dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: state_dir.into().join("pairing-tokens"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn insert(&self, session: &PairingSession) -> Result<(), SyncError> {
        validate_token(&session.token)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| SyncError::Io(e.to_string()))?;

        let path = self.token_path(&session.token);
        let file = TokenFile {
            expires_at_ms: session.expires_at_ms,
            permissions: session.config.permissions,
        };
        write_atomic_json(&path, &file)?;
        Ok(())
    }

    pub fn remove(&self, token: &str) -> Result<(), SyncError> {
        validate_token(token)?;
        let path = self.token_path(token);
        std::fs::remove_file(&path).map_err(|e| SyncError::Io(e.to_string()))
    }

    pub fn take(&self, token: &str, now_ms: u64) -> Result<PairingSession, SyncError> {
        validate_token(token)?;

        let path = self.token_path(token);
        let taken = self.dir.join(format!(
            "{}.taken.{}.json",
            token,
            Uuid::new_v4().to_string()
        ));

        std::fs::rename(&path, &taken).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SyncError::Unauthorized("invalid pair token".into())
            } else {
                SyncError::Io(e.to_string())
            }
        })?;

        let bytes = std::fs::read(&taken).map_err(|e| SyncError::Io(e.to_string()))?;
        let file = serde_json::from_slice::<TokenFile>(&bytes)
            .map_err(|e| SyncError::InvalidData(e.to_string()))?;
        let _ = std::fs::remove_file(&taken);

        if now_ms > file.expires_at_ms {
            return Err(SyncError::Unauthorized("pair token expired".into()));
        }

        Ok(PairingSession {
            token: token.to_owned(),
            config: PairingConfig {
                permissions: file.permissions,
                expires_in_secs: 0,
            },
            expires_at_ms: file.expires_at_ms,
        })
    }

    fn token_path(&self, token: &str) -> PathBuf {
        self.dir.join(format!("{token}.json"))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct TokenFile {
    expires_at_ms: u64,
    permissions: Permissions,
}

fn validate_token(token: &str) -> Result<(), SyncError> {
    if token.is_empty() {
        return Err(SyncError::InvalidData("empty pair token".into()));
    }
    if token.len() > 128 {
        return Err(SyncError::InvalidData("pair token too long".into()));
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(SyncError::InvalidData("invalid pair token".into()));
    }
    Ok(())
}

fn write_atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<(), SyncError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| SyncError::Io(e.to_string()))?;
    }

    let bytes = serde_json::to_vec_pretty(value).map_err(|e| SyncError::Io(e.to_string()))?;
    let tmp = path.with_extension("ttsync.tmp");

    std::fs::write(&tmp, bytes).map_err(|e| SyncError::Io(e.to_string()))?;

    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = std::fs::remove_file(path);
            std::fs::rename(&tmp, path).map_err(|e| SyncError::Io(e.to_string()))
        }
        Err(e) => Err(SyncError::Io(e.to_string())),
    }
}
