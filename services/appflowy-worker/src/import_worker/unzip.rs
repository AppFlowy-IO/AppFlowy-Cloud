use anyhow::{Context, Result};
use async_zip::base::read::stream::{Ready, ZipFileReader};
use async_zip::{StringEncoding, ZipString};
use futures::io::{AsyncBufRead, AsyncReadExt};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use tracing::error;

pub struct UnzipFile {
  pub file_name: String,
  pub unzip_dir_path: PathBuf,
}

pub async fn unzip_async<R: AsyncBufRead + Unpin>(
  mut zip_reader: ZipFileReader<Ready<R>>,
  out_dir: PathBuf,
) -> Result<UnzipFile, anyhow::Error> {
  let mut real_file_name = None;
  while let Some(mut next_reader) = zip_reader.next_with_entry().await? {
    let entry_reader = next_reader.reader_mut();
    let filename = get_filename(entry_reader.entry().filename())
      .with_context(|| "Failed to extract filename from entry".to_string())?;

    // Save the root folder name if we haven't done so yet
    if real_file_name.is_none() && filename.ends_with('/') {
      real_file_name = Some(filename.split('/').next().unwrap_or(&filename).to_string());
    }

    let output_path = out_dir.join(&filename);
    if filename.ends_with('/') {
      fs::create_dir_all(&output_path)
        .await
        .with_context(|| format!("Failed to create directory: {}", output_path.display()))?;
    } else {
      // Ensure parent directories exist
      if let Some(parent) = output_path.parent() {
        if !parent.exists() {
          fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create parent directory: {}", parent.display()))?;
        }
      }

      // Write file contents
      let mut outfile = File::create(&output_path)
        .await
        .with_context(|| format!("Failed to create file: {}", output_path.display()))?;
      let mut buffer = vec![];
      match entry_reader.read_to_end(&mut buffer).await {
        Ok(_) => {
          outfile
            .write_all(&buffer)
            .await
            .with_context(|| format!("Failed to write data to file: {}", output_path.display()))?;
        },
        Err(err) => {
          error!(
            "Failed to read entry: {:?}. Error: {:?}",
            entry_reader.entry(),
            err,
          );
          return Err(anyhow::anyhow!(
            "Unexpected EOF while reading: {}",
            filename
          ));
        },
      }
    }

    // Move to the next file in the zip
    zip_reader = next_reader.done().await?;
  }

  match real_file_name {
    None => Err(anyhow::anyhow!("No files found in the zip archive")),
    Some(file_name) => Ok(UnzipFile {
      file_name: file_name.clone(),
      unzip_dir_path: out_dir.join(file_name),
    }),
  }
}

pub fn get_filename(zip_string: &ZipString) -> Result<String, anyhow::Error> {
  match zip_string.encoding() {
    StringEncoding::Utf8 => match zip_string.as_str() {
      Ok(valid_str) => Ok(valid_str.to_string()),
      Err(err) => Err(err.into()),
    },

    StringEncoding::Raw => {
      let raw_bytes = zip_string.as_bytes();
      let os_string = OsString::from_vec(raw_bytes.to_vec());
      Ok(os_string.to_string_lossy().into_owned())
    },
  }
}
