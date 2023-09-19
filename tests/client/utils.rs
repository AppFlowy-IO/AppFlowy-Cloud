use dotenv::dotenv;
use std::time::SystemTime;
use tokio::sync::Mutex;

use lazy_static::lazy_static;

lazy_static! {
  pub static ref REGISTERED_USERS_MUTEX: Mutex<()> = Mutex::new(());
  pub static ref REGISTERED_USERS: [RegisteredUser; 2] = {
    dotenv().ok();
    [
      RegisteredUser {
        email: std::env::var("GOTRUE_REGISTERED_EMAIL_1").unwrap(),
        password: std::env::var("GOTRUE_REGISTERED_PASSWORD_1").unwrap(),
      },
      RegisteredUser {
        email: std::env::var("GOTRUE_REGISTERED_EMAIL_2").unwrap(),
        password: std::env::var("GOTRUE_REGISTERED_PASSWORD_2").unwrap(),
      },
    ]
  };
}

pub struct RegisteredUser {
  pub email: String,
  pub password: String,
}

pub fn timestamp_nano() -> u128 {
  SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_nanos()
}

pub fn generate_unique_email() -> String {
  format!("user_{}@appflowy.io", timestamp_nano())
}
