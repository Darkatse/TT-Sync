use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use ttsync_contract::manifest::ManifestEntryV2;
use ttsync_contract::path::SyncPath;
use ttsync_core::bundle::{
    BUNDLE_STREAM_BUFFER_SIZE, ExactSizeReader, MAX_BUNDLE_PATH_LEN, copy_exact_and_expect_eof,
    expect_eof, read_u32_be, write_u32_be,
};
use ttsync_core::error::SyncError;

use crate::workspace::{ClientWorkspace, WorkspaceWriteError};

#[derive(Debug)]
pub struct BundleFileProgress {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug)]
pub struct BundleWriteError {
    error: SyncError,
    target_changed: bool,
}

impl BundleWriteError {
    fn unchanged(error: SyncError) -> Self {
        Self {
            error,
            target_changed: false,
        }
    }

    fn from_workspace(error: WorkspaceWriteError) -> Self {
        Self {
            target_changed: error.target_changed(),
            error: error.into_error(),
        }
    }

    pub fn target_changed(&self) -> bool {
        self.target_changed
    }

    pub fn into_error(self) -> SyncError {
        self.error
    }
}

pub async fn write_bundle_to_workspace<W, R, F>(
    workspace: &W,
    transfer_entries: Vec<ManifestEntryV2>,
    reader: &mut R,
    mut on_file_written: F,
) -> Result<(), BundleWriteError>
where
    W: ClientWorkspace,
    R: AsyncRead + Send + Unpin,
    F: FnMut(BundleFileProgress),
{
    let files_total = transfer_entries.len();
    let mut files_written = 0usize;
    let mut remaining = transfer_entries
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect::<std::collections::HashMap<SyncPath, ManifestEntryV2>>();

    loop {
        let path_len = read_u32_be(reader)
            .await
            .map_err(BundleWriteError::unchanged)?;
        if path_len == 0 {
            break;
        }
        if path_len > MAX_BUNDLE_PATH_LEN {
            return Err(BundleWriteError::unchanged(SyncError::InvalidData(
                format!("bundle path too long: {} bytes", path_len),
            )));
        }

        let mut path_bytes = vec![0u8; path_len as usize];
        reader
            .read_exact(&mut path_bytes)
            .await
            .map_err(|e| BundleWriteError::unchanged(SyncError::Io(e.to_string())))?;

        let path_text = String::from_utf8(path_bytes).map_err(|_| {
            BundleWriteError::unchanged(SyncError::InvalidData("bundle path is not UTF-8".into()))
        })?;
        let sync_path = SyncPath::new(path_text)
            .map_err(|e| BundleWriteError::unchanged(SyncError::InvalidData(e.to_string())))?;

        let entry = remaining
            .remove(&sync_path)
            .ok_or_else(|| SyncError::NotFound(format!("bundle file not in plan: {}", sync_path)))
            .map_err(BundleWriteError::unchanged)?;

        let mut exact = ExactSizeReader::new(&mut *reader, entry.size_bytes);
        workspace
            .write_file(&entry.path, &mut exact, entry.modified_ms)
            .await
            .map_err(BundleWriteError::from_workspace)?;

        files_written += 1;
        on_file_written(BundleFileProgress {
            path: entry.path.to_string(),
            size_bytes: entry.size_bytes,
        });
    }

    if !remaining.is_empty() {
        return Err(BundleWriteError::unchanged(SyncError::InvalidData(
            format!(
                "bundle ended early: {}/{} files received",
                files_written, files_total
            ),
        )));
    }

    expect_eof(reader, "bundle stream")
        .await
        .map_err(BundleWriteError::unchanged)
}

pub async fn write_bundle_upload<W>(
    workspace: &W,
    transfer: Vec<ManifestEntryV2>,
    mut out: tokio::io::DuplexStream,
    progress: tokio::sync::mpsc::UnboundedSender<BundleFileProgress>,
) -> Result<(), SyncError>
where
    W: ClientWorkspace,
{
    let mut buffer = vec![0u8; BUNDLE_STREAM_BUFFER_SIZE];

    for entry in transfer {
        let path_bytes = entry.path.as_str().as_bytes();
        let path_len = u32::try_from(path_bytes.len())
            .map_err(|_| SyncError::InvalidData("bundle path is too long to encode".into()))?;
        if path_len > MAX_BUNDLE_PATH_LEN {
            return Err(SyncError::InvalidData(format!(
                "bundle path is too long to encode: {} bytes",
                path_len
            )));
        }

        write_u32_be(&mut out, path_len).await?;
        out.write_all(path_bytes)
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;

        let mut reader = workspace.read_file(&entry.path).await?;
        copy_exact_and_expect_eof(&mut reader, &mut out, entry.size_bytes, &mut buffer).await?;
        let _ = progress.send(BundleFileProgress {
            path: entry.path.to_string(),
            size_bytes: entry.size_bytes,
        });
    }

    write_u32_be(&mut out, 0).await
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use ttsync_contract::manifest::ManifestV2;
    use ttsync_contract::path::SyncPath;
    use ttsync_core::dataset::ResolvedDatasetPolicy;

    use super::*;

    struct EmptyWorkspace;

    impl ClientWorkspace for EmptyWorkspace {
        async fn scan(&self, _policy: ResolvedDatasetPolicy) -> Result<ManifestV2, SyncError> {
            unreachable!("bundle reader test does not scan")
        }

        async fn read_file(
            &self,
            _path: &SyncPath,
        ) -> Result<Box<dyn tokio::io::AsyncRead + Send + Unpin>, SyncError> {
            unreachable!("bundle reader test does not read local files")
        }

        async fn write_file(
            &self,
            _path: &SyncPath,
            _data: &mut (dyn tokio::io::AsyncRead + Send + Unpin),
            _modified_ms: u64,
        ) -> Result<(), WorkspaceWriteError> {
            unreachable!("empty transfer must not write files")
        }

        async fn delete_file(&self, _path: &SyncPath) -> Result<(), WorkspaceWriteError> {
            unreachable!("bundle reader test does not delete files")
        }
    }

    #[tokio::test]
    async fn write_bundle_to_workspace_rejects_trailing_bytes_after_terminator() {
        let mut reader = Cursor::new(vec![0, 0, 0, 0, b'x']);

        let error = write_bundle_to_workspace(&EmptyWorkspace, vec![], &mut reader, |_| {})
            .await
            .expect_err("trailing byte");

        assert!(matches!(error.into_error(), SyncError::InvalidData(_)));
    }
}
