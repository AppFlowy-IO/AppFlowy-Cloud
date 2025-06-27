use crate::import_worker::report::{ImportNotifier, ImportProgress, ImportResult};
use crate::s3_client::{download_file, AutoRemoveDownloadedFile, S3StreamResponse};
use anyhow::anyhow;
use aws_sdk_s3::primitives::ByteStream;

use crate::error::{ImportError, WorkerError};
use crate::mailer::ImportNotionMailerParam;
use crate::s3_client::S3Client;

use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use collab::entity::{EncodedCollab, EncoderVersion};
use collab_database::workspace_database::WorkspaceDatabase;
use collab_entity::CollabType;
use collab_folder::{Folder, View, ViewLayout};
use collab_importer::imported_collab::ImportType;
use collab_importer::notion::page::CollabResource;
use collab_importer::notion::NotionImporter;
use collab_importer::util::FileId;
use database::collab::{insert_into_af_collab_bulk_for_user, select_blob_from_af_collab};
use database::resource_usage::{insert_blob_metadata_bulk, BulkInsertMeta};
use database::workspace::{
  delete_from_workspace, select_import_task, select_workspace_database_storage_id,
  update_import_task_status, update_updated_at_of_workspace_with_uid, update_workspace_status,
  ImportTaskState,
};
use database_entity::dto::CollabParams;

use crate::metric::ImportMetrics;
use async_zip::base::read::stream::{Ready, ZipFileReader};
use collab_importer::zip_tool::async_zip::async_unzip;
use collab_importer::zip_tool::sync_zip::sync_unzip;

use futures::stream::FuturesUnordered;
use futures::{stream, AsyncBufRead, AsyncReadExt, StreamExt};
use infra::env_util::get_env_var;
use redis::aio::ConnectionManager;
use redis::streams::{
  StreamClaimOptions, StreamClaimReply, StreamId, StreamPendingReply, StreamReadOptions,
  StreamReadReply,
};
use redis::{AsyncCommands, RedisResult, Value};

use collab::core::collab::default_client_id;
use database::pg_row::AFImportTask;
use serde::{Deserialize, Serialize};
use serde_json::from_str;
use sqlx::types::chrono::{DateTime, TimeZone, Utc};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::env::temp_dir;
use std::fmt::Display;
use std::fs::Permissions;
use std::io::ErrorKind;
use std::ops::DerefMut;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::task::spawn_local;
use tokio::time::{interval, MissedTickBehavior};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::{error, info, trace, warn};
use uuid::Uuid;

const GROUP_NAME: &str = "import_task_group";
const CONSUMER_NAME: &str = "appflowy_worker";
const MAXIMUM_CONTENT_LENGTH: &str = "3221225472";

#[allow(clippy::too_many_arguments)]
pub async fn run_import_worker(
  pg_pool: PgPool,
  mut redis_client: ConnectionManager,
  metrics: Option<Arc<ImportMetrics>>,
  s3_client: Arc<dyn S3Client>,
  notifier: Arc<dyn ImportNotifier>,
  stream_name: &str,
  tick_interval_secs: u64,
  max_import_file_size: u64,
) -> Result<(), ImportError> {
  info!("Starting importer worker");
  if let Err(err) = ensure_consumer_group(stream_name, GROUP_NAME, &mut redis_client).await {
    error!("Failed to ensure consumer group: {:?}", err);
  }

  let storage_dir = temp_dir();
  process_un_acked_tasks(
    &storage_dir,
    &mut redis_client,
    &s3_client,
    &pg_pool,
    stream_name,
    GROUP_NAME,
    CONSUMER_NAME,
    notifier.clone(),
    &metrics,
    max_import_file_size,
  )
  .await;

  process_upcoming_tasks(
    &storage_dir,
    &mut redis_client,
    &s3_client,
    pg_pool,
    stream_name,
    GROUP_NAME,
    CONSUMER_NAME,
    notifier.clone(),
    tick_interval_secs,
    &metrics,
    max_import_file_size,
  )
  .await?;

  Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn process_un_acked_tasks(
  storage_dir: &Path,
  redis_client: &mut ConnectionManager,
  s3_client: &Arc<dyn S3Client>,
  pg_pool: &PgPool,
  stream_name: &str,
  group_name: &str,
  consumer_name: &str,
  notifier: Arc<dyn ImportNotifier>,
  metrics: &Option<Arc<ImportMetrics>>,
  maximum_import_file_size: u64,
) {
  // when server restarts, we need to check if there are any unacknowledged tasks
  match get_un_ack_tasks(stream_name, group_name, consumer_name, redis_client).await {
    Ok(un_ack_tasks) => {
      info!("Found {} unacknowledged tasks", un_ack_tasks.len());
      for un_ack_task in un_ack_tasks {
        let context = TaskContext {
          storage_dir: storage_dir.to_path_buf(),
          redis_client: redis_client.clone(),
          s3_client: s3_client.clone(),
          pg_pool: pg_pool.clone(),
          notifier: notifier.clone(),
          metrics: metrics.clone(),
          maximum_import_file_size,
        };
        // Ignore the error here since the consume task will handle the error
        let _ = consume_task(
          context,
          un_ack_task.task,
          stream_name,
          group_name,
          un_ack_task.stream_id.id,
        )
        .await;
      }
    },
    Err(err) => error!("Failed to get unacknowledged tasks: {:?}", err),
  }
}

#[allow(clippy::too_many_arguments)]
async fn process_upcoming_tasks(
  storage_dir: &Path,
  redis_client: &mut ConnectionManager,
  s3_client: &Arc<dyn S3Client>,
  pg_pool: PgPool,
  stream_name: &str,
  group_name: &str,
  consumer_name: &str,
  notifier: Arc<dyn ImportNotifier>,
  interval_secs: u64,
  metrics: &Option<Arc<ImportMetrics>>,
  maximum_import_file_size: u64,
) -> Result<(), ImportError> {
  let options = StreamReadOptions::default()
    .group(group_name, consumer_name)
    .count(10);
  let mut interval = interval(Duration::from_secs(interval_secs));
  interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
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

        // Use command:
        // docker exec -it appflowy-cloud-redis-1 redis-cli FLUSHDB to generate the error
        // NOGROUP: No such key 'import_task_stream' or consumer group 'import_task_group' in XREADGROUP with GROUP option
        if let Some(code) = err.code() {
          if code == "NOGROUP" {
            if let Err(err) = ensure_consumer_group(stream_name, GROUP_NAME, redis_client).await {
              error!("Failed to ensure consumer group: {:?}", err);
            }
          }
        }
        continue;
      },
    };

    let mut task_handlers = FuturesUnordered::new();
    for stream_key in tasks.keys {
      // For each stream key, iterate through the stream entries
      for stream_id in stream_key.ids {
        match ImportTask::try_from(&stream_id) {
          Ok(import_task) => {
            let stream_name = stream_name.to_string();
            let group_name = group_name.to_string();

            let context = TaskContext {
              storage_dir: storage_dir.to_path_buf(),
              redis_client: redis_client.clone(),
              s3_client: s3_client.clone(),
              pg_pool: pg_pool.clone(),
              notifier: notifier.clone(),
              metrics: metrics.clone(),
              maximum_import_file_size,
            };

            let handle = spawn_local(async move {
              consume_task(
                context,
                import_task,
                &stream_name,
                &group_name,
                stream_id.id,
              )
              .await?;
              Ok::<(), ImportError>(())
            });
            task_handlers.push(handle);
          },
          Err(err) => {
            error!("Failed to deserialize task: {:?}", err);
          },
        }
      }
    }

    while let Some(result) = task_handlers.next().await {
      match result {
        Ok(Ok(())) => {},
        Ok(Err(e)) => error!("Task failed: {:?}", e),
        Err(e) => error!("Runtime error: {:?}", e),
      }
    }
  }
}
#[derive(Clone)]
struct TaskContext {
  storage_dir: PathBuf,
  redis_client: ConnectionManager,
  s3_client: Arc<dyn S3Client>,
  pg_pool: PgPool,
  notifier: Arc<dyn ImportNotifier>,
  metrics: Option<Arc<ImportMetrics>>,
  maximum_import_file_size: u64,
}

