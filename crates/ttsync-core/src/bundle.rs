use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

use crate::error::SyncError;

pub const FEATURE_BUNDLE_V1: &str = "bundle_v1";
pub const FEATURE_ZSTD_V1: &str = "zstd_v1";

pub const BUNDLE_CONTENT_TYPE: &str = "application/x-ttsync-bundle";
pub const BUNDLE_STREAM_BUFFER_SIZE: usize = 64 * 1024;
pub const BUNDLE_ZSTD_DECODE_BUFFER_SIZE: usize = 1024 * 1024;
pub const MAX_BUNDLE_PATH_LEN: u32 = 16 * 1024;

pub async fn read_u32_be<R>(reader: &mut R) -> Result<u32, SyncError>
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

pub async fn write_u32_be<W>(writer: &mut W, value: u32) -> Result<(), SyncError>
where
    W: AsyncWrite + Unpin,
{
    writer
        .write_all(&value.to_be_bytes())
        .await
        .map_err(|e| SyncError::Io(e.to_string()))
}

pub async fn copy_exact<R, W>(
    reader: &mut R,
    writer: &mut W,
    mut remaining: u64,
    buffer: &mut [u8],
) -> Result<(), SyncError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
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

pub async fn copy_exact_and_expect_eof<R, W>(
    reader: &mut R,
    writer: &mut W,
    size_bytes: u64,
    buffer: &mut [u8],
) -> Result<(), SyncError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    copy_exact(reader, writer, size_bytes, buffer).await?;
    expect_eof(reader, "file stream").await
}

pub async fn expect_eof<R>(reader: &mut R, context: &str) -> Result<(), SyncError>
where
    R: AsyncRead + Unpin,
{
    let mut extra = [0u8; 1];
    let read = reader
        .read(&mut extra)
        .await
        .map_err(|e| SyncError::Io(e.to_string()))?;
    if read == 0 {
        Ok(())
    } else {
        Err(SyncError::InvalidData(format!(
            "{context} contained more bytes than planned"
        )))
    }
}

pub struct ExactSizeReader<R> {
    inner: R,
    remaining: u64,
}

impl<R> ExactSizeReader<R> {
    pub fn new(inner: R, size_bytes: u64) -> Self {
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::error::SyncError;

    use super::{ExactSizeReader, copy_exact_and_expect_eof, read_u32_be, write_u32_be};

    #[tokio::test]
    async fn exact_size_reader_errors_on_short_stream() {
        let (mut reader, mut writer) = tokio::io::duplex(64);
        tokio::spawn(async move {
            writer.write_all(b"abc").await.expect("write");
            drop(writer);
        });

        let mut exact = ExactSizeReader::new(&mut reader, 4);
        let mut buffer = Vec::new();
        let error = exact.read_to_end(&mut buffer).await.expect_err("short");

        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn u32_be_round_trips() {
        let (mut reader, mut writer) = tokio::io::duplex(4);
        tokio::spawn(async move {
            write_u32_be(&mut writer, 42).await.expect("write");
        });

        assert_eq!(read_u32_be(&mut reader).await.expect("read"), 42);
    }

    #[tokio::test]
    async fn copy_exact_and_expect_eof_rejects_extra_bytes() {
        let mut reader = Cursor::new(b"abcd".to_vec());
        let mut writer = tokio::io::sink();
        let mut buffer = [0u8; 2];

        let error = copy_exact_and_expect_eof(&mut reader, &mut writer, 3, &mut buffer)
            .await
            .expect_err("extra byte");

        assert!(matches!(error, SyncError::InvalidData(_)));
    }
}
