use access_control::act::Action;
use actix_http::body::BoxBody;
use actix_web::http::header::{
  ContentLength, ContentType, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE,
  LAST_MODIFIED,
};
use actix_web::web::{Json, Payload};
use actix_web::{
  web::{self, Data},
  HttpRequest, ResponseError, Scope,
};
use actix_web::{HttpResponse, Result};
use app_error::AppError;

use chrono::DateTime;
use database::file::BlobKey;
use database::resource_usage::{get_all_workspace_blob_metadata, get_workspace_usage_size};
use database_entity::file_dto::{
  CompleteUploadRequest, CreateUploadRequest, CreateUploadResponse, UploadPartData,
  UploadPartResponse,
};

use crate::biz::authentication::jwt::UserUuid;
use crate::biz::data_import::LimitedPayload;
use crate::state::AppState;
use anyhow::anyhow;
use appflowy_ai_client::client::AppFlowyAIClient;
use aws_sdk_s3::primitives::ByteStream;
use collab_importer::util::FileId;
use database::pg_row::{AFBlobSource, AFBlobStatus};
use serde::Deserialize;
use shared_entity::dto::file_dto::PutFileResponse;
use shared_entity::dto::workspace_dto::{BlobMetadata, RepeatedBlobMetaData, WorkspaceSpaceUsage};
use shared_entity::response::{AppResponse, AppResponseError, JsonAppResponse};
use sqlx::types::Uuid;
use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_stream::StreamExt;
use tokio_util::io::StreamReader;
use tracing::{error, event, info, instrument, trace};

