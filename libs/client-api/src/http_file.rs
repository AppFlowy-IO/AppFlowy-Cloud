use crate::ws::{ConnectInfo, WSClientConnectURLProvider, WSClientHttpSender, WSError};
use crate::{process_response_data, process_response_error, Client};

use app_error::AppError;
use async_trait::async_trait;
use std::fs::metadata;

use client_api_entity::{
  CompleteUploadRequest, CreateUploadRequest, CreateUploadResponse, UploadPartResponse,
};
use client_api_entity::{CreateImportTask, CreateImportTaskResponse};

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::{multipart, Body, Method};
use shared_entity::response::AppResponseError;
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use shared_entity::dto::import_dto::UserImportTask;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::codec::{BytesCodec, FramedRead};
use tracing::{error, trace};
use uuid::Uuid;

impl Client {
  pub async fn create_upload(
    &self,
    workspace_id: &Uuid,
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
    process_response_data::<CreateUploadResponse>(resp).await
  }

  /// Upload a part of a file. The part number should be 1-based.
  ///
  /// In Amazon S3, the minimum chunk size for multipart uploads is 5 MB,except for the last part,
  /// which can be smaller.(https://docs.aws.amazon.com/AmazonS3/latest/userguide/qfacts.html)
  pub async fn upload_part(
    &self,
    workspace_id: &Uuid,
    parent_dir: &str,
    file_id: &str,
    upload_id: &str,
    part_number: i32,
    body: Vec<u8>,
  ) -> Result<UploadPartResponse, AppResponseError> {
    if body.is_empty() {
      return Err(AppResponseError::from(AppError::InvalidRequest(
        "Empty body".to_string(),
      )));
    }

    // Encode the parent directory to ensure it's URL-safe.
    let parent_dir = utf8_percent_encode(parent_dir, NON_ALPHANUMERIC).to_string();
    let url = format!(
            "{}/api/file_storage/{workspace_id}/upload_part/{parent_dir}/{file_id}/{upload_id}/{part_number}",
            self.base_url
        );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .body(body)
      .send()
      .await?;
    process_response_data::<UploadPartResponse>(resp).await
  }

  pub async fn complete_upload(
    &self,
    workspace_id: &Uuid,
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
    process_response_error(resp).await
  }

  /// Sends a POST request to import a file to the server.
  ///
  /// This function streams the contents of a file located at the provided `file_path`
  /// as part of a multipart form data request to the server's `/api/import` endpoint.
  ///
  /// ### HTTP Request Details:
  ///
  /// - **Method:** POST
  /// - **URL:** `{base_url}/api/import`
  ///   - The `base_url` is dynamically provided and appended with `/api/import`.
  ///
  /// - **Headers:**
  ///   - `X-Host`: The value of the `base_url` is sent as the host header.
  ///   - `X-Content-Length`: The size of the file, in bytes, is provided from the file's metadata.
  ///
  /// - **Multipart Form:**
  ///   - The file is sent as a multipart form part:
  ///     - **Field Name:** The file name derived from the file path or a UUID if unavailable.
  ///     - **File Content:** The file's content is streamed using `reqwest::Body::wrap_stream`.
  ///     - **MIME Type:** Guessed from the file's extension using the `mime_guess` crate,
  ///       defaulting to `application/octet-stream` if undetermined.
  ///
  /// ### Parameters:
  /// - `file_path`: The path to the file to be uploaded.
  ///   - The file is opened asynchronously and its metadata (like size) is extracted.
  /// - The MIME type is automatically determined based on the file extension using `mime_guess`.
  ///
  pub async fn import_file(&self, file_path: &Path) -> Result<(), AppResponseError> {
    let md5_base64 = calculate_md5(file_path).await?;
    let file = File::open(&file_path).await?;
    let metadata = file.metadata().await?;
    let file_name = file_path
      .file_stem()
      .map(|s| s.to_string_lossy().to_string())
      .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let stream = FramedRead::new(file, BytesCodec::new());
    let mime = mime_guess::from_path(file_path)
      .first_or_octet_stream()
      .to_string();

    let file_part = multipart::Part::stream(reqwest::Body::wrap_stream(stream))
      .file_name(file_name.clone())
      .mime_str(&mime)?;

    let form = multipart::Form::new().part(file_name, file_part);
    let url = format!("{}/api/import", self.base_url);
    let mut builder = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .multipart(form);

    // set the host header
    builder = builder
      .header("X-Host", self.base_url.clone())
      .header("X-Content-MD5", md5_base64)
      .header("X-Content-Length", metadata.len());
    let resp = builder.send().await?;

    process_response_error(resp).await
  }

