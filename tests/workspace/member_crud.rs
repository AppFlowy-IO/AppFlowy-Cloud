use database_entity::AFRole;

use shared_entity::dto::workspace_dto::{CreateWorkspaceMember, WorkspaceMemberChangeset};
use shared_entity::error_code::ErrorCode;

use crate::user::utils::generate_unique_registered_user_client;

#[tokio::test]
async fn add_workspace_members_not_enough_permission() {
  let (c1, user1) = generate_unique_registered_user_client().await;
  let (c2, _user2) = generate_unique_registered_user_client().await;
  let workspace_id = c2
    .get_workspaces()
    .await
    .unwrap()
    .first()
    .unwrap()
    .workspace_id;

  // attempt to add user2 to user1's workspace
  let err = c1
    .add_workspace_members(
      workspace_id.to_string(),
      vec![CreateWorkspaceMember {
        email: user1.email,
        role: AFRole::Member,
      }],
    )
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::NotEnoughPermissions);
}

#[tokio::test]
async fn add_workspace_members_then_delete() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let (c2, _user2) = generate_unique_registered_user_client().await;
  let c2_email = c2.token().read().as_ref().unwrap().user.email.clone();

  let c1_workspace = c1.get_workspaces().await.unwrap();
  let c1_workspace_id = c1_workspace.first().unwrap().workspace_id;

  let email = c2.token().read().as_ref().unwrap().user.email.to_owned();
  c1.add_workspace_members(
    c1_workspace_id.to_string(),
    vec![CreateWorkspaceMember {
      email,
      role: AFRole::Member,
    }],
  )
  .await
  .unwrap();

  {
    // check if user2's email is in c1's workspace members
    assert!(c1
      .get_workspace_members(c1_workspace_id)
      .await
      .unwrap()
      .iter()
      .any(|w| w.email == *c2_email));
  }

  let email = c2.token().read().as_ref().unwrap().user.email.to_owned();
  c1.remove_workspace_members(c1_workspace_id, [email].to_vec())
    .await
    .unwrap();

  {
    // check if user2's email is NOT in c1's workspace members
    assert!(!c1
      .get_workspace_members(c1_workspace_id)
      .await
      .unwrap()
      .iter()
      .any(|w| w.email == *c2_email));
  }
}

#[tokio::test]
async fn workspace_member_add_new_member() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let (c2, user2) = generate_unique_registered_user_client().await;
  let (_c3, user3) = generate_unique_registered_user_client().await;

  let workspace_id = c1
    .get_workspaces()
    .await
    .unwrap()
    .first()
    .unwrap()
    .workspace_id;

  c1.add_workspace_members(
    workspace_id.to_string(),
    vec![CreateWorkspaceMember {
      email: user2.email,
      role: AFRole::Member,
    }],
  )
  .await
  .unwrap();

  let err = c2
    .add_workspace_members(
      workspace_id.to_string(),
      vec![CreateWorkspaceMember {
        email: user3.email,
        role: AFRole::Member,
      }],
    )
    .await
    .unwrap_err();

  assert_eq!(err.code, ErrorCode::NotEnoughPermissions);
}

#[tokio::test]
async fn workspace_owner_add_new_owner() {
  let (c1, user1) = generate_unique_registered_user_client().await;
  let (_c2, user2) = generate_unique_registered_user_client().await;

  let workspace_id = c1
    .get_workspaces()
    .await
    .unwrap()
    .first()
    .unwrap()
    .workspace_id;
  c1.add_workspace_members(
    workspace_id.to_string(),
    vec![CreateWorkspaceMember {
      email: user2.email.clone(),
      role: AFRole::Owner,
    }],
  )
  .await
  .unwrap();

  let members = c1.get_workspace_members(workspace_id).await.unwrap();
  assert_eq!(members[0].email, user1.email);
  assert_eq!(members[0].role, AFRole::Owner);

  assert_eq!(members[1].email, user2.email);
  assert_eq!(members[1].role, AFRole::Owner);
}

#[tokio::test]
async fn workspace_second_owner_add_new_member() {
  let (c1, user1) = generate_unique_registered_user_client().await;
  let (c2, user2) = generate_unique_registered_user_client().await;
  let (_c3, user3) = generate_unique_registered_user_client().await;

  let workspace_id = c1
    .get_workspaces()
    .await
    .unwrap()
    .first()
    .unwrap()
    .workspace_id;
  c1.add_workspace_members(
    workspace_id.to_string(),
    vec![CreateWorkspaceMember {
      email: user2.email.clone(),
      role: AFRole::Owner,
    }],
  )
  .await
  .unwrap();

  c2.add_workspace_members(
    workspace_id.to_string(),
    vec![CreateWorkspaceMember {
      email: user3.email.clone(),
      role: AFRole::Member,
    }],
  )
  .await
  .unwrap();

  let members = c1.get_workspace_members(workspace_id).await.unwrap();
  assert_eq!(members[0].email, user1.email);
  assert_eq!(members[0].role, AFRole::Owner);

  assert_eq!(members[1].email, user2.email);
  assert_eq!(members[1].role, AFRole::Owner);

  assert_eq!(members[2].email, user3.email);
  assert_eq!(members[2].role, AFRole::Member);
}

#[tokio::test]
async fn workspace_second_owner_can_not_delete_origin_owner() {
  let (c1, user1) = generate_unique_registered_user_client().await;
  let (c2, user2) = generate_unique_registered_user_client().await;

  let workspace_id = c1
    .get_workspaces()
    .await
    .unwrap()
    .first()
    .unwrap()
    .workspace_id;
  c1.add_workspace_members(
    workspace_id.to_string(),
    vec![CreateWorkspaceMember {
      email: user2.email.clone(),
      role: AFRole::Owner,
    }],
  )
  .await
  .unwrap();

  let err = c2
    .remove_workspace_members(workspace_id, [user1.email].to_vec())
    .await
    .unwrap_err();

  assert_eq!(err.code, ErrorCode::NotEnoughPermissions);
}

#[tokio::test]
async fn workspace_owner_update_member_role() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let (_c2, user2) = generate_unique_registered_user_client().await;

  let workspace_id = c1
    .get_workspaces()
    .await
    .unwrap()
    .first()
    .unwrap()
    .workspace_id;
  c1.add_workspace_members(
    workspace_id.to_string(),
    vec![CreateWorkspaceMember {
      email: user2.email.clone(),
      role: AFRole::Member,
    }],
  )
  .await
  .unwrap();

  let members = c1.get_workspace_members(workspace_id).await.unwrap();
  assert_eq!(members[1].email, user2.email);
  assert_eq!(members[1].role, AFRole::Member);

  // Update user2's role to Owner
  c1.update_workspace_member(
    workspace_id,
    WorkspaceMemberChangeset::new(user2.email.clone()).with_role(AFRole::Owner),
  )
  .await
  .unwrap();
  let members = c1.get_workspace_members(workspace_id).await.unwrap();
  assert_eq!(members[1].role, AFRole::Owner);
}
