use crate::v2::actor::WorkspaceControllerActor;
use crate::v2::controller::{ConnectionStatus, DisconnectedReason};
use arc_swap::ArcSwap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, trace};

#[derive(Debug, Clone)]
pub(crate) struct ReconnectionManager {
  initial_delay: Duration,
  max_delay: Duration,
  max_attempts: u32,
  in_progress: Arc<AtomicBool>,
  weak_actor: Weak<WorkspaceControllerActor>,
  access_token: Arc<ArcSwap<String>>,
}

impl ReconnectionManager {
  pub fn new(actor: Weak<WorkspaceControllerActor>) -> Self {
    Self {
      initial_delay: Duration::from_secs(1),
      max_delay: Duration::from_secs(60),
      max_attempts: 10,
      in_progress: Arc::new(AtomicBool::new(false)),
      weak_actor: actor,
      access_token: Arc::new(Default::default()),
    }
  }

  pub fn set_access_token(&self, access_token: String) {
    self.access_token.store(Arc::new(access_token));
  }

  /// Trigger a reconnection attempt if one is not already in progress
  pub fn trigger_reconnect(&self, reason: &DisconnectedReason) {
    info!("trigger_reconnect {:?}", reason);
    let access_token = self.access_token.load().to_string();
    if access_token.is_empty() {
      tracing::warn!("Access token is empty when trigger reconnect");
      return;
    }

    // Check if reconnection is already in progress
    if self.in_progress.load(Ordering::SeqCst) {
      trace!("Reconnection already in progress, skipping new attempt");
      return;
    }

    // Only proceed if we have an access token
    if access_token.is_empty() {
      error!("Cannot reconnect: no access token available");
      return;
    }

    // Set the flag to indicate reconnection is in progress
    if self
      .in_progress
      .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
      .is_err()
    {
      trace!("Another thread started reconnection, skipping");
      return;
    }

    let weak_actor = self.weak_actor.clone();
    let manager = self.clone();
    tokio::spawn(async move {
      if let Some(actor) = weak_actor.upgrade() {
        manager
          .retry_connect_with_exponential_backoff(actor, access_token)
          .await;
      } else {
        manager.in_progress.store(false, Ordering::SeqCst);
      }
    });
  }

  /// Reset the in-progress flag
  pub fn reset_in_progress_flag(&self) {
    self.in_progress.store(false, Ordering::SeqCst);
    trace!("Reconnection process complete, flag reset");
  }

  /// Attempt to reconnect with exponential backoff
  async fn retry_connect_with_exponential_backoff(
    self,
    actor: Arc<WorkspaceControllerActor>,
    access_token: String,
  ) {
    // Use a guard pattern to ensure cleanup on all code paths
    struct ReconnectionGuard<'a> {
      manager: &'a ReconnectionManager,
    }

    #[allow(clippy::needless_lifetimes)]
    impl<'a> Drop for ReconnectionGuard<'a> {
      fn drop(&mut self) {
        self.manager.reset_in_progress_flag();
      }
    }

    // Create the guard to ensure we reset the flag when this function returns
    let _guard = ReconnectionGuard { manager: &self };
    let mut retry_delay = self.initial_delay;
    let mut attempt = 0;

    while attempt < self.max_attempts {
      trace!(
        "Reconnection attempt {} after {:?} delay",
        attempt + 1,
        retry_delay
      );

      sleep(retry_delay).await;
      // Check if we're already connected or connecting
      match &*actor.status_channel().borrow() {
        ConnectionStatus::Connected { .. } | ConnectionStatus::Connecting { .. } => {
          trace!("Already connected or connecting, stopping retry attempts");
          return;
        },
        ConnectionStatus::Disconnected {
          reason: Some(reason),
        } => {
          if !reason.retriable() {
            return;
          }
        },
        _ => {},
      }

      // Try to connect
      match WorkspaceControllerActor::handle_connect(&actor, access_token.clone()).await {
        Ok(_) => {
          trace!("Reconnection successful on attempt {}", attempt + 1);
          return;
        },
        Err(err) => {
          error!("Reconnection attempt {} failed: {}", attempt + 1, err);
          // We'll update the status, but with retry still set to true
          actor.set_connection_status(ConnectionStatus::Disconnected {
            reason: Some(DisconnectedReason::Other(err.to_string().into())),
          });
        },
      }

      // Exponential backoff: double the delay for next attempt, but cap at max_delay
      retry_delay = std::cmp::min(retry_delay * 2, self.max_delay);
      attempt += 1;
    }

    // If we've reached the maximum number of attempts, stop retrying
    if attempt >= self.max_attempts {
      error!(
        "Maximum reconnection attempts ({}) reached, giving up",
        self.max_attempts
      );
      actor.set_connection_status(ConnectionStatus::Disconnected {
        reason: Some(DisconnectedReason::ReachMaximumRetry),
      });
    }
  }
}
