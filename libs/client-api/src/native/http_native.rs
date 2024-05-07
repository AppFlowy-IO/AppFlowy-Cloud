use crate::http::log_request_id;
use crate::native::GetCollabAction;
use crate::ws::{ConnectInfo, WSClientConnectURLProvider, WSClientHttpSender, WSError};
use crate::{spawn_blocking_brotli_compress, Client};
use crate::{RefreshTokenAction, RefreshTokenRetryCondition};
use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;
use collab_rt_entity::HttpRealtimeMessage;
use database_entity::dto::{CollabParams, QueryCollabParams};
use futures_util::stream;
use prost::Message;
use reqwest::{Body, Method};
use shared_entity::dto::workspace_dto::CollabResponse;
use shared_entity::response::{AppResponse, AppResponseError};
use std::future::Future;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio_retry::strategy::{ExponentialBackoff, FixedInterval};
use tokio_retry::{Retry, RetryIf};
use tracing::{event, info, instrument};

impl Client {
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
