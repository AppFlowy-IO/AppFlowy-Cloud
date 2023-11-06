use crate::localhost_client;
use crate::user::utils::generate_unique_registered_user_client;
use app_error::ErrorCode;
use serde_json::json;
use shared_entity::dto::auth_dto::{UpdateUserParams, UserMetaData};

#[tokio::test]
async fn update_but_not_logged_in() {
  let client = localhost_client();
  let error = client
    .update_user(UpdateUserParams::new().with_name("new name"))
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
      UpdateUserParams::new()
        .with_password(user.password)
        .with_email(user.email),
    )
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::InvalidRequest);
  assert_eq!(
    err.message,
    "Invalid request:New password should be different from the old password."
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

    c.update_user(UpdateUserParams::new().with_password(new_password))
      .await
      .unwrap();
  }
  {
    // revert password to old_password
    let c = localhost_client();
    c.sign_in_password(&user.email, new_password).await.unwrap();
    c.update_user(UpdateUserParams::new().with_password(user.password))
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
  c.update_user(UpdateUserParams::new().with_name("lucas"))
    .await
    .unwrap();

  let profile = c.get_profile().await.unwrap();
  assert_eq!(profile.name.unwrap().as_str(), "lucas");
}

#[tokio::test]
async fn update_user_metadata() {
  let (c, user) = generate_unique_registered_user_client().await;
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();

  let mut metadata = UserMetaData::new();
  metadata.insert("str_value", "value");
  metadata.insert("int_value", 1);

  c.update_user(UpdateUserParams::new().with_metadata(metadata.clone()))
    .await
    .unwrap();

  let profile = c.get_profile().await.unwrap();
  assert_eq!(profile.metadata.unwrap(), json!(metadata));
}

#[tokio::test]
async fn user_metadata_override() {
  let (c, user) = generate_unique_registered_user_client().await;
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();

  let mut metadata_1 = UserMetaData::new();
  metadata_1.insert("str_value", "value");
  metadata_1.insert("int_value", 1);
  c.update_user(UpdateUserParams::new().with_metadata(metadata_1.clone()))
    .await
    .unwrap();

  let mut metadata_2 = UserMetaData::new();
  metadata_2.insert("bool_value", false);
  c.update_user(UpdateUserParams::new().with_metadata(metadata_2))
    .await
    .unwrap();
  metadata_1.insert("bool_value", false);

  let profile = c.get_profile().await.unwrap();
  assert_eq!(profile.metadata.unwrap(), json!(metadata_1));
}

#[tokio::test]
async fn user_empty_metadata_override() {
  let (c, user) = generate_unique_registered_user_client().await;
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();

  let mut metadata_1 = UserMetaData::new();
  metadata_1.insert("str_value", "value");
  metadata_1.insert("int_value", 1);
  c.update_user(UpdateUserParams::new().with_metadata(metadata_1.clone()))
    .await
    .unwrap();

  c.update_user(UpdateUserParams::new().with_metadata(UserMetaData::new()))
    .await
    .unwrap();

  let profile = c.get_profile().await.unwrap();
  assert_eq!(profile.metadata.unwrap(), json!(metadata_1));
}
