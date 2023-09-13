use gotrue_entity::OAuthProvider;
use shared_entity::error_code::ErrorCode;

use crate::{
  client::utils::{generate_unique_email, REGISTERED_EMAIL, REGISTERED_PASSWORD},
  client_api_client,
};

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

#[tokio::test]
async fn sign_up_oauth_not_available() {
  let c = client_api_client();
  assert_eq!(
    // Change Zoom to any other valid OAuth provider
    // to manually open the browser and login
    c.oauth_login(OAuthProvider::Zoom).await.err().unwrap().code,
    ErrorCode::InvalidOAuthProvider
  );
}
