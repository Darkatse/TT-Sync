//! Shared fixtures for the settings translation and persona bridging tests.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::AsyncReadExt;
use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DatasetSelection};
use ttsync_contract::path::SyncPath;
use ttsync_core::dataset::ResolvedDatasetPolicy;
use ttsync_core::ports::ManifestStore;

use crate::layout::WorkspaceMounts;
use crate::persona_bridge::{self, PersonaCard};

pub(crate) const AVATAR_WIRE_PATH: &str = "default-user/User Avatars/p1.png";

pub(crate) fn unique_temp_root() -> PathBuf {
    // The Windows system clock is coarse (~15 ms), so a timestamp alone can
    // collide between tests launched in parallel; pair it with a process-wide
    // sequence number so temp roots never alias.
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("ttsync-fs-test-{now}-{sequence}"))
}

pub(crate) fn mounts(root: &std::path::Path) -> WorkspaceMounts {
    WorkspaceMounts {
        data_root: root.to_path_buf(),
        default_user_root: root.join("default-user"),
        extensions_root: root.join("extensions").join("third-party"),
    }
}

pub(crate) fn settings_policy() -> ResolvedDatasetPolicy {
    ResolvedDatasetPolicy::from_selection(&DatasetSelection::new(
        DATASET_POLICY_VERSION,
        vec![
            "settings.core".to_owned(),
            "settings.appearance".to_owned(),
            "settings.presets".to_owned(),
            "settings.layout".to_owned(),
        ],
    ))
    .expect("valid settings policy")
}

pub(crate) fn avatar_policy() -> ResolvedDatasetPolicy {
    ResolvedDatasetPolicy::from_selection(&DatasetSelection::new(
        DATASET_POLICY_VERSION,
        vec!["character.avatars".to_owned(), "settings.core".to_owned()],
    ))
    .expect("valid avatar policy")
}

pub(crate) fn avatar_disk_path(root: &std::path::Path) -> PathBuf {
    root.join("default-user/User Avatars/p1.png")
}

pub(crate) fn future_ms(seconds: u64) -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64
        + seconds * 1000
}

pub(crate) async fn read_bytes<M: ManifestStore>(store: &M, path: &str) -> Vec<u8> {
    let mut reader = store
        .read_file(&SyncPath::new(path.to_owned()).expect("valid path"))
        .await
        .expect("read file");
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await.expect("read to end");
    bytes
}

/// Minimal structurally-valid PNG (fake CRCs are fine: chunk walking never
/// verifies them). Persona injection only needs IHDR/IDAT/IEND framing.
pub(crate) fn plain_png() -> Vec<u8> {
    fn empty_chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut raw = Vec::new();
        raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
        raw.extend_from_slice(kind);
        raw.extend_from_slice(body);
        raw.extend_from_slice(&0u32.to_be_bytes());
        raw
    }
    let mut data = vec![137u8, 80, 78, 71, 13, 10, 26, 10];
    data.extend(empty_chunk(b"IHDR", &[0u8; 13]));
    data.extend(empty_chunk(b"IDAT", b"pixels"));
    data.extend(empty_chunk(b"IEND", &[]));
    data
}

pub(crate) fn png_with_card(name: &str, description: &str) -> Vec<u8> {
    let card = PersonaCard::new(
        Some(name.to_owned()),
        Some(serde_json::json!({ "description": description, "position": 0 })),
    );
    persona_bridge::inject_persona(&plain_png(), &card).expect("inject card")
}
