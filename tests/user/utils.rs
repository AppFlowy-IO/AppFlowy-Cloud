use client_api::Client;
use dotenv::dotenv;
use scraper::{Html, Selector};
use sqlx::types::Uuid;

use lazy_static::lazy_static;

use crate::localhost_client;
use crate::util::setup_log;

lazy_static! {
  pub static ref ADMIN_USER: User = {
    dotenv().ok();
    User {
      email: std::env::var("GOTRUE_ADMIN_EMAIL").unwrap(),
      password: std::env::var("GOTRUE_ADMIN_PASSWORD").unwrap(),
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

pub async fn generate_sign_in_url_for_email(email: &str) -> String {
  setup_log();
  let admin_client = admin_user_client().await;
  admin_client.generate_sign_in_url(email).await.unwrap()
}

pub fn extract_sign_in_url(html_str: &str) -> String {
  let fragment = Html::parse_fragment(html_str);
  let selector = Selector::parse("a").unwrap();
  fragment
    .select(&selector)
    .next()
    .unwrap()
    .value()
    .attr("href")
    .unwrap()
    .to_string()
}
