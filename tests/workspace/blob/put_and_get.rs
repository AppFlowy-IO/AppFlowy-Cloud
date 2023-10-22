use crate::collab::workspace_id_from_client;
use reqwest::Url;
use shared_entity::error_code::ErrorCode;

use std::path::Path;

use crate::user::utils::generate_unique_registered_user_client;
use crate::util::test_client::{assert_image_equal, generate_temp_file_path, TestClient};

#[tokio::test]
async fn get_but_not_exists() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let err = c1.get_blob("not_exists_file_id").await.unwrap_err();
  assert_eq!(err.code, ErrorCode::InvalidUrl);
}

#[tokio::test]
async fn put_and_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "hello world";
  let file_url = c1.put_blob(&workspace_id, data, &mime).await.unwrap();

  let url = Url::parse(&file_url).unwrap();
  let file_id = url.path_segments().unwrap().last().unwrap();
  assert_eq!(file_id, "uU0nuZNNPgilLlLX2n2r-sSE7-N6U4DukIj3rOLvzek=");

  let got_data = c1.get_blob(&file_url).await.unwrap();
  assert_eq!(got_data, data.as_bytes());

  c1.delete_blob(&file_url).await.unwrap();
}

#[tokio::test]
async fn put_giant_file() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let error = c1
    .put_blob_with_content_length(&workspace_id, "123", &mime, 10 * 1024 * 1024 * 1024)
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
  let url_1 = c1.put_blob(&workspace_id, data1, &mime).await.unwrap();
  let url_2 = c1.put_blob(&workspace_id, data2, &mime).await.unwrap();

  let got_data = c1.get_blob(&url_1).await.unwrap();
  assert_eq!(got_data, data1.as_bytes());

  let got_data = c1.get_blob(&url_2).await.unwrap();
  assert_eq!(got_data, data2.as_bytes());

  c1.delete_blob(&url_1).await.unwrap();
  c1.delete_blob(&url_2).await.unwrap();
}

#[tokio::test]
async fn put_delete_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "my contents";
  let url = c1.put_blob(&workspace_id, data, &mime).await.unwrap();
  c1.delete_blob(&url).await.unwrap();

  let err = c1.get_blob(&url).await.unwrap_err();
  assert_eq!(err.code, ErrorCode::RecordNotFound);
}

#[tokio::test]
async fn put_and_download_png() {
  let image_path = "tests/workspace/blob/asset/16kb_logo.png";
  let client = TestClient::new_user_without_ws_conn().await;
  let url = client.upload_file_with_path(image_path).await;

  let url = Url::parse(&url).unwrap();
  let file_id = url.path_segments().unwrap().last().unwrap();
  assert_eq!(file_id, "u_XUU5snbo_IDNQlqacesNyT6LVkxBKbnk3d82oq1og=");

  let usage = client.get_workspace_usage().await;
  assert_eq!(usage.consumed_capacity, 15694);

  // download the image and check the content is equal to the original file
  let temp_file = generate_temp_file_path("temp.png");
  std::fs::write(&temp_file, client.download_blob(&url).await).unwrap();

  assert_image_equal(Path::new(image_path), &temp_file);
}
