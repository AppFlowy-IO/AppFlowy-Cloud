use crate::http::log_request_id;
use crate::Client;
use anyhow::anyhow;
use app_error::AppError;
use bytes::Bytes;
use database_entity::file_dto::{
  CompleteUploadRequest, CreateUploadRequest, CreateUploadResponse, FileDir, UploadPartRequest,
  UploadPartResponse,
};
use futures_util::TryStreamExt;
use mime::Mime;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::{header, Method, StatusCode};
use shared_entity::dto::workspace_dto::{BlobMetadata, RepeatedBlobMetaData};
use shared_entity::response::{AppResponse, AppResponseError};
use std::ops::Deref;
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use tracing::{instrument, trace};
const CHUNK_SIZE: usize = 5 * 1024 * 1024; // 5 MB
impl Client {
  pub fn get_blob_url(&self, workspace_id: &str, file_id: &str) -> String {
    format!(
      "{}/api/file_storage/{}/blob/{}",
      self.base_url, workspace_id, file_id
    )
  }

  pub async fn create_upload(
    &self,
    workspace_id: &str,
    req: CreateUploadRequest,
  ) -> Result<CreateUploadResponse, AppResponseError> {
    trace!("create_upload: {}", req);
    let url = format!(
      "{}/api/file_storage/{workspace_id}/create_upload",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&req)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<CreateUploadResponse>::from_response(resp)
      .await?
      .into_data()
  }