pub fn file_storage_scope() -> Scope {
  web::scope("/api/file_storage")
    .service(
      // Deprecated, use put_blob_handler_v1 instead
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
    .service(web::resource("/{workspace_id}/create_upload").route(web::post().to(create_upload)))
    .service(
      web::resource("/{workspace_id}/upload_part/{parent_dir}/{file_id}/{upload_id}/{part_num}")
        .route(web::put().to(upload_part_handler)),
    )
    .service(
      web::resource("/{workspace_id}/complete_upload")
        .route(web::put().to(complete_upload_handler)),
    )
    .service(
      web::resource("/{workspace_id}/v1/blob/{parent_dir}/{file_id}")
        .route(web::get().to(get_blob_v1_handler))
        .route(web::delete().to(delete_blob_v1_handler)),
    )
    .service(
      web::resource("/{workspace_id}/v1/metadata/{parent_dir}/{file_id}")
        .route(web::get().to(get_blob_metadata_v1_handler)),
    )
    .service(
      // Upload the file in a single request. This process combines the steps of create-upload,
      // upload-part, and complete-upload into a single operation. The file can then be retrieved
      // using the get_blob_v1_handler. If you want to support resumeable uploads, you should use
      // the create-upload, upload-part, and complete-upload endpoints.
      web::resource("/{workspace_id}/v1/blob/{parent_dir}")
        .route(web::put().to(put_blob_handler_v1)),
    )
}

#[instrument(skip_all, err)]
async fn create_upload(
  user_uuid: UserUuid,
  workspace_id: web::Path<Uuid>,
  state: web::Data<AppState>,
  req: web::Json<CreateUploadRequest>,
) -> Result<JsonAppResponse<CreateUploadResponse>> {
  let req = req.into_inner();
  if req.parent_dir.is_empty() {
    return Err(AppError::InvalidRequest("parent_dir is empty".to_string()).into());
  }

  if req.file_id.is_empty() {
    return Err(AppError::InvalidRequest("file_id is empty".to_string()).into());
  }
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let workspace_id = workspace_id.into_inner();
  state
    .workspace_access_control
    .enforce_action(&uid, &workspace_id, Action::Write)
    .await?;

  let key = BlobPathV1 {
    workspace_id,
    parent_dir: req.parent_dir.clone(),
    file_id: req.file_id.clone(),
  };
  let resp = state
    .bucket_storage
    .create_upload(key, req)
    .await
    .map_err(AppResponseError::from)?;

  Ok(AppResponse::Ok().with_data(resp).into())
}

#[derive(Deserialize)]
struct UploadPartPath {
  workspace_id: Uuid,
  parent_dir: String,
  file_id: String,
  upload_id: String,
  part_num: i32,
}

#[instrument(level = "debug", skip_all, err)]
async fn upload_part_handler(
  user_uuid: UserUuid,
  path: web::Path<UploadPartPath>,
  state: web::Data<AppState>,
  content_length: web::Header<ContentLength>,
  mut payload: Payload,
) -> Result<JsonAppResponse<UploadPartResponse>> {
  let path_params = path.into_inner();
  trace!(
    "upload part: workspace_id: {}, parent_dir: {}, file_id: {}, upload_id: {}, part_num: {}",
    path_params.workspace_id,
    path_params.parent_dir,
    path_params.file_id,
    path_params.upload_id,
    path_params.part_num
  );
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let workspace_id = path_params.workspace_id;
  state
    .workspace_access_control
    .enforce_action(&uid, &workspace_id, Action::Write)
    .await?;

  let content_length = content_length.into_inner().into_inner();
  let mut content = Vec::with_capacity(content_length);
  while let Some(chunk) = payload.try_next().await? {
    content.extend_from_slice(&chunk);
  }

  if content.len() != content_length {
    return Err(
      AppError::InvalidRequest(format!(
        "Content length is {}, but received {} bytes",
        content_length,
        content.len()
      ))
      .into(),
    );
  }
  let data = UploadPartData {
    file_id: path_params.file_id.clone(),
    upload_id: path_params.upload_id,
    part_number: path_params.part_num,
    body: content,
  };

  let key = BlobPathV1 {
    workspace_id,
    parent_dir: path_params.parent_dir,
    file_id: path_params.file_id,
  };

  let resp = state
    .bucket_storage
    .upload_part(key, data)
    .await
    .map_err(AppResponseError::from)?;
  Ok(AppResponse::Ok().with_data(resp).into())
}

async fn complete_upload_handler(
  user_uuid: UserUuid,
  workspace_id: web::Path<Uuid>,
  state: web::Data<AppState>,
  req: web::Json<CompleteUploadRequest>,
) -> Result<JsonAppResponse<()>> {
  let req = req.into_inner();
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let workspace_id = workspace_id.into_inner();
  state
    .workspace_access_control
    .enforce_action(&uid, &workspace_id, Action::Write)
    .await?;

  let key = BlobPathV1 {
    workspace_id,
    parent_dir: req.parent_dir.clone(),
    file_id: req.file_id.clone(),
  };
  state
    .bucket_storage
    .complete_upload(key, req)
    .await
    .map_err(AppResponseError::from)?;

  Ok(AppResponse::Ok().into())
}

#[instrument(skip(state, payload), err)]
async fn put_blob_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  path: web::Path<BlobPathV0>,
  content_type: web::Header<ContentType>,
  content_length: web::Header<ContentLength>,
  payload: Payload,
) -> Result<JsonAppResponse<()>> {
  let path = path.into_inner();
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let workspace_id = path.workspace_id;
  state
    .workspace_access_control
    .enforce_action(&uid, &workspace_id, Action::Write)
    .await?;

  let content_length = content_length.into_inner().into_inner();
  let content_type = content_type.into_inner().to_string();
  let content = {
    let mut payload_reader = payload_to_async_read(payload);
    let mut content = Vec::with_capacity(content_length);
    if content.try_reserve_exact(content_length).is_err() {
      return Err(
        AppError::Internal(anyhow!(
          "Can not alloc mem for blob content size:{}",
          content_length
        ))
        .into(),
      );
    }
    content.resize(content_length, 0);

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
    path.file_id,
    content_length
  );

  let file_size = content.len();
  let file_stream = ByteStream::from(content);
  state
    .bucket_storage
    .put_blob_with_content_type(path, file_stream, content_type, file_size)
    .await
    .map_err(AppResponseError::from)?;

  Ok(AppResponse::Ok().into())
}

#[instrument(level = "debug", skip(state), err)]
async fn delete_blob_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  path: web::Path<BlobPathV0>,
) -> Result<JsonAppResponse<()>> {
  let path = path.into_inner();
  let workspace_id = path.workspace_id;
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  state
    .workspace_access_control
    .enforce_action(&uid, &workspace_id, Action::Write)
    .await?;
  state
    .bucket_storage
    .delete_blob(path)
    .await
    .map_err(AppResponseError::from)?;

  Ok(AppResponse::Ok().into())
}

