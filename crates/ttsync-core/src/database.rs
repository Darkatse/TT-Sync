//! Database files form one replacement unit per namespace.

pub const DATASET_ID: &str = "extensions.databases";
pub const ROOT: &str = "_tauritavern/databases";

pub fn namespace_directory(path: &str) -> Option<&str> {
    let relative = path.strip_prefix(ROOT)?.strip_prefix('/')?;
    let (directory, file) = relative.split_once('/')?;
    let namespace = directory.strip_prefix("db-")?;
    if namespace.is_empty()
        || namespace.len() > 128
        || !namespace
            .bytes()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, b'-' | b'_'))
        || !(file == "database.tdb" || file.starts_with("database.tdb."))
        || file.contains('/')
        || file == "database.tdb.lock"
        || file.ends_with(".tmp")
    {
        return None;
    }
    Some(&path[..ROOT.len() + 1 + directory.len()])
}
