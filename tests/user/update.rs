use shared_entity::dto::auth_dto::UpdateUsernameParams;
use shared_entity::error_code::ErrorCode;

use crate::localhost_client;
use crate::user::utils::generate_unique_registered_user_client;

#[tokio::test]
async fn update_but_not_logged_in() {
  let client = localhost_client();
  let error = client
    .update_user(UpdateUsernameParams::new().with_name("new name"))
    .await
    .unwrap_err();

  assert_eq!(error.code, ErrorCode::NotLoggedIn);
}

#[tokio::test]
async fn update_password_same_password() {
  let (c, user) = generate_unique_registered_user_client().await;
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();
  let err = c
    .update_user(
      UpdateUsernameParams::new()
        .with_password(user.password)
        .with_email(user.email),
    )
    .await
    .unwrap_err();
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

    c.update_user(UpdateUsernameParams::new().with_password(new_password))
      .await
      .unwrap();
  }
  {
    // revert password to old_password
    let c = localhost_client();
    c.sign_in_password(&user.email, new_password).await.unwrap();
    c.update_user(UpdateUsernameParams::new().with_password(user.password))
      .await
      .unwrap();
  }
}

#[tokio::test]
async fn update_user_name() {
  let (c, user) = generate_unique_registered_user_client().await;
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();
  c.update_user(UpdateUsernameParams::new().with_name("lucas"))
    .await
    .unwrap();

  let profile = c.get_profile().await.unwrap();
  assert_eq!(profile.name.unwrap().as_str(), "lucas");
}
