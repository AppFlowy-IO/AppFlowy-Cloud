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
use app_error::AppError;
use chrono::DateTime;
use database::resource_usage::{get_all_workspace_blob_metadata, get_workspace_usage_size};
use shared_entity::dto::workspace_dto::{BlobMetadata, RepeatedBlobMetaData, WorkspaceSpaceUsage};
use shared_entity::response::{AppResponse, AppResponseError, JsonAppResponse};
use sqlx::types::Uuid;
use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_stream::StreamExt;
use tokio_util::io::StreamReader;
use tracing::{error, event, instrument};

use crate::state::AppState;

pub fn file_storage_scope() -> Scope {
  web::scope("/api/file_storage")
    .service(
      web::resource("/{workspace_id}/blob/{file_id}")
        .route(web::put().to(put_blob_handler))
        .route(web::get().to(get_blob_handler))
        .route(web::delete().to(delete_blob_handler)),
    )
    .service(
      web::resource("/{workspace_id}/metadata/{file_id}")
        .route(web::get().to(get_blob_metadata_handler)),
    )
    .service(
      web::resource("/{workspace_id}/usage").route(web::get().to(get_workspace_usage_handler)),
    )
    .service(
      web::resource("/{workspace_id}/blobs")
        .route(web::get().to(get_all_workspace_blob_metadata_handler)),
    )
}

#[instrument(skip(state, payload), err)]
async fn put_blob_handler(
  state: Data<AppState>,
  path: web::Path<(Uuid, String)>,
  content_type: web::Header<ContentType>,
  content_length: web::Header<ContentLength>,
  payload: Payload,
) -> Result<JsonAppResponse<()>> {
  let (workspace_id, file_id) = path.into_inner();
  let content_length = content_length.into_inner().into_inner();
  let content_type = content_type.into_inner().to_string();
  let content = {
    let mut payload_reader = payload_to_async_read(payload);
    let mut content = vec![0; content_length];
    let n = payload_reader.read_exact(&mut content).await?;
    if n != content_length {
      error!(
        "Content length is {}, but the actual content is larger",
        content_length
      );
    }
    let res = payload_reader.read_u8().await;
    match res {
      Ok(_) => {
        return Ok(
          AppResponse::from(AppError::PayloadTooLarge(
            "Content length is {}, but the actual content is larger".to_string(),
          ))
          .into(),
        );
      },
      Err(e) => match e.kind() {
        std::io::ErrorKind::UnexpectedEof => (),
        _ => return Err(AppError::Internal(anyhow::anyhow!(e)).into()),
      },
    };
    content
  };

  event!(
    tracing::Level::TRACE,
    "start put blob. workspace_id: {}, file_id: {}, content_length: {}",
    workspace_id,
    file_id,
    content_length
  );

  state
    .bucket_storage
    .put_blob(workspace_id, file_id.to_string(), content, content_type)
    .await
    .map_err(AppResponseError::from)?;

  Ok(AppResponse::Ok().into())
}

#[instrument(level = "debug", skip(state), err)]
async fn delete_blob_handler(
  state: Data<AppState>,
  path: web::Path<(Uuid, String)>,
) -> Result<JsonAppResponse<()>> {
  let (workspace_id, file_id) = path.into_inner();
  state
    .bucket_storage
    .delete_blob(&workspace_id, &file_id)
    .await
    .map_err(AppResponseError::from)?;
  Ok(AppResponse::Ok().into())
}

#[instrument(level = "debug", skip(state), err)]
async fn get_blob_handler(
  state: Data<AppState>,
  path: web::Path<(Uuid, String)>,
  req: HttpRequest,
) -> Result<HttpResponse<BoxBody>> {
  let (workspace_id, file_id) = path.into_inner();

  // Get the metadata
  let result = state
    .bucket_storage
    .get_blob_metadata(&workspace_id, &file_id)
    .await;

  if let Err(err) = result.as_ref() {
    return if err.is_record_not_found() {
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
    .get_blob(&workspace_id, &file_id)
    .await
    .map_err(AppResponseError::from)?;

  let response = HttpResponse::Ok()
    .append_header((ETAG, file_id))
    .append_header((CONTENT_TYPE, metadata.file_type))
    .append_header((LAST_MODIFIED, metadata.modified_at.to_rfc2822()))
    .append_header((CONTENT_LENGTH, blob.len()))
    .append_header((CACHE_CONTROL, "public, immutable, max-age=31536000"))// 31536000 seconds = 1 year
    .body(blob);

  Ok(response)
}

#[instrument(level = "debug", skip(state), err)]
async fn get_blob_metadata_handler(
  state: Data<AppState>,
  path: web::Path<(Uuid, String)>,
) -> Result<JsonAppResponse<BlobMetadata>> {
  let (workspace_id, file_id) = path.into_inner();

  // Get the metadata
  let metadata = state
    .bucket_storage
    .get_blob_metadata(&workspace_id, &file_id)
    .await
    .map(|meta| BlobMetadata {
      workspace_id: meta.workspace_id,
      file_id: meta.file_id,
      file_type: meta.file_type,
      file_size: meta.file_size,
      modified_at: meta.modified_at,
    })
    .map_err(AppResponseError::from)?;

  Ok(Json(AppResponse::Ok().with_data(metadata)))
}

#[instrument(level = "debug", skip(state), err)]
async fn get_workspace_usage_handler(
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<WorkspaceSpaceUsage>> {
  let current = get_workspace_usage_size(&state.pg_pool, &workspace_id)
    .await
    .map_err(AppResponseError::from)?;
  let usage = WorkspaceSpaceUsage {
    consumed_capacity: current,
  };
  Ok(AppResponse::Ok().with_data(usage).into())
}

// TODO(nathan): implement pagination
#[instrument(level = "debug", skip(state), err)]
async fn get_all_workspace_blob_metadata_handler(
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<RepeatedBlobMetaData>> {
  let metas = get_all_workspace_blob_metadata(&state.pg_pool, &workspace_id)
    .await
    .map_err(AppResponseError::from)?
    .into_iter()
    .map(|meta| BlobMetadata {
      workspace_id: meta.workspace_id,
      file_id: meta.file_id,
      file_type: meta.file_type,
      file_size: meta.file_size,
      modified_at: meta.modified_at,
    })
    .collect::<Vec<_>>();
  Ok(
    AppResponse::Ok()
      .with_data(RepeatedBlobMetaData(metas))
      .into(),
  )
}
fn payload_to_async_read(payload: Payload) -> Pin<Box<dyn AsyncRead>> {
  let mapped =
    payload.map(|chunk| chunk.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)));
  let reader = StreamReader::new(mapped);
  Box::pin(reader)
}
