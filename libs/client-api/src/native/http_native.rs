use crate::http::log_request_id;
use crate::ws::{WSClientHttpSender, WSError};
use crate::{spawn_blocking_brotli_compress, Client};
use app_error::gotrue::GoTrueError;
use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::CollabParams;
use futures_util::stream;
use gotrue::grant::{Grant, RefreshTokenGrant};
use prost::Message;
use realtime_entity::realtime_proto::HttpRealtimeMessage;
use reqwest::{Body, Method};
use shared_entity::response::{AppResponse, AppResponseError};
use std::future::Future;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tracing::{event, instrument, warn};

impl Client {
  #[instrument(level = "debug", skip_all, err)]
  pub async fn post_realtime_msg(
    &self,
    device_id: &str,
    msg: websocket::Message,
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
        platform_spawn(async move {
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
  pub async fn refresh_token(&self) -> Result<crate::http::RefreshTokenRet, AppResponseError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    self.refresh_ret_txs.write().push(tx);

    if !self.is_refreshing_token.load(Ordering::SeqCst) {
      self.is_refreshing_token.store(true, Ordering::SeqCst);
      let txs = std::mem::take(&mut *self.refresh_ret_txs.write());
      let result = self.inner_refresh_token().await;
      for tx in txs {
        let _ = tx.send(result.clone());
      }
      self.is_refreshing_token.store(false, Ordering::SeqCst);
    }
    Ok(rx)
  }

  async fn inner_refresh_token(&self) -> Result<(), AppResponseError> {
    let mut current_retry_count = 0;
    let max_retry_count = 4;

    let refresh_token = self
      .token
      .read()
      .as_ref()
      .ok_or(GoTrueError::NotLoggedIn(
        "fail to refresh user token".to_owned(),
      ))?
      .refresh_token
      .as_str()
      .to_owned();
    let grant_req = Grant::RefreshToken(RefreshTokenGrant { refresh_token });

    while current_retry_count < max_retry_count {
      current_retry_count += 1;

      match self.gotrue_client.token(&grant_req).await {
        Ok(new_token) => {
          self.token.write().set(new_token);
          return Ok(());
        },
        Err(err) => {
          warn!("refresh token failed: {}", err);
          if err.is_network_error() {
            std::thread::sleep(Duration::from_secs(2));
            continue;
          }
          return Err(AppResponseError::from(AppError::from(err)));
        },
      };
    }

    Err(AppResponseError::from(AppError::from(
      GoTrueError::NotLoggedIn("fail to refresh user token, max retry count reached".to_owned()),
    )))
  }
}

#[async_trait]
impl WSClientHttpSender for Client {
  async fn send_ws_msg(&self, device_id: &str, message: websocket::Message) -> Result<(), WSError> {
    self
      .post_realtime_msg(device_id, message)
      .await
      .map_err(|err| WSError::Internal(anyhow::Error::from(err)))
  }
}

pub fn platform_spawn<T>(future: T) -> tokio::task::JoinHandle<T::Output>
where
  T: Future + Send + 'static,
  T::Output: Send + 'static,
{
  tokio::spawn(future)
}
