use gotrue_entity::OAuthProvider;
use shared_entity::error_code::ErrorCode;

use crate::{
  client_api_client,
  user::utils::{generate_unique_email, generate_unique_registered_user_client},
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
  let error = client_api_client()
    .sign_up(invalid_email, password)
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::InvalidRequestParams);
  assert_eq!(
    error.message,
    "Unable to validate email address: invalid format"
  );
}

#[tokio::test]
async fn sign_up_invalid_password() {
  let email = generate_unique_email();
  let password = "3";
  let c = client_api_client();
  let error = c.sign_up(&email, password).await.unwrap_err();
  assert_eq!(error.code, ErrorCode::InvalidRequestParams);
  assert_eq!(error.message, "Password should be at least 6 characters");
}

#[tokio::test]
async fn sign_up_but_existing_user() {
  let (c, user) = generate_unique_registered_user_client().await;
  c.sign_up(&user.email, &user.password).await.unwrap();
}

#[tokio::test]
async fn sign_up_oauth_not_available() {
  let c = client_api_client();
  let err = c
    .generate_oauth_url_with_provider(&OAuthProvider::Zoom)
    .await
    .err()
    .unwrap();
  assert_eq!(
    // Change Zoom to any other valid OAuth provider
    // to manually open the browser and login
    err.code,
    ErrorCode::InvalidOAuthProvider
  );
}

#[tokio::test]
async fn sign_up_with_google_oauth() {
  let c = client_api_client();
  let _ = c
    .generate_oauth_url_with_provider(&OAuthProvider::Google)
    .await
    .unwrap();
}
