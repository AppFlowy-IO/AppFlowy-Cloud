use crate::client::utils::{
  generate_unique_email, REGISTERED_EMAIL, REGISTERED_PASSWORD, REGISTERED_USER_MUTEX,
};
use crate::client_api_client;

#[tokio::test]
async fn update_but_not_logged_in() {
  let mut c = client_api_client();
  let new_email = generate_unique_email();
  let new_password = "Hello123!";
  let res = c.update(&new_email, new_password).await;
  assert!(res.is_err());
}

#[tokio::test]
async fn update_password_same_password() {
  let _guard = REGISTERED_USER_MUTEX.lock().await;

  let mut c = client_api_client();
  c.sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap();
  c.update(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap();
}

#[tokio::test]
async fn update_password_and_revert() {
  let _guard = REGISTERED_USER_MUTEX.lock().await;

  let new_password = "Hello456!";
  {
    // change password to new_password
    let mut c = client_api_client();
    c.sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
      .await
      .unwrap();
    c.update(&REGISTERED_EMAIL, new_password).await.unwrap();
  }
  {
    // revert password to old_password
    let mut c = client_api_client();
    c.sign_in_password(&REGISTERED_EMAIL, new_password)
      .await
      .unwrap();
    c.update(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
      .await
      .unwrap();
  }
}
