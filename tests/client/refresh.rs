use std::time::SystemTime;

use gotrue_entity::AccessTokenResponse;

use crate::{
  client::utils::{REGISTERED_EMAIL, REGISTERED_PASSWORD, REGISTERED_USER_MUTEX},
  client_api_client,
};

#[tokio::test]
async fn refresh_success() {
  let _guard = REGISTERED_USER_MUTEX.lock().await;

  let email = &REGISTERED_EMAIL;
  let password = &REGISTERED_PASSWORD;
  let mut c = client_api_client();
  c.sign_in_password(email, password).await.unwrap();
  let old_token = c.token().unwrap().access_token.to_owned();
  std::thread::sleep(std::time::Duration::from_secs(2));
  c.refresh().await.unwrap();
  let new_token = c.token().unwrap().access_token.to_owned();
  assert_ne!(old_token, new_token);
}

#[tokio::test]
async fn refresh_trigger() {
  let _guard = REGISTERED_USER_MUTEX.lock().await;

  let email = &REGISTERED_EMAIL;
  let password = &REGISTERED_PASSWORD;

  let mut c = client_api_client();

  c.sign_in_password(email, password).await.unwrap();
  std::thread::sleep(std::time::Duration::from_secs(2));
  let token = c.token().unwrap();
  let old_access_token = token.access_token.to_owned();

  // Set the token to be expired
  unsafe {
    let token_mut = token as *const AccessTokenResponse as *mut AccessTokenResponse;
    token_mut.as_mut().unwrap().expires_at = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap()
      .as_secs() as i64
  };

  // querying that requires auth should trigger a refresh
  let _workspaces = c.workspaces().await.unwrap();
  let new_token = c.token().unwrap().access_token.to_owned();

  assert_ne!(old_access_token, new_token);
}
