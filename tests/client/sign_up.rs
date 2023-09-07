use crate::client::{constants::LOCALHOST_URL, utils::generate_unique_email};
use client_api::Client;

#[tokio::test]
async fn sign_up_success() {
  let email = generate_unique_email();
  let password = "Hello!123#";
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp = c.sign_up(&email, password).await.unwrap();
  assert!(resp.confirmation_sent_at.is_some());
  assert!(resp.confirmed_at.is_none());
}

#[tokio::test]
async fn sign_up_invalid_email() {
  let invalid_email = "not_email_address";
  let password = "Hello!123#";
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp = c.sign_up(invalid_email, password).await;
  assert!(resp.is_err());
}

#[tokio::test]
async fn sign_up_invalid_password() {
  let email = generate_unique_email();
  let password = "123";
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp = c.sign_up(&email, password).await;
  assert!(resp.is_err());
}
