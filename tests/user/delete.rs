use gotrue::params::AdminDeleteUserParams;

use crate::user::utils::{
  admin_user_client, generate_unique_registered_user_client, localhost_gotrue_client,
};

#[tokio::test]
async fn admin_delete_user() {
  let user_uuid = {
    let (client, _) = generate_unique_registered_user_client().await;
    client.get_profile().await.unwrap().uuid
  };

  let admin_token = {
    let admin_client = admin_user_client().await;
    admin_client.access_token().unwrap()
  };

  let gotrue_client: gotrue::api::Client = localhost_gotrue_client();
  // Delete user as admin
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

  // Delete user
}
