use client_api_test::LOCALHOST_GOTRUE;
use gotrue::api::Client;

#[tokio::test]
async fn gotrue_health() {
  let http_client = reqwest::Client::new();
  let gotrue_client = Client::new(http_client, &LOCALHOST_GOTRUE);
  gotrue_client.health().await.unwrap();
}
