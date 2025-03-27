use client_api::entity::AFRole;
use client_api_test::*;
use gotrue::params::{AdminDeleteUserParams, AdminUserParams};

#[tokio::test]
async fn user_delete_self() {
  let (client, user) = generate_unique_registered_user_client().await;
  let admin_client = admin_user_client().await;
  {
    // user found before deletion
    let search_result = admin_client
      .admin_list_users(Some(&user.email))
      .await
      .unwrap();
    let _target_user = search_result
      .into_iter()
      .find(|u| u.email == user.email)
      .unwrap();
  }

  client.delete_user().await.unwrap();

  {
    // user cannot be found after deletion
    let search_result = admin_client
      .admin_list_users(Some(&user.email))
      .await
      .unwrap();
    let target_user = search_result.into_iter().find(|u| u.email == user.email);
    assert!(target_user.is_none(), "User should be deleted: {:?}", user);
  }
}

/// Scenario:
/// - User1 owns WorkspaceA
/// - User1 invites User2 to WorkspaceA
/// - User2 deletes itself
/// - WorkspaceA should still exist
#[tokio::test]
async fn user_delete_self_shared_workspace() {
  let user_1 = TestClient::new_user_without_ws_conn().await;
  let workspace_a = user_1.workspace_id().await;
  let user_2 = TestClient::new_user_without_ws_conn().await;
  user_1
    .invite_and_accepted_workspace_member(&workspace_a, &user_2, AFRole::Member)
    .await
    .unwrap();
  user_2.api_client.delete_user().await.unwrap();
  let user_1_workspaces = user_1.api_client.get_workspaces().await.unwrap();
  let _workspace_a = user_1_workspaces
    .into_iter()
    .find(|w| w.workspace_id == workspace_a)
    .unwrap();
}

#[tokio::test]
async fn admin_delete_create_same_user_hard() {
  let (client, user) = generate_unique_registered_user_client().await;
  let workspaces = client.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id;
  let user_uuid = client.get_profile().await.unwrap().uuid;

  let admin_token = {
    let admin_client = admin_user_client().await;
    admin_client.access_token().unwrap()
  };

  // Delete user as admin
  let gotrue_client: gotrue::api::Client = localhost_gotrue_client();
  gotrue_client
    .admin_delete_user(
      &admin_token,
      &user_uuid.to_string(),
      &AdminDeleteUserParams {
        should_soft_delete: false,
      },
    )
    .await
    .unwrap();

  // Recreate same user
  gotrue_client
    .admin_add_user(
      &admin_token,
      &AdminUserParams {
        email: user.email.clone(),
        password: Some(user.password.clone()),
        email_confirm: true,
        ..Default::default()
      },
    )
    .await
    .unwrap();

  // Login with recreated user
  let client = localhost_client();
  client
    .sign_in_password(&user.email, &user.password)
    .await
    .unwrap();
  let recreated_user_uuid = client.get_profile().await.unwrap().uuid;
  let recreated_workspace_uuid = client.get_workspaces().await.unwrap()[0].workspace_id;
  assert_ne!(user_uuid, recreated_user_uuid);
  assert_ne!(workspace_id, recreated_workspace_uuid);
}
