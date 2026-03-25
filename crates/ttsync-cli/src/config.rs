//! Configuration management: `config.toml` and `identity.json`.

use std::path::{Path, PathBuf};

use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use ttsync_fs::layout::LayoutMode;

/// Persistent configuration written to `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// User-provided folder path used as the layout anchor.
    pub workspace_path: PathBuf,
    #[serde(default)]
    pub layout: LayoutMode,
    /// Public base URL for pair URIs (e.g., https://my-vps:8443).
    pub public_url: String,
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default)]
    pub ui: UiConfig,
}

fn default_listen() -> String {
    "0.0.0.0:8443".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub language: UiLanguage,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            language: UiLanguage::ZhCn,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum UiLanguage {
    ZhCn,
    En,
}

impl Default for UiLanguage {
    fn default() -> Self {
        Self::ZhCn
    }
}

/// Device identity persisted in `identity.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub device_id: String,
    pub device_name: String,
    /// Ed25519 private key, base64url-encoded.
    pub private_key: String,
}

/// Resolve the state directory path.
pub fn state_dir(override_dir: Option<&Path>) -> PathBuf {
    if let Some(dir) = override_dir {
        return dir.to_path_buf();
    }
    if let Ok(dir) = std::env::var("TT_SYNC_STATE_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tt-sync")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigPathMode {
    Tui,
    Cli,
}

pub fn default_config_path() -> Result<PathBuf, CliError> {
    let exe = std::env::current_exe().map_err(|e| CliError::Config(e.to_string()))?;
    let dir = exe
        .parent()
        .ok_or_else(|| CliError::Config("current_exe has no parent directory".into()))?;
    Ok(dir.join("config.toml"))
}

pub fn resolve_config_path(
    default_path: PathBuf,
    override_path: Option<&Path>,
    mode: ConfigPathMode,
) -> PathBuf {
    match (mode, override_path) {
        (ConfigPathMode::Cli, Some(path)) => path.to_path_buf(),
        _ => default_path,
    }
}

pub fn identity_path(state_dir: &Path) -> PathBuf {
    state_dir.join("identity.json")
}

pub fn load_config(config_path: &Path) -> Result<Config, CliError> {
    let text = std::fs::read_to_string(config_path)
        .map_err(|e| CliError::Config(format!("read {}: {}", config_path.display(), e)))?;
    toml::from_str(&text)
        .map_err(|e| CliError::Config(format!("parse {}: {}", config_path.display(), e)))
}

pub fn save_config(config_path: &Path, config: &Config) -> Result<(), CliError> {
    let dir = config_path
        .parent()
        .ok_or_else(|| CliError::Config("config path has no parent directory".into()))?;
    std::fs::create_dir_all(dir).map_err(|e| CliError::Config(e.to_string()))?;
    let text = toml::to_string_pretty(config).map_err(|e| CliError::Config(e.to_string()))?;
    std::fs::write(config_path, text).map_err(|e| CliError::Config(e.to_string()))
}

pub fn load_identity(state_dir: &Path) -> Result<Identity, CliError> {
    let path = identity_path(state_dir);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| CliError::Config(format!("read {}: {}", path.display(), e)))?;
    serde_json::from_str(&text)
        .map_err(|e| CliError::Config(format!("parse {}: {}", path.display(), e)))
}

pub fn save_identity(state_dir: &Path, identity: &Identity) -> Result<(), CliError> {
    std::fs::create_dir_all(state_dir).map_err(|e| CliError::Config(e.to_string()))?;
    let text =
        serde_json::to_string_pretty(identity).map_err(|e| CliError::Config(e.to_string()))?;
    std::fs::write(identity_path(state_dir), text).map_err(|e| CliError::Config(e.to_string()))
}

pub fn load_or_create_identity(state_dir: &Path) -> Result<Identity, CliError> {
    if identity_path(state_dir).exists() {
        return load_identity(state_dir);
    }
    let secret: [u8; 32] = rand::random();
    let signing = SigningKey::from_bytes(&secret);
    let private_key = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signing.to_bytes());
    let identity = Identity {
        device_id: uuid::Uuid::new_v4().to_string(),
        device_name: "TT-Sync".into(),
        private_key,
    };
    save_identity(state_dir, &identity)?;
    Ok(identity)
}

use base64::Engine;

pub fn _signing_key(identity: &Identity) -> Result<SigningKey, CliError> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&identity.private_key)
        .map_err(|e| CliError::Config(format!("decode private key: {}", e)))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CliError::Config("invalid private key length".into()))?;
    Ok(SigningKey::from_bytes(&bytes))
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Sync(#[from] ttsync_core::error::SyncError),
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_mode_uses_override_path() {
        let resolved = resolve_config_path(
            PathBuf::from("config.toml"),
            Some(Path::new("custom.toml")),
            ConfigPathMode::Cli,
        );

        assert_eq!(resolved, PathBuf::from("custom.toml"));
    }

    #[test]
    fn tui_mode_ignores_override_path() {
        let resolved = resolve_config_path(
            PathBuf::from("config.toml"),
            Some(Path::new("custom.toml")),
            ConfigPathMode::Tui,
        );

        assert_eq!(resolved, PathBuf::from("config.toml"));
    }
}
