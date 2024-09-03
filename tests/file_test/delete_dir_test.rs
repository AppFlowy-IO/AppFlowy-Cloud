use crate::collab::util::generate_random_string;
use app_error::ErrorCode;
use bytes::Bytes;
use client_api::ChunkedBytes;
use client_api_test::{generate_unique_registered_user_client, workspace_id_from_client};
use database_entity::file_dto::{CompleteUploadRequest, CompletedPartRequest, CreateUploadRequest};
use uuid::Uuid;

#[tokio::test]
async fn delete_workspace_resource_test() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let mime = mime::TEXT_PLAIN_UTF_8;
  let data = "hello world";
  let file_id = uuid::Uuid::new_v4().to_string();
  let url = c1.get_blob_url(&workspace_id, &file_id);
  c1.put_blob(&url, data, &mime).await.unwrap();
  c1.delete_workspace(&workspace_id).await.unwrap();

  let error = c1.get_blob(&url).await.unwrap_err();
  assert_eq!(error.code, ErrorCode::RecordNotFound);
}

#[tokio::test]
async fn delete_workspace_sub_folder_resource_test() {
  let (c1, _user1) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c1).await;
  let parent_dir = format!("SubFolder:{}", uuid::Uuid::new_v4());
  let mime = mime::TEXT_PLAIN_UTF_8;
  let mut file_ids = vec![];

  for i in 1..5 {
    let text = generate_random_string(i * 2 * 1024 * 1024);
    let file_id = Uuid::new_v4().to_string();
    file_ids.push(file_id.clone());
    let upload = c1
      .create_upload(
        &workspace_id,
        CreateUploadRequest {
          file_id: file_id.clone(),
          parent_dir: parent_dir.clone(),
          content_type: mime.to_string(),
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

    let url = c1.get_blob_url_v1(&workspace_id, &parent_dir, &file_id);
    let (workspace_id_2, parent_dir_2, file_id_2) = c1.parse_blob_url_v1(&url).unwrap();
    assert_eq!(workspace_id, workspace_id_2);
    assert_eq!(parent_dir, parent_dir_2);
    assert_eq!(file_id, file_id_2);
    assert_eq!(blob_text, text);
  }
  c1.delete_workspace(&workspace_id).await.unwrap();

  for file_id in file_ids {
    let error = c1
      .get_blob_v1(&workspace_id, &parent_dir, &file_id)
      .await
      .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecordNotFound);
  }
}
