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

pub async fn generate_unique_registered_user() -> User {
  // log in as admin
  let admin_client = client_api_client();
  admin_client
    .sign_in_password(&ADMIN_USER.email, &ADMIN_USER.password)
    .await
    .unwrap();

  // create new user
  let email = generate_unique_email();
  let password = "Hello123!";
  admin_client
    .create_email_verified_user(&email, password)
    .await
    .unwrap();

  User {
    email,
    password: password.to_string(),
  }
}

pub async fn generate_unique_registered_user_client() -> (Client, User) {
  let registered_user = generate_unique_registered_user().await;
  let registered_user_client = client_api_client();
  registered_user_client
    .sign_in_password(&registered_user.email, &registered_user.password)
    .await
    .unwrap();
  (registered_user_client, registered_user)
}
