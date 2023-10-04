use std::time::SystemTime;

use crate::user::utils::generate_unique_registered_user_client;

#[tokio::test]
async fn refresh_success() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let old_token = c.access_token().unwrap();
  tokio::time::sleep(std::time::Duration::from_secs(2)).await;
  c.refresh().await.unwrap();
  let new_token = c.access_token().unwrap();
  assert_ne!(old_token, new_token);
}

#[tokio::test]
async fn refresh_trigger() {
  let (c, _user) = generate_unique_registered_user_client().await;
  tokio::time::sleep(std::time::Duration::from_secs(2)).await;
  let old_access_token = c.access_token().unwrap();

  // Set the token to be expired
  c.token().write().as_mut().unwrap().expires_at = SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64;

  // querying that requires auth should trigger a refresh
  let _workspaces = c.workspaces().await.unwrap();
  let new_token = c.access_token().unwrap();

  assert_ne!(old_access_token, new_token);
}
