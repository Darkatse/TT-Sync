use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::http::{HeaderMap, header};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadBuf};
use ttsync_contract::path::SyncPath;
use ttsync_core::error::SyncError;
use ttsync_core::ports::ManifestStore;

use super::plans::TransferMeta;

pub(super) const BUNDLE_CONTENT_TYPE: &str = "application/x-ttsync-bundle";
pub(super) const MAX_BUNDLE_PATH_LEN: u32 = 16 * 1024;

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

pub(super) async fn read_u32_be<R>(reader: &mut R) -> Result<u32, SyncError>
where
    R: AsyncRead + Unpin,
{
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;
    Ok(u32::from_be_bytes(buf))
}

pub(super) async fn write_bundle_download<M>(
    manifest_store: Arc<M>,
    transfer: Vec<(SyncPath, TransferMeta)>,
    mut out: tokio::io::DuplexStream,
) -> Result<(), SyncError>
where
    M: ManifestStore + 'static,
{
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

        out.write_all(&path_len.to_be_bytes())
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;
        out.write_all(path_bytes)
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;

        let mut reader = manifest_store.read_file(&path).await?;
        copy_exact(&mut reader, &mut out, meta.size_bytes).await?;
    }

    out.write_all(&0u32.to_be_bytes())
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;

    Ok(())
}

async fn copy_exact<R, W>(
    reader: &mut R,
    writer: &mut W,
    mut remaining: u64,
) -> Result<(), SyncError>
where
    R: AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buffer = vec![0u8; 64 * 1024];
    while remaining > 0 {
        let to_read = (buffer.len() as u64).min(remaining) as usize;
        let read = reader
            .read(&mut buffer[..to_read])
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;
        if read == 0 {
            return Err(SyncError::Io("unexpected EOF in bundle stream".into()));
        }
        writer
            .write_all(&buffer[..read])
            .await
            .map_err(|e| SyncError::Io(e.to_string()))?;
        remaining -= read as u64;
    }
    Ok(())
}

pub(super) struct ExactSizeReader<R> {
    inner: R,
    remaining: u64,
}

impl<R> ExactSizeReader<R> {
    pub(super) fn new(inner: R, size_bytes: u64) -> Self {
        Self {
            inner,
            remaining: size_bytes,
        }
    }
}

impl<R> AsyncRead for ExactSizeReader<R>
where
    R: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.remaining == 0 {
            return Poll::Ready(Ok(()));
        }

        let max = (self.remaining as usize).min(buf.remaining());
        if max == 0 {
            return Poll::Ready(Ok(()));
        }

        let dst = buf.initialize_unfilled_to(max);
        let mut limited = ReadBuf::new(dst);
        match Pin::new(&mut self.inner).poll_read(cx, &mut limited) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => {
                let read = limited.filled().len();
                if read == 0 {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "bundle file stream ended early",
                    )));
                }

                buf.advance(read);
                self.remaining -= read as u64;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
        }
    }
}
