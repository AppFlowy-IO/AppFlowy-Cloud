use shared_entity::server_error::ErrorCode;

use crate::client::utils::{generate_unique_email, REGISTERED_EMAIL, REGISTERED_PASSWORD};
use crate::client_api_client;

#[tokio::test]
async fn sign_in_unknown_user() {
  let email = generate_unique_email();
  let password = "Hello123!";
  let mut c = client_api_client();
  let err = c.sign_in_password(&email, password).await.unwrap_err();
  assert_eq!(err.code, ErrorCode::OAuthError);
  assert!(!err.message.is_empty());
}

#[tokio::test]
async fn sign_in_wrong_password() {
  let mut c = client_api_client();

  let email = generate_unique_email();
  let password = "Hello123!";

  c.sign_up(&email, password).await.unwrap();

  let wrong_password = "Hllo123!";
  let err = c
    .sign_in_password(&email, wrong_password)
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::OAuthError);
  assert!(!err.message.is_empty());
}

#[tokio::test]
async fn sign_in_unconfirmed_email() {
  let mut c = client_api_client();

  let email = generate_unique_email();
  let password = "Hello123!";

  c.sign_up(&email, password).await.unwrap();

  let err = c.sign_in_password(&email, password).await.unwrap_err();
  assert_eq!(err.code, ErrorCode::OAuthError);
  assert!(!err.message.is_empty());
}

#[tokio::test]
async fn sign_in_success() {
  let mut c = client_api_client();
  c.sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap();
  let token = c.token().unwrap();
  assert!(token.user.confirmed_at.is_some());

  let workspaces = c.workspaces().await.unwrap();
  assert!(!workspaces.0.is_empty());
  let profile = c.profile().await.unwrap();
  let latest_workspace = workspaces.get_latest(profile);
  assert!(latest_workspace.is_some());
}
