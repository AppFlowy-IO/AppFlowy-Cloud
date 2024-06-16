use super::TestBucket;
use crate::collab::util::{generate_random_bytes, generate_random_string};
use aws_sdk_s3::types::CompletedPart;
use bytes::Bytes;
use client_api::http_blob::ChunkedBytes;
use client_api_test::{generate_unique_registered_user_client, workspace_id_from_client};
use database::file::{BucketClient, ResponseBlob};
use database_entity::file_dto::{
  CompleteUploadRequest, CompletedPartRequest, CreateUploadRequest, UploadPartRequest,
};
use uuid::Uuid;

#[tokio::test]
async fn multiple_part_put_and_get_test() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let text = generate_random_string(8 * 1024 * 1024);
  let file_id = uuid::Uuid::new_v4().to_string();

  let upload = c1
    .create_multi_upload(
      &workspace_id,
      CreateUploadRequest {
        file_id: file_id.clone(),
        content_type: mime.to_string(),
      },
    )
    .await
    .unwrap();
  let chunked_bytes = ChunkedBytes::from_bytes(Bytes::from(text.clone())).unwrap();
  assert_eq!(chunked_bytes.offsets.len(), 2);

  let mut completed_parts = Vec::new();
  let mut iter = chunked_bytes.iter().enumerate();
  while let Some((index, next)) = iter.next() {
    let resp = c1
      .upload_part(
        &workspace_id,
        UploadPartRequest {
          file_id: file_id.clone(),
          upload_id: upload.upload_id.clone(),
          part_number: index as i32 + 1,
          body: next.to_vec(),
        },
      )
      .await
      .unwrap();

    completed_parts.push(CompletedPartRequest {
      e_tag: resp.e_tag,
      part_number: resp.part_num,
    });
  }

  assert_eq!(completed_parts.len(), 2);
  assert_eq!(completed_parts[0].part_number, 1);
  assert_eq!(completed_parts[1].part_number, 2);

  let req = CompleteUploadRequest {
    file_id: file_id.clone(),
    upload_id: upload.upload_id,
    parts: completed_parts,
  };
  c1.complete_upload(&workspace_id, req).await.unwrap();

  let url = c1.get_blob_url(&workspace_id, &file_id);
  let blob = c1.get_blob(&url).await.unwrap().1;
  let blob_text = String::from_utf8(blob.to_vec()).unwrap();
  assert_eq!(blob_text, text);
}
#[tokio::test]
async fn multiple_part_upload_test() {
  let test_bucket = TestBucket::new().await;

  // Test with a payload of less than 5MB
  let small_file_size = 4 * 1024 * 1024; // 4 MB
  let small_blob = generate_random_bytes(small_file_size);
  perform_upload_test(&test_bucket, small_blob, small_file_size, "small_file").await;

  // Test with a payload of exactly 10MB
  let file_size = 10 * 1024 * 1024; // 10 MB
  let blob = generate_random_bytes(file_size);
  perform_upload_test(&test_bucket, blob, file_size, "large_file").await;

  // Test with a payload of exactly 20MB
  let file_size = 20 * 1024 * 1024; // 20 MB
  let blob = generate_random_bytes(file_size);
  perform_upload_test(&test_bucket, blob, file_size, "large_file").await;
}

#[tokio::test]
#[should_panic]
async fn multiple_part_upload_empty_data_test() {
  let test_bucket = TestBucket::new().await;
  let empty_blob = Vec::new();
  perform_upload_test(&test_bucket, empty_blob, 0, "empty_file").await;
}

async fn perform_upload_test(
  test_bucket: &TestBucket,
  blob: Vec<u8>,
  file_size: usize,
  description: &str,
) {
  let chunk_size = 5 * 1024 * 1024; // 5 MB

  let req = CreateUploadRequest {
    file_id: Uuid::new_v4().to_string(),
    content_type: "text".to_string(),
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
      file_id: upload.file_id.clone(),
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
    file_id: upload.file_id.clone(),
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
  let object = test_bucket.get_blob(&upload.file_id).await.unwrap();
  assert_eq!(object.len(), file_size, "Failed for {}", description);
  assert_eq!(object.to_blob(), blob, "Failed for {}", description);
}
