use super::TestBucket;
use app_error::ErrorCode;
use client_api_test::{generate_unique_registered_user_client, workspace_id_from_client};

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

// TODO: fix inconsistent behavior due to different error handling with nginx
#[tokio::test]
async fn put_giant_file() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let file_id = uuid::Uuid::new_v4().to_string();

  let url = c1.get_blob_url(&workspace_id, &file_id);
  let data = vec![0; 10 * 1024 * 1024 * 1024];
  let error = c1.put_blob(&url, data, &mime).await.unwrap_err();

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

#[tokio::test]
async fn put_and_delete_workspace() {
  let test_bucket = TestBucket::new().await;

  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let file_id = uuid::Uuid::new_v4().to_string();
  let blob_to_put = "some contents 1";
  {
    // put blob
    let mime = mime::TEXT_PLAIN_UTF_8;
    let url = c1.get_blob_url(&workspace_id, &file_id);
    c1.put_blob(&url, blob_to_put, &mime).await.unwrap();
  }

  {
    // blob exists in the bucket
    let raw_data = test_bucket
      .get_object(&workspace_id, &file_id)
      .await
      .unwrap();
    assert_eq!(blob_to_put, String::from_utf8_lossy(&raw_data));
  }

  // delete workspace
  c1.delete_workspace(&workspace_id).await.unwrap();

  {
    // blob does not exist in the bucket
    let is_none = test_bucket
      .get_object(&workspace_id, &file_id)
      .await
      .is_none();
    assert!(is_none);
  }
}

#[tokio::test]
async fn simulate_30_put_blob_request_test() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;

  let mut handles = vec![];
  for _ in 0..30 {
    let cloned_client = c1.clone();
    let cloned_workspace_id = workspace_id.clone();
    let handle = tokio::spawn(async move {
      let mime = mime::TEXT_PLAIN_UTF_8;
      let file_id = uuid::Uuid::new_v4().to_string();
      let url = cloned_client.get_blob_url(&cloned_workspace_id, &file_id);
      let data = vec![0; 3 * 1024 * 1024];
      cloned_client.put_blob(&url, data, &mime).await.unwrap();
      url
    });
    handles.push(handle);
  }

  let results = futures::future::join_all(handles).await;
  for result in results {
    let url = result.unwrap();
    let (_, got_data) = c1.get_blob(&url).await.unwrap();
    assert_eq!(got_data, vec![0; 3 * 1024 * 1024]);
    c1.delete_blob(&url).await.unwrap();
  }
}
