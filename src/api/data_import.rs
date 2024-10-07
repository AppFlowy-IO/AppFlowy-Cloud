use crate::state::AppState;
use actix_multipart::Multipart;
use actix_web::web::Data;
use actix_web::{web, HttpRequest, Scope};
use anyhow::anyhow;
use app_error::AppError;
use authentication::jwt::UserUuid;
use aws_sdk_s3::primitives::ByteStream;
use database::file::BucketClient;

use crate::biz::workspace::ops::{create_empty_workspace, create_upload_task};
use database::workspace::select_import_task;
use futures_util::StreamExt;
use shared_entity::dto::import_dto::{ImportTaskDetail, ImportTaskStatus, UserImportTask};
use shared_entity::response::{AppResponse, JsonAppResponse};
use std::env::temp_dir;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::{error, trace};
use uuid::Uuid;

pub fn data_import_scope() -> Scope {
  web::scope("/api/import").service(
    web::resource("")
      .route(web::post().to(import_data_handler))
      .route(web::get().to(get_import_detail_handler)),
  )
}

async fn get_import_detail_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<UserImportTask>> {
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let tasks = select_import_task(uid, &state.pg_pool, None)
    .await
    .map(|tasks| {
      tasks
        .into_iter()
        .map(|task| ImportTaskDetail {
          task_id: task.task_id.to_string(),
          file_size: task.file_size as u64,
          created_at: task.created_at.timestamp(),
          status: ImportTaskStatus::from(task.status),
        })
        .collect::<Vec<_>>()
    })?;

  Ok(
    AppResponse::Ok()
      .with_data(UserImportTask {
        tasks,
        has_more: false,
      })
      .into(),
  )
}

async fn import_data_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  mut payload: Multipart,
  req: HttpRequest,
) -> actix_web::Result<JsonAppResponse<()>> {
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let host = get_host_from_request(&req);
  let content_length = req
    .headers()
    .get("Content-Length")
    .and_then(|h| h.to_str().ok())
    .and_then(|s| s.parse::<usize>().ok())
    .unwrap_or(0);

  let time = chrono::Local::now().format("%d/%m/%Y %H:%M").to_string();
  let workspace_name = format!("import-{}", time);

  // file_name must be unique
  let file_name = format!("{}.zip", Uuid::new_v4());
  let file_path = temp_dir().join(&file_name);

  let mut file_size = 0;
  let mut file = File::create(&file_path).await?;
  while let Some(item) = payload.next().await {
    let mut field = item?;
    while let Some(chunk) = field.next().await {
      let data = chunk?;
      file_size += data.len();
      file.write_all(&data).await?;
    }
  }
  file.shutdown().await?;
  drop(file);

  if content_length != file_size {
    return Err(
      AppError::InvalidRequest(format!(
        "Content-Length:{} doesn't match file size:{}",
        content_length, file_size
      ))
      .into(),
    );
  }

  let workspace = create_empty_workspace(
    &state.pg_pool,
    &state.workspace_access_control,
    &state.collab_access_control_storage,
    &user_uuid,
    uid,
    &workspace_name,
  )
  .await?;

  let workspace_id = workspace.workspace_id.to_string();
  trace!(
    "User:{} import data:{} to new workspace:{}",
    uid,
    file_size,
    workspace_id,
  );
  let stream = ByteStream::from_path(&file_path).await.map_err(|e| {
    AppError::Internal(anyhow!("Failed to create ByteStream from file path: {}", e))
  })?;
  state
    .bucket_client
    .put_blob_as_content_type(&workspace_id, stream, "zip")
    .await?;

  // delete the file after uploading
  tokio::spawn(async move {
    if let Err(err) = tokio::fs::remove_file(file_path).await {
      error!("Failed to delete file after uploading: {}", err);
    }
  });

  create_upload_task(
    uid,
    &user_uuid,
    &workspace_id,
    file_size,
    &host,
    &state.redis_connection_manager,
    &state.pg_pool,
  )
  .await?;

  Ok(AppResponse::Ok().into())
}

fn get_host_from_request(req: &HttpRequest) -> String {
  req
    .headers()
    .get("X-Host")
    .and_then(|h| h.to_str().ok())
    .unwrap_or("https://beta.appflowy.cloud")
    .to_string()
}
