use serde::{Deserialize, Serialize};
use std::fmt::Display;

#[derive(Serialize, Deserialize)]
pub struct CreateUploadRequest {
  pub file_id: String,
  pub content_type: String,
}

impl Display for CreateUploadRequest {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "CreateUploadRequest: file_id: {}, content_type: {}",
      self.file_id, self.content_type
    )
  }
}

#[derive(Serialize, Deserialize)]
pub struct CreateUploadResponse {
  pub file_id: String,
  pub upload_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct UploadPartRequest {
  pub file_id: String,
  pub upload_id: String,
  pub part_number: i32,
  pub body: Vec<u8>,
}

impl Display for UploadPartRequest {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "UploadPartRequest: file_id: {}, upload_id: {}, part_number: {}, size:{}",
      self.file_id,
      self.upload_id,
      self.part_number,
      self.body.len()
    )
  }
}

#[derive(Serialize, Deserialize)]
pub struct UploadPartResponse {
  pub e_tag: String,
  pub part_num: i32,
}

#[derive(Serialize, Deserialize)]
pub struct CompleteUploadRequest {
  pub file_id: String,
  pub upload_id: String,
  pub parts: Vec<CompletedPartRequest>,
}

impl Display for CompleteUploadRequest {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "CompleteUploadRequest: file_id: {}, upload_id: {}, parts: {}",
      self.file_id,
      self.upload_id,
      self.parts.len()
    )
  }
}

#[derive(Serialize, Deserialize)]
pub struct CompletedPartRequest {
  pub e_tag: String,
  pub part_number: i32,
}

#[derive(Serialize, Deserialize)]
pub struct CompleteUploadResponse {
  pub file_id: String,
  pub upload_id: String,
  pub parts: Vec<CompletedPartRequest>,
}
