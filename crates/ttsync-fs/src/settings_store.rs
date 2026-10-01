//! Settings storage format translation for SillyTavern-layout workspaces.
//!
//! SillyTavern keeps every user setting in a single `settings.json`, while the
//! TauriTavern client splits them into `settings/appearance.json`,
//! `settings/presets.json`, `settings/layout.json` and
//! `settings/persona-state.json`. When a TauriTavern client pushes to a
//! SillyTavern-layout TT-Sync server, raw file semantics would write dead split
//! files SillyTavern never reads, and overwrite `settings.json` with a
//! section-less core. This store translates both directions so pushes actually
//! take effect on SillyTavern and pulls present the split layout back.
//!
//! The section ownership table and the split/merge functions are ported
//! verbatim from TauriTavern (see `settings_sections`).
//!
//! Persona registry bridging (PNG text chunks vs. `settings.json` keys) lives
//! in the separate `persona_store` wrapping layer; this store never touches
//! persona data.
//!
//! Known boundaries:
//! - `settings.appearance` also contains `settings/dynamic-theme.json`, a
//!   TauriTavern-native file with no SillyTavern equivalent. It is not
//!   synthesized here, so a Mirror pull from a SillyTavern server deletes the
//!   local copy; use Incremental when this matters.
//! - Merging is a read-modify-write of `settings.json` without a lock.
//!   Concurrent pushes from multiple peers may lose a section update
//!   (last-write-wins per file); a single-user deployment is unaffected.
//! - A corrupt `settings.json` fails the whole operation with the path in the
//!   error instead of silently skipping the settings datasets (fail fast).

use std::io::Cursor;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::AsyncRead;
use ttsync_contract::manifest::{ManifestEntryV2, ManifestV2};
use ttsync_contract::path::SyncPath;
use ttsync_core::error::SyncError;
use ttsync_core::ports::ManifestStore;

use crate::layout::WorkspaceMounts;
use crate::settings_file::{read_settings_json, settings_modified_ms, write_settings_json};
use crate::settings_sections::{
    SETTINGS_CORE_FILE, SETTINGS_SECTION_PATHS, UserSettingsSections, section_value,
    set_section_value, settings_section_path,
};
use crate::writer::read_all;

/// How a workspace stores user settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsStyle {
    /// TauriTavern layout: settings are already split into per-section files.
    Split,
    /// SillyTavern layout: `settings.json` is the single superset file.
    Integrated,
}

/// A [`ManifestStore`] wrapper that translates settings sections when the
/// workspace uses the integrated SillyTavern layout. For `Split` style it is a
/// pure pass-through.
#[derive(Debug)]
pub struct SettingsTranslatingStore<M> {
    inner: Arc<M>,
    mounts: WorkspaceMounts,
    style: SettingsStyle,
}

impl<M> SettingsTranslatingStore<M> {
    pub fn new(inner: Arc<M>, mounts: WorkspaceMounts, style: SettingsStyle) -> Self {
        Self {
            inner,
            mounts,
            style,
        }
    }

    fn is_integrated(&self) -> bool {
        self.style == SettingsStyle::Integrated
    }
}

