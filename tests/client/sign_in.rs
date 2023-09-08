use client_api::Client;
use shared_entity::server_error::ErrorCode;

use crate::client::{
  constants::LOCALHOST_URL,
  utils::{generate_unique_email, REGISTERED_EMAIL, REGISTERED_PASSWORD},
};

#[tokio::test]
async fn sign_in_unknown_user() {
  let email = generate_unique_email();
  let password = "Hello123!";
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp = c.sign_in_password(&email, password).await;
  let resp = resp.unwrap();
  match resp {
    Ok(()) => panic!("should not be ok"),
    Err(e) => {
      assert_eq!(
        e.message,
        "oauth error: invalid_grant: Invalid login credentials"
      );
      assert_eq!(e.code, ErrorCode::OAuthError);
    },
  }
}

#[tokio::test]
async fn sign_in_wrong_password() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);

  let email = generate_unique_email();
  let password = "Hello123!";

  c.sign_up(&email, password).await.unwrap().unwrap();

  let wrong_password = "Hllo123!";
  let resp = c.sign_in_password(&email, wrong_password).await;
  let resp = resp.unwrap();
  match resp {
    Ok(()) => panic!("should not be ok"),
    Err(e) => {
      assert_eq!(
        e.message,
        "oauth error: invalid_grant: Invalid login credentials"
      );
      assert_eq!(e.code, ErrorCode::OAuthError);
    },
  }
}

#[tokio::test]
async fn sign_in_unconfirmed_email() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);

  let email = generate_unique_email();
  let password = "Hello123!";

  c.sign_up(&email, password).await.unwrap().unwrap();

  let resp = c.sign_in_password(&email, password).await;
  let resp = resp.unwrap();
  match resp {
    Ok(()) => panic!("should not be ok"),
    Err(e) => {
      assert_eq!(e.message, "oauth error: invalid_grant: Email not confirmed");
      assert_eq!(e.code, ErrorCode::OAuthError);
    },
  }
}

#[tokio::test]
async fn sign_in_success() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  c.sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap()
    .unwrap();
  let token = c.token().unwrap();
  assert!(token.user.confirmed_at.is_some());
}
