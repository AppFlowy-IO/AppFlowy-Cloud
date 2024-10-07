use crate::http::log_request_id;
use crate::native::GetCollabAction;
use crate::ws::{ConnectInfo, WSClientConnectURLProvider, WSClientHttpSender, WSError};
use crate::{blocking_brotli_compress, brotli_compress, Client};
use crate::{RefreshTokenAction, RefreshTokenRetryCondition};
use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;
use rayon::iter::ParallelIterator;

use bytes::Bytes;
use client_api_entity::{CollabParams, PublishCollabItem, QueryCollabParams};
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

use rayon::prelude::IntoParallelIterator;
use reqwest::header::CONTENT_LENGTH;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::fs::File;
use tokio_retry::strategy::{ExponentialBackoff, FixedInterval};
use tokio_retry::{Condition, RetryIf};
use tokio_util::codec::{BytesCodec, FramedRead};
use tracing::{debug, event, info, instrument, trace};

pub use infra::file_util::ChunkedBytes;
use shared_entity::dto::ai_dto::CompleteTextParams;
use shared_entity::dto::import_dto::UserImportTask;

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

  pub async fn import_file(&self, file_path: &Path) -> Result<(), AppResponseError> {
    let file = File::open(&file_path).await?;
    let metadata = file.metadata().await?;
    let file_name = file_path
      .file_name()
      .map(|s| s.to_string_lossy().to_string())
      .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let stream = FramedRead::new(file, BytesCodec::new());
    let mime = mime_guess::from_path(file_path)
      .first_or_octet_stream()
      .to_string();

    let file_part = multipart::Part::stream(reqwest::Body::wrap_stream(stream))
      .file_name(file_name)
      .mime_str(&mime)?;

    let form = multipart::Form::new().part("file", file_part);
    let url = format!("{}/api/import", self.base_url);
    let mut builder = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .multipart(form);

    // set the host header
    builder = builder.header("X-Host", self.base_url.clone());
    builder = builder.header(CONTENT_LENGTH, metadata.len());
    let resp = builder.send().await?;

    AppResponse::<()>::from_response(resp).await?.into_error()
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