impl<M> ManifestStore for SettingsTranslatingStore<M>
where
    M: ManifestStore + 'static,
{
    async fn prepare(
        self: Arc<Self>,
        policy: ttsync_core::dataset::ResolvedDatasetPolicy,
        receiving: bool,
    ) -> Result<Arc<Self>, SyncError> {
        let inner = self.inner.clone().prepare(policy, receiving).await?;
        Ok(Arc::new(Self {
            inner,
            mounts: self.mounts.clone(),
            style: self.style,
        }))
    }

    async fn commit(self: Arc<Self>) -> Result<(), SyncError> {
        self.inner.clone().commit().await
    }

    fn set_plan(&self, plan: &ttsync_contract::plan::SyncPlan) {
        self.inner.set_plan(plan);
    }

    async fn scan(
        &self,
        policy: ttsync_core::dataset::ResolvedDatasetPolicy,
    ) -> Result<ManifestV2, SyncError> {
        let mut manifest = self.inner.scan(policy).await?;
        if !self.is_integrated() {
            return Ok(manifest);
        }

        // Drop physical (dead) section files: they are synthesized from settings.json.
        manifest
            .entries
            .retain(|entry| settings_section_path(entry.path.as_str()).is_none());

        let Some(settings) = read_settings_json(&self.mounts).await? else {
            return Ok(manifest);
        };
        let sections = UserSettingsSections::split(settings);
        let modified_ms = settings_modified_ms(&self.mounts).await?;

        // Advertise settings.json as its core-only view (split removes the
        // section fields), so an incremental diff against the TauriTavern
        // client's core-only settings.json converges instead of re-transferring
        // the merged file on every push.
        let core_bytes = serde_json::to_vec_pretty(&sections.core)
            .map_err(|e| SyncError::InvalidData(format!("serialize settings core: {e}")))?;
        if let Some(entry) = manifest
            .entries
            .iter_mut()
            .find(|entry| entry.path.as_str() == SETTINGS_CORE_FILE)
        {
            entry.size_bytes = core_bytes.len() as u64;
        } else {
            manifest.entries.push(ManifestEntryV2 {
                path: SyncPath::new(SETTINGS_CORE_FILE).expect("settings core path is valid"),
                size_bytes: core_bytes.len() as u64,
                modified_ms,
                content_hash: None,
            });
        }

        for path in SETTINGS_SECTION_PATHS {
            let section = section_value(&sections, path).expect("settings section path");
            if section.as_object().is_none_or(|fields| fields.is_empty()) {
                continue;
            }
            let bytes = serde_json::to_vec_pretty(section)
                .map_err(|e| SyncError::InvalidData(format!("serialize settings section: {e}")))?;
            manifest.entries.push(ManifestEntryV2 {
                path: SyncPath::new(path).expect("settings section path is valid"),
                size_bytes: bytes.len() as u64,
                modified_ms,
                content_hash: None,
            });
        }

        manifest
            .entries
            .sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
        Ok(manifest)
    }

    async fn read_file(
        &self,
        path: &SyncPath,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>, SyncError> {
        if self.is_integrated()
            && let Some(section_path) = settings_section_path(path.as_str())
        {
            let settings = read_settings_json(&self.mounts)
                .await?
                .ok_or_else(|| SyncError::NotFound("settings.json not found".into()))?;
            let sections = UserSettingsSections::split(settings);
            let section = section_value(&sections, section_path)
                .expect("settings section path")
                .clone();
            let bytes = serde_json::to_vec_pretty(&section)
                .map_err(|e| SyncError::InvalidData(format!("serialize settings section: {e}")))?;
            return Ok(Box::new(Cursor::new(bytes)));
        }

        if self.is_integrated() && path.as_str() == SETTINGS_CORE_FILE {
            let settings = read_settings_json(&self.mounts)
                .await?
                .ok_or_else(|| SyncError::NotFound("settings.json not found".into()))?;
            let sections = UserSettingsSections::split(settings);
            let bytes = serde_json::to_vec_pretty(&sections.core)
                .map_err(|e| SyncError::InvalidData(format!("serialize settings core: {e}")))?;
            return Ok(Box::new(Cursor::new(bytes)));
        }

        self.inner.read_file(path).await
    }

    async fn write_file(
        &self,
        path: &SyncPath,
        data: &mut (dyn AsyncRead + Send + Unpin),
        modified_ms: u64,
    ) -> Result<(), SyncError> {
        if self.is_integrated()
            && let Some(section_path) = settings_section_path(path.as_str())
        {
            let incoming = read_all(data).await?;
            let incoming = serde_json::from_slice(&incoming)
                .map_err(|e| SyncError::InvalidData(format!("invalid settings section: {e}")))?;
            let existing = read_settings_json(&self.mounts)
                .await?
                .unwrap_or_else(|| json!({}));
            let mut sections = UserSettingsSections::split(existing);
            set_section_value(&mut sections, section_path, incoming);
            let merged = sections.into_settings();
            return write_settings_json(&self.mounts, &merged, modified_ms).await;
        }

        if self.is_integrated() && path.as_str() == SETTINGS_CORE_FILE {
            let incoming = read_all(data).await?;
            let incoming_core: Value = serde_json::from_slice(&incoming)
                .map_err(|e| SyncError::InvalidData(format!("invalid settings.json: {e}")))?;
            let existing = read_settings_json(&self.mounts)
                .await?
                .unwrap_or_else(|| json!({}));
            let existing_sections = UserSettingsSections::split(existing);
            // The incoming core owns the core fields; sections not present in
            // it are preserved from the existing merged file. Persona keys
            // travel with the core untouched — the persona bridging layer
            // owns their reconciliation.
            let merged = UserSettingsSections {
                core: incoming_core,
                appearance: existing_sections.appearance,
                presets: existing_sections.presets,
                layout: existing_sections.layout,
                persona_state: existing_sections.persona_state,
            }
            .into_settings();
            return write_settings_json(&self.mounts, &merged, modified_ms).await;
        }

        self.inner.write_file(path, data, modified_ms).await
    }

    async fn delete_file(&self, path: &SyncPath) -> Result<(), SyncError> {
        if self.is_integrated()
            && let Some(section_path) = settings_section_path(path.as_str())
        {
            let Some(existing) = read_settings_json(&self.mounts).await? else {
                return Ok(());
            };
            let modified_ms = settings_modified_ms(&self.mounts).await?;
            let mut sections = UserSettingsSections::split(existing);
            set_section_value(&mut sections, section_path, json!({}));
            let merged = sections.into_settings();
            return write_settings_json(&self.mounts, &merged, modified_ms).await;
        }

        self.inner.delete_file(path).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ttsync_contract::path::SyncPath;

    use crate::persona_bridge;
    use crate::test_support::{
        AVATAR_WIRE_PATH, avatar_disk_path, mounts, png_with_card, read_bytes, settings_policy,
        unique_temp_root,
    };

    use super::*;

    fn integrated_store(
        root: &std::path::Path,
    ) -> SettingsTranslatingStore<crate::manifest_store::FsManifestStore> {
        SettingsTranslatingStore::new(
            Arc::new(crate::manifest_store::FsManifestStore::new(mounts(root))),
            mounts(root),
            SettingsStyle::Integrated,
        )
    }

    #[tokio::test]
    async fn push_section_merges_into_settings_json_without_dead_file() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(&user).unwrap();
        let settings_path = user.join("settings.json");
        std::fs::write(
            &settings_path,
            br#"{"background":"old","main_api":"openai","power_user":{"theme":"old","allow_name1_display":true}}"#,
        )
        .unwrap();

        let store = integrated_store(&root);
        store
            .write_file(
                &SyncPath::new("default-user/settings/appearance.json").unwrap(),
                &mut std::io::Cursor::new(
                    br#"{"background":"new","power_user":{"theme":"dark"}}"#.to_vec(),
                ),
                500,
            )
            .await
            .unwrap();

        let merged: Value =
            serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        assert_eq!(
            merged,
            json!({
                "main_api": "openai",
                "power_user": { "allow_name1_display": true, "theme": "dark" },
                "background": "new",
            })
        );
        assert!(!user.join("settings/appearance.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn push_core_preserves_existing_sections() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(&user).unwrap();
        let settings_path = user.join("settings.json");
        std::fs::write(
            &settings_path,
            br#"{"background":"bg","main_api":"openai","power_user":{"theme":"dark","aux_field":"x"}}"#,
        )
        .unwrap();

        let store = integrated_store(&root);
        // TauriTavern cores never carry section fields; they are split out.
        store
            .write_file(
                &SyncPath::new("default-user/settings.json").unwrap(),
                &mut std::io::Cursor::new(
                    br#"{"world_info_settings":{"scan_depth":4},"power_user":{"allow_name1_display":true}}"#
                        .to_vec(),
                ),
                600,
            )
            .await
            .unwrap();

        let merged: Value =
            serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        assert_eq!(
            merged,
            json!({
                "main_api": "openai",
                "world_info_settings": { "scan_depth": 4 },
                "power_user": { "allow_name1_display": true, "theme": "dark", "aux_field": "x" },
                "background": "bg",
            })
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn scan_synthesizes_sections_and_reads_them() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(user.join("settings")).unwrap();
        std::fs::write(
            user.join("settings.json"),
            br#"{"background":"bg","main_api":"openai","power_user":{"theme":"dark","instruct":"w","movingUI":true,"allow_name1_display":true}}"#,
        )
        .unwrap();
        // Dead split file that must be ignored in Integrated mode.
        std::fs::write(
            user.join("settings/appearance.json"),
            br#"{"background":"STALE"}"#,
        )
        .unwrap();

        let store = integrated_store(&root);
        let manifest = store.scan(settings_policy()).await.unwrap();
        let paths: Vec<String> = manifest
            .entries
            .iter()
            .map(|entry| entry.path.to_string())
            .collect();
        assert!(paths.contains(&"default-user/settings.json".to_owned()));
        assert!(paths.contains(&"default-user/settings/appearance.json".to_owned()));
        assert!(paths.contains(&"default-user/settings/presets.json".to_owned()));
        assert!(paths.contains(&"default-user/settings/layout.json".to_owned()));

        // The section is synthesized from settings.json, not the stale dead file.
        let appearance: Value = serde_json::from_slice(
            &read_bytes(&store, "default-user/settings/appearance.json").await,
        )
        .unwrap();
        assert_eq!(
            appearance,
            json!({ "background": "bg", "power_user": { "theme": "dark" } })
        );

        let presets: Value =
            serde_json::from_slice(&read_bytes(&store, "default-user/settings/presets.json").await)
                .unwrap();
        assert_eq!(
            presets,
            json!({ "main_api": "openai", "power_user": { "instruct": "w" } })
        );

        // settings.json is exposed as its core-only view, not the merged file.
        let core: Value =
            serde_json::from_slice(&read_bytes(&store, "default-user/settings.json").await)
                .unwrap();
        assert_eq!(
            core,
            json!({ "power_user": { "allow_name1_display": true } })
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn scan_advertises_core_view_so_incremental_diff_converges() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(
            user.join("settings.json"),
            br#"{"background":"bg","main_api":"openai","power_user":{"theme":"dark","allow_name1_display":true}}"#,
        )
        .unwrap();

        let store = integrated_store(&root);
        // TauriTavern pushes its core-only settings.json, then a section file.
        store
            .write_file(
                &SyncPath::new("default-user/settings.json").unwrap(),
                &mut std::io::Cursor::new(
                    br#"{"power_user":{"allow_name1_display":true}}"#.to_vec(),
                ),
                600,
            )
            .await
            .unwrap();
        store
            .write_file(
                &SyncPath::new("default-user/settings/appearance.json").unwrap(),
                &mut std::io::Cursor::new(
                    br#"{"background":"new","power_user":{"theme":"dark"}}"#.to_vec(),
                ),
                700,
            )
            .await
            .unwrap();

        let manifest = store.scan(settings_policy()).await.unwrap();
        let settings_entry = manifest
            .entries
            .iter()
            .find(|entry| entry.path.as_str() == "default-user/settings.json")
            .expect("settings.json entry");
        // The advertised view equals the client's core-only file (same size and
        // mtime after the section push), so the next push skips settings.json.
        let core_bytes = serde_json::to_vec_pretty(&json!({
            "power_user": { "allow_name1_display": true }
        }))
        .unwrap();
        assert_eq!(settings_entry.size_bytes as usize, core_bytes.len());
        assert_eq!(settings_entry.modified_ms, 700);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn delete_section_removes_fields_from_settings_json() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(&user).unwrap();
        let settings_path = user.join("settings.json");
        std::fs::write(
            &settings_path,
            br#"{"background":"bg","main_api":"openai","power_user":{"theme":"dark","allow_name1_display":true}}"#,
        )
        .unwrap();

        let store = integrated_store(&root);
        store
            .delete_file(&SyncPath::new("default-user/settings/appearance.json").unwrap())
            .await
            .unwrap();

        let merged: Value =
            serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        assert_eq!(
            merged,
            json!({ "main_api": "openai", "power_user": { "allow_name1_display": true } })
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn split_style_writes_section_files_verbatim() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(&user).unwrap();

        let store = SettingsTranslatingStore::new(
            Arc::new(crate::manifest_store::FsManifestStore::new(mounts(&root))),
            mounts(&root),
            SettingsStyle::Split,
        );
        store
            .write_file(
                &SyncPath::new("default-user/settings/appearance.json").unwrap(),
                &mut std::io::Cursor::new(br#"{"background":"bg"}"#.to_vec()),
                100,
            )
            .await
            .unwrap();

        assert!(user.join("settings/appearance.json").exists());
        assert!(!user.join("settings.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn split_style_passes_persona_png_through_without_touching_settings() {
        // The TauriTavern-layout workspace is served by this store in Split
        // style with no persona layer installed: chunk-bearing avatar bytes
        // pass through verbatim and no settings.json translation happens.
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(&user).unwrap();
        let card_png = png_with_card("甲", "甲设定");

        let store = SettingsTranslatingStore::new(
            Arc::new(crate::manifest_store::FsManifestStore::new(mounts(&root))),
            mounts(&root),
            SettingsStyle::Split,
        );
        store
            .write_file(
                &SyncPath::new(AVATAR_WIRE_PATH).unwrap(),
                &mut std::io::Cursor::new(card_png.clone()),
                100,
            )
            .await
            .unwrap();

        assert_eq!(std::fs::read(avatar_disk_path(&root)).unwrap(), card_png);
        assert!(!user.join("settings.json").exists());
        let bytes = read_bytes(&store, AVATAR_WIRE_PATH).await;
        assert_eq!(bytes, card_png);
        // The chunk survives the pass-through untouched.
        let card = persona_bridge::extract_persona(&bytes)
            .unwrap()
            .expect("card survives");
        assert_eq!(card.name.as_deref(), Some("甲"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
