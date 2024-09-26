use async_zip::base::read::stream::ZipFileReader;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use redis::aio::ConnectionManager;
use redis::streams::{StreamReadOptions, StreamReadReply};
use redis::{AsyncCommands, Value};
use serde::{Deserialize, Serialize};
use serde_json::from_str;
use std::env::temp_dir;

use crate::notion_import::unzip::unzip_async;
use crate::s3_client::{S3Client, S3StreamResponse};

use crate::error::WorkerError;
use collab_importer::notion::NotionImporter;
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, trace};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportTask {
  user_uuid: String,
  workspace_id: String,
  s3_key: String,
  file_type: ImportFileType,
  host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportFileType {
  Zip,
}

pub fn run_notion_importer(redis_client: ConnectionManager, s3_client: S3Client) {
  info!("Starting notion importer worker");
  tokio::spawn(async move {
    run(redis_client, s3_client).await.unwrap();
  });
}

async fn run(
  mut redis_client: ConnectionManager,
  s3_client: S3Client,
) -> Result<(), redis::RedisError> {
  let stream_name = "import_notion_task_stream";
  let consumer_group = "import_notion_task_group";
  let consumer_name = "appflowy_worker_notion_importer";
  let options = StreamReadOptions::default().group(consumer_group, consumer_name);
  let mut interval = interval(Duration::from_secs(30));
  interval.tick().await;

  loop {
    interval.tick().await;
    let tasks: StreamReadReply = match redis_client
      .xread_options(&[stream_name], &[">"], &options)
      .await
    {
      Ok(tasks) => tasks,
      Err(err) => {
        error!("Failed to read tasks from Redis stream: {:?}", err);
        continue;
      },
    };

    let mut task_handlers = FuturesUnordered::new();
    for stream_key in tasks.keys {
      // For each stream key, iterate through the stream entries
      for stream_id in stream_key.ids {
        let task_str = match stream_id.map.get("task") {
          Some(value) => match value {
            Value::Data(data) => String::from_utf8_lossy(data).to_string(),
            _ => {
              error!("Unexpected value type for task field: {:?}", value);
              continue;
            },
          },
          None => {
            error!("Task field not found in Redis stream entry");
            continue;
          },
        };

        match from_str::<ImportTask>(&task_str) {
          Ok(import_task) => {
            let entry_id = stream_id.id.clone();
            let mut cloned_redis_client = redis_client.clone();
            let cloned_s3_client = s3_client.clone();
            task_handlers.push(tokio::spawn(async move {
              process_task(import_task, &cloned_s3_client).await?;
              let _: () = cloned_redis_client
                .xack(stream_name, consumer_group, &[entry_id])
                .await
                .map_err(|e| {
                  error!("Failed to acknowledge task: {:?}", e);
                  WorkerError::Internal(e.into())
                })?;
              Ok::<_, WorkerError>(())
            }));
          },
          Err(err) => {
            error!("Failed to deserialize task: {:?}", err);
          },
        }
      }
    }

    while let Some(result) = task_handlers.next().await {
      match result {
        Ok(Ok(())) => {
          trace!("Task completed successfully");
        },
        Ok(Err(e)) => {
          error!("Task failed: {:?}", e);
        },
        Err(e) => {
          error!("Runtime error: {:?}", e);
        },
      }
    }
  }
}

async fn process_task(import_task: ImportTask, s3_client: &S3Client) -> Result<(), WorkerError> {
  trace!("Processing task: {:?}", import_task);

  let S3StreamResponse {
    stream,
    content_type: _,
  } = s3_client.get_blob(import_task.s3_key.as_str()).await?;
  let zip_reader = ZipFileReader::new(stream);
  let temp_dir = temp_dir();

  // 1. unzip file to temp dir
  let unzip_file = unzip_async(zip_reader, temp_dir)
    .await
    .map_err(WorkerError::Internal)?;

  // 2. import zip
  let imported = NotionImporter::new(&unzip_file, import_task.workspace_id, import_task.host)?
    .import()
    .await?;

  // 2.1 insert collab to pg

  // 2.2 insert collab resource to s3

  // 3. delete zip
  // 4. send email
  Ok(())
}
