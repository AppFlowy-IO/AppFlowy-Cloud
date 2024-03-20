use anyhow::Error;
use gotrue_entity::dto::GotrueTokenResponse;
use std::ops::{Deref, DerefMut};
use tokio::sync::broadcast::{channel, Receiver, Sender};
use tracing::{event, warn};

pub type TokenStateReceiver = Receiver<TokenState>;

#[derive(Debug, Clone)]
pub enum TokenState {
  Refresh,
  Invalid,
}

pub struct ClientToken {
  sender: Sender<TokenState>,
  token: Option<GotrueTokenResponse>,
}

impl ClientToken {
  pub(crate) fn new() -> Self {
    let (sender, _) = channel(100);
    Self {
      sender,
      token: None,
    }
  }

  pub fn is_empty(&self) -> bool {
    self.token.is_none()
  }

  pub fn try_get(&self) -> Result<String, Error> {
    match &self.token {
      None => Err(anyhow::anyhow!("No access token")),
      Some(token) => Ok(serde_json::to_string(token)?),
    }
  }

  /// Sets a new access token and notifies interested parties of the refresh.
  ///
  /// This function updates the internal access token state and sends a `TokenState::Refresh`
  /// notification to signal that the token has been refreshed.
  ///
  /// # Parameters
  ///
  /// - `token`: The new `AccessTokenResponse` to be set.
  pub(crate) fn set(&mut self, new_token: GotrueTokenResponse) {
    match &self.token {
      None => {
        self.token = Some(new_token);
        let _ = self.sender.send(TokenState::Refresh);
      },
      Some(old_token) => {
        event!(
          tracing::Level::INFO,
          "old token:{}, new token:{}",
          old_token,
          new_token
        );

        if old_token.expires_at > new_token.expires_at {
          warn!(
            "new token expires_at:{} is less than old token expires_at:{}",
            new_token.expires_at, old_token.expires_at
          );
        } else {
          self.token = Some(new_token);
          tracing::trace!("Set new token");
          let _ = self.sender.send(TokenState::Refresh);
        }
      },
    };
  }

  /// Unsets the current access token and notifies receivers of the invalidation.
  ///
  /// If there's an existing token, this function clears the internal access token state and sends
  /// a `TokenState::Invalid` notification to signal that the token has been invalidated.
  ///
  #[allow(dead_code)]
  pub(crate) fn unset(&mut self) {
    if self.token.is_some() {
      self.token = None;
      event!(tracing::Level::DEBUG, "unset token");
      let _ = self.sender.send(TokenState::Invalid);
    }
  }

  /// Subscribe to token state change
  /// Receiver will receive `TokenState::Refresh` when the token is refreshed
  /// Receiver will receive `TokenState::Invalid` when the token is invalid
  pub(crate) fn subscribe(&self) -> Receiver<TokenState> {
    self.sender.subscribe()
  }
}

impl Deref for ClientToken {
  type Target = Option<GotrueTokenResponse>;

  fn deref(&self) -> &Self::Target {
    &self.token
  }
}

impl DerefMut for ClientToken {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.token
  }
}
