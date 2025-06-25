use crate::v2::actor::WorkspaceControllerActor;
use crate::v2::controller::{ConnectionStatus, DisconnectedReason};
use crate::{sync_error, sync_info, sync_trace};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use shared_entity::response::AppResponseError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::time::sleep;

#[async_trait]
pub trait ReconnectTarget: Send + Sync {
  fn status_channel(&self) -> &tokio::sync::watch::Receiver<ConnectionStatus>;

  async fn attempt_connect(self: Arc<Self>, token: String) -> Result<(), AppResponseError>;

  fn set_disconnected(&self, reason: DisconnectedReason);
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
  /// Initial delay before first reconnect attempt.
  pub initial_delay: Duration,
  /// Maximum delay between reconnect attempts.
  pub max_delay: Duration,
  /// Maximum number of reconnect attempts before giving up.
  pub max_attempts: u32,
}

impl Default for RetryConfig {
  fn default() -> Self {
    Self {
      initial_delay: Duration::from_secs(5),
      max_delay: Duration::from_secs(120),
      max_attempts: 5,
    }
  }
}

#[derive(Clone)]
pub(crate) struct ReconnectionManager {
  config: RetryConfig,
  in_progress: Arc<AtomicBool>,
  target: Weak<dyn ReconnectTarget + Send + Sync>,
  access_token: Arc<ArcSwap<String>>,
}

impl ReconnectionManager {
  /// Creates a new manager for the given target with custom config.
  pub fn with_config_for_target(
    target: Arc<dyn ReconnectTarget + Send + Sync>,
    config: RetryConfig,
  ) -> Self {
    Self {
      config,
      in_progress: Arc::new(AtomicBool::new(false)),
      target: Arc::downgrade(&target),
      access_token: Default::default(),
    }
  }

  /// Construct using the default retry config.
  pub fn new_for_target(target: Arc<dyn ReconnectTarget + Send + Sync>) -> Self {
    Self::with_config_for_target(target, RetryConfig::default())
  }

  /// Convenience ctor for production: pass the concrete actor.
  pub fn new(actor: Arc<WorkspaceControllerActor>) -> Self {
    // Coerce to trait object automatically.
    let dyn_target: Arc<dyn ReconnectTarget + Send + Sync> = actor.clone();
    Self::new_for_target(dyn_target)
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
      sync_trace!("reconnect already in progress, skip retry");
      return;
    }

    let manager = self.clone();
    let weak_target = self.target.clone();
    tokio::spawn(async move {
      if let Some(target) = weak_target.upgrade() {
        if manager.retry_with_exponential_backoff(target, token).await {
          sync_info!("reconnect succeeded");
        } else {
          sync_error!("reconnect failed");
        }
      }
      manager.in_progress.store(false, Ordering::SeqCst);
    });
  }

  async fn retry_with_exponential_backoff(
    &self,
    target: Arc<dyn ReconnectTarget + Send + Sync>,
    token: String,
  ) -> bool {
    let mut delay = self.config.initial_delay;
    for attempt in 1..=self.config.max_attempts {
      sync_trace!(attempt, ?delay, "waiting before reconnect");
      sleep(delay).await;

      // Stop if we're already (re)connecting or non-retriable
      match &*target.status_channel().borrow() {
        ConnectionStatus::Connected { .. } | ConnectionStatus::Connecting { .. } => {
          sync_trace!("already connected/connecting; stopping retries");
          return false;
        },
        ConnectionStatus::Disconnected { reason: Some(r) } if !r.retriable() => {
          sync_trace!(?r, "non-retriable disconnect; aborting");
          return false;
        },
        _ => {},
      }

      // Attempt to connect
      match target.clone().attempt_connect(token.clone()).await {
        Ok(()) => {
          sync_info!(attempt, "reconnect successfully");
          return true;
        },
        Err(err) => {
          sync_error!(attempt, %err, "reconnect attempt failed");
          let reason = DisconnectedReason::from(err);
          target.set_disconnected(reason);
        },
      }

      // Exponential backoff: double, capped at max_delay
      delay = std::cmp::min(delay * 2, self.config.max_delay);
    }

    // give up after max_attempts
    sync_error!(
      max_attempts = self.config.max_attempts,
      "max reconnect attempts reached; giving up"
    );
    target.set_disconnected(DisconnectedReason::ReachMaximumRetry);
    false
  }
}

