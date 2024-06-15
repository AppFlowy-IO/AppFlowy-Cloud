use super::TestBucket;
use crate::collab::util::{generate_random_bytes, generate_random_string};
use app_error::ErrorCode;
use aws_sdk_s3::types::CompletedPart;
use client_api_test::{generate_unique_registered_user_client, workspace_id_from_client};
use database::file::{BucketClient, ResponseBlob};
use database_entity::file_dto::{
  CompleteUploadRequest, CompletedPartRequest, CreateUploadRequest, UploadPartRequest,
};
use uuid::Uuid;

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
    let obj_key = format!("{}/{}", workspace_id, file_id);
    let raw_data = test_bucket.get_blob(&obj_key).await.unwrap().to_blob();
    assert_eq!(blob_to_put, String::from_utf8_lossy(&raw_data));
  }

  // delete workspace
  c1.delete_workspace(&workspace_id).await.unwrap();

  {
    // blob does not exist in the bucket
    let obj_key = format!("{}/{}", workspace_id, file_id);
    let err = test_bucket.get_blob(&obj_key).await.unwrap_err();
    assert!(err.is_record_not_found());
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

#[tokio::test]
async fn multiple_part_upload_test() {
  let file_size = 10 * 1024 * 1024; // 10 MB
  let chunk_size = 5 * 1024 * 1024; // 5 MB
  let blob = generate_random_bytes(file_size);
  let test_bucket = TestBucket::new().await;

  let req = CreateUploadRequest {
    key: Uuid::new_v4().to_string(),
  };
  let upload = test_bucket.create_upload(req).await.unwrap();
  let mut chunk_count = (file_size / chunk_size) + 1;
  let mut size_of_last_chunk = file_size % chunk_size;
  if size_of_last_chunk == 0 {
    size_of_last_chunk = chunk_size;
    chunk_count -= 1;
  }

  let mut completed_parts = Vec::new();
  for chunk_index in 0..chunk_count {
    let start = chunk_index * chunk_size;
    let end = start
      + if chunk_index == chunk_count - 1 {
        size_of_last_chunk
      } else {
        chunk_size
      };

    let chunk = &blob[start..end];
    let part_number = (chunk_index + 1) as i32;

    let req = UploadPartRequest {
      key: upload.key.clone(),
      upload_id: upload.upload_id.clone(),
      part_number,
      body: chunk.to_vec(),
    };
    let resp = test_bucket.upload_part(req).await.unwrap();

    completed_parts.push(
      CompletedPart::builder()
        .e_tag(resp.e_tag)
        .part_number(resp.part_num)
        .build(),
    );
  }

  let complete_req = CompleteUploadRequest {
    key: upload.key.clone(),
    upload_id: upload.upload_id.clone(),
    parts: completed_parts
      .into_iter()
      .map(|p| CompletedPartRequest {
        e_tag: p.e_tag().unwrap().to_string(),
        part_number: p.part_number.unwrap(),
      })
      .collect(),
  };
  test_bucket.complete_upload(complete_req).await.unwrap();

  // Verify the upload
  let object = test_bucket.get_blob(&upload.key).await.unwrap();
  assert_eq!(object.len(), file_size);
  assert_eq!(object.to_blob(), blob);
}