#[allow(clippy::too_many_arguments)]
async fn consume_task(
  mut context: TaskContext,
  mut import_task: ImportTask,
  stream_name: &str,
  group_name: &str,
  entry_id: String,
) -> Result<(), ImportError> {
  if let ImportTask::Notion(task) = &mut import_task {
    // If no created_at timestamp, proceed directly to processing
    if task.created_at.is_none() {
      return process_and_ack_task(context, import_task, stream_name, group_name, &entry_id).await;
    }

    match task.file_size {
      None => {
        return Err(ImportError::UpgradeToLatestVersion(format!(
          "Missing file_size for task: {}",
          task.task_id
        )))
      },
      Some(file_size) => {
        if file_size > context.maximum_import_file_size as i64 {
          let file_size_in_mb = file_size as f64 / 1_048_576.0;
          let max_size_in_mb = (context.maximum_import_file_size as f64 / 1_048_576.0).ceil();
          if let Ok(import_record) = select_import_task(&context.pg_pool, &task.task_id).await {
            handle_failed_task(
              &mut context,
              &import_record,
              task,
              stream_name,
              group_name,
              &entry_id,
              ImportError::UploadFileTooLarge {
                file_size_in_mb,
                max_size_in_mb,
              },
              ImportTaskState::Failed,
            )
            .await?;
          }

          return Err(ImportError::UploadFileTooLarge {
            file_size_in_mb,
            max_size_in_mb,
          });
        }
      },
    }

    // Check if the task is expired
    if let Err(reason) = is_task_expired(task.created_at.unwrap(), task.last_process_at) {
      if let Ok(import_record) = select_import_task(&context.pg_pool, &task.task_id).await {
        error!("[Import] {} task is expired: {}", task.workspace_id, reason);
        handle_failed_task(
          &mut context,
          &import_record,
          task,
          stream_name,
          group_name,
          &entry_id,
          ImportError::UploadFileExpire,
          ImportTaskState::Expire,
        )
        .await?;
      }
      return Ok(());
    }

    // Check if the blob exists
    if check_blob_existence(&context.s3_client, &task.s3_key).await? {
      if task.last_process_at.is_none() {
        task.last_process_at = Some(Utc::now().timestamp());
      }
      process_and_ack_task(context, import_task, stream_name, group_name, &entry_id).await
    } else {
      info!(
        "[Import] {} zip file not found, queue task",
        task.workspace_id
      );
      push_task(
        &mut context.redis_client,
        stream_name,
        group_name,
        import_task,
        &entry_id,
      )
      .await?;
      Ok(())
    }
  } else {
    // If the task is not a notion task, proceed directly to processing
    process_and_ack_task(context, import_task, stream_name, group_name, &entry_id).await
  }
}

#[allow(clippy::too_many_arguments)]
async fn handle_failed_task(
  context: &mut TaskContext,
  import_record: &AFImportTask,
  task: &NotionImportTask,
  stream_name: &str,
  group_name: &str,
  entry_id: &str,
  error: ImportError,
  task_state: ImportTaskState,
) -> Result<(), ImportError> {
  info!(
    "[Import]: {} import was failed with reason:{}",
    task.workspace_id, error
  );

  update_import_task_status(&import_record.task_id, task_state, &context.pg_pool)
    .await
    .map_err(|e| {
      error!("Failed to update import task status: {:?}", e);
      ImportError::Internal(e.into())
    })?;
  remove_workspace(&import_record.workspace_id, &context.pg_pool).await;
  info!("[Import]: deleted workspace {}", task.workspace_id);

  if let Err(err) = context.s3_client.delete_blob(task.s3_key.as_str()).await {
    error!(
      "[Import]: {} failed to delete zip file from S3: {:?}",
      task.workspace_id, err
    );
  }
  if let Err(err) = delete_task(&mut context.redis_client, stream_name, group_name, entry_id).await
  {
    error!(
      "[Import] failed to acknowledge task:{} error:{:?}",
      task.workspace_id, err
    );
  }
  notify_user(task, Err(error), context.notifier.clone(), &context.metrics).await?;
  Ok(())
}

