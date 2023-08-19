use appflowy_server::client::http::Client;

use crate::client::constants::LOCALHOST_URL;

#[tokio::test]
async fn sign_out_but_not_sign_in() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let res = c.sign_out().await;
  assert!(res.is_err());
}

#[tokio::test]
async fn sign_out_after_sign_in() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);

  // pre-registered and confirmed email
  let email = "xigahi8979@tipent.com";
  c.sign_in_password(email, "Hello123!")
    .await
    .unwrap()
    .unwrap();
  c.sign_out().await.unwrap();
}
