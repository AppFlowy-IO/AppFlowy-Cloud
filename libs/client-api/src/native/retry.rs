use crate::http::log_request_id;
use crate::notify::ClientToken;
use crate::ws::{
  ConnectState, ConnectStateNotify, StateNotify, WSClientConnectURLProvider, WSError,
};
use crate::Client;
use app_error::gotrue::GoTrueError;
use client_websocket::{connect_async, WebSocketStream};
use database_entity::dto::QueryCollabParams;
use gotrue::grant::{Grant, RefreshTokenGrant};
use parking_lot::RwLock;
use reqwest::header::HeaderMap;
use reqwest::Method;
use shared_entity::dto::workspace_dto::{CollabResponse, CollabTypeParam};
use shared_entity::response::{AppResponse, AppResponseError};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio_retry::strategy::FixedInterval;
use tokio_retry::{Action, Condition, RetryIf};
use tracing::{debug, info, trace};

pub(crate) struct RefreshTokenAction {
  token: Arc<RwLock<ClientToken>>,
  gotrue_client: Arc<gotrue::api::Client>,
}

impl RefreshTokenAction {
  pub fn new(token: Arc<RwLock<ClientToken>>, gotrue_client: gotrue::api::Client) -> Self {
    Self {
      token,
      gotrue_client: Arc::new(gotrue_client),
    }
  }
}

impl Action for RefreshTokenAction {
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + Send + Sync>>;
  type Item = ();
  type Error = GoTrueError;

  fn run(&mut self) -> Self::Future {
    let weak_token = Arc::downgrade(&self.token);
    let weak_gotrue_client = Arc::downgrade(&self.gotrue_client);
    Box::pin(async move {
      if let (Some(token), Some(gotrue_client)) =
        (weak_token.upgrade(), weak_gotrue_client.upgrade())
      {
        let refresh_token = token
          .read()
          .as_ref()
          .ok_or(GoTrueError::NotLoggedIn(
            "fail to refresh user token".to_owned(),
          ))?
          .refresh_token
          .as_str()
          .to_owned();
        let access_token_resp = gotrue_client
          .token(&Grant::RefreshToken(RefreshTokenGrant { refresh_token }))
          .await?;
        token.write().set(access_token_resp);
      }
      Ok(())
    })
  }
}

pub(crate) struct RefreshTokenRetryCondition;
impl Condition<GoTrueError> for RefreshTokenRetryCondition {
  fn should_retry(&mut self, error: &GoTrueError) -> bool {
    error.is_network_error()
  }
}

pub async fn retry_connect(
  connect_provider: Arc<dyn WSClientConnectURLProvider>,
  state_notify: Weak<StateNotify>,
) -> Result<WebSocketStream, WSError> {
  let stream = RetryIf::spawn(
    FixedInterval::new(Duration::from_secs(15)),
    ConnectAction::new(connect_provider),
    RetryCondition { state_notify },
  )
  .await?;
  Ok(stream)
}

struct ConnectAction {
  connect_provider: Arc<dyn WSClientConnectURLProvider>,
}

impl ConnectAction {
  fn new(connect_provider: Arc<dyn WSClientConnectURLProvider>) -> Self {
    Self { connect_provider }
  }
}

impl Action for ConnectAction {
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + Send>>;
  type Item = WebSocketStream;
  type Error = WSError;

  fn run(&mut self) -> Self::Future {
    let connect_provider = self.connect_provider.clone();
    Box::pin(async move {
      info!("🔵websocket start connecting");
      let url = connect_provider.connect_ws_url();
      let headers: HeaderMap = connect_provider.connect_info().await?.into();
      trace!("websocket url:{}, headers: {:?}", url, headers);
      match connect_async(&url, headers).await {
        Ok(stream) => {
          info!("🟢websocket connect success");
          Ok(stream)
        },
        Err(e) => Err(e.into()),
      }
    })
  }
}

struct RetryCondition {
  state_notify: Weak<parking_lot::Mutex<ConnectStateNotify>>,
}
impl Condition<WSError> for RetryCondition {
  fn should_retry(&mut self, error: &WSError) -> bool {
    if let WSError::AuthError(err) = error {
      debug!("{}, stop retry connect", err);
      if let Some(state_notify) = self.state_notify.upgrade() {
        state_notify.lock().set_state(ConnectState::Unauthorized);
      }
      return false;
    }

    true
  }
}

pub(crate) struct GetCollabAction {
  client: Client,
  params: QueryCollabParams,
}

impl GetCollabAction {
  pub fn new(client: Client, params: QueryCollabParams) -> Self {
    Self { client, params }
  }
}

impl Action for GetCollabAction {
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + Send + Sync>>;
  type Item = CollabResponse;
  type Error = AppResponseError;

  fn run(&mut self) -> Self::Future {
    let client = self.client.clone();
    let params = self.params.clone();
    let collab_type = self.params.collab_type.clone();

    Box::pin(async move {
      let url = format!(
        "{}/api/workspace/v1/{}/collab/{}",
        client.base_url, &params.workspace_id, &params.object_id
      );
      let resp = client
        .http_client_with_auth(Method::GET, &url)
        .await?
        .query(&CollabTypeParam { collab_type })
        .send()
        .await?;
      log_request_id(&resp);
      let resp = AppResponse::<CollabResponse>::from_response(resp).await?;
      resp.into_data()
    })
  }
}
