use crate::notify::ClientToken;
use app_error::gotrue::GoTrueError;
use gotrue::grant::{Grant, RefreshTokenGrant};
use parking_lot::RwLock;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio_retry::{Action, Condition};

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
