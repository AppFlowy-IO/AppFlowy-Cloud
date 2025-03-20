use super::TestBucket;
use crate::collab::util::{generate_random_bytes, generate_random_string};
use app_error::ErrorCode;
use appflowy_cloud::api::file_storage::BlobPathV1;
use aws_sdk_s3::types::CompletedPart;
use bytes::Bytes;
use client_api_test::{generate_unique_registered_user_client, workspace_id_from_client};
use database::file::{BlobKey, BucketClient, ResponseBlob};
use database_entity::file_dto::{
  CompleteUploadRequest, CompletedPartRequest, CreateUploadRequest, UploadPartData,
};
use infra::file_util::ChunkedBytes;
use uuid::Uuid;

#[tokio::test]
async fn multiple_part_put_and_get_test() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let parent_dir = workspace_id.to_string();
  let mime = mime::TEXT_PLAIN_UTF_8;
  let text = generate_random_string(8 * 1024 * 1024);
  let file_id = Uuid::new_v4().to_string();

  let upload = c1
    .create_upload(
      &workspace_id,
      CreateUploadRequest {
        file_id: file_id.clone(),
        parent_dir: parent_dir.to_string(),
        content_type: mime.to_string(),
        file_size: Some(text.len() as u64),
      },
    )
    .await
    .unwrap();
  let mut chunked_bytes = ChunkedBytes::from_bytes(Bytes::from(text.clone())).unwrap();
  assert_eq!(chunked_bytes.offsets.len(), 2);
  chunked_bytes.set_chunk_size(5 * 1024 * 1024).unwrap();

  let mut completed_parts = Vec::new();
  let iter = chunked_bytes.iter().enumerate();
  for (index, next) in iter {
    let resp = c1
      .upload_part(
        &workspace_id,
        &parent_dir,
        &file_id,
        &upload.upload_id,
        index as i32 + 1,
        next.to_vec(),
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
    parent_dir: parent_dir.clone(),
    upload_id: upload.upload_id,
    parts: completed_parts,
  };
  c1.complete_upload(&workspace_id, req).await.unwrap();

  let blob = c1
    .get_blob_v1(&workspace_id, &parent_dir, &file_id)
    .await
    .unwrap()
    .1;

  let blob_text = String::from_utf8(blob.to_vec()).unwrap();
  assert_eq!(blob_text, text);
}

#[tokio::test]
async fn single_part_put_and_get_test() {
  // Test with smaller file (single part)
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let text = generate_random_string(1024);
  let file_id = Uuid::new_v4().to_string();

  let upload = c1
    .create_upload(
      &workspace_id,
      CreateUploadRequest {
        file_id: file_id.clone(),
        parent_dir: workspace_id.to_string(),
        content_type: mime.to_string(),
        file_size: Some(text.len() as u64),
      },
    )
    .await
    .unwrap();

  let chunked_bytes = ChunkedBytes::from_bytes(Bytes::from(text.clone())).unwrap();
  assert_eq!(chunked_bytes.offsets.len(), 1);

  let mut completed_parts = Vec::new();
  let iter = chunked_bytes.iter().enumerate();
  for (index, next) in iter {
    let resp = c1
      .upload_part(
        &workspace_id,
        &workspace_id.to_string(),
        &file_id,
        &upload.upload_id,
        index as i32 + 1,
        next.to_vec(),
      )
      .await
      .unwrap();

    completed_parts.push(CompletedPartRequest {
      e_tag: resp.e_tag,
      part_number: resp.part_num,
    });
  }
  assert_eq!(completed_parts.len(), 1);
  assert_eq!(completed_parts[0].part_number, 1);

  let req = CompleteUploadRequest {
    file_id: file_id.clone(),
    parent_dir: workspace_id.to_string(),
    upload_id: upload.upload_id,
    parts: completed_parts,
  };
  c1.complete_upload(&workspace_id, req).await.unwrap();

  let blob = c1
    .get_blob_v1(&workspace_id, &workspace_id.to_string(), &file_id)
    .await
    .unwrap()
    .1;
  let blob_text = String::from_utf8(blob.to_vec()).unwrap();
  assert_eq!(blob_text, text);
}

