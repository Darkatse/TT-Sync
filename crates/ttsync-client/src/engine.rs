use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use async_compression::tokio::bufread::{ZstdDecoder, ZstdEncoder};
use futures_util::TryStreamExt;
use reqwest::Body;
use tokio::io::BufReader;
use tokio::task::JoinSet;
use tokio_util::io::{ReaderStream, StreamReader};
use ttsync_contract::dataset::DatasetSelection;
use ttsync_contract::manifest::ManifestEntryV2;
use ttsync_contract::peer::{DeviceId, Permissions};
use ttsync_contract::plan::{PlanId, SyncPlan};
use ttsync_contract::session::SessionToken;
use ttsync_contract::status::StatusResponse;
use ttsync_contract::sync::{SyncMode, SyncPhase};
use ttsync_core::bundle::{
    BUNDLE_STREAM_BUFFER_SIZE, BUNDLE_ZSTD_DECODE_BUFFER_SIZE, ExactSizeReader, FEATURE_BUNDLE_V1,
    FEATURE_ZSTD_V1, copy_exact_and_expect_eof, expect_eof,
};
use ttsync_core::dataset::ResolvedDatasetPolicy;
use ttsync_core::error::SyncError;
use ttsync_core::plan::validate_plan_scope;
use ttsync_http::client::{SyncClient, ensure_dataset_scope_v1};

use crate::bundle::{BundleFileProgress, write_bundle_to_workspace, write_bundle_upload};
use crate::workspace::ClientWorkspace;

#[derive(Debug, Clone)]
pub struct ClientSyncTarget {
    pub device_id: DeviceId,
    pub ed25519_seed_b64url: String,
}

#[derive(Debug, Clone)]
pub struct ClientSyncOptions {
    pub mode: SyncMode,
    pub selection: DatasetSelection,
    pub require_bundle_zstd: bool,
    pub file_concurrency: usize,
}

