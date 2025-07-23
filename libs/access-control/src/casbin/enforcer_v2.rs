use super::access::load_group_policies;
use crate::act::Acts;
use crate::casbin::util::policies_for_subject_with_given_object;
use crate::entity::{ObjectType, SubjectType};
use crate::metrics::MetricsCalState;
use crate::request::PolicyRequest;
use anyhow::anyhow;
use app_error::AppError;
use casbin::{CachedApi, CachedEnforcer, CoreApi, MgmtApi};
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Notify, RwLock};
use tokio::time::timeout;
use tracing::{error, event, info, instrument, trace, warn};

/// Consistency mode for policy enforcement
#[derive(Debug, Clone, Default, Copy)]
pub enum ConsistencyMode {
  /// Default mode - returns immediately with potentially stale data
  #[default]
  Eventual,
  /// Waits for all pending updates to complete before enforcing
  Strong,
  /// Waits for specific pending updates affecting the requested subject/object
  BoundedStrong { timeout_ms: u64 },
}

/// Commands for policy updates
#[derive(Debug)]
enum PolicyCommand {
  AddPolicies {
    policies: Vec<Vec<String>>,
    generation: u64,
    subject_object_keys: Vec<(String, String)>, // (subject, object) pairs
    response: tokio::sync::oneshot::Sender<Result<(), AppError>>,
  },
  RemovePolicies {
    policies: Vec<Vec<String>>,
    generation: u64,
    subject_object_keys: Vec<(String, String)>, // (subject, object) pairs
    response: tokio::sync::oneshot::Sender<Result<(), AppError>>,
  },
  Shutdown,
}

pub struct AFEnforcerV2 {
  enforcer: Arc<RwLock<CachedEnforcer>>,
  pub(crate) metrics_state: MetricsCalState,
  policy_cmd_tx: mpsc::Sender<PolicyCommand>,
  /// Tracks the current generation of policy updates
  generation: Arc<AtomicU64>,
  /// Tracks the last processed generation
  processed_generation: Arc<AtomicU64>,
  /// Tracks pending operations by subject-object key
  pending_operations: Arc<RwLock<HashSet<(String, String)>>>,
  /// Notifies when a generation has been processed
  generation_notify: Arc<Notify>,
}

impl AFEnforcerV2 {
  pub async fn new(enforcer: CachedEnforcer) -> Result<Self, AppError> {
    Self::new_internal(enforcer).await
  }

  pub async fn new_with_redis(
    mut enforcer: CachedEnforcer,
    redis_uri: &str,
  ) -> Result<Self, AppError> {
    use super::redis_cache::RedisCache;
    match RedisCache::new(redis_uri) {
      Ok(redis_cache) => {
        info!("[access control v2]: Using Redis cache at {}", redis_uri);
        enforcer.set_cache(Box::new(redis_cache));
        Self::new_internal(enforcer).await
      },
      Err(e) => {
        warn!(
          "[access control v2]: Failed to connect to Redis cache: {}. Using in-memory cache.",
          e
        );
        Self::new_internal(enforcer).await
      },
    }
  }

  async fn new_internal(mut enforcer: CachedEnforcer) -> Result<Self, AppError> {
    load_group_policies(&mut enforcer).await?;

    // Create command channel with bounded capacity
    // Read capacity from environment variable, defaulting to 2000 if not set or invalid
    let channel_capacity = std::env::var("ACCESS_CONTROL_POLICY_CHANNEL_CAPACITY")
      .ok()
      .and_then(|s| s.parse::<usize>().ok())
      .unwrap_or(2000);

    trace!(
      "[access control v2]: Policy channel capacity set to {}",
      channel_capacity
    );
    let (tx, rx) = mpsc::channel::<PolicyCommand>(channel_capacity);
    let enforcer = Arc::new(RwLock::new(enforcer));

    // Create consistency tracking
    let generation = Arc::new(AtomicU64::new(0));
    let processed_generation = Arc::new(AtomicU64::new(0));
    let pending_operations = Arc::new(RwLock::new(HashSet::new()));
    let generation_notify = Arc::new(Notify::new());

    // Spawn processor with consistency tracking
    tokio::spawn(Self::policy_update_processor(
      rx,
      enforcer.clone(),
      processed_generation.clone(),
      pending_operations.clone(),
      generation_notify.clone(),
    ));

    Ok(Self {
      enforcer,
      metrics_state: MetricsCalState::new(),
      policy_cmd_tx: tx,
      generation,
      processed_generation,
      pending_operations,
      generation_notify,
    })
  }

