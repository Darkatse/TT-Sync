use crate::dataset::path::is_same_or_under;
use crate::dataset::runtime::{
    agent_run_component_may_contain_path, agent_run_file_may_contain_path,
    agent_workspace_component_may_contain_path, is_agent_run_file_path, is_agent_run_subtree_path,
    is_agent_workspace_component_path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DatasetVisibility {
    Public,
    Internal,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DatasetDefinition {
    pub(super) id: &'static str,
    pub(super) visibility: DatasetVisibility,
    pub(super) scan_roots: &'static [&'static str],
    pub(super) files: &'static [&'static str],
    pub(super) rules: &'static [DatasetRule],
}

impl DatasetDefinition {
    pub(super) fn is_public(self) -> bool {
        self.visibility == DatasetVisibility::Public
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum DatasetRule {
    Prefix(&'static str),
    AgentWorkspaceComponent(&'static str),
    AgentRunFile(&'static str),
    AgentRunComponent(&'static str),
}

impl DatasetRule {
    pub(super) fn matches(self, relative_path: &str) -> bool {
        match self {
            Self::Prefix(prefix) => is_same_or_under(relative_path, prefix),
            Self::AgentWorkspaceComponent(component) => {
                is_agent_workspace_component_path(relative_path, component)
            }
            Self::AgentRunFile(file_name) => is_agent_run_file_path(relative_path, file_name),
            Self::AgentRunComponent(component) => {
                is_agent_run_subtree_path(relative_path, component)
            }
        }
    }

    pub(super) fn may_match_descendant(self, relative_dir: &str) -> bool {
        match self {
            Self::Prefix(prefix) => {
                is_same_or_under(relative_dir, prefix) || is_same_or_under(prefix, relative_dir)
            }
            Self::AgentWorkspaceComponent(component) => {
                agent_workspace_component_may_contain_path(relative_dir, component)
            }
            Self::AgentRunFile(file_name) => {
                agent_run_file_may_contain_path(relative_dir, file_name)
            }
            Self::AgentRunComponent(component) => {
                agent_run_component_may_contain_path(relative_dir, component)
            }
        }
    }
}

pub(super) fn dataset_definition(id: &str) -> Option<&'static DatasetDefinition> {
    DATASETS.iter().find(|definition| definition.id == id)
}

pub(super) fn public_dataset_ids() -> Vec<String> {
    DATASETS
        .iter()
        .filter(|definition| definition.is_public())
        .map(|definition| definition.id.to_owned())
        .collect()
}

const EMPTY_DIRS: &[&str] = &[];
const EMPTY_FILES: &[&str] = &[];

macro_rules! public_dataset {
    ($id:literal, dirs: [$($dir:literal),* $(,)?], files: [$($file:literal),* $(,)?]) => {
        DatasetDefinition {
            id: $id,
            visibility: DatasetVisibility::Public,
            scan_roots: &[$($dir),*],
            files: &[$($file),*],
            rules: &[$(DatasetRule::Prefix($dir)),*],
        }
    };
    ($id:literal, dirs: [$($dir:literal),* $(,)?]) => {
        public_dataset!($id, dirs: [$($dir),*], files: [])
    };
    ($id:literal, files: [$($file:literal),* $(,)?]) => {
        DatasetDefinition {
            id: $id,
            visibility: DatasetVisibility::Public,
            scan_roots: EMPTY_DIRS,
            files: &[$($file),*],
            rules: &[],
        }
    };
}

macro_rules! internal_dataset {
    ($id:literal, dirs: [$($dir:literal),* $(,)?]) => {
        DatasetDefinition {
            id: $id,
            visibility: DatasetVisibility::Internal,
            scan_roots: &[$($dir),*],
            files: EMPTY_FILES,
            rules: &[$(DatasetRule::Prefix($dir)),*],
        }
    };
}

pub(super) const DATASETS: &[DatasetDefinition] = &[
    public_dataset!(
        "settings.core",
        files: [
            "default-user/settings.json",
            "default-user/tauritavern-settings.json",
            "default-user/image-metadata.json",
        ]
    ),
    public_dataset!("secrets.api_keys", files: ["default-user/secrets.json"]),
    public_dataset!("chat.character.history", dirs: ["default-user/chats"]),
    public_dataset!("chat.group.metadata", dirs: ["default-user/groups"]),
    public_dataset!("chat.group.history", dirs: ["default-user/group chats"]),
    public_dataset!("character.cards", dirs: ["default-user/characters"]),
    public_dataset!("character.avatars", dirs: ["default-user/User Avatars"]),
    public_dataset!("world.info", dirs: ["default-user/worlds"]),
    public_dataset!("preset.openai", dirs: ["default-user/OpenAI Settings"]),
    public_dataset!("preset.novelai", dirs: ["default-user/NovelAI Settings"]),
    public_dataset!("preset.textgen", dirs: ["default-user/TextGen Settings"]),
    public_dataset!("preset.kobold", dirs: ["default-user/KoboldAI Settings"]),
    public_dataset!("prompt.instruct", dirs: ["default-user/instruct"]),
    public_dataset!("prompt.context", dirs: ["default-user/context"]),
    public_dataset!("prompt.sysprompt", dirs: ["default-user/sysprompt"]),
    public_dataset!("prompt.reasoning", dirs: ["default-user/reasoning"]),
    public_dataset!("quick.replies", dirs: ["default-user/QuickReplies"]),
    public_dataset!("ui.themes", dirs: ["default-user/themes"]),
    public_dataset!("ui.moving", dirs: ["default-user/movingUI"]),
    public_dataset!("media.backgrounds", dirs: ["default-user/backgrounds"]),
    public_dataset!("media.assets", dirs: ["default-user/assets"]),
    public_dataset!("media.thumbnails", dirs: ["default-user/thumbnails"]),
    public_dataset!("media.user_images", dirs: ["default-user/user/images"]),
    public_dataset!("user.files", dirs: ["default-user/user/files"]),
    public_dataset!("user.workflows", dirs: ["default-user/user/workflows"]),
    public_dataset!("vectors", dirs: ["default-user/vectors"]),
    public_dataset!("backups", dirs: ["default-user/backups"]),
    internal_dataset!("legacy.user", dirs: ["default-user/user"]),
    public_dataset!("extensions.local", dirs: ["default-user/extensions"]),
    public_dataset!("extensions.third_party", dirs: ["extensions/third-party"]),
    public_dataset!(
        "extensions.sources",
        dirs: [
            "_tauritavern/extension-sources/local",
            "_tauritavern/extension-sources/global",
        ]
    ),
    public_dataset!("extensions.store", dirs: ["_tauritavern/extension-store"]),
    public_dataset!("agent.profiles", dirs: ["_tauritavern/agent-profiles/profiles"]),
    public_dataset!("agent.llm_connections", dirs: ["_tauritavern/llm-connections"]),
    public_dataset!(
        "agent.skills",
        dirs: ["_tauritavern/skills/installed", "_tauritavern/skills/index"]
    ),
    DatasetDefinition {
        id: "agent.persistent_state",
        visibility: DatasetVisibility::Public,
        scan_roots: &["_tauritavern/agent-workspaces/chats"],
        files: EMPTY_FILES,
        rules: &[DatasetRule::AgentWorkspaceComponent("persistent-states")],
    },
    DatasetDefinition {
        id: "agent.run_journal",
        visibility: DatasetVisibility::Public,
        scan_roots: &[
            "_tauritavern/agent-workspaces/index/runs",
            "_tauritavern/agent-workspaces/chats",
        ],
        files: EMPTY_FILES,
        rules: &[
            DatasetRule::Prefix("_tauritavern/agent-workspaces/index/runs"),
            DatasetRule::AgentRunFile("run.json"),
            DatasetRule::AgentRunFile("events.jsonl"),
        ],
    },
    DatasetDefinition {
        id: "agent.run_context",
        visibility: DatasetVisibility::Public,
        scan_roots: &["_tauritavern/agent-workspaces/chats"],
        files: EMPTY_FILES,
        rules: &[
            DatasetRule::AgentRunFile("manifest.json"),
            DatasetRule::AgentRunComponent("input"),
            DatasetRule::AgentRunComponent("invocations"),
        ],
    },
    DatasetDefinition {
        id: "agent.run_workspace_projection",
        visibility: DatasetVisibility::Public,
        scan_roots: &["_tauritavern/agent-workspaces/chats"],
        files: EMPTY_FILES,
        rules: &[
            DatasetRule::AgentRunComponent("persist"),
            DatasetRule::AgentRunComponent("summaries"),
            DatasetRule::AgentRunComponent("plan"),
        ],
    },
    DatasetDefinition {
        id: "agent.run_tool_io",
        visibility: DatasetVisibility::Public,
        scan_roots: &["_tauritavern/agent-workspaces/chats"],
        files: EMPTY_FILES,
        rules: &[
            DatasetRule::AgentRunComponent("tool-args"),
            DatasetRule::AgentRunComponent("tool-results"),
            DatasetRule::AgentRunComponent("agent-results"),
        ],
    },
    DatasetDefinition {
        id: "agent.workspace_outputs",
        visibility: DatasetVisibility::Public,
        scan_roots: &["_tauritavern/agent-workspaces/chats"],
        files: EMPTY_FILES,
        rules: &[DatasetRule::AgentRunComponent("output")],
    },
    DatasetDefinition {
        id: "agent.workspace_scratch",
        visibility: DatasetVisibility::Public,
        scan_roots: &["_tauritavern/agent-workspaces/chats"],
        files: EMPTY_FILES,
        rules: &[DatasetRule::AgentRunComponent("scratch")],
    },
    DatasetDefinition {
        id: "agent.tasks",
        visibility: DatasetVisibility::Public,
        scan_roots: &["_tauritavern/agent-workspaces/chats"],
        files: EMPTY_FILES,
        rules: &[DatasetRule::AgentRunComponent("tasks")],
    },
    DatasetDefinition {
        id: "agent.model_responses",
        visibility: DatasetVisibility::Public,
        scan_roots: &["_tauritavern/agent-workspaces/chats"],
        files: EMPTY_FILES,
        rules: &[DatasetRule::AgentRunComponent("model-responses")],
    },
    DatasetDefinition {
        id: "agent.checkpoints",
        visibility: DatasetVisibility::Public,
        scan_roots: &["_tauritavern/agent-workspaces/chats"],
        files: EMPTY_FILES,
        rules: &[DatasetRule::AgentRunComponent("checkpoints")],
    },
];
