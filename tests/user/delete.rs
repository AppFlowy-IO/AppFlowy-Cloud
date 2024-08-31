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
