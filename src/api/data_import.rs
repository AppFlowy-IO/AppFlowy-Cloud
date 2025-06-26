use crate::biz::authentication::jwt::UserUuid;
use crate::state::AppState;
use actix_multipart::Multipart;
use actix_web::web::{Data, Json};
use actix_web::{web, HttpRequest, Scope};
use anyhow::anyhow;
use app_error::AppError;

use aws_sdk_s3::primitives::ByteStream;
use database::file::BucketClient;

use crate::biz::workspace::ops::{create_empty_workspace, create_upload_task, num_pending_task};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use database::user::select_name_and_email_from_uuid;
use database::workspace::select_import_task_by_state;
use database_entity::dto::{CreateImportTask, CreateImportTaskResponse};
use futures_util::StreamExt;
use infra::env_util::get_env_var;
use serde_json::json;
use shared_entity::dto::import_dto::{ImportTaskDetail, UserImportTask};
use shared_entity::response::{AppResponse, JsonAppResponse};
use std::env::temp_dir;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::{error, info, instrument, trace};
use uuid::Uuid;
use validator::Validate;

pub fn data_import_scope() -> Scope {
  web::scope("/api/import")
    .service(
      web::resource("")
        .route(web::post().to(import_data_handler))
        .route(web::get().to(get_import_detail_handler)),
    )
    .service(web::resource("/create").route(web::post().to(create_import_handler)))
}

#[instrument(level = "debug", skip_all)]
async fn create_import_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  payload: Json<CreateImportTask>,
  req: HttpRequest,
) -> actix_web::Result<JsonAppResponse<CreateImportTaskResponse>> {
  let params = payload.into_inner();
  params.validate().map_err(AppError::from)?;
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  check_maximum_task(&state, uid).await?;
  let s3_key = format!("import_presigned_url_{}", Uuid::new_v4());

  // Generate presigned url with 10 minutes expiration
  let presigned_url = state
    .bucket_client
    .gen_presigned_url(&s3_key, params.content_length, 600)
    .await?;
  trace!("[Import] Presigned url: {}", presigned_url);

  let (user_name, user_email) = select_name_and_email_from_uuid(&state.pg_pool, &user_uuid).await?;
  let host = get_host_from_request(&req);
  let workspace = create_empty_workspace(
    &state.pg_pool,
    state.workspace_access_control.clone(),
    &state.collab_storage,
    &state.metrics.collab_metrics,
    &user_uuid,
    uid,
    &params.workspace_name,
  )
  .await?;

  let workspace_id = workspace.workspace_id.to_string();
  info!(
    "User:{} import new workspace:{}, name:{}",
    uid, workspace_id, params.workspace_name,
  );
  let timestamp = chrono::Utc::now().timestamp();
  let task_id = Uuid::new_v4();
  let task = json!({
      "notion": {
         "uid": uid,
         "user_name": user_name,
         "user_email": user_email,
         "task_id": task_id.to_string(),
         "workspace_id": workspace_id,
         "file_size":params.content_length,
         "created_at": timestamp,
         "s3_key": s3_key,
         "host": host,
         "workspace_name": &params.workspace_name,
      }
  });

  let data = CreateImportTaskResponse {
    task_id: task_id.to_string(),
    presigned_url: presigned_url.clone(),
  };

  create_upload_task(
    uid,
    task_id,
    task,
    &host,
    &workspace_id,
    0,
    Some(presigned_url),
    &state.redis_connection_manager,
    &state.pg_pool,
  )
  .await?;

  Ok(AppResponse::Ok().with_data(data).into())
}