async fn check_blob_existence(
  s3_client: &Arc<dyn S3Client>,
  s3_key: &str,
) -> Result<bool, ImportError> {
  s3_client.is_blob_exist(s3_key).await.map_err(|e| {
    error!("Failed to check blob existence: {:?}", e);
    ImportError::Internal(e.into())
  })
}

async fn process_and_ack_task(
  mut context: TaskContext,
  import_task: ImportTask,
  stream_name: &str,
  group_name: &str,
  entry_id: &str,
) -> Result<(), ImportError> {
  let result = process_task(context.clone(), import_task).await;
  delete_task(&mut context.redis_client, stream_name, group_name, entry_id)
    .await
    .ok();
  result
}

fn is_task_expired(created_timestamp: i64, last_process_at: Option<i64>) -> Result<(), String> {
  match Utc.timestamp_opt(created_timestamp, 0).single() {
    None => Err(format!(
      "[Import] failed to parse timestamp: {}",
      created_timestamp
    )),
    Some(created_at) => {
      let now = Utc::now();
      if created_at > now {
        return Err(format!(
          "[Import] created_at is in the future: {} > {}",
          created_at.format("%m/%d/%y %H:%M"),
          now.format("%m/%d/%y %H:%M")
        ));
      }

      let elapsed = now - created_at;
      let hours = get_env_var("APPFLOWY_WORKER_IMPORT_TASK_PROCESS_EXPIRE_HOURS", "6")
        .parse::<i64>()
        .unwrap_or(6);

      if elapsed.num_hours() >= hours {
        return Err(format!(
          "task is expired: created_at: {}, last_process_at: {:?}, elapsed: {} hours",
          created_at.format("%m/%d/%y %H:%M"),
          last_process_at,
          elapsed.num_hours()
        ));
      }

      if last_process_at.is_none() {
        return Ok(());
      }

      let elapsed = now - created_at;
      let minutes = get_env_var("APPFLOWY_WORKER_IMPORT_TASK_EXPIRE_MINUTES", "30")
        .parse::<i64>()
        .unwrap_or(30);

      if elapsed.num_minutes() >= minutes {
        Err(format!(
          "[Import] task is expired: created_at: {}, last_process_at: {:?}, elapsed: {} minutes",
          created_at.format("%m/%d/%y %H:%M"),
          last_process_at,
          elapsed.num_minutes()
        ))
      } else {
        Ok(())
      }
    },
  }
}

async fn push_task(
  redis_client: &mut ConnectionManager,
  stream_name: &str,
  _group_name: &str,
  task: ImportTask,
  entry_id: &str,
) -> Result<(), ImportError> {
  let task_str = serde_json::to_string(&task).map_err(|e| {
    error!("Failed to serialize task: {:?}", e);
    ImportError::Internal(e.into())
  })?;

  let mut pipeline = redis::pipe();
  pipeline
      .atomic() // Ensures the commands are executed atomically
      .cmd("XDEL") // delete the task
      .arg(stream_name)
      .arg(entry_id)
      .ignore() // Ignore the result of XDEL
      .cmd("XADD") // Re-add the task to the stream
      .arg(stream_name)
      .arg("*")
      .arg("task")
      .arg(task_str);

  let result: Result<(), redis::RedisError> = pipeline.query_async(redis_client).await;
  match result {
    Ok(_) => Ok(()),
    Err(err) => {
      error!(
        "Failed to execute transaction for re-adding task: {:?}",
        err
      );
      Err(ImportError::Internal(err.into()))
    },
  }
}

async fn delete_task(
  redis_client: &mut ConnectionManager,
  stream_name: &str,
  _group_name: &str,
  entry_id: &str,
) -> Result<(), ImportError> {
  let _: () = redis_client
    .xdel(stream_name, &[entry_id])
    .await
    .map_err(|e| {
      error!("Failed to delete import task: {:?}", e);
      ImportError::Internal(e.into())
    })?;
  Ok(())
}