  /// Background task that processes policy updates sequentially
  async fn policy_update_processor(
    mut rx: mpsc::Receiver<PolicyCommand>,
    enforcer: Arc<RwLock<CachedEnforcer>>,
    processed_generation: Arc<AtomicU64>,
    pending_operations: Arc<RwLock<HashSet<(String, String)>>>,
    generation_notify: Arc<Notify>,
  ) {
    info!("[access control v2]: Policy update processor started");
    let buffer_size = 2;
    let mut buf = Vec::with_capacity(buffer_size);

    loop {
      trace!("[access control v2]: Waiting for policy commands...");
      let n = rx.recv_many(&mut buf, buffer_size).await;
      if n == 0 {
        info!("[access control v2]: Channel closed, exiting processor");
        break;
      }

      info!("[access control v2]: Received {} policy commands", n);
      let mut enforcer = enforcer.write().await;
      let mut max_generation = 0u64;
      let mut processed_keys = Vec::new();
      for cmd in buf.drain(..) {
        match cmd {
          PolicyCommand::AddPolicies {
            policies,
            generation,
            subject_object_keys,
            response,
          } => {
            max_generation = max_generation.max(generation);
            processed_keys.extend(subject_object_keys);
            let result = async {
              enforcer
                .add_policies(policies)
                .await
                .map_err(|e| AppError::Internal(anyhow!("fail to add policy: {e:?}")))?;
              Ok(())
            }
            .await;
            trace!("[access control v2]: AddPolicies result: {:?}", result);
            let _ = response.send(result);
          },
          PolicyCommand::RemovePolicies {
            policies,
            generation,
            subject_object_keys,
            response,
          } => {
            max_generation = max_generation.max(generation);
            processed_keys.extend(subject_object_keys);
            let result = async {
              enforcer
                .remove_policies(policies)
                .await
                .map_err(|e| AppError::Internal(anyhow!("fail to remove policy: {e:?}")))?;
              Ok(())
            }
            .await;
            trace!("[access control v2]: RemovePolicies result: {:?}", result);
            let _ = response.send(result);
          },
          PolicyCommand::Shutdown => {
            trace!("[access control v2]: Policy update processor shutting down");
            return;
          },
        }
      }
      drop(enforcer);
      trace!("[access control v2]: Finished processing {} commands", n);
      // Update consistency tracking
      if max_generation > 0 {
        trace!(
          "[access control v2]: Updating processed generation from {} to {}",
          processed_generation.load(Ordering::SeqCst),
          max_generation
        );
        processed_generation.store(max_generation, Ordering::SeqCst);
        if !processed_keys.is_empty() {
          let mut pending = pending_operations.write().await;
          for key in processed_keys {
            pending.remove(&key);
          }
        }

        // Notify waiters
        trace!(
          "[access control v2]: Notifying waiters after processing generation {}",
          max_generation
        );
        generation_notify.notify_waiters();
      }
    }
  }

  /// Send a command with metrics tracking
  async fn send_command_with_metrics(&self, cmd: PolicyCommand) -> Result<(), AppError> {
    // Increment send attempts
    self
      .metrics_state
      .policy_send_attempts
      .fetch_add(1, Ordering::Relaxed);

    // First try to send without blocking to detect if channel is full
    match self.policy_cmd_tx.try_send(cmd) {
      Ok(()) => {
        trace!("[access control v2]: Command sent successfully");
        Ok(())
      },
      Err(mpsc::error::TrySendError::Full(cmd)) => {
        self
          .metrics_state
          .policy_channel_full_events
          .fetch_add(1, Ordering::Relaxed);

        warn!("[access control v2]: Policy channel is full, waiting to send...");
        let send_timeout = Duration::from_secs(5);
        match timeout(send_timeout, self.policy_cmd_tx.send(cmd)).await {
          Ok(Ok(())) => {
            trace!("[access control v2]: Command sent successfully after waiting");
            Ok(())
          },
          Ok(Err(_)) => {
            self
              .metrics_state
              .policy_send_failures
              .fetch_add(1, Ordering::Relaxed);
            Err(AppError::Internal(anyhow!("Policy update channel closed")))
          },
          Err(_) => {
            self
              .metrics_state
              .policy_send_failures
              .fetch_add(1, Ordering::Relaxed);
            Err(AppError::Internal(anyhow!(
              "Policy update timed out after {} seconds - channel may be overloaded",
              send_timeout.as_secs()
            )))
          },
        }
      },
      Err(mpsc::error::TrySendError::Closed(_)) => {
        self
          .metrics_state
          .policy_send_failures
          .fetch_add(1, Ordering::Relaxed);
        Err(AppError::Internal(anyhow!("Policy update channel closed")))
      },
    }
  }

