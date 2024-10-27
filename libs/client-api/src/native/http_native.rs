use crate::http::log_request_id;
use crate::native::GetCollabAction;
use crate::ws::{ConnectInfo, WSClientConnectURLProvider, WSClientHttpSender, WSError};
use crate::{blocking_brotli_compress, brotli_compress, Client};
use crate::{RefreshTokenAction, RefreshTokenRetryCondition};
use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;
use rayon::iter::ParallelIterator;
use std::fs::metadata;

use bytes::Bytes;
use client_api_entity::{
  CollabParams, CreateImportTask, CreateImportTaskResponse, PublishCollabItem, QueryCollabParams,
};
use client_api_entity::{
  CompleteUploadRequest, CreateUploadRequest, CreateUploadResponse, UploadPartResponse,
};
use collab_rt_entity::HttpRealtimeMessage;
use futures::Stream;
use futures_util::stream;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use prost::Message;
use reqwest::{multipart, Body, Method};
use serde::Serialize;
use shared_entity::dto::workspace_dto::CollabResponse;
use shared_entity::response::{AppResponse, AppResponseError};
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::Ordering;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
pub use infra::file_util::ChunkedBytes;
use rayon::prelude::IntoParallelIterator;
use shared_entity::dto::ai_dto::CompleteTextParams;
use shared_entity::dto::import_dto::UserImportTask;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_retry::strategy::{ExponentialBackoff, FixedInterval};
use tokio_retry::{Condition, RetryIf};
use tokio_util::codec::{BytesCodec, FramedRead};

use tracing::{debug, error, event, info, instrument, trace};

