//! Dataset scope definition: which wire paths are included/excluded in TT-Sync v2.

/// Directories included in the v2 default dataset.
pub const DIRECTORIES: &[&str] = &[
    "default-user/chats",
    "default-user/characters",
    "default-user/groups",
    "default-user/group chats",
    "default-user/worlds",
    "default-user/backgrounds",
    "default-user/themes",
    "default-user/user",
    "default-user/User Avatars",
    "default-user/OpenAI Settings",
    "default-user/NovelAI Settings",
    "default-user/TextGen Settings",
    "default-user/KoboldAI Settings",
    "default-user/instruct",
    "default-user/context",
    "default-user/QuickReplies",
    "default-user/assets",
    "default-user/extensions",
    "extensions/third-party",
    "_tauritavern/extension-sources/local",
    "_tauritavern/extension-sources/global",
];

/// Individual files included in the v2 default dataset.
pub const FILES: &[&str] = &[
    "default-user/settings.json",
    "default-user/secrets.json",
    "default-user/tauritavern-settings.json",
    "default-user/image-metadata.json",
];

/// Paths excluded from the dataset.
pub const EXCLUSIONS: &[&str] = &["default-user/user/lan-sync"];

pub fn included_directories() -> &'static [&'static str] {
    DIRECTORIES
}

pub fn included_files() -> &'static [&'static str] {
    FILES
}

pub fn is_excluded(relative_path: &str) -> bool {
    EXCLUSIONS.iter().any(|excluded| {
        relative_path == *excluded
            || relative_path
                .strip_prefix(excluded)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

pub fn is_in_scope(relative_path: &str) -> bool {
    if is_excluded(relative_path) {
        return false;
    }

    if FILES.contains(&relative_path) {
        return true;
    }

    DIRECTORIES.iter().any(|dir| {
        relative_path
            .strip_prefix(dir)
            .is_some_and(|suffix| suffix.starts_with('/'))
    })
}
