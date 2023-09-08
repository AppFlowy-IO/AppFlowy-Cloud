use client_api::Client;

use crate::client::{
  constants::LOCALHOST_URL,
  utils::{REGISTERED_EMAIL, REGISTERED_PASSWORD},
};

#[tokio::test]
async fn sign_out_but_not_sign_in() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let res = c.sign_out().await;
  assert!(res.is_err());
}

#[tokio::test]
async fn sign_out_after_sign_in() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);

  c.sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap()
    .unwrap();
  c.sign_out().await.unwrap();
}