#[cfg(test)]
mod tests {
  use app_error::ErrorCode;
  use async_trait::async_trait;
  use shared_entity::response::AppResponseError;
  use std::{
    sync::{
      atomic::{AtomicUsize, Ordering},
      Arc,
    },
    time::Duration,
  };
  use tokio::sync::watch;
  use tokio_util::sync::CancellationToken;

  use super::{ReconnectTarget, ReconnectionManager, RetryConfig};
  use crate::v2::controller::{spawn_reconnection, ConnectionStatus, DisconnectedReason};

  // A more robust fake ReconnectTarget for driving tests
  #[derive(Clone)]
  struct FakeTarget {
    status_tx: watch::Sender<ConnectionStatus>,
    status_rx: watch::Receiver<ConnectionStatus>,
    call_count: Arc<AtomicUsize>,
    responses: Arc<tokio::sync::Mutex<Vec<Result<(), AppResponseError>>>>,
    last_disconnect_reason: Arc<tokio::sync::Mutex<Option<DisconnectedReason>>>,
  }

  impl FakeTarget {
    fn new(
      initial_status: ConnectionStatus,
      responses: Vec<Result<(), AppResponseError>>,
    ) -> (Self, Arc<AtomicUsize>) {
      let (tx, rx) = watch::channel(initial_status);
      let call_count = Arc::new(AtomicUsize::new(0));
      (
        FakeTarget {
          status_tx: tx,
          status_rx: rx,
          call_count: call_count.clone(),
          responses: Arc::new(tokio::sync::Mutex::new(responses)),
          last_disconnect_reason: Arc::new(tokio::sync::Mutex::new(None)),
        },
        call_count,
      )
    }

    async fn get_last_disconnect_reason(&self) -> Option<DisconnectedReason> {
      self.last_disconnect_reason.lock().await.clone()
    }

    async fn set_status(&self, status: ConnectionStatus) {
      let _ = self.status_tx.send(status);
    }
  }

  #[async_trait]
  impl ReconnectTarget for FakeTarget {
    fn status_channel(&self) -> &watch::Receiver<ConnectionStatus> {
      &self.status_rx
    }

    async fn attempt_connect(self: Arc<Self>, _token: String) -> Result<(), AppResponseError> {
      let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
      let responses_guard = self.responses.lock().await;
      responses_guard.get(idx).cloned().unwrap_or_else(|| {
        Err(AppResponseError {
          code: ErrorCode::Internal,
          message: "no more responses configured".into(),
        })
      })
    }

    fn set_disconnected(&self, reason: DisconnectedReason) {
      // Store the reason for test verification
      if let Ok(mut guard) = self.last_disconnect_reason.try_lock() {
        *guard = Some(reason.clone());
      }
      let _ = self.status_tx.send(ConnectionStatus::Disconnected {
        reason: Some(reason),
      });
    }
  }

