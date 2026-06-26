use std::sync::Arc;

use axum::http::{HeaderMap, header};
use ttsync_contract::path::SyncPath;
pub(super) use ttsync_core::bundle::{
    BUNDLE_CONTENT_TYPE, ExactSizeReader, MAX_BUNDLE_PATH_LEN, expect_eof, read_u32_be,
};
use ttsync_core::bundle::{BUNDLE_STREAM_BUFFER_SIZE, copy_exact_and_expect_eof, write_u32_be};
use ttsync_core::error::SyncError;
use ttsync_core::ports::ManifestStore;

use super::plans::TransferMeta;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BundleContentEncoding {
    Identity,
    Zstd,
}

pub(super) fn accepts_zstd(headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(header::ACCEPT_ENCODING) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };

    value.split(',').any(|part| {
        let name = part.split(';').next().unwrap_or_default().trim();
        name.eq_ignore_ascii_case("zstd")
    })
}

pub(super) fn request_content_encoding(
    headers: &HeaderMap,
) -> Result<BundleContentEncoding, SyncError> {
    let Some(value) = headers.get(header::CONTENT_ENCODING) else {
        return Ok(BundleContentEncoding::Identity);
    };

    let value = value
        .to_str()
        .map_err(|_| SyncError::InvalidData("invalid Content-Encoding header".into()))?
        .trim();

    if value.eq_ignore_ascii_case("identity") {
        Ok(BundleContentEncoding::Identity)
    } else if value.eq_ignore_ascii_case("zstd") {
        Ok(BundleContentEncoding::Zstd)
    } else {
        Err(SyncError::InvalidData(format!(
            "unsupported Content-Encoding: {}",
            value
        )))
    }
}

pub(super) async fn write_bundle_download<M>(
    manifest_store: Arc<M>,
    transfer: Vec<(SyncPath, TransferMeta)>,
    mut out: tokio::io::DuplexStream,
) -> Result<(), SyncError>
where
    M: ManifestStore + 'static,
{
    let mut buffer = vec![0u8; BUNDLE_STREAM_BUFFER_SIZE];
    for (path, meta) in transfer {
        let path_bytes = path.as_str().as_bytes();
        let path_len = u32::try_from(path_bytes.len())
            .map_err(|_| SyncError::InvalidData("bundle path is too long to encode".into()))?;
        if path_len > MAX_BUNDLE_PATH_LEN {
            return Err(SyncError::InvalidData(format!(
                "bundle path is too long to encode: {} bytes",
                path_len
            )));
        }

        write_u32_be(&mut out, path_len).await?;
        tokio::io::AsyncWriteExt::write_all(&mut out, path_bytes)
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;

        let mut reader = manifest_store.read_file(&path).await?;
        copy_exact_and_expect_eof(&mut reader, &mut out, meta.size_bytes, &mut buffer).await?;
    }

    write_u32_be(&mut out, 0).await?;

    Ok(())
}
