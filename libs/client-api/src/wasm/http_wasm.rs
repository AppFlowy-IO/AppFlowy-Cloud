use crate::http::log_request_id;
use crate::ws::{WSClientHttpSender, WSError};
use crate::Client;
use app_error::gotrue::GoTrueError;
use app_error::ErrorCode;
use async_trait::async_trait;
use collab_rt_entity::EncodedCollab;
use database_entity::dto::{CollabParams, QueryCollabParams};
use gotrue::grant::{Grant, RefreshTokenGrant};
use reqwest::Method;
use shared_entity::response::{AppResponse, AppResponseError};
use std::future::Future;
use std::sync::atomic::Ordering;
use tracing::instrument;

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
  ) -> Result<EncodedCollab, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}",
      self.base_url, &params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    let resp = AppResponse::<EncodedCollab>::from_response(resp).await?;
    resp.into_data()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn refresh_token(&self) -> Result<(), AppResponseError> {
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

    rx.await
      .map_err(|err| AppResponseError::new(ErrorCode::Internal, err.to_string()))??;
    Ok(())
  }

  async fn inner_refresh_token(&self) -> Result<(), AppResponseError> {
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
