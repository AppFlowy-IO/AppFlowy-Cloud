use crate::client::utils::timestamp_nano;
use crate::client::utils::LOCALHOST_URL;
use appflowy_cloud::client::http;

#[tokio::test]
async fn register_success() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let email = format!("deep_fake{}@appflowy.io", timestamp_nano());
  c.register("user1", &email, "DeepFakePassword!123")
    .await
    .unwrap();
  assert!(c.logged_in_token().is_some())
}

#[tokio::test]
async fn register_with_invalid_password() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let email = format!("deep_fake{}@appflowy.io", timestamp_nano());
  let res = c.register("user1", &email, "123").await;
  assert!(res.is_err());
  assert!(c.logged_in_token().is_none())
}

#[tokio::test]
async fn register_with_invalid_name() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let email = format!("deep_fake{}@appflowy.io", timestamp_nano());
  let res = c.register("", &email, "DeepFakePassword!123").await;
  assert!(res.is_err());
  assert!(c.logged_in_token().is_none())
}

#[tokio::test]
async fn register_with_invalid_email() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let res = c
    .register("user1", "appflowy.io", "DeepFakePassword!123")
    .await;
  assert!(res.is_err());
  assert!(c.logged_in_token().is_none())
}
