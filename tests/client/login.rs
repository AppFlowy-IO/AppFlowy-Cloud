use crate::client::utils::{timestamp_nano, LOCALHOST_URL};
use appflowy_server::client::client;

#[tokio::test]
async fn login_success() {
  let mut c = client::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let (email, _user, password) = register_deep_fake(&mut c).await;
  let initial_token = c.logged_in_token().unwrap().to_string();
  c.login(&email, &password).await.unwrap();
  let relogin_token = c.logged_in_token().unwrap();
  assert!(&initial_token != relogin_token);
  assert!(c.logged_in_token().is_some())
}

#[tokio::test]
async fn login_with_empty_email() {
  let mut c = client::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let (_email, _user, password) = register_deep_fake(&mut c).await;
  assert!(c.login("", &password).await.is_err());
}

#[tokio::test]
async fn login_with_empty_password() {
  let mut c = client::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let (email, _user, _password) = register_deep_fake(&mut c).await;
  assert!(c.login(&email, "").await.is_err());
}

#[tokio::test]
async fn login_with_unknown_user() {
  let mut c = client::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let token = c.login("unknown@appflowy.io", "Abc@123!").await;
  assert!(token.is_err());
}

async fn register_deep_fake(c: &mut client::Client) -> (String, String, String) {
  let email = format!("deep_fake{}@appflowy.io", timestamp_nano());
  let user = "user1";
  let password = "DeepFakePassword!123";
  c.register(user, &email, password).await.unwrap();

  assert!(c.logged_in_token().is_some());
  (email, user.to_string(), password.to_string())
}
