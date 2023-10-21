use crate::collab::workspace_id_from_client;
use reqwest::Url;
use shared_entity::error_code::ErrorCode;

use crate::user::utils::generate_unique_registered_user_client;

#[tokio::test]
async fn get_but_not_exists() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let err = c1.get_file("not_exists_file_id").await.unwrap_err();
  assert_eq!(err.code, ErrorCode::InvalidUrl);
}

#[tokio::test]
async fn put_and_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "hello world";
  let file_url = c1.put_file(&workspace_id, data, &mime).await.unwrap();

  let url = Url::parse(&file_url).unwrap();
  let file_id = url.path_segments().unwrap().last().unwrap();
  assert_eq!(file_id, "uU0nuZNNPgilLlLX2n2r-sSE7-N6U4DukIj3rOLvzek=");

  let got_data = c1.get_file(&file_url).await.unwrap();
  assert_eq!(got_data, data.as_bytes());

  c1.delete_file(&file_url).await.unwrap();
}

#[tokio::test]
async fn put_giant_file() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let error = c1
    .put_file_with_content_length(&workspace_id, "123", &mime, 10 * 1024 * 1024 * 1024)
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
  let url_1 = c1.put_file(&workspace_id, data1, &mime).await.unwrap();
  let url_2 = c1.put_file(&workspace_id, data2, &mime).await.unwrap();

  let got_data = c1.get_file(&url_1).await.unwrap();
  assert_eq!(got_data, data1.as_bytes());

  let got_data = c1.get_file(&url_2).await.unwrap();
  assert_eq!(got_data, data2.as_bytes());

  c1.delete_file(&url_1).await.unwrap();
  c1.delete_file(&url_2).await.unwrap();
}

#[tokio::test]
async fn put_delete_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "my contents";
  let url = c1.put_file(&workspace_id, data, &mime).await.unwrap();
  c1.delete_file(&url).await.unwrap();

  let err = c1.get_file(&url).await.unwrap_err();
  assert_eq!(err.code, ErrorCode::RecordNotFound);
}