async fn process_task(
  mut context: TaskContext,
  import_task: ImportTask,
) -> Result<(), ImportError> {
  let retry_interval: u64 = get_env_var("APPFLOWY_WORKER_IMPORT_TASK_RETRY_INTERVAL", "10")
    .parse()
    .unwrap_or(10);

  let streaming = get_env_var("APPFLOWY_WORKER_IMPORT_TASK_STREAMING", "false")
    .parse()
    .unwrap_or(false);

  info!("[Import]: Processing task: {}", import_task);

  match import_task {
    ImportTask::Notion(task) => {
      // 1. download zip file
      let unzip_result = download_and_unzip_file_retry(
        &context.storage_dir,
        &task,
        &context.s3_client,
        3,
        Duration::from_secs(retry_interval),
        streaming,
        &context.metrics,
      )
      .await;

      trace!(
        "[Import]: {} download and unzip file result: {:?}",
        task.workspace_id,
        unzip_result
      );
      match unzip_result {
        Ok(unzip_dir_path) => {
          // 2. process unzip file
          let result = process_unzip_file(
            &task,
            &unzip_dir_path,
            &context.pg_pool,
            &mut context.redis_client,
            &context.s3_client,
          )
          .await;

          // If there is any errors when processing the unzip file, we will remove the workspace and notify the user.
          if result.is_err() {
            info!(
              "[Import]: failed to import notion file, delete workspace:{}",
              task.workspace_id
            );
            remove_workspace(&task.workspace_id, &context.pg_pool).await;
          }

          clean_up(&context.s3_client, &task).await;
          notify_user(&task, result, context.notifier, &context.metrics).await?;

          tokio::spawn(async move {
            match fs::remove_dir_all(&unzip_dir_path).await {
              Ok(_) => info!(
                "[Import]: {} deleted unzip file: {:?}",
                task.workspace_id, unzip_dir_path
              ),
              Err(err) => {
                if err.kind() != ErrorKind::NotFound {
                  error!("Failed to delete unzip file: {:?}", err);
                }
              },
            }
          });
        },
        Err(err) => {
          // If there is any errors when download or unzip the file, we will remove the file from S3 and notify the user.
          if let Err(err) = &context.s3_client.delete_blob(task.s3_key.as_str()).await {
            error!("Failed to delete zip file from S3: {:?}", err);
          }
          remove_workspace(&task.workspace_id, &context.pg_pool).await;
          clean_up(&context.s3_client, &task).await;
          notify_user(&task, Err(err), context.notifier, &context.metrics).await?;
        },
      }

      Ok(())
    },
    ImportTask::Custom(value) => {
      trace!("Custom task: {:?}", value);
      let result = ImportResult {
        user_name: "".to_string(),
        user_email: "".to_string(),
        is_success: true,
        value: Default::default(),
      };
      context
        .notifier
        .notify_progress(ImportProgress::Finished(result))
        .await;
      Ok(())
    },
  }
}
/// Retries the download and unzipping of a file from an S3 source.
///
/// This function attempts to download a zip file from an S3 bucket and unzip it to a local directory.
/// If the operation fails, it will retry up to `max_retries` times, waiting for `interval` between each attempt.
///
pub async fn download_and_unzip_file_retry(
  storage_dir: &Path,
  import_task: &NotionImportTask,
  s3_client: &Arc<dyn S3Client>,
  max_retries: usize,
  interval: Duration,
  streaming: bool,
  metrics: &Option<Arc<ImportMetrics>>,
) -> Result<PathBuf, ImportError> {
  let mut attempt = 0;
  loop {
    attempt += 1;
    match download_and_unzip_file(storage_dir, import_task, s3_client, streaming, metrics).await {
      Ok(result) => return Ok(result),
      Err(err) => {
        // If the Upload file not found error occurs, we will not retry.
        if matches!(err, ImportError::UploadFileNotFound) {
          return Err(err);
        }

        if attempt < max_retries && !err.is_file_not_found() {
          warn!(
            "{} attempt {} failed: {}. Retrying in {:?}...",
            import_task.workspace_id, attempt, err, interval
          );
          tokio::time::sleep(interval).await;
        } else {
          return Err(ImportError::Internal(anyhow!(
            "Failed after {} attempts: {}",
            attempt,
            err
          )));
        }
      },
    }
  }
}
/// Downloads a zip file from S3 and unzips it to the local directory.
///
/// This function fetches a zip file from an S3 source using the provided S3 client,
/// downloads it (if needed), and unzips the contents to the specified local directory.
///
async fn download_and_unzip_file(
  storage_dir: &Path,
  import_task: &NotionImportTask,
  s3_client: &Arc<dyn S3Client>,
  streaming: bool,
  metrics: &Option<Arc<ImportMetrics>>,
) -> Result<PathBuf, ImportError> {
  let blob_meta = s3_client.get_blob_meta(import_task.s3_key.as_str()).await?;
  match blob_meta.content_type {
    None => {
      error!(
        "[Import] {} failed to get content type for file: {:?}",
        import_task.workspace_id, import_task.s3_key
      );
    },
    Some(content_type) => {
      let valid_zip_types = [
        "application/zip",
        "application/x-zip-compressed",
        "multipart/x-zip",
        "application/x-compressed",
      ];

      if !valid_zip_types.contains(&content_type.as_str()) {
        return Err(ImportError::Internal(anyhow!(
          "Invalid content type: {}",
          content_type
        )));
      }
    },
  }

  let max_content_length = get_env_var(
    "APPFLOWY_WORKER_IMPORT_TASK_MAX_FILE_SIZE_BYTES",
    MAXIMUM_CONTENT_LENGTH,
  )
  .parse::<i64>()
  .unwrap();
  if blob_meta.content_length > max_content_length {
    return Err(ImportError::Internal(anyhow!(
      "File size is too large: {} bytes, max allowed: {} bytes",
      blob_meta.content_length,
      max_content_length
    )));
  }

  trace!(
    "[Import] {} start download file: {:?}, size: {}",
    import_task.workspace_id,
    import_task.s3_key,
    blob_meta.content_length
  );

  let S3StreamResponse {
    stream,
    content_type: _,
    content_length,
  } = s3_client
    .get_blob_stream(import_task.s3_key.as_str())
    .await?;

  let buffer_size = buffer_size_from_content_length(content_length);
  if let Some(metrics) = metrics {
    metrics.record_import_size_bytes(buffer_size);
  }
  if streaming {
    let zip_reader = get_zip_reader(buffer_size, StreamOrFile::Stream(stream)).await?;
    let unique_file_name = Uuid::new_v4().to_string();
    let output_file_path = storage_dir.join(unique_file_name);
    fs::create_dir_all(&output_file_path)
      .await
      .map_err(|err| ImportError::Internal(err.into()))?;
    fs::set_permissions(&output_file_path, Permissions::from_mode(0o777))
      .await
      .map_err(|err| {
        ImportError::Internal(anyhow!("Failed to set permissions for temp dir: {:?}", err))
      })?;
    let unzip_file = async_unzip(
      zip_reader.inner,
      output_file_path,
      Some(import_task.workspace_name.clone()),
    )
    .await?;
    Ok(unzip_file.unzip_dir_path)
  } else {
    let file = download_file(
      &import_task.workspace_id,
      storage_dir,
      stream,
      &import_task.md5_base64,
    )
    .await?;
    trace!(
      "[Import] {} start unzip file: {:?}",
      import_task.workspace_id,
      file.path_buf()
    );

    let file_path = file.path_buf().clone();
    let storage_dir = storage_dir.to_path_buf();
    let workspace_name = import_task.workspace_name.clone();
    let unzip_file =
      tokio::task::spawn_blocking(move || sync_unzip(file_path, storage_dir, Some(workspace_name)))
        .await
        .map_err(|err| ImportError::Internal(err.into()))??;

    info!(
      "[Import] {} finish unzip file to dir:{}, file:{:?}",
      import_task.workspace_id, unzip_file.dir_name, unzip_file.unzip_dir
    );
    Ok(unzip_file.unzip_dir)
  }
}

struct ZipReader {
  inner: ZipFileReader<Ready<Pin<Box<dyn AsyncBufRead + Unpin + Send>>>>,
  #[allow(dead_code)]
  file: Option<AutoRemoveDownloadedFile>,
}

#[allow(dead_code)]
enum StreamOrFile {
  Stream(Box<dyn AsyncBufRead + Unpin + Send>),
  File(AutoRemoveDownloadedFile),
}