impl Client {
  pub async fn stream_completion_text(
    &self,
    workspace_id: &str,
    params: CompleteTextParams,
  ) -> Result<impl Stream<Item = Result<Bytes, AppResponseError>>, AppResponseError> {
    let url = format!("{}/api/ai/{}/complete/stream", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::answer_response_stream(resp).await
  }

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

    // Encode the parent directory to ensure it's URL-safe.
    let parent_dir = utf8_percent_encode(parent_dir, NON_ALPHANUMERIC).to_string();
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
    RetryIf::spawn(retry_strategy, action, RetryGetCollabCondition).await
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn post_realtime_msg(
    &self,
    device_id: &str,
    msg: client_websocket::Message,
  ) -> Result<(), AppResponseError> {
    let device_id = device_id.to_string();
    let payload =
      blocking_brotli_compress(msg.into_data(), 6, self.config.compression_buffer_size).await?;

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

    let compression_tasks = params_list
      .into_par_iter()
      .filter_map(|params| {
        let data = params.to_bytes().ok()?;
        brotli_compress(
          data,
          self.config.compression_quality,
          self.config.compression_buffer_size,
        )
        .ok()
      })
      .collect::<Vec<_>>();

    let mut framed_data = Vec::new();
    let mut size_count = 0;
    for compressed in compression_tasks {
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
    } else {
      debug!("refresh token is already in progress");
    }

    // Wait for the result of the refresh token request.
    match tokio::time::timeout(Duration::from_secs(60), rx).await {
      Ok(Ok(result)) => result,
      Ok(Err(err)) => Err(AppError::Internal(anyhow!("refresh token error: {}", err)).into()),
      Err(_) => {
        self.is_refreshing_token.store(false, Ordering::SeqCst);
        Err(AppError::RequestTimeout("refresh token timeout".to_string()).into())
      },
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

  pub async fn publish_collabs<Metadata, Data>(
    &self,
    workspace_id: &str,
    items: Vec<PublishCollabItem<Metadata, Data>>,
  ) -> Result<(), AppResponseError>
  where
    Metadata: serde::Serialize + Send + 'static + Unpin,
    Data: AsRef<[u8]> + Send + 'static + Unpin,
  {
    let publish_collab_stream = PublishCollabItemStream::new(items);
    let url = format!("{}/api/workspace/{}/publish", self.base_url, workspace_id,);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .body(Body::wrap_stream(publish_collab_stream))
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  /// Sends a POST request to import a file to the server.
  ///
  /// This function streams the contents of a file located at the provided `file_path`
  /// as part of a multipart form data request to the server's `/api/import` endpoint.
  ///
  /// ### HTTP Request Details:
  ///
  /// - **Method:** POST
  /// - **URL:** `{base_url}/api/import`
  ///   - The `base_url` is dynamically provided and appended with `/api/import`.
  ///
  /// - **Headers:**
  ///   - `X-Host`: The value of the `base_url` is sent as the host header.
  ///   - `X-Content-Length`: The size of the file, in bytes, is provided from the file's metadata.
  ///
  /// - **Multipart Form:**
  ///   - The file is sent as a multipart form part:
  ///     - **Field Name:** The file name derived from the file path or a UUID if unavailable.
  ///     - **File Content:** The file's content is streamed using `reqwest::Body::wrap_stream`.
  ///     - **MIME Type:** Guessed from the file's extension using the `mime_guess` crate,
  ///       defaulting to `application/octet-stream` if undetermined.
  ///
  /// ### Parameters:
  /// - `file_path`: The path to the file to be uploaded.
  ///   - The file is opened asynchronously and its metadata (like size) is extracted.
  /// - The MIME type is automatically determined based on the file extension using `mime_guess`.
  ///
  pub async fn import_file(&self, file_path: &Path) -> Result<(), AppResponseError> {
    let md5_base64 = calculate_md5(file_path).await?;
    let file = File::open(&file_path).await?;
    let metadata = file.metadata().await?;
    let file_name = file_path
      .file_stem()
      .map(|s| s.to_string_lossy().to_string())
      .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let stream = FramedRead::new(file, BytesCodec::new());
    let mime = mime_guess::from_path(file_path)
      .first_or_octet_stream()
      .to_string();

    let file_part = multipart::Part::stream(reqwest::Body::wrap_stream(stream))
      .file_name(file_name.clone())
      .mime_str(&mime)?;

    let form = multipart::Form::new().part(file_name, file_part);
    let url = format!("{}/api/import", self.base_url);
    let mut builder = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .multipart(form);

    // set the host header
    builder = builder
      .header("X-Host", self.base_url.clone())
      .header("X-Content-MD5", md5_base64)
      .header("X-Content-Length", metadata.len());
    let resp = builder.send().await?;

    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  /// Creates an import task for a file and returns the import task response.
  ///
  /// This function initiates an import task by sending a POST request to the
  /// `/api/import/create` endpoint. The request includes the `workspace_name` derived
  /// from the provided file's name (or a generated UUID if the file name cannot be determined).
  ///
  /// After creating the import task, you should use [Self::upload_import_file] to upload
  /// the actual file to the presigned URL obtained from the [CreateImportTaskResponse].
  ///
  pub async fn create_import(
    &self,
    file_path: &Path,
  ) -> Result<CreateImportTaskResponse, AppResponseError> {
    let url = format!("{}/api/import/create", self.base_url);
    let file_name = file_path
      .file_stem()
      .map(|s| s.to_string_lossy().to_string())
      .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let params = CreateImportTask {
      workspace_name: file_name.clone(),
    };
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .header("X-Host", self.base_url.clone())
      .json(&params)
      .send()
      .await?;

    log_request_id(&resp);
    AppResponse::<CreateImportTaskResponse>::from_response(resp)
      .await?
      .into_data()
  }

  /// Uploads a file to a specified presigned URL obtained from the import task response.
  ///
  /// This function uploads a file to the given presigned URL using an HTTP PUT request.
  /// The file's metadata is read to determine its size, and the upload stream is created
  /// and sent to the provided URL. It is recommended to call this function after successfully
  /// creating an import task using [Self::create_import].
  ///
  pub async fn upload_import_file(
    &self,
    file_path: &Path,
    url: &str,
  ) -> Result<(), AppResponseError> {
    let file_metadata = metadata(file_path)?;
    let file_size = file_metadata.len();
    // Open the file
    let file = File::open(file_path).await?;
    let file_stream = FramedRead::new(file, BytesCodec::new());
    let stream_body = Body::wrap_stream(file_stream);
    trace!("start upload file to s3: {}", url);

    let client = reqwest::Client::new();
    let upload_resp = client
      .put(url)
      .header("Content-Type", "application/octet-stream")
      .header("Content-Length", file_size)
      .body(stream_body)
      .send()
      .await?;

    if !upload_resp.status().is_success() {
      error!("File upload failed: {:?}", upload_resp);
      return Err(AppError::S3ResponseError("Cannot upload file to S3".to_string()).into());
    }

    Ok(())
  }

  pub async fn get_import_list(&self) -> Result<UserImportTask, AppResponseError> {
    let url = format!("{}/api/import", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;

    AppResponse::<UserImportTask>::from_response(resp)
      .await?
      .into_data()
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

pub struct PublishCollabItemStream<Metadata, Data> {
  items: Vec<PublishCollabItem<Metadata, Data>>,
  idx: usize,
  done: bool,
}

impl<Metadata, Data> PublishCollabItemStream<Metadata, Data> {
  pub fn new(publish_collab_items: Vec<PublishCollabItem<Metadata, Data>>) -> Self {
    PublishCollabItemStream {
      items: publish_collab_items,
      idx: 0,
      done: false,
    }
  }
}

impl<Metadata, Data> Stream for PublishCollabItemStream<Metadata, Data>
where
  Metadata: Serialize + Send + 'static + Unpin,
  Data: AsRef<[u8]> + Send + 'static + Unpin,
{
  type Item = Result<Bytes, std::io::Error>;

  fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let mut self_mut = self.as_mut();

    if self_mut.idx >= self_mut.items.len() {
      if !self_mut.done {
        self_mut.done = true;
        return Poll::Ready(Some(Ok((0_u32).to_le_bytes().to_vec().into())));
      }
      return Poll::Ready(None);
    }

    let item = &self_mut.items[self_mut.idx];
    match serialize_metadata_data(&item.meta, item.data.as_ref()) {
      Err(e) => Poll::Ready(Some(Err(e))),
      Ok(chunk) => {
        self_mut.idx += 1;
        Poll::Ready(Some(Ok::<bytes::Bytes, std::io::Error>(chunk)))
      },
    }
  }
}

fn serialize_metadata_data<Metadata>(m: Metadata, d: &[u8]) -> Result<Bytes, std::io::Error>
where
  Metadata: Serialize,
{
  let meta = serde_json::to_vec(&m)?;

  let mut chunk = Vec::with_capacity(8 + meta.len() + d.len());
  chunk.extend_from_slice(&(meta.len() as u32).to_le_bytes()); // Encode metadata length
  chunk.extend_from_slice(&meta);
  chunk.extend_from_slice(&(d.len() as u32).to_le_bytes()); // Encode data length
  chunk.extend_from_slice(d);

  Ok(Bytes::from(chunk))
}

struct RetryGetCollabCondition;
impl Condition<AppResponseError> for RetryGetCollabCondition {
  fn should_retry(&mut self, error: &AppResponseError) -> bool {
    !error.is_record_not_found()
  }
}

/// Calculates the MD5 hash of a file and returns the base64-encoded MD5 digest.
///
/// # Arguments
/// * `file_path` - The path of the file for which the MD5 hash is to be calculated.
///
/// # Returns
/// A `Result` containing the base64-encoded MD5 hash on success, or an error if the file cannot be read.

/// Asynchronously calculates the MD5 hash of a file using efficient buffer handling and returns it as a base64-encoded string.
///
/// # Arguments
/// * `file_path` - The path to the file to be hashed.
///
/// # Returns
/// Returns a `Result` containing the base64-encoded MD5 hash on success, or an error if the file cannot be read.
pub async fn calculate_md5(file_path: &Path) -> Result<String, anyhow::Error> {
  let file = File::open(file_path).await?;
  let mut reader = BufReader::with_capacity(1_000_000, file);
  let mut context = md5::Context::new();
  loop {
    let part = reader.fill_buf().await?;
    if part.is_empty() {
      break;
    }

    context.consume(part);
    let part_len = part.len();
    reader.consume(part_len);
  }

  let md5_hash = context.compute();
  let md5_base64 = STANDARD.encode(md5_hash.as_ref());
  Ok(md5_base64)
}
