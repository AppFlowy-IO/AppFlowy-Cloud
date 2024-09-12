use app_error::AppError;
use brotli::{CompressorReader, Decompressor};
use std::io::Read;

pub const X_COMPRESSION_TYPE: &str = "X-Compression-Type";

pub const X_COMPRESSION_BUFFER_SIZE: &str = "X-Compression-Buffer-Size";
pub enum CompressionType {
  Brotli { buffer_size: usize },
}

impl CompressionType {
  pub fn buffer_size(&self) -> usize {
    match self {
      CompressionType::Brotli { buffer_size } => *buffer_size,
    }
  }
}

pub async fn compress(
  data: Vec<u8>,
  quality: u32,
  buffer_size: usize,
) -> Result<Vec<u8>, AppError> {
  tokio::task::spawn_blocking(move || {
    let mut compressor = CompressorReader::new(&*data, buffer_size, quality, 22);
    let mut compressed_data = Vec::new();
    compressor
      .read_to_end(&mut compressed_data)
      .map_err(|err| AppError::InvalidRequest(format!("Failed to compress data: {}", err)))?;
    Ok(compressed_data)
  })
  .await
  .map_err(AppError::from)?
}

pub fn decompress(data: Vec<u8>, buffer_size: usize) -> Result<Vec<u8>, AppError> {
  let mut decompressor = Decompressor::new(&*data, buffer_size);
  let mut decompressed_data = Vec::new();
  decompressor
    .read_to_end(&mut decompressed_data)
    .map_err(|err| {
      AppError::InvalidRequest(format!("Failed to decompress data:{} {}", data.len(), err))
    })?;
  Ok(decompressed_data)
}

pub async fn blocking_decompress(data: Vec<u8>, buffer_size: usize) -> Result<Vec<u8>, AppError> {
  tokio::task::spawn_blocking(move || decompress(data, buffer_size))
    .await
    .map_err(AppError::from)?
}
