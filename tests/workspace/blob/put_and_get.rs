use crate::collab::workspace_id_from_client;
use crate::user::utils::generate_unique_registered_user_client;
use app_error::ErrorCode;

#[tokio::test]
async fn get_but_not_exists() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let url = c1.get_blob_url("hello", "world");
  let err = c1.get_blob(&url).await.unwrap_err();
  assert_eq!(err.code, ErrorCode::RecordNotFound);

  let workspace_id = c1
    .get_workspaces()
    .await
    .unwrap()
    .0
    .first()
    .unwrap()
    .workspace_id
    .to_string();

  let url = c1.get_blob_url(&workspace_id, "world");
  let err = c1.get_blob(&url).await.unwrap_err();
  assert_eq!(err.code, ErrorCode::RecordNotFound);
}

#[tokio::test]
async fn put_and_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "hello world";
  let file_id = uuid::Uuid::new_v4().to_string();
  let url = c1.get_blob_url(&workspace_id, &file_id);
  c1.put_blob(&url, data, &mime).await.unwrap();

  let (got_mime, got_data) = c1.get_blob(&url).await.unwrap();
  assert_eq!(got_data, Vec::from(data));
  assert_eq!(got_mime, mime);

  c1.delete_blob(&url).await.unwrap();
}

#[tokio::test]
async fn put_giant_file() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let file_id = uuid::Uuid::new_v4().to_string();

  let url = c1.get_blob_url(&workspace_id, &file_id);
  let error = c1
    .put_blob_with_content_length(&url, "123", &mime, 10 * 1024 * 1024 * 1024)
    .await
    .unwrap_err();

  assert_eq!(error.code, ErrorCode::PayloadTooLarge);
}

#[tokio::test]
async fn put_and_put_and_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data1 = "my content 1";
  let data2 = "my content 2";
  let file_id_1 = uuid::Uuid::new_v4().to_string();
  let file_id_2 = uuid::Uuid::new_v4().to_string();
  let url_1 = c1.get_blob_url(&workspace_id, &file_id_1);
  let url_2 = c1.get_blob_url(&workspace_id, &file_id_2);
  c1.put_blob(&url_1, data1, &mime).await.unwrap();
  c1.put_blob(&url_2, data2, &mime).await.unwrap();

  let (got_mime, got_data) = c1.get_blob(&url_1).await.unwrap();
  assert_eq!(got_data, Vec::from(data1));
  assert_eq!(got_mime, mime);

  let (got_mime, got_data) = c1.get_blob(&url_2).await.unwrap();
  assert_eq!(got_data, Vec::from(data2));
  assert_eq!(got_mime, mime);

  c1.delete_blob(&url_1).await.unwrap();
  c1.delete_blob(&url_2).await.unwrap();
}

#[tokio::test]
async fn put_delete_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "my contents";
  let file_id = uuid::Uuid::new_v4().to_string();
  let url = c1.get_blob_url(&workspace_id, &file_id);
  c1.put_blob(&url, data, &mime).await.unwrap();
  c1.delete_blob(&url).await.unwrap();

  let url = c1.get_blob_url(&workspace_id, &file_id);
  let err = c1.get_blob(&url).await.unwrap_err();
  assert_eq!(err.code, ErrorCode::RecordNotFound);
}