/// Asynchronously returns a `ZipFileReader` that can read from a stream or a downloaded file, based on the environment setting.
///
/// This function checks whether streaming is enabled via the `APPFLOWY_WORKER_IMPORT_TASK_STREAMING` environment variable.
/// If streaming is enabled, it reads the zip file directly from the provided stream.
/// Otherwise, it first downloads the zip file to a local file and then reads from it.
///
async fn get_zip_reader(
  buffer_size: usize,
  stream_or_file: StreamOrFile,
) -> Result<ZipReader, ImportError> {
  match stream_or_file {
    StreamOrFile::Stream(stream) => {
      // Occasionally, we encounter the error 'unable to locate the end of central directory record'
      // when streaming a ZIP file to async-zip. This indicates that the ZIP reader couldn't find
      // the necessary end-of-file marker. The issue might occur if the entire ZIP file has not been
      // fully downloaded or buffered before the reader attempts to process the end-of-file information.
      let reader = futures::io::BufReader::with_capacity(buffer_size, stream);
      let boxed_reader: Pin<Box<dyn AsyncBufRead + Unpin + Send>> = Box::pin(reader);
      Ok(ZipReader {
        inner: async_zip::base::read::stream::ZipFileReader::new(boxed_reader),
        file: None,
      })
    },
    StreamOrFile::File(file) => {
      let handle = fs::File::open(&file)
        .await
        .map_err(|err| ImportError::Internal(err.into()))?;
      let reader = tokio::io::BufReader::with_capacity(buffer_size, handle).compat();
      let boxed_reader: Pin<Box<dyn AsyncBufRead + Unpin + Send>> = Box::pin(reader);
      Ok(ZipReader {
        inner: async_zip::base::read::stream::ZipFileReader::new(boxed_reader),
        // Make sure the lifetime of file is the same as zip reader.
        file: Some(file),
      })
    },
  }
}

/// Determines the buffer size based on the content length of the file.
/// If the buffer is too small, the zip reader will frequently pause to fetch more data,
/// causing delays. This can make the unzip process appear slower and can even cause premature
/// errors (like EOF) if there is a delay in fetching more data.
#[inline]
fn buffer_size_from_content_length(content_length: Option<i64>) -> usize {
  match content_length {
    Some(file_size) => {
      if file_size < 10 * 1024 * 1024 {
        3 * 1024 * 1024
      } else if file_size < 100 * 1024 * 1024 {
        5 * 1024 * 1024 // 5MB buffer
      } else {
        10 * 1024 * 1024 // 10MB buffer
      }
    },
    None => 3 * 1024 * 1024,
  }
}