async fn get_import_detail_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<UserImportTask>> {
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let tasks = select_import_task_by_state(uid, &state.pg_pool, None)
    .await
    .map(|tasks| {
      tasks
        .into_iter()
        .map(|task| ImportTaskDetail {
          task_id: task.task_id.to_string(),
          file_size: task.file_size as u64,
          created_at: task.created_at.timestamp(),
          status: task.status,
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
  check_maximum_task(&state, uid).await?;

  let (user_name, user_email) = select_name_and_email_from_uuid(&state.pg_pool, &user_uuid).await?;
  let host = get_host_from_request(&req);
  let content_length = req
    .headers()
    .get("X-Content-Length")
    .and_then(|h| h.to_str().ok())
    .and_then(|s| s.parse::<usize>().ok())
    .unwrap_or(0);

  let md5_base64 = req
    .headers()
    .get("X-Content-MD5")
    .and_then(|h| h.to_str().ok())
    .unwrap_or("");

  let file_path = temp_dir().join(format!("import_data_{}.zip", Uuid::new_v4()));
  let file = write_multiple_part(&mut payload, file_path).await?;

  trace!(
    "[Import] content length: {}, content md5: {}",
    content_length,
    md5_base64
  );
  if file.md5_base64 != md5_base64 {
    trace!(
      "Import file fail. The Content-MD5:{} doesn't match file md5:{}",
      md5_base64,
      file.md5_base64
    );

    return Err(
      AppError::InvalidRequest(format!(
        "Content-MD5:{} doesn't match file md5:{}",
        md5_base64, file.md5_base64
      ))
      .into(),
    );
  }

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
    &state.collab_storage,
    &state.metrics.collab_metrics,
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

  upload_file_with_retry(&state, &workspace_id, &file.file_path).await?;

  // This task will be deserialized into ImportTask
  let task_id = Uuid::new_v4();
  let task = json!({
      "notion": {
         "uid": uid,
         "user_name": user_name,
         "user_email": user_email,
         "task_id": task_id.to_string(),
         "workspace_id": workspace_id,
         "s3_key": workspace_id,
         "host": host,
         "workspace_name": &file.name,
         "md5_base64": md5_base64,
      }
  });

  create_upload_task(
    uid,
    task_id,
    task,
    &host,
    &workspace_id,
    file.size,
    None,
    &state.redis_connection_manager,
    &state.pg_pool,
  )
  .await?;

  Ok(AppResponse::Ok().into())
}

async fn upload_file_with_retry(
  state: &AppState,
  workspace_id: &str,
  file_path: &PathBuf,
) -> Result<(), AppError> {
  let mut attempt = 0;
  let max_retries = 3;

  while attempt <= max_retries {
    let stream = ByteStream::from_path(file_path).await.map_err(|e| {
      AppError::Internal(anyhow!("Failed to create ByteStream from file path: {}", e))
    })?;
    let result = state
      .bucket_client
      .put_blob_with_content_type(workspace_id, stream, "application/zip")
      .await;

    match result {
      Ok(_) => return Ok(()),
      Err(AppError::ServiceTemporaryUnavailable(_)) if attempt < max_retries => {
        attempt += 1;
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
      },
      Err(err) => return Err(err),
    }
  }

  Err(AppError::ServiceTemporaryUnavailable(
    "Failed to upload file to S3".to_string(),
  ))
}

async fn check_maximum_task(state: &Data<AppState>, uid: i64) -> Result<(), AppError> {
  let count = num_pending_task(uid, &state.pg_pool).await?;
  let maximum_pending_task = get_env_var("MAXIMUM_IMPORT_PENDING_TASK", "3")
    .parse::<i64>()
    .unwrap_or(3);

  if count >= maximum_pending_task {
    return Err(AppError::TooManyImportTask(format!(
      "{} tasks are pending. Please wait until they are completed",
      count
    )));
  }
  Ok(())
}

pub struct AutoDeletedFile {
  name: String,
  file_path: PathBuf,
  size: usize,
  md5_base64: String,
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
pub async fn write_multiple_part(
  payload: &mut Multipart,
  file_path: PathBuf,
) -> Result<AutoDeletedFile, AppError> {
  let mut file_name = "".to_string();
  let mut file_size = 0;

  // Create the file to write to
  let mut file = File::create(&file_path).await?;
  let mut context = md5::Context::new();

  // Process the multipart form fields
  while let Some(Ok(mut field)) = payload.next().await {
    // Extract the file name from the content disposition
    file_name = field
      .content_disposition()
      .and_then(|c| c.get_name().map(|f| f.to_string()))
      .unwrap_or_else(|| format!("import-{}", chrono::Local::now().format("%d/%m/%Y %H:%M")));

    // Write data chunks to the file and update the MD5 context
    while let Some(Ok(data)) = field.next().await {
      file_size += data.len();
      file.write_all(&data).await?;
      context.consume(&data);
    }
  }

  // Flush and close the file
  file.shutdown().await?;
  drop(file);

  // If file_name is empty, remove the file and return an error
  if file_name.is_empty() {
    if let Err(err) = tokio::fs::remove_file(&file_path).await {
      error!(
        "Failed to delete the file: {:?} when importing data, error: {}",
        file_path, err
      );
    }
    return Err(AppError::InvalidRequest(
      "Cannot get the file name".to_string(),
    ));
  }

  // Finalize the MD5 hash and encode it in base64
  let digest = context.compute();
  let md5_base64 = STANDARD.encode(digest.as_ref());

  // Return the file metadata and the calculated MD5 hash
  Ok(AutoDeletedFile {
    name: file_name,
    file_path,
    size: file_size,
    md5_base64,
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
