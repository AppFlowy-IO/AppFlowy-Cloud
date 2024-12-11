use crate::domain::compression::{CompressionType, X_COMPRESSION_BUFFER_SIZE, X_COMPRESSION_TYPE};
use actix_http::header::HeaderMap;
use actix_web::web::Payload;
use app_error::AppError;

use actix_web::HttpRequest;
use appflowy_ai_client::dto::AIModel;
use async_trait::async_trait;
use byteorder::{ByteOrder, LittleEndian};
use collab_rt_protocol::spawn_blocking_validate_encode_collab;
use database_entity::dto::CollabParams;
use std::str::FromStr;
use tokio_stream::StreamExt;

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

fn value_from_headers(
  headers: &HeaderMap,
  keys: &[&str],
  missing_msg: &str,
) -> Result<String, AppError> {
  keys
    .iter()
    .find_map(|key| headers.get(*key))
    .ok_or_else(|| AppError::InvalidRequest(missing_msg.to_string()))
    .and_then(|header| {
      header
        .to_str()
        .map_err(|err| AppError::InvalidRequest(format!("Failed to parse header: {}", err)))
    })
    .map(|s| s.to_string())
}

/// Retrieve client version from headers
pub fn client_version_from_headers(headers: &HeaderMap) -> Result<String, AppError> {
  value_from_headers(
    headers,
    &["Client-Version", "client-version"],
    "Missing Client-Version or client-version header",
  )
}

/// Retrieve device ID from headers
pub fn device_id_from_headers(headers: &HeaderMap) -> Result<String, AppError> {
  value_from_headers(
    headers,
    &["Device-Id", "device_id", "device-id"],
    "Missing Device-Id or device_id header",
  )
}

#[async_trait]
pub trait CollabValidator {
  async fn check_encode_collab(&self) -> Result<(), AppError>;
}

#[async_trait]
impl CollabValidator for CollabParams {
  async fn check_encode_collab(&self) -> Result<(), AppError> {
    spawn_blocking_validate_encode_collab(
      &self.object_id,
      &self.encoded_collab_v1,
      &self.collab_type,
    )
    .await
    .map_err(|err| AppError::NoRequiredData(err.to_string()))
  }
}

pub struct PayloadReader {
  payload: Payload,
  buffer: Vec<u8>,
  buf_start: usize,
  buf_end: usize,
}

impl PayloadReader {
  pub fn new(payload: Payload) -> Self {
    Self {
      payload,
      buffer: Vec::new(),
      buf_start: 0,
      buf_end: 0,
    }
  }

  pub async fn read_exact(&mut self, dest: &mut [u8]) -> actix_web::Result<()> {
    let mut written = 0;
    while written < dest.len() {
      if self.buf_start == self.buf_end {
        self.fill_buffer().await?;
      }

      let current_dest = &mut dest[written..];
      let current_src = &self.buffer[self.buf_start..self.buf_end];
      let n = copy_buffer(current_src, current_dest);
      written += n;
      self.buf_start += n;
    }
    Ok(())
  }

  pub async fn read_u32_little_endian(&mut self) -> actix_web::Result<u32> {
    self.fill_at_least(4).await?;

    let bytes: [u8; 4] = [
      self.buffer[self.buf_start],
      self.buffer[self.buf_start + 1],
      self.buffer[self.buf_start + 2],
      self.buffer[self.buf_start + 3],
    ];
    self.buf_start += 4;

    Ok(LittleEndian::read_u32(&bytes))
  }

  async fn fill_at_least(&mut self, min_len: usize) -> actix_web::Result<()> {
    while self.len() < min_len {
      let n = self.fill_buffer().await?;
      if n == 0 {
        return Err(AppError::InvalidRequest("unexpected EOF".to_string()).into());
      }
    }
    Ok(())
  }

  fn len(&self) -> usize {
    self.buf_end - self.buf_start
  }

  async fn fill_buffer(&mut self) -> actix_web::Result<usize> {
    if self.buf_start == self.buf_end {
      self.buffer.clear();
      self.buf_start = 0;
      self.buf_end = 0;
    }

    let bytes = self.payload.try_next().await?;
    match bytes {
      Some(bytes) => {
        self.buffer.extend_from_slice(&bytes);
        self.buf_end += bytes.len();
        Ok(bytes.len())
      },
      None => Ok(0),
    }
  }
}

fn copy_buffer(src: &[u8], dest: &mut [u8]) -> usize {
  let bytes_to_copy = std::cmp::min(src.len(), dest.len());
  dest[..bytes_to_copy].copy_from_slice(&src[..bytes_to_copy]);
  bytes_to_copy
}

#[inline]
pub(crate) fn ai_model_from_header(req: &HttpRequest) -> AIModel {
  req
    .headers()
    .get("ai-model")
    .and_then(|header| {
      let header = header.to_str().ok()?;
      AIModel::from_str(header).ok()
    })
    .unwrap_or(AIModel::GPT4oMini)
}
