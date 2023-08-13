use appflowy_server::{client::http::Client, component::auth::gotrue::models::TokenResult};

use crate::client::utils::LOCALHOST_URL;

#[tokio::test]
async fn sign_in_unknown_user() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp = c
    .sign_in_password("unknown999@appflowy.io", "Hello123!")
    .await;
  let resp = resp.unwrap();
  match resp {
    TokenResult::Success(s) => panic!("should not be ok: {:?}", s),
    TokenResult::Fail(e) => {
      assert_eq!(e.error, "invalid_grant");
      assert_eq!(e.error_description.unwrap(), "Invalid login credentials");
    },
  }
}

#[tokio::test]
async fn sign_in_wrong_password() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);

  {
    let _ = c
      .sign_up("unknown123@appflowy.io", "Hello123!")
      .await
      .unwrap();
  }

  let resp = c
    .sign_in_password("unknown123@appflowy.io", "Hllo123!")
    .await;
  let resp = resp.unwrap();
  match resp {
    TokenResult::Success(s) => panic!("should not be ok: {:?}", s),
    TokenResult::Fail(e) => {
      assert_eq!(e.error, "invalid_grant");
      assert_eq!(e.error_description.unwrap(), "Invalid login credentials");
    },
  }
}

#[tokio::test]
async fn sign_in_unconfirmed_email() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);

  {
    let _ = c
      .sign_up("unknown123@appflowy.io", "Hello123!")
      .await
      .unwrap();
  }

  let resp = c
    .sign_in_password("unknown123@appflowy.io", "Hello123!")
    .await;
  let resp = resp.unwrap();
  match resp {
    TokenResult::Success(s) => panic!("should not be ok: {:?}", s),
    TokenResult::Fail(e) => {
      assert_eq!(e.error, "invalid_grant");
      assert_eq!(e.error_description.unwrap(), "Email not confirmed");
    },
  }
}

#[tokio::test]
async fn sign_in_success() {
  let c = Client::from(reqwest::Client::new(), LOCALHOST_URL);

  // pre-registered and confirmed email
  let email = "xigahi8979@tipent.com";

  let resp = c.sign_in_password(email, "Hello123!").await;
  let resp = resp.unwrap();
  match resp {
    TokenResult::Success(s) => {
      assert!(s.user.email_confirmed_at.is_some());
    },
    TokenResult::Fail(e) => panic!("should not fail: {:?}", e),
  }
}
