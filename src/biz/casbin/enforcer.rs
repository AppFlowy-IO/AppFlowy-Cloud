use crate::biz::casbin::access_control::{
  ActionType, ObjectType, ToCasbinAction, POLICY_FIELD_INDEX_ACTION, POLICY_FIELD_INDEX_OBJECT,
  POLICY_FIELD_INDEX_USER,
};
use anyhow::anyhow;
use app_error::AppError;
use casbin::{CoreApi, Enforcer, MgmtApi};
use dashmap::DashMap;

use std::ops::{Deref, DerefMut};
use tracing::{event, trace};

pub struct AFEnforcer {
  enforcer: Enforcer,
  /// Cache for the result of the policy check. It's a memory cache for faster access.
  result_by_policy_cache: DashMap<CachePolicyKey, bool>,
  action_by_object_cache: DashMap<CacheObjectKey, String>,
}

impl Deref for AFEnforcer {
  type Target = Enforcer;
  fn deref(&self) -> &Self::Target {
    &self.enforcer
  }
}
impl DerefMut for AFEnforcer {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.enforcer
  }
}

impl AFEnforcer {
  pub fn new(enforcer: Enforcer) -> Self {
    Self {
      enforcer,
      result_by_policy_cache: DashMap::new(),
      action_by_object_cache: Default::default(),
    }
  }
  pub async fn contains(&self, obj: &ObjectType<'_>) -> bool {
    self
      .enforcer
      .get_all_objects()
      .contains(&obj.to_object_id())
  }

  /// Update permission for a user.
  ///
  /// [`ObjectType::Workspace`] has to be paired with [`ActionType::Role`],
  /// [`ObjectType::Collab`] has to be paired with [`ActionType::Level`],
  pub async fn update(
    &mut self,
    uid: &i64,
    obj: &ObjectType<'_>,
    act: &ActionType,
  ) -> Result<bool, AppError> {
    validate_obj_action(obj, act)?;
    let policy = vec![uid.to_string(), obj.to_object_id(), act.to_action()];
    let policy_key = CachePolicyKey::new(&policy);

    // if the policy is already in the cache, return. Only update the policy if it's not in the cache.
    if let Some(value) = self.result_by_policy_cache.get(&policy_key) {
      return Ok(*value);
    }

    event!(tracing::Level::INFO, "updating policy: {:?}", policy);
    // only one policy per user per object. So remove the old policy and add the new one.
    let _remove_policies = self.remove(uid, obj).await?;
    let object_key = CacheObjectKey::new(uid, obj);
    let result = self
      .add_policy(policy)
      .await
      .map_err(|e| AppError::Internal(anyhow!("fail to add policy: {e:?}")));
    if result.is_ok() {
      trace!("cache action: {}:{}", object_key.0, act.to_action());
      self
        .action_by_object_cache
        .insert(object_key, act.to_action());
    }
    result
  }

  /// Returns policies that match the filter.
  pub async fn remove(
    &mut self,
    uid: &i64,
    object_type: &ObjectType<'_>,
    // TODO(nathan): replace with SmallVec
  ) -> Result<Vec<Vec<String>>, AppError> {
    let object_type_id = object_type.to_object_id();
    let policies_related_to_object =
      self.get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![object_type_id]);

    let policies_for_user_on_object = policies_related_to_object
      .into_iter()
      .filter(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string())
      .collect::<Vec<_>>();

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
      .remove_policies(policies_for_user_on_object.clone())
      .await
      .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))?;

    let object_key = CacheObjectKey::new(uid, object_type);
    self.action_by_object_cache.remove(&object_key);
    for policy in &policies_for_user_on_object {
      self
        .result_by_policy_cache
        .remove(&CachePolicyKey::new(policy));
    }

    Ok(policies_for_user_on_object)
  }

  pub async fn enforce<A>(&self, uid: &i64, obj: &ObjectType<'_>, act: A) -> Result<bool, AppError>
  where
    A: ToCasbinAction,
  {
    let policy = vec![uid.to_string(), obj.to_object_id(), act.to_action()];
    let policy_key = CachePolicyKey::new(&policy);
    if let Some(value) = self.result_by_policy_cache.get(&policy_key) {
      return Ok(*value);
    }

    let result = self
      .enforcer
      .enforce(policy)
      .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))?;

    self.result_by_policy_cache.insert(policy_key, result);
    Ok(result)
  }

  pub async fn get_action(&self, uid: &i64, object_type: &ObjectType<'_>) -> Option<String> {
    let object_key = CacheObjectKey::new(uid, object_type);
    if let Some(value) = self.action_by_object_cache.get(&object_key) {
      return Some(value.clone());
    }

    let policies = self
      .enforcer
      .get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![object_type.to_object_id()]);

    // There should only be one entry per user per object, which is enforced in [AccessControl], so just take one using next.
    let values = policies
      .into_iter()
      .find(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string())?;
    let action = values[POLICY_FIELD_INDEX_ACTION].clone();

    trace!("cache action: {}:{}", object_key.0, action.clone());
    self
      .action_by_object_cache
      .insert(object_key, action.clone());
    Some(action)
  }
}

#[derive(Debug, Hash, Eq, PartialEq)]
struct CachePolicyKey(String);

impl CachePolicyKey {
  fn new(policy: &[String]) -> Self {
    Self(policy.join(":"))
  }
}

impl Deref for CachePolicyKey {
  type Target = str;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

#[derive(Debug, Hash, Eq, PartialEq)]
struct CacheObjectKey(String);

impl CacheObjectKey {
  fn new(uid: &i64, object_type: &ObjectType<'_>) -> Self {
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
