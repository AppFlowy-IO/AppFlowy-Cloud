use actix_http::body::BoxBody;
use actix_web::http::header::{
  ContentLength, ContentType, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE,
  LAST_MODIFIED,
};
use actix_web::web::{Json, Payload};
use actix_web::{
  web::{self, Data},
  HttpRequest, Scope,
};
use actix_web::{HttpResponse, Result};
use chrono::DateTime;
use database::file::MAX_BLOB_SIZE;
use database_entity::AFBlobRecord;
use serde::Deserialize;
use shared_entity::app_error::AppError;
use shared_entity::data::{AppResponse, JsonAppResponse};
use shared_entity::error_code::ErrorCode;
use sqlx::types::Uuid;
use std::pin::Pin;
use tokio::io::AsyncRead;
use tokio_stream::StreamExt;
use tokio_util::io::StreamReader;
use tracing::{instrument, trace};
use tracing_actix_web::RequestId;

use crate::state::AppState;

pub fn file_storage_scope() -> Scope {
  web::scope("/api/file_storage")
    .service(web::resource("/{workspace_id}").route(web::put().to(put_handler)))
    .service(
      web::resource("/{workspace_id}/{file_id:.*}")
        .route(web::get().to(get_handler))
        .route(web::delete().to(delete_handler)),
    )
}

#[derive(Deserialize, Debug)]
pub struct BlobPathInfo {
  workspace_id: Uuid,
  file_id: String,
}

#[instrument(skip(state, payload), err)]
async fn put_handler(
  state: Data<AppState>,
  payload: Payload,
  content_type: web::Header<ContentType>,
  content_length: web::Header<ContentLength>,
  workspace_id: web::Path<Uuid>,
  required_id: RequestId,
) -> Result<JsonAppResponse<AFBlobRecord>> {
  let content_length = content_length.into_inner().into_inner();
  // Check content length, if it's too large, return error.
  if content_length > MAX_BLOB_SIZE {
    return Ok(
      AppResponse::new(
        ErrorCode::PayloadTooLarge,
        "The uploading file is too large".to_string(),
      )
      .into(),
    );
  }
  let file_type = content_type.into_inner().0.to_string();
  let blob_stream = payload_to_async_read(payload);
  let workspace_id = workspace_id.into_inner();

  trace!("start put blob: {}:{}", file_type, content_length);
  let file_id = state
    .bucket_storage
    .put_blob(blob_stream, workspace_id, file_type, content_length as i64)
    .await
    .map_err(AppError::from)?;

  let record = AFBlobRecord::new(file_id);
  trace!("did put blob: {:?}", record);
  Ok(Json(AppResponse::Ok().with_data(record)))
}

#[instrument(level = "debug", skip(state), err)]
async fn delete_handler(
  state: Data<AppState>,
  path: web::Path<BlobPathInfo>,
) -> Result<JsonAppResponse<()>> {
  let BlobPathInfo {
    workspace_id,
    file_id,
  } = path.into_inner();
  state
    .bucket_storage
    .delete_blob(&workspace_id, &file_id)
    .await
    .map_err(AppError::from)?;
  Ok(AppResponse::Ok().into())
}

#[instrument(skip(state), err)]
async fn get_handler(
  state: Data<AppState>,
  path: web::Path<BlobPathInfo>,
  required_id: RequestId,
  req: HttpRequest,
) -> Result<HttpResponse<BoxBody>> {
  let BlobPathInfo {
    workspace_id,
    file_id,
  } = path.into_inner();

  // Get the metadata
  let result = state
    .bucket_storage
    .get_blob_metadata(&workspace_id, &file_id)
    .await;

  if let Err(err) = result.as_ref() {
    return if err.is_not_found() {
      Ok(HttpResponse::NotFound().finish())
    } else {
      Ok(HttpResponse::InternalServerError().finish())
    };
  }

  let metadata = result.unwrap();
  // Check if the file is modified since the last time
  if let Some(modified_since) = req
    .headers()
    .get(IF_MODIFIED_SINCE)
    .and_then(|h| h.to_str().ok())
    .and_then(|s| DateTime::parse_from_rfc2822(s).ok())
  {
    if metadata.modified_at.naive_utc() <= modified_since.naive_utc() {
      return Ok(HttpResponse::NotModified().finish());
    }
  }
  let blob = state
    .bucket_storage
    .get_blob(&file_id)
    .await
    .map_err(AppError::from)?;

  let response = HttpResponse::Ok()
    .append_header((ETAG, file_id))
    .append_header((CONTENT_TYPE, metadata.file_type))
    .append_header((LAST_MODIFIED, metadata.modified_at.to_rfc2822()))
    .append_header((CONTENT_LENGTH, blob.len()))
    .append_header((CACHE_CONTROL, "public, immutable, max-age=31536000"))// 31536000 seconds = 1 year
    .body(blob);

  Ok(response)
}

fn payload_to_async_read(payload: actix_web::web::Payload) -> Pin<Box<dyn AsyncRead>> {
  let mapped =
    payload.map(|chunk| chunk.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)));
  let reader = StreamReader::new(mapped);
  Box::pin(reader)
}