impl ClientSyncOptions {
    pub fn new(mode: SyncMode, selection: DatasetSelection) -> Self {
        Self {
            mode,
            selection,
            require_bundle_zstd: false,
            file_concurrency: 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    Pull,
    Push,
}

#[derive(Debug, Clone)]
pub struct SyncProgress {
    pub direction: SyncDirection,
    pub phase: SyncPhase,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current_path: Option<String>,
}

pub trait SyncObserver: Send + Sync {
    fn on_progress(&self, progress: SyncProgress);
}

pub struct NoopSyncObserver;

impl SyncObserver for NoopSyncObserver {
    fn on_progress(&self, _progress: SyncProgress) {}
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalChangeSummary {
    pub files_written: usize,
    pub bytes_written: u64,
    pub files_deleted: usize,
}

impl LocalChangeSummary {
    pub fn changed(&self) -> bool {
        self.files_written > 0 || self.files_deleted > 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientSyncSummary {
    pub files_total: usize,
    pub bytes_total: u64,
    pub files_deleted: usize,
}

#[derive(Debug, Clone)]
pub struct ClientSyncReport {
    pub summary: ClientSyncSummary,
    pub local_applied: LocalChangeSummary,
    pub granted_permissions: Permissions,
}

#[derive(Debug)]
pub struct ClientSyncFailure {
    pub error: SyncError,
    pub local_applied: LocalChangeSummary,
    pub local_changed: bool,
    pub granted_permissions: Option<Permissions>,
}

impl ClientSyncFailure {
    fn without_local_change(error: SyncError) -> Self {
        Self {
            error,
            local_applied: LocalChangeSummary::default(),
            local_changed: false,
            granted_permissions: None,
        }
    }

    fn with_local_state(
        error: SyncError,
        local_applied: LocalChangeSummary,
        local_changed: bool,
    ) -> Self {
        Self {
            error,
            local_applied,
            local_changed,
            granted_permissions: None,
        }
    }

    fn with_permissions(mut self, permissions: Permissions) -> Self {
        self.granted_permissions = Some(permissions);
        self
    }
}

pub struct ClientSyncEngine<W> {
    client: SyncClient,
    workspace: Arc<W>,
    target: ClientSyncTarget,
    peer_label: String,
}

impl<W> ClientSyncEngine<W>
where
    W: ClientWorkspace + 'static,
{
    pub fn new(
        client: SyncClient,
        workspace: Arc<W>,
        target: ClientSyncTarget,
        peer_label: impl Into<String>,
    ) -> Self {
        Self {
            client,
            workspace,
            target,
            peer_label: peer_label.into(),
        }
    }

    pub async fn pull<O>(
        &self,
        options: ClientSyncOptions,
        observer: &O,
    ) -> Result<ClientSyncReport, ClientSyncFailure>
    where
        O: SyncObserver,
    {
        validate_options(&options)?;
        let (transport, permissions, session_token) = self.prepare_session(&options).await?;

        ensure_pull_allowed(permissions, options.mode)
            .map_err(ClientSyncFailure::without_local_change)
            .map_err(|failure| failure.with_permissions(permissions))?;

        emit(
            observer,
            SyncDirection::Pull,
            SyncPhase::Scanning,
            ProgressCounts::default(),
            None,
        );
        let policy = ResolvedDatasetPolicy::from_selection(&options.selection)
            .map_err(ClientSyncFailure::without_local_change)
            .map_err(|failure| failure.with_permissions(permissions))?;
        let target_manifest = self
            .workspace
            .scan(policy.clone())
            .await
            .map_err(ClientSyncFailure::without_local_change)
            .map_err(|failure| failure.with_permissions(permissions))?;

        emit(
            observer,
            SyncDirection::Pull,
            SyncPhase::Diffing,
            ProgressCounts::default(),
            None,
        );
        let plan = self
            .client
            .pull_plan(
                &session_token,
                options.mode,
                options.selection.clone(),
                target_manifest,
            )
            .await
            .map_err(ClientSyncFailure::without_local_change)
            .map_err(|failure| failure.with_permissions(permissions))?;
        validate_plan_scope(&plan, &policy)
            .map_err(ClientSyncFailure::without_local_change)
            .map_err(|failure| failure.with_permissions(permissions))?;

        let files_total = plan.files_total;
        let bytes_total = plan.bytes_total;
        let local_applied = self
            .apply_pull_plan(
                plan,
                options.mode,
                transport,
                &session_token,
                options.file_concurrency,
                observer,
            )
            .await
            .map_err(|failure| failure.with_permissions(permissions))?;

        Ok(ClientSyncReport {
            summary: ClientSyncSummary {
                files_total,
                bytes_total,
                files_deleted: local_applied.files_deleted,
            },
            local_applied,
            granted_permissions: permissions,
        })
    }

    pub async fn direct_push<O>(
        &self,
        options: ClientSyncOptions,
        observer: &O,
    ) -> Result<ClientSyncReport, ClientSyncFailure>
    where
        O: SyncObserver,
    {
        validate_options(&options)?;
        let (transport, permissions, session_token) = self.prepare_session(&options).await?;

        ensure_push_allowed(permissions, options.mode)
            .map_err(ClientSyncFailure::without_local_change)
            .map_err(|failure| failure.with_permissions(permissions))?;

        emit(
            observer,
            SyncDirection::Push,
            SyncPhase::Scanning,
            ProgressCounts::default(),
            None,
        );
        let policy = ResolvedDatasetPolicy::from_selection(&options.selection)
            .map_err(ClientSyncFailure::without_local_change)
            .map_err(|failure| failure.with_permissions(permissions))?;
        let source_manifest = self
            .workspace
            .scan(policy.clone())
            .await
            .map_err(ClientSyncFailure::without_local_change)
            .map_err(|failure| failure.with_permissions(permissions))?;

        emit(
            observer,
            SyncDirection::Push,
            SyncPhase::Diffing,
            ProgressCounts::default(),
            None,
        );
        let plan = self
            .client
            .push_plan(
                &session_token,
                options.mode,
                options.selection.clone(),
                source_manifest,
            )
            .await
            .map_err(ClientSyncFailure::without_local_change)
            .map_err(|failure| failure.with_permissions(permissions))?;
        validate_plan_scope(&plan, &policy)
            .map_err(ClientSyncFailure::without_local_change)
            .map_err(|failure| failure.with_permissions(permissions))?;

        let files_total = plan.files_total;
        let bytes_total = plan.bytes_total;
        let files_deleted = if options.mode == SyncMode::Mirror {
            plan.delete.len()
        } else {
            0
        };
        self.apply_push_plan(
            plan,
            options.mode,
            transport,
            &session_token,
            options.file_concurrency,
            observer,
        )
        .await
        .map_err(|failure| failure.with_permissions(permissions))?;

        Ok(ClientSyncReport {
            summary: ClientSyncSummary {
                files_total,
                bytes_total,
                files_deleted,
            },
            local_applied: LocalChangeSummary::default(),
            granted_permissions: permissions,
        })
    }

    async fn prepare_session(
        &self,
        options: &ClientSyncOptions,
    ) -> Result<(BundleTransport, Permissions, SessionToken), ClientSyncFailure> {
        let status = self
            .client
            .status()
            .await
            .map_err(ClientSyncFailure::without_local_change)?;
        ensure_dataset_scope_v1(&status)
            .map_err(|error| relabel_dataset_error(error, &self.peer_label))
            .map_err(ClientSyncFailure::without_local_change)?;
        let transport =
            bundle_transport_for_status(&status, &self.peer_label, options.require_bundle_zstd)
                .map_err(ClientSyncFailure::without_local_change)?;

        let session = self
            .client
            .open_session(&self.target.device_id, &self.target.ed25519_seed_b64url)
            .await
            .map_err(ClientSyncFailure::without_local_change)?;

        Ok((
            transport,
            session.granted_permissions,
            session.session_token,
        ))
    }

    async fn apply_pull_plan<O>(
        &self,
        plan: SyncPlan,
        mode: SyncMode,
        transport: BundleTransport,
        session_token: &SessionToken,
        file_concurrency: usize,
        observer: &O,
    ) -> Result<LocalChangeSummary, ClientSyncFailure>
    where
        O: SyncObserver,
    {
        let plan_id = plan.plan_id;
        let transfer_entries = plan.transfer;
        let delete = plan.delete;
        let tracker = Arc::new(LocalChangeTracker::default());
        let mut files_done = 0usize;
        let mut bytes_done = 0u64;
        let files_total = transfer_entries.len();
        let bytes_total = transfer_entries
            .iter()
            .map(|entry| entry.size_bytes)
            .sum::<u64>();

        emit(
            observer,
            SyncDirection::Pull,
            SyncPhase::Downloading,
            ProgressCounts::new(files_done, files_total, bytes_done, bytes_total),
            None,
        );

        if transport.prefer_bundle && !transfer_entries.is_empty() {
            let response = self
                .client
                .download_bundle(session_token, &plan_id, transport.use_zstd)
                .await
                .map_err(ClientSyncFailure::without_local_change)?;
            let content_encoding = response
                .headers()
                .get(reqwest::header::CONTENT_ENCODING)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            let is_zstd = content_encoding.eq_ignore_ascii_case("zstd");

            let stream = response.bytes_stream().map_err(std::io::Error::other);
            let reader = StreamReader::new(stream);
            let mut reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = if is_zstd {
                Box::new(ZstdDecoder::new(BufReader::with_capacity(
                    BUNDLE_ZSTD_DECODE_BUFFER_SIZE,
                    reader,
                )))
            } else {
                Box::new(reader)
            };

            if let Err(error) = write_bundle_to_workspace(
                &*self.workspace,
                transfer_entries,
                &mut reader,
                |progress| {
                    files_done += 1;
                    bytes_done += progress.size_bytes;
                    tracker.record_write(progress.size_bytes);

                    if should_emit_progress(files_done, files_total) {
                        emit(
                            observer,
                            SyncDirection::Pull,
                            SyncPhase::Downloading,
                            ProgressCounts::new(files_done, files_total, bytes_done, bytes_total),
                            Some(progress.path),
                        );
                    }
                },
            )
            .await
            {
                let local_applied = tracker.summary();
                let local_changed = local_applied.changed() || error.target_changed();
                return Err(ClientSyncFailure::with_local_state(
                    error.into_error(),
                    local_applied,
                    local_changed,
                ));
            }
        } else {
            self.download_files(
                transfer_entries,
                session_token.clone(),
                plan_id,
                tracker.clone(),
                file_concurrency,
                observer,
            )
            .await?;
        }

        if mode != SyncMode::Mirror || delete.is_empty() {
            return Ok(tracker.summary());
        }

        let delete_total = delete.len();
        emit(
            observer,
            SyncDirection::Pull,
            SyncPhase::Deleting,
            ProgressCounts::new(0, delete_total, 0, 0),
            None,
        );

        let mut files_deleted = 0usize;
        for sync_path in delete {
            if let Err(error) = self.workspace.delete_file(&sync_path).await {
                let local_applied = tracker.summary();
                let local_changed = local_applied.changed() || error.target_changed();
                return Err(ClientSyncFailure::with_local_state(
                    error.into_error(),
                    local_applied,
                    local_changed,
                ));
            }

            files_deleted += 1;
            tracker.record_delete();
            if should_emit_progress(files_deleted, delete_total) {
                emit(
                    observer,
                    SyncDirection::Pull,
                    SyncPhase::Deleting,
                    ProgressCounts::new(files_deleted, delete_total, 0, 0),
                    Some(sync_path.to_string()),
                );
            }
        }

        Ok(tracker.summary())
    }

    async fn download_files<O>(
        &self,
        transfer_entries: Vec<ManifestEntryV2>,
        session_token: SessionToken,
        plan_id: PlanId,
        tracker: Arc<LocalChangeTracker>,
        file_concurrency: usize,
        observer: &O,
    ) -> Result<(), ClientSyncFailure>
    where
        O: SyncObserver,
    {
        let files_total = transfer_entries.len();
        let bytes_total = transfer_entries
            .iter()
            .map(|entry| entry.size_bytes)
            .sum::<u64>();
        let mut files_done = 0usize;
        let mut bytes_done = 0u64;
        let mut join_set = JoinSet::new();
        let mut download_iter = transfer_entries.into_iter();
        let mut in_flight = 0usize;

        while in_flight < file_concurrency {
            let Some(entry) = download_iter.next() else {
                break;
            };
            spawn_download_task(
                &mut join_set,
                self.client.clone(),
                self.workspace.clone(),
                session_token.clone(),
                plan_id.clone(),
                entry,
                tracker.clone(),
            );
            in_flight += 1;
        }

        let mut first_error = None;
        while in_flight > 0 {
            let joined = match join_set.join_next().await {
                Some(Ok(Ok(joined))) => Some(joined),
                Some(Ok(Err(error))) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    None
                }
                Some(Err(error)) => {
                    if first_error.is_none() {
                        first_error = Some((SyncError::Internal(error.to_string()), false));
                    }
                    None
                }
                None => {
                    if first_error.is_none() {
                        first_error = Some((
                            SyncError::Internal("download join set ended early".into()),
                            false,
                        ));
                    }
                    None
                }
            };

            in_flight -= 1;
            if let Some(joined) = joined
                && first_error.is_none()
            {
                files_done += 1;
                bytes_done += joined.size_bytes;

                if should_emit_progress(files_done, files_total) {
                    emit(
                        observer,
                        SyncDirection::Pull,
                        SyncPhase::Downloading,
                        ProgressCounts::new(files_done, files_total, bytes_done, bytes_total),
                        Some(joined.path),
                    );
                }
            }

            if first_error.is_none()
                && let Some(entry) = download_iter.next()
            {
                spawn_download_task(
                    &mut join_set,
                    self.client.clone(),
                    self.workspace.clone(),
                    session_token.clone(),
                    plan_id.clone(),
                    entry,
                    tracker.clone(),
                );
                in_flight += 1;
            }
        }

        match first_error {
            Some((error, local_changed)) => {
                let local_applied = tracker.summary();
                Err(ClientSyncFailure::with_local_state(
                    error,
                    local_applied,
                    local_applied.changed() || local_changed,
                ))
            }
            None => Ok(()),
        }
    }

    async fn apply_push_plan<O>(
        &self,
        plan: SyncPlan,
        mode: SyncMode,
        transport: BundleTransport,
        session_token: &SessionToken,
        file_concurrency: usize,
        observer: &O,
    ) -> Result<(), ClientSyncFailure>
    where
        O: SyncObserver,
    {
        let plan_id = plan.plan_id;
        let transfer_entries = plan.transfer;
        let delete = plan.delete;
        let files_total = transfer_entries.len();
        let bytes_total = transfer_entries
            .iter()
            .map(|entry| entry.size_bytes)
            .sum::<u64>();

        emit(
            observer,
            SyncDirection::Push,
            SyncPhase::Uploading,
            ProgressCounts::new(0, files_total, 0, bytes_total),
            None,
        );

        if transport.prefer_bundle && !transfer_entries.is_empty() {
            self.upload_bundle(
                transfer_entries,
                session_token,
                &plan_id,
                transport.use_zstd,
                observer,
            )
            .await?;
        } else {
            self.upload_files(
                transfer_entries,
                session_token.clone(),
                plan_id.clone(),
                file_concurrency,
                observer,
            )
            .await?;
        }

        if mode == SyncMode::Mirror && !delete.is_empty() {
            emit(
                observer,
                SyncDirection::Push,
                SyncPhase::Deleting,
                ProgressCounts::new(0, delete.len(), 0, 0),
                None,
            );
        }

        let commit = self
            .client
            .commit(session_token, &plan_id)
            .await
            .map_err(ClientSyncFailure::without_local_change)?;
        if !commit.ok {
            return Err(ClientSyncFailure::without_local_change(
                SyncError::Internal("TT-Sync commit returned ok=false".into()),
            ));
        }

        if mode == SyncMode::Mirror && !delete.is_empty() {
            emit(
                observer,
                SyncDirection::Push,
                SyncPhase::Deleting,
                ProgressCounts::new(delete.len(), delete.len(), 0, 0),
                None,
            );
        }

        Ok(())
    }

    async fn upload_bundle<O>(
        &self,
        transfer_entries: Vec<ManifestEntryV2>,
        session_token: &SessionToken,
        plan_id: &PlanId,
        allow_zstd: bool,
        observer: &O,
    ) -> Result<(), ClientSyncFailure>
    where
        O: SyncObserver,
    {
        let files_total = transfer_entries.len();
        let bytes_total = transfer_entries
            .iter()
            .map(|entry| entry.size_bytes)
            .sum::<u64>();
        let mut files_done = 0usize;
        let mut bytes_done = 0u64;
        let (progress_tx, mut progress_rx) =
            tokio::sync::mpsc::unbounded_channel::<BundleFileProgress>();
        let (reader, writer) = tokio::io::duplex(BUNDLE_STREAM_BUFFER_SIZE);
        let workspace = self.workspace.clone();
        let writer_task = tokio::spawn(async move {
            write_bundle_upload(&*workspace, transfer_entries, writer, progress_tx).await
        });

        let reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = if allow_zstd {
            Box::new(ZstdEncoder::new(BufReader::with_capacity(
                BUNDLE_STREAM_BUFFER_SIZE,
                reader,
            )))
        } else {
            Box::new(reader)
        };
        let stream = ReaderStream::with_capacity(reader, BUNDLE_STREAM_BUFFER_SIZE);
        let body = Body::wrap_stream(stream);

        let mut upload =
            Box::pin(
                self.client
                    .upload_bundle(session_token, plan_id, body, allow_zstd),
            );
        let upload_result = loop {
            tokio::select! {
                result = &mut upload => break result,
                Some(progress) = progress_rx.recv() => {
                    files_done += 1;
                    bytes_done += progress.size_bytes;
                    if should_emit_progress(files_done, files_total) {
                        emit(
                            observer,
                            SyncDirection::Push,
                            SyncPhase::Uploading,
                            ProgressCounts::new(files_done, files_total, bytes_done, bytes_total),
                            Some(progress.path),
                        );
                    }
                }
            }
        };

        let writer_result = writer_task.await.map_err(|error| {
            ClientSyncFailure::without_local_change(SyncError::Internal(error.to_string()))
        })?;
        while let Ok(progress) = progress_rx.try_recv() {
            files_done += 1;
            bytes_done += progress.size_bytes;
            if should_emit_progress(files_done, files_total) {
                emit(
                    observer,
                    SyncDirection::Push,
                    SyncPhase::Uploading,
                    ProgressCounts::new(files_done, files_total, bytes_done, bytes_total),
                    Some(progress.path),
                );
            }
        }

        upload_result.map_err(ClientSyncFailure::without_local_change)?;
        writer_result.map_err(ClientSyncFailure::without_local_change)
    }

    async fn upload_files<O>(
        &self,
        transfer_entries: Vec<ManifestEntryV2>,
        session_token: SessionToken,
        plan_id: PlanId,
        file_concurrency: usize,
        observer: &O,
    ) -> Result<(), ClientSyncFailure>
    where
        O: SyncObserver,
    {
        let files_total = transfer_entries.len();
        let bytes_total = transfer_entries
            .iter()
            .map(|entry| entry.size_bytes)
            .sum::<u64>();
        let mut files_done = 0usize;
        let mut bytes_done = 0u64;
        let mut join_set = JoinSet::new();
        let mut upload_iter = transfer_entries.into_iter();
        let mut in_flight = 0usize;

        while in_flight < file_concurrency {
            let Some(entry) = upload_iter.next() else {
                break;
            };
            spawn_upload_task(
                &mut join_set,
                self.client.clone(),
                self.workspace.clone(),
                session_token.clone(),
                plan_id.clone(),
                entry,
            );
            in_flight += 1;
        }

        while in_flight > 0 {
            let joined = join_set
                .join_next()
                .await
                .ok_or_else(|| {
                    ClientSyncFailure::without_local_change(SyncError::Internal(
                        "upload join set ended early".into(),
                    ))
                })?
                .map_err(|error| {
                    ClientSyncFailure::without_local_change(SyncError::Internal(error.to_string()))
                })?
                .map_err(ClientSyncFailure::without_local_change)?;

            in_flight -= 1;
            files_done += 1;
            bytes_done += joined.size_bytes;

            if should_emit_progress(files_done, files_total) {
                emit(
                    observer,
                    SyncDirection::Push,
                    SyncPhase::Uploading,
                    ProgressCounts::new(files_done, files_total, bytes_done, bytes_total),
                    Some(joined.path),
                );
            }

            if let Some(entry) = upload_iter.next() {
                spawn_upload_task(
                    &mut join_set,
                    self.client.clone(),
                    self.workspace.clone(),
                    session_token.clone(),
                    plan_id.clone(),
                    entry,
                );
                in_flight += 1;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct BundleTransport {
    prefer_bundle: bool,
    use_zstd: bool,
}

#[derive(Debug)]
struct TransferResult {
    path: String,
    size_bytes: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct ProgressCounts {
    files_done: usize,
    files_total: usize,
    bytes_done: u64,
    bytes_total: u64,
}

impl ProgressCounts {
    fn new(files_done: usize, files_total: usize, bytes_done: u64, bytes_total: u64) -> Self {
        Self {
            files_done,
            files_total,
            bytes_done,
            bytes_total,
        }
    }
}

fn validate_options(options: &ClientSyncOptions) -> Result<(), ClientSyncFailure> {
    if options.file_concurrency == 0 {
        return Err(ClientSyncFailure::without_local_change(
            SyncError::InvalidData("file_concurrency must be greater than 0".into()),
        ));
    }

    ResolvedDatasetPolicy::from_selection(&options.selection)
        .map(|_| ())
        .map_err(ClientSyncFailure::without_local_change)
}

fn ensure_pull_allowed(permissions: Permissions, mode: SyncMode) -> Result<(), SyncError> {
    if !permissions.read {
        return Err(SyncError::Unauthorized("read not granted".into()));
    }
    if mode == SyncMode::Mirror && !permissions.mirror_delete {
        return Err(SyncError::Unauthorized("mirror_delete not granted".into()));
    }
    Ok(())
}

fn ensure_push_allowed(permissions: Permissions, mode: SyncMode) -> Result<(), SyncError> {
    if !permissions.write {
        return Err(SyncError::Unauthorized("write not granted".into()));
    }
    if mode == SyncMode::Mirror && !permissions.mirror_delete {
        return Err(SyncError::Unauthorized("mirror_delete not granted".into()));
    }
    Ok(())
}

fn bundle_transport_for_status(
    status: &StatusResponse,
    peer_label: &str,
    require_bundle_zstd: bool,
) -> Result<BundleTransport, SyncError> {
    let has_bundle = status
        .features
        .iter()
        .any(|feature| feature == FEATURE_BUNDLE_V1);
    let has_zstd = status
        .features
        .iter()
        .any(|feature| feature == FEATURE_ZSTD_V1);

    if require_bundle_zstd && !has_bundle {
        return Err(SyncError::InvalidData(format!(
            "{peer_label} does not support bundle_v1"
        )));
    }
    if require_bundle_zstd && !has_zstd {
        return Err(SyncError::InvalidData(format!(
            "{peer_label} does not support zstd_v1"
        )));
    }

    Ok(BundleTransport {
        prefer_bundle: has_bundle,
        use_zstd: has_bundle && has_zstd,
    })
}

fn relabel_dataset_error(error: SyncError, peer_label: &str) -> SyncError {
    match error {
        SyncError::InvalidData(message) => {
            SyncError::InvalidData(message.replace("TT-Sync server", peer_label))
        }
        other => other,
    }
}

fn spawn_download_task<W>(
    join_set: &mut JoinSet<Result<TransferResult, (SyncError, bool)>>,
    client: SyncClient,
    workspace: Arc<W>,
    session_token: SessionToken,
    plan_id: PlanId,
    entry: ManifestEntryV2,
    tracker: Arc<LocalChangeTracker>,
) where
    W: ClientWorkspace + 'static,
{
    join_set.spawn(async move {
        let response = client
            .download_file(&session_token, &plan_id, &entry.path)
            .await
            .map_err(|error| (error, false))?;
        if let Some(content_length) = response.content_length()
            && content_length != entry.size_bytes
        {
            return Err((
                SyncError::InvalidData(format!(
                    "downloaded file size mismatch for {}: expected {}, got {}",
                    entry.path, entry.size_bytes, content_length
                )),
                false,
            ));
        }
        let stream = response.bytes_stream().map_err(std::io::Error::other);
        let mut reader = StreamReader::new(stream);
        let mut exact = ExactSizeReader::new(&mut reader, entry.size_bytes);
        workspace
            .write_file(&entry.path, &mut exact, entry.modified_ms)
            .await
            .map_err(|error| {
                let target_changed = error.target_changed();
                (error.into_error(), target_changed)
            })?;
        tracker.record_write(entry.size_bytes);
        expect_eof(&mut reader, "downloaded file")
            .await
            .map_err(|error| (error, true))?;

        Ok(TransferResult {
            path: entry.path.to_string(),
            size_bytes: entry.size_bytes,
        })
    });
}

fn spawn_upload_task<W>(
    join_set: &mut JoinSet<Result<TransferResult, SyncError>>,
    client: SyncClient,
    workspace: Arc<W>,
    session_token: SessionToken,
    plan_id: PlanId,
    entry: ManifestEntryV2,
) where
    W: ClientWorkspace + 'static,
{
    join_set.spawn(async move {
        let mut source = workspace.read_file(&entry.path).await?;
        let (reader, mut writer) = tokio::io::duplex(BUNDLE_STREAM_BUFFER_SIZE);
        let size_bytes = entry.size_bytes;
        let writer_task = tokio::spawn(async move {
            let mut buffer = vec![0u8; BUNDLE_STREAM_BUFFER_SIZE];
            copy_exact_and_expect_eof(&mut source, &mut writer, size_bytes, &mut buffer).await
        });

        let stream = ReaderStream::with_capacity(reader, BUNDLE_STREAM_BUFFER_SIZE);
        let body = Body::wrap_stream(stream);
        let upload_result = client
            .upload_file(&session_token, &plan_id, &entry.path, body)
            .await;
        let writer_result = writer_task
            .await
            .map_err(|error| SyncError::Internal(error.to_string()))?;

        upload_result?;
        writer_result?;

        Ok(TransferResult {
            path: entry.path.to_string(),
            size_bytes: entry.size_bytes,
        })
    });
}

fn emit<O>(
    observer: &O,
    direction: SyncDirection,
    phase: SyncPhase,
    counts: ProgressCounts,
    current_path: Option<String>,
) where
    O: SyncObserver,
{
    observer.on_progress(SyncProgress {
        direction,
        phase,
        files_done: counts.files_done,
        files_total: counts.files_total,
        bytes_done: counts.bytes_done,
        bytes_total: counts.bytes_total,
        current_path,
    });
}

fn should_emit_progress(files_done: usize, files_total: usize) -> bool {
    files_done == files_total || files_done == 1 || files_done.is_multiple_of(10)
}

#[derive(Default)]
struct LocalChangeTracker {
    files_written: AtomicUsize,
    bytes_written: AtomicU64,
    files_deleted: AtomicUsize,
}

impl LocalChangeTracker {
    fn record_write(&self, size_bytes: u64) {
        self.files_written.fetch_add(1, Ordering::Relaxed);
        self.bytes_written.fetch_add(size_bytes, Ordering::Relaxed);
    }

    fn record_delete(&self) {
        self.files_deleted.fetch_add(1, Ordering::Relaxed);
    }

    fn summary(&self) -> LocalChangeSummary {
        LocalChangeSummary {
            files_written: self.files_written.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            files_deleted: self.files_deleted.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use tokio::io::AsyncReadExt;
    use ttsync_contract::dataset::DatasetSelection;
    use ttsync_contract::manifest::{ManifestEntryV2, ManifestV2};
    use ttsync_contract::path::SyncPath;
    use ttsync_contract::peer::{DeviceId, PeerGrant, Permissions};
    use ttsync_core::crypto::device_pubkey_b64url;
    use ttsync_core::dataset::ResolvedDatasetPolicy;
    use ttsync_core::ports::{ManifestStore, PeerStore};
    use ttsync_core::session::{SessionManager, SessionManagerConfig};
    use ttsync_http::client::SyncClient;
    use ttsync_http::pairing_store::PairingTokenStore;
    use ttsync_http::server::{ServerState, spawn_server};
    use ttsync_http::tls::{SelfManagedTls, TlsProvider};

    use ttsync_contract::sync::SyncMode;

    use super::{ClientSyncEngine, ClientSyncOptions, ClientSyncTarget, NoopSyncObserver};

    #[derive(Debug, Clone)]
    struct MemoryFile {
        bytes: Vec<u8>,
        modified_ms: u64,
    }

    #[derive(Debug, Default)]
    struct MemoryManifestStore {
        files: Mutex<HashMap<SyncPath, MemoryFile>>,
    }

    impl MemoryManifestStore {
        fn insert(&self, path: &str, bytes: &[u8], modified_ms: u64) {
            self.files.lock().expect("files mutex").insert(
                SyncPath::new(path.to_owned()).expect("valid sync path"),
                MemoryFile {
                    bytes: bytes.to_vec(),
                    modified_ms,
                },
            );
        }

        fn bytes(&self, path: &str) -> Vec<u8> {
            self.files
                .lock()
                .expect("files mutex")
                .get(&SyncPath::new(path.to_owned()).expect("valid sync path"))
                .expect("file exists")
                .bytes
                .clone()
        }

        fn contains(&self, path: &str) -> bool {
            self.files
                .lock()
                .expect("files mutex")
                .contains_key(&SyncPath::new(path.to_owned()).expect("valid sync path"))
        }
    }

    impl ManifestStore for MemoryManifestStore {
        fn scan(
            &self,
            policy: ResolvedDatasetPolicy,
        ) -> impl std::future::Future<Output = Result<ManifestV2, ttsync_core::error::SyncError>> + Send
        {
            let mut entries = self
                .files
                .lock()
                .expect("files mutex")
                .iter()
                .filter(|(path, _)| policy.contains_path(path.as_str()))
                .map(|(path, file)| ManifestEntryV2 {
                    path: path.clone(),
                    size_bytes: file.bytes.len() as u64,
                    modified_ms: file.modified_ms,
                    content_hash: None,
                })
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));

            async move { Ok(ManifestV2 { entries }) }
        }

        fn read_file(
            &self,
            path: &SyncPath,
        ) -> impl std::future::Future<
            Output = Result<
                Box<dyn tokio::io::AsyncRead + Send + Unpin>,
                ttsync_core::error::SyncError,
            >,
        > + Send {
            let bytes = self
                .files
                .lock()
                .expect("files mutex")
                .get(path)
                .map(|file| file.bytes.clone());

            async move {
                let bytes = bytes.ok_or_else(|| {
                    ttsync_core::error::SyncError::NotFound("file not found".into())
                })?;
                Ok(Box::new(Cursor::new(bytes)) as Box<dyn tokio::io::AsyncRead + Send + Unpin>)
            }
        }

        fn write_file(
            &self,
            path: &SyncPath,
            data: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
            modified_ms: u64,
        ) -> impl std::future::Future<Output = Result<(), ttsync_core::error::SyncError>> + Send
        {
            let path = path.clone();
            async move {
                let mut bytes = Vec::new();
                data.read_to_end(&mut bytes)
                    .await
                    .map_err(|e| ttsync_core::error::SyncError::Io(e.to_string()))?;
                self.files
                    .lock()
                    .expect("files mutex")
                    .insert(path, MemoryFile { bytes, modified_ms });
                Ok(())
            }
        }

        fn delete_file(
            &self,
            path: &SyncPath,
        ) -> impl std::future::Future<Output = Result<(), ttsync_core::error::SyncError>> + Send
        {
            let path = path.clone();
            async move {
                self.files.lock().expect("files mutex").remove(&path);
                Ok(())
            }
        }
    }

    #[derive(Debug, Default)]
    struct MemoryPeerStore {
        peers: Mutex<HashMap<DeviceId, PeerGrant>>,
    }

    impl MemoryPeerStore {
        fn save(&self, grant: PeerGrant) {
            self.peers
                .lock()
                .expect("peers mutex")
                .insert(grant.device_id.clone(), grant);
        }
    }

    impl PeerStore for MemoryPeerStore {
        fn get_peer(
            &self,
            device_id: &DeviceId,
        ) -> impl std::future::Future<Output = Result<PeerGrant, ttsync_core::error::SyncError>> + Send
        {
            let grant = self
                .peers
                .lock()
                .expect("peers mutex")
                .get(device_id)
                .cloned();

            async move {
                grant
                    .ok_or_else(|| ttsync_core::error::SyncError::NotFound("peer not found".into()))
            }
        }

        async fn save_peer(&self, grant: PeerGrant) -> Result<(), ttsync_core::error::SyncError> {
            self.save(grant);
            Ok(())
        }

        fn remove_peer(
            &self,
            device_id: &DeviceId,
        ) -> impl std::future::Future<Output = Result<(), ttsync_core::error::SyncError>> + Send
        {
            let device_id = device_id.clone();
            async move {
                self.peers.lock().expect("peers mutex").remove(&device_id);
                Ok(())
            }
        }

        fn list_peers(
            &self,
        ) -> impl std::future::Future<
            Output = Result<Vec<PeerGrant>, ttsync_core::error::SyncError>,
        > + Send {
            let peers = self
                .peers
                .lock()
                .expect("peers mutex")
                .values()
                .cloned()
                .collect::<Vec<_>>();
            async move { Ok(peers) }
        }
    }

    #[tokio::test]
    async fn engine_pulls_and_pushes_with_bundle_zstd() {
        let state_dir = unique_temp_dir();
        let tls = SelfManagedTls::load_or_create(&state_dir).expect("TLS identity");
        let spki_sha256 = tls.spki_sha256().to_owned();

        let server_files = Arc::new(MemoryManifestStore::default());
        server_files.insert("default-user/chats/server.jsonl", b"server", 1234);
        let peer_store = Arc::new(MemoryPeerStore::default());

        let client_device_id = DeviceId::new("00000000-0000-4000-8000-000000000020".to_owned())
            .expect("valid device id");
        let client_seed = URL_SAFE_NO_PAD.encode([9u8; 32]);
        let client_pubkey = URL_SAFE_NO_PAD
            .decode(device_pubkey_b64url(&client_seed).expect("public key"))
            .expect("decode public key");
        peer_store.save(PeerGrant {
            device_id: client_device_id.clone(),
            device_name: "TT-Sync Test Client".to_owned(),
            public_key: client_pubkey,
            permissions: Permissions {
                read: true,
                write: true,
                mirror_delete: true,
            },
            paired_at_ms: 1,
            last_sync_ms: None,
        });

        let server_device_id = DeviceId::new("00000000-0000-4000-8000-000000000010".to_owned())
            .expect("valid device id");
        let state = Arc::new(ServerState::new(
            server_device_id,
            "TT-Sync Test Server".to_owned(),
            server_files.clone(),
            peer_store,
            Arc::new(SessionManager::new(SessionManagerConfig::default())),
        ));

        let handle = spawn_server(
            "127.0.0.1:0".parse::<SocketAddr>().expect("valid addr"),
            Arc::new(tls),
            state,
            PairingTokenStore::from_state_dir(state_dir.clone()),
        )
        .await
        .expect("spawn server");

        let client_files = Arc::new(MemoryManifestStore::default());
        client_files.insert("default-user/chats/stale.jsonl", b"stale", 1111);
        let client = SyncClient::new(
            format!("https://127.0.0.1:{}", handle.addr.port()),
            Some(spki_sha256),
        )
        .expect("client");
        let engine = ClientSyncEngine::new(
            client,
            client_files.clone(),
            ClientSyncTarget {
                device_id: client_device_id,
                ed25519_seed_b64url: client_seed,
            },
            "TT-Sync test server",
        );

        let options = ClientSyncOptions {
            mode: SyncMode::Mirror,
            selection: DatasetSelection::legacy_v2(),
            require_bundle_zstd: true,
            file_concurrency: 2,
        };
        let pull_report = engine.pull(options, &NoopSyncObserver).await.expect("pull");

        assert_eq!(pull_report.summary.files_total, 1);
        assert_eq!(pull_report.local_applied.files_written, 1);
        assert_eq!(pull_report.local_applied.files_deleted, 1);
        assert_eq!(
            client_files.bytes("default-user/chats/server.jsonl"),
            b"server"
        );
        assert!(!client_files.contains("default-user/chats/stale.jsonl"));

        client_files.insert("default-user/chats/client.jsonl", b"client", 2345);
        let push_report = engine
            .direct_push(
                ClientSyncOptions {
                    mode: SyncMode::Incremental,
                    selection: DatasetSelection::legacy_v2(),
                    require_bundle_zstd: true,
                    file_concurrency: 2,
                },
                &NoopSyncObserver,
            )
            .await
            .expect("push");

        assert_eq!(push_report.summary.files_total, 1);
        assert_eq!(
            server_files.bytes("default-user/chats/client.jsonl"),
            b"client"
        );

        handle.shutdown();
        let _ = std::fs::remove_dir_all(state_dir);
    }

    fn unique_temp_dir() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("ttsync-client-e2e-{now}"))
    }
}