async fn process_unzip_file(
  import_task: &NotionImportTask,
  unzip_dir_path: &PathBuf,
  pg_pool: &PgPool,
  redis_client: &mut ConnectionManager,
  s3_client: &Arc<dyn S3Client>,
) -> Result<(), ImportError> {
  let client_id = default_client_id();
  let _ =
    Uuid::parse_str(&import_task.workspace_id).map_err(|err| ImportError::Internal(err.into()))?;
  let notion_importer = NotionImporter::new(
    import_task.uid,
    unzip_dir_path,
    import_task.workspace_id.clone(),
    import_task.host.clone(),
  )
  .map_err(ImportError::ImportCollabError)?;

  trace!(
    "[Import]: {} start import notion data",
    import_task.workspace_id
  );
  let imported = notion_importer
    .import()
    .await
    .map_err(ImportError::ImportCollabError)?;
  let nested_views = imported.build_nested_views().await;
  trace!(
    "[Import]: {} imported nested views:{}",
    import_task.workspace_id,
    nested_views
  );

  // 1. Open the workspace folder
  let workspace_id = Uuid::parse_str(&imported.workspace_id)?;
  let folder_collab = get_encode_collab_from_bytes(
    &workspace_id,
    &workspace_id,
    &CollabType::Folder,
    pg_pool,
    s3_client,
  )
  .await?;
  let mut folder = Folder::from_collab_doc_state(
    CollabOrigin::Server,
    folder_collab.into(),
    &imported.workspace_id,
    client_id,
  )
  .map_err(|err| ImportError::CannotOpenWorkspace(err.to_string()))?;

  // 2. Insert collabs' views into the folder
  trace!(
    "[Import]: {} insert views:{} to folder",
    import_task.workspace_id,
    nested_views.len()
  );
  folder.insert_nested_views(nested_views.into_inner(), import_task.uid);

  let mut resources = vec![];
  let mut collab_params_list = vec![];
  let mut database_view_ids_by_database_id: HashMap<String, Vec<String>> = HashMap::new();
  let mut orphan_view_ids = HashSet::new();

  // 3. Collect all collabs and resources
  let mut stream = imported.into_collab_stream().await;
  let updated_at = Utc::now();
  while let Some(imported_collab_info) = stream.next().await {
    trace!(
      "[Import]: {} imported collab: {}",
      import_task.workspace_id,
      imported_collab_info
    );
    resources.extend(imported_collab_info.resources);
    collab_params_list.extend(
      imported_collab_info
        .imported_collabs
        .into_iter()
        .map(|imported_collab| CollabParams {
          object_id: imported_collab.object_id.parse().unwrap(),
          collab_type: imported_collab.collab_type,
          encoded_collab_v1: Bytes::from(imported_collab.encoded_collab.encode_to_bytes().unwrap()),
          updated_at: Some(updated_at),
        })
        .collect::<Vec<_>>(),
    );

    match imported_collab_info.import_type {
      ImportType::Database {
        database_id,
        view_ids,
        row_document_ids,
      } => {
        database_view_ids_by_database_id.insert(database_id, view_ids);
        orphan_view_ids.extend(row_document_ids);
      },
      ImportType::Document => {
        // do nothing
      },
    }
  }

  let w_database_id = select_workspace_database_storage_id(pg_pool, &import_task.workspace_id)
    .await
    .map_err(|err| {
      ImportError::Internal(anyhow!(
        "Failed to select workspace database storage id: {:?}",
        err
      ))
    })?;

  // 4. Edit workspace database collab and then encode workspace database collab
  if !database_view_ids_by_database_id.is_empty() {
    let w_db_collab = get_encode_collab_from_bytes(
      &workspace_id,
      &w_database_id,
      &CollabType::WorkspaceDatabase,
      pg_pool,
      s3_client,
    )
    .await?;
    let mut w_database = WorkspaceDatabase::from_collab_doc_state(
      &w_database_id.to_string(),
      CollabOrigin::Server,
      w_db_collab.into(),
      client_id,
    )
    .map_err(|err| ImportError::CannotOpenWorkspace(err.to_string()))?;
    w_database.batch_add_database(database_view_ids_by_database_id);

    let w_database_collab = w_database.encode_collab_v1().map_err(|err| {
      ImportError::Internal(anyhow!(
        "Failed to encode workspace database collab: {:?}",
        err
      ))
    })?;

    match w_database_collab.encode_to_bytes() {
      Ok(bytes) => {
        if let Err(err) = redis_client
          .set_ex::<String, Vec<u8>, Value>(
            encode_collab_key(w_database_id.to_string()),
            bytes,
            2592000, // WorkspaceDatabase => 1 month
          )
          .await
        {
          warn!(
            "[Import] Failed to insert workspace database to Redis: {}",
            err
          );
        }
      },
      Err(err) => warn!(
        "[Import] Failed to encode workspace database collab payload: {}",
        err
      ),
    }

    trace!(
      "[Import]: {} did encode workspace database collab",
      import_task.workspace_id
    );
    let w_database_collab_params = CollabParams {
      object_id: w_database_id,
      collab_type: CollabType::WorkspaceDatabase,
      encoded_collab_v1: Bytes::from(w_database_collab.encode_to_bytes().unwrap()),
      updated_at: Some(updated_at),
    };
    collab_params_list.push(w_database_collab_params);
  }

  // 5. Insert orphan view to folder
  let orphan_views = orphan_view_ids
    .into_iter()
    .map(|orphan_view_id| {
      View::orphan_view(&orphan_view_id, ViewLayout::Document, Some(import_task.uid))
    })
    .collect::<Vec<_>>();
  if !orphan_views.is_empty() {
    folder.insert_views(orphan_views, import_task.uid);
  }

  // 6. Encode Folder
  let folder_collab = folder
    .encode_collab_v1(|collab| CollabType::Folder.validate_require_data(collab))
    .map_err(|err| ImportError::Internal(err.into()))?;

  match folder_collab.encode_to_bytes() {
    Ok(bytes) => {
      if let Err(err) = redis_client
        .set_ex::<String, Vec<u8>, Value>(
          encode_collab_key(&import_task.workspace_id),
          bytes,
          604800, // Folder => 1 week
        )
        .await
      {
        warn!("[Import] Failed to insert folder collab to Redis: {}", err);
      }
    },
    Err(err) => warn!("[Import] Failed to encode folder collab payload: {}", err),
  }

  let folder_collab_params = CollabParams {
    object_id: workspace_id,
    collab_type: CollabType::Folder,
    encoded_collab_v1: Bytes::from(folder_collab.encode_to_bytes().unwrap()),
    updated_at: Some(updated_at),
  };
  trace!(
    "[Import]: {} did encode folder collab",
    import_task.workspace_id
  );
  collab_params_list.push(folder_collab_params);

  let upload_resources = process_resources(resources).await;

  // 7. Start a transaction to insert all collabs
  let mut transaction = pg_pool.begin().await.map_err(|err| {
    ImportError::Internal(anyhow!(
      "Failed to start transaction when importing data: {:?}",
      err
    ))
  })?;

  trace!(
    "[Import]: {} insert collabs into database",
    import_task.workspace_id
  );

  // 8. write all collab to disk
  insert_into_af_collab_bulk_for_user(
    &mut transaction,
    &import_task.uid,
    workspace_id,
    &collab_params_list,
  )
  .await
  .map_err(|err| {
    ImportError::Internal(anyhow!(
      "Failed to insert collabs into database when importing data: {:?}",
      err
    ))
  })?;

  trace!(
    "[Import]: {} update task:{} status to completed",
    import_task.workspace_id,
    import_task.task_id,
  );
  update_import_task_status(
    &import_task.task_id,
    ImportTaskState::Completed,
    transaction.deref_mut(),
  )
  .await
  .map_err(|err| {
    ImportError::Internal(anyhow!(
      "Failed to update import task status when importing data: {:?}",
      err
    ))
  })?;

  trace!(
    "[Import]: {} set is_initialized to true",
    import_task.workspace_id,
  );
  update_workspace_status(transaction.deref_mut(), &workspace_id, true)
    .await
    .map_err(|err| {
      ImportError::Internal(anyhow!(
        "Failed to update workspace status when importing data: {:?}",
        err
      ))
    })?;

  // Set the workspace's updated_at to the earliest possible timestamp, as it is created by an import task
  // and not actively updated by a user. This ensures that when sorting workspaces by updated_at to find
  // the most recent, the imported workspace doesn't appear as the most recently visited workspace.
  let updated_at = DateTime::from_timestamp(0, 0).unwrap_or_else(Utc::now);
  update_updated_at_of_workspace_with_uid(
    transaction.deref_mut(),
    import_task.uid,
    &workspace_id,
    updated_at,
  )
  .await
  .map_err(|err| {
    ImportError::Internal(anyhow!(
      "Failed to update workspace updated_at when importing data: {:?}",
      err
    ))
  })?;

  // insert metadata into database
  let metas = upload_resources
    .iter()
    .map(|res| res.meta.clone())
    .collect::<Vec<_>>();
  let affected_rows = insert_blob_metadata_bulk(transaction.deref_mut(), &workspace_id, metas)
    .await
    .map_err(|err| {
      ImportError::Internal(anyhow!(
        "Failed to insert blob metadata into database when importing data: {:?}",
        err
      ))
    })?;

  if affected_rows != upload_resources.len() as u64 {
    warn!(
      "[Import]: {}, Affected rows: {}, upload resources: {}",
      import_task.workspace_id,
      affected_rows,
      upload_resources.len()
    );
  }

  let result = transaction.commit().await.map_err(|err| {
    ImportError::Internal(anyhow!(
      "Failed to commit transaction when importing data: {:?}",
      err
    ))
  });

  if result.is_err() {
    let _: RedisResult<Value> = redis_client.del(encode_collab_key(w_database_id)).await;
    let _: RedisResult<Value> = redis_client
      .del(encode_collab_key(&import_task.workspace_id))
      .await;

    return result;
  }

  // 9. after inserting all collabs, upload all files to S3
  trace!("[Import]: {} upload files to s3", import_task.workspace_id,);
  batch_upload_files_to_s3(&import_task.workspace_id, s3_client, upload_resources)
    .await
    .map_err(|err| ImportError::Internal(anyhow!("Failed to upload files to S3: {:?}", err)))?;
  Ok(())
}

