use client_api::Client;

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
      assert_eq!(e.error, "invalid_grant");
      assert_eq!(e.error_description.unwrap(), "Invalid login credentials");
    },
  }
}

#[tokio::test]
async fn sign_in_wrong_password() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);

  let email = generate_unique_email();
  let password = "Hello123!";
  {
    let _ = c.sign_up(&email, password).await.unwrap();
  }

  let wrong_password = "Hllo123!";
  let resp = c.sign_in_password(&email, wrong_password).await;
  let resp = resp.unwrap();
  match resp {
    Ok(()) => panic!("should not be ok"),
    Err(e) => {
      assert_eq!(e.error, "invalid_grant");
      assert_eq!(e.error_description.unwrap(), "Invalid login credentials");
    },
  }
}

#[tokio::test]
async fn sign_in_unconfirmed_email() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);

  let email = generate_unique_email();
  let password = "Hello123!";
  {
    let _ = c.sign_up(&email, password).await.unwrap();
  }

  let resp = c.sign_in_password(&email, password).await;
  let resp = resp.unwrap();
  match resp {
    Ok(()) => panic!("should not be ok"),
    Err(e) => {
      assert_eq!(e.error, "invalid_grant");
      assert_eq!(e.error_description.unwrap(), "Email not confirmed");
    },
  }
}

#[tokio::test]
async fn sign_in_success() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);

  let resp = c
    .sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await;
  let resp = resp.unwrap();
  match resp {
    Ok(()) => {},
    Err(e) => panic!("should not fail: {:?}", e),
  }
  let token = c.token().unwrap();
  assert!(token.user.confirmed_at.is_some());
}
