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
use database::user::select_name_and_email_from_uuid;
use database::workspace::select_import_task;
use futures_util::StreamExt;
use shared_entity::dto::import_dto::{ImportTaskDetail, ImportTaskStatus, UserImportTask};
use shared_entity::response::{AppResponse, JsonAppResponse};
use std::env::temp_dir;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::{error, info, trace};
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
  let (user_name, user_email) = select_name_and_email_from_uuid(&state.pg_pool, &user_uuid).await?;
  let host = get_host_from_request(&req);
  let content_length = req
    .headers()
    .get("X-Content-Length")
    .and_then(|h| h.to_str().ok())
    .and_then(|s| s.parse::<usize>().ok())
    .unwrap_or(0);

  let file_path = temp_dir().join(format!("import_data_{}.zip", Uuid::new_v4()));
  let file = write_multiple_part(&mut payload, file_path).await?;

  if content_length != file.size {
    trace!(
      "Import file fail. The Content-Length:{} doesn't match file size:{}",
      content_length,
      file.size
    );

    return Err(
      AppError::InvalidRequest(format!(
        "Content-Length:{} doesn't match file size:{}",
        content_length, file.size
      ))
      .into(),
    );
  }

  let workspace = create_empty_workspace(
    &state.pg_pool,
    state.workspace_access_control.clone(),
    &state.collab_access_control_storage,
    &user_uuid,
    uid,
    &file.name,
  )
  .await?;

  let workspace_id = workspace.workspace_id.to_string();
  info!(
    "User:{} import data:{} to new workspace:{}, name:{}",
    uid, file.size, workspace_id, file.name,
  );
  let stream = ByteStream::from_path(&file.file_path).await.map_err(|e| {
    AppError::Internal(anyhow!("Failed to create ByteStream from file path: {}", e))
  })?;
  state
    .bucket_client
    .put_blob_as_content_type(&workspace_id, stream, "zip")
    .await?;

  create_upload_task(
    uid,
    &user_name,
    &user_email,
    &workspace_id,
    &file.name,
    file.size,
    &host,
    &state.redis_connection_manager,
    &state.pg_pool,
  )
  .await?;

  Ok(AppResponse::Ok().into())
}

struct AutoDeletedFile {
  name: String,
  file_path: PathBuf,
  size: usize,
}

impl Drop for AutoDeletedFile {
  fn drop(&mut self) {
    let path = self.file_path.clone();
    tokio::spawn(async move {
      trace!("[AutoDeletedFile]: delete file: {:?}", path);
      if let Err(err) = tokio::fs::remove_file(&path).await {
        error!(
          "Failed to delete the auto deleted file: {:?}, error: {}",
          path, err
        )
      }
    });
  }
}

async fn write_multiple_part(
  payload: &mut Multipart,
  file_path: PathBuf,
) -> Result<AutoDeletedFile, AppError> {
  let mut file_name = "".to_string();
  let mut file_size = 0;
  let mut file = File::create(&file_path).await?;
  while let Some(Ok(mut field)) = payload.next().await {
    file_name = field
      .content_disposition()
      .and_then(|c| c.get_name().map(|f| f.to_string()))
      .unwrap_or_else(|| format!("import-{}", chrono::Local::now().format("%d/%m/%Y %H:%M")));

    while let Some(Ok(data)) = field.next().await {
      file_size += data.len();
      file.write_all(&data).await?;
    }
  }
  file.shutdown().await?;
  drop(file);

  if file_name.is_empty() {
    return Err(AppError::InvalidRequest(
      "Can not get the file name".to_string(),
    ));
  }

  Ok(AutoDeletedFile {
    name: file_name,
    file_path,
    size: file_size,
  })
}

fn get_host_from_request(req: &HttpRequest) -> String {
  req
    .headers()
    .get("X-Host")
    .and_then(|h| h.to_str().ok())
    .unwrap_or("https://beta.appflowy.cloud")
    .to_string()
}
