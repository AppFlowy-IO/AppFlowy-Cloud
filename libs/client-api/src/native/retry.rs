use crate::notify::ClientToken;
use crate::ws::{ConnectState, ConnectStateNotify, CurrentAddr, StateNotify, WSError};
use app_error::gotrue::GoTrueError;
use gotrue::grant::{Grant, RefreshTokenGrant};
use parking_lot::RwLock;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio_retry::strategy::FixedInterval;
use tokio_retry::{Action, Condition, RetryIf};
use tracing::{debug, info};
use websocket::{connect_async, WebSocketStream};

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
  addr: &str,
  state_notify: Weak<StateNotify>,
  current_addr: Weak<CurrentAddr>,
) -> Result<WebSocketStream, WSError> {
  let connecting_addr = addr.to_owned();
  let stream = RetryIf::spawn(
    FixedInterval::new(Duration::from_secs(10)),
    ConnectAction::new(connecting_addr.clone()),
    RetryCondition {
      connecting_addr,
      current_addr,
      state_notify,
    },
  )
  .await?;
  Ok(stream)
}

struct ConnectAction {
  addr: String,
}

impl ConnectAction {
  fn new(addr: String) -> Self {
    Self { addr }
  }
}

impl Action for ConnectAction {
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + Send + Sync>>;
  type Item = WebSocketStream;
  type Error = WSError;

  fn run(&mut self) -> Self::Future {
    let cloned_addr = self.addr.clone();
    Box::pin(async move {
      info!("🔵websocket start connecting");
      match connect_async(&cloned_addr).await {
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
  connecting_addr: String,
  current_addr: Weak<parking_lot::Mutex<Option<String>>>,
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

    let should_retry = self
      .current_addr
      .upgrade()
      .map(|addr| match addr.try_lock() {
        None => false,
        Some(addr) => match &*addr {
          None => false,
          Some(addr) => addr == &self.connecting_addr,
        },
      })
      .unwrap_or(false);

    debug!("WSClient should_retry: {}", should_retry);
    should_retry
  }
}