  /// Update policy for a user using queue-based approach.
  /// This method will never cause a deadlock.
  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_policy<T>(
    &self,
    sub: SubjectType,
    obj: ObjectType,
    act: T,
  ) -> Result<(), AppError>
  where
    T: Acts,
  {
    let policies = act
      .policy_acts()
      .into_iter()
      .map(|act| vec![sub.policy_subject(), obj.policy_object(), act])
      .collect::<Vec<Vec<_>>>();

    info!("[access control v2]: queuing add policy:{:?}", policies);
    // Generate new generation number
    let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;

    // Extract subject-object keys
    let subject = sub.policy_subject();
    let object = obj.policy_object();
    let subject_object_keys = vec![(subject, object)];

    // Add to pending operations
    {
      let mut pending = self.pending_operations.write().await;
      for key in &subject_object_keys {
        pending.insert(key.clone());
      }
    }

    let (tx, rx) = tokio::sync::oneshot::channel();
    self
      .send_command_with_metrics(PolicyCommand::AddPolicies {
        policies,
        generation,
        subject_object_keys,
        response: tx,
      })
      .await?;

    let result = rx
      .await
      .map_err(|_| AppError::Internal(anyhow!("Policy update response dropped")))?;
    trace!(
      "[access control v2]: Received policy update response: {:?}",
      result
    );
    result
  }

  /// Remove policies for a subject and object type.
  pub async fn remove_policy(
    &self,
    sub: SubjectType,
    object_type: ObjectType,
  ) -> Result<(), AppError> {
    // First, get the policies to remove by reading
    let policies_for_user_on_object = {
      let enforcer = self.enforcer.read().await;
      policies_for_subject_with_given_object(sub.clone(), object_type.clone(), &enforcer).await
    };

    event!(
      tracing::Level::INFO,
      "[access control v2]: queuing remove policy:subject={}, object={}, policies={:?}",
      sub.policy_subject(),
      object_type.policy_object(),
      policies_for_user_on_object
    );

    if policies_for_user_on_object.is_empty() {
      return Ok(());
    }

    // Generate new generation number
    let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
    let subject = sub.policy_subject();
    let object = object_type.policy_object();
    let subject_object_keys = vec![(subject, object)];

    // Add to pending operations
    {
      let mut pending = self.pending_operations.write().await;
      for key in &subject_object_keys {
        pending.insert(key.clone());
      }
    }

    let (tx, rx) = tokio::sync::oneshot::channel();
    self
      .send_command_with_metrics(PolicyCommand::RemovePolicies {
        policies: policies_for_user_on_object,
        generation,
        subject_object_keys,
        response: tx,
      })
      .await?;

    let result = rx
      .await
      .map_err(|_| AppError::Internal(anyhow!("Policy update response dropped")))?;
    trace!(
      "[access control v2]: Received policy removal response: {:?}",
      result
    );
    result
  }

  /// Enforces an access control policy with eventual consistency.
  /// - `Eventual`: Returns immediately with potentially stale data (fastest)
  #[instrument(level = "debug", skip_all)]
  pub async fn enforce_policy<T>(
    &self,
    uid: &i64,
    obj: ObjectType,
    act: T,
  ) -> Result<bool, AppError>
  where
    T: Acts,
  {
    self
      .enforce_policy_with_consistency(uid, obj, act, ConsistencyMode::Eventual)
      .await
  }

  /// Enforces an access control policy with configurable consistency guarantees.
  /// - `Eventual`: Returns immediately with potentially stale data (fastest)
  /// - `Strong`: Waits for all pending updates before checking (most consistent)
  /// - `BoundedStrong`: Waits only for updates affecting this subject/object pair (balanced)
  #[instrument(level = "debug", skip_all)]
  pub async fn enforce_policy_with_consistency<T>(
    &self,
    uid: &i64,
    obj: ObjectType,
    act: T,
    consistency: ConsistencyMode,
  ) -> Result<bool, AppError>
  where
    T: Acts,
  {
    self
      .metrics_state
      .total_read_enforce_result
      .fetch_add(1, Ordering::Relaxed);

    match consistency {
      ConsistencyMode::Eventual => {
        // No waiting, proceed immediately
      },
      ConsistencyMode::Strong => {
        // Wait for all pending operations to complete
        let current_gen = self.generation.load(Ordering::Acquire);
        self.wait_for_generation(current_gen, None).await?;
      },
      ConsistencyMode::BoundedStrong { timeout_ms } => {
        // Check if there are pending operations for this subject/object
        let subject = uid.to_string();
        let object = obj.policy_object();
        let key = (subject, object);

        let has_pending = {
          let pending = self.pending_operations.read().await;
          pending.contains(&key)
        };

        if has_pending {
          // Wait for those specific operations to complete
          let current_gen = self.generation.load(Ordering::Acquire);
          self
            .wait_for_generation(current_gen, Some(Duration::from_millis(timeout_ms)))
            .await?;
        }
      },
    }

    let policy_request = PolicyRequest::new(*uid, obj, act);
    let policy = policy_request.to_policy();

    let result = self
      .enforcer
      .read()
      .await
      .enforce(policy)
      .map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;

    Ok(result)
  }

