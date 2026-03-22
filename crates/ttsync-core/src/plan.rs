use std::collections::HashMap;

use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::plan::{PlanId, SyncPlan};
use ttsync_contract::sync::SyncMode;

/// Compute a synchronization plan by diffing source and target manifests.
///
/// - `source`: the manifest of the authoritative side (the data to replicate from).
/// - `target`: the manifest of the receiving side (the data to replicate to).
/// - `mode`: whether to include deletions for files missing on the source side.
///
/// Returns a plan describing which files to transfer and (if mirror mode) which to delete.
pub fn compute_plan(
    plan_id: PlanId,
    source: &ManifestV2,
    target: &ManifestV2,
    mode: SyncMode,
) -> SyncPlan {
    let source_index: HashMap<&str, ()> = source
        .entries
        .iter()
        .map(|e| (e.path.as_str(), ()))
        .collect();

    let target_index: HashMap<&str, (u64, u64)> = target
        .entries
        .iter()
        .map(|e| (e.path.as_str(), (e.size_bytes, e.modified_ms)))
        .collect();

    let mut transfer = Vec::new();
    let mut bytes_total = 0u64;

    for entry in &source.entries {
        let unchanged = target_index
            .get(entry.path.as_str())
            .is_some_and(|&(size, mtime)| size == entry.size_bytes && mtime == entry.modified_ms);

        if !unchanged {
            bytes_total += entry.size_bytes;
            transfer.push(entry.clone());
        }
    }

    let delete = if mode == SyncMode::Mirror {
        target
            .entries
            .iter()
            .filter(|e| !source_index.contains_key(e.path.as_str()))
            .map(|e| e.path.clone())
            .collect()
    } else {
        Vec::new()
    };

    let files_total = transfer.len();

    SyncPlan {
        plan_id,
        transfer,
        delete,
        files_total,
        bytes_total,
    }
}

#[cfg(test)]
mod tests {
    use ttsync_contract::manifest::{ManifestEntryV2, ManifestV2};
    use ttsync_contract::path::SyncPath;
    use ttsync_contract::plan::PlanId;
    use ttsync_contract::sync::SyncMode;

    use super::compute_plan;

    fn entry(path: &str, size: u64, mtime: u64) -> ManifestEntryV2 {
        ManifestEntryV2 {
            path: SyncPath::new(path).unwrap(),
            size_bytes: size,
            modified_ms: mtime,
            content_hash: None,
        }
    }

    #[test]
    fn incremental_skips_unchanged_files() {
        let source = ManifestV2 {
            entries: vec![
                entry("default-user/a.json", 100, 1000),
                entry("default-user/b.json", 200, 2000),
            ],
        };
        let target = ManifestV2 {
            entries: vec![entry("default-user/a.json", 100, 1000)],
        };

        let plan = compute_plan(
            PlanId("test".into()),
            &source,
            &target,
            SyncMode::Incremental,
        );
        assert_eq!(plan.files_total, 1);
        assert_eq!(plan.transfer[0].path.as_str(), "default-user/b.json");
        assert!(plan.delete.is_empty());
    }

    #[test]
    fn mirror_includes_deletions() {
        let source = ManifestV2 {
            entries: vec![entry("default-user/a.json", 100, 1000)],
        };
        let target = ManifestV2 {
            entries: vec![
                entry("default-user/a.json", 100, 1000),
                entry("default-user/old.json", 50, 500),
            ],
        };

        let plan = compute_plan(
            PlanId("test".into()),
            &source,
            &target,
            SyncMode::Mirror,
        );
        assert_eq!(plan.files_total, 0);
        assert_eq!(plan.delete.len(), 1);
        assert_eq!(plan.delete[0].as_str(), "default-user/old.json");
    }
}
