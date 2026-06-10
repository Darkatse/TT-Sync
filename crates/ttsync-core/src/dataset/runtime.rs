use crate::error::SyncError;

pub fn is_agent_run_root_dir(relative_path: &str) -> bool {
    let parts = relative_path.split('/').collect::<Vec<_>>();
    parts.len() == 6
        && parts[0] == "_tauritavern"
        && parts[1] == "agent-workspaces"
        && parts[2] == "chats"
        && parts[4] == "runs"
}

pub fn is_agent_run_index_file(relative_path: &str) -> bool {
    let parts = relative_path.split('/').collect::<Vec<_>>();
    parts.len() == 5
        && parts[0] == "_tauritavern"
        && parts[1] == "agent-workspaces"
        && parts[2] == "index"
        && parts[3] == "runs"
        && parts[4].ends_with(".json")
}

pub fn is_terminal_agent_run_status(status: &str) -> bool {
    matches!(
        status,
        "completed" | "partial_success" | "cancelled" | "failed"
    )
}

pub fn agent_run_json_is_terminal(text: &str) -> Result<bool, SyncError> {
    let value = serde_json::from_str::<serde_json::Value>(text)
        .map_err(|e| SyncError::InvalidData(format!("invalid agent run JSON: {e}")))?;
    let status = value
        .get("status")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| SyncError::InvalidData("agent run JSON missing status".to_owned()))?;

    Ok(is_terminal_agent_run_status(status))
}

pub(super) fn is_agent_workspace_component_path(relative_path: &str, component: &str) -> bool {
    let parts = relative_path.split('/').collect::<Vec<_>>();
    is_agent_workspace_path(&parts) && parts.len() > 5 && parts[4] == component
}

pub(super) fn is_agent_run_subtree_path(relative_path: &str, component: &str) -> bool {
    let parts = relative_path.split('/').collect::<Vec<_>>();
    is_agent_run_path(&parts) && parts.len() > 6 && parts[6] == component
}

pub(super) fn is_agent_run_file_path(relative_path: &str, file_name: &str) -> bool {
    let parts = relative_path.split('/').collect::<Vec<_>>();
    is_agent_run_path(&parts) && parts.len() == 7 && parts[6] == file_name
}

pub(super) fn agent_workspace_component_may_contain_path(
    relative_dir: &str,
    component: &str,
) -> bool {
    let parts = relative_dir.split('/').collect::<Vec<_>>();
    if !matches_agent_workspace_prefix(&parts) {
        return false;
    }

    parts.len() <= 4 || parts[4] == component
}

pub(super) fn agent_run_component_may_contain_path(relative_dir: &str, component: &str) -> bool {
    let parts = relative_dir.split('/').collect::<Vec<_>>();
    if !matches_agent_workspace_prefix(&parts) {
        return false;
    }

    match parts.len() {
        0..=4 => true,
        5 => parts[4] == "runs",
        6 => parts[4] == "runs",
        _ => parts[4] == "runs" && parts[6] == component,
    }
}

pub(super) fn agent_run_file_may_contain_path(relative_dir: &str, _file_name: &str) -> bool {
    let parts = relative_dir.split('/').collect::<Vec<_>>();
    if !matches_agent_workspace_prefix(&parts) {
        return false;
    }

    match parts.len() {
        0..=4 => true,
        5 => parts[4] == "runs",
        6 => parts[4] == "runs",
        _ => false,
    }
}

fn is_agent_workspace_path(parts: &[&str]) -> bool {
    parts.len() > 4
        && parts[0] == "_tauritavern"
        && parts[1] == "agent-workspaces"
        && parts[2] == "chats"
}

fn is_agent_run_path(parts: &[&str]) -> bool {
    is_agent_workspace_path(parts) && parts[4] == "runs"
}

fn matches_agent_workspace_prefix(parts: &[&str]) -> bool {
    const PREFIX: &[&str] = &["_tauritavern", "agent-workspaces", "chats"];

    if parts.len() <= PREFIX.len() {
        return parts
            .iter()
            .zip(PREFIX.iter())
            .all(|(actual, expected)| actual == expected);
    }

    parts[0] == PREFIX[0] && parts[1] == PREFIX[1] && parts[2] == PREFIX[2]
}
