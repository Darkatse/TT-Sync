//! Persona bridging store for SillyTavern-layout (Integrated) workspaces.
//!
//! TauriTavern keeps persona names/descriptions inside `User Avatars/*.png`
//! text chunks and migrates the corresponding `settings.json` keys away on
//! startup, while SillyTavern stores them only in `settings.json`. This
//! wrapper translates between the two representations so a sync can never
//! overwrite a real card with whatever SillyTavern currently holds
//! (historically, synthesized `[Unnamed Persona]` shells):
//!
//! - Pulls (SillyTavern -> TauriTavern): the `settings.json` core view is
//!   served with the persona registry stripped, and avatar PNGs are served
//!   with the settings-side persona card injected into the image bytes.
//! - Pushes (TauriTavern -> SillyTavern): the chunk-bearing avatar bytes land
//!   on disk verbatim and the card is reprojected into `settings.json`; an
//!   avatar deletion drops the matching persona entry.
//!
//! The layer knows nothing about the split settings section translation and
//! can wrap any [`ManifestStore`]. In production it wraps the section
//! translating store on Integrated workspaces; TauriTavern-layout workspaces
//! are not wrapped at all.
//!
//! Known boundaries:
//! - Persona cards can only be embedded in PNG bytes. A persona whose avatar
//!   is a JPEG/WebP/GIF cannot be bridged through the image channel and stays
//!   SillyTavern-only until the avatar is re-uploaded as PNG.
//! - Reprojection is a lock-free read-modify-write of `settings.json`; a
//!   single-user deployment is unaffected (last-write-wins under contention).

use std::io::Cursor;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tokio::io::AsyncRead;
use ttsync_contract::manifest::ManifestV2;
use ttsync_contract::path::SyncPath;
use ttsync_core::error::SyncError;
use ttsync_core::ports::ManifestStore;

use crate::layout::WorkspaceMounts;
use crate::persona_bridge::{self, AVATARS_WIRE_PREFIX, OnDiskPersona, PersonaRegistry};
use crate::settings_file::{
    SETTINGS_JSON_WIRE_PATH, read_settings_json, settings_modified_ms, write_settings_json,
};
use crate::writer::read_all;

/// [`ManifestStore`] wrapper that bridges the persona registry between
/// `settings.json` and avatar PNG chunks.
#[derive(Debug)]
pub struct PersonaBridgingStore<M> {
    inner: Arc<M>,
    mounts: WorkspaceMounts,
}

impl<M: ManifestStore> PersonaBridgingStore<M> {
    pub fn new(inner: Arc<M>, mounts: WorkspaceMounts) -> Self {
        Self { inner, mounts }
    }

    /// Return the avatar file name for a wire path inside
    /// `default-user/User Avatars/`, or `None` for anything else. Only
    /// single-segment names pass, so traversal sequences cannot escape the
    /// avatar directory.
    fn avatar_file_name(path: &str) -> Option<String> {
        let rest = path.strip_prefix(AVATARS_WIRE_PREFIX)?;
        if rest.is_empty() || rest.contains(['/', '\\']) || rest == "." || rest == ".." {
            return None;
        }
        Some(rest.to_owned())
    }

    fn avatars_dir(&self) -> std::path::PathBuf {
        self.mounts.default_user_root.join("User Avatars")
    }

