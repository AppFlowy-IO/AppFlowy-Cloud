use serde::{Deserialize, Serialize};
use std::fmt::Display;

pub trait FileDir {
  fn directory(&self) -> &str;
  fn file_id(&self) -> &str;

  fn object_key(&self) -> String {
    if self.directory().is_empty() {
      self.file_id().to_string()
    } else {
      format!("{}/{}", self.directory(), self.file_id())
    }
  }
}

#[derive(Serialize, Deserialize)]
pub struct CreateUploadRequest {
  pub file_id: String,
  pub directory: String,
  pub content_type: String,
}

impl FileDir for CreateUploadRequest {
  fn directory(&self) -> &str {
    &self.directory
  }

  fn file_id(&self) -> &str {
    &self.file_id
  }
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
  pub directory: String,
  pub upload_id: String,
  pub part_number: i32,
  pub body: Vec<u8>,
}

impl FileDir for UploadPartRequest {
  fn directory(&self) -> &str {
    &self.directory
  }

  fn file_id(&self) -> &str {
    &self.file_id
  }
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

#[derive(Serialize, Deserialize, Debug)]
pub struct UploadPartResponse {
  pub e_tag: String,
  pub part_num: i32,
}

#[derive(Serialize, Deserialize)]
pub struct CompleteUploadRequest {
  pub file_id: String,
  pub directory: String,
  pub upload_id: String,
  pub parts: Vec<CompletedPartRequest>,
}

impl FileDir for CompleteUploadRequest {
  fn directory(&self) -> &str {
    &self.directory
  }

  fn file_id(&self) -> &str {
    &self.file_id
  }
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
