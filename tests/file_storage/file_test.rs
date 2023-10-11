use crate::collab::workspace_id_from_client;
use shared_entity::error_code::ErrorCode;

use crate::user::utils::generate_unique_registered_user_client;

#[tokio::test]
async fn get_but_not_exists() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let err = c1
    .get_file(&workspace_id, "not_exists_file_id")
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::RecordNotFound);
}

#[tokio::test]
async fn delete_but_not_exists() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let err = c1
    .delete_file(&workspace_id, "not_exists_file_id")
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::RecordNotFound);
}

#[tokio::test]
async fn put_and_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "hello world";
  let file_id = c1
    .put_file(&workspace_id, data, &mime)
    .await
    .unwrap()
    .file_id;

  let got_data = c1.get_file(&workspace_id, &file_id).await.unwrap();
  assert_eq!(got_data, data.as_bytes());
}

#[tokio::test]
async fn put_and_put_and_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data1 = "my content 1";
  let data2 = "my content 2";
  let file_id_1 = c1
    .put_file(&workspace_id, data1, &mime)
    .await
    .unwrap()
    .file_id;
  let file_id_2 = c1
    .put_file(&workspace_id, data2, &mime)
    .await
    .unwrap()
    .file_id;

  let got_data = c1.get_file(&workspace_id, &file_id_1).await.unwrap();
  assert_eq!(got_data, data1.as_bytes());

  let got_data = c1.get_file(&workspace_id, &file_id_2).await.unwrap();
  assert_eq!(got_data, data2.as_bytes());
}

#[tokio::test]
async fn put_delete_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "my contents";
  let file_id = c1
    .put_file(&workspace_id, data, &mime)
    .await
    .unwrap()
    .file_id;
  c1.delete_file(&workspace_id, &file_id).await.unwrap();

  let err = c1.get_file(&workspace_id, &file_id).await.unwrap_err();
  assert_eq!(err.code, ErrorCode::RecordNotFound);
}
