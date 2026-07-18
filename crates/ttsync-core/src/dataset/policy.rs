use std::collections::BTreeSet;

use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DatasetSelection};

use crate::dataset::catalog::{DATASETS, DatasetRule, dataset_definition};
use crate::dataset::path::{is_globally_excluded, is_same_or_under, is_under};
use crate::dataset::profile::{expand_dataset_ids, tauri_tavern_default_selection};
use crate::error::SyncError;

#[derive(Debug, Clone)]
pub struct ResolvedDatasetPolicy {
    selection: DatasetSelection,
    scan_roots: Vec<&'static str>,
    files: Vec<&'static str>,
    rules: Vec<DatasetRule>,
}

impl ResolvedDatasetPolicy {
    pub fn from_selection(selection: &DatasetSelection) -> Result<Self, SyncError> {
        if selection.policy_version != DATASET_POLICY_VERSION {
            return Err(SyncError::InvalidData(format!(
                "unsupported dataset policy version: {}",
                selection.policy_version
            )));
        }

        let dataset_ids = expand_dataset_ids(&selection.dataset_ids)?;
        if dataset_ids.is_empty() {
            return Err(SyncError::InvalidData(
                "dataset selection must not be empty".to_owned(),
            ));
        }

        let mut scan_roots = Vec::new();
        let mut files = BTreeSet::new();
        let mut rules = Vec::new();

        for dataset_id in &dataset_ids {
            let definition = dataset_definition(dataset_id).ok_or_else(|| {
                SyncError::InvalidData(format!("unknown dataset id: {}", dataset_id))
            })?;

            for root in definition.scan_roots {
                push_compact_root(&mut scan_roots, root);
            }
            for file in definition.files {
                files.insert(*file);
            }
            rules.extend_from_slice(definition.rules);
        }

        let selection = DatasetSelection::new(
            DATASET_POLICY_VERSION,
            dataset_ids.into_iter().map(str::to_owned).collect(),
        );

        Ok(Self {
            selection,
            scan_roots,
            files: files.into_iter().collect(),
            rules,
        })
    }

    pub fn tauri_tavern_default() -> Self {
        Self::from_selection(&tauri_tavern_default_selection())
            .expect("TauriTavern default dataset policy must be valid")
    }

    pub fn selection(&self) -> &DatasetSelection {
        &self.selection
    }

    pub fn scan_roots(&self) -> &[&'static str] {
        &self.scan_roots
    }

    pub fn files(&self) -> &[&'static str] {
        &self.files
    }

    pub fn contains_path(&self, relative_path: &str) -> bool {
        !is_globally_excluded(relative_path)
            && (self.files.contains(&relative_path)
                || self.rules.iter().any(|rule| rule.matches(relative_path)))
    }

    pub fn should_descend_dir(&self, relative_dir: &str) -> bool {
        !is_globally_excluded(relative_dir)
            && self
                .rules
                .iter()
                .any(|rule| rule.may_match_descendant(relative_dir))
    }

    pub fn allows_delete(&self, relative_path: &str) -> bool {
        self.contains_path(relative_path)
    }
}

/// Return the canonical directory boundary for pruning parents after deleting
/// a synchronized file. The boundary itself must never be removed.
///
/// Exact-file datasets return `None`. Directory datasets use the deepest
/// matching scan root, keeping future overlapping definitions conservative.
pub fn prune_boundary_for_path(relative_path: &str) -> Result<Option<&'static str>, SyncError> {
    if is_globally_excluded(relative_path) {
        return Err(SyncError::InvalidData(format!(
            "path is outside the dataset catalog: {relative_path}"
        )));
    }

    if DATASETS
        .iter()
        .any(|definition| definition.files.contains(&relative_path))
    {
        return Ok(None);
    }

    let mut boundary = None;
    for definition in DATASETS {
        if !definition
            .rules
            .iter()
            .any(|rule| rule.matches(relative_path))
        {
            continue;
        }

        let matching_root = definition
            .scan_roots
            .iter()
            .copied()
            .filter(|root| is_same_or_under(relative_path, root))
            .max_by_key(|root| root.len())
            .ok_or_else(|| {
                SyncError::Internal(format!(
                    "dataset {} has no scan root for path {relative_path}",
                    definition.id
                ))
            })?;

        if boundary.is_none_or(|current: &str| matching_root.len() > current.len()) {
            boundary = Some(matching_root);
        }
    }

    boundary.map(Some).ok_or_else(|| {
        SyncError::InvalidData(format!(
            "path is outside the dataset catalog: {relative_path}"
        ))
    })
}

fn push_compact_root(roots: &mut Vec<&'static str>, root: &'static str) {
    if roots
        .iter()
        .any(|existing| root == *existing || is_under(root, existing))
    {
        return;
    }

    roots.retain(|existing| !is_under(existing, root));
    roots.push(root);
}