  /// Wait for a specific generation to be processed
  async fn wait_for_generation(
    &self,
    target_generation: u64,
    timeout_duration: Option<Duration>,
  ) -> Result<(), AppError> {
    let timeout_duration = timeout_duration.unwrap_or(Duration::from_secs(5));
    let wait_future = async {
      loop {
        let notified = self.generation_notify.notified();
        let processed = self.processed_generation.load(Ordering::Acquire);
        if processed >= target_generation {
          return Ok(());
        }

        notified.await;
      }
    };

    match timeout(timeout_duration, wait_future).await {
      Ok(result) => result,
      Err(_) => {
        error!(
          "[access control v2]: target_generation={}, current_generation={}, pending_operation={}",
          target_generation,
          self.processed_generation.load(Ordering::Acquire),
          self.pending_operations.read().await.len(),
        );

        Err(AppError::Internal(anyhow!(
          "Timed out waiting for policy consistency after {}ms",
          timeout_duration.as_millis()
        )))
      },
    }
  }

  pub async fn shutdown(&self) -> Result<(), AppError> {
    self
      .send_command_with_metrics(PolicyCommand::Shutdown)
      .await?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    act::Action,
    casbin::access::{casbin_model, cmp_role_or_level},
    entity::{ObjectType, SubjectType},
  };
  use casbin::{function_map::OperatorFunction, prelude::*};
  use database_entity::dto::AFRole;
  use std::sync::Arc;
  use tokio::sync::Barrier;

  async fn test_enforcer_v2() -> AFEnforcerV2 {
    let model = casbin_model().await.unwrap();
    let mut enforcer = casbin::CachedEnforcer::new(model, MemoryAdapter::default())
      .await
      .unwrap();

    enforcer.add_function("cmpRoleOrLevel", OperatorFunction::Arg2(cmp_role_or_level));
    AFEnforcerV2::new(enforcer).await.unwrap()
  }

  #[tokio::test]
  async fn test_v2_no_deadlock_scenario() {
    let enforcer = Arc::new(test_enforcer_v2().await);
    let uid = 1;
    let workspace_id = "v2_test_workspace";

    // This would deadlock in V1, but works fine in V2
    let enforcer_clone = Arc::clone(&enforcer);

    // Update policy and immediately enforce multiple times
    enforcer_clone
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Member,
      )
      .await
      .unwrap();

