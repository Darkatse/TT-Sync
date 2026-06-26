use std::collections::{HashMap, HashSet};

use ttsync_contract::dataset::DatasetSelection;
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
fn compute_plan(
    plan_id: PlanId,
    source: &ManifestV2,
    target: &ManifestV2,
    mode: SyncMode,
    selection: DatasetSelection,
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
        selection,
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

    let mut plan = compute_plan(plan_id, source, target, mode, policy.selection().clone());
    if mode == SyncMode::Mirror {
        plan.delete
            .retain(|path| policy.allows_delete(path.as_str()));
    }
    validate_plan_scope(&plan, policy)?;
    Ok(plan)
}

pub fn validate_plan_scope(
    plan: &SyncPlan,
    policy: &ResolvedDatasetPolicy,
) -> Result<(), SyncError> {
    if &plan.selection != policy.selection() {
        return Err(SyncError::InvalidData(
            "sync plan dataset selection does not match the requested policy".to_owned(),
        ));
    }

    if plan.files_total != plan.transfer.len() {
        return Err(SyncError::InvalidData(format!(
            "sync plan files_total mismatch: expected {}, got {}",
            plan.transfer.len(),
            plan.files_total
        )));
    }

    let mut bytes_total = 0u64;
    let mut transfer_paths = HashSet::new();
    for entry in &plan.transfer {
        if !transfer_paths.insert(entry.path.as_str()) {
            return Err(SyncError::InvalidData(format!(
                "sync plan contains duplicate transfer path: {}",
                entry.path
            )));
        }
        bytes_total = bytes_total
            .checked_add(entry.size_bytes)
            .ok_or_else(|| SyncError::InvalidData("sync plan bytes_total overflow".into()))?;
        if !policy.contains_path(entry.path.as_str()) {
            return Err(SyncError::InvalidData(format!(
                "sync plan contains transfer outside selected dataset scope: {}",
                entry.path
            )));
        }
    }

    if plan.bytes_total != bytes_total {
        return Err(SyncError::InvalidData(format!(
            "sync plan bytes_total mismatch: expected {}, got {}",
            bytes_total, plan.bytes_total
        )));
    }

    let mut delete_paths = HashSet::new();
    for path in &plan.delete {
        if !delete_paths.insert(path.as_str()) {
            return Err(SyncError::InvalidData(format!(
                "sync plan contains duplicate delete path: {}",
                path
            )));
        }
        if transfer_paths.contains(path.as_str()) {
            return Err(SyncError::InvalidData(format!(
                "sync plan contains path in both transfer and delete: {}",
                path
            )));
        }
        if !policy.allows_delete(path.as_str()) {
            return Err(SyncError::InvalidData(format!(
                "sync plan contains delete outside selected dataset scope: {}",
                path
            )));
        }
    }

    Ok(())
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
    use ttsync_contract::plan::{PlanId, SyncPlan};
    use ttsync_contract::sync::SyncMode;

    use ttsync_contract::dataset::DatasetSelection;

    use crate::dataset::ResolvedDatasetPolicy;

    use super::{compute_plan, compute_plan_for_policy, validate_plan_scope};

    fn entry(path: &str, size: u64, mtime: u64) -> ManifestEntryV2 {
        ManifestEntryV2 {
            path: SyncPath::new(path).unwrap(),
            size_bytes: size,
            modified_ms: mtime,
            content_hash: None,
        }
    }

    fn dataset_selection(ids: &[&str]) -> DatasetSelection {
        DatasetSelection::new(
            ttsync_contract::dataset::DATASET_POLICY_VERSION,
            ids.iter().map(|id| (*id).to_owned()).collect(),
        )
    }

    #[test]
    fn incremental_skips_unchanged_files() {
        let source = ManifestV2 {
            entries: vec![
                entry("default-user/chats/alice/a.jsonl", 100, 1000),
                entry("default-user/chats/alice/b.jsonl", 200, 2000),
            ],
        };
        let target = ManifestV2 {
            entries: vec![entry("default-user/chats/alice/a.jsonl", 100, 1000)],
        };

        let plan = compute_plan(
            PlanId("test".into()),
            &source,
            &target,
            SyncMode::Incremental,
            dataset_selection(&["chat.character.history"]),
        );
        assert_eq!(plan.files_total, 1);
        assert_eq!(
            plan.transfer[0].path.as_str(),
            "default-user/chats/alice/b.jsonl"
        );
        assert!(plan.delete.is_empty());
    }

    #[test]
    fn mirror_includes_deletions() {
        let source = ManifestV2 {
            entries: vec![entry("default-user/chats/alice/a.jsonl", 100, 1000)],
        };
        let target = ManifestV2 {
            entries: vec![
                entry("default-user/chats/alice/a.jsonl", 100, 1000),
                entry("default-user/chats/alice/old.jsonl", 50, 500),
            ],
        };

        let plan = compute_plan(
            PlanId("test".into()),
            &source,
            &target,
            SyncMode::Mirror,
            dataset_selection(&["chat.character.history"]),
        );
        assert_eq!(plan.files_total, 0);
        assert_eq!(plan.delete.len(), 1);
        assert_eq!(
            plan.delete[0].as_str(),
            "default-user/chats/alice/old.jsonl"
        );
    }

    #[test]
    fn policy_plan_rejects_manifest_entries_outside_selection() {
        let selection = dataset_selection(&["chat.character.history"]);
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
        let selection = dataset_selection(&["chat.character.history"]);
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
        assert_eq!(&plan.selection, policy.selection());
    }

    #[test]
    fn plan_scope_validation_rejects_mismatched_policy_selection() {
        let selection = dataset_selection(&["chat.character.history"]);
        let policy = ResolvedDatasetPolicy::from_selection(&selection).unwrap();
        let plan = SyncPlan {
            plan_id: PlanId("test".into()),
            selection: dataset_selection(&["media.backgrounds"]),
            transfer: vec![entry("default-user/chats/alice/chat.jsonl", 1, 1)],
            delete: vec![],
            files_total: 1,
            bytes_total: 1,
        };

        assert!(validate_plan_scope(&plan, &policy).is_err());
    }

    #[test]
    fn plan_scope_validation_rejects_duplicate_paths_and_bad_totals() {
        let selection = dataset_selection(&["chat.character.history"]);
        let policy = ResolvedDatasetPolicy::from_selection(&selection).unwrap();
        let mut plan = SyncPlan {
            plan_id: PlanId("test".into()),
            selection,
            transfer: vec![
                entry("default-user/chats/alice/chat.jsonl", 1, 1),
                entry("default-user/chats/alice/chat.jsonl", 1, 1),
            ],
            delete: vec![],
            files_total: 2,
            bytes_total: 2,
        };

        assert!(validate_plan_scope(&plan, &policy).is_err());

        plan.transfer.pop();
        plan.files_total = 2;
        plan.bytes_total = 1;
        assert!(validate_plan_scope(&plan, &policy).is_err());

        plan.files_total = 1;
        plan.bytes_total = 2;
        assert!(validate_plan_scope(&plan, &policy).is_err());
    }
}
