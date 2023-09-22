use crate::client::utils::{register_deep_fake, LOCALHOST_URL};
use appflowy_cloud::client::http;

#[tokio::test]
async fn login_success() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let (email, _user, password) = register_deep_fake(&mut c).await;
  let initial_token = c.logged_in_token().unwrap().to_string();
  c.login(&email, &password).await.unwrap();
  let relogin_token = c.logged_in_token().unwrap();
  assert_ne!(&initial_token, relogin_token);
  assert!(c.logged_in_token().is_some())
}

#[tokio::test]
async fn login_with_empty_email() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let (_email, _user, password) = register_deep_fake(&mut c).await;
  assert!(c.login("", &password).await.is_err());
}

#[tokio::test]
async fn login_with_empty_password() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let (email, _user, _password) = register_deep_fake(&mut c).await;
  assert!(c.login(&email, "").await.is_err());
}

#[tokio::test]
async fn login_with_unknown_user() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let token = c.login("unknown@appflowy.io", "Abc@123!").await;
  assert!(token.is_err());
}