  /// Creates an import task for a file and returns the import task response.
  ///
  /// This function initiates an import task by sending a POST request to the
  /// `/api/import/create` endpoint. The request includes the `workspace_name` derived
  /// from the provided file's name (or a generated UUID if the file name cannot be determined).
  ///
  /// After creating the import task, you should use [Self::upload_import_file] to upload
  /// the actual file to the presigned URL obtained from the [CreateImportTaskResponse].
  ///
  pub async fn create_import(
    &self,
    file_path: &Path,
  ) -> Result<CreateImportTaskResponse, AppResponseError> {
    let url = format!("{}/api/import/create", self.base_url);
    let file_name = file_path
      .file_stem()
      .map(|s| s.to_string_lossy().to_string())
      .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let content_length = tokio::fs::metadata(file_path).await?.len();
    let params = CreateImportTask {
      workspace_name: file_name.clone(),
      content_length,
    };
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .header("X-Host", self.base_url.clone())
      .json(&params)
      .send()
      .await?;

    process_response_data::<CreateImportTaskResponse>(resp).await
  }

  /// Uploads a file to a specified presigned URL obtained from the import task response.
  ///
  /// This function uploads a file to the given presigned URL using an HTTP PUT request.
  /// The file's metadata is read to determine its size, and the upload stream is created
  /// and sent to the provided URL. It is recommended to call this function after successfully
  /// creating an import task using [Self::create_import].
  ///
  pub async fn upload_import_file(
    &self,
    file_path: &Path,
    url: &str,
  ) -> Result<(), AppResponseError> {
    let file_metadata = metadata(file_path)?;
    let file_size = file_metadata.len();
    // Open the file
    let file = File::open(file_path).await?;
    let file_stream = FramedRead::new(file, BytesCodec::new());
    let stream_body = Body::wrap_stream(file_stream);
    trace!("start upload file to s3: {}", url);

    let client = reqwest::Client::new();
    let upload_resp = client
      .put(url)
      .header("Content-Length", file_size)
      .header("Content-Type", "application/zip")
      .body(stream_body)
      .send()
      .await?;

    if !upload_resp.status().is_success() {
      error!("File upload failed: {:?}", upload_resp);
      return Err(AppError::S3ResponseError("Cannot upload file to S3".to_string()).into());
    }

    Ok(())
  }

  pub async fn get_import_list(&self) -> Result<UserImportTask, AppResponseError> {
    let url = format!("{}/api/import", self.base_url);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;

    process_response_data::<UserImportTask>(resp).await
  }
}

#[async_trait]
impl WSClientHttpSender for Client {
  async fn send_ws_msg(
    &self,
    device_id: &str,
    message: client_websocket::Message,
  ) -> Result<(), WSError> {
    self
      .post_realtime_msg(device_id, message)
      .await
      .map_err(|err| WSError::Http(err.to_string()))
  }
}

#[async_trait]
impl WSClientConnectURLProvider for Client {
  fn connect_ws_url(&self) -> String {
    self.ws_addr.clone()
  }

  async fn connect_info(&self) -> Result<ConnectInfo, WSError> {
    let conn_info = self
      .ws_connect_info(true)
      .await
      .map_err(|err| WSError::Http(err.to_string()))?;
    Ok(conn_info)
  }
}

/// Calculates the MD5 hash of a file and returns the base64-encoded MD5 digest.
///
/// # Arguments
/// * `file_path` - The path of the file for which the MD5 hash is to be calculated.
///
/// # Returns
/// A `Result` containing the base64-encoded MD5 hash on success, or an error if the file cannot be read.
///
/// Asynchronously calculates the MD5 hash of a file using efficient buffer handling and returns it as a base64-encoded string.
///
/// # Arguments
/// * `file_path` - The path to the file to be hashed.
///
/// # Returns
/// Returns a `Result` containing the base64-encoded MD5 hash on success, or an error if the file cannot be read.
pub async fn calculate_md5(file_path: &Path) -> Result<String, anyhow::Error> {
  let file = File::open(file_path).await?;
  let mut reader = BufReader::with_capacity(1_000_000, file);
  let mut context = md5::Context::new();
  loop {
    let part = reader.fill_buf().await?;
    if part.is_empty() {
      break;
    }

    context.consume(part);
    let part_len = part.len();
    reader.consume(part_len);
  }

  let md5_hash = context.compute();
  let md5_base64 = STANDARD.encode(md5_hash.as_ref());
  Ok(md5_base64)
}
