use crate::client::{
  constants::LOCALHOST_URL,
  utils::{generate_unique_email, REGISTERED_EMAIL, REGISTERED_PASSWORD},
};
use client_api::Client;
use shared_entity::{error::AppError, server_error::ErrorCode};

#[tokio::test]
async fn sign_up_success() {
  let email = generate_unique_email();
  let password = "Hello!123#";
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  c.sign_up(&email, password).await.unwrap().unwrap();
}

#[tokio::test]
async fn sign_up_invalid_email() {
  let invalid_email = "not_email_address";
  let password = "Hello!123#";
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp: Result<(), AppError> = c.sign_up(invalid_email, password).await.unwrap();
  assert_eq!(resp.unwrap_err().code, ErrorCode::InvalidEmail)
}

#[tokio::test]
async fn sign_up_invalid_password() {
  let email = generate_unique_email();
  let password = "123";
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp: Result<(), AppError> = c.sign_up(&email, password).await.unwrap();
  assert_eq!(resp.unwrap_err().code, ErrorCode::InvalidPassword);
}

#[tokio::test]
async fn sign_up_but_existing_user() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  c.sign_up(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap()
    .unwrap();
}
