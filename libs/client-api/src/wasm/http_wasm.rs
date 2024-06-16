use crate::http::log_request_id;
use crate::ws::{ConnectInfo, WSClientConnectURLProvider, WSClientHttpSender, WSError};
use crate::Client;
use app_error::gotrue::GoTrueError;
use app_error::ErrorCode;
use async_trait::async_trait;
use database_entity::dto::{CollabParams, QueryCollabParams};
use gotrue::grant::{Grant, RefreshTokenGrant};
use reqwest::Method;
use shared_entity::dto::workspace_dto::{CollabResponse, CollabTypeParam};
use shared_entity::response::{AppResponse, AppResponseError};
use std::future::Future;
use std::sync::atomic::Ordering;
use tracing::{info, instrument};

impl Client {
  pub async fn create_collab_list(
    &self,
    workspace_id: &str,
    _params_list: Vec<CollabParams>,
  ) -> Result<(), AppResponseError> {
    let _url = self.batch_create_collab_url(workspace_id);
    Err(AppResponseError::new(
      ErrorCode::Unhandled,
      "not implemented",
    ))
  }

  #[instrument(level = "debug", skip_all)]
  pub async fn get_collab(
    &self,
    params: QueryCollabParams,
  ) -> Result<CollabResponse, AppResponseError> {
    let url = format!(
      "{}/api/workspace/v1/{}/collab/{}",
      self.base_url, &params.workspace_id, &params.object_id
    );
    let collab_type = params.collab_type.clone();
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .query(&CollabTypeParam { collab_type })
      .send()
      .await?;
    log_request_id(&resp);
    let resp = AppResponse::<CollabResponse>::from_response(resp).await?;
    resp.into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn refresh_token(&self, reason: &str) -> Result<(), AppResponseError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    self.refresh_ret_txs.write().push(tx);

    if !self.is_refreshing_token.load(Ordering::SeqCst) {
      self.is_refreshing_token.store(true, Ordering::SeqCst);

      info!("refresh token reason:{}", reason);
      let txs = std::mem::take(&mut *self.refresh_ret_txs.write());
      let result = self.inner_refresh_token().await;
      for tx in txs {
        let _ = tx.send(result.clone());
      }
      self.is_refreshing_token.store(false, Ordering::SeqCst);
    }

    rx.await
      .map_err(|err| AppResponseError::new(ErrorCode::Internal, err.to_string()))??;
    Ok(())
  }

  async fn inner_refresh_token(&self) -> Result<(), AppResponseError> {
    // let policy = RetryPolicy::fixed(Duration::from_secs(2)).with_max_retries(4).with_jitter(false);
    // let refresh_token = self
    //   .token
    //   .read()
    //   .as_ref()
    //   .ok_or(GoTrueError::NotLoggedIn(
    //     "fail to refresh user token".to_owned(),
    //   ))?
    //   .refresh_token
    //   .as_str()
    //   .to_owned();
    // match policy.retry_if(move || {
    //   let grant = Grant::RefreshToken(RefreshTokenGrant { refresh_token: refresh_token.clone() });
    //   async move {
    //     self
    //       .gotrue_client
    //       .token(&grant).await
    //   }
    //
    // }, RefreshTokenRetryCondition).await {
    //   Ok(new_token) => {
    //     event!(tracing::Level::INFO, "refresh token success");
    //     self.token.write().set(new_token);
    //     Ok(())
    //   },
    //   Err(err) => {
    //     let err = AppError::from(err);
    //     event!(tracing::Level::ERROR, "refresh token failed: {}", err);
    //
    //     // If the error is an OAuth error, unset the token.
    //     if err.is_unauthorized() {
    //       self.token.write().unset();
    //     }
    //     Err(err.into())
    //   },
    // }
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
    let new_token = self
      .gotrue_client
      .token(&Grant::RefreshToken(RefreshTokenGrant { refresh_token }))
      .await?;
    self.token.write().set(new_token);
    Ok(())
  }
}

pub fn af_spawn<T>(future: T) -> tokio::task::JoinHandle<T::Output>
where
  T: Future + 'static,
  T::Output: Send + 'static,
{
  tokio::task::spawn_local(future)
}

#[async_trait]
impl WSClientHttpSender for Client {
  async fn send_ws_msg(
    &self,
    _device_id: &str,
    _message: client_websocket::Message,
  ) -> Result<(), WSError> {
    Err(WSError::Internal(anyhow::Error::msg("not supported")))
  }
}

#[async_trait]
impl WSClientConnectURLProvider for Client {
  fn connect_ws_url(&self) -> String {
    self.ws_addr.clone()
  }

  async fn connect_info(&self) -> Result<ConnectInfo, WSError> {
    Err(WSError::Internal(anyhow::Error::msg("not supported")))
  }
}
