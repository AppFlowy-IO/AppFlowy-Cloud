use crate::domain::compression::{CompressionType, X_COMPRESSION_BUFFER_SIZE, X_COMPRESSION_TYPE};
use actix_http::header::HeaderMap;
use app_error::AppError;
use std::str::FromStr;

#[inline]
pub fn compress_type_from_header_value(headers: &HeaderMap) -> Result<CompressionType, AppError> {
  let compression_type_str = headers
    .get(X_COMPRESSION_TYPE)
    .ok_or(AppError::InvalidRequest(
      "Missing X-Compression-Type header".to_string(),
    ))?
    .to_str()
    .map_err(|err| {
      AppError::InvalidRequest(format!("Failed to parse X-Compression-Type: {}", err))
    })?;
  let buffer_size_str = headers
    .get(X_COMPRESSION_BUFFER_SIZE)
    .ok_or_else(|| {
      AppError::InvalidRequest("Missing X-Compression-Buffer-Size header".to_string())
    })?
    .to_str()
    .map_err(|err| {
      AppError::InvalidRequest(format!(
        "Failed to parse X-Compression-Buffer-Size: {}",
        err
      ))
    })?;

  let buffer_size = usize::from_str(buffer_size_str).map_err(|err| {
    AppError::InvalidRequest(format!(
      "X-Compression-Buffer-Size is not a valid usize: {}",
      err
    ))
  })?;

  match compression_type_str {
    "brotli" => Ok(CompressionType::Brotli { buffer_size }),
    s => Err(AppError::InvalidRequest(format!(
      "Unknown compression type: {}",
      s
    ))),
  }
}
