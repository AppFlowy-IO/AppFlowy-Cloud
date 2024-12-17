use crate::domain::compression::{CompressionType, X_COMPRESSION_BUFFER_SIZE, X_COMPRESSION_TYPE};
use actix_http::header::HeaderMap;
use actix_web::web::Payload;
use app_error::AppError;

use actix_web::HttpRequest;
use appflowy_ai_client::dto::AIModel;
use async_trait::async_trait;
use byteorder::{ByteOrder, LittleEndian};
use chrono::Utc;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_protocol::spawn_blocking_validate_encode_collab;
use database_entity::dto::CollabParams;
use std::str::FromStr;
use tokio_stream::StreamExt;
use uuid::Uuid;

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

fn value_from_headers<'a>(
  headers: &'a HeaderMap,
  keys: &[&str],
  missing_msg: &str,
) -> Result<&'a str, AppError> {
  keys
    .iter()
    .find_map(|key| headers.get(*key))
    .ok_or_else(|| AppError::InvalidRequest(missing_msg.to_string()))
    .and_then(|header| {
      header
        .to_str()
        .map_err(|err| AppError::InvalidRequest(format!("Failed to parse header: {}", err)))
    })
}

/// Retrieve client version from headers
pub fn client_version_from_headers(headers: &HeaderMap) -> Result<&str, AppError> {
  value_from_headers(
    headers,
    &["Client-Version", "client-version", "client_version"],
    "Missing Client-Version or client-version header",
  )
}

/// Retrieve device ID from headers
pub fn device_id_from_headers(headers: &HeaderMap) -> Result<&str, AppError> {
  value_from_headers(
    headers,
    &["Device-Id", "device-id", "device_id", "Device-ID"],
    "Missing Device-Id or device_id header",
  )
}

/// Create new realtime user for requests from appflowy web
pub fn realtime_user_for_web_request(
  headers: &HeaderMap,
  uid: i64,
) -> Result<RealtimeUser, AppError> {
  let app_version = client_version_from_headers(headers)
    .map(|s| s.to_string())
    .unwrap_or_else(|_| "web".to_string());
  let device_id = device_id_from_headers(headers)
    .map(|s| s.to_string())
    .unwrap_or_else(|_| Uuid::new_v4().to_string());
  let session_id = device_id.clone();
  let user = RealtimeUser {
    uid,
    device_id,
    connect_at: Utc::now().timestamp(),
    session_id,
    app_version,
  };
  Ok(user)
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

#[cfg(test)]
mod tests {
  use super::*;
  use actix_http::header::{HeaderMap, HeaderName, HeaderValue};

  fn setup_headers(key: &str, value: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
      HeaderName::from_str(key).unwrap(),
      HeaderValue::from_str(value).unwrap(),
    );
    headers
  }

  #[test]
  fn test_client_version_valid_variations() {
    let test_cases = [
      ("Client-Version", "1.0.0"),
      ("client-version", "2.0.0"),
      ("client_version", "3.0.0"),
    ];

    for (key, value) in test_cases.iter() {
      let headers = setup_headers(key, value);
      let result = client_version_from_headers(&headers);
      assert!(result.is_ok());
      assert_eq!(result.unwrap(), *value);
    }
  }

  #[test]
  fn test_device_id_valid_variations() {
    let test_cases = [
      ("Device-Id", "device123"),
      ("device-id", "device456"),
      ("device_id", "device789"),
      ("Device-ID", "device000"),
    ];

    for (key, value) in test_cases.iter() {
      let headers = setup_headers(key, value);
      let result = device_id_from_headers(&headers);
      assert!(result.is_ok());
      assert_eq!(result.unwrap(), *value);
    }
  }

  #[test]
  fn test_missing_client_version() {
    let headers = HeaderMap::new();
    let result = client_version_from_headers(&headers);
    assert!(result.is_err());
    match result {
      Err(AppError::InvalidRequest(msg)) => {
        assert_eq!(msg, "Missing Client-Version or client-version header");
      },
      _ => panic!("Expected InvalidRequest error"),
    }
  }

  #[test]
  fn test_missing_device_id() {
    let headers = HeaderMap::new();
    let result = device_id_from_headers(&headers);
    assert!(result.is_err());
    match result {
      Err(AppError::InvalidRequest(msg)) => {
        assert_eq!(msg, "Missing Device-Id or device_id header");
      },
      _ => panic!("Expected InvalidRequest error"),
    }
  }

  #[test]
  fn test_invalid_header_value() {
    let mut headers = HeaderMap::new();
    // Create an invalid UTF-8 header value
    headers.insert(
      HeaderName::from_str("Client-Version").unwrap(),
      HeaderValue::from_bytes(&[0xFF, 0xFF]).unwrap(),
    );

    let result = client_version_from_headers(&headers);
    assert!(result.is_err());
    match result {
      Err(AppError::InvalidRequest(msg)) => {
        assert!(msg.starts_with("Failed to parse header:"));
      },
      _ => panic!("Expected InvalidRequest error"),
    }
  }

  #[test]
  fn test_value_from_headers_multiple_keys_present() {
    let mut headers = HeaderMap::new();
    headers.insert(
      HeaderName::from_str("key1").unwrap(),
      HeaderValue::from_static("value1"),
    );
    headers.insert(
      HeaderName::from_str("key2").unwrap(),
      HeaderValue::from_static("value2"),
    );

    let result = value_from_headers(&headers, &["key1", "key2"], "Missing key");
    assert!(result.is_ok());
    // Should return the first matching key's value
    assert_eq!(result.unwrap(), "value1");
  }
}