    /// The `settings.json` bytes served towards TauriTavern: the same view the
    /// inner store serves, minus the persona registry keys. `None` means the
    /// inner store has no settings file at all.
    async fn stripped_core_view(&self) -> Result<Option<Vec<u8>>, SyncError> {
        let path = SyncPath::new(SETTINGS_JSON_WIRE_PATH).expect("settings core path is valid");
        let mut reader = match self.inner.read_file(&path).await {
            Ok(reader) => reader,
            Err(SyncError::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let bytes = read_all(&mut reader).await?;
        let mut value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| SyncError::InvalidData(format!("invalid settings.json: {e}")))?;
        persona_bridge::take_persona_registry(&mut value).map_err(SyncError::InvalidData)?;
        serde_json::to_vec_pretty(&value)
            .map(Some)
            .map_err(|e| SyncError::InvalidData(format!("serialize settings core: {e}")))
    }

    /// Enumerate regular files in the avatar directory together with their
    /// embedded persona cards and modification times.
    async fn list_avatar_personas(&self) -> Result<Vec<OnDiskPersona>, SyncError> {
        let dir = self.avatars_dir();
        let mut reader = match tokio::fs::read_dir(&dir).await {
            Ok(reader) => reader,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(SyncError::Io(e.to_string())),
        };
        let mut items = Vec::new();
        while let Some(entry) = reader
            .next_entry()
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?
        {
            // Metadata/IO failures must surface: silently dropping a file here
            // would make reconcile treat its persona as deleted. The only
            // expected recovery is the file disappearing between read_dir and
            // open (a concurrent delete), which NotFound covers explicitly.
            let metadata = entry
                .metadata()
                .await
                .map_err(|e| SyncError::Io(e.to_string()))?;
            if !metadata.is_file() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            let modified_ms = metadata
                .modified()
                .map_err(|e| SyncError::Io(e.to_string()))?
                .duration_since(UNIX_EPOCH)
                .map_err(|e| SyncError::Internal(e.to_string()))?
                .as_millis() as u64;
            let bytes = match tokio::fs::read(entry.path()).await {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(SyncError::Io(e.to_string())),
            };
            let card = persona_bridge::extract_persona(&bytes).map_err(SyncError::InvalidData)?;
            items.push(OnDiskPersona {
                id,
                card,
                modified_ms,
            });
        }
        Ok(items)
    }

    /// Rebuild `settings.json`'s persona registry from the current settings
    /// values and the avatar cards on disk. Called after an avatar write or
    /// delete (TauriTavern -> SillyTavern direction) so SillyTavern keeps
    /// reading correct names/descriptions and never synthesizes
    /// `[Unnamed Persona]` shells. The file is only rewritten when the
    /// registry actually changed; its mtime never moves backwards.
    async fn reproject_personas(&self, trigger_modified_ms: u64) -> Result<(), SyncError> {
        let Some(existing) = read_settings_json(&self.mounts).await? else {
            return Ok(());
        };
        let existing_registry =
            persona_bridge::read_persona_registry(&existing).map_err(SyncError::InvalidData)?;
        let settings_modified_ms = settings_modified_ms(&self.mounts).await?;
        let on_disk = self.list_avatar_personas().await?;
        let reconciled = persona_bridge::reconcile_persona_registry(
            &existing_registry,
            on_disk,
            settings_modified_ms,
        );

        let mut merged = existing.clone();
        persona_bridge::insert_persona_registry(&mut merged, &reconciled);
        if merged == existing {
            return Ok(());
        }
        let new_modified_ms = settings_modified_ms.max(trigger_modified_ms);
        write_settings_json(&self.mounts, &merged, new_modified_ms).await
    }

    /// Inject the settings-side persona card into PNG avatar bytes served to
    /// TauriTavern. Non-PNG avatars and ids absent from the registry pass
    /// through unchanged (a disk-resident card remains visible that way).
    async fn persona_view_bytes(&self, path: &str, bytes: Vec<u8>) -> Result<Vec<u8>, SyncError> {
        if !persona_bridge::is_png(&bytes) {
            return Ok(bytes);
        }
        let Some(name) = Self::avatar_file_name(path) else {
            return Ok(bytes);
        };
        let Some(settings) = read_settings_json(&self.mounts).await? else {
            return Ok(bytes);
        };
        let registry: PersonaRegistry =
            persona_bridge::read_persona_registry(&settings).map_err(SyncError::InvalidData)?;
        match registry.get(&name) {
            // Shells are "no data": pass the disk bytes through so an existing
            // real card in the PNG is not overwritten.
            Some(card) if !persona_bridge::is_unnamed_shell(card) => {
                persona_bridge::inject_persona(&bytes, card).map_err(SyncError::InvalidData)
            }
            _ => Ok(bytes),
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }
}

impl<M> ManifestStore for PersonaBridgingStore<M>
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

        // Advertise the persona-stripped core view with its real synthesized
        // size, so an incremental diff against TauriTavern's persona-less
        // settings.json converges instead of re-transferring forever.
        if let Some(core_bytes) = self.stripped_core_view().await?
            && let Some(entry) = manifest
                .entries
                .iter_mut()
                .find(|entry| entry.path.as_str() == SETTINGS_JSON_WIRE_PATH)
        {
            entry.size_bytes = core_bytes.len() as u64;
        }

        // Advertise avatar PNGs with the persona card injected, so the
        // incremental diff transfers exactly the bytes read_file would serve
        // (size must match or the file re-transfers forever). The advertised
        // mtime also tracks settings.json, so editing a persona description in
        // SillyTavern marks the avatar changed even though the image bytes on
        // disk are untouched.
        let settings_for_avatars = read_settings_json(&self.mounts).await?;
        let persona_registry = match &settings_for_avatars {
            Some(settings) => {
                persona_bridge::read_persona_registry(settings).map_err(SyncError::InvalidData)?
            }
            None => PersonaRegistry::new(),
        };
        // Only an existing settings.json can contribute injection mtimes;
        // without it every registry lookup misses and the stat would race.
        let settings_modified_ms = if settings_for_avatars.is_some() {
            Some(settings_modified_ms(&self.mounts).await?)
        } else {
            None
        };

        for entry in &mut manifest.entries {
            let Some(name) = Self::avatar_file_name(entry.path.as_str()) else {
                continue;
            };
            let disk_path = self.avatars_dir().join(&name);
            // A concurrent delete between scan listing and open is the only
            // silent case; other read errors fail the scan.
            let disk_bytes = match tokio::fs::read(&disk_path).await {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(SyncError::Io(e.to_string())),
            };
            if !persona_bridge::is_png(&disk_bytes) {
                continue;
            }
            let real_card = persona_registry
                .get(&name)
                .filter(|card| !persona_bridge::is_unnamed_shell(card));
            let (view_bytes, injected) = match real_card {
                Some(card) => (
                    persona_bridge::inject_persona(&disk_bytes, card)
                        .map_err(SyncError::InvalidData)?,
                    true,
                ),
                // No registry entry, or an `[Unnamed Persona]` shell: serve
                // the disk bytes as-is, preserving any real card they carry.
                None => (disk_bytes, false),
            };
            entry.size_bytes = view_bytes.len() as u64;
            // Only an injected view depends on settings.json; a pure disk
            // passthrough keeps the image mtime so identical images converge.
            if injected && let Some(modified_ms) = settings_modified_ms {
                entry.modified_ms = entry.modified_ms.max(modified_ms);
            }
        }

        Ok(manifest)
    }

