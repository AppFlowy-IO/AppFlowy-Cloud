use crate::http::log_request_id;
use crate::Client;
use app_error::AppError;
use bytes::Bytes;
use futures_util::StreamExt;
use mime::Mime;
use reqwest::{header, Method, StatusCode};
use shared_entity::dto::workspace_dto::{BlobMetadata, RepeatedBlobMetaData};
use shared_entity::response::{AppResponse, AppResponseError};
use tracing::instrument;

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
      reqwest::StatusCode::OK => {
        // get mime from resp header
        let mime = {
          match resp.headers().get(header::CONTENT_TYPE) {
            Some(v) => match v.to_str() {
              Ok(v) => match v.parse::<Mime>() {
                Ok(v) => v,
                Err(e) => {
                  tracing::error!("failed to parse mime from header: {:?}", e);
                  mime::TEXT_PLAIN
                },
              },
              Err(e) => {
                tracing::error!("failed to get mime from header: {:?}", e);
                mime::TEXT_PLAIN
              },
            },
            None => mime::TEXT_PLAIN,
          }
        };

        let mut stream = resp.bytes_stream();
        let mut acc: Vec<u8> = Vec::new();
        while let Some(raw_bytes) = stream.next().await {
          acc.extend_from_slice(&raw_bytes?);
        }
        Ok((mime, acc))
      },
      reqwest::StatusCode::NOT_FOUND => Err(AppResponseError::from(AppError::RecordNotFound(
        url.to_owned(),
      ))),
      c => Err(AppResponseError::from(AppError::Unhandled(format!(
        "status code: {}, message: {}",
        c,
        resp.text().await?
      )))),
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