async fn clean_up(s3_client: &Arc<dyn S3Client>, task: &NotionImportTask) {
  if let Err(err) = s3_client.delete_blob(task.s3_key.as_str()).await {
    error!("Failed to delete zip file from S3: {:?}", err);
  }
}

async fn remove_workspace(workspace_id: &str, pg_pool: &PgPool) {
  if let Ok(workspace_id) = Uuid::from_str(workspace_id) {
    if let Err(err) = delete_from_workspace(pg_pool, &workspace_id).await {
      error!(
        "Failed to delete workspace: {:?} when fail to import notion file",
        err
      );
    }
  }
}

async fn notify_user(
  import_task: &NotionImportTask,
  result: Result<(), ImportError>,
  notifier: Arc<dyn ImportNotifier>,
  metrics: &Option<Arc<ImportMetrics>>,
) -> Result<(), ImportError> {
  let task_id = import_task.task_id.to_string();
  let (error, error_detail) = match result {
    Ok(_) => {
      info!("[Import]: successfully imported:{}", import_task);
      if let Some(metrics) = metrics {
        metrics.incr_import_success_count(1);
      }
      (None, None)
    },
    Err(err) => {
      error!(
        "[Import]: failed to import:{}: error:{:?}",
        import_task, err
      );
      if let Some(metrics) = metrics {
        metrics.incr_import_fail_count(1);
      }
      let (error, error_detail) = err.report(&task_id);
      (Some(error), Some(error_detail))
    },
  };

  let is_success = error.is_none();

  let value = serde_json::to_value(ImportNotionMailerParam {
    import_task_id: task_id,
    user_name: import_task.user_name.clone(),
    import_file_name: import_task.workspace_name.clone(),
    workspace_id: import_task.workspace_id.clone(),
    workspace_name: import_task.workspace_name.clone(),
    open_workspace: false,
    error,
    error_detail,
  })
  .unwrap();

  notifier
    .notify_progress(ImportProgress::Finished(ImportResult {
      user_name: import_task.user_name.clone(),
      user_email: import_task.user_email.clone(),
      is_success,
      value,
    }))
    .await;
  Ok(())
}

async fn batch_upload_files_to_s3(
  workspace_id: &str,
  client: &Arc<dyn S3Client>,
  resources: Vec<UploadCollabResource>,
) -> Result<(), anyhow::Error> {
  // Create a stream of upload tasks
  let upload_stream = stream::iter(resources.into_iter().map(|res| async move {
    match upload_file_to_s3(
      client,
      workspace_id,
      &res.object_id,
      &res.meta.file_id,
      &res.meta.file_type,
      &res.file_path,
    )
    .await
    {
      Ok(_) => {
        trace!("Successfully uploaded: {}", res);
        Ok(())
      },
      Err(e) => {
        error!("Failed to upload {}: {:?}", res, e);
        Err(e)
      },
    }
  }))
  .buffer_unordered(5);
  let results: Vec<_> = upload_stream.collect().await;
  let errors: Vec<_> = results.into_iter().filter_map(Result::err).collect();

  if !errors.is_empty() {
    error!("Some uploads failed: {:?}", errors);
  }

  Ok(())
}

async fn upload_file_to_s3(
  client: &Arc<dyn S3Client>,
  workspace_id: &str,
  object_id: &str,
  file_id: &str,
  file_type: &str,
  file_path: &str,
) -> Result<(), anyhow::Error> {
  let path = Path::new(file_path);
  if !path.exists() {
    return Err(anyhow!("File does not exist: {:?}", path));
  }

  let mut attempt = 0;
  let max_retries = 2;

  let object_key = format!("{}/{}/{}", workspace_id, object_id, file_id);
  while attempt <= max_retries {
    let byte_stream = ByteStream::from_path(path).await?;
    match client
      .put_blob(&object_key, byte_stream, Some(file_type))
      .await
    {
      Ok(_) => return Ok(()),
      Err(WorkerError::S3ServiceUnavailable(_)) if attempt < max_retries => {
        attempt += 1;
        tokio::time::sleep(Duration::from_secs(3)).await;
      },
      Err(err) => return Err(err.into()),
    }
  }

  Err(anyhow!(
    "Failed to upload file to S3 after {} attempts",
    max_retries + 1
  ))
}

async fn get_encode_collab_from_bytes(
  workspace_id: &Uuid,
  object_id: &Uuid,
  collab_type: &CollabType,
  pg_pool: &PgPool,
  s3: &Arc<dyn S3Client>,
) -> Result<EncodedCollab, ImportError> {
  let key = collab_key(workspace_id, object_id);
  match s3.get_blob_stream(&key).await {
    Ok(mut resp) => {
      let mut buf = Vec::with_capacity(resp.content_length.unwrap_or(1024) as usize);
      resp
        .stream
        .read_to_end(&mut buf)
        .await
        .map_err(|err| ImportError::Internal(err.into()))?;
      let decompressed = zstd::decode_all(&*buf).map_err(|e| ImportError::Internal(e.into()))?;
      Ok(EncodedCollab {
        state_vector: Default::default(),
        doc_state: decompressed.into(),
        version: EncoderVersion::V1,
      })
    },
    Err(WorkerError::RecordNotFound(_)) => {
      // fallback to postgres
      let (_, bytes) = select_blob_from_af_collab(pg_pool, collab_type, object_id)
        .await
        .map_err(|err| ImportError::Internal(err.into()))?;

      Ok(
        EncodedCollab::decode_from_bytes(&bytes)
          .map_err(|err| ImportError::Internal(err.into()))?,
      )
    },
    Err(err) => Err(err.into()),
  }
}

