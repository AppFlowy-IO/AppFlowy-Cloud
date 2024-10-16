use actix_multipart::Multipart;
use anyhow::Result;
use async_zip::base::write::ZipFileWriter;
use async_zip::{Compression, ZipEntryBuilder};
use futures_lite::{AsyncWriteExt, StreamExt};
use std::path::PathBuf;
use tokio::fs::File;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use uuid::Uuid;

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
