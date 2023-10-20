use gotrue_entity::UserUpdateParams;
use shared_entity::error_code::ErrorCode;

use crate::localhost_client;
use crate::user::utils::{generate_unique_email, generate_unique_registered_user_client};

#[tokio::test]
async fn update_but_not_logged_in() {
  let c = localhost_client();
  let new_email = generate_unique_email();
  let new_password = "Hello123!";
  let res = c
    .user_update(
      &UserUpdateParams {
        email: new_email,
        password: Some(new_password.to_owned()),
        ..Default::default()
      },
      None,
    )
    .await;
  assert!(res.is_err());
}

#[tokio::test]
async fn update_password_same_password() {
  let (c, user) = generate_unique_registered_user_client().await;
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();
  let err = c
    .user_update(
      &UserUpdateParams {
        email: user.email.to_owned(),
        password: Some(user.password.to_owned()),
        ..Default::default()
      },
      None,
    )
    .await
    .err()
    .unwrap();
  assert_eq!(err.code, ErrorCode::InvalidRequestParams);
  assert_eq!(
    err.message,
    "New password should be different from the old password."
  );
}

#[tokio::test]
async fn update_password_and_revert() {
  let (c, user) = generate_unique_registered_user_client().await;
  let new_password = "Hello456!";
  {
    // change password to new_password
    c.sign_in_password(&user.email, &user.password)
      .await
      .unwrap();
    c.user_update(
      &UserUpdateParams {
        password: Some(new_password.to_owned()),
        ..Default::default()
      },
      None,
    )
    .await
    .unwrap();
  }
  {
    // revert password to old_password
    let c = localhost_client();
    c.sign_in_password(&user.email, new_password).await.unwrap();
    c.user_update(
      &UserUpdateParams {
        password: Some(user.password.to_owned()),
        ..Default::default()
      },
      None,
    )
    .await
    .unwrap();
  }
}
