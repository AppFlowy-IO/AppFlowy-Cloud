use app_error::ErrorCode;
use client_api::ws::{WSClient, WSClientConfig};
use client_api_test::*;
use serde_json::json;
use shared_entity::dto::auth_dto::{UpdateUserParams, UserMetaData};
use std::time::Duration;
use uuid::Uuid;

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
async fn update_user_email() {
  let (c, user) = generate_unique_registered_user_client().await;
  c.sign_in_password(&user.email, &user.password)
    .await
    .unwrap();

  let new_email = format!("{}@appflowy.io", Uuid::new_v4());
  c.update_user(UpdateUserParams::new().with_email(new_email.clone()))
    .await
    .unwrap();

  let profile = c.get_profile().await.unwrap();
  assert_eq!(profile.email.unwrap().as_str(), &new_email);
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

#[tokio::test]
async fn user_change_notify_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let ws_client = WSClient::new(WSClientConfig::default(), c.clone(), c.clone());
  let mut user_change_recv = ws_client.subscribe_user_changed();
  ws_client.connect().await.unwrap();

  // After update user, the user_change_recv should receive a user change message via the websocket
  let fut = Box::pin(async move {
    c.update_user(UpdateUserParams::new().with_name("lucas"))
      .await
      .unwrap();
    let profile = c.get_profile().await.unwrap();
    assert_eq!(profile.name.unwrap().as_str(), "lucas");
    tokio::time::sleep(Duration::from_secs(5)).await;
  });

  tokio::select! {
    result = tokio::time::timeout(Duration::from_secs(5), async {
      println!("user_change: {:?}", user_change_recv.recv().await.unwrap());
    }) => {
      result.unwrap();
    },
    _ = fut => {
      panic!("update user timeout");
    },
  }
}

#[cfg(feature = "sync-v2")]
#[tokio::test]
async fn user_change_notify_test_v2() {
  use appflowy_proto::WorkspaceNotification;
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let mut workspace_changed = test_client.subscribe_workspace_notification(&workspace_id);

  // Update user name
  let new_name = "lucas";
  let user_id = test_client.uid().await;
  let api_client = test_client.api_client.clone();
  let clone_api_client = api_client.clone();
  tokio::spawn(async move {
    tokio::time::sleep(Duration::from_secs(3)).await;
    clone_api_client
      .update_user(UpdateUserParams::new().with_name(new_name))
      .await
      .unwrap();
  });

  // Wait for notification with a reasonable timeout
  match tokio::time::timeout(Duration::from_secs(30), workspace_changed.recv()).await {
    Ok(notification) => {
      if let Ok(WorkspaceNotification::UserProfileChange { uid, .. }) = notification {
        assert_eq!(user_id, uid);
        let profile = api_client.get_profile().await.unwrap();
        assert_eq!(profile.name.unwrap().as_str(), new_name);
      }
    },
    Err(_) => {
      panic!("Timed out waiting for user change notification");
    },
  }
}