/// Ensure the consumer group exists, if not, create it.
async fn ensure_consumer_group(
  stream_key: &str,
  group_name: &str,
  redis_client: &mut ConnectionManager,
) -> Result<(), WorkerError> {
  let result: RedisResult<()> = redis_client
    .xgroup_create_mkstream(stream_key, group_name, "0")
    .await;

  if let Err(redis_error) = result {
    if let Some(code) = redis_error.code() {
      if code == "BUSYGROUP" {
        return Ok(());
      }

      if code == "NOGROUP" {
        return Err(WorkerError::StreamGroupNotExist(group_name.to_string()));
      }
    }
    error!("Error when creating consumer group: {:?}", redis_error);
    return Err(WorkerError::Internal(redis_error.into()));
  }

  Ok(())
}

struct UnAckTask {
  stream_id: StreamId,
  task: ImportTask,
}

async fn get_un_ack_tasks(
  stream_key: &str,
  group_name: &str,
  consumer_name: &str,
  redis_client: &mut ConnectionManager,
) -> Result<Vec<UnAckTask>, anyhow::Error> {
  let reply: StreamPendingReply = redis_client.xpending(stream_key, group_name).await?;
  match reply {
    StreamPendingReply::Empty => Ok(vec![]),
    StreamPendingReply::Data(pending) => {
      let opts = StreamClaimOptions::default()
        .idle(500)
        .with_force()
        .retry(2);

      // If the start_id and end_id are the same, we only need to claim one message.
      let mut ids = Vec::with_capacity(2);
      ids.push(pending.start_id.clone());
      if pending.start_id != pending.end_id {
        ids.push(pending.end_id);
      }

      let result: StreamClaimReply = redis_client
        .xclaim_options(stream_key, group_name, consumer_name, 500, &ids, opts)
        .await?;

      let tasks = result
        .ids
        .into_iter()
        .filter_map(|stream_id| {
          ImportTask::try_from(&stream_id)
            .map(|task| UnAckTask { stream_id, task })
            .ok()
        })
        .collect::<Vec<_>>();

      trace!("Claimed tasks: {}", tasks.len());
      Ok(tasks)
    },
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotionImportTask {
  pub uid: i64,
  pub user_name: String,
  pub user_email: String,
  pub task_id: Uuid,
  pub workspace_id: String,
  pub workspace_name: String,
  pub s3_key: String,
  pub host: String,
  #[serde(default)]
  pub created_at: Option<i64>,
  #[serde(default)]
  pub md5_base64: Option<String>,
  #[serde(default)]
  pub last_process_at: Option<i64>,
  #[serde(default)]
  pub file_size: Option<i64>,
}

impl Display for NotionImportTask {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let file_size_mb = self.file_size.map(|size| size as f64 / 1_048_576.0);
    write!(
      f,
      "NotionImportTask {{ task_id: {}, workspace_id: {}, file_size:{:?}MB, workspace_name: {}, user_name: {}, user_email: {} }}",
      self.task_id, self.workspace_id, file_size_mb, self.workspace_name, self.user_name, self.user_email
    )
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportTask {
  // boxing the large fields to reduce the total size of the enum
  Notion(Box<NotionImportTask>),
  Custom(serde_json::Value),
}

impl Display for ImportTask {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ImportTask::Notion(task) => write!(
        f,
        "NotionImportTask {{ workspace_id: {}, workspace_name: {} }}",
        task.workspace_id, task.workspace_name
      ),
      ImportTask::Custom(value) => write!(f, "CustomTask {{ {} }}", value),
    }
  }
}

impl TryFrom<&StreamId> for ImportTask {
  type Error = ImportError;

  fn try_from(stream_id: &StreamId) -> Result<Self, Self::Error> {
    let task_str = match stream_id.map.get("task") {
      Some(value) => match value {
        Value::SimpleString(value) => value.to_string(),
        Value::BulkString(data) => String::from_utf8_lossy(data).to_string(),
        _ => {
          error!("Unexpected value type for task field: {:?}", value);
          return Err(ImportError::Internal(anyhow!(
            "Unexpected value type for task field: {:?}",
            value
          )));
        },
      },
      None => {
        error!("Task field not found in Redis stream entry");
        return Err(ImportError::Internal(anyhow!(
          "Task field not found in Redis stream entry"
        )));
      },
    };

    from_str::<ImportTask>(&task_str).map_err(|err| ImportError::Internal(err.into()))
  }
}

async fn process_resources(resources: Vec<CollabResource>) -> Vec<UploadCollabResource> {
  let upload_resources_stream = stream::iter(resources)
    .flat_map(|resource| {
      let object_id = resource.object_id.clone();
      stream::iter(resource.files.into_iter().map(move |file_path| {
        let object_id = object_id.clone();
        let path = PathBuf::from(file_path.clone());
        async move {
          match insert_meta_from_path(&object_id, &path).await {
            Ok(meta) => Some(UploadCollabResource {
              object_id,
              file_path,
              meta,
            }),
            Err(_) => None,
          }
        }
      }))
    })
    // buffer_unordered method limits how many futures (tasks) are run concurrently.
    .buffer_unordered(20);

  upload_resources_stream
    .filter_map(|result| async { result })
    .collect::<Vec<UploadCollabResource>>()
    .await
}

struct UploadCollabResource {
  object_id: String,
  file_path: String,
  meta: BulkInsertMeta,
}

impl Display for UploadCollabResource {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "UploadCollabResource {{ object_id: {}, file_path: {}, file_size: {} }}",
      self.object_id, self.file_path, self.meta.file_size
    )
  }
}

async fn insert_meta_from_path(
  object_id: &str,
  path: &PathBuf,
) -> Result<BulkInsertMeta, ImportError> {
  let file_id = FileId::from_path(path).await?;
  let object_id = object_id.to_string();
  let file_type = mime_guess::from_path(path)
    .first_or_octet_stream()
    .to_string();
  let file_size = fs::metadata(path)
    .await
    .map_err(|err| ImportError::Internal(err.into()))?
    .len() as i64;

  Ok(BulkInsertMeta {
    object_id,
    file_id,
    file_type,
    file_size,
  })
}

fn collab_key(workspace_id: &Uuid, object_id: &Uuid) -> String {
  format!(
    "collabs/{}/{}/encoded_collab.v1.zstd",
    workspace_id, object_id
  )
}

fn encode_collab_key<T: Display>(object_id: T) -> String {
  format!("encode_collab_v0:{}", object_id)
}
