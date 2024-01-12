use gotrue::api::Client;

use crate::LOCALHOST_GOTRUE;

#[tokio::test]
async fn gotrue_health() {
  let http_client = reqwest::Client::new();
  let gotrue_client = Client::new(http_client, &LOCALHOST_GOTRUE);
  gotrue_client.health().await.unwrap();
}