#[tokio::test]
async fn empty_part_upload_test() {
  // Test with empty part
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let file_id = Uuid::new_v4().to_string();

  let upload = c1
    .create_upload(
      &workspace_id,
      CreateUploadRequest {
        file_id: file_id.clone(),
        parent_dir: workspace_id.to_string(),
        content_type: mime.to_string(),
        file_size: Some(0),
      },
    )
    .await
    .unwrap();

  let result = c1
    .upload_part(&workspace_id, "", &file_id, &upload.upload_id, 1, vec![])
    .await
    .unwrap_err();
  assert_eq!(result.code, ErrorCode::InvalidRequest)
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
  let file_id = Uuid::new_v4().to_string();
  let workspace_id = Uuid::new_v4();
  let parent_dir = workspace_id.to_string();

  let req = CreateUploadRequest {
    file_id: file_id.clone(),
    parent_dir: parent_dir.clone(),
    content_type: "text".to_string(),
    file_size: Some(file_size as u64),
  };

  let key = BlobPathV1 {
    workspace_id,
    parent_dir: parent_dir.clone(),
    file_id,
  };
  let upload = test_bucket
    .create_upload(&key.object_key(), req)
    .await
    .unwrap();

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

    let req = UploadPartData {
      file_id: upload.file_id.clone(),
      upload_id: upload.upload_id.clone(),
      part_number,
      body: chunk.to_vec(),
    };
    let key = BlobPathV1 {
      workspace_id,
      parent_dir: parent_dir.clone(),
      file_id: upload.file_id.clone(),
    };
    let resp = test_bucket
      .upload_part(&key.object_key(), req)
      .await
      .unwrap();

    completed_parts.push(
      CompletedPart::builder()
        .e_tag(resp.e_tag)
        .part_number(resp.part_num)
        .build(),
    );
  }

  let complete_req = CompleteUploadRequest {
    file_id: upload.file_id.clone(),
    parent_dir: parent_dir.clone(),
    upload_id: upload.upload_id.clone(),
    parts: completed_parts
      .into_iter()
      .map(|p| CompletedPartRequest {
        e_tag: p.e_tag().unwrap().to_string(),
        part_number: p.part_number.unwrap(),
      })
      .collect(),
  };

  let key = BlobPathV1 {
    workspace_id,
    parent_dir: parent_dir.clone(),
    file_id: upload.file_id.clone(),
  };
  test_bucket
    .complete_upload(&key.object_key(), complete_req)
    .await
    .unwrap();

  // Verify the upload
  let object = test_bucket.get_blob(&key.object_key()).await.unwrap();
  assert_eq!(object.len(), file_size, "Failed for {}", description);
  assert_eq!(object.to_blob(), blob, "Failed for {}", description);
}

#[tokio::test]
async fn invalid_test() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let parent_dir = workspace_id.clone();
  let file_id = uuid::Uuid::new_v4().to_string();
  let mime = mime::TEXT_PLAIN_UTF_8;

  // test invalid create upload request
  for request in [
    CreateUploadRequest {
      file_id: "".to_string(),
      parent_dir: parent_dir.clone(),
      content_type: mime.to_string(),
      file_size: Some(0),
    },
    CreateUploadRequest {
      file_id: file_id.clone(),
      parent_dir: "".to_string(),
      content_type: mime.to_string(),
      file_size: Some(0),
    },
  ] {
    let err = c1.create_upload(&workspace_id, request).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidRequest);
  }

  // test invalid upload part request
  let upload_id = uuid::Uuid::new_v4().to_string();
  for request in vec![
    // workspace_id, parent_dir, file_id, upload_id, part_number, body
    (
      "".to_string(),
      parent_dir.clone(),
      file_id.clone(),
      upload_id.clone(),
      1,
      vec![1, 2, 3],
    ),
    (
      workspace_id.to_string(),
      Uuid::default(),
      file_id.clone(),
      upload_id.clone(),
      1,
      vec![1, 2, 3],
    ),
    (
      workspace_id.to_string(),
      parent_dir.clone(),
      "".to_string(),
      upload_id.clone(),
      1,
      vec![1, 2, 3],
    ),
  ] {
    let err = c1
      .upload_part(
        &request.0, &request.1, &request.2, &request.3, request.4, request.5,
      )
      .await
      .unwrap_err();
    assert_eq!(err.code, ErrorCode::Internal);
  }
}

#[tokio::test]
async fn multiple_level_dir_upload_file_test() {
  // Test with smaller file (single part)
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let text = generate_random_string(1024);
  let file_id = Uuid::new_v4().to_string();
  let parent_dir = "file/v1/image".to_string();
  let upload = c1
    .create_upload(
      &workspace_id,
      CreateUploadRequest {
        file_id: file_id.clone(),
        parent_dir: parent_dir.clone(),
        content_type: mime.to_string(),
        file_size: Some(text.len() as u64),
      },
    )
    .await
    .unwrap();
  let chunked_bytes = ChunkedBytes::from_bytes(Bytes::from(text.clone())).unwrap();
  let mut completed_parts = Vec::new();
  let iter = chunked_bytes.iter().enumerate();
  for (index, next) in iter {
    let resp = c1
      .upload_part(
        &workspace_id,
        &parent_dir,
        &file_id,
        &upload.upload_id,
        index as i32 + 1,
        next.to_vec(),
      )
      .await
      .unwrap();

    completed_parts.push(CompletedPartRequest {
      e_tag: resp.e_tag,
      part_number: resp.part_num,
    });
  }
  let req = CompleteUploadRequest {
    file_id: file_id.clone(),
    parent_dir: parent_dir.clone(),
    upload_id: upload.upload_id,
    parts: completed_parts,
  };
  c1.complete_upload(&workspace_id, req).await.unwrap();

  let blob = c1
    .get_blob_v1(&workspace_id, &parent_dir, &file_id)
    .await
    .unwrap()
    .1;

  let blob_text = String::from_utf8(blob.to_vec()).unwrap();
  assert_eq!(blob_text, text);
}
