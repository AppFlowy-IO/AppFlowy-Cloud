use appflowy_cloud::client::http::Client;

use crate::client::constants::LOCALHOST_URL;

#[tokio::test]
async fn sign_in_unknown_user() {
  let mut c = Client::from(reqwest::Client::new(), LOCALHOST_URL);
  let resp = c
    .sign_in_password("unknown999@appflowy.io", "Hello123!")
    .await;
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

  // pre-registered and confirmed email
  let email = "xigahi8979@tipent.com";

  let resp = c.sign_in_password(email, "Hello123!").await;
  let resp = resp.unwrap();
  match resp {
    Ok(()) => {},
    Err(e) => panic!("should not fail: {:?}", e),
  }
  let token = c.token().unwrap();
  assert!(token.user.confirmed_at.is_some());
}