  #[tokio::test]
  async fn test_reconnect_succeeds_immediately() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
        max_attempts: 3,
      },
    );

    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;

    assert!(result, "Reconnection should succeed immediately");
    assert_eq!(
      call_count.load(Ordering::SeqCst),
      1,
      "Should attempt connection exactly once"
    );
  }

  #[tokio::test]
  async fn test_reconnect_succeeds_after_failures() {
    let responses = vec![
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "first failure".into(),
      }),
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "second failure".into(),
      }),
      Ok(()),
    ];
    let (fake_target, call_count) =
      FakeTarget::new(ConnectionStatus::Disconnected { reason: None }, responses);
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(2),
        max_attempts: 5,
      },
    );

    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;

    assert!(result, "Reconnection should succeed on third attempt");
    assert_eq!(
      call_count.load(Ordering::SeqCst),
      3,
      "Should attempt connection three times"
    );
  }

  #[tokio::test]
  async fn test_reconnect_fails_after_max_attempts() {
    let responses = vec![
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "failure 1".into(),
      }),
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "failure 2".into(),
      }),
    ];
    let (fake_target, call_count) =
      FakeTarget::new(ConnectionStatus::Disconnected { reason: None }, responses);
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
        max_attempts: 2,
      },
    );

    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;

    assert!(
      !result,
      "Reconnection should fail after exhausting attempts"
    );
    assert_eq!(
      call_count.load(Ordering::SeqCst),
      2,
      "Should attempt connection twice"
    );

    // Verify that the target was set to disconnected with the correct reason
    let disconnect_reason = fake_target.get_last_disconnect_reason().await;
    assert!(matches!(
      disconnect_reason,
      Some(DisconnectedReason::ReachMaximumRetry)
    ));
  }

  #[tokio::test]
  async fn test_aborts_when_already_connecting() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Connecting {
        cancel: CancellationToken::new(),
      },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
        max_attempts: 3,
      },
    );

    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;

    assert!(!result, "Should abort when already connecting");
    assert_eq!(
      call_count.load(Ordering::SeqCst),
      0,
      "Should not attempt connection when already connecting"
    );
  }

  #[tokio::test]
  async fn test_aborts_on_non_retriable_disconnect() {
    let non_retriable_reason = DisconnectedReason::ReachMaximumRetry;
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected {
        reason: Some(non_retriable_reason),
      },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
        max_attempts: 3,
      },
    );

    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;

    assert!(!result, "Should abort on non-retriable disconnect reason");
    assert_eq!(
      call_count.load(Ordering::SeqCst),
      0,
      "Should not attempt connection for non-retriable disconnect"
    );
  }

  #[tokio::test]
  async fn test_trigger_reconnect_with_valid_token() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(10),
        max_attempts: 1,
      },
    );

    manager.set_access_token("valid_token".into());

    // Initial state should be no reconnection in progress
    assert!(!manager.in_progress.load(Ordering::SeqCst));

    manager.trigger_reconnect("test trigger");

    // Should set in_progress flag
    assert!(manager.in_progress.load(Ordering::SeqCst));

    // Wait a bit for the async reconnection to complete
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should have attempted connection and reset the in_progress flag
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
    assert!(!manager.in_progress.load(Ordering::SeqCst));
  }

  #[tokio::test]
  async fn test_trigger_reconnect_without_token() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::new_for_target(target.clone());

    // Don't set an access token
    manager.trigger_reconnect("test trigger without token");

    // Should not set in_progress flag
    assert!(!manager.in_progress.load(Ordering::SeqCst));

    // Wait a bit to ensure no async work is done
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Should not have attempted connection
    assert_eq!(call_count.load(Ordering::SeqCst), 0);
  }

  #[tokio::test]
  async fn test_trigger_reconnect_when_already_in_progress() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(()), Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(50), // Longer delay to test concurrency
        max_delay: Duration::from_millis(50),
        max_attempts: 1,
      },
    );

    manager.set_access_token("valid_token".into());

    // Start first reconnection
    manager.trigger_reconnect("first trigger");
    assert!(manager.in_progress.load(Ordering::SeqCst));

    // Try to start second reconnection while first is in progress
    manager.trigger_reconnect("second trigger");

    // Wait for completion
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should have only attempted connection once (second call should be ignored)
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
    assert!(!manager.in_progress.load(Ordering::SeqCst));
  }

  #[tokio::test]
  async fn test_exponential_backoff_timing() {
    let responses = vec![
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "fail 1".into(),
      }),
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "fail 2".into(),
      }),
      Ok(()),
    ];
    let (fake_target, call_count) =
      FakeTarget::new(ConnectionStatus::Disconnected { reason: None }, responses);
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(100),
        max_attempts: 3,
      },
    );

    let start = std::time::Instant::now();
    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;
    let elapsed = start.elapsed();

    assert!(result, "Should eventually succeed");
    assert_eq!(call_count.load(Ordering::SeqCst), 3);

    // Should have waited at least: initial_delay + (initial_delay * 2) = 10ms + 20ms = 30ms
    assert!(
      elapsed >= Duration::from_millis(25),
      "Should respect exponential backoff timing, elapsed: {:?}",
      elapsed
    );
  }

  #[tokio::test]
  async fn test_spawn_reconnection_integration() {
    let retriable_reason = DisconnectedReason::Unexpected("connection lost".into());
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());

    let manager = Arc::new(ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(10),
        max_attempts: 1,
      },
    ));
    manager.set_access_token("test_token".into());

    // Start the reconnection monitoring
    spawn_reconnection(Arc::downgrade(&manager), target.status_channel().clone());

    // Initially not in progress
    assert!(!manager.in_progress.load(Ordering::SeqCst));

    // Simulate a retriable disconnection
    fake_target
      .set_status(ConnectionStatus::Disconnected {
        reason: Some(retriable_reason),
      })
      .await;

    // Give some time for the spawn_reconnection task to react
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should have triggered reconnection
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn test_spawn_reconnection_ignores_non_retriable() {
    let non_retriable_reason = DisconnectedReason::Unauthorized("invalid token".into());
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());

    let manager = Arc::new(ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(10),
        max_attempts: 1,
      },
    ));
    manager.set_access_token("test_token".into());

    spawn_reconnection(Arc::downgrade(&manager), target.status_channel().clone());

    // Simulate a non-retriable disconnection
    fake_target
      .set_status(ConnectionStatus::Disconnected {
        reason: Some(non_retriable_reason),
      })
      .await;

    // Give some time for the spawn_reconnection task to potentially react
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should NOT have triggered reconnection
    assert_eq!(call_count.load(Ordering::SeqCst), 0);
    assert!(!manager.in_progress.load(Ordering::SeqCst));
  }

  #[tokio::test]
  async fn test_start_reconnect_status_triggers_reconnection() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());

    let manager = Arc::new(ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(10),
        max_attempts: 1,
      },
    ));
    manager.set_access_token("test_token".into());

    spawn_reconnection(Arc::downgrade(&manager), target.status_channel().clone());

    // Initially not in progress
    assert!(!manager.in_progress.load(Ordering::SeqCst));

    // Trigger StartReconnect status
    fake_target
      .set_status(ConnectionStatus::StartReconnect)
      .await;

    // Give some time for the spawn_reconnection task to react
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should have triggered reconnection
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn test_start_reconnect_without_token() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());

    let manager = Arc::new(ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(10),
        max_attempts: 1,
      },
    ));
    // Don't set access token

    spawn_reconnection(Arc::downgrade(&manager), target.status_channel().clone());

    // Trigger StartReconnect status
    fake_target
      .set_status(ConnectionStatus::StartReconnect)
      .await;

    // Give some time for the spawn_reconnection task to react
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should NOT have triggered reconnection due to missing token
    assert_eq!(call_count.load(Ordering::SeqCst), 0);
    assert!(!manager.in_progress.load(Ordering::SeqCst));
  }

  #[tokio::test]
  async fn test_multiple_status_changes_handled_correctly() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(()), Ok(()), Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());

    let manager = Arc::new(ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(5),
        max_attempts: 1,
      },
    ));
    manager.set_access_token("test_token".into());

    spawn_reconnection(Arc::downgrade(&manager), target.status_channel().clone());

    // Send multiple status changes
    fake_target
      .set_status(ConnectionStatus::StartReconnect)
      .await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    fake_target
      .set_status(ConnectionStatus::Disconnected {
        reason: Some(DisconnectedReason::Unexpected("network error".into())),
      })
      .await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    fake_target
      .set_status(ConnectionStatus::StartReconnect)
      .await;
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Should have attempted reconnection multiple times
    let final_count = call_count.load(Ordering::SeqCst);
    assert!(
      final_count >= 2,
      "Expected at least 2 reconnection attempts, got {}",
      final_count
    );
  }

  // ============= STATUS TRANSITION EDGE CASES =============

  #[tokio::test]
  async fn test_connected_status_ignored_by_retry_logic() {
    // Test that retry logic aborts when status is Connected
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
        max_attempts: 1,
      },
    );

    // Change to a Connected-like state (we'll simulate by changing to Connecting)
    fake_target
      .set_status(ConnectionStatus::Connecting {
        cancel: CancellationToken::new(),
      })
      .await;

    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;

    // Should abort when status indicates already connected/connecting
    assert!(!result, "Should abort when already connected/connecting");
    assert_eq!(
      call_count.load(Ordering::SeqCst),
      0,
      "Should not attempt connection"
    );
  }

  #[tokio::test]
  async fn test_status_change_during_retry_sleep() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(50), // Long enough to change status during sleep
        max_delay: Duration::from_millis(50),
        max_attempts: 1,
      },
    );

    // Start retry in background
    let manager_clone = manager.clone();
    let target_clone = target.clone();
    let handle = tokio::spawn(async move {
      manager_clone
        .retry_with_exponential_backoff(target_clone, "test_token".into())
        .await
    });

    // Change status to Connecting while retry is sleeping
    tokio::time::sleep(Duration::from_millis(25)).await;
    fake_target
      .set_status(ConnectionStatus::Connecting {
        cancel: CancellationToken::new(),
      })
      .await;

    let result = handle.await.unwrap();

    // Should abort after sleep when it checks status and finds Connecting
    assert!(
      !result,
      "Should abort when status changes to Connecting during sleep"
    );
    assert_eq!(
      call_count.load(Ordering::SeqCst),
      0,
      "Should not attempt connection"
    );
  }

  // ============= TOKEN EDGE CASES =============

  #[tokio::test]
  async fn test_empty_string_token() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::new_for_target(target.clone());

    manager.set_access_token("".into()); // Empty string
    manager.trigger_reconnect("test trigger");

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Should not attempt connection with empty token
    assert_eq!(call_count.load(Ordering::SeqCst), 0);
    assert!(!manager.in_progress.load(Ordering::SeqCst));
  }

  #[tokio::test]
  async fn test_token_change_during_reconnection() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(()), Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(20),
        max_delay: Duration::from_millis(20),
        max_attempts: 1,
      },
    );

    manager.set_access_token("initial_token".into());
    manager.trigger_reconnect("first trigger");

    // Change token while first reconnection is in progress
    tokio::time::sleep(Duration::from_millis(5)).await;
    manager.set_access_token("new_token".into());

    // Trigger another reconnection
    manager.trigger_reconnect("second trigger");

    tokio::time::sleep(Duration::from_millis(50)).await;

    // First reconnection should complete, second should be ignored (in_progress)
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
    assert!(!manager.in_progress.load(Ordering::SeqCst));
  }

  #[tokio::test]
  async fn test_very_long_token() {
    let long_token = "a".repeat(10000); // 10KB token
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(10),
        max_attempts: 1,
      },
    );

    manager.set_access_token(long_token);
    manager.trigger_reconnect("test trigger");

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should handle very long tokens without issues
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
  }

  // ============= ERROR HANDLING EDGE CASES =============

  #[tokio::test]
  async fn test_specific_error_codes_handling() {
    let error_test_cases = vec![
      (ErrorCode::UserUnAuthorized, true), // Should trigger disconnect
      (ErrorCode::NetworkError, true),
      (ErrorCode::RequestTimeout, true),
      (ErrorCode::Internal, true),
    ];

    for (error_code, should_set_disconnected) in error_test_cases {
      let (fake_target, call_count) = FakeTarget::new(
        ConnectionStatus::Disconnected { reason: None },
        vec![Err(AppResponseError {
          code: error_code,
          message: "test error".into(),
        })],
      );
      let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());
      let manager = ReconnectionManager::with_config_for_target(
        target.clone(),
        RetryConfig {
          initial_delay: Duration::from_millis(1),
          max_delay: Duration::from_millis(1),
          max_attempts: 1,
        },
      );

      let result = manager
        .retry_with_exponential_backoff(target.clone(), "test_token".into())
        .await;

      assert!(!result, "Should fail for error code: {:?}", error_code);
      assert_eq!(call_count.load(Ordering::SeqCst), 1);

      if should_set_disconnected {
        let disconnect_reason = fake_target.get_last_disconnect_reason().await;
        assert!(
          disconnect_reason.is_some(),
          "Should set disconnect reason for error: {:?}",
          error_code
        );
      }
    }
  }

  #[tokio::test]
  async fn test_target_weak_reference_expired() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());

    let manager = Arc::new(ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig::default(),
    ));
    manager.set_access_token("test_token".into());

    // Drop the target reference
    drop(target);

    // Trigger reconnection after target is dropped
    manager.trigger_reconnect("test trigger");

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should not crash and should reset in_progress flag
    assert!(!manager.in_progress.load(Ordering::SeqCst));
    assert_eq!(call_count.load(Ordering::SeqCst), 0);
  }

  // ============= BACKOFF CALCULATION EDGE CASES =============

  #[tokio::test]
  async fn test_backoff_delay_capping_behavior() {
    let responses = vec![
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "fail 1".into(),
      }),
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "fail 2".into(),
      }),
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "fail 3".into(),
      }),
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "fail 4".into(),
      }),
      Ok(()),
    ];
    let (fake_target, call_count) =
      FakeTarget::new(ConnectionStatus::Disconnected { reason: None }, responses);
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(25), // Should cap at this value
        max_attempts: 5,
      },
    );

    let start = std::time::Instant::now();
    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;
    let elapsed = start.elapsed();

    assert!(result, "Should succeed");
    assert_eq!(call_count.load(Ordering::SeqCst), 5);

    // Expected delays: 10ms, 20ms, 25ms (capped), 25ms (capped)
    // Total minimum: 10 + 20 + 25 + 25 = 80ms
    assert!(
      elapsed >= Duration::from_millis(75),
      "Should respect delay capping: {:?}",
      elapsed
    );
    assert!(
      elapsed < Duration::from_millis(200),
      "Should not exceed reasonable bounds: {:?}",
      elapsed
    );
  }

  // ============= spawn_reconnection EDGE CASES =============

  #[tokio::test]
  async fn test_multiple_spawn_reconnection_calls() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(()); 5],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());

    let manager = Arc::new(ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(5),
        max_delay: Duration::from_millis(5),
        max_attempts: 1,
      },
    ));
    manager.set_access_token("test_token".into());

    // Start multiple spawn_reconnection tasks
    for _ in 0..3 {
      spawn_reconnection(Arc::downgrade(&manager), target.status_channel().clone());
    }

    // Trigger reconnection
    fake_target
      .set_status(ConnectionStatus::StartReconnect)
      .await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should handle multiple spawn_reconnection tasks gracefully
    // The in_progress flag should prevent multiple concurrent reconnections
    let attempts = call_count.load(Ordering::SeqCst);
    assert!(
      attempts >= 1,
      "Should have at least one reconnection attempt"
    );
    assert!(
      attempts <= 3,
      "Should not have excessive attempts due to multiple spawn tasks: {}",
      attempts
    );
  }

  #[tokio::test]
  async fn test_state_consistency_during_concurrent_operations() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(()); 10],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());

    let manager = Arc::new(ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(10),
        max_attempts: 1,
      },
    ));
    manager.set_access_token("test_token".into());

    spawn_reconnection(Arc::downgrade(&manager), target.status_channel().clone());

    // Concurrent operations: status changes, trigger_reconnect calls, token changes
    let tasks = vec![
      // Status change task
      {
        let fake_target = fake_target.clone();
        tokio::spawn(async move {
          for i in 0..5 {
            fake_target
              .set_status(ConnectionStatus::StartReconnect)
              .await;
            tokio::time::sleep(Duration::from_millis(5)).await;
            fake_target
              .set_status(ConnectionStatus::Disconnected {
                reason: Some(DisconnectedReason::Unexpected(
                  format!("error {}", i).into(),
                )),
              })
              .await;
            tokio::time::sleep(Duration::from_millis(5)).await;
          }
        })
      },
      // Direct trigger task
      {
        let manager = manager.clone();
        tokio::spawn(async move {
          for i in 0..3 {
            manager.trigger_reconnect(&format!("manual trigger {}", i));
            tokio::time::sleep(Duration::from_millis(15)).await;
          }
        })
      },
      // Token change task
      {
        let manager = manager.clone();
        tokio::spawn(async move {
          for i in 0..3 {
            manager.set_access_token(format!("token_{}", i));
            tokio::time::sleep(Duration::from_millis(20)).await;
          }
        })
      },
    ];

    // Wait for all tasks to complete
    for task in tasks {
      let _ = task.await;
    }

    // Wait for all reconnections to settle
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify final state consistency
    assert!(
      !manager.in_progress.load(Ordering::SeqCst),
      "in_progress flag should be reset"
    );

    let final_attempts = call_count.load(Ordering::SeqCst);
    assert!(
      final_attempts > 0,
      "Should have attempted at least one reconnection"
    );
    assert!(
      final_attempts <= 15,
      "Should not have excessive attempts: {}",
      final_attempts
    );
  }

  // ============= CONFIGURATION EDGE CASES =============

  #[tokio::test]
  async fn test_zero_max_attempts() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
        max_attempts: 0, // Zero attempts
      },
    );

    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;

    assert!(!result, "Should fail with zero max attempts");
    assert_eq!(
      call_count.load(Ordering::SeqCst),
      0,
      "Should not attempt connection with zero max attempts"
    );
  }

  #[tokio::test]
  async fn test_zero_initial_delay() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(0), // Zero delay
        max_delay: Duration::from_millis(100),
        max_attempts: 1,
      },
    );

    let start = std::time::Instant::now();
    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;
    let elapsed = start.elapsed();

    assert!(result, "Should succeed with zero initial delay");
    assert_eq!(call_count.load(Ordering::SeqCst), 1);
    // Should complete very quickly with zero delay
    assert!(
      elapsed < Duration::from_millis(50),
      "Should complete quickly with zero delay"
    );
  }

  #[tokio::test]
  async fn test_max_delay_smaller_than_initial_delay() {
    let responses = vec![
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "fail 1".into(),
      }),
      Err(AppResponseError {
        code: ErrorCode::NetworkError,
        message: "fail 2".into(),
      }),
      Ok(()),
    ];
    let (fake_target, call_count) =
      FakeTarget::new(ConnectionStatus::Disconnected { reason: None }, responses);
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(50),
        max_delay: Duration::from_millis(10), // Smaller than initial
        max_attempts: 3,
      },
    );

    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;

    assert!(result, "Should succeed despite config inconsistency");
    assert_eq!(call_count.load(Ordering::SeqCst), 3);
  }

  #[tokio::test]
  async fn test_connecting_status_prevents_reconnection() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Connecting {
        cancel: CancellationToken::new(),
      },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target);
    let manager = ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(1),
        max_attempts: 1,
      },
    );

    let result = manager
      .retry_with_exponential_backoff(target.clone(), "test_token".into())
      .await;

    assert!(!result, "Should abort when already connecting");
    assert_eq!(
      call_count.load(Ordering::SeqCst),
      0,
      "Should not attempt connection when already connecting"
    );
  }

  #[tokio::test]
  async fn test_spawn_reconnection_terminates_when_manager_dropped() {
    let (fake_target, call_count) = FakeTarget::new(
      ConnectionStatus::Disconnected { reason: None },
      vec![Ok(())],
    );
    let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());

    let manager = Arc::new(ReconnectionManager::with_config_for_target(
      target.clone(),
      RetryConfig {
        initial_delay: Duration::from_millis(10),
        max_delay: Duration::from_millis(10),
        max_attempts: 1,
      },
    ));
    manager.set_access_token("test_token".into());

    spawn_reconnection(Arc::downgrade(&manager), target.status_channel().clone());

    // Drop the manager
    drop(manager);

    // Try to trigger reconnection after manager is dropped
    fake_target
      .set_status(ConnectionStatus::StartReconnect)
      .await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Should NOT have triggered reconnection since manager was dropped
    assert_eq!(call_count.load(Ordering::SeqCst), 0);
  }

  #[tokio::test]
  async fn test_retriable_vs_non_retriable_disconnect_reasons() {
    let test_cases = vec![
      (DisconnectedReason::Unexpected("network error".into()), true),
      (DisconnectedReason::ResetWithoutClosingHandshake, true),
      (DisconnectedReason::ReachMaximumRetry, false),
      (
        DisconnectedReason::Unauthorized("invalid token".into()),
        false,
      ),
      (
        DisconnectedReason::UserDisconnect("manual disconnect".into()),
        false,
      ),
    ];

    for (reason, should_reconnect) in test_cases {
      let (fake_target, call_count) = FakeTarget::new(
        ConnectionStatus::Disconnected { reason: None },
        vec![Ok(())],
      );
      let target: Arc<dyn ReconnectTarget + Send + Sync> = Arc::new(fake_target.clone());

      let manager = Arc::new(ReconnectionManager::with_config_for_target(
        target.clone(),
        RetryConfig {
          initial_delay: Duration::from_millis(5),
          max_delay: Duration::from_millis(5),
          max_attempts: 1,
        },
      ));
      manager.set_access_token("test_token".into());

      spawn_reconnection(Arc::downgrade(&manager), target.status_channel().clone());

      // Trigger disconnection with the specific reason
      fake_target
        .set_status(ConnectionStatus::Disconnected {
          reason: Some(reason.clone()),
        })
        .await;

      tokio::time::sleep(Duration::from_millis(30)).await;
      let attempts = call_count.load(Ordering::SeqCst);
      if should_reconnect {
        assert!(
          attempts > 0,
          "Expected reconnection for retriable reason: {:?}",
          reason
        );
      } else {
        assert_eq!(
          attempts, 0,
          "Expected no reconnection for non-retriable reason: {:?}",
          reason
        );
      }
    }
  }
}
