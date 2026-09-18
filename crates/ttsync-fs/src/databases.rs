//! Receive database namespaces beside their destination, then publish complete directories.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use ttsync_contract::manifest::ManifestEntryV2;
use ttsync_contract::path::SyncPath;
use ttsync_contract::plan::SyncPlan;
use ttsync_core::database::{ROOT, namespace_directory};
use ttsync_core::error::SyncError;

use crate::writer::write_file_to_path;

#[derive(Debug)]
pub struct DatabaseTransfer {
    root: PathBuf,
    staging: PathBuf,
    groups: Mutex<BTreeMap<String, Vec<ManifestEntryV2>>>,
    applied: Mutex<BTreeSet<String>>,
    rollback_failed: AtomicBool,
}

impl DatabaseTransfer {
    pub fn new(data_root: &Path) -> Self {
        let root = data_root.join(ROOT);
        Self {
            staging: root.join(".staging").join(uuid::Uuid::new_v4().to_string()),
            root,
            groups: Mutex::new(BTreeMap::new()),
            applied: Mutex::new(BTreeSet::new()),
            rollback_failed: AtomicBool::new(false),
        }
    }

    pub fn set_plan(&self, plan: &SyncPlan) {
        let mut groups = self.groups.lock().expect("database transfer groups");
        for entry in &plan.transfer {
            if let Some(directory) = namespace_directory(entry.path.as_str()) {
                groups
                    .entry(directory.to_owned())
                    .or_default()
                    .push(entry.clone());
            }
        }
        for path in &plan.delete {
            if let Some(directory) = namespace_directory(path.as_str()) {
                groups.entry(directory.to_owned()).or_default();
            }
        }
    }

    pub async fn write_file(
        &self,
        path: &SyncPath,
        data: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
        modified_ms: u64,
    ) -> Result<(), SyncError> {
        let relative = path
            .as_str()
            .strip_prefix(ROOT)
            .expect("database path")
            .trim_start_matches('/');
        write_file_to_path(&self.staging.join(relative), data, modified_ms).await
    }

    pub fn is_applied(&self, path: &str) -> bool {
        namespace_directory(path).is_none_or(|directory| {
            self.applied
                .lock()
                .expect("database applied groups")
                .contains(directory)
        })
    }

    pub fn changed(&self) -> bool {
        self.rollback_failed.load(Ordering::Relaxed)
            || !self
                .applied
                .lock()
                .expect("database applied groups")
                .is_empty()
    }

    /// Publish while the caller holds its maintenance guard, on a blocking thread.
    pub fn commit(&self) -> Result<(), SyncError> {
        let groups = self.groups.lock().expect("database transfer groups");
        for (directory, entries) in groups.iter() {
            let name = directory.rsplit('/').next().expect("namespace directory");
            let source = self.staging.join(name);
            let target = self.root.join(name);
            for entry in entries {
                let file = source.join(
                    entry
                        .path
                        .as_str()
                        .rsplit('/')
                        .next()
                        .expect("database file"),
                );
                let size = std::fs::metadata(&file)
                    .map_err(|error| {
                        SyncError::Io(format!("Incomplete database {directory}: {error}"))
                    })?
                    .len();
                if size != entry.size_bytes {
                    return Err(SyncError::InvalidData(format!(
                        "Incomplete database file: {}",
                        entry.path
                    )));
                }
            }
            // Renaming the old directory avoids copying it and allows ordinary publish failures to roll back.
            let previous = self.staging.join(format!("old-{name}"));
            std::fs::create_dir_all(&self.staging).map_err(io_error)?;
            let existed = target.exists();
            if existed {
                std::fs::rename(&target, &previous).map_err(io_error)?;
            }
            if !entries.is_empty()
                && let Err(error) = std::fs::rename(&source, &target)
            {
                if existed && let Err(restore_error) = std::fs::rename(&previous, &target) {
                    self.rollback_failed.store(true, Ordering::Relaxed);
                    return Err(SyncError::Io(format!(
                        "Publish {directory}: {error}; restore: {restore_error}; previous database retained at {}",
                        previous.display()
                    )));
                }
                return Err(io_error(error));
            }
            self.applied
                .lock()
                .expect("database applied groups")
                .insert(directory.clone());
        }
        Ok(())
    }
}

impl Drop for DatabaseTransfer {
    fn drop(&mut self) {
        if self.rollback_failed.load(Ordering::Relaxed) {
            return;
        }
        if let Err(error) = std::fs::remove_dir_all(&self.staging)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(
                "Remove database staging {}: {error}",
                self.staging.display()
            );
        }
    }
}

fn io_error(error: std::io::Error) -> SyncError {
    SyncError::Io(error.to_string())
}
