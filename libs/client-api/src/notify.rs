use gotrue_entity::AccessTokenResponse;
use std::ops::{Deref, DerefMut};
use tokio::sync::broadcast::{channel, Receiver, Sender};

pub type TokenStateReceiver = Receiver<TokenState>;

#[derive(Debug, Clone)]
pub enum TokenState {
  Refresh,
  Invalid,
}

pub struct ClientToken {
  sender: Sender<TokenState>,
  token: Option<AccessTokenResponse>,
}

impl ClientToken {
  pub(crate) fn new() -> Self {
    let (sender, _) = channel(100);
    Self {
      sender,
      token: None,
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
  pub(crate) fn set(&mut self, token: AccessTokenResponse) {
    tracing::trace!("Set new access token: {:?}", token);
    self.token = Some(token);
    let _ = self.sender.send(TokenState::Refresh);
  }

  /// Unsets the current access token and notifies receivers of the invalidation.
  ///
  /// If there's an existing token, this function clears the internal access token state and sends
  /// a `TokenState::Invalid` notification to signal that the token has been invalidated.
  ///
  #[allow(dead_code)]
  pub(crate) fn unset(&mut self) {
    if self.token.is_some() {
      tracing::trace!("Set new access token: {:?}", self.token);
      self.token = None;
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
  type Target = Option<AccessTokenResponse>;

  fn deref(&self) -> &Self::Target {
    &self.token
  }
}

impl DerefMut for ClientToken {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.token
  }
}
