use crate::state::AppState;
use actix_multipart::Multipart;
use actix_web::web::Data;
use actix_web::{web, HttpRequest, Scope};
use anyhow::anyhow;
use app_error::AppError;
use authentication::jwt::UserUuid;
use aws_sdk_s3::primitives::ByteStream;
use database::file::BucketClient;
use futures_util::StreamExt;
use redis::AsyncCommands;
use serde_json::json;
use sha2::{Digest, Sha256};
use shared_entity::response::{AppResponse, JsonAppResponse};
use std::env::temp_dir;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};
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
  let file_name = format!("{}.zip", Uuid::new_v4().to_string());
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

  let task = json!({
      "user_uuid": user_uuid,
      "s3_key": workspace_id,
      "file_type": "zip",
  });

  trace!("Push task:{} to redis queue", task);
  // push task to redis queue
  let _: () = state
    .redis_connection_manager
    .clone()
    .rpush("import_notion_task_queue", task.to_string())
    .await
    .map_err(|err| AppError::Internal(anyhow!("Failed to push task to redis queue: {}", err)))?;

  Ok(AppResponse::Ok().into())
}

async fn calculate_key(
  user_uuid: &UserUuid,
  file: &mut File,
) -> Result<String, Box<dyn std::error::Error>> {
  let mut hasher = Sha256::new();
  hasher.update(user_uuid.as_bytes());

  let mut buf_reader = BufReader::new(file);
  let mut buffer = vec![0; 1024 * 1024]; // 1MB buffer
  while let Ok(bytes_read) = buf_reader.read(&mut buffer).await {
    if bytes_read == 0 {
      break;
    }
    hasher.update(&buffer[..bytes_read]);
  }

  Ok(format!("{:x}", hasher.finalize()))
}
