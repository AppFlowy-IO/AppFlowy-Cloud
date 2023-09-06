use std::time::SystemTime;

pub const REGISTERED_EMAIL: &str = "xigahi8979@tipent.com";
pub const REGISTERED_PASSWORD: &str = "Hello123!";

pub fn timestamp_nano() -> u128 {
  SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_nanos()
}

pub fn generate_unique_email() -> String {
  format!("{}@appflowy.io", timestamp_nano())
}
