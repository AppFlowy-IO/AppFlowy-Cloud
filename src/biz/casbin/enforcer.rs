use crate::biz::casbin::access_control::{
  ActionType, ObjectType, ToCasbinAction, POLICY_FIELD_INDEX_ACTION, POLICY_FIELD_INDEX_OBJECT,
  POLICY_FIELD_INDEX_USER,
};
use anyhow::anyhow;
use app_error::AppError;
use casbin::{CoreApi, Enforcer, MgmtApi};

use async_trait::async_trait;
use std::ops::Deref;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::biz::casbin::metrics::AccessControlMetrics;

use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{error, event, trace};

#[async_trait]
pub trait AFEnforcerCache: Send + Sync {
  async fn set_enforcer_result(&self, key: &PolicyCacheKey, value: bool) -> Result<(), AppError>;
  async fn get_enforcer_result(&self, key: &PolicyCacheKey) -> Option<bool>;
  async fn remove_enforcer_result(&self, key: &PolicyCacheKey);
  async fn set_action(&self, key: &ActionCacheKey, value: String) -> Result<(), AppError>;
  async fn get_action(&self, key: &ActionCacheKey) -> Option<String>;
  async fn remove_action(&self, key: &ActionCacheKey);
}

pub const ENFORCER_METRICS_TICK_INTERVAL: Duration = Duration::from_secs(30);

pub struct AFEnforcer {
  enforcer: RwLock<Enforcer>,
  cache: Arc<dyn AFEnforcerCache>,
  metrics_cal: MetricsCal,
}

impl AFEnforcer {
  pub fn new(
    enforcer: Enforcer,
    cache: Arc<dyn AFEnforcerCache>,
    metrics: Arc<AccessControlMetrics>,
  ) -> Self {
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
      cache,
      metrics_cal,
    }
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
    if let Some(value) = self.cache.get_enforcer_result(&policy_key).await {
      return Ok(value);
    }

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

    match &result {
      Ok(value) => {
        trace!("[access control]: add policy:{} => {}", policy_key.0, value);
        if let Err(err) = self.cache.set_action(&object_key, act.to_action()).await {
          error!("{}", err);
        }
      },
      Err(err) => {
        trace!(
          "[access control]: fail to add policy:{} => {:?}",
          policy_key.0,
          err
        );
      },
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
      "[access control]: remove policy: object={}, user={}, policies={:?}",
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
      .map_err(|e| AppError::Internal(anyhow!("error enforce: {e:?}")))?;

    let object_key = ActionCacheKey::new(uid, object_type);
    self.cache.remove_action(&object_key).await;
    for policy in &policies_for_user_on_object {
      self
        .cache
        .remove_enforcer_result(&PolicyCacheKey::new(policy))
        .await;
    }

    Ok(policies_for_user_on_object)
  }

  pub async fn enforce<A>(&self, uid: &i64, obj: &ObjectType<'_>, act: A) -> Result<bool, AppError>
  where
    A: ToCasbinAction,
  {
    self
      .metrics_cal
      .total_read_enforce_result
      .fetch_add(1, Ordering::Relaxed);
    let policy = vec![uid.to_string(), obj.to_object_id(), act.to_action()];
    let policy_key = PolicyCacheKey::new(&policy);
    if let Some(value) = self.cache.get_enforcer_result(&policy_key).await {
      self
        .metrics_cal
        .read_enforce_result_from_cache
        .fetch_add(1, Ordering::Relaxed);
      return Ok(value);
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

    trace!("[access control]: policy:{} => {}", policy_key.0, result);
    if let Err(err) = self.cache.set_enforcer_result(&policy_key, result).await {
      error!("{}", err)
    }
    Ok(result)
  }

  pub async fn get_action(&self, uid: &i64, object_type: &ObjectType<'_>) -> Option<String> {
    let object_key = ActionCacheKey::new(uid, object_type);
    if let Some(value) = self.cache.get_action(&object_key).await {
      return Some(value.clone());
    }

    // There should only be one entry per user per object, which is enforced in [AccessControl], so just take one using next.
    let policies = self
      .policies_for_user_with_given_object(uid, object_type)
      .await;

    let action = policies.first()?[POLICY_FIELD_INDEX_ACTION].clone();
    trace!("cache action: {}:{}", object_key.0, action.clone());
    let _ = self.cache.set_action(&object_key, action.clone()).await;
    Some(action)
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

#[derive(Debug, Hash, Eq, PartialEq)]
pub struct ActionCacheKey(String);

impl AsRef<str> for ActionCacheKey {
  fn as_ref(&self) -> &str {
    &self.0
  }
}

impl Deref for ActionCacheKey {
  type Target = str;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

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
