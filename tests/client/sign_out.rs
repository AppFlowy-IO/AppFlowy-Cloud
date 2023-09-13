use crate::client::utils::{REGISTERED_EMAIL, REGISTERED_PASSWORD};
use crate::client_api_client;

#[tokio::test]
async fn sign_out_but_not_sign_in() {
  let c = client_api_client();
  let res = c.sign_out().await;
  assert!(res.is_err());
}

#[tokio::test]
async fn sign_out_after_sign_in() {
  let mut c = client_api_client();

  c.sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap();
  c.sign_out().await.unwrap();
}
