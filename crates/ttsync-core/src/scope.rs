//! Compatibility view for the original fixed TT-Sync v2 scope.
//!
//! New code should use `crate::dataset::ResolvedDatasetPolicy` so scan,
//! validation, and mirror-delete boundaries share the same selection.

use std::sync::OnceLock;

use crate::dataset::ResolvedDatasetPolicy;

static LEGACY_POLICY: OnceLock<ResolvedDatasetPolicy> = OnceLock::new();

pub fn included_directories() -> &'static [&'static str] {
    LEGACY_POLICY
        .get_or_init(ResolvedDatasetPolicy::legacy_v2)
        .scan_roots()
}

pub fn included_files() -> &'static [&'static str] {
    LEGACY_POLICY
        .get_or_init(ResolvedDatasetPolicy::legacy_v2)
        .files()
}

pub fn is_excluded(relative_path: &str) -> bool {
    crate::dataset::is_excluded(relative_path)
}

pub fn is_in_scope(relative_path: &str) -> bool {
    LEGACY_POLICY
        .get_or_init(ResolvedDatasetPolicy::legacy_v2)
        .contains_path(relative_path)
}
