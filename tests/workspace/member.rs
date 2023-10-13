use shared_entity::dto::{CreateWorkspaceMember, WorkspacePermission};
use shared_entity::error_code::ErrorCode;

use crate::user::utils::generate_unique_registered_user_client;

#[tokio::test]
async fn add_workspace_members_not_enough_permission() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let (c2, _user2) = generate_unique_registered_user_client().await;

  let user2_workspace = c2.workspaces().await.unwrap();
  let user2_workspace_id = user2_workspace.first().unwrap().workspace_id;

  // attempt to add user2 to user1's workspace
  // using user1's client
  let email = c1.token().read().as_ref().unwrap().user.email.to_owned();
  let err = c1
    .add_workspace_members(
      user2_workspace_id,
      vec![CreateWorkspaceMember {
        email,
        permission: WorkspacePermission::Member,
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

  let c1_workspace = c1.workspaces().await.unwrap();
  let c1_workspace_id = c1_workspace.first().unwrap().workspace_id;

  let email = c2.token().read().as_ref().unwrap().user.email.to_owned();
  c1.add_workspace_members(
    c1_workspace_id,
    vec![CreateWorkspaceMember {
      email,
      permission: WorkspacePermission::Member,
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