  /// Upload a part of a file. The part number should be 1-based.
  ///
  /// In Amazon S3, the minimum chunk size for multipart uploads is 5 MB,except for the last part,
  /// which can be smaller.(https://docs.aws.amazon.com/AmazonS3/latest/userguide/qfacts.html)
  pub async fn upload_part(
    &self,
    workspace_id: &str,
    req: UploadPartRequest,
  ) -> Result<UploadPartResponse, AppResponseError> {
    trace!("upload_part: {}", req);
    let file_id = req.file_id;
    let upload_id = req.upload_id;
    let part_number = req.part_number;

    let url = format!(
      "{}/api/file_storage/{workspace_id}/upload_part/{file_id}/{upload_id}/{part_number}",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .body(req.body)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<UploadPartResponse>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn complete_upload(
    &self,
    workspace_id: &str,
    req: CompleteUploadRequest,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/file_storage/{}/complete_upload",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&req)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "info", skip_all)]
  pub async fn put_blob<T: Into<Bytes>>(
    &self,
    url: &str,
    data: T,
    mime: &Mime,
  ) -> Result<(), AppResponseError> {
    let data = data.into();
    let resp = self
      .http_client_with_auth(Method::PUT, url)
      .await?
      .header(header::CONTENT_TYPE, mime.to_string())
      .body(data)
      .send()
      .await?;
    log_request_id(&resp);
    if resp.status() == StatusCode::PAYLOAD_TOO_LARGE {
      return Err(AppResponseError::from(AppError::PayloadTooLarge(
        StatusCode::PAYLOAD_TOO_LARGE.to_string(),
      )));
    }
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  /// Only expose this method for testing
  #[instrument(level = "info", skip_all)]
  #[cfg(debug_assertions)]
  pub async fn put_blob_with_content_length<T: Into<Bytes>>(
    &self,
    url: &str,
    data: T,
    mime: &Mime,
    content_length: usize,
  ) -> Result<crate::entity::AFBlobRecord, AppResponseError> {
    let resp = self
      .http_client_with_auth(Method::PUT, url)
      .await?
      .header(header::CONTENT_TYPE, mime.to_string())
      .header(header::CONTENT_LENGTH, content_length)
      .body(data.into())
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<crate::entity::AFBlobRecord>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "info", skip_all)]
  pub async fn get_blob_v1(
    &self,
    workspace_id: &str,
    dir: &str,
    file_id: &str,
  ) -> Result<(Mime, Vec<u8>), AppResponseError> {
    let object_key = FileDirImpl { dir, file_id }.object_key();
    let file_id = utf8_percent_encode(&object_key, NON_ALPHANUMERIC).to_string();
    let url = format!(
      "{}/api/file_storage/{workspace_id}/v1/blob/{file_id}",
      self.base_url
    );
    self.get_blob(&url).await
  }

  #[instrument(level = "info", skip_all)]
  pub async fn delete_blob_v1(
    &self,
    workspace_id: &str,
    dir: &str,
    file_id: &str,
  ) -> Result<(), AppResponseError> {
    let object_key = FileDirImpl { dir, file_id }.object_key();
    let file_id = utf8_percent_encode(&object_key, NON_ALPHANUMERIC).to_string();

    let url = format!(
      "{}/api/file_storage/{workspace_id}/v1/blob/{file_id}",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "info", skip_all)]
  pub async fn get_blob_v1_metadata(
    &self,
    workspace_id: &str,
    dir: &str,
    file_id: &str,
  ) -> Result<BlobMetadata, AppResponseError> {
    let object_key = FileDirImpl { dir, file_id }.object_key();
    let file_id = utf8_percent_encode(&object_key, NON_ALPHANUMERIC).to_string();
    let url = format!(
      "{}/api/file_storage/{workspace_id}/v1/metadata/{file_id}",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;

    log_request_id(&resp);
    AppResponse::<BlobMetadata>::from_response(resp)
      .await?
      .into_data()
  }

  /// Get the file with the given url. The url should be in the format of
  /// `https://appflowy.io/api/file_storage/<workspace_id>/<file_id>`.
  #[instrument(level = "info", skip_all)]
  pub async fn get_blob(&self, url: &str) -> Result<(Mime, Vec<u8>), AppResponseError> {
    let resp = self
      .http_client_with_auth(Method::GET, url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);

    match resp.status() {
      StatusCode::OK => {
        let mime = resp
          .headers()
          .get(header::CONTENT_TYPE)
          .and_then(|v| v.to_str().ok())
          .and_then(|v| v.parse::<Mime>().ok())
          .unwrap_or(mime::TEXT_PLAIN);

        let bytes = resp
          .bytes_stream()
          .try_fold(Vec::new(), |mut acc, chunk| async move {
            acc.extend_from_slice(&chunk);
            Ok(acc)
          })
          .await?;

        Ok((mime, bytes))
      },
      StatusCode::NOT_FOUND => Err(AppResponseError::from(AppError::RecordNotFound(
        url.to_owned(),
      ))),
      status => {
        let message = resp
          .text()
          .await
          .unwrap_or_else(|_| "Unknown error".to_string());
        Err(AppResponseError::from(AppError::Unhandled(format!(
          "status code: {}, message: {}",
          status, message
        ))))
      },
    }
  }

  #[instrument(level = "info", skip_all)]
  pub async fn get_blob_metadata(&self, url: &str) -> Result<BlobMetadata, AppResponseError> {
    let resp = self
      .http_client_with_auth(Method::GET, url)
      .await?
      .send()
      .await?;

    log_request_id(&resp);
    AppResponse::<BlobMetadata>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "info", skip_all)]
  pub async fn delete_blob(&self, url: &str) -> Result<(), AppResponseError> {
    let resp = self
      .http_client_with_auth(Method::DELETE, url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn get_workspace_all_blob_metadata(
    &self,
    workspace_id: &str,
  ) -> Result<RepeatedBlobMetaData, AppResponseError> {
    let url = format!("{}/api/file_storage/{}/blobs", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<RepeatedBlobMetaData>::from_response(resp)
      .await?
      .into_data()
  }
}

pub struct ChunkedBytes {
  pub data: Bytes,
  pub offsets: Vec<(usize, usize)>,
}

impl Deref for ChunkedBytes {
  type Target = Bytes;

  fn deref(&self) -> &Self::Target {
    &self.data
  }
}

impl ChunkedBytes {
  pub fn from_bytes(data: Bytes) -> Result<Self, anyhow::Error> {
    let offsets = split_into_chunks(&data);
    Ok(ChunkedBytes { data, offsets })
  }

  pub async fn from_file(file_path: &PathBuf) -> Result<Self, anyhow::Error> {
    let mut file = tokio::fs::File::open(file_path).await?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await?;
    let data = Bytes::from(buffer);

    let offsets = split_into_chunks(&data);
    Ok(ChunkedBytes { data, offsets })
  }

  pub fn iter(&self) -> ChunkedBytesIterator {
    ChunkedBytesIterator {
      chunked_data: self,
      current_index: 0,
    }
  }
}

pub struct ChunkedBytesIterator<'a> {
  chunked_data: &'a ChunkedBytes,
  current_index: usize,
}
impl<'a> Iterator for ChunkedBytesIterator<'a> {
  type Item = Bytes;

  fn next(&mut self) -> Option<Self::Item> {
    if self.current_index >= self.chunked_data.offsets.len() {
      None
    } else {
      let (start, end) = self.chunked_data.offsets[self.current_index];
      self.current_index += 1;
      Some(self.chunked_data.data.slice(start..end))
    }
  }
}
// Function to split input bytes into several chunks and return offsets
pub fn split_into_chunks(data: &Bytes) -> Vec<(usize, usize)> {
  let mut offsets = Vec::new();
  let mut start = 0;

  while start < data.len() {
    let end = std::cmp::min(start + CHUNK_SIZE, data.len());
    offsets.push((start, end));
    start = end;
  }
  offsets
}

// Function to get chunk data using chunk number
pub async fn get_chunk(
  data: Bytes,
  chunk_number: usize,
  offsets: &[(usize, usize)],
) -> Result<Bytes, anyhow::Error> {
  if chunk_number >= offsets.len() {
    return Err(anyhow!("Chunk number out of range"));
  }

  let (start, end) = offsets[chunk_number];
  let chunk = data.slice(start..end);

  Ok(chunk)
}

struct FileDirImpl<'a> {
  pub dir: &'a str,
  pub file_id: &'a str,
}

impl FileDir for FileDirImpl<'_> {
  fn directory(&self) -> &str {
    self.dir
  }

  fn file_id(&self) -> &str {
    self.file_id
  }
}

#[cfg(test)]
mod tests {
  use crate::http_blob::ChunkedBytes;
  use bytes::Bytes;
  use std::env::temp_dir;
  use tokio::io::AsyncWriteExt;

  #[tokio::test]
  async fn test_chunked_bytes_less_than_chunk_size() {
    let data = Bytes::from(vec![0; 1024 * 1024]); // 1 MB of zeroes
    let chunked_data = ChunkedBytes::from_bytes(data.clone()).unwrap();

    // Check if the offsets are correct
    assert_eq!(chunked_data.offsets.len(), 1); // Should have 1 chunk
    assert_eq!(chunked_data.offsets[0], (0, 1024 * 1024));

    // Check if the data can be iterated correctly
    let mut iter = chunked_data.iter();
    assert_eq!(iter.next().unwrap().len(), 1024 * 1024);
    assert!(iter.next().is_none());
  }

  #[tokio::test]
  async fn test_chunked_bytes_from_bytes() {
    let data = Bytes::from(vec![0; 15 * 1024 * 1024]); // 15 MB of zeroes
    let chunked_data = ChunkedBytes::from_bytes(data.clone()).unwrap();

    // Check if the offsets are correct
    assert_eq!(chunked_data.offsets.len(), 3); // Should have 3 chunks
    assert_eq!(chunked_data.offsets[0], (0, 5 * 1024 * 1024));
    assert_eq!(chunked_data.offsets[1], (5 * 1024 * 1024, 10 * 1024 * 1024));
    assert_eq!(
      chunked_data.offsets[2],
      (10 * 1024 * 1024, 15 * 1024 * 1024)
    );

    // Check if the data can be iterated correctly
    let mut iter = chunked_data.iter();
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert!(iter.next().is_none());
  }

  #[tokio::test]
  async fn test_chunked_bytes_from_file() {
    // Create a temporary file with 15 MB of zeroes
    let mut file_path = temp_dir();
    file_path.push("test_file");

    let mut file = tokio::fs::File::create(&file_path).await.unwrap();
    file.write_all(&vec![0; 15 * 1024 * 1024]).await.unwrap();
    file.flush().await.unwrap();

    // Read the file into ChunkedBytes
    let chunked_data = ChunkedBytes::from_file(&file_path).await.unwrap();

    // Check if the offsets are correct
    assert_eq!(chunked_data.offsets.len(), 3); // Should have 3 chunks
    assert_eq!(chunked_data.offsets[0], (0, 5 * 1024 * 1024));
    assert_eq!(chunked_data.offsets[1], (5 * 1024 * 1024, 10 * 1024 * 1024));
    assert_eq!(
      chunked_data.offsets[2],
      (10 * 1024 * 1024, 15 * 1024 * 1024)
    );

    // Check if the data can be iterated correctly
    let mut iter = chunked_data.iter();
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert_eq!(iter.next().unwrap().len(), 5 * 1024 * 1024);
    assert!(iter.next().is_none());

    // Clean up the temporary file
    tokio::fs::remove_file(file_path).await.unwrap();
  }
}
