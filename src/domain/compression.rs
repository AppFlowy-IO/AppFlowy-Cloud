use anyhow::anyhow;
use app_error::AppError;
use brotli::{CompressorReader, Decompressor};

use std::io::Read;

pub const X_COMPRESSION_TYPE: &str = "X-Compression-Type";

pub enum CompressionType {
  Brotli,
}

impl TryFrom<&str> for CompressionType {
  type Error = anyhow::Error;

  fn try_from(value: &str) -> Result<Self, Self::Error> {
    match value {
      "brotli" => Ok(CompressionType::Brotli),
      _ => Err(anyhow!("Unknown compression type: {}", value)),
    }
  }
}

pub fn compress(data: &[u8], quality: u32) -> Result<Vec<u8>, AppError> {
  let mut compressor = CompressorReader::new(data, 10240, quality, 22);
  let mut compressed_data = Vec::new();
  compressor
    .read_to_end(&mut compressed_data)
    .map_err(|err| AppError::InvalidRequest(format!("Failed to compress data: {}", err)))?;
  Ok(compressed_data)
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, AppError> {
  let mut decompressor = Decompressor::new(data, 10240);
  let mut decompressed_data = Vec::new();
  decompressor
    .read_to_end(&mut decompressed_data)
    .map_err(|err| AppError::InvalidRequest(format!("Failed to decompress data: {}", err)))?;
  Ok(decompressed_data)
}
