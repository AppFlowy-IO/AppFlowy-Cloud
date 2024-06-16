use crate::http::log_request_id;
use crate::native::GetCollabAction;
use crate::ws::{ConnectInfo, WSClientConnectURLProvider, WSClientHttpSender, WSError};
use crate::{spawn_blocking_brotli_compress, Client};
use crate::{RefreshTokenAction, RefreshTokenRetryCondition};
use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;
use bytes::Bytes;
use collab_rt_entity::HttpRealtimeMessage;
use database_entity::dto::{CollabParams, QueryCollabParams};
use database_entity::file_dto::{
  CompleteUploadRequest, CreateUploadRequest, CreateUploadResponse, UploadPartResponse,
};
use futures_util::stream;
use prost::Message;
use reqwest::{Body, Method};
use shared_entity::dto::workspace_dto::CollabResponse;
use shared_entity::response::{AppResponse, AppResponseError};
use std::future::Future;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio_retry::strategy::{ExponentialBackoff, FixedInterval};
use tokio_retry::{Retry, RetryIf};
use tracing::{event, info, instrument, trace};

const CHUNK_SIZE: usize = 5 * 1024 * 1024; // 5 MB
impl Client {
  pub async fn create_upload(
    &self,
    workspace_id: &str,
    req: CreateUploadRequest,
  ) -> Result<CreateUploadResponse, AppResponseError> {
    trace!("create_upload: {}", req);
    let url = format!(
      "{}/api/file_storage/{workspace_id}/create_upload",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&req)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<CreateUploadResponse>::from_response(resp)
      .await?
      .into_data()
  }

