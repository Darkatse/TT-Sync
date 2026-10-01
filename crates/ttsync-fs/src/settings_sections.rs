//! Settings section split/merge logic.
//!
//! Ported verbatim from TauriTavern's
//! `src-tauri/crates/tt-adapter-storage-core/src/repositories/file_settings_repository/fields.rs`
//! (and `sections.rs`) so the field ownership table stays in lockstep with the
//! TauriTavern client that produces these section files. Only the wire path
//! prefix differs: TauriTavern paths are relative to the user directory, here
//! they carry the `default-user/` prefix used on the sync wire.

use serde_json::{Value, json};

// Wire path of the core settings file (the merged SillyTavern settings.json).
// The literal lives in `settings_file`, shared with the persona bridging layer.
pub(crate) const SETTINGS_CORE_FILE: &str = crate::settings_file::SETTINGS_JSON_WIRE_PATH;
pub(crate) const APPEARANCE_FILE: &str = "default-user/settings/appearance.json";
pub(crate) const PRESETS_FILE: &str = "default-user/settings/presets.json";
pub(crate) const LAYOUT_FILE: &str = "default-user/settings/layout.json";
pub(crate) const PERSONA_STATE_FILE: &str = "default-user/settings/persona-state.json";

/// All settings section files in canonical order.
pub(crate) const SETTINGS_SECTION_PATHS: [&str; 4] = [
    APPEARANCE_FILE,
    PRESETS_FILE,
    LAYOUT_FILE,
    PERSONA_STATE_FILE,
];

type FieldGroup = (&'static [&'static str], &'static [&'static str]);

// Keep the appearance snapshot aligned with getThemeObject in power-user.js.
const APPEARANCE_FIELDS: &[FieldGroup] = &[
    (&[], &["background"]),
    (
        &["power_user"],
        &[
            "theme",
            "theme_fallback",
            "theme_bindings",
            "blur_strength",
            "main_text_color",
            "italics_text_color",
            "underline_text_color",
            "quote_text_color",
            "blur_tint_color",
            "chat_tint_color",
            "user_mes_blur_tint_color",
            "bot_mes_blur_tint_color",
            "shadow_color",
            "shadow_width",
            "border_color",
            "font_scale",
            "fast_ui_mode",
            "waifuMode",
            "avatar_style",
            "chat_display",
            "toastr_position",
            "noShadows",
            "chat_width",
            "timer_enabled",
            "timestamps_enabled",
            "timestamp_model_icon",
            "mesIDDisplay_enabled",
            "hideChatAvatars_enabled",
            "message_token_count_enabled",
            "message_ttft_enabled",
            "message_token_rate_enabled",
            "message_cache_enabled",
            "expand_message_actions",
            "enableZenSliders",
            "enableLabMode",
            "hotswap_enabled",
            "custom_css",
            "bogus_folders",
            "zoomed_avatar_magnification",
            "reduced_motion",
            "compact_input_area",
            "show_swipe_num_all_messages",
            "click_to_edit",
            "media_display",
        ],
    ),
];

const PRESET_FIELDS: &[FieldGroup] = &[
    // These are the active preset snapshots, not the named preset libraries.
    // Keeping only their names would pair local names with another device's parameters.
    (
        &[],
        &[
            "main_api",
            "selected_proxy",
            "amount_gen",
            "max_context",
            "oai_settings",
            "nai_settings",
            "kai_settings",
            "textgenerationwebui_settings",
            "horde_settings",
            "preset_settings",
            "preset_settings_novel",
        ],
    ),
    (
        &["power_user"],
        &["instruct", "context", "sysprompt", "reasoning"],
    ),
    (
        &["extension_settings", "connectionManager"],
        &["selectedProfile", "selectedItem"],
    ),
];

const PERSONA_STATE_FIELDS: &[FieldGroup] = &[
    (&[], &["username", "user_avatar"]),
    (
        &["power_user"],
        &[
            "default_persona",
            "persona_description",
            "persona_description_position",
            "persona_description_depth",
            "persona_description_role",
            "persona_description_lorebook",
            "persona_show_notifications",
            "persona_auto_lock",
            "persona_allow_multi_connections",
        ],
    ),
];

const LAYOUT_FIELDS: &[FieldGroup] = &[
    (
        &[],
        &[
            "firstRun",
            "currentVersion",
            "active_character",
            "active_group",
            "selected_button",
        ],
    ),
    (
        &["power_user"],
        &[
            "movingUI",
            "movingUIState",
            "movingUIPreset",
            "mobile_immersive_fullscreen",
            "charListGrid",
            "sort_field",
            "sort_order",
            "sort_rule",
            "persona_sort_order",
            "show_tag_filters",
            "show_tag_filters_group_candidates",
            "show_tag_filters_group_members",
            "aux_field",
        ],
    ),
    // AccountStorage also contains extension data; only known UI state belongs here.
    (
        &["accountStorage"],
        &[
            "Characters_PerPage",
            "Personas_PerPage",
            "Personas_GridView",
            "GroupMembers_PerPage",
            "GroupCandidates_PerPage",
            "FeatherlessModels_PerPage",
            "WI_PerPage",
            "WINavLockOn",
            "WINavOpened",
            "LNavLockOn",
            "LNavOpened",
            "NavLockOn",
            "NavOpened",
            "SelectedNavTab",
            "characterSearchFormVisible",
            "world_info_sort_order",
            "DataBank_sortField",
            "DataBank_sortOrder",
        ],
    ),
];

