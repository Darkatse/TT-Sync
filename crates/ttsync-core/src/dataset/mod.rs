mod catalog;
mod path;
mod policy;
mod profile;
mod runtime;

pub use path::is_excluded;
pub use policy::{ResolvedDatasetPolicy, prune_boundary_for_path};
pub use profile::{
    AGENT_RUN_HISTORY_FULL_PROFILE_ID, TAURI_TAVERN_DEFAULT_PROFILE_ID,
    TAURI_TAVERN_FULL_PROFILE_ID, supported_dataset_ids, supported_profile_ids,
    tauri_tavern_default_selection, tauri_tavern_full_selection,
};
pub use runtime::{
    agent_run_json_is_terminal, is_agent_run_index_file, is_agent_run_root_dir,
    is_terminal_agent_run_status,
};

#[cfg(test)]
mod tests {
    use ttsync_contract::dataset::{DATASET_POLICY_VERSION, DatasetSelection};

    use super::*;

    #[test]
    fn tauri_tavern_default_includes_agent_continuity_without_secrets() {
        let policy = ResolvedDatasetPolicy::tauri_tavern_default();

        assert!(policy.contains_path("_tauritavern/agent-profiles/profiles/writer.json"));
        assert!(policy.contains_path("_tauritavern/llm-connections/main.json"));
        assert!(policy.contains_path("_tauritavern/skills/index/skills.json"));
        assert!(policy.contains_path(
            "_tauritavern/agent-workspaces/chats/ws/persistent-states/run-1/manifest.json"
        ));
        assert!(policy.contains_path("_tauritavern/agent-workspaces/index/runs/run-1.json"));
        assert!(
            policy.contains_path("_tauritavern/agent-workspaces/chats/ws/runs/run-1/events.jsonl")
        );
        assert!(policy.contains_path("_tauritavern/agent-workspaces/chats/ws/runs/run-1/run.json"));
        assert!(!policy.contains_path(
            "_tauritavern/agent-workspaces/chats/ws/runs/run-1/input/prompt_snapshot.json"
        ));
        assert!(!policy.contains_path(
            "_tauritavern/agent-workspaces/chats/ws/runs/run-1/model-responses/round-001.json"
        ));
        assert!(!policy.contains_path(
            "_tauritavern/agent-workspaces/chats/ws/runs/run-1/checkpoints/cp-1/manifest.json"
        ));
        assert!(!policy.contains_path("default-user/secrets.json"));
        assert!(!policy.contains_path("_tauritavern/prompt-cache/cache.json"));
    }

    #[test]
    fn unknown_dataset_fails_fast() {
        let selection = DatasetSelection::new(DATASET_POLICY_VERSION, vec!["missing".to_owned()]);
        assert!(ResolvedDatasetPolicy::from_selection(&selection).is_err());
    }

    #[test]
    fn public_catalog_hides_compatibility_only_datasets() {
        let public_ids = supported_dataset_ids();

        assert!(public_ids.contains(&"chat.character.history".to_owned()));
        assert!(public_ids.contains(&"agent.profiles".to_owned()));
        assert!(!public_ids.contains(&"legacy.user".to_owned()));
        assert!(!public_ids.contains(&"legacy.v2".to_owned()));
    }

    #[test]
    fn profile_ids_are_exposed_separately_from_leaf_datasets() {
        let profile_ids = supported_profile_ids();
        let dataset_ids = supported_dataset_ids();

        assert!(profile_ids.contains(&TAURI_TAVERN_DEFAULT_PROFILE_ID.to_owned()));
        assert!(profile_ids.contains(&TAURI_TAVERN_FULL_PROFILE_ID.to_owned()));
        assert!(profile_ids.contains(&AGENT_RUN_HISTORY_FULL_PROFILE_ID.to_owned()));
        assert!(!dataset_ids.contains(&TAURI_TAVERN_DEFAULT_PROFILE_ID.to_owned()));
    }

    #[test]
    fn full_profile_composes_precise_agent_run_leaves() {
        let selection = DatasetSelection::new(
            DATASET_POLICY_VERSION,
            vec![
                TAURI_TAVERN_FULL_PROFILE_ID.to_owned(),
                "chat.character.history".to_owned(),
            ],
        );
        let policy = ResolvedDatasetPolicy::from_selection(&selection).unwrap();

        assert!(policy.contains_path("default-user/secrets.json"));
        assert!(policy.contains_path(
            "_tauritavern/agent-workspaces/chats/ws/runs/run-1/model-responses/round-001.json"
        ));
        assert!(policy.contains_path(
            "_tauritavern/agent-workspaces/chats/ws/runs/run-1/tool-results/result-001.json"
        ));
        assert!(policy.contains_path(
            "_tauritavern/agent-workspaces/chats/ws/runs/run-1/input/prompt_snapshot.json"
        ));
        assert!(
            policy.contains_path(
                "_tauritavern/agent-workspaces/chats/ws/runs/run-1/persist/memory.md"
            )
        );
    }

