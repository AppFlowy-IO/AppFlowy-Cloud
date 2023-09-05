use appflowy_cloud::client::http::Client;

use crate::client::constants::LOCALHOST_URL;

#[tokio::test]
async fn update_but_not_logged_in() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let res = c.update("new_email_182@somemail.com", "Hello123!!").await;
  assert!(res.is_err());
}

#[tokio::test]
async fn update_password_same_password() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let email = "xigahi8979@tipent.com";
  let password = "Hello123!";
  c.sign_in_password(email, password).await.unwrap().unwrap();
  c.update(email, "Hello123!").await.unwrap();
}

#[tokio::test]
async fn update_password_and_revert() {
  let email = "xigahi8979@tipent.com";
  let old_password = "Hello123!";
  let new_password = "Hello456!";
  {
    // change password to new_password
    let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
    c.sign_in_password(email, old_password)
      .await
      .unwrap()
      .unwrap();
    c.update(email, new_password).await.unwrap();
  }
  {
    // revert password to old_password
    let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
    c.sign_in_password(email, new_password)
      .await
      .unwrap()
      .unwrap();
    c.update(email, old_password).await.unwrap();
  }
}
