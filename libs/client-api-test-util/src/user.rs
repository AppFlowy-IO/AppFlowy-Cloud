use crate::client::{localhost_client, LOCALHOST_GOTRUE};
use crate::log::setup_log;
use client_api::Client;
use lazy_static::lazy_static;
use uuid::Uuid;

lazy_static! {
  pub static ref ADMIN_USER: User = {
    dotenvy::dotenv().ok();
    User {
      email: std::env::var("GOTRUE_ADMIN_EMAIL").unwrap_or("admin@example.com".to_string()),
      password: std::env::var("GOTRUE_ADMIN_PASSWORD").unwrap_or("password".to_string()),
    }
  };
}

#[derive(Clone, Debug)]
pub struct User {
  pub email: String,
  pub password: String,
}

pub fn generate_unique_email() -> String {
  format!("user_{}@appflowy.io", Uuid::new_v4())
}

pub async fn admin_user_client() -> Client {
  let admin_client = localhost_client();
  #[cfg(target_arch = "wasm32")]
  {
    let msg = format!("{}", admin_client);
    web_sys::console::log_1(&msg.into());
  }

  admin_client
    .sign_in_password(&ADMIN_USER.email, &ADMIN_USER.password)
    .await
    .unwrap();
  admin_client
}

pub async fn generate_unique_registered_user() -> User {
  let admin_client = admin_user_client().await;

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
  setup_log();
  let registered_user = generate_unique_registered_user().await;
  let registered_user_client = localhost_client();
  registered_user_client
    .sign_in_password(&registered_user.email, &registered_user.password)
    .await
    .unwrap();
  (registered_user_client, registered_user)
}

pub async fn generate_sign_in_action_link(email: &str) -> String {
  setup_log();
  let admin_client = admin_user_client().await;
  admin_client
    .generate_sign_in_action_link(email)
    .await
    .unwrap()
}

// same as generate_unique_registered_user_client
// but with specific email
pub async fn api_client_with_email(user_email: &str) -> client_api::Client {
  let new_user_sign_in_link = {
    let admin_client = admin_user_client().await;
    admin_client
      .generate_sign_in_action_link(user_email)
      .await
      .unwrap()
  };

  let client = localhost_client();
  let appflowy_sign_in_url = client
    .extract_sign_in_url(&new_user_sign_in_link)
    .await
    .unwrap();
  client
    .sign_in_with_url(&appflowy_sign_in_url)
    .await
    .unwrap();

  client
}

pub fn localhost_gotrue_client() -> gotrue::api::Client {
  let reqwest_client = reqwest::Client::new();
  gotrue::api::Client::new(reqwest_client, &LOCALHOST_GOTRUE)
}
