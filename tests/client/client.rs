use appflowy_server::client::client;
use std::time::SystemTime;

const LOCALHOST_URL: &str = "http://127.0.0.1:8000"; //TODO: change to default port

#[tokio::test]
async fn register_success() {
  let c = client::Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let email = format!("deep_fake{}@appflowy.io", timestamp_nano());
  let token = c
    .register("user1", &email, "DeepFakePassword!123")
    .await
    .unwrap();
  assert!(token.len() > 0);
}

// Utils
fn timestamp_nano() -> u128 {
  SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_nanos()
}
