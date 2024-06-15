use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct CreateUploadRequest {
  pub key: String,
}

#[derive(Serialize, Deserialize)]
pub struct CreateUploadResponse {
  pub key: String,
  pub upload_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct UploadPartRequest {
  pub key: String,
  pub upload_id: String,
  pub part_number: i32,
  pub body: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct UploadPartResponse {
  pub e_tag: String,
  pub part_num: i32,
}

#[derive(Serialize, Deserialize)]
pub struct CompleteUploadRequest {
  pub key: String,
  pub upload_id: String,
  pub parts: Vec<CompletedPartRequest>,
}

#[derive(Serialize, Deserialize)]
pub struct CompletedPartRequest {
  pub e_tag: String,
  pub part_number: i32,
}

#[derive(Serialize, Deserialize)]
pub struct CompleteUploadResponse {
  pub key: String,
  pub upload_id: String,
  pub parts: Vec<CompletedPartRequest>,
}
