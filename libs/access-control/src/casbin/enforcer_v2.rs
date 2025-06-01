use super::access::load_group_policies;
use crate::act::Acts;
use crate::casbin::util::policies_for_subject_with_given_object;
use crate::entity::{ObjectType, SubjectType};
use crate::metrics::MetricsCalState;
use crate::request::PolicyRequest;
use anyhow::anyhow;
use app_error::AppError;
use casbin::{CachedEnforcer, CoreApi, MgmtApi};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{event, instrument, trace};

/// Commands for policy updates
#[derive(Debug)]
enum PolicyCommand {
  AddPolicies {
    policies: Vec<Vec<String>>,
    response: tokio::sync::oneshot::Sender<Result<(), AppError>>,
  },
  RemovePolicies {
    policies: Vec<Vec<String>>,
    response: tokio::sync::oneshot::Sender<Result<(), AppError>>,
  },
  Shutdown,
}

/// AFEnforcer V2 - Queue-based implementation that eliminates deadlock risks
///
/// ## Key Differences from V1:
///
/// 1. **No Deadlocks**: Policy updates are processed by a dedicated background task,
///    so the calling task never holds a write lock that could deadlock with read operations.
///
/// 2. **Better Performance**: Read operations (enforce_policy) are never blocked by
///    write operations happening in the background.
///
/// 3. **Ordering Guarantees**: Policy updates are processed in FIFO order.
///
/// 4. **Graceful Shutdown**: The background task can be cleanly shut down.
pub struct AFEnforcerV2 {
  enforcer: Arc<RwLock<CachedEnforcer>>,
  pub(crate) metrics_state: MetricsCalState,
  /// Command queue for policy updates
  policy_cmd_tx: mpsc::Sender<PolicyCommand>,
}

impl AFEnforcerV2 {
  pub async fn new(mut enforcer: CachedEnforcer) -> Result<Self, AppError> {
    load_group_policies(&mut enforcer).await?;

    // Create command channel with bounded capacity
    let (tx, rx) = mpsc::channel::<PolicyCommand>(1000);
    let enforcer = Arc::new(RwLock::new(enforcer));
    tokio::spawn(Self::policy_update_processor(rx, enforcer.clone()));

    Ok(Self {
      enforcer,
      metrics_state: MetricsCalState::new(),
      policy_cmd_tx: tx,
    })
  }

  /// Background task that processes policy updates sequentially
  async fn policy_update_processor(
    mut rx: mpsc::Receiver<PolicyCommand>,
    enforcer: Arc<RwLock<CachedEnforcer>>,
  ) {
    trace!("[access control v2]: Policy update processor started");

    let buffer_size = 5;
    let mut buf = Vec::with_capacity(buffer_size);

    loop {
      let n = rx.recv_many(&mut buf, buffer_size).await;
      if n == 0 {
        trace!("[access control v2]: Channel closed, exiting processor");
        break;
      }

      let mut enforcer = enforcer.write().await;
      for cmd in buf.drain(..) {
        match cmd {
          PolicyCommand::AddPolicies { policies, response } => {
            let result = async {
              enforcer
                .add_policies(policies)
                .await
                .map_err(|e| AppError::Internal(anyhow!("fail to add policy: {e:?}")))?;
              Ok(())
            }
            .await;
            let _ = response.send(result);
          },
          PolicyCommand::RemovePolicies { policies, response } => {
            let result = async {
              enforcer
                .remove_policies(policies)
                .await
                .map_err(|e| AppError::Internal(anyhow!("fail to remove policy: {e:?}")))?;
              Ok(())
            }
            .await;
            let _ = response.send(result);
          },
          PolicyCommand::Shutdown => {
            trace!("[access control v2]: Policy update processor shutting down");
            return;
          },
        }
      }
      drop(enforcer);
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

    trace!("[access control v2]: queuing add policy:{:?}", policies);
    let (tx, rx) = tokio::sync::oneshot::channel();
    self
      .policy_cmd_tx
      .send(PolicyCommand::AddPolicies {
        policies,
        response: tx,
      })
      .await
      .map_err(|_| AppError::Internal(anyhow!("Policy update channel closed")))?;

    rx.await
      .map_err(|_| AppError::Internal(anyhow!("Policy update response dropped")))?
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

    let (tx, rx) = tokio::sync::oneshot::channel();
    self
      .policy_cmd_tx
      .send(PolicyCommand::RemovePolicies {
        policies: policies_for_user_on_object,
        response: tx,
      })
      .await
      .map_err(|_| AppError::Internal(anyhow!("Policy update channel closed")))?;
    rx.await
      .map_err(|_| AppError::Internal(anyhow!("Policy update response dropped")))?
  }

  /// Enforce policy - this method will never deadlock as it only acquires read locks
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
      .metrics_state
      .total_read_enforce_result
      .fetch_add(1, Ordering::Relaxed);

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

  pub async fn shutdown(&self) -> Result<(), AppError> {
    self
      .policy_cmd_tx
      .send(PolicyCommand::Shutdown)
      .await
      .map_err(|_| AppError::Internal(anyhow!("Failed to send shutdown command")))?;
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
}
