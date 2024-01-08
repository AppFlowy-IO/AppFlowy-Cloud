use crate::domain::compression::CompressionType;
use app_error::AppError;
use reqwest::header::HeaderValue;

pub fn compress_type_from_header_value(value: &HeaderValue) -> Result<CompressionType, AppError> {
  let s = value.to_str().map_err(|err| {
    AppError::InvalidRequest(format!("Failed to parse X-Compression-Type: {}", err))
  })?;
  let compression_type = CompressionType::try_from(s).map_err(|err| {
    AppError::InvalidRequest(format!("Failed to parse X-Compression-Type: {}", err))
  })?;
  Ok(compression_type)
}
