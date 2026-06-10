pub(super) fn is_globally_excluded(relative_path: &str) -> bool {
    GLOBAL_EXCLUDED_PREFIXES
        .iter()
        .any(|prefix| is_same_or_under(relative_path, prefix))
        || relative_path.split('/').any(is_excluded_component)
}

pub fn is_excluded(relative_path: &str) -> bool {
    is_globally_excluded(relative_path)
}

pub(super) fn is_same_or_under(path: &str, prefix: &str) -> bool {
    path == prefix || is_under(path, prefix)
}

pub(super) fn is_under(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix)
        .is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_excluded_component(component: &str) -> bool {
    component == ".staging" || component.starts_with(".tmp-") || component.ends_with(".ttsync.tmp")
}

const GLOBAL_EXCLUDED_PREFIXES: &[&str] = &[
    "default-user/user/lan-sync",
    "_tauritavern/prompt-cache",
    "_tauritavern/.ios-policy.json",
    "_cache",
];
