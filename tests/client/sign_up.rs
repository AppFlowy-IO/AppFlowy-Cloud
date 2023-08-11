use crate::client::utils::LOCALHOST_URL;
use appflowy_server::client::http::Client;

#[tokio::test]
async fn sign_up_success() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let _ = c
    .sign_up("exampleuser@appflowy.io", "Hello!123#")
    .await
    .unwrap();
}
