use app_error::ErrorCode;
use client_api_test::*;
use gotrue_entity::dto::AuthProvider;
use std::time::Duration;

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
async fn concurrent_user_sign_up_test() {
  let mut tasks = Vec::new();
  for _i in 0..30 {
    let task = tokio::spawn(async move {
      let _ = TestClient::new_user().await;
      tokio::time::sleep(Duration::from_millis(300)).await;
    });
    tasks.push(task);
  }

  let results = futures::future::join_all(tasks).await;
  for result in results {
    assert!(result.is_ok(), "Task completed successfully");
  }
}
