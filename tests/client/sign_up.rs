use crate::client::utils::LOCALHOST_URL;
use appflowy_server::client::http::Client;

#[tokio::test]
async fn sign_up_success() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp = c
    .sign_up("exampleuser@appflowy.io", "Hello!123#")
    .await
    .unwrap();
  assert!(resp.confirmation_sent_at.is_some());
  assert!(resp.confirmed_at.is_none());
}

#[tokio::test]
async fn sign_up_invalid_email() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp = c.sign_up("exampleuser", "Hello!123#").await;
  assert!(resp.is_err());
}

#[tokio::test]
async fn sign_up_invalid_password() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp = c.sign_up("exampleuser@appflowy.io", "123").await;
  assert!(resp.is_err());
}
