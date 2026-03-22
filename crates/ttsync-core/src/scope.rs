//! Scope profile definitions: which paths are included/excluded per profile.

use ttsync_contract::sync::ScopeProfileId;

/// Directories included in the `CompatibleMinimal` profile (v1 whitelist equivalent).
pub const COMPATIBLE_MINIMAL_DIRECTORIES: &[&str] = &[
    "default-user/chats",
    "default-user/characters",
    "default-user/groups",
    "default-user/group chats",
    "default-user/worlds",
    "default-user/themes",
    "default-user/user",
    "default-user/User Avatars",
    "default-user/OpenAI Settings",
    "default-user/extensions",
    "extensions/third-party",
    "_tauritavern/extension-sources/local",
    "_tauritavern/extension-sources/global",
];

/// Files included in the `CompatibleMinimal` profile.
pub const COMPATIBLE_MINIMAL_FILES: &[&str] = &["default-user/settings.json"];

/// Directories included in the `Default` (tauritavern-user) profile.
pub const DEFAULT_DIRECTORIES: &[&str] = &[
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

/// Files included in the `Default` profile.
pub const DEFAULT_FILES: &[&str] = &["default-user/settings.json"];

/// Paths excluded from all profiles.
pub const GLOBAL_EXCLUSIONS: &[&str] = &[
    "default-user/user/lan-sync",
    "secrets.json",
];

/// Returns the included directories for a given scope profile.
pub fn included_directories(profile: &ScopeProfileId) -> &'static [&'static str] {
    match profile {
        ScopeProfileId::CompatibleMinimal => COMPATIBLE_MINIMAL_DIRECTORIES,
        ScopeProfileId::Default => DEFAULT_DIRECTORIES,
    }
}

/// Returns the included individual files for a given scope profile.
pub fn included_files(profile: &ScopeProfileId) -> &'static [&'static str] {
    match profile {
        ScopeProfileId::CompatibleMinimal => COMPATIBLE_MINIMAL_FILES,
        ScopeProfileId::Default => DEFAULT_FILES,
    }
}

/// Returns true if the given relative path is globally excluded.
pub fn is_excluded(relative_path: &str) -> bool {
    GLOBAL_EXCLUSIONS.iter().any(|excluded| {
        relative_path == *excluded
            || relative_path
                .strip_prefix(excluded)
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

/// Returns true if the path falls within one of the scoped directories for the profile.
pub fn is_in_scope(relative_path: &str, profile: &ScopeProfileId) -> bool {
    if is_excluded(relative_path) {
        return false;
    }

    if included_files(profile).contains(&relative_path) {
        return true;
    }

    included_directories(profile).iter().any(|dir| {
        relative_path
            .strip_prefix(dir)
            .is_some_and(|suffix| suffix.starts_with('/'))
    })
}
