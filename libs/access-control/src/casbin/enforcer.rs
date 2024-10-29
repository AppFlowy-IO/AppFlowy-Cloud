use super::access::{load_group_policies, POLICY_FIELD_INDEX_OBJECT, POLICY_FIELD_INDEX_SUBJECT};
use crate::act::ActionVariant;
use crate::entity::{ObjectType, SubjectType};
use crate::metrics::MetricsCalState;
use crate::request::{PolicyRequest, WorkspacePolicyRequest};
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
  pub async fn update_policy(
    &self,
    sub: SubjectType,
    obj: ObjectType<'_>,
    act: ActionVariant<'_>,
  ) -> Result<(), AppError> {
    validate_obj_action(&obj, &act)?;

    let policies = act
      .policy_acts()
      .into_iter()
      .map(|act| vec![sub.policy_subject(), obj.policy_object(), act.to_string()])
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
    sub: &SubjectType,
    object_type: &ObjectType<'_>,
  ) -> Result<(), AppError> {
    let mut enforcer = self.enforcer.write().await;
    self
      .remove_with_enforcer(sub, object_type, &mut enforcer)
      .await
  }

  /// Add a grouping policy.
  #[allow(dead_code)]
  pub async fn add_grouping_policy(
    &self,
    sub: &SubjectType,
    group_sub: &SubjectType,
  ) -> Result<(), AppError> {
    let mut enforcer = self.enforcer.write().await;
    enforcer
      .add_grouping_policy(vec![sub.policy_subject(), group_sub.policy_subject()])
      .await
      .map_err(|e| AppError::Internal(anyhow!("fail to add grouping policy: {e:?}")))?;
    Ok(())
  }

  /// 1. **Workspace Policy**: Initially, it checks if the user has permission at the workspace level. If the user
  ///    has permission to perform the action on the workspace, the function returns `true` without further checks.
  ///
  /// 2. **Object-Specific Policy**: If workspace policy check fail, the function evaluates the policy
  ///    specific to the object itself.
  ///
  /// ## Parameters:
  /// - `workspace_id`: The identifier for the workspace containing the object.
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
  pub async fn enforce_policy(
    &self,
    workspace_id: &str,
    uid: &i64,
    obj: ObjectType<'_>,
    act: ActionVariant<'_>,
  ) -> Result<(), AppError> {
    self
      .metrics_state
      .total_read_enforce_result
      .fetch_add(1, Ordering::Relaxed);

    // 1. First, check workspace-level permissions.
    let workspace_policy_request = WorkspacePolicyRequest::new(workspace_id, uid, &obj, &act);
    let policy = workspace_policy_request.to_policy();
    let mut result = self
      .enforcer
      .read()
      .await
      .enforce(policy)
      .map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;

    // 2. Finally, enforce object-specific policy if previous checks fail.
    if !result {
      let policy_request = PolicyRequest::new(*uid, &obj, &act);
      let policy = policy_request.to_policy();
      result = self
        .enforcer
        .read()
        .await
        .enforce(policy)
        .map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;
    }

    if result {
      Ok(())
    } else {
      Err(AppError::NotEnoughPermissions)
    }
  }

  #[inline]
  async fn remove_with_enforcer(
    &self,
    sub: &SubjectType,
    object_type: &ObjectType<'_>,
    enforcer: &mut Enforcer,
  ) -> Result<(), AppError> {
    let policies_for_user_on_object =
      policies_for_subject_with_given_object(sub, object_type, enforcer).await;

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

fn validate_obj_action(obj: &ObjectType<'_>, act: &ActionVariant) -> Result<(), AppError> {
  match (obj, act) {
    (ObjectType::Workspace(_), ActionVariant::FromRole(_))
    | (ObjectType::Collab(_), ActionVariant::FromAccessLevel(_)) => Ok(()),
    _ => Err(AppError::Internal(anyhow!(
      "invalid object type and action type combination: object={:?}, action={:?}",
      obj,
      act.to_enforce_act()
    ))),
  }
}
#[inline]
async fn policies_for_subject_with_given_object(
  subject: &SubjectType,
  object_type: &ObjectType<'_>,
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
mod tests {
  use crate::{
    act::{Action, ActionVariant},
    casbin::access::{casbin_model, cmp_role_or_level},
    entity::{ObjectType, SubjectType},
  };
  use app_error::ErrorCode;
  use casbin::{function_map::OperatorFunction, prelude::*};
  use database_entity::dto::{AFAccessLevel, AFRole};

  use super::AFEnforcer;

  async fn test_enforcer() -> AFEnforcer {
    let model = casbin_model().await.unwrap();
    let mut enforcer = casbin::Enforcer::new(model, MemoryAdapter::default())
      .await
      .unwrap();

    enforcer.add_function("cmpRoleOrLevel", OperatorFunction::Arg2(cmp_role_or_level));
    AFEnforcer::new(enforcer).await.unwrap()
  }

  #[tokio::test]
  async fn collab_group_test() {
    let enforcer = test_enforcer().await;

    let uid = 1;
    let group_id = "collab_owner_group:w1";
    let workspace_id = "w1";
    let object_1 = "o1";

    // allow workspace member to access collab
    enforcer
      .update_policy(
        SubjectType::Group(group_id.to_string()),
        ObjectType::Collab(object_1),
        ActionVariant::FromAccessLevel(&AFAccessLevel::FullAccess),
      )
      .await
      .unwrap();

    // include user in the collab owner group
    enforcer
      .add_grouping_policy(
        &SubjectType::User(uid),
        &SubjectType::Group(group_id.to_string()),
      )
      .await
      .unwrap();

    // when the user is the owner of the collab, then the user should have access to the collab
    for action in [Action::Write, Action::Read] {
      let result = enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromAction(&action),
        )
        .await;
      assert!(result.is_ok());
    }
  }

  #[tokio::test]
  async fn workspace_group_policy_test() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Member),
      )
      .await
      .unwrap();

    // test the user has permission to write and read the workspace
    for action in [Action::Write, Action::Read] {
      let result = enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Workspace(workspace_id),
          ActionVariant::FromAction(&action),
        )
        .await;
      assert!(result.is_ok(), "action={:?}", action);
    }
  }

  #[tokio::test]
  async fn workspace_owner_and_try_to_full_access_collab_test() {
    let enforcer = test_enforcer().await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Owner),
      )
      .await
      .unwrap();

    for action in [Action::Write, Action::Read, Action::Delete] {
      let result = enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromAction(&action),
        )
        .await;
      assert!(result.is_ok(), "action={:?}", action);
    }
  }

  #[tokio::test]
  async fn workspace_member_collab_owner_try_to_full_access_collab_test() {
    let enforcer = test_enforcer().await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Member),
      )
      .await
      .unwrap();

    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Collab(object_1),
        ActionVariant::FromAccessLevel(&AFAccessLevel::FullAccess),
      )
      .await
      .unwrap();

    for action in [Action::Write, Action::Read, Action::Delete] {
      let result = enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromAction(&action),
        )
        .await;
      assert!(result.is_ok(), "action={:?}", action);
    }
  }

  #[tokio::test]
  async fn workspace_owner_collab_member_try_to_full_access_collab_test() {
    let enforcer = test_enforcer().await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Owner),
      )
      .await
      .unwrap();

    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Collab(object_1),
        ActionVariant::FromAccessLevel(&AFAccessLevel::ReadAndWrite),
      )
      .await
      .unwrap();

    for action in [Action::Write, Action::Read, Action::Delete] {
      let result = enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromAction(&action),
        )
        .await;
      assert!(result.is_ok(), "action={:?}", action);
    }
  }

  #[tokio::test]
  async fn workspace_member_collab_member_try_to_full_access_collab_test() {
    let enforcer = test_enforcer().await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Member),
      )
      .await
      .unwrap();

    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Collab(object_1),
        ActionVariant::FromAccessLevel(&AFAccessLevel::ReadAndWrite),
      )
      .await
      .unwrap();

    for action in [Action::Write, Action::Read] {
      let result = enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromAction(&action),
        )
        .await;
      assert!(result.is_ok(), "action={:?}", action);
    }

    let result = enforcer
      .enforce_policy(
        workspace_id,
        &uid,
        ObjectType::Collab(object_1),
        ActionVariant::FromAction(&Action::Delete),
      )
      .await;
    assert!(result.is_err(), "only the owner can perform delete");
    let error_code = result.unwrap_err().code();
    assert_eq!(error_code, ErrorCode::NotEnoughPermissions);
  }

  #[tokio::test]
  async fn workspace_member_but_not_collab_member_and_try_full_access_collab_test() {
    let enforcer = test_enforcer().await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Member),
      )
      .await
      .unwrap();

    // Although the user is not directly associated with the collab object, they are a member of the
    // workspace containing it. Therefore, the system will evaluate their permissions based on the
    // workspace policy as a fallback.
    for action in [Action::Write, Action::Read] {
      let result = enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromAction(&action),
        )
        .await;
      assert!(result.is_ok(), "action={:?}", action);
    }

    let result = enforcer
      .enforce_policy(
        workspace_id,
        &uid,
        ObjectType::Collab(object_1),
        ActionVariant::FromAction(&Action::Delete),
      )
      .await;
    assert!(result.is_err(), "only the owner can perform delete");
    let error_code = result.unwrap_err().code();
    assert_eq!(error_code, ErrorCode::NotEnoughPermissions);
  }

  #[tokio::test]
  async fn not_workspace_member_but_collab_owner_try_full_access_collab_test() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Collab(object_1),
        ActionVariant::FromAccessLevel(&AFAccessLevel::FullAccess),
      )
      .await
      .unwrap();

    for action in [Action::Write, Action::Read, Action::Delete] {
      let result = enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromAction(&action),
        )
        .await;
      assert!(result.is_ok(), "action={:?}", action);
    }
  }

  #[tokio::test]
  async fn not_workspace_member_not_collab_member_and_try_full_access_collab_test() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // Since the user is not a member of the specified collaboration object, the access control system
    // should check if the user has fallback permissions from being a member of the workspace.
    // However, as the user is not a member of the workspace either, they should not have permission
    // to perform the actions on the collaboration object.
    // Therefore, for both actions, the expected result is `false`, indicating that the permission to
    // perform the action is denied.
    for action in [Action::Write, Action::Read] {
      let result = enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromAction(&action),
        )
        .await;
      assert!(result.is_err(), "action={:?}", action);
      let error_code = result.unwrap_err().code();
      assert_eq!(error_code, ErrorCode::NotEnoughPermissions);
    }
  }

  #[tokio::test]
  async fn cmp_owner_role_test() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Owner),
      )
      .await
      .unwrap();

    for role in [AFRole::Owner, AFRole::Member, AFRole::Guest] {
      assert!(enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Workspace(workspace_id),
          ActionVariant::FromRole(&role),
        )
        .await
        .is_ok());
      assert!(enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromRole(&role),
        )
        .await
        .is_ok());
    }
  }

  #[tokio::test]
  async fn cmp_member_role_test() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Member),
      )
      .await
      .unwrap();

    for role in [AFRole::Owner, AFRole::Member, AFRole::Guest] {
      if role == AFRole::Owner {
        let result = enforcer
          .enforce_policy(
            workspace_id,
            &uid,
            ObjectType::Workspace(workspace_id),
            ActionVariant::FromRole(&role),
          )
          .await;
        assert!(result.is_err());
        let error_code = result.unwrap_err().code();
        assert_eq!(error_code, ErrorCode::NotEnoughPermissions);

        let result = enforcer
          .enforce_policy(
            workspace_id,
            &uid,
            ObjectType::Collab(object_1),
            ActionVariant::FromRole(&role),
          )
          .await;
        assert!(result.is_err());
        let error_code = result.unwrap_err().code();
        assert_eq!(error_code, ErrorCode::NotEnoughPermissions);
      } else {
        assert!(enforcer
          .enforce_policy(
            workspace_id,
            &uid,
            ObjectType::Workspace(workspace_id),
            ActionVariant::FromRole(&role),
          )
          .await
          .is_ok());
        assert!(enforcer
          .enforce_policy(
            workspace_id,
            &uid,
            ObjectType::Collab(object_1),
            ActionVariant::FromRole(&role),
          )
          .await
          .is_ok());
      }
    }
  }

  #[tokio::test]
  async fn cmp_guest_role_test() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Guest),
      )
      .await
      .unwrap();

    for role in [AFRole::Owner, AFRole::Member, AFRole::Guest] {
      if role == AFRole::Owner || role == AFRole::Member {
        let result = enforcer
          .enforce_policy(
            workspace_id,
            &uid,
            ObjectType::Collab(object_1),
            ActionVariant::FromRole(&role),
          )
          .await;
        assert!(result.is_err());
        let error_code = result.unwrap_err().code();
        assert_eq!(error_code, ErrorCode::NotEnoughPermissions);
      } else {
        assert!(enforcer
          .enforce_policy(
            workspace_id,
            &uid,
            ObjectType::Collab(object_1),
            ActionVariant::FromRole(&role),
          )
          .await
          .is_ok());
      }
    }
  }

  #[tokio::test]
  async fn cmp_full_access_level_test() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Collab(object_1),
        ActionVariant::FromAccessLevel(&AFAccessLevel::FullAccess),
      )
      .await
      .unwrap();

    for level in [
      AFAccessLevel::ReadAndComment,
      AFAccessLevel::ReadAndWrite,
      AFAccessLevel::ReadOnly,
    ] {
      assert!(enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromAccessLevel(&level),
        )
        .await
        .is_ok());
    }
  }

  #[tokio::test]
  async fn cmp_read_only_level_test() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Collab(object_1),
        ActionVariant::FromAccessLevel(&AFAccessLevel::ReadOnly),
      )
      .await
      .unwrap();

    for level in [
      AFAccessLevel::ReadAndComment,
      AFAccessLevel::ReadAndWrite,
      AFAccessLevel::ReadOnly,
    ] {
      if matches!(level, AFAccessLevel::ReadOnly) {
        assert!(enforcer
          .enforce_policy(
            workspace_id,
            &uid,
            ObjectType::Collab(object_1),
            ActionVariant::FromAccessLevel(&level),
          )
          .await
          .is_ok());
      } else {
        let result = enforcer
          .enforce_policy(
            workspace_id,
            &uid,
            ObjectType::Collab(object_1),
            ActionVariant::FromAccessLevel(&level),
          )
          .await;
        assert!(result.is_err());
        let error_code = result.unwrap_err().code();
        assert_eq!(error_code, ErrorCode::NotEnoughPermissions);
        let result = enforcer
          .enforce_policy(
            workspace_id,
            &uid,
            ObjectType::Collab(object_1),
            ActionVariant::FromAccessLevel(&level),
          )
          .await;
        assert!(result.is_err());
        let error_code = result.unwrap_err().code();
        assert_eq!(error_code, ErrorCode::NotEnoughPermissions);
      }
    }
  }
}
