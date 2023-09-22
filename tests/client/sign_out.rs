use crate::client::utils::{REGISTERED_USERS, REGISTERED_USERS_MUTEX};
use crate::client_api_client;

#[tokio::test]
async fn sign_out_but_not_sign_in() {
  let c = client_api_client();
  let res = c.sign_out().await;
  assert!(res.is_err());
}

#[tokio::test]
async fn sign_out_after_sign_in() {
  let _guard = REGISTERED_USERS_MUTEX.lock().await;

  let c = client_api_client();
  let user = &REGISTERED_USERS[0];
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();
  c.sign_out().await.unwrap();
}