    #[test]
    fn agent_run_component_leaves_match_only_run_subtrees() {
        let selection = DatasetSelection::new(
            DATASET_POLICY_VERSION,
            vec!["agent.model_responses".to_owned()],
        );
        let policy = ResolvedDatasetPolicy::from_selection(&selection).unwrap();

        assert!(policy.contains_path(
            "_tauritavern/agent-workspaces/chats/ws/runs/run-1/model-responses/round-001.json"
        ));
        assert!(!policy.contains_path(
            "_tauritavern/agent-workspaces/chats/ws/persistent-states/run-1/model-responses/round-001.json"
        ));
    }

    #[test]
    fn temporary_exclusion_is_specific_to_known_temp_shapes() {
        let policy = ResolvedDatasetPolicy::tauri_tavern_default();

        assert!(policy.contains_path("default-user/chats/alice/not.tmp-user.jsonl"));
        assert!(!policy.contains_path("default-user/chats/alice/chat.jsonl.ttsync.tmp"));
        assert!(!policy.contains_path("default-user/chats/alice/.tmp-write/chat.jsonl"));
    }

    #[test]
    fn prune_boundaries_follow_dataset_ownership() {
        assert_eq!(
            prune_boundary_for_path("extensions/third-party/example/.git/HEAD").unwrap(),
            Some("extensions/third-party")
        );
        assert_eq!(
            prune_boundary_for_path("_tauritavern/extension-sources/local/source.json").unwrap(),
            Some("_tauritavern/extension-sources/local")
        );
        assert_eq!(
            prune_boundary_for_path("_tauritavern/agent-workspaces/index/runs/run-1.json").unwrap(),
            Some("_tauritavern/agent-workspaces/index/runs")
        );
        assert_eq!(
            prune_boundary_for_path("default-user/user/images/chat/image.png").unwrap(),
            Some("default-user/user/images")
        );
        assert_eq!(
            prune_boundary_for_path("default-user/settings.json").unwrap(),
            None
        );
    }

    #[test]
    fn prune_boundary_rejects_paths_outside_dataset_scope() {
        assert!(prune_boundary_for_path("outside/file.txt").is_err());
        assert!(prune_boundary_for_path("default-user/chats/.staging/file.jsonl").is_err());
    }

    #[test]
    fn default_policy_prunes_unselected_agent_run_subtrees() {
        let policy = ResolvedDatasetPolicy::tauri_tavern_default();

        assert!(policy.should_descend_dir("_tauritavern/agent-workspaces/chats/ws"));
        assert!(
            policy.should_descend_dir("_tauritavern/agent-workspaces/chats/ws/persistent-states")
        );
        assert!(policy.should_descend_dir("_tauritavern/agent-workspaces/chats/ws/runs/run-1"));
        assert!(
            !policy.should_descend_dir("_tauritavern/agent-workspaces/chats/ws/runs/run-1/input")
        );
        assert!(!policy.should_descend_dir(
            "_tauritavern/agent-workspaces/chats/ws/runs/run-1/model-responses"
        ));
        assert!(
            !policy.should_descend_dir(
                "_tauritavern/agent-workspaces/chats/ws/runs/run-1/checkpoints"
            )
        );
    }

    #[test]
    fn identifies_agent_run_boundaries_and_terminal_statuses() {
        assert!(is_agent_run_root_dir(
            "_tauritavern/agent-workspaces/chats/ws/runs/run-1"
        ));
        assert!(!is_agent_run_root_dir(
            "_tauritavern/agent-workspaces/chats/ws/runs/run-1/events.jsonl"
        ));
        assert!(is_agent_run_index_file(
            "_tauritavern/agent-workspaces/index/runs/run-1.json"
        ));
        assert!(is_terminal_agent_run_status("partial_success"));
        assert!(!is_terminal_agent_run_status("calling_model"));
    }

    #[test]
    fn parses_agent_run_terminal_state_from_json() {
        assert!(agent_run_json_is_terminal(r#"{"status":"completed"}"#).unwrap());
        assert!(!agent_run_json_is_terminal(r#"{"status":"running"}"#).unwrap());
        assert!(agent_run_json_is_terminal(r#"{}"#).is_err());
    }
}
