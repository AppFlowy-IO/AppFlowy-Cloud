use shared_entity::error_code::ErrorCode;

use crate::client::utils::generate_unique_registered_user_client;

#[tokio::test]
async fn get_but_not_exists() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let err = c1
    .get_file_storage_object("not_exists_file")
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::FileNotFound);
}

#[tokio::test]
async fn put_and_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "hello world";
  let path = "mydata";
  c1.put_file_storage_object(path, data.into(), &mime)
    .await
    .unwrap();

  let got_data = c1.get_file_storage_object(path).await.unwrap();
  assert_eq!(got_data, data.as_bytes());
}

#[tokio::test]
async fn put_and_put_and_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data1 = "my content 1";
  let data2 = "my content 2";
  let path = "mydata";
  c1.put_file_storage_object(path, data1.into(), &mime)
    .await
    .unwrap();
  c1.put_file_storage_object(path, data2.into(), &mime)
    .await
    .unwrap();

  let got_data = c1.get_file_storage_object(path).await.unwrap();
  assert_eq!(got_data, data2.as_bytes());
}

#[tokio::test]
async fn delete_but_not_exists() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let err = c1
    .delete_file_storage_object("not_exists_file")
    .await
    .unwrap_err();
  assert_eq!(err.code, ErrorCode::FileNotFound);
}

#[tokio::test]
async fn put_delete_get() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "my contents";
  let path = "mydata";
  c1.put_file_storage_object(path, data.into(), &mime)
    .await
    .unwrap();
  c1.delete_file_storage_object(path).await.unwrap();

  let err = c1.get_file_storage_object(path).await.unwrap_err();
  assert_eq!(err.code, ErrorCode::FileNotFound);
}
