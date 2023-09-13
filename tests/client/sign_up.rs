use crate::client::utils::{generate_unique_email, REGISTERED_EMAIL, REGISTERED_PASSWORD};
use crate::client_api_client;

use shared_entity::server_error::ErrorCode;

#[tokio::test]
async fn sign_up_success() {
  let email = generate_unique_email();
  let password = "Hello!123#";
  let c = client_api_client();
  c.sign_up(&email, password).await.unwrap();
}

#[tokio::test]
async fn sign_up_invalid_email() {
  let invalid_email = "not_email_address";
  let password = "Hello!123#";
  let c = client_api_client();
  let error = c.sign_up(invalid_email, password).await.unwrap_err();
  assert_eq!(error.code, ErrorCode::InvalidEmail);
  assert_eq!(error.message, "invalid email: not_email_address");
}

#[tokio::test]
async fn sign_up_invalid_password() {
  let email = generate_unique_email();
  let password = "123";
  let c = client_api_client();
  let error = c.sign_up(&email, password).await.unwrap_err();
  assert_eq!(error.code, ErrorCode::InvalidPassword);
  assert_eq!(error.message, "invalid password: 123")
}

#[tokio::test]
async fn sign_up_but_existing_user() {
  let c = client_api_client();
  c.sign_up(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap();
}
