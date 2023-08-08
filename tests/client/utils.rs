use std::time::SystemTime;

use appflowy_server::client::client::Client;

pub const LOCALHOST_URL: &str = "http://127.0.0.1:8000"; //TODO: change to default port

pub fn timestamp_nano() -> u128 {
  SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_nanos()
}

pub async fn register_deep_fake(c: &mut Client) -> (String, String, String) {
  let email = format!("deep_fake{}@appflowy.io", timestamp_nano());
  let user = "user1";
  let password = "DeepFakePassword!123";
  c.register(user, &email, password).await.unwrap();

  assert!(c.logged_in_token().is_some());
  (email, user.to_string(), password.to_string())
}
