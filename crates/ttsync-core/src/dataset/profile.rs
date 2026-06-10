use std::collections::BTreeSet;

use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DatasetSelection, LEGACY_V2_DATASET_ID};

use crate::dataset::catalog::{dataset_definition, public_dataset_ids};
use crate::error::SyncError;

pub const TAURI_TAVERN_DEFAULT_PROFILE_ID: &str = "tauritavern.default";
pub const TAURI_TAVERN_FULL_PROFILE_ID: &str = "tauritavern.full";
pub const AGENT_RUN_HISTORY_FULL_PROFILE_ID: &str = "agent.run_history.full";

const LEGACY_V2_DATASETS: &[&str] = &[
    "chat.character.history",
    "character.cards",
    "chat.group.metadata",
    "chat.group.history",
    "world.info",
    "media.backgrounds",
    "ui.themes",
    "legacy.user",
    "character.avatars",
    "preset.openai",
    "preset.novelai",
    "preset.textgen",
    "preset.kobold",
    "prompt.instruct",
    "prompt.context",
    "quick.replies",
    "media.assets",
    "extensions.local",
    "extensions.third_party",
    "extensions.sources",
    "settings.core",
    "secrets.api_keys",
];

const TAURI_TAVERN_DEFAULT_DATASETS: &[&str] = &[
    "settings.core",
    "chat.character.history",
    "chat.group.metadata",
    "chat.group.history",
    "character.cards",
    "character.avatars",
    "world.info",
    "preset.openai",
    "preset.novelai",
    "preset.textgen",
    "preset.kobold",
    "prompt.instruct",
    "prompt.context",
    "prompt.sysprompt",
    "prompt.reasoning",
    "quick.replies",
    "ui.themes",
    "ui.moving",
    "media.backgrounds",
    "media.assets",
    "media.user_images",
    "user.files",
    "user.workflows",
    "extensions.local",
    "extensions.third_party",
    "extensions.sources",
    "extensions.store",
    "agent.profiles",
    "agent.llm_connections",
    "agent.skills",
    "agent.persistent_state",
    "agent.run_journal",
];

const AGENT_RUN_HISTORY_FULL_DATASETS: &[&str] = &[
    "agent.run_journal",
    "agent.run_context",
    "agent.run_workspace_projection",
    "agent.run_tool_io",
    "agent.workspace_outputs",
    "agent.workspace_scratch",
    "agent.tasks",
    "agent.model_responses",
    "agent.checkpoints",
];

const TAURI_TAVERN_FULL_EXTRA_DATASETS: &[&str] = &[
    "secrets.api_keys",
    "media.thumbnails",
    "vectors",
    "backups",
    "agent.run_context",
    "agent.run_workspace_projection",
    "agent.run_tool_io",
    "agent.workspace_outputs",
    "agent.workspace_scratch",
    "agent.tasks",
    "agent.model_responses",
    "agent.checkpoints",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileVisibility {
    Public,
    Internal,
}

#[derive(Debug, Clone, Copy)]
struct ProfileDefinition {
    id: &'static str,
    visibility: ProfileVisibility,
    expansion: ProfileExpansion,
}

#[derive(Debug, Clone, Copy)]
enum ProfileExpansion {
    Direct(&'static [&'static str]),
    DefaultPlus(&'static [&'static str]),
}

impl ProfileDefinition {
    fn is_public(self) -> bool {
        self.visibility == ProfileVisibility::Public
    }
}

const PROFILES: &[ProfileDefinition] = &[
    ProfileDefinition {
        id: LEGACY_V2_DATASET_ID,
        visibility: ProfileVisibility::Internal,
        expansion: ProfileExpansion::Direct(LEGACY_V2_DATASETS),
    },
    ProfileDefinition {
        id: TAURI_TAVERN_DEFAULT_PROFILE_ID,
        visibility: ProfileVisibility::Public,
        expansion: ProfileExpansion::Direct(TAURI_TAVERN_DEFAULT_DATASETS),
    },
    ProfileDefinition {
        id: TAURI_TAVERN_FULL_PROFILE_ID,
        visibility: ProfileVisibility::Public,
        expansion: ProfileExpansion::DefaultPlus(TAURI_TAVERN_FULL_EXTRA_DATASETS),
    },
    ProfileDefinition {
        id: AGENT_RUN_HISTORY_FULL_PROFILE_ID,
        visibility: ProfileVisibility::Public,
        expansion: ProfileExpansion::Direct(AGENT_RUN_HISTORY_FULL_DATASETS),
    },
];

pub fn legacy_v2_selection() -> DatasetSelection {
    DatasetSelection::legacy_v2()
}

pub fn tauri_tavern_default_selection() -> DatasetSelection {
    DatasetSelection::new(
        DATASET_POLICY_VERSION,
        TAURI_TAVERN_DEFAULT_DATASETS
            .iter()
            .map(|id| (*id).to_owned())
            .collect(),
    )
}

pub fn tauri_tavern_full_selection() -> DatasetSelection {
    let mut ids = TAURI_TAVERN_DEFAULT_DATASETS
        .iter()
        .map(|id| (*id).to_owned())
        .collect::<Vec<_>>();
    ids.extend(
        TAURI_TAVERN_FULL_EXTRA_DATASETS
            .iter()
            .map(|id| (*id).to_owned()),
    );
    DatasetSelection::new(DATASET_POLICY_VERSION, ids)
}

pub fn supported_dataset_ids() -> Vec<String> {
    public_dataset_ids()
}

pub fn supported_profile_ids() -> Vec<String> {
    PROFILES
        .iter()
        .filter(|profile| profile.is_public())
        .map(|profile| profile.id.to_owned())
        .collect()
}

pub(super) fn expand_dataset_ids(ids: &[String]) -> Result<Vec<&'static str>, SyncError> {
    let mut expanded = Vec::new();
    let mut seen = BTreeSet::new();

    for id in ids {
        if let Some(profile) = profile_definition(id) {
            expand_profile(profile, &mut expanded, &mut seen);
            continue;
        }

        let expansion: &[&str] = {
            let definition = dataset_definition(id)
                .ok_or_else(|| SyncError::InvalidData(format!("unknown dataset id: {}", id)))?;
            std::slice::from_ref(&definition.id)
        };

        for dataset_id in expansion {
            if seen.insert(*dataset_id) {
                expanded.push(*dataset_id);
            }
        }
    }

    Ok(expanded)
}

fn profile_definition(id: &str) -> Option<&'static ProfileDefinition> {
    PROFILES.iter().find(|profile| profile.id == id)
}

fn expand_profile(
    profile: &ProfileDefinition,
    expanded: &mut Vec<&'static str>,
    seen: &mut BTreeSet<&'static str>,
) {
    match profile.expansion {
        ProfileExpansion::Direct(dataset_ids) => push_dataset_ids(dataset_ids, expanded, seen),
        ProfileExpansion::DefaultPlus(extra_dataset_ids) => {
            push_dataset_ids(TAURI_TAVERN_DEFAULT_DATASETS, expanded, seen);
            push_dataset_ids(extra_dataset_ids, expanded, seen);
        }
    }
}

fn push_dataset_ids(
    dataset_ids: &'static [&'static str],
    expanded: &mut Vec<&'static str>,
    seen: &mut BTreeSet<&'static str>,
) {
    for dataset_id in dataset_ids {
        if seen.insert(*dataset_id) {
            expanded.push(*dataset_id);
        }
    }
}
