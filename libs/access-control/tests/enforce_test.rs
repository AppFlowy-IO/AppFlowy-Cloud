use access_control::access::{casbin_model, cmp_role_or_level, ObjectType};
use access_control::act::{Action, ActionVariant};
use access_control::enforcer::{AFEnforcer, EnforcerGroup, NoEnforceGroup};
use async_trait::async_trait;
use casbin::rhai::ImmutableString;
use casbin::{CoreApi, MemoryAdapter};
use database_entity::dto::{AFAccessLevel, AFRole};

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

  enforcer.add_function(
    "cmpRoleOrLevel",
    |r: ImmutableString, p: ImmutableString| cmp_role_or_level(r.as_str(), p.as_str()),
  );
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
      .await
      .unwrap();
    assert!(result);
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
      .await
      .unwrap();
    assert!(result, "action={:?}", action);
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
      .await
      .unwrap();
    assert!(result, "action={:?}", action);
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
      .await
      .unwrap();
    assert!(result, "action={:?}", action);
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
      .await
      .unwrap();
    assert!(result, "action={:?}", action);
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
      .await
      .unwrap();
    assert!(result, "action={:?}", action);
  }

  let result = enforcer
    .enforce_policy(
      workspace_id,
      &uid,
      ObjectType::Collab(object_1),
      ActionVariant::FromAction(&Action::Delete),
    )
    .await
    .unwrap();
  assert!(!result, "only the owner can perform delete")
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
      .await
      .unwrap();
    assert!(result, "action={:?}", action);
  }

  let result = enforcer
    .enforce_policy(
      workspace_id,
      &uid,
      ObjectType::Collab(object_1),
      ActionVariant::FromAction(&Action::Delete),
    )
    .await
    .unwrap();
  assert!(!result, "only the owner can perform delete")
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
      .await
      .unwrap();
    assert!(result, "action={:?}", action);
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
      .await
      .unwrap();
    assert!(!result, "action={:?}", action);
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
      .unwrap());
    assert!(enforcer
      .enforce_policy(
        workspace_id,
        &uid,
        ObjectType::Collab(object_1),
        ActionVariant::FromRole(&role),
      )
      .await
      .unwrap());
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
      assert!(!enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Workspace(workspace_id),
          ActionVariant::FromRole(&role),
        )
        .await
        .unwrap());

      assert!(!enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromRole(&role),
        )
        .await
        .unwrap());
    } else {
      assert!(enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Workspace(workspace_id),
          ActionVariant::FromRole(&role),
        )
        .await
        .unwrap());
      assert!(enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromRole(&role),
        )
        .await
        .unwrap());
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
      assert!(!enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromRole(&role),
        )
        .await
        .unwrap());
    } else {
      assert!(enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromRole(&role),
        )
        .await
        .unwrap());
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
      .unwrap());
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
        .unwrap());
    } else {
      assert!(!enforcer
        .enforce_policy(
          workspace_id,
          &uid,
          ObjectType::Collab(object_1),
          ActionVariant::FromAccessLevel(&level),
        )
        .await
        .unwrap());
    }
  }
}
