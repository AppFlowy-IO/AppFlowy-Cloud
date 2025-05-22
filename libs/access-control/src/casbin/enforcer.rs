use super::access::{POLICY_FIELD_INDEX_OBJECT, POLICY_FIELD_INDEX_SUBJECT, load_group_policies};
use crate::act::Acts;
use crate::entity::{ObjectType, SubjectType};
use crate::metrics::MetricsCalState;
use crate::request::PolicyRequest;
use anyhow::anyhow;
use app_error::AppError;
use casbin::{CoreApi, Enforcer, MgmtApi};
use std::sync::atomic::Ordering;
use tokio::sync::RwLock;
use tracing::{event, instrument, trace};

pub struct AFEnforcer {
  enforcer: RwLock<Enforcer>,
  pub(crate) metrics_state: MetricsCalState,
}

impl AFEnforcer {
  pub async fn new(mut enforcer: Enforcer) -> Result<Self, AppError> {
    load_group_policies(&mut enforcer).await?;
    Ok(Self {
      enforcer: RwLock::new(enforcer),
      metrics_state: MetricsCalState::new(),
    })
  }

  /// Update policy for a user.
  /// If the policy is already exist, then it will return Ok(false).
  ///
  /// [`ObjectType::Workspace`] has to be paired with [`ActionType::Role`],
  /// [`ObjectType::Collab`] has to be paired with [`ActionType::Level`],
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

    trace!("[access control]: add policy:{:?}", policies);
    self
      .enforcer
      .write()
      .await
      .add_policies(policies)
      .await
      .map_err(|e| AppError::Internal(anyhow!("fail to add policy: {e:?}")))?;

    Ok(())
  }

  /// Returns policies that match the filter.
  pub async fn remove_policy(
    &self,
    sub: SubjectType,
    object_type: ObjectType,
  ) -> Result<(), AppError> {
    let mut enforcer = self.enforcer.write().await;
    self
      .remove_with_enforcer(sub, object_type, &mut enforcer)
      .await
  }

  /// ## Parameters:
  /// - `uid`: The user ID of the user attempting the action.
  /// - `obj`: The type of object being accessed, encapsulated within an `ObjectType`.
  /// - `act`: The action being attempted, encapsulated within an `ActionVariant`.
  ///
  /// ## Returns:
  /// - `Ok(true)`: If the user is authorized to perform the action based on any of the evaluated policies.
  /// - `Ok(false)`: If none of the policies authorize the user to perform the action.
  /// - `Err(AppError)`: If an error occurs during policy enforcement.
  ///
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

  #[inline]
  async fn remove_with_enforcer(
    &self,
    sub: SubjectType,
    object_type: ObjectType,
    enforcer: &mut Enforcer,
  ) -> Result<(), AppError> {
    let policies_for_user_on_object =
      policies_for_subject_with_given_object(sub.clone(), object_type.clone(), enforcer).await;

    event!(
      tracing::Level::INFO,
      "[access control]: remove policy:subject={}, object={}, policies={:?}",
      sub.policy_subject(),
      object_type.policy_object(),
      policies_for_user_on_object
    );

    enforcer
      .remove_policies(policies_for_user_on_object)
      .await
      .map_err(|e| AppError::Internal(anyhow!("error enforce: {e:?}")))?;

    Ok(())
  }
}

#[inline]
async fn policies_for_subject_with_given_object(
  subject: SubjectType,
  object_type: ObjectType,
  enforcer: &Enforcer,
) -> Vec<Vec<String>> {
  let subject_id = subject.policy_subject();
  let object_type_id = object_type.policy_object();
  let policies_related_to_object =
    enforcer.get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![object_type_id]);

  policies_related_to_object
    .into_iter()
    .filter(|p| p[POLICY_FIELD_INDEX_SUBJECT] == subject_id)
    .collect::<Vec<_>>()
}

#[cfg(test)]
pub(crate) mod tests {
  use crate::{
    act::Action,
    casbin::access::{casbin_model, cmp_role_or_level},
    entity::{ObjectType, SubjectType},
  };
  use casbin::{function_map::OperatorFunction, prelude::*};
  use database_entity::dto::{AFAccessLevel, AFRole};

  use super::AFEnforcer;

  pub async fn test_enforcer() -> AFEnforcer {
    let model = casbin_model().await.unwrap();
    let mut enforcer = casbin::Enforcer::new(model, MemoryAdapter::default())
      .await
      .unwrap();

    enforcer.add_function("cmpRoleOrLevel", OperatorFunction::Arg2(cmp_role_or_level));
    AFEnforcer::new(enforcer).await.unwrap()
  }

  #[tokio::test]
  async fn policy_comparison_test() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Member,
      )
      .await
      .expect("update policy failed");

    // test if enforce can compare requested action and the role policy
    for action in [Action::Write, Action::Read] {
      let result = enforcer
        .enforce_policy(
          &uid,
          ObjectType::Workspace(workspace_id.to_string()),
          action.clone(),
        )
        .await
        .unwrap_or_else(|_| panic!("enforcing action={:?} failed", action));
      assert!(result, "action={:?} should be allowed", action);
    }
    let result = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        Action::Delete,
      )
      .await
      .expect("enforcing action=Delete failed");
    assert!(!result, "action=Delete should not be allowed");

    let result = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Member,
      )
      .await
      .expect("enforcing role=Member failed");
    assert!(result, "role=Member should be allowed");

    let result = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Owner,
      )
      .await
      .expect("enforcing role=Owner failed");
    assert!(!result, "role=Owner should not be allowed");

    for access_level in [
      AFAccessLevel::ReadOnly,
      AFAccessLevel::ReadAndComment,
      AFAccessLevel::ReadAndWrite,
    ] {
      let result = enforcer
        .enforce_policy(
          &uid,
          ObjectType::Workspace(workspace_id.to_string()),
          access_level,
        )
        .await
        .unwrap_or_else(|_| panic!("enforcing access_level={:?} failed", access_level));
      assert!(result, "access_level={:?} should be allowed", access_level);
    }
    let result = enforcer
      .enforce_policy(
        &uid,
        ObjectType::Workspace(workspace_id.to_string()),
        AFAccessLevel::FullAccess,
      )
      .await
      .expect("enforcing access_level=FullAccess failed");
    assert!(!result, "access_level=FullAccess should not be allowed")
  }
}
