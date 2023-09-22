use crate::client::utils::{register_deep_fake, LOCALHOST_URL};
use appflowy_cloud::client::http;

#[tokio::test]
async fn change_password_with_unmatch_password() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let new_password = "HelloWord@1a";
  let new_password_confirm = "HeloWord@1a";
  let (_email, _user, password) = register_deep_fake(&mut c).await;
  let res = c
    .change_password(&password, new_password, new_password_confirm)
    .await;
  assert!(res.is_err())
}

#[tokio::test]
async fn login_failed_after_change_password() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let new_password = "HelloWord@1a";
  let (email, _user, old_password) = register_deep_fake(&mut c).await;
  let res = c
    .change_password(&old_password, new_password, new_password)
    .await;
  assert!(res.is_ok());
  let res = c.login(&email, &old_password).await;
  assert!(res.is_err())
}

#[tokio::test]
async fn login_success_after_change_password() {
  let c = http::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let new_password = "HelloWord@1a";
  let (email, _user, old_password) = register_deep_fake(&mut c).await;
  let res = c
    .change_password(&old_password, new_password, new_password)
    .await;
  assert!(res.is_ok());
  let res = c.login(&email, new_password).await;
  assert!(res.is_ok())
}
