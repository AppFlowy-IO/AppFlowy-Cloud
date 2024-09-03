use crate::http::log_request_id;
use crate::Client;

use app_error::AppError;
use bytes::Bytes;
use futures_util::TryStreamExt;
use mime::Mime;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use reqwest::{header, Method, StatusCode};
use shared_entity::dto::workspace_dto::{BlobMetadata, RepeatedBlobMetaData};
use shared_entity::response::{AppResponse, AppResponseError};

use tracing::instrument;
use url::Url;

impl Client {
  pub fn get_blob_url(&self, workspace_id: &str, file_id: &str) -> String {
    format!(
      "{}/api/file_storage/{}/blob/{}",
      self.base_url, workspace_id, file_id
    )
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
  pub fn get_blob_url_v1(&self, workspace_id: &str, parent_dir: &str, file_id: &str) -> String {
    let parent_dir = utf8_percent_encode(parent_dir, NON_ALPHANUMERIC).to_string();
    format!(
      "{}/api/file_storage/{workspace_id}/v1/blob/{parent_dir}/{file_id}",
      self.base_url
    )
  }

  /// Returns the workspace_id, parent_dir, and file_id from the given blob url.
  pub fn parse_blob_url_v1(&self, url: &str) -> Option<(String, String, String)> {
    let parsed_url = Url::parse(url).ok()?;
    let segments: Vec<&str> = parsed_url.path_segments()?.collect();
    // Check if the path has the expected number of segments
    if segments.len() < 6 {
      return None;
    }

    // Extract the workspace_id, parent_dir, and file_id from the segments
    let workspace_id = segments[2].to_string();
    let encoded_parent_dir = segments[5].to_string();
    let file_id = segments[6].to_string();

    // Decode the percent-encoded parent_dir
    let parent_dir = percent_decode_str(&encoded_parent_dir)
      .decode_utf8()
      .ok()?
      .to_string();

    Some((workspace_id, parent_dir, file_id))
  }

  #[instrument(level = "info", skip_all)]
  pub async fn get_blob_v1(
    &self,
    workspace_id: &str,
    parent_dir: &str,
    file_id: &str,
  ) -> Result<(Mime, Vec<u8>), AppResponseError> {
    // Encode the parent directory to ensure it's URL-safe.
    let url = self.get_blob_url_v1(workspace_id, parent_dir, file_id);
    self.get_blob(&url).await
  }

  #[instrument(level = "info", skip_all)]
  pub async fn delete_blob_v1(
    &self,
    workspace_id: &str,
    parent_dir: &str,
    file_id: &str,
  ) -> Result<(), AppResponseError> {
    let parent_dir = utf8_percent_encode(parent_dir, NON_ALPHANUMERIC).to_string();
    let url = format!(
      "{}/api/file_storage/{workspace_id}/v1/blob/{parent_dir}/{file_id}",
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
    parent_dir: &str,
    file_id: &str,
  ) -> Result<BlobMetadata, AppResponseError> {
    let parent_dir = utf8_percent_encode(parent_dir, NON_ALPHANUMERIC).to_string();
    let url = format!(
      "{}/api/file_storage/{workspace_id}/v1/metadata/{parent_dir}/{file_id}",
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
