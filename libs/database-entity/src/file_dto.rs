use serde::Deserialize;

#[derive(Deserialize)]
struct CreateUploadRequest {
  bucket: String,
  key: String,
}

#[derive(Deserialize)]
struct UploadPartRequest {
  bucket: String,
  key: String,
  upload_id: String,
  part_number: i32,
  body: Vec<u8>,
}

#[derive(Deserialize)]
struct CompleteUploadRequest {
  bucket: String,
  key: String,
  upload_id: String,
  parts: Vec<CompletedPart>,
}

#[derive(Deserialize)]
struct CompletedPart {
  e_tag: String,
  part_number: i32,
}
