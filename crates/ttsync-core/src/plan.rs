use std::collections::HashMap;

use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::plan::{PlanId, SyncPlan};
use ttsync_contract::sync::SyncMode;

use crate::dataset::ResolvedDatasetPolicy;
use crate::error::SyncError;

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
        selection: None,
        transfer,
        delete,
        files_total,
        bytes_total,
    }
}

pub fn compute_plan_for_policy(
    plan_id: PlanId,
    source: &ManifestV2,
    target: &ManifestV2,
    mode: SyncMode,
    policy: &ResolvedDatasetPolicy,
) -> Result<SyncPlan, SyncError> {
    validate_manifest_scope(source, policy)?;
    validate_manifest_scope(target, policy)?;

    let mut plan = compute_plan(plan_id, source, target, mode);
    if mode == SyncMode::Mirror {
        plan.delete
            .retain(|path| policy.allows_delete(path.as_str()));
    }
    plan.selection = Some(policy.selection().clone());
    Ok(plan)
}

fn validate_manifest_scope(
    manifest: &ManifestV2,
    policy: &ResolvedDatasetPolicy,
) -> Result<(), SyncError> {
    for entry in &manifest.entries {
        if !policy.contains_path(entry.path.as_str()) {
            return Err(SyncError::InvalidData(format!(
                "manifest path outside selected dataset scope: {}",
                entry.path
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ttsync_contract::manifest::{ManifestEntryV2, ManifestV2};
    use ttsync_contract::path::SyncPath;
    use ttsync_contract::plan::PlanId;
    use ttsync_contract::sync::SyncMode;

    use ttsync_contract::dataset::DatasetSelection;

    use crate::dataset::ResolvedDatasetPolicy;

    use super::{compute_plan, compute_plan_for_policy};

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

        let plan = compute_plan(PlanId("test".into()), &source, &target, SyncMode::Mirror);
        assert_eq!(plan.files_total, 0);
        assert_eq!(plan.delete.len(), 1);
        assert_eq!(plan.delete[0].as_str(), "default-user/old.json");
    }

    #[test]
    fn policy_plan_rejects_manifest_entries_outside_selection() {
        let selection = DatasetSelection::new(
            ttsync_contract::dataset::DATASET_POLICY_VERSION,
            vec!["chat.character.history".to_owned()],
        );
        let policy = ResolvedDatasetPolicy::from_selection(&selection).unwrap();
        let source = ManifestV2 { entries: vec![] };
        let target = ManifestV2 {
            entries: vec![entry("default-user/backgrounds/bg.png", 1, 1)],
        };

        let result = compute_plan_for_policy(
            PlanId("test".into()),
            &source,
            &target,
            SyncMode::Mirror,
            &policy,
        );

        assert!(result.is_err());
    }

    #[test]
    fn policy_plan_deletes_only_selected_scope_entries() {
        let selection = DatasetSelection::new(
            ttsync_contract::dataset::DATASET_POLICY_VERSION,
            vec!["chat.character.history".to_owned()],
        );
        let policy = ResolvedDatasetPolicy::from_selection(&selection).unwrap();
        let source = ManifestV2 { entries: vec![] };
        let target = ManifestV2 {
            entries: vec![entry("default-user/chats/alice/old.jsonl", 1, 1)],
        };

        let plan = compute_plan_for_policy(
            PlanId("test".into()),
            &source,
            &target,
            SyncMode::Mirror,
            &policy,
        )
        .unwrap();

        assert_eq!(plan.delete.len(), 1);
        assert_eq!(
            plan.delete[0].as_str(),
            "default-user/chats/alice/old.jsonl"
        );
        assert!(plan.selection.is_some());
    }
}
