use appflowy_server::client::client;

const LOCALHOST_URL: &str = "http://127.0.0.1:8000"; //TODO: change to default port

#[tokio::test]
async fn register_success() {
  let c = client::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let token = c
    .register("user1", "deep_fake4@appflowy.io", "DeepFakePassword!123")
    .await
    .unwrap();
  assert!(token.len() > 0);
}
