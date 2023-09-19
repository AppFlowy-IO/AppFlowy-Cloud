use shared_entity::error_code::ErrorCode;

use crate::{client::utils::REGISTERED_USERS_MUTEX, user_1_signed_in, user_2_signed_in};

#[tokio::test]
async fn add_workspace_members_not_enough_permission() {
  let _guard = REGISTERED_USERS_MUTEX.lock().await;

  let mut c1 = user_1_signed_in().await;
  let mut c2 = user_2_signed_in().await;

  let user2_workspace = c2.workspaces().await.unwrap();
  let user2_workspace_id = user2_workspace.first().unwrap().workspace_id;

  // attempt to add user2 to user1's workspace
  // using user1's client
  let err = c1
    .add_workspace_members(
      user2_workspace_id,
      [c1.token().unwrap().user.email.to_owned()].to_vec(),
    )
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::NotEnoughPermissions);
}

#[tokio::test]
async fn add_workspace_members_then_delete() {
  let _guard = REGISTERED_USERS_MUTEX.lock().await;

  let mut c1 = user_1_signed_in().await;
  let c2 = user_2_signed_in().await;
  let c2_email = &c2.token().unwrap().user.email;

  let c1_workspace = c1.workspaces().await.unwrap();
  let c1_workspace_id = c1_workspace.first().unwrap().workspace_id;

  c1.add_workspace_members(
    c1_workspace_id,
    [c2.token().unwrap().user.email.to_owned()].to_vec(),
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

  c1.remove_workspace_members(
    c1_workspace_id,
    [c2.token().unwrap().user.email.to_owned()].to_vec(),
  )
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
