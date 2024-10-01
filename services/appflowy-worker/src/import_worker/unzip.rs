use anyhow::Result;
use async_zip::base::read::stream::{Ready, ZipFileReader};
use futures::io::{AsyncBufRead, AsyncReadExt};
use std::path::PathBuf;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;

pub struct UnzipFile {
  pub file_name: String,
  pub unzip_dir_path: PathBuf,
}

pub async fn unzip_async<R: AsyncBufRead + Unpin>(
  mut zip_reader: ZipFileReader<Ready<R>>,
  out: PathBuf,
) -> Result<UnzipFile, anyhow::Error> {
  let mut real_file_name = None;
  while let Some(mut next_reader) = zip_reader.next_with_entry().await? {
    let entry_reader = next_reader.reader_mut();
    let filename = entry_reader.entry().filename().as_str()?;

    if real_file_name.is_none() && filename.ends_with('/') {
      real_file_name = Some(filename.split('/').next().unwrap_or(filename).to_string());
    }

    let output_path = out.join(filename);
    if filename.ends_with('/') {
      fs::create_dir_all(&output_path).await?;
    } else {
      if let Some(parent) = output_path.parent() {
        if !parent.exists() {
          fs::create_dir_all(parent).await?;
        }
      }

      let mut outfile = File::create(&output_path).await?;
      let mut buffer = vec![];
      entry_reader.read_to_end(&mut buffer).await?;
      outfile.write_all(&buffer).await?;
    }

    zip_reader = next_reader.done().await?;
  }

  match real_file_name {
    None => Err(anyhow::anyhow!("No files found in zip archive")),
    Some(file_name) => Ok(UnzipFile {
      file_name: file_name.clone(),
      unzip_dir_path: out.join(file_name),
    }),
  }
}
