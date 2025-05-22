use crate::v2::actor::WorkspaceControllerActor;
use crate::v2::controller::{ConnectionStatus, DisconnectedReason};
use crate::{sync_error, sync_info, sync_trace};
use arc_swap::ArcSwap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::time::sleep;

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
      initial_delay: Duration::from_secs(5),
      max_delay: Duration::from_secs(120),
      max_attempts: 5,
      in_progress: Arc::new(AtomicBool::new(false)),
      weak_actor: actor,
      access_token: Default::default(),
    }
  }

  pub fn set_access_token(&self, token: String) {
    self.access_token.store(Arc::new(token));
  }

  pub fn trigger_reconnect(&self, reason: &str) {
    sync_info!(?reason, "trigger_reconnect");

    let token = self.access_token.load().as_ref().clone();
    if token.is_empty() {
      sync_info!("no access token → abort reconnect");
      return;
    }

    if self.in_progress.swap(true, Ordering::SeqCst) {
      sync_trace!("reconnect already in progress");
      return;
    }

    let manager = self.clone();
    let weak_actor = self.weak_actor.clone();
    tokio::spawn(async move {
      if let Some(actor) = weak_actor.upgrade() {
        manager.retry_with_exponential_backoff(actor, token).await;
      }

      manager.in_progress.store(false, Ordering::SeqCst);
      sync_trace!("reconnect stop");
    });
  }

  async fn retry_with_exponential_backoff(
    &self,
    actor: Arc<WorkspaceControllerActor>,
    token: String,
  ) {
    let mut delay = self.initial_delay;
    for attempt in 1..=self.max_attempts {
      sync_trace!(attempt, ?delay, "waiting before reconnect");
      sleep(delay).await;

      // stop if we're already (re)connecting or non-retriable
      match &*actor.status_channel().borrow() {
        ConnectionStatus::Connected { .. } | ConnectionStatus::Connecting { .. } => {
          sync_trace!("already connected/connecting; stopping retries");
          return;
        },
        ConnectionStatus::Disconnected { reason: Some(r) } if !r.retriable() => {
          sync_trace!(?r, "non-retriable disconnect; aborting");
          return;
        },
        _ => {},
      }

      // attempt to connect
      match WorkspaceControllerActor::handle_connect(&actor, token.clone()).await {
        Ok(()) => {
          sync_info!(attempt, "reconnected successfully");
          return;
        },
        Err(err) => {
          sync_error!(attempt, %err, "reconnect attempt failed");
          let reason = DisconnectedReason::from(err);
          actor.set_connection_status(ConnectionStatus::Disconnected {
            reason: Some(reason),
          });
        },
      }

      // exponential backoff: double, capped at max_delay
      delay = std::cmp::min(delay * 2, self.max_delay);
    }

    // give up after max_attempts
    sync_error!(
      max_attempts = self.max_attempts,
      "max reconnect attempts reached; giving up"
    );
    actor.set_connection_status(ConnectionStatus::Disconnected {
      reason: Some(DisconnectedReason::ReachMaximumRetry),
    });
  }
}