#[instrument(level = "debug", skip(state), err)]
async fn get_blob_v1_handler(
  state: Data<AppState>,
  path: web::Path<BlobPathV1>,
  req: HttpRequest,
) -> Result<HttpResponse<BoxBody>> {
  let path = path.into_inner();
  get_blob_by_object_key(state, &path, req).await
}

#[instrument(level = "debug", skip(state), err)]
async fn delete_blob_v1_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  path: web::Path<BlobPathV1>,
) -> Result<JsonAppResponse<()>> {
  let path = path.into_inner();
  let workspace_id = path.workspace_id;
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  state
    .workspace_access_control
    .enforce_action(&uid, &workspace_id, Action::Write)
    .await?;
  state
    .bucket_storage
    .delete_blob(path)
    .await
    .map_err(AppResponseError::from)?;

  Ok(AppResponse::Ok().into())
}

async fn get_blob_by_object_key(
  state: Data<AppState>,
  key: &impl BlobKey,
  req: HttpRequest,
) -> Result<HttpResponse<BoxBody>> {
  // Get the metadata
  let result = state
    .bucket_storage
    .get_blob_metadata(key.workspace_id(), &key.blob_metadata_key())
    .await;

  if let Err(err) = result.as_ref() {
    return if err.is_record_not_found() {
      Ok(HttpResponse::NotFound().finish())
    } else {
      Ok(HttpResponse::InternalServerError().finish())
    };
  }

  let metadata = result.unwrap();
  let source = AFBlobSource::from(metadata.source);
  trace!("blob metadata: {:?}", metadata);
  match source {
    AFBlobSource::UserUpload => {},
    AFBlobSource::AIGen => {
      let spawn_regenerate_image =
        |client: AppFlowyAIClient, source_metadata: serde_json::Value| {
          tokio::spawn(async move {
            info!("Regenerate ai image: {:?}", source_metadata);
            let _ = client.regenerate_image(source_metadata).await;
          });
        };
      let source_metadata = metadata.source_metadata;
      let status = AFBlobStatus::from(metadata.status);
      trace!("AI image {}: {:?}", key.object_key(), status);
      match status {
        AFBlobStatus::PolicyViolation => {
          return Ok(HttpResponse::UnprocessableEntity().finish());
        },
        AFBlobStatus::Pending => {
          if metadata.modified_at + chrono::Duration::minutes(1) < chrono::Utc::now() {
            spawn_regenerate_image(state.ai_client.clone(), source_metadata);
          } else {
            trace!("AI image is pending, wait for 1 minute");
          }
        },
        AFBlobStatus::Failed => {
          spawn_regenerate_image(state.ai_client.clone(), source_metadata);
        },
        _ => {},
      };
    },
  }

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

  trace!("Get blob data from bucket storage: {:?}", key.object_key());
  let blob_result = state.bucket_storage.get_blob(key).await;
  match blob_result {
    Ok(blob) => {
      let response = HttpResponse::Ok()
          .append_header((ETAG, key.e_tag()))
          .append_header((CONTENT_TYPE, metadata.file_type))
          .append_header((LAST_MODIFIED, metadata.modified_at.to_rfc2822()))
          .append_header((CONTENT_LENGTH, blob.len()))
          .append_header((CACHE_CONTROL, "public, immutable, max-age=31536000"))// 31536000 seconds = 1 year
          .body(blob);

      Ok(response)
    },
    Err(err) => {
      if err.is_record_not_found() {
        Ok(HttpResponse::NotFound().finish())
      } else {
        Ok(AppResponseError::from(err).error_response())
      }
    },
  }
}

#[instrument(level = "debug", skip(state), err)]
async fn get_blob_handler(
  state: Data<AppState>,
  path: web::Path<BlobPathV0>,
  req: HttpRequest,
) -> Result<HttpResponse<BoxBody>> {
  let blob_path = path.into_inner();
  get_blob_by_object_key(state, &blob_path, req).await
}

