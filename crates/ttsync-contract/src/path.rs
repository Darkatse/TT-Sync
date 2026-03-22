use std::fmt;

use serde::{Deserialize, Serialize};

/// A validated, data-root-relative file path used on the wire.
///
/// Invariants enforced at construction:
/// - UTF-8
/// - Forward-slash separated (no backslashes)
/// - No leading `/`
/// - No empty segments, `.`, or `..`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SyncPath(String);

impl SyncPath {
    pub fn new(raw: impl Into<String>) -> Result<Self, SyncPathError> {
        let value = raw.into();
        validate_sync_path(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for SyncPath {
    type Error = SyncPathError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<SyncPath> for String {
    fn from(path: SyncPath) -> Self {
        path.0
    }
}

impl fmt::Display for SyncPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SyncPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SyncPathError {
    #[error("path is empty")]
    Empty,
    #[error("path must not start with '/'")]
    LeadingSlash,
    #[error("path must use '/' separators, found backslash")]
    Backslash,
    #[error("path contains invalid component: {0:?}")]
    InvalidComponent(String),
}

fn validate_sync_path(value: &str) -> Result<(), SyncPathError> {
    if value.is_empty() {
        return Err(SyncPathError::Empty);
    }
    if value.starts_with('/') {
        return Err(SyncPathError::LeadingSlash);
    }
    if value.contains('\\') {
        return Err(SyncPathError::Backslash);
    }
    for segment in value.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(SyncPathError::InvalidComponent(segment.to_owned()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::SyncPath;

    #[test]
    fn accepts_valid_paths() {
        assert!(SyncPath::new("default-user/characters/Alice.json").is_ok());
        assert!(SyncPath::new("extensions/third-party/ext/index.js").is_ok());
        assert!(SyncPath::new("settings.json").is_ok());
    }

    #[test]
    fn rejects_invalid_paths() {
        assert!(SyncPath::new("").is_err());
        assert!(SyncPath::new("/absolute").is_err());
        assert!(SyncPath::new("back\\slash").is_err());
        assert!(SyncPath::new("foo/../bar").is_err());
        assert!(SyncPath::new("foo/./bar").is_err());
        assert!(SyncPath::new("foo//bar").is_err());
    }
}
