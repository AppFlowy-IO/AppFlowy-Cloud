use super::access::{
  load_group_policies, AccessControlChange, POLICY_FIELD_INDEX_OBJECT, POLICY_FIELD_INDEX_SUBJECT,
};
use crate::act::ActionVariant;
use crate::entity::ObjectType;
use crate::metrics::MetricsCalState;
use crate::request::{GroupPolicyRequest, PolicyRequest, WorkspacePolicyRequest};
use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;
use casbin::{CoreApi, Enforcer, MgmtApi};
use std::sync::atomic::Ordering;
use tokio::sync::RwLock;
use tracing::{event, instrument, trace};

#[async_trait]
pub trait EnforcerGroup {
  /// Get the group id of the user.
  /// User might belong to multiple groups. So return the highest permission group id.
  async fn get_enforce_group_id(&self, uid: &i64) -> Option<String>;
}

pub struct AFEnforcer<T> {
  enforcer: RwLock<Enforcer>,
  pub(crate) metrics_state: MetricsCalState,
  enforce_group: T,
}

impl<T> AFEnforcer<T>
where
  T: EnforcerGroup,
{
  pub async fn new(mut enforcer: Enforcer, enforce_group: T) -> Result<Self, AppError> {
    load_group_policies(&mut enforcer).await?;
    Ok(Self {
      enforcer: RwLock::new(enforcer),
      metrics_state: MetricsCalState::new(),
      enforce_group,
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
    uid: &i64,
    obj: ObjectType<'_>,
    act: ActionVariant<'_>,
  ) -> Result<Option<AccessControlChange>, AppError> {
    validate_obj_action(&obj, &act)?;

    let policies = act
      .policy_acts()
      .into_iter()
      .map(|act| vec![uid.to_string(), obj.policy_object(), act.to_string()])
      .collect::<Vec<Vec<_>>>();
    let number_of_updated_policies = policies.len();

    trace!("[access control]: add policy:{:?}", policies);
    self
      .enforcer
      .write()
      .await
      .add_policies(policies)
      .await
      .map_err(|e| AppError::Internal(anyhow!("fail to add policy: {e:?}")))?;

    if number_of_updated_policies > 0 {
      Ok(Some(AccessControlChange::UpdatePolicy {
        uid: *uid,
        oid: obj.object_id().to_string(),
      }))
    } else {
      Ok(None)
    }
  }

  /// Returns policies that match the filter.
  pub async fn remove_policy(
    &self,
    uid: &i64,
    object_type: &ObjectType<'_>,
  ) -> Result<Option<AccessControlChange>, AppError> {
    let mut enforcer = self.enforcer.write().await;
    self
      .remove_with_enforcer(uid, object_type, &mut enforcer)
      .await
  }

  /// 1. **Workspace Policy**: Initially, it checks if the user has permission at the workspace level. If the user
  ///    has permission to perform the action on the workspace, the function returns `true` without further checks.
  ///
  /// 2. **Group Policy**: (If applicable) If the workspace policy check fails (`false`), the function will then
  ///    evaluate group-level policies.
  ///
  /// 3. **Object-Specific Policy**: If both previous checks fail, the function finally evaluates the policy
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

    // 2. Fallback to group policy if workspace-level check fails.
    if !result {
      if let Some(guid) = self.enforce_group.get_enforce_group_id(uid).await {
        let policy_request = GroupPolicyRequest::new(&guid, &obj, &act);
        result = self
          .enforcer
          .read()
          .await
          .enforce(policy_request.to_policy())
          .map_err(|e| AppError::Internal(anyhow!("enforce: {e:?}")))?;
      }
    }

    // 3. Finally, enforce object-specific policy if previous checks fail.
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
    uid: &i64,
    object_type: &ObjectType<'_>,
    enforcer: &mut Enforcer,
  ) -> Result<Option<AccessControlChange>, AppError> {
    let policies_for_user_on_object =
      policies_for_subject_with_given_object(uid, object_type, enforcer).await;

    // if there are no policies for the user on the object, return early.
    if policies_for_user_on_object.is_empty() {
      return Ok(None);
    }

    event!(
      tracing::Level::INFO,
      "[access control]: remove policy:user={}, object={}, policies={:?}",
      uid,
      object_type.policy_object(),
      policies_for_user_on_object
    );

    enforcer
      .remove_policies(policies_for_user_on_object)
      .await
      .map_err(|e| AppError::Internal(anyhow!("error enforce: {e:?}")))?;

    Ok(Some(AccessControlChange::RemovePolicy {
      uid: *uid,
      oid: object_type.object_id().to_string(),
    }))
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
async fn policies_for_subject_with_given_object<T: ToString>(
  subject: T,
  object_type: &ObjectType<'_>,
  enforcer: &Enforcer,
) -> Vec<Vec<String>> {
  let subject = subject.to_string();
  let object_type_id = object_type.policy_object();
  let policies_related_to_object =
    enforcer.get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![object_type_id]);

  policies_related_to_object
    .into_iter()
    .filter(|p| p[POLICY_FIELD_INDEX_SUBJECT] == subject)
    .collect::<Vec<_>>()
}

pub struct NoEnforceGroup;
#[async_trait]
impl EnforcerGroup for NoEnforceGroup {
  async fn get_enforce_group_id(&self, _uid: &i64) -> Option<String> {
    None
  }
}

#[cfg(test)]
mod tests {
  use crate::{
    act::{Action, ActionVariant},
    casbin::{
      access::{casbin_model, cmp_role_or_level},
      enforcer::NoEnforceGroup,
    },
    entity::ObjectType,
  };
  use app_error::ErrorCode;
  use async_trait::async_trait;
  use casbin::{function_map::OperatorFunction, prelude::*};
  use database_entity::dto::{AFAccessLevel, AFRole};

  use super::{AFEnforcer, EnforcerGroup};

  pub struct TestEnforceGroup {
    guid: String,
  }
  #[async_trait]
  impl EnforcerGroup for TestEnforceGroup {
    async fn get_enforce_group_id(&self, _uid: &i64) -> Option<String> {
      Some(self.guid.clone())
    }
  }

  async fn test_enforcer<T>(enforce_group: T) -> AFEnforcer<T>
  where
    T: EnforcerGroup,
  {
    let model = casbin_model().await.unwrap();
    let mut enforcer = casbin::Enforcer::new(model, MemoryAdapter::default())
      .await
      .unwrap();

    enforcer.add_function("cmpRoleOrLevel", OperatorFunction::Arg2(cmp_role_or_level));
    AFEnforcer::new(enforcer, enforce_group).await.unwrap()
  }
  #[tokio::test]
  async fn collab_group_test() {
    let enforcer = test_enforcer(NoEnforceGroup).await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the collab
    enforcer
      .update_policy(
        &uid,
        ObjectType::Collab(object_1),
        ActionVariant::FromAccessLevel(&AFAccessLevel::FullAccess),
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
    let enforcer = test_enforcer(NoEnforceGroup).await;
    let uid = 1;
    let workspace_id = "w1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        &uid,
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Member),
      )
      .await
      .unwrap();

    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        &uid,
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Owner),
      )
      .await
      .unwrap();

    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        &uid,
        ObjectType::Workspace(workspace_id),
        ActionVariant::FromRole(&AFRole::Member),
      )
      .await
      .unwrap();

    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;

    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;
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
    let enforcer = test_enforcer(NoEnforceGroup).await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    // add user as a member of the workspace
    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    enforcer
      .update_policy(
        &uid,
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
    let enforcer = test_enforcer(NoEnforceGroup).await;
    let uid = 1;
    let workspace_id = "w1";
    let object_1 = "o1";

    enforcer
      .update_policy(
        &uid,
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
