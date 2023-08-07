use std::rc::Rc;

use crate::client::utils::{timestamp_nano, LOCALHOST_URL};
use appflowy_server::client::client;

#[tokio::test]
async fn login_success() {
  let c = client::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let c = Rc::new(c);
  let (email, _user, password) = register_deep_fake(c.clone()).await;
  let token = c.login(&email, &password).await.unwrap();
  assert!(token.len() > 0);
}

#[tokio::test]
async fn login_with_empty_email() {
  let c = client::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let c = Rc::new(c);
  let (_email, _user, password) = register_deep_fake(c.clone()).await;
  let token = c.login("", &password).await;
  assert!(token.is_err());
}

#[tokio::test]
async fn login_with_empty_password() {
  let c = client::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let c = Rc::new(c);
  let (email, _user, _password) = register_deep_fake(c.clone()).await;
  let token = c.login(&email, "").await;
  assert!(token.is_err());
}

#[tokio::test]
async fn login_with_unknown_user() {
  let c = client::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let c = Rc::new(c);
  let token = c.login("unknown@appflowy.io", "Abc@123!").await;
  assert!(token.is_err());
}

async fn register_deep_fake(c: Rc<client::Client>) -> (String, String, String) {
  let email = format!("deep_fake{}@appflowy.io", timestamp_nano());
  let user = "user1";
  let password = "DeepFakePassword!123";
  let token = c.register(user, &email, password).await.unwrap();
  assert!(token.len() > 0);
  (email, user.to_string(), password.to_string())
}
