use actix_multipart::Multipart;
use anyhow::Result;
use async_zip::base::write::ZipFileWriter;
use async_zip::{Compression, ZipEntryBuilder};
use futures_lite::{AsyncWriteExt, StreamExt};
use std::path::PathBuf;
use tokio::fs::File;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use uuid::Uuid;

use actix_web::web::Payload;
use app_error::AppError;
use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

#[allow(dead_code)]
pub async fn create_archive(
  mut body: Multipart,
  file_path: &PathBuf,
) -> Result<(String, usize), anyhow::Error> {
  let mut file_name = "".to_string();
  let mut file_size = 0;

  let archive = File::create(file_path).await?.compat_write();
  let mut writer = ZipFileWriter::new(archive);

  while let Some(Ok(mut field)) = body.next().await {
    let name = match field.content_disposition().and_then(|c| c.get_filename()) {
      Some(filename) => sanitize_filename::sanitize(filename),
      None => Uuid::new_v4().to_string(),
    };

    if file_name.is_empty() {
      file_name = field
        .content_disposition()
        .and_then(|c| c.get_name().map(|f| f.to_string()))
        .unwrap_or_else(|| format!("import-{}", chrono::Local::now().format("%d/%m/%Y %H:%M")));
    }

    // Build the zip entry
    let builder = ZipEntryBuilder::new(name.into(), Compression::Deflate);
    let mut entry_writer = writer.write_entry_stream(builder).await?;
    while let Some(Ok(chunk)) = field.next().await {
      file_size += chunk.len();
      entry_writer.write_all(&chunk).await?;
    }
    entry_writer.close().await?;
  }
  writer.close().await?;
  Ok((file_name, file_size))
}

pub struct LimitedPayload {
  payload: Payload,
  remaining: usize,
}

impl LimitedPayload {
  pub fn new(payload: Payload, limit: usize) -> Self {
    LimitedPayload {
      payload,
      remaining: limit,
    }
  }
}

impl Stream for LimitedPayload {
  type Item = Result<Bytes, AppError>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    if self.remaining == 0 {
      // If there's remaining data in the payload, it's larger than expected
      if futures::ready!(Pin::new(&mut self.payload).poll_next(cx)).is_some() {
        return Poll::Ready(Some(Err(AppError::PayloadTooLarge(
          "Content length exceeded".into(),
        ))));
      }
      return Poll::Ready(None);
    }

    match Pin::new(&mut self.payload).poll_next(cx) {
      Poll::Ready(Some(Ok(chunk))) => {
        let chunk_size = chunk.len();
        if chunk_size > self.remaining {
          return Poll::Ready(Some(Err(AppError::PayloadTooLarge(
            "Content length exceeded".into(),
          ))));
        }
        self.remaining -= chunk_size;
        Poll::Ready(Some(Ok(chunk)))
      },
      Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(AppError::Internal(anyhow::anyhow!(e))))),
      Poll::Ready(None) => {
        if self.remaining > 0 {
          return Poll::Ready(Some(Err(AppError::InvalidRequest(
            "Content shorter than Content-Length".into(),
          ))));
        }
        Poll::Ready(None)
      },
      Poll::Pending => Poll::Pending,
    }
  }
}
