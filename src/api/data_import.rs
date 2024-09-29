use crate::state::AppState;
use actix_multipart::Multipart;
use actix_web::web::Data;
use actix_web::{web, Scope};
use anyhow::anyhow;
use app_error::AppError;
use authentication::jwt::UserUuid;
use aws_sdk_s3::primitives::ByteStream;
use database::file::BucketClient;
use database::user::select_uid_from_uuid;
use futures_util::StreamExt;
use redis::AsyncCommands;
use serde_json::json;
use shared_entity::response::{AppResponse, JsonAppResponse};
use std::env::temp_dir;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::trace;
use uuid::Uuid;

pub fn data_import_scope() -> Scope {
  web::scope("/api/import")
    .service(web::resource("/notion").route(web::post().to(import_notion_data_handler)))
}

async fn import_notion_data_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  mut payload: Multipart,
) -> actix_web::Result<JsonAppResponse<()>> {
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let host = "".to_string();
  let file_name = format!("{}.zip", Uuid::new_v4());
  let file_path = temp_dir().join(&file_name);
  let mut file = File::create(&file_path).await?;
  while let Some(item) = payload.next().await {
    let mut field = item?;
    while let Some(chunk) = field.next().await {
      let data = chunk?;
      file.write_all(&data).await?;
    }
  }
  file.shutdown().await?;

  let workspace_id = uuid::Uuid::new_v4().to_string();
  trace!("Upload file:{:?} to s3", file_path);
  let stream = ByteStream::from_path(file_path).await.map_err(|e| {
    AppError::Internal(anyhow!("Failed to create ByteStream from file path: {}", e))
  })?;
  state
    .bucket_client
    .put_blob_as_content_type(&workspace_id, stream, "zip")
    .await?;

  // This task will be deserialized into ImportTask
  let task = json!({
      "notion": {
         "uid": uid,
         "user_uuid": user_uuid,
         "workspace_id": workspace_id,
         "s3_key": workspace_id,
         "file_type": "zip",
         "host": host,
      }
  });

  trace!("Push task:{} to redis queue", task);
  let _: () = state
    .redis_connection_manager
    .clone()
    .xadd(
      "import_notion_task_stream",
      "*",
      &[("task", task.to_string())],
    )
    .await
    .map_err(|err| AppError::Internal(anyhow!("Failed to push task to Redis stream: {}", err)))?;

  Ok(AppResponse::Ok().into())
}