#[instrument(level = "debug", skip(state), err)]
async fn get_blob_metadata_handler(
  state: Data<AppState>,
  path: web::Path<BlobPathV0>,
) -> Result<JsonAppResponse<BlobMetadata>> {
  let path = path.into_inner();

  // Get the metadata
  let metadata = state
    .bucket_storage
    .get_blob_metadata(&path.workspace_id, &path.blob_metadata_key())
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
async fn get_blob_metadata_v1_handler(
  state: Data<AppState>,
  path: web::Path<BlobPathV1>,
) -> Result<JsonAppResponse<BlobMetadata>> {
  let path = path.into_inner();

  // Get the metadata
  let metadata = state
    .bucket_storage
    .get_blob_metadata(&path.workspace_id, &path.blob_metadata_key())
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

#[instrument(skip(state, payload), err)]
async fn put_blob_handler_v1(
  user_uuid: UserUuid,
  state: Data<AppState>,
  path: web::Path<BlobPathV2>,
  content_type: web::Header<ContentType>,
  content_length: web::Header<ContentLength>,
  payload: Payload,
) -> Result<JsonAppResponse<PutFileResponse>> {
  let path = path.into_inner();
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  state
    .workspace_access_control
    .enforce_action(&uid, &path.workspace_id, Action::Write)
    .await?;

  let content_length = content_length.into_inner().into_inner();
  let content_type = content_type.into_inner().to_string();

  let mut content = Vec::with_capacity(content_length);
  if content.try_reserve_exact(content_length).is_err() {
    return Err(
      AppError::Internal(anyhow!(
        "Can not alloc mem for blob content size:{}",
        content_length
      ))
      .into(),
    );
  }
  content.resize(content_length, 0);

  let mut limited_payload = LimitedPayload::new(payload, content_length);
  let mut offset = 0;
  while let Some(bytes) = limited_payload.next().await {
    let bytes = bytes?;
    let len = bytes.len();
    content[offset..offset + len].copy_from_slice(&bytes);
    offset += len;
  }

  let file_id = FileId::from_bytes(&content, "".to_string());
  let resp_data = PutFileResponse {
    file_id: file_id.clone(),
  };
  event!(
    tracing::Level::TRACE,
    "start put blob. workspace_id: {}, file_id: {}, content_length: {}",
    path.workspace_id,
    file_id,
    content_length
  );

  let file_stream = ByteStream::from(content);
  state
    .bucket_storage
    .put_blob_with_content_type(
      BlobPathV1::from((path, file_id)),
      file_stream,
      content_type,
      content_length,
    )
    .await
    .map_err(AppResponseError::from)?;
  Ok(AppResponse::Ok().with_data(resp_data).into())
}

/// Use [BlobPathV0] when get/put object by single part
#[derive(Deserialize, Debug)]
struct BlobPathV0 {
  workspace_id: Uuid,
  file_id: String,
}

impl BlobKey for BlobPathV0 {
  fn workspace_id(&self) -> &Uuid {
    &self.workspace_id
  }

  fn object_key(&self) -> String {
    format!("{}/{}", self.workspace_id, self.file_id)
  }

  fn blob_metadata_key(&self) -> String {
    self.file_id.clone()
  }

  fn e_tag(&self) -> &str {
    &self.file_id
  }
}

/// Use [BlobPathV1] when put/get object by multiple upload parts
#[derive(Deserialize, Debug)]
pub struct BlobPathV1 {
  pub workspace_id: Uuid,
  pub parent_dir: String,
  pub file_id: String,
}

impl BlobKey for BlobPathV1 {
  fn workspace_id(&self) -> &Uuid {
    &self.workspace_id
  }

  fn object_key(&self) -> String {
    format!("{}/{}/{}", self.workspace_id, self.parent_dir, self.file_id)
  }

  fn blob_metadata_key(&self) -> String {
    format!("{}_{}", self.parent_dir, self.file_id)
  }

  fn e_tag(&self) -> &str {
    &self.file_id
  }
}

#[derive(Deserialize, Debug)]
pub struct BlobPathV2 {
  pub workspace_id: Uuid,
  pub parent_dir: String,
}

impl From<(BlobPathV2, String)> for BlobPathV1 {
  fn from((path, file_id): (BlobPathV2, String)) -> Self {
    BlobPathV1 {
      workspace_id: path.workspace_id,
      parent_dir: path.parent_dir,
      file_id,
    }
  }
}