  /// Upload a part of a file. The part number should be 1-based.
  ///
  /// In Amazon S3, the minimum chunk size for multipart uploads is 5 MB,except for the last part,
  /// which can be smaller.(https://docs.aws.amazon.com/AmazonS3/latest/userguide/qfacts.html)
  pub async fn upload_part(
    &self,
    workspace_id: &str,
    parent_dir: &str,
    file_id: &str,
    upload_id: &str,
    part_number: i32,
    body: Vec<u8>,
  ) -> Result<UploadPartResponse, AppResponseError> {
    if body.is_empty() {
      return Err(AppResponseError::from(AppError::InvalidRequest(
        "Empty body".to_string(),
      )));
    }

    let url = format!(
      "{}/api/file_storage/{workspace_id}/upload_part/{parent_dir}/{file_id}/{upload_id}/{part_number}",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .body(body)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<UploadPartResponse>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn complete_upload(
    &self,
    workspace_id: &str,
    req: CompleteUploadRequest,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/file_storage/{}/complete_upload",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&req)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all)]
  pub async fn get_collab(
    &self,
    params: QueryCollabParams,
  ) -> Result<CollabResponse, AppResponseError> {
    info!("get collab:{}", params);
    // 2 seconds, 4 seconds, 8 seconds
    let retry_strategy = ExponentialBackoff::from_millis(2).factor(1000).take(3);
    let action = GetCollabAction::new(self.clone(), params);
    Retry::spawn(retry_strategy, action).await
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn post_realtime_msg(
    &self,
    device_id: &str,
    msg: client_websocket::Message,
  ) -> Result<(), AppResponseError> {
    let device_id = device_id.to_string();
    let payload =
      spawn_blocking_brotli_compress(msg.into_data(), 6, self.config.compression_buffer_size)
        .await?;

    let msg = HttpRealtimeMessage { device_id, payload }.encode_to_vec();
    let body = Body::wrap_stream(stream::iter(vec![Ok::<_, reqwest::Error>(msg)]));
    let url = format!("{}/api/realtime/post/stream", self.base_url);
    let resp = self
      .http_client_with_auth_compress(Method::POST, &url)
      .await?
      .body(body)
      .send()
      .await?;
    crate::http::log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn create_collab_list(
    &self,
    workspace_id: &str,
    params_list: Vec<CollabParams>,
  ) -> Result<(), AppResponseError> {
    let url = self.batch_create_collab_url(workspace_id);

    // Parallel compression
    let compression_tasks: Vec<_> = params_list
      .into_iter()
      .map(|params| {
        let config = self.config.clone();
        af_spawn(async move {
          let data = params.to_bytes().map_err(AppError::from)?;
          spawn_blocking_brotli_compress(
            data,
            config.compression_quality,
            config.compression_buffer_size,
          )
          .await
        })
      })
      .collect();

    let mut framed_data = Vec::new();
    let mut size_count = 0;
    for task in compression_tasks {
      let compressed = task.await??;
      // The length of a u32 in bytes is 4. The server uses a u32 to read the size of each data frame,
      // hence the frame size header is always 4 bytes. It's crucial not to alter this size value,
      // as the server's logic for frame size reading is based on this fixed 4-byte length.
      // note:
      // the size of a u32 is a constant 4 bytes across all platforms that Rust supports.
      let size = compressed.len() as u32;
      framed_data.extend_from_slice(&size.to_be_bytes());
      framed_data.extend_from_slice(&compressed);
      size_count += size;
    }
    event!(
      tracing::Level::INFO,
      "create batch collab with size: {}",
      size_count
    );
    let body = Body::wrap_stream(stream::once(async { Ok::<_, AppError>(framed_data) }));
    let resp = self
      .http_client_with_auth_compress(Method::POST, &url)
      .await?
      .timeout(Duration::from_secs(60))
      .body(body)
      .send()
      .await?;

    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  /// Refreshes the access token using the stored refresh token.
  ///
  /// This function attempts to refresh the access token by sending a request to the authentication server
  /// using the stored refresh token. If successful, it updates the stored access token with the new one
  /// received from the server.
  #[instrument(level = "debug", skip_all, err)]
  pub async fn refresh_token(&self, reason: &str) -> Result<(), AppResponseError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    self.refresh_ret_txs.write().push(tx);

    if !self.is_refreshing_token.load(Ordering::SeqCst) {
      self.is_refreshing_token.store(true, Ordering::SeqCst);

      info!("refresh token reason:{}", reason);
      let result = self.inner_refresh_token().await;
      let txs = std::mem::take(&mut *self.refresh_ret_txs.write());
      for tx in txs {
        let _ = tx.send(result.clone());
      }
      self.is_refreshing_token.store(false, Ordering::SeqCst);
    }

    // Wait for the result of the refresh token request.
    match tokio::time::timeout(Duration::from_secs(60), rx).await {
      Ok(Ok(result)) => result,
      Ok(Err(err)) => Err(AppError::Internal(anyhow!("refresh token error: {}", err)).into()),
      Err(_) => Err(AppError::RequestTimeout("refresh token timeout".to_string()).into()),
    }
  }

  async fn inner_refresh_token(&self) -> Result<(), AppResponseError> {
    let retry_strategy = FixedInterval::new(Duration::from_secs(2)).take(4);
    let action = RefreshTokenAction::new(self.token.clone(), self.gotrue_client.clone());
    match RetryIf::spawn(retry_strategy, action, RefreshTokenRetryCondition).await {
      Ok(_) => {
        event!(tracing::Level::INFO, "refresh token success");
        Ok(())
      },
      Err(err) => {
        let err = AppError::from(err);
        event!(tracing::Level::ERROR, "refresh token failed: {}", err);

        // If the error is an OAuth error, unset the token.
        if err.is_unauthorized() {
          self.token.write().unset();
        }
        Err(err.into())
      },
    }
  }
}

#[async_trait]
impl WSClientHttpSender for Client {
  async fn send_ws_msg(
    &self,
    device_id: &str,
    message: client_websocket::Message,
  ) -> Result<(), WSError> {
    self
      .post_realtime_msg(device_id, message)
      .await
      .map_err(|err| WSError::Http(err.to_string()))
  }
}

#[async_trait]
impl WSClientConnectURLProvider for Client {
  fn connect_ws_url(&self) -> String {
    self.ws_addr.clone()
  }

  async fn connect_info(&self) -> Result<ConnectInfo, WSError> {
    let conn_info = self
      .ws_connect_info(true)
      .await
      .map_err(|err| WSError::Http(err.to_string()))?;
    Ok(conn_info)
  }
}

// TODO(nathan): spawn for wasm
pub fn af_spawn<T>(future: T) -> tokio::task::JoinHandle<T::Output>
where
  T: Future + Send + 'static,
  T::Output: Send + 'static,
{
  tokio::spawn(future)
}

pub struct ChunkedBytes {
  pub data: Bytes,
  pub offsets: Vec<(usize, usize)>,
}

impl Deref for ChunkedBytes {
  type Target = Bytes;

  fn deref(&self) -> &Self::Target {
    &self.data
  }
}

impl ChunkedBytes {
  pub fn from_bytes(data: Bytes) -> Result<Self, anyhow::Error> {
    let offsets = split_into_chunks(&data);
    Ok(ChunkedBytes { data, offsets })
  }

  pub async fn from_file(file_path: &PathBuf) -> Result<Self, anyhow::Error> {
    let mut file = tokio::fs::File::open(file_path).await?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await?;
    let data = Bytes::from(buffer);

    let offsets = split_into_chunks(&data);
    Ok(ChunkedBytes { data, offsets })
  }

  pub fn iter(&self) -> ChunkedBytesIterator {
    ChunkedBytesIterator {
      chunked_data: self,
      current_index: 0,
    }
  }
}

pub struct ChunkedBytesIterator<'a> {
  chunked_data: &'a ChunkedBytes,
  current_index: usize,
}
impl<'a> Iterator for ChunkedBytesIterator<'a> {
  type Item = Bytes;

  fn next(&mut self) -> Option<Self::Item> {
    if self.current_index >= self.chunked_data.offsets.len() {
      None
    } else {
      let (start, end) = self.chunked_data.offsets[self.current_index];
      self.current_index += 1;
      Some(self.chunked_data.data.slice(start..end))
    }
  }
}
// Function to split input bytes into several chunks and return offsets
pub fn split_into_chunks(data: &Bytes) -> Vec<(usize, usize)> {
  let mut offsets = Vec::new();
  let mut start = 0;

  while start < data.len() {
    let end = std::cmp::min(start + CHUNK_SIZE, data.len());
    offsets.push((start, end));
    start = end;
  }
  offsets
}

// Function to get chunk data using chunk number
pub async fn get_chunk(
  data: Bytes,
  chunk_number: usize,
  offsets: &[(usize, usize)],
) -> Result<Bytes, anyhow::Error> {
  if chunk_number >= offsets.len() {
    return Err(anyhow!("Chunk number out of range"));
  }

  let (start, end) = offsets[chunk_number];
  let chunk = data.slice(start..end);

  Ok(chunk)
}

#[cfg(test)]
mod tests {
  use crate::ChunkedBytes;
  use bytes::Bytes;
  use std::env::temp_dir;
  use tokio::io::AsyncWriteExt;

  #[tokio::test]
  async fn test_chunked_bytes_less_than_chunk_size() {
    let data = Bytes::from(vec![0; 1024 * 1024]); // 1 MB of zeroes
    let chunked_data = ChunkedBytes::from_bytes(data.clone()).unwrap();

    // Check if the offsets are correct
    assert_eq!(chunked_data.offsets.len(), 1); // Should have 1 chunk
    assert_eq!(chunked_data.offsets[0], (0, 1024 * 1024));

    // Check if the data can be iterated correctly
    let mut iter = chunked_data.iter();
    assert_eq!(iter.next().unwrap().len(), 1024 * 1024);
    assert!(iter.next().is_none());
  }

  #[tokio::test]
  async fn test_chunked_bytes_from_bytes() {
    let data = Bytes::from(vec![0; 15 * 1024 * 1024]); // 15 MB of zeroes
    let chunked_data = ChunkedBytes::from_bytes(data.clone()).unwrap();

    // Check if the offsets are correct
    assert_eq!(chunked_data.offsets.len(), 3); // Should have 3 chunks
    assert_eq!(chunked_data.offsets[0], (0, 5 * 1024 * 1024));
    assert_eq!(chunked_data.offsets[1], (5 * 1024 * 1024, 10 * 1024 * 1024));
    assert_eq!(
      chunked_data.offsets[2],
      (10 * 1024 * 1024, 15 * 1024 * 1024)
    );

    // Check if the data can be iterated correctly
    let mut iter = chunked_data.iter();
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert!(iter.next().is_none());
  }

  #[tokio::test]
  async fn test_chunked_bytes_from_file() {
    // Create a temporary file with 15 MB of zeroes
    let mut file_path = temp_dir();
    file_path.push("test_file");

    let mut file = tokio::fs::File::create(&file_path).await.unwrap();
    file.write_all(&vec![0; 15 * 1024 * 1024]).await.unwrap();
    file.flush().await.unwrap();

    // Read the file into ChunkedBytes
    let chunked_data = ChunkedBytes::from_file(&file_path).await.unwrap();

    // Check if the offsets are correct
    assert_eq!(chunked_data.offsets.len(), 3); // Should have 3 chunks
    assert_eq!(chunked_data.offsets[0], (0, 5 * 1024 * 1024));
    assert_eq!(chunked_data.offsets[1], (5 * 1024 * 1024, 10 * 1024 * 1024));
    assert_eq!(
      chunked_data.offsets[2],
      (10 * 1024 * 1024, 15 * 1024 * 1024)
    );

    // Check if the data can be iterated correctly
    let mut iter = chunked_data.iter();
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert!(iter.next().is_none());

    // Clean up the temporary file
    tokio::fs::remove_file(file_path).await.unwrap();
  }
}