pub(crate) struct UserSettingsSections {
    pub(crate) core: Value,
    pub(crate) appearance: Value,
    pub(crate) presets: Value,
    pub(crate) layout: Value,
    pub(crate) persona_state: Value,
}

impl UserSettingsSections {
    pub(crate) fn split(mut settings: Value) -> Self {
        let mut appearance = json!({});
        let mut presets = json!({});
        let mut layout = json!({});
        let mut persona_state = json!({});
        move_fields(&mut settings, &mut appearance, APPEARANCE_FIELDS);
        move_fields(&mut settings, &mut presets, PRESET_FIELDS);
        move_fields(&mut settings, &mut layout, LAYOUT_FIELDS);
        move_fields(&mut settings, &mut persona_state, PERSONA_STATE_FIELDS);
        Self {
            core: settings,
            appearance,
            presets,
            layout,
            persona_state,
        }
    }

    pub(crate) fn into_settings(mut self) -> Value {
        // A selected section cannot overwrite fields owned by another section.
        move_fields(&mut self.appearance, &mut self.core, APPEARANCE_FIELDS);
        move_fields(&mut self.presets, &mut self.core, PRESET_FIELDS);
        move_fields(&mut self.layout, &mut self.core, LAYOUT_FIELDS);
        move_fields(
            &mut self.persona_state,
            &mut self.core,
            PERSONA_STATE_FIELDS,
        );
        self.core
    }
}

fn move_fields(source: &mut Value, target: &mut Value, groups: &[FieldGroup]) {
    for (parents, fields) in groups {
        let Some(source_object) = parents
            .iter()
            .try_fold(&mut *source, |value, key| value.get_mut(*key))
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        for field in *fields {
            let Some(value) = source_object.remove(*field) else {
                continue;
            };
            let mut target_parent = &mut *target;
            for parent in *parents {
                if !target_parent[*parent].is_object() {
                    target_parent[*parent] = json!({});
                }
                target_parent = &mut target_parent[*parent];
            }
            target_parent[*field] = value;
        }
    }
}

/// Look up a section value by its wire path.
pub(crate) fn section_value<'a>(
    sections: &'a UserSettingsSections,
    path: &str,
) -> Option<&'a Value> {
    match path {
        APPEARANCE_FILE => Some(&sections.appearance),
        PRESETS_FILE => Some(&sections.presets),
        LAYOUT_FILE => Some(&sections.layout),
        PERSONA_STATE_FILE => Some(&sections.persona_state),
        _ => None,
    }
}

/// Replace the section value for a wire path. Returns false for unknown paths.
pub(crate) fn set_section_value(
    sections: &mut UserSettingsSections,
    path: &str,
    value: Value,
) -> bool {
    match path {
        APPEARANCE_FILE => sections.appearance = value,
        PRESETS_FILE => sections.presets = value,
        LAYOUT_FILE => sections.layout = value,
        PERSONA_STATE_FILE => sections.persona_state = value,
        _ => return false,
    };
    true
}

/// Returns the wire path when `path` names a settings section file.
pub(crate) fn settings_section_path(path: &str) -> Option<&'static str> {
    SETTINGS_SECTION_PATHS
        .iter()
        .copied()
        .find(|item| *item == path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_into_settings_round_trips() {
        let settings = serde_json::json!({
            "background": "bg",
            "main_api": "openai",
            "username": "user",
            "firstRun": false,
            "power_user": {
                "theme": "Dark",
                "instruct": "w",
                "movingUI": true,
                "default_persona": "p",
                "allow_name1_display": true,
            },
            "world_info_settings": { "scan_depth": 4 },
        });

        let sections = UserSettingsSections::split(settings.clone());
        assert_eq!(
            sections.core,
            serde_json::json!({
                "power_user": { "allow_name1_display": true },
                "world_info_settings": { "scan_depth": 4 },
            })
        );
        assert_eq!(
            sections.appearance,
            serde_json::json!({
                "background": "bg",
                "power_user": { "theme": "Dark" },
            })
        );
        assert_eq!(
            sections.presets,
            serde_json::json!({
                "main_api": "openai",
                "power_user": { "instruct": "w" },
            })
        );
        assert_eq!(
            sections.layout,
            serde_json::json!({
                "firstRun": false,
                "power_user": { "movingUI": true },
            })
        );
        assert_eq!(
            sections.persona_state,
            serde_json::json!({
                "username": "user",
                "power_user": { "default_persona": "p" },
            })
        );

        assert_eq!(sections.into_settings(), settings);
    }

    #[test]
    fn split_keeps_unknown_fields_in_core() {
        let settings = serde_json::json!({
            "custom_field": 42,
            "power_user": { "unknown_key": "kept" },
        });
        let sections = UserSettingsSections::split(settings);
        assert_eq!(
            sections.core,
            serde_json::json!({ "custom_field": 42, "power_user": { "unknown_key": "kept" } })
        );
    }

    #[test]
    fn replacing_a_section_with_empty_removes_its_fields() {
        let settings = serde_json::json!({
            "background": "bg",
            "main_api": "openai",
            "power_user": { "theme": "Dark", "allow_name1_display": true },
        });
        let mut sections = UserSettingsSections::split(settings);
        assert!(set_section_value(&mut sections, APPEARANCE_FILE, json!({})));
        assert_eq!(
            sections.into_settings(),
            serde_json::json!({
                "main_api": "openai",
                "power_user": { "allow_name1_display": true },
            })
        );
    }
}
