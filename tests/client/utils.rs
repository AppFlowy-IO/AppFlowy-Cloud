use client_api::Client;
use dotenv::dotenv;
use sqlx::types::Uuid;

use lazy_static::lazy_static;

use crate::client_api_client;

lazy_static! {
  pub static ref ADMIN_USER: User = {
    dotenv().ok();
    User {
      email: std::env::var("GOTRUE_ADMIN_EMAIL").unwrap(),
      password: std::env::var("GOTRUE_ADMIN_PASSWORD").unwrap(),
    }
  };
}

pub struct User {
  pub email: String,
  pub password: String,
}

pub fn generate_unique_email() -> String {
  format!("user_{}@appflowy.io", Uuid::new_v4())
}

pub async fn generate_unique_registered_user_client() -> (Client, User) {
  let admin_client = client_api_client();
  admin_client
    .sign_in_password(&ADMIN_USER.email, &ADMIN_USER.password)
    .await
    .unwrap();
  // TODO: register user

  let email = generate_unique_email();
  let password = "Hello123!";
  let user_client = client_api_client();
  user_client
    .sign_in_password(&email, password)
    .await
    .unwrap();

  todo!()
}