    async fn read_file(
        &self,
        path: &SyncPath,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>, SyncError> {
        if path.as_str() == SETTINGS_JSON_WIRE_PATH {
            let bytes = self
                .stripped_core_view()
                .await?
                .ok_or_else(|| SyncError::NotFound("settings.json not found".into()))?;
            return Ok(Box::new(Cursor::new(bytes)));
        }

        // Avatar pull (SillyTavern -> TauriTavern): serve PNG bytes with the
        // settings-side persona card injected into the image.
        if Self::avatar_file_name(path.as_str()).is_some() {
            let mut reader = self.inner.read_file(path).await?;
            let bytes = read_all(&mut reader).await?;
            let view = self.persona_view_bytes(path.as_str(), bytes).await?;
            return Ok(Box::new(Cursor::new(view)));
        }

        self.inner.read_file(path).await
    }

    async fn write_file(
        &self,
        path: &SyncPath,
        data: &mut (dyn AsyncRead + Send + Unpin),
        modified_ms: u64,
    ) -> Result<(), SyncError> {
        if path.as_str() == SETTINGS_JSON_WIRE_PATH {
            let incoming = read_all(data).await?;
            let mut incoming_core: Value = serde_json::from_slice(&incoming)
                .map_err(|e| SyncError::InvalidData(format!("invalid settings.json: {e}")))?;
            // TauriTavern cores carry no persona keys; another Integrated peer
            // might. Lift whatever is present before forwarding the core.
            let incoming_registry = persona_bridge::take_persona_registry(&mut incoming_core)
                .map_err(SyncError::InvalidData)?;

            // Rebuild the registry from the physical settings.json and the
            // surviving avatar cards (mtime decides card-vs-settings
            // conflicts), then overlay ids the incoming core explicitly
            // carried. The enriched core is forwarded to the inner store,
            // which owns any other translation (e.g. section merging).
            let existing_opt = read_settings_json(&self.mounts).await?;
            // No settings.json yet: there is no existing side in the mtime
            // race, so 0 correctly makes every on-disk card the newer value.
            let settings_modified_ms = match &existing_opt {
                Some(_) => settings_modified_ms(&self.mounts).await?,
                None => 0,
            };
            let existing = existing_opt.unwrap_or_else(|| serde_json::json!({}));
            let existing_registry =
                persona_bridge::read_persona_registry(&existing).map_err(SyncError::InvalidData)?;
            let on_disk = self.list_avatar_personas().await?;
            let mut registry = persona_bridge::reconcile_persona_registry(
                &existing_registry,
                on_disk,
                settings_modified_ms,
            );
            for (id, card) in incoming_registry {
                registry.insert(id, card);
            }
            persona_bridge::insert_persona_registry(&mut incoming_core, &registry);

            let bytes = serde_json::to_vec_pretty(&incoming_core)
                .map_err(|e| SyncError::InvalidData(format!("serialize settings.json: {e}")))?;
            return self
                .inner
                .write_file(path, &mut Cursor::new(bytes), modified_ms)
                .await;
        }

        // Avatar push (TauriTavern -> SillyTavern): keep the original bytes on
        // disk (SillyTavern ignores the chunk, but it is the bridge's data
        // source), then project the card back into settings.json.
        if Self::avatar_file_name(path.as_str()).is_some() {
            let incoming = read_all(data).await?;
            self.inner
                .write_file(path, &mut Cursor::new(incoming.clone()), modified_ms)
                .await?;
            if persona_bridge::is_png(&incoming) {
                self.reproject_personas(modified_ms).await?;
            }
            return Ok(());
        }

        self.inner.write_file(path, data, modified_ms).await
    }

    async fn delete_file(&self, path: &SyncPath) -> Result<(), SyncError> {
        // Avatar deletion: remove the file first, then drop the persona entry
        // from settings.json (reconcile keeps entries for existing files only).
        if Self::avatar_file_name(path.as_str()).is_some() {
            self.inner.delete_file(path).await?;
            self.reproject_personas(Self::now_ms()).await?;
            return Ok(());
        }

        self.inner.delete_file(path).await
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::{Value, json};
    use ttsync_contract::manifest::ManifestV2;
    use ttsync_contract::path::SyncPath;

    use crate::manifest_store::FsManifestStore;
    use crate::persona_bridge::{self, PersonaCard};
    use crate::settings_store::{SettingsStyle, SettingsTranslatingStore};
    use crate::test_support::{
        AVATAR_WIRE_PATH, avatar_disk_path, avatar_policy, future_ms, mounts, plain_png,
        png_with_card, read_bytes, unique_temp_root,
    };

    use super::*;

    /// The persona layer over a plain filesystem store: proves the bridge is
    /// self-contained and does not depend on the section translation layer.
    fn persona_store(root: &std::path::Path) -> PersonaBridgingStore<FsManifestStore> {
        PersonaBridgingStore::new(Arc::new(FsManifestStore::new(mounts(root))), mounts(root))
    }

    /// Production Integrated composition: persona bridging outside section
    /// translation outside the raw filesystem store.
    fn composed_store(
        root: &std::path::Path,
    ) -> PersonaBridgingStore<SettingsTranslatingStore<FsManifestStore>> {
        let translating = SettingsTranslatingStore::new(
            Arc::new(FsManifestStore::new(mounts(root))),
            mounts(root),
            SettingsStyle::Integrated,
        );
        PersonaBridgingStore::new(Arc::new(translating), mounts(root))
    }

    #[tokio::test]
    async fn pull_injects_settings_persona_into_avatar_and_strips_core() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(user.join("User Avatars")).unwrap();
        std::fs::write(
            user.join("settings.json"),
            r#"{"power_user":{"allow_name1_display":true,"personas":{"p1.png":"十一月雨"},"persona_descriptions":{"p1.png":{"description":"男高中生","position":0}}}}"#,
        )
        .unwrap();
        std::fs::write(avatar_disk_path(&root), plain_png()).unwrap();

        let store = persona_store(&root);

        // The avatar bytes served to TauriTavern carry the persona card.
        let bytes = read_bytes(&store, AVATAR_WIRE_PATH).await;
        let card = persona_bridge::extract_persona(&bytes)
            .unwrap()
            .expect("injected card");
        assert_eq!(card.name.as_deref(), Some("十一月雨"));
        assert_eq!(
            card.description.expect("description")["description"],
            json!("男高中生")
        );
        // Injection is synthesized: the physical PNG on disk stays chunk-free.
        let disk = std::fs::read(avatar_disk_path(&root)).unwrap();
        assert!(persona_bridge::extract_persona(&disk).unwrap().is_none());

        // The core view never carries the persona registry.
        let core: Value =
            serde_json::from_slice(&read_bytes(&store, "default-user/settings.json").await)
                .unwrap();
        assert!(core["power_user"].get("personas").is_none());
        assert!(core["power_user"].get("persona_descriptions").is_none());
        assert_eq!(core["power_user"]["allow_name1_display"], json!(true));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn scan_advertises_injected_avatar_size_and_tracks_settings_edits() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(user.join("User Avatars")).unwrap();
        let settings_path = user.join("settings.json");
        std::fs::write(
            &settings_path,
            r#"{"power_user":{"personas":{"p1.png":"旧名"},"persona_descriptions":{"p1.png":{"description":"旧描述","position":0}}}}"#,
        )
        .unwrap();
        std::fs::write(avatar_disk_path(&root), plain_png()).unwrap();

        let store = persona_store(&root);
        let expected_card = PersonaCard::new(
            Some("旧名".to_owned()),
            Some(json!({ "description": "旧描述", "position": 0 })),
        );
        let expected_bytes = persona_bridge::inject_persona(&plain_png(), &expected_card).unwrap();

        let find_avatar = |manifest: &ManifestV2| {
            manifest
                .entries
                .iter()
                .find(|entry| entry.path.as_str() == AVATAR_WIRE_PATH)
                .expect("avatar entry")
                .clone()
        };

        let entry = find_avatar(&store.scan(avatar_policy()).await.unwrap());
        assert_eq!(entry.size_bytes as usize, expected_bytes.len());
        // Re-scanning is stable: the diff converges instead of re-transferring.
        let entry_again = find_avatar(&store.scan(avatar_policy()).await.unwrap());
        assert_eq!(entry_again.size_bytes, entry.size_bytes);
        assert_eq!(entry_again.modified_ms, entry.modified_ms);

        // Editing the persona in SillyTavern changes the advertised bytes even
        // though the physical image is untouched.
        std::fs::write(
            &settings_path,
            r#"{"power_user":{"personas":{"p1.png":"十一月雨-加长版"},"persona_descriptions":{"p1.png":{"description":"这是加长后的新描述文本","position":0}}}}"#,
        )
        .unwrap();
        let edited = find_avatar(&store.scan(avatar_policy()).await.unwrap());
        assert_ne!(edited.size_bytes, entry.size_bytes);
        let served = read_bytes(&store, AVATAR_WIRE_PATH).await;
        assert_eq!(served.len(), edited.size_bytes as usize);
        let card = persona_bridge::extract_persona(&served).unwrap().unwrap();
        assert_eq!(card.name.as_deref(), Some("十一月雨-加长版"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn push_tt_avatar_keeps_chunk_bytes_and_projects_settings() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(&user).unwrap();
        let settings_path = user.join("settings.json");
        std::fs::write(
            &settings_path,
            br#"{"main_api":"openai","power_user":{"theme":"dark"}}"#,
        )
        .unwrap();
        let card_png = png_with_card("克莱因", "空描述位");

        let store = persona_store(&root);
        store
            .write_file(
                &SyncPath::new(AVATAR_WIRE_PATH).unwrap(),
                &mut Cursor::new(card_png.clone()),
                future_ms(60),
            )
            .await
            .unwrap();

        // Original chunk-bearing bytes land on disk verbatim.
        assert_eq!(std::fs::read(avatar_disk_path(&root)).unwrap(), card_png);

        // The card is projected into settings.json; unrelated settings survive.
        let merged: Value =
            serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        assert_eq!(merged["main_api"], json!("openai"));
        assert_eq!(merged["power_user"]["theme"], json!("dark"));
        assert_eq!(merged["power_user"]["personas"]["p1.png"], json!("克莱因"));
        assert_eq!(
            merged["power_user"]["persona_descriptions"]["p1.png"]["description"],
            json!("空描述位")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn push_core_keeps_personas_for_existing_avatars_and_drops_deleted() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(user.join("User Avatars")).unwrap();
        let settings_path = user.join("settings.json");
        std::fs::write(
            &settings_path,
            r#"{"power_user":{"personas":{"p1.png":"甲","gone.png":"乙"},"persona_descriptions":{"p1.png":{"description":"甲设定","position":0},"gone.png":{"description":"乙设定","position":0}}}}"#,
        )
        .unwrap();
        // p1.png survives on disk (with a card), gone.png was already deleted.
        std::fs::write(avatar_disk_path(&root), png_with_card("甲", "甲设定")).unwrap();

        let store = persona_store(&root);
        // TauriTavern pushes a persona-less core (its native format).
        store
            .write_file(
                &SyncPath::new("default-user/settings.json").unwrap(),
                &mut Cursor::new(
                    br#"{"main_api":"openai","power_user":{"allow_name1_display":true}}"#.to_vec(),
                ),
                future_ms(180),
            )
            .await
            .unwrap();

        let merged: Value =
            serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        let personas = merged["power_user"]["personas"]
            .as_object()
            .expect("personas map");
        assert!(
            personas.contains_key("p1.png"),
            "surviving avatar keeps entry"
        );
        assert!(
            !personas.contains_key("gone.png"),
            "deleted avatar drops entry"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn delete_avatar_removes_its_persona_entry() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(user.join("User Avatars")).unwrap();
        let settings_path = user.join("settings.json");
        std::fs::write(
            &settings_path,
            r#"{"power_user":{"personas":{"p1.png":"甲"},"persona_descriptions":{"p1.png":{"description":"甲设定","position":0}}}}"#,
        )
        .unwrap();
        std::fs::write(avatar_disk_path(&root), png_with_card("甲", "甲设定")).unwrap();

        let store = persona_store(&root);
        store
            .delete_file(&SyncPath::new(AVATAR_WIRE_PATH).unwrap())
            .await
            .unwrap();

        assert!(!avatar_disk_path(&root).exists());
        let merged: Value =
            serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        assert!(
            merged["power_user"]["personas"]
                .as_object()
                .is_none_or(|map| !map.contains_key("p1.png"))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn pull_passes_disk_card_through_when_settings_holds_shell() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(user.join("User Avatars")).unwrap();
        let settings_path = user.join("settings.json");
        std::fs::write(
            &settings_path,
            r#"{"power_user":{"personas":{"p1.png":"[Unnamed Persona]"},"persona_descriptions":{"p1.png":{"description":null,"position":0}}}}"#,
        )
        .unwrap();
        let card_png = png_with_card("达达利亚", "原神执行官");
        std::fs::write(avatar_disk_path(&root), &card_png).unwrap();
        // settings.json is explicitly newer than the image: a naive bridge
        // would treat the shell as the latest value.
        let settings_seconds = (future_ms(300) / 1000) as i64;
        filetime::set_file_mtime(
            &settings_path,
            filetime::FileTime::from_unix_time(settings_seconds, 0),
        )
        .unwrap();

        let store = persona_store(&root);

        // The served bytes are the untouched disk PNG, real card intact.
        let bytes = read_bytes(&store, AVATAR_WIRE_PATH).await;
        assert_eq!(bytes, card_png);
        let card = persona_bridge::extract_persona(&bytes)
            .unwrap()
            .expect("disk card survives");
        assert_eq!(card.name.as_deref(), Some("达达利亚"));

        // The scan advertises the raw disk view: size equals the file and the
        // mtime does not track settings.json, so no shell is transferred and
        // identical images converge.
        let entry = store
            .scan(avatar_policy())
            .await
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.path.as_str() == AVATAR_WIRE_PATH)
            .expect("avatar entry");
        assert_eq!(entry.size_bytes as usize, card_png.len());
        assert!(
            entry.modified_ms / 1000 < settings_seconds as u64,
            "shell passthrough must not adopt the settings mtime"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn push_avatar_heals_shell_even_when_card_is_older() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(&user).unwrap();
        let settings_path = user.join("settings.json");
        std::fs::write(
            &settings_path,
            r#"{"power_user":{"personas":{"p1.png":"[Unnamed Persona]"},"persona_descriptions":{"p1.png":{"description":null,"position":0}}}}"#,
        )
        .unwrap();
        // The incoming card is an hour older than the shell-bearing settings.
        let past_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 3_600_000;

        let store = persona_store(&root);
        store
            .write_file(
                &SyncPath::new(AVATAR_WIRE_PATH).unwrap(),
                &mut Cursor::new(png_with_card("达达利亚", "原神执行官")),
                past_ms,
            )
            .await
            .unwrap();

        let merged: Value =
            serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        assert_eq!(
            merged["power_user"]["personas"]["p1.png"],
            json!("达达利亚"),
            "the real card replaces the shell regardless of mtime"
        );
        assert_eq!(
            merged["power_user"]["persona_descriptions"]["p1.png"]["description"],
            json!("原神执行官")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn composed_layers_bridge_personas_and_translate_sections_together() {
        let root = unique_temp_root();
        let user = root.join("default-user");
        std::fs::create_dir_all(user.join("User Avatars")).unwrap();
        let settings_path = user.join("settings.json");
        std::fs::write(
            &settings_path,
            r#"{"background":"bg","main_api":"openai","power_user":{"theme":"dark","allow_name1_display":true,"personas":{"p1.png":"克莱因"},"persona_descriptions":{"p1.png":{"description":"棋手","position":0}}}}"#,
        )
        .unwrap();
        std::fs::write(avatar_disk_path(&root), plain_png()).unwrap();

        let store = composed_store(&root);
        let manifest = store.scan(avatar_policy()).await.unwrap();
        let paths: Vec<&str> = manifest
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        // The section layer still synthesizes split files...
        assert!(paths.contains(&"default-user/settings/appearance.json"));
        // ...and the persona layer resizes settings.json to the stripped core.
        let core_bytes = read_bytes(&store, "default-user/settings.json").await;
        let core: Value = serde_json::from_slice(&core_bytes).unwrap();
        assert!(core["power_user"].get("personas").is_none());
        assert_eq!(core["power_user"]["allow_name1_display"], json!(true));
        let settings_entry = manifest
            .entries
            .iter()
            .find(|entry| entry.path.as_str() == "default-user/settings.json")
            .expect("settings entry");
        assert_eq!(settings_entry.size_bytes as usize, core_bytes.len());
        // Section reads and avatar injection both work through both layers.
        let appearance: Value = serde_json::from_slice(
            read_bytes(&store, "default-user/settings/appearance.json")
                .await
                .as_slice(),
        )
        .unwrap();
        assert_eq!(
            appearance,
            json!({ "background": "bg", "power_user": { "theme": "dark" } })
        );
        let avatar_bytes = read_bytes(&store, AVATAR_WIRE_PATH).await;
        assert_eq!(
            persona_bridge::extract_persona(&avatar_bytes)
                .unwrap()
                .expect("card")
                .name
                .as_deref(),
            Some("克莱因")
        );

        // TauriTavern pushes a persona-less core: sections (inner layer) and
        // personas (outer layer, from the disk card) both survive on disk.
        store
            .write_file(
                &SyncPath::new("default-user/settings.json").unwrap(),
                &mut Cursor::new(
                    br#"{"main_api":"openai","power_user":{"allow_name1_display":true}}"#.to_vec(),
                ),
                future_ms(200),
            )
            .await
            .unwrap();
        let merged: Value =
            serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        assert_eq!(merged["background"], json!("bg"));
        assert_eq!(merged["power_user"]["theme"], json!("dark"));
        assert_eq!(merged["power_user"]["personas"]["p1.png"], json!("克莱因"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
