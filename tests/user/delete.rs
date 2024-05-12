use client_api_test::*;
use gotrue::params::{AdminDeleteUserParams, AdminUserParams};

#[tokio::test]
async fn admin_delete_create_same_user_hard() {
  let (client, user) = generate_unique_registered_user_client().await;
  let workspaces = client.get_workspaces().await.unwrap().0;
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
  let recreated_workspace_uuid = client.get_workspaces().await.unwrap().0[0].workspace_id;
  assert_ne!(user_uuid, recreated_user_uuid);
  assert_ne!(workspace_id, recreated_workspace_uuid);
}