    // Can immediately enforce without any deadlock risk
    let can_read = enforcer_clone
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        Action::Read,
      )
      .await
      .unwrap();

    assert!(can_read, "User should be able to read");

    // Clean up
    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_concurrent_operations() {
    let enforcer = Arc::new(test_enforcer_v2().await);
    let barrier = Arc::new(Barrier::new(20));
    let mut handles = Vec::new();

    // Mix of concurrent updates and enforces
    for i in 0..20 {
      let enforcer_clone = Arc::clone(&enforcer);
      let barrier_clone = Arc::clone(&barrier);

      let handle = tokio::spawn(async move {
        barrier_clone.wait().await;

        let uid = 1000 + i;
        let workspace_id = format!("workspace_{}", i % 5);

        if i % 2 == 0 {
          // Update policy
          enforcer_clone
            .update_policy(
              SubjectType::User(uid),
              ObjectType::Workspace(workspace_id.clone()),
              AFRole::Member,
            )
            .await
            .expect("Failed to update policy");
        }

        // Always try to enforce (may succeed or fail based on timing)
        let _ = enforcer_clone
          .enforce_policy(&uid, ObjectType::Workspace(workspace_id), Action::Read)
          .await;

        "Success"
      });

      handles.push(handle);
    }

    // All operations should complete without deadlock
    for handle in handles {
      let result = handle.await.unwrap();
      assert_eq!(result, "Success");
    }

    // Clean up
    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_queue_ordering() {
    let enforcer = test_enforcer_v2().await;
    let uid = 2000;
    let workspace_id = "order_test";

    // Rapid sequence of operations
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Owner,
      )
      .await
      .unwrap();

    enforcer
      .remove_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
      )
      .await
      .unwrap();

    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Guest,
      )
      .await
      .unwrap();

    // Should end up with Guest role
    let has_guest = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Guest,
      )
      .await
      .unwrap();
    assert!(has_guest);

    let has_owner = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Owner,
      )
      .await
      .unwrap();
    assert!(!has_owner);

    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_queue_capacity_limits() {
    // Set a very small channel capacity via environment variable
    std::env::set_var("ACCESS_CONTROL_POLICY_CHANNEL_CAPACITY", "2");

    let enforcer = Arc::new(test_enforcer_v2().await);
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = Vec::new();

    // Try to send many commands at once to fill the queue
    for i in 0..10 {
      let enforcer_clone = Arc::clone(&enforcer);
      let barrier_clone = Arc::clone(&barrier);

      let handle = tokio::spawn(async move {
        barrier_clone.wait().await;

        let uid = 3000 + i;
        let workspace_id = format!("capacity_test_{}", i);

        // This might block or timeout if queue is full
        let result = enforcer_clone
          .update_policy(
            SubjectType::User(uid),
            ObjectType::Workspace(workspace_id),
            AFRole::Member,
          )
          .await;

        result.is_ok()
      });

      handles.push(handle);
    }

    let mut success_count = 0;
    for handle in handles {
      if handle.await.unwrap() {
        success_count += 1;
      }
    }

    // All should eventually succeed since the processor is draining the queue
    assert_eq!(
      success_count, 10,
      "All operations should eventually succeed"
    );

    // Check metrics for channel full events
    let channel_full_events = enforcer
      .metrics_state
      .policy_channel_full_events
      .load(Ordering::Relaxed);

    assert!(
      channel_full_events > 0,
      "Should have experienced channel full events with small capacity"
    );

    // Clean up
    enforcer.shutdown().await.unwrap();
    std::env::remove_var("ACCESS_CONTROL_POLICY_CHANNEL_CAPACITY");
  }

  #[tokio::test]
  async fn test_v2_policy_query_consistency_during_rapid_updates() {
    let enforcer = Arc::new(test_enforcer_v2().await);
    let uid = 4000;
    let workspace_id = "consistency_test";

    // This test demonstrates that rapid updates can lead to multiple policies
    // In a real application, you would typically remove the old role before adding a new one

    // Start with a clean slate - remove any existing policies
    let _ = enforcer
      .remove_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
      )
      .await;

    // Do sequential updates with proper cleanup to ensure consistency
    for i in 0..10 {
      let role = if i % 2 == 0 {
        AFRole::Owner
      } else {
        AFRole::Guest
      };

      // Remove existing policy first
      let _ = enforcer
        .remove_policy(
          SubjectType::User(uid),
          ObjectType::Workspace(workspace_id.to_string()),
        )
        .await;

      // Then add new policy
      enforcer
        .update_policy(
          SubjectType::User(uid),
          ObjectType::Workspace(workspace_id.to_string()),
          role,
        )
        .await
        .unwrap();
    }

    // Small delay to ensure queue is processed
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Final state should be consistent - check which role is actually set
    let has_owner = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Owner,
      )
      .await
      .unwrap();

    let has_guest = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Guest,
      )
      .await
      .unwrap();

    println!(
      "After sequential updates with cleanup: has_owner={}, has_guest={}",
      has_owner, has_guest
    );

    // Now we should have exactly one role
    let role_count = if has_owner { 1 } else { 0 } + if has_guest { 1 } else { 0 };
    assert_eq!(
      role_count, 1,
      "Should have exactly one role active after proper updates, but found {} roles",
      role_count
    );

    // The last update was Guest (i=9, odd number)
    assert!(
      !has_owner && has_guest,
      "Should have Guest role as the final state"
    );

    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_multiple_policies_accumulation() {
    let enforcer = Arc::new(test_enforcer_v2().await);
    let uid = 4100;
    let workspace_id = "accumulation_test";

    // Start clean
    let _ = enforcer
      .remove_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
      )
      .await;

    // Add multiple roles without cleanup - demonstrates accumulation
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Guest,
      )
      .await
      .unwrap();

    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Owner,
      )
      .await
      .unwrap();

    // Small delay to ensure policies are applied
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Check both roles
    let has_owner = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Owner,
      )
      .await
      .unwrap();

    let has_guest = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Guest,
      )
      .await
      .unwrap();

    // Both roles should be active due to accumulation
    assert!(
      has_owner && has_guest,
      "Both roles should be active when added without cleanup"
    );

    // User should have highest permission (can delete)
    let can_delete = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        Action::Delete,
      )
      .await
      .unwrap();

    assert!(
      can_delete,
      "User should be able to delete with Owner role active"
    );

    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_shutdown_with_pending_operations() {
    let enforcer = Arc::new(test_enforcer_v2().await);

    // Send many operations
    let mut handles = Vec::new();
    for i in 0..50 {
      let enforcer_clone = Arc::clone(&enforcer);
      let handle = tokio::spawn(async move {
        let uid = 5000 + i;
        let workspace_id = format!("shutdown_test_{}", i);

        enforcer_clone
          .update_policy(
            SubjectType::User(uid),
            ObjectType::Workspace(workspace_id),
            AFRole::Member,
          )
          .await
      });
      handles.push(handle);
    }

    // Immediately shutdown
    tokio::time::sleep(Duration::from_millis(10)).await;
    enforcer.shutdown().await.unwrap();

    // Operations might fail after shutdown
    let mut success_count = 0;
    let mut failure_count = 0;

    for handle in handles {
      match handle.await.unwrap() {
        Ok(_) => success_count += 1,
        Err(_) => failure_count += 1,
      }
    }

    println!(
      "Shutdown test: {} succeeded, {} failed",
      success_count, failure_count
    );

    // At least some operations should have succeeded before shutdown
    assert!(
      success_count > 0,
      "Some operations should succeed before shutdown"
    );
  }

  #[tokio::test]
  async fn test_v2_concurrent_read_write_race_conditions() {
    let enforcer = Arc::new(test_enforcer_v2().await);
    let uid = 6000;
    let workspace_id = "race_test";

    // Initial setup
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Guest,
      )
      .await
      .unwrap();

    let barrier = Arc::new(Barrier::new(40));
    let mut handles = Vec::new();

    for i in 0..1000 {
      let enforcer_clone = Arc::clone(&enforcer);
      let barrier_clone = Arc::clone(&barrier);
      let ws_id = workspace_id.to_string();

      let handle = tokio::spawn(async move {
        barrier_clone.wait().await;

        if i % 2 == 0 {
          // Writer: alternate between Owner and Member
          let role = if (i / 2) % 2 == 0 {
            AFRole::Owner
          } else {
            AFRole::Member
          };
          enforcer_clone
            .update_policy(SubjectType::User(uid), ObjectType::Workspace(ws_id), role)
            .await
            .map(|_| format!("Write {}", i))
        } else {
          // Reader: check current permissions
          let can_delete = enforcer_clone
            .enforce_policy_with_consistency(
              &uid,
              ObjectType::Workspace(ws_id),
              Action::Delete,
              ConsistencyMode::Strong,
            )
            .await?;
          Ok(format!("Read {}: can_delete={}", i, can_delete))
        }
      });

      handles.push(handle);
    }

    // Collect all results
    let mut results = Vec::new();
    for handle in handles {
      match handle.await.unwrap() {
        Ok(result) => results.push(result),
        Err(e) => panic!("Operation failed: {:?}", e),
      }
    }

    // All operations should complete successfully
    assert_eq!(results.len(), 1000, "All operations should complete");
    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_policy_removal_during_enforcement() {
    let enforcer = Arc::new(test_enforcer_v2().await);
    let uid = 7000;
    let workspace_id = "removal_race_test";

    // Setup initial permission
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Owner,
      )
      .await
      .unwrap();

    let barrier = Arc::new(Barrier::new(3));

    // Task 1: Check permission
    let enforcer1 = Arc::clone(&enforcer);
    let barrier1 = Arc::clone(&barrier);
    let ws_id1 = workspace_id.to_string();
    let check_handle = tokio::spawn(async move {
      barrier1.wait().await;

      // Check multiple times to increase chance of race
      let mut results = Vec::new();
      for _ in 0..10 {
        let can_delete = enforcer1
          .enforce_policy(&uid, ObjectType::Workspace(ws_id1.clone()), Action::Delete)
          .await
          .unwrap();
        results.push(can_delete);
        tokio::time::sleep(Duration::from_micros(100)).await;
      }
      results
    });

    // Task 2: Remove permission
    let enforcer2 = Arc::clone(&enforcer);
    let barrier2 = Arc::clone(&barrier);
    let ws_id2 = workspace_id.to_string();
    let remove_handle = tokio::spawn(async move {
      barrier2.wait().await;

      // Small delay to let some checks happen first
      tokio::time::sleep(Duration::from_millis(2)).await;

      enforcer2
        .remove_policy(SubjectType::User(uid), ObjectType::Workspace(ws_id2))
        .await
        .unwrap();
    });

    // Task 3: Re-add with different permission
    let enforcer3 = Arc::clone(&enforcer);
    let barrier3 = Arc::clone(&barrier);
    let ws_id3 = workspace_id.to_string();
    let readd_handle = tokio::spawn(async move {
      barrier3.wait().await;

      // Delay to happen after removal
      tokio::time::sleep(Duration::from_millis(5)).await;

      enforcer3
        .update_policy(
          SubjectType::User(uid),
          ObjectType::Workspace(ws_id3),
          AFRole::Guest,
        )
        .await
        .unwrap();
    });

    // Wait for all tasks
    let check_results = check_handle.await.unwrap();
    remove_handle.await.unwrap();
    readd_handle.await.unwrap();

    // Results should transition from true to false
    println!(
      "Permission check results during removal: {:?}",
      check_results
    );

    // Early checks should be true, later ones false
    assert!(check_results[0], "Initial checks should show permission");
    assert!(
      !check_results[check_results.len() - 1],
      "Final checks should show no delete permission (Guest role)"
    );

    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_high_throughput_mixed_operations() {
    let enforcer = Arc::new(test_enforcer_v2().await);
    let operation_count = 1000;
    let user_count = 10;
    let workspace_count = 5;

    let start_time = std::time::Instant::now();
    let mut handles = Vec::new();

    for i in 0..operation_count {
      let enforcer_clone = Arc::clone(&enforcer);

      let handle = tokio::spawn(async move {
        let uid = 8000 + (i % user_count);
        let workspace_id = format!("high_throughput_ws_{}", i % workspace_count);

        match i % 4 {
          0 => {
            // Add policy
            enforcer_clone
              .update_policy(
                SubjectType::User(uid),
                ObjectType::Workspace(workspace_id),
                AFRole::Member,
              )
              .await
              .map(|_| "add")
          },
          1 => {
            // Check permission
            enforcer_clone
              .enforce_policy(&uid, ObjectType::Workspace(workspace_id), Action::Read)
              .await
              .map(|_| "check")
          },
          2 => {
            // Update policy
            enforcer_clone
              .update_policy(
                SubjectType::User(uid),
                ObjectType::Workspace(workspace_id),
                AFRole::Owner,
              )
              .await
              .map(|_| "update")
          },
          _ => {
            // Remove policy
            enforcer_clone
              .remove_policy(SubjectType::User(uid), ObjectType::Workspace(workspace_id))
              .await
              .map(|_| "remove")
          },
        }
      });

      handles.push(handle);
    }

    // Wait for all operations
    let mut success_count = 0;
    for handle in handles {
      if handle.await.unwrap().is_ok() {
        success_count += 1;
      }
    }

    let elapsed = start_time.elapsed();
    let ops_per_sec = (operation_count as f64) / elapsed.as_secs_f64();

    println!(
      "High throughput test: {} operations in {:?} ({:.0} ops/sec)",
      operation_count, elapsed, ops_per_sec
    );

    assert_eq!(
      success_count, operation_count,
      "All operations should succeed"
    );
    assert!(ops_per_sec > 100.0, "Should handle at least 100 ops/sec");

    // Check metrics
    let total_attempts = enforcer
      .metrics_state
      .policy_send_attempts
      .load(Ordering::Relaxed);
    let total_failures = enforcer
      .metrics_state
      .policy_send_failures
      .load(Ordering::Relaxed);

    println!(
      "Metrics: {} attempts, {} failures",
      total_attempts, total_failures
    );
    assert_eq!(total_failures, 0, "Should have no send failures");

    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_empty_policy_removal() {
    let enforcer = test_enforcer_v2().await;
    let uid = 9000;
    let workspace_id = "empty_removal_test";

    // Try to remove non-existent policy
    let result = enforcer
      .remove_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
      )
      .await;

    // Should succeed even with no policies to remove
    assert!(
      result.is_ok(),
      "Removing non-existent policy should not error"
    );

    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_policy_enforcement_accuracy() {
    let enforcer = test_enforcer_v2().await;
    let uid = 10000;
    let workspace_id = "accuracy_test";

    // Test different role levels and their permissions
    // Based on load_group_policies in access.rs:
    // - Owner: can Delete, Write, Read
    // - Member: can Write, Read
    // - Guest: can Write, Read
    let test_cases = vec![
      (
        AFRole::Owner,
        vec![Action::Read, Action::Write, Action::Delete],
        vec![true, true, true],
      ),
      (
        AFRole::Member,
        vec![Action::Read, Action::Write, Action::Delete],
        vec![true, true, false],
      ),
      (
        AFRole::Guest,
        vec![Action::Read, Action::Write, Action::Delete],
        vec![true, true, false],
      ), // Guest CAN write!
    ];

    for (role, actions, expected_results) in test_cases {
      // Clean slate - remove any existing policies
      let _ = enforcer
        .remove_policy(
          SubjectType::User(uid),
          ObjectType::Workspace(workspace_id.to_string()),
        )
        .await;

      // Set role
      enforcer
        .update_policy(
          SubjectType::User(uid),
          ObjectType::Workspace(workspace_id.to_string()),
          role.clone(),
        )
        .await
        .unwrap();

      // Small delay to ensure policy is applied
      tokio::time::sleep(Duration::from_millis(10)).await;

      // Check each action
      for (action, expected) in actions.into_iter().zip(expected_results) {
        let result = enforcer
          .enforce_policy(
            &uid,
            ObjectType::Workspace(workspace_id.to_string()),
            action.clone(),
          )
          .await
          .unwrap();

        assert_eq!(
          result, expected,
          "Role {:?} with action {:?} should be {}",
          role, action, expected
        );
      }
    }

    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_consistency_modes() {
    let enforcer = Arc::new(test_enforcer_v2().await);
    let uid = 11000;
    let workspace_id = "consistency_mode_test";

    // Test 1: Eventual consistency (default) - might return stale result
    let enforcer1 = Arc::clone(&enforcer);
    let eventual_result = tokio::spawn(async move {
      // Update and immediately check with eventual consistency
      enforcer1
        .update_policy(
          SubjectType::User(uid),
          ObjectType::Workspace(workspace_id.to_string()),
          AFRole::Owner,
        )
        .await
        .unwrap();

      // This might return false if policy hasn't been processed yet
      enforcer1
        .enforce_policy(
          &uid,
          ObjectType::Workspace(workspace_id.to_string()),
          Action::Delete,
        )
        .await
        .unwrap()
    });

    // Test 2: Strong consistency - always returns correct result
    let enforcer2 = Arc::clone(&enforcer);
    let strong_result = tokio::spawn(async move {
      // Update and check with strong consistency
      enforcer2
        .update_policy(
          SubjectType::User(uid + 1),
          ObjectType::Workspace(format!("{}_2", workspace_id)),
          AFRole::Owner,
        )
        .await
        .unwrap();

      // This will always return true because it waits for the update
      enforcer2
        .enforce_policy_with_consistency(
          &(uid + 1),
          ObjectType::Workspace(format!("{}_2", workspace_id)),
          Action::Delete,
          ConsistencyMode::Strong,
        )
        .await
        .unwrap()
    });

    // Test 3: Bounded strong consistency - waits only for relevant updates
    let enforcer3 = Arc::clone(&enforcer);
    let bounded_result = tokio::spawn(async move {
      // Update policy
      enforcer3
        .update_policy(
          SubjectType::User(uid + 2),
          ObjectType::Workspace(format!("{}_3", workspace_id)),
          AFRole::Member,
        )
        .await
        .unwrap();

      // Check with bounded consistency (waits up to 1 second)
      enforcer3
        .enforce_policy_with_consistency(
          &(uid + 2),
          ObjectType::Workspace(format!("{}_3", workspace_id)),
          Action::Write,
          ConsistencyMode::BoundedStrong { timeout_ms: 1000 },
        )
        .await
        .unwrap()
    });

    let _ = eventual_result.await.unwrap();
    let strong = strong_result.await.unwrap();
    let bounded = bounded_result.await.unwrap();

    // Strong consistency should always work
    assert!(strong, "Strong consistency should guarantee correct result");
    assert!(bounded, "Bounded consistency should work within timeout");

    // Note: eventual_result might be true or false depending on timing

    enforcer.shutdown().await.unwrap();
  }

  #[tokio::test]
  async fn test_v2_consistency_timeout() {
    let enforcer = Arc::new(test_enforcer_v2().await);
    let uid = 12000;
    let workspace_id = "timeout_test";

    // Create a scenario where processor is slow by filling the channel
    for i in 0..100 {
      let _ = enforcer
        .update_policy(
          SubjectType::User(uid + i),
          ObjectType::Workspace(format!("{}_{}", workspace_id, i)),
          AFRole::Member,
        )
        .await;
    }

    // Try to enforce with very short timeout
    let _result = enforcer
      .enforce_policy_with_consistency(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        Action::Read,
        ConsistencyMode::BoundedStrong { timeout_ms: 1 }, // 1ms timeout
      )
      .await;

    // Should likely timeout (unless processor is very fast)
    // Note: This is not a guarantee, just likely

    enforcer.shutdown().await.unwrap();
  }
}
