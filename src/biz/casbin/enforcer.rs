use crate::biz::casbin::access_control::{
  ActionType, ObjectType, ToACAction, POLICY_FIELD_INDEX_OBJECT, POLICY_FIELD_INDEX_USER,
};
use anyhow::anyhow;
use app_error::AppError;
use casbin::{CoreApi, Enforcer, MgmtApi};

use async_trait::async_trait;
use std::ops::Deref;

use std::sync::Arc;

use crate::biz::casbin::metrics::AccessControlMetrics;

use tokio::sync::RwLock;

use tracing::{event, instrument, trace};

#[async_trait]
pub trait AFEnforcerCache: Send + Sync {
  async fn set_enforcer_result(
    &mut self,
    key: &PolicyCacheKey,
    value: bool,
  ) -> Result<(), AppError>;
  async fn get_enforcer_result(&mut self, key: &PolicyCacheKey) -> Option<bool>;
  async fn remove_enforcer_result(&mut self, key: &PolicyCacheKey);
}

pub struct AFEnforcer {
  // TODO(nathan): read is more than write. considering using SharedLock
  // https://docs.rs/crossbeam/latest/crossbeam/sync/struct.ShardedLock.html
  enforcer: RwLock<Enforcer>,
}

impl AFEnforcer {
  pub fn new(enforcer: Enforcer, _metrics: Arc<AccessControlMetrics>) -> Self {
    Self {
      enforcer: RwLock::new(enforcer),
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
  pub async fn enforce_policy<A>(
    &self,
    uid: &i64,
    obj: &ObjectType<'_>,
    act: A,
  ) -> Result<bool, AppError>
  where
    A: ToACAction,
  {
    // create policy request
    let policy_request = vec![
      uid.to_string(),
      obj.policy_object(),
      act.to_action().to_string(),
    ];

    let policy_key = PolicyCacheKey::new(&policy_request);
    // Perform the action and capture the result or error
    let action_result = self.enforcer.read().await.enforce(policy_request);
    match &action_result {
      Ok(result) => trace!(
        "[access control]: enforce policy:{} with result:{}",
        policy_key.0,
        result
      ),
      Err(e) => trace!(
        "[access control]: enforce policy:{} with error: {:?}",
        policy_key.0,
        e
      ),
    }

    // Convert the action result into the original method's result type, handling errors as before
    let result = action_result.map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;
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
