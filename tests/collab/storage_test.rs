use crate::client::constants::LOCALHOST_URL;
use crate::client::utils::{REGISTERED_EMAIL, REGISTERED_PASSWORD};
use client_api::Client;

#[tokio::test]
async fn insert_collab_test() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  c.sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap();
  let token = c.token().unwrap();
  assert!(token.user.confirmed_at.is_some());

  let workspaces = c.workspaces().await.unwrap();
  assert!(!workspaces.0.is_empty());
}
