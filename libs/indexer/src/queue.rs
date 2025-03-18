use crate::error::IndexerError;
use crate::scheduler::UnindexedCollabTask;
use anyhow::anyhow;
use app_error::AppError;
use redis::aio::ConnectionManager;
use redis::streams::{StreamId, StreamReadOptions, StreamReadReply};
use redis::{AsyncCommands, RedisResult, Value};
use serde_json::from_str;
use tracing::error;

pub const INDEX_TASK_STREAM_NAME: &str = "index_collab_task_stream";
const INDEXER_WORKER_GROUP_NAME: &str = "indexer_worker_group";
const INDEXER_CONSUMER_NAME: &str = "appflowy_worker";

impl TryFrom<&StreamId> for UnindexedCollabTask {
  type Error = IndexerError;

  fn try_from(stream_id: &StreamId) -> Result<Self, Self::Error> {
    let task_str = match stream_id.map.get("task") {
      Some(value) => match value {
        Value::BulkString(data) => String::from_utf8_lossy(data).to_string(),
        Value::SimpleString(value) => value.clone(),
        _ => {
          error!("Unexpected value type for task field: {:?}", value);
          return Err(IndexerError::Internal(anyhow!(
            "Unexpected value type for task field: {:?}",
            value
          )));
        },
      },
      None => {
        error!("Task field not found in Redis stream entry");
        return Err(IndexerError::Internal(anyhow!(
          "Task field not found in Redis stream entry"
        )));
      },
    };

    from_str::<UnindexedCollabTask>(&task_str).map_err(|err| IndexerError::Internal(err.into()))
  }
}

/// Adds a list of tasks to the Redis stream.
///
/// This function pushes a batch of `EmbedderTask` items into the Redis stream for processing.
/// The tasks are serialized into JSON format before being added to the stream.
///
pub async fn add_background_embed_task(
  redis_client: ConnectionManager,
  tasks: Vec<UnindexedCollabTask>,
) -> Result<(), AppError> {
  let items = tasks
    .into_iter()
    .flat_map(|task| {
      let task = serde_json::to_string(&task).ok()?;
      Some(("task", task))
    })
    .collect::<Vec<_>>();

  let _: () = redis_client
    .clone()
    .xadd(INDEX_TASK_STREAM_NAME, "*", &items)
    .await
    .map_err(|err| {
      AppError::Internal(anyhow!(
        "Failed to push embedder task to Redis stream: {}",
        err
      ))
    })?;
  Ok(())
}

/// Reads tasks from the Redis stream for processing by a consumer group.
pub async fn read_background_embed_tasks(
  redis_client: &mut ConnectionManager,
  options: &StreamReadOptions,
) -> Result<StreamReadReply, IndexerError> {
  let tasks: StreamReadReply = match redis_client
    .xread_options(&[INDEX_TASK_STREAM_NAME], &[">"], options)
    .await
  {
    Ok(tasks) => tasks,
    Err(err) => {
      error!("Failed to read tasks from Redis stream: {:?}", err);
      if let Some(code) = err.code() {
        if code == "NOGROUP" {
          return Err(IndexerError::StreamGroupNotExist(
            INDEXER_WORKER_GROUP_NAME.to_string(),
          ));
        }
      }
      return Err(IndexerError::Internal(err.into()));
    },
  };
  Ok(tasks)
}

/// Acknowledges a task in a Redis stream and optionally removes it from the stream.
///
/// It is used to acknowledge the processing of a task in a Redis stream
/// within a specific consumer group. Once a task is acknowledged, it is removed from
/// the **Pending Entries List (PEL)** for the consumer group. If the `delete_task`
/// flag is set to `true`, the task will also be removed from the Redis stream entirely.
///
/// # Parameters:
/// - `redis_client`: A mutable reference to the Redis `ConnectionManager`, used to
///   interact with the Redis server.
/// - `stream_entity_id`: The unique identifier (ID) of the task in the stream.
/// - `delete_task`: A boolean flag that indicates whether the task should be removed
///   from the stream after it is acknowledged. If `true`, the task is deleted from the stream.
///   If `false`, the task remains in the stream after acknowledgment.
pub async fn ack_task(
  redis_client: &mut ConnectionManager,
  stream_entity_ids: Vec<String>,
  delete_task: bool,
) -> Result<(), IndexerError> {
  let _: () = redis_client
    .xack(
      INDEX_TASK_STREAM_NAME,
      INDEXER_WORKER_GROUP_NAME,
      &stream_entity_ids,
    )
    .await
    .map_err(|err| {
      error!("Failed to ack task: {:?}", err);
      IndexerError::Internal(err.into())
    })?;

  if delete_task {
    let _: () = redis_client
      .xdel(INDEX_TASK_STREAM_NAME, &stream_entity_ids)
      .await
      .map_err(|err| {
        error!("Failed to delete task: {:?}", err);
        IndexerError::Internal(err.into())
      })?;
  }

  Ok(())
}

pub fn default_indexer_group_option(limit: usize) -> StreamReadOptions {
  StreamReadOptions::default()
    .group(INDEXER_WORKER_GROUP_NAME, INDEXER_CONSUMER_NAME)
    .count(limit)
}

/// Ensure the consumer group exists, if not, create it.
pub async fn ensure_indexer_consumer_group(
  redis_client: &mut ConnectionManager,
) -> Result<(), IndexerError> {
  let result: RedisResult<()> = redis_client
    .xgroup_create_mkstream(INDEX_TASK_STREAM_NAME, INDEXER_WORKER_GROUP_NAME, "0")
    .await;

  if let Err(redis_error) = result {
    if let Some(code) = redis_error.code() {
      if code == "BUSYGROUP" {
        return Ok(());
      }

      if code == "NOGROUP" {
        return Err(IndexerError::StreamGroupNotExist(
          INDEXER_WORKER_GROUP_NAME.to_string(),
        ));
      }
    }
    error!("Error when creating consumer group: {:?}", redis_error);
    return Err(IndexerError::Internal(redis_error.into()));
  }

  Ok(())
}
