use crate::biz::casbin::access_control::{
  ActionType, ObjectType, ToCasbinAction, POLICY_FIELD_INDEX_ACTION, POLICY_FIELD_INDEX_OBJECT,
  POLICY_FIELD_INDEX_USER,
};
use anyhow::anyhow;
use app_error::AppError;
use casbin::{CoreApi, Enforcer, MgmtApi};
use dashmap::DashMap;

use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{event, trace};

pub struct AFEnforcer {
  enforcer: RwLock<Enforcer>,
  /// Cache for the result of the policy check. It's a memory cache for faster access.
  enforcer_result_cache: Arc<DashMap<PolicyCacheKey, bool>>,
  action_cache: Arc<DashMap<ActionCacheKey, String>>,
}

impl AFEnforcer {
  pub fn new(
    enforcer: Enforcer,
    enforcer_result_cache: Arc<DashMap<PolicyCacheKey, bool>>,
    action_cache: Arc<DashMap<ActionCacheKey, String>>,
  ) -> Self {
    Self {
      enforcer: RwLock::new(enforcer),
      enforcer_result_cache,
      action_cache,
    }
  }
  pub async fn contains(&self, obj: &ObjectType<'_>) -> bool {
    self
      .enforcer
      .read()
      .await
      .get_all_objects()
      .contains(&obj.to_object_id())
  }

  pub async fn policies_for_user_with_given_object(
    &self,
    uid: &i64,
    object_type: &ObjectType<'_>,
  ) -> Vec<Vec<String>> {
    let object_type_id = object_type.to_object_id();
    let policies_related_to_object = self
      .enforcer
      .read()
      .await
      .get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![object_type_id]);

    policies_related_to_object
      .into_iter()
      .filter(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string())
      .collect::<Vec<_>>()
  }

  /// Update permission for a user.
  ///
  /// [`ObjectType::Workspace`] has to be paired with [`ActionType::Role`],
  /// [`ObjectType::Collab`] has to be paired with [`ActionType::Level`],
  pub async fn update(
    &self,
    uid: &i64,
    obj: &ObjectType<'_>,
    act: &ActionType,
  ) -> Result<bool, AppError> {
    validate_obj_action(obj, act)?;
    let policy = vec![uid.to_string(), obj.to_object_id(), act.to_action()];
    let policy_key = PolicyCacheKey::new(&policy);

    // if the policy is already in the cache, return. Only update the policy if it's not in the cache.
    if let Some(value) = self.enforcer_result_cache.get(&policy_key) {
      return Ok(*value);
    }

    event!(tracing::Level::INFO, "updating policy: {:?}", policy);
    // only one policy per user per object. So remove the old policy and add the new one.
    let _remove_policies = self.remove(uid, obj).await?;
    let object_key = ActionCacheKey::new(uid, obj);
    let result = self
      .enforcer
      .write()
      .await
      .add_policy(policy)
      .await
      .map_err(|e| AppError::Internal(anyhow!("fail to add policy: {e:?}")));
    if result.is_ok() {
      trace!("cache action: {}:{}", object_key.0, act.to_action());
      self.action_cache.insert(object_key, act.to_action());
    }
    result
  }

  /// Returns policies that match the filter.
  pub async fn remove(
    &self,
    uid: &i64,
    object_type: &ObjectType<'_>,
  ) -> Result<Vec<Vec<String>>, AppError> {
    let policies_for_user_on_object = self
      .policies_for_user_with_given_object(uid, object_type)
      .await;

    // if there are no policies for the user on the object, return early.
    if policies_for_user_on_object.is_empty() {
      return Ok(vec![]);
    }

    event!(
      tracing::Level::INFO,
      "removing policy: object={}, user={}, policies={:?}",
      object_type.to_object_id(),
      uid,
      policies_for_user_on_object
    );
    debug_assert!(
      policies_for_user_on_object.len() == 1,
      "only one policy per user per object"
    );
    self
      .enforcer
      .write()
      .await
      .remove_policies(policies_for_user_on_object.clone())
      .await
      .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))?;

    let object_key = ActionCacheKey::new(uid, object_type);
    self.action_cache.remove(&object_key);
    for policy in &policies_for_user_on_object {
      self
        .enforcer_result_cache
        .remove(&PolicyCacheKey::new(policy));
    }

    Ok(policies_for_user_on_object)
  }

  pub async fn enforce<A>(&self, uid: &i64, obj: &ObjectType<'_>, act: A) -> Result<bool, AppError>
  where
    A: ToCasbinAction,
  {
    let policy = vec![uid.to_string(), obj.to_object_id(), act.to_action()];
    let policy_key = PolicyCacheKey::new(&policy);
    if let Some(value) = self.enforcer_result_cache.get(&policy_key) {
      return Ok(*value);
    }

    let policies_for_object = self
      .enforcer
      .read()
      .await
      .get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![obj.to_object_id()]);

    if policies_for_object.is_empty() {
      return Ok(true);
    }

    let result = self
      .enforcer
      .read()
      .await
      .enforce(policy)
      .map_err(|e| AppError::Internal(anyhow!("error enforce: {e:?}")))?;
    self.enforcer_result_cache.insert(policy_key, result);
    Ok(result)
  }

  pub async fn get_action(&self, uid: &i64, object_type: &ObjectType<'_>) -> Option<String> {
    let object_key = ActionCacheKey::new(uid, object_type);
    if let Some(value) = self.action_cache.get(&object_key) {
      return Some(value.clone());
    }

    // There should only be one entry per user per object, which is enforced in [AccessControl], so just take one using next.
    let policies = self
      .policies_for_user_with_given_object(uid, object_type)
      .await;
    let action = policies.first()?[POLICY_FIELD_INDEX_ACTION].clone();

    trace!("cache action: {}:{}", object_key.0, action.clone());
    self.action_cache.insert(object_key, action.clone());
    Some(action)
  }
}

#[derive(Debug, Hash, Eq, PartialEq)]
pub struct PolicyCacheKey(String);

impl PolicyCacheKey {
  fn new(policy: &[String]) -> Self {
    Self(policy.join(":"))
  }
}

impl Deref for PolicyCacheKey {
  type Target = str;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

#[derive(Debug, Hash, Eq, PartialEq)]
pub struct ActionCacheKey(String);

impl ActionCacheKey {
  pub(crate) fn new(uid: &i64, object_type: &ObjectType<'_>) -> Self {
    Self(format!("{}:{}", uid, object_type.to_object_id()))
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
