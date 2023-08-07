use std::time::SystemTime;

pub const LOCALHOST_URL: &str = "http://127.0.0.1:8000"; //TODO: change to default port

pub fn timestamp_nano() -> u128 {
  SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_nanos()
}
