use crate::biz::casbin::access_control::{
  ActionType, ActionVariant, ObjectType, ToACAction, POLICY_FIELD_INDEX_OBJECT,
  POLICY_FIELD_INDEX_USER,
};
use crate::biz::casbin::metrics::AccessControlMetrics;
use crate::biz::casbin::request::{PolicyRequest, WorkspacePolicyRequest};
use anyhow::anyhow;
use app_error::AppError;

use casbin::{CoreApi, Enforcer, MgmtApi};

use std::ops::Deref;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{event, instrument, trace};

pub const ENFORCER_METRICS_TICK_INTERVAL: Duration = Duration::from_secs(30);

pub struct AFEnforcer {
  enforcer: RwLock<Enforcer>,
  metrics_cal: MetricsCal,
}

impl AFEnforcer {
  pub fn new(enforcer: Enforcer, metrics: Arc<AccessControlMetrics>) -> Self {
    let metrics_cal = MetricsCal::new();
    let cloned_metrics_cal = metrics_cal.clone();

    tokio::spawn(async move {
      let mut interval = interval(ENFORCER_METRICS_TICK_INTERVAL);
      loop {
        interval.tick().await;

        metrics.record_enforce_count(
          cloned_metrics_cal
            .total_read_enforce_result
            .load(Ordering::Relaxed),
          cloned_metrics_cal
            .read_enforce_result_from_cache
            .load(Ordering::Relaxed),
        );
      }
    });

    Self {
      enforcer: RwLock::new(enforcer),
      metrics_cal,
    }
  }

  /// Update policy for a user.
  /// If the policy is already exist, then it will return Ok(false).
  ///
  /// [`ObjectType::Workspace`] has to be paired with [`ActionType::Role`],
  /// [`ObjectType::Collab`] has to be paired with [`ActionType::Level`],
  #[instrument(level = "debug", skip_all, err)]
  pub async fn update_policy(
    &self,
    uid: &i64,
    obj: &ObjectType<'_>,
    act: &ActionType,
  ) -> Result<(), AppError> {
    validate_obj_action(obj, act)?;
    let policy = vec![
      uid.to_string(),
      obj.policy_object(),
      act.to_action().to_string(),
    ];
    let policy_key = PolicyCacheKey::new(&policy);
    // only one policy per user per object. So remove the old policy and add the new one.
    let mut write_guard = self.enforcer.write().await;
    let result = write_guard
      .add_policy(policy)
      .await
      .map_err(|e| AppError::Internal(anyhow!("fail to add policy: {e:?}")))?;
    trace!(
      "[access control]: add policy:{} => {:?}",
      policy_key.0,
      result
    );
    drop(write_guard);
    Ok(())
  }

  /// Returns policies that match the filter.
  pub async fn remove_policy(
    &self,
    uid: &i64,
    object_type: &ObjectType<'_>,
  ) -> Result<(), AppError> {
    let mut enforcer = self.enforcer.write().await;
    self
      .remove_with_enforcer(uid, object_type, &mut enforcer)
      .await
  }

  #[instrument(level = "debug", skip_all)]
  pub async fn enforce_policy(
    &self,
    workspace_id: &str,
    uid: &i64,
    obj: ObjectType<'_>,
    act: ActionVariant<'_>,
  ) -> Result<bool, AppError> {
    self
      .metrics_cal
      .total_read_enforce_result
      .fetch_add(1, Ordering::Relaxed);

    // check workspace policy first
    let workspace_policy_request = WorkspacePolicyRequest::new(workspace_id, uid, &act);
    let segments = workspace_policy_request.into_segments();
    let mut result = self
      .enforcer
      .read()
      .await
      .enforce(segments)
      .map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;

    if !result {
      // if the result is false, then try to use group policy request
    }

    // if the result is false, then try to check the collab object policy
    if !result {
      let policy_request = PolicyRequest::new(*uid, &obj, &act);
      let segments = policy_request.into_segments();
      result = self
        .enforcer
        .read()
        .await
        .enforce(segments)
        .map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;
    }

    Ok(result)
  }

  #[inline]
  async fn remove_with_enforcer(
    &self,
    uid: &i64,
    object_type: &ObjectType<'_>,
    enforcer: &mut Enforcer,
  ) -> Result<(), AppError> {
    let policies_for_user_on_object =
      policies_for_user_with_given_object(uid, object_type, enforcer).await;

    // if there are no policies for the user on the object, return early.
    if policies_for_user_on_object.is_empty() {
      return Ok(());
    }

    event!(
      tracing::Level::INFO,
      "[access control]: remove policy: object={}, user={}, policies={:?}",
      object_type.policy_object(),
      uid,
      policies_for_user_on_object
    );

    enforcer
      .remove_policies(policies_for_user_on_object)
      .await
      .map_err(|e| AppError::Internal(anyhow!("error enforce: {e:?}")))?;

    Ok(())
  }
}

#[derive(Debug, Hash, Eq, PartialEq)]
pub struct PolicyCacheKey(String);

impl PolicyCacheKey {
  fn new(policy: &[String]) -> Self {
    Self(policy.join(":"))
  }

  pub fn into_inner(self) -> String {
    self.0
  }
}

impl Deref for PolicyCacheKey {
  type Target = str;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl AsRef<str> for PolicyCacheKey {
  fn as_ref(&self) -> &str {
    &self.0
  }
}

fn validate_obj_action(obj: &ObjectType<'_>, act: &ActionType) -> Result<(), AppError> {
  match (obj, act) {
    (ObjectType::Workspace(_), ActionType::Role(_))
    | (ObjectType::Collab(_), ActionType::Level(_)) => Ok(()),
    _ => Err(AppError::Internal(anyhow!(
      "invalid object type and action type combination: object={:?}, action={:?}",
      obj,
      act
    ))),
  }
}
#[inline]
async fn policies_for_user_with_given_object(
  uid: &i64,
  object_type: &ObjectType<'_>,
  enforcer: &Enforcer,
) -> Vec<Vec<String>> {
  let object_type_id = object_type.policy_object();
  let policies_related_to_object =
    enforcer.get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![object_type_id]);

  policies_related_to_object
    .into_iter()
    .filter(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string())
    .collect::<Vec<_>>()
}

#[derive(Clone)]
struct MetricsCal {
  total_read_enforce_result: Arc<AtomicI64>,
  read_enforce_result_from_cache: Arc<AtomicI64>,
}

impl MetricsCal {
  fn new() -> Self {
    Self {
      total_read_enforce_result: Arc::new(Default::default()),
      read_enforce_result_from_cache: Arc::new(Default::default()),
    }
  }
}
