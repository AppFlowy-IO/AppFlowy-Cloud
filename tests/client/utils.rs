use dotenv::dotenv;
use std::time::SystemTime;

use lazy_static::lazy_static;

lazy_static! {
  pub static ref REGISTERED_EMAIL: String = {
    dotenv().ok();
    std::env::var("GOTRUE_REGISTERED_EMAIL").unwrap()
  };
  pub static ref REGISTERED_PASSWORD: String = {
    dotenv().ok();
    std::env::var("GOTRUE_REGISTERED_PASSWORD").unwrap()
  };
}

pub fn timestamp_nano() -> u128 {
  SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_nanos()
}

pub fn generate_unique_email() -> String {
  format!("{}@appflowy.io", timestamp_nano())
}
