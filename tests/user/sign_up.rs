use app_error::ErrorCode;
use gotrue_entity::dto::AuthProvider;

use crate::{
  localhost_client, test_appflowy_cloud_client,
  user::utils::{generate_unique_email, generate_unique_registered_user_client},
};

#[tokio::test]
async fn sign_up_success() {
  let email = generate_unique_email();
  let password = "Hello!123#";
  let c = localhost_client();
  c.sign_up(&email, password).await.unwrap();
}

#[tokio::test]
async fn sign_up_invalid_email() {
  let invalid_email = "not_email_address";
  let password = "Hello!123#";
  let error = localhost_client()
    .sign_up(invalid_email, password)
    .await
    .unwrap_err();
  assert_eq!(error.code, ErrorCode::InvalidRequest);
  assert_eq!(
    error.message,
    "Invalid request:Unable to validate email address: invalid format"
  );
}

#[tokio::test]
async fn sign_up_invalid_password() {
  let email = generate_unique_email();
  let password = "3";
  let c = localhost_client();
  let error = c.sign_up(&email, password).await.unwrap_err();
  assert_eq!(error.code, ErrorCode::InvalidRequest);
  assert_eq!(
    error.message,
    "Invalid request:Password should be at least 6 characters"
  );
}

#[tokio::test]
async fn sign_up_but_existing_user() {
  let (c, user) = generate_unique_registered_user_client().await;
  c.sign_up(&user.email, &user.password).await.unwrap();
}

#[tokio::test]
async fn sign_up_oauth_not_available() {
  let c = localhost_client();
  let err = c
    .generate_oauth_url_with_provider(&AuthProvider::Zoom)
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
  let c = localhost_client();
  let url = c
    .generate_oauth_url_with_provider(&AuthProvider::Google)
    .await
    .unwrap();
  assert!(!url.is_empty());

  let c = test_appflowy_cloud_client();
  let url = c
    .generate_oauth_url_with_provider(&AuthProvider::Google)
    .await
    .unwrap();
  assert!(!url.is_empty());

  // let a = r#"appflowy-flutter://#access_token=eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJleHAiOjE2OTY0MzExODAsImlhdCI6MTY5NjQyNzU4MCwic3ViIjoiZWQ4Y2RhMzUtM2Q5MC00YTdjLWI3NTEtMzA2OWQ5Nzk4ZTZiIiwiZW1haWwiOiJuYXRoYW5AYXBwZmxvd3kuaW8iLCJwaG9uZSI6IiIsImFwcF9tZXRhZGF0YSI6eyJwcm92aWRlciI6Imdvb2dsZSIsInByb3ZpZGVycyI6WyJnb29nbGUiXX0sInVzZXJfbWV0YWRhdGEiOnsiYXZhdGFyX3VybCI6Imh0dHBzOi8vbGgzLmdvb2dsZXVzZXJjb250ZW50LmNvbS9hL0FDZzhvY0lHb2tMeFE2U2dWY2F3UERwcmYxY05abVV3MU5yXzF0djR5bXlTc2VvaT1zOTYtYyIsImN1c3RvbV9jbGFpbXMiOnsiaGQiOiJhcHBmbG93eS5pbyJ9LCJlbWFpbCI6Im5hdGhhbkBhcHBmbG93eS5pbyIsImVtYWlsX3ZlcmlmaWVkIjp0cnVlLCJmdWxsX25hbWUiOiJOYXRoYW4gRm9vIiwiaXNzIjoiaHR0cHM6Ly9hY2NvdW50cy5nb29nbGUuY29tIiwibmFtZSI6Ik5hdGhhbiBGb28iLCJwaWN0dXJlIjoiaHR0cHM6Ly9saDMuZ29vZ2xldXNlcmNvbnRlbnQuY29tL2EvQUNnOG9jSUdva0x4UTZTZ1ZjYXdQRHByZjFjTlptVXcxTnJfMXR2NHlteVNzZW9pPXM5Ni1jIiwicHJvdmlkZXJfaWQiOiIxMDk0ODEzOTczMjQ4MjM2Mzk0MzUiLCJzdWIiOiIxMDk0ODEzOTczMjQ4MjM2Mzk0MzUifSwicm9sZSI6IiIsImFhbCI6ImFhbDEiLCJhbXIiOlt7Im1ldGhvZCI6Im9hdXRoIiwidGltZXN0YW1wIjoxNjk2NDI3NTgwfV0sInNlc3Npb25faWQiOiIyNzkwOGExNS02MDIxLTQ4MjctOTNhOS0wZGU3Y2EwYjg3MjgifQ.UNCSfcIVqFRRRtTkhGipEXBOleHQt35lhbMaIYLZuv4&expires_at=1696431180&expires_in=3600&provider_token=ya29.a0AfB_byDtFDX9UfiXw3IKzGTrZeebaQCxheWpqVg3tZi5jCWdKmZRBFsh7p7k0svqxaaX8rqN0lQsFeBbdGtd7KOSYtjsfcOkpHMH0d1fMSxrlyl_KkuvlkJe9q_X4SvpsJmx0VsVZ1CMypszLd4nzZitB0KotQPMzFMaCgYKAcASARESFQGOcNnC8cf9LzY-ZeirJHIwfL9r8w0170&refresh_token=tXCWxsp3cQF8U2307mGuMQ&token_type=bearer"#;
  // c.sign_in_with_url(a).await.unwrap();
}
