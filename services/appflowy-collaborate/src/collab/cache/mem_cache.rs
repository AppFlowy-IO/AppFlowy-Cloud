use crate::collab::cache::{
  encode_collab_from_bytes, encode_collab_from_bytes_with_thread_pool, DECODE_SPAWN_THRESHOLD,
};
use crate::config::get_env_var;
use crate::CollabMetrics;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_proto::Rid;
use bytes::Bytes;
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use collab_stream::model::UpdateStreamMessage;
use collab_stream::stream_router::FromRedisStream;
use infra::thread_pool::ThreadPoolNoAbort;
use rayon::prelude::*;
use redis::streams::StreamRangeReply;
use redis::{pipe, AsyncCommands, FromRedisValue};
use std::fmt::Display;
use std::sync::Arc;
use tracing::{error, instrument, trace};
use uuid::Uuid;

const SEVEN_DAYS: u64 = 604800;
const ONE_MONTH: u64 = 2592000;

/// Threshold for spawning blocking tasks for encoding operations.
/// Data smaller than this will be processed on the current thread for efficiency.
/// Data larger than this will be spawned to avoid blocking the current thread.
const ENCODE_SPAWN_THRESHOLD: usize = 4096; // 4KB

#[derive(Debug, Clone, Copy)]
pub struct MillisSeconds(pub u64);

impl Display for MillisSeconds {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

impl From<&Rid> for MillisSeconds {
  fn from(rid: &Rid) -> Self {
    Self(rid.timestamp)
  }
}

impl From<u64> for MillisSeconds {
  fn from(value: u64) -> Self {
    Self(value)
  }
}

impl MillisSeconds {
  pub fn now() -> Self {
    Self(chrono::Utc::now().timestamp_millis() as u64)
  }

  pub fn into_inner(self) -> u64 {
    self.0
  }
}
#[derive(Clone)]
pub struct CollabMemCache {
  thread_pool: Arc<ThreadPoolNoAbort>,
  connection_manager: redis::aio::ConnectionManager,
  metrics: Arc<CollabMetrics>,
  /// Threshold for spawning background tasks for memory cache operations.
  /// Data smaller than this will be processed on the current thread.
  /// Data larger than this will be spawned to avoid blocking.
  small_collab_size: usize,
}

impl CollabMemCache {
  pub fn new(
    thread_pool: Arc<ThreadPoolNoAbort>,
    connection_manager: redis::aio::ConnectionManager,
    metrics: Arc<CollabMetrics>,
  ) -> Self {
    let small_collab_size = get_env_var("APPFLOWY_SMALL_COLLAB_SIZE", "4096")
      .parse::<usize>()
      .unwrap_or(DECODE_SPAWN_THRESHOLD);
    Self {
      thread_pool,
      connection_manager,
      metrics,
      small_collab_size,
    }
  }

  /// Checks if an object with the given ID exists in the cache.
  pub async fn is_exist(&self, object_id: &Uuid) -> Result<bool, AppError> {
    let cache_object_id = encode_collab_key(object_id);
    let exists: bool = self
      .connection_manager
      .clone()
      .exists(&cache_object_id)
      .await
      .map_err(|err| AppError::Internal(err.into()))?;
    Ok(exists)
  }

  pub async fn remove_encode_collab(&self, object_id: &Uuid) -> Result<(), AppError> {
    trace!("Removing encode collab from cache: {}", object_id);
    let cache_object_id = encode_collab_key(object_id);
    self
      .connection_manager
      .clone()
      .del::<&str, ()>(&cache_object_id)
      .await
      .map_err(|err| {
        AppError::Internal(anyhow!(
          "Failed to remove encoded collab from redis: {}",
          err
        ))
      })
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn get_encode_collab(&self, object_id: &Uuid) -> Option<(Rid, EncodedCollab)> {
    match self.get_data_with_timestamp(object_id).await {
      Ok(Some((timestamp, bytes))) => {
        let encoded_collab = encode_collab_from_bytes_with_thread_pool(&self.thread_pool, bytes)
          .await
          .ok()?;
        let rid = Rid::new(timestamp, 0);
        Some((rid, encoded_collab))
      },
      Ok(None) => None,
      Err(err) => {
        error!("Failed to get encoded collab from redis: {}", err);
        None
      },
    }
  }

  pub async fn batch_get_data(
    &self,
    object_ids: &[Uuid],
  ) -> Result<Vec<(Uuid, Vec<u8>)>, AppError> {
    if object_ids.is_empty() {
      return Ok(Vec::new());
    }

    trace!("Batch getting {} raw data", object_ids.len());
    let cache_keys: Vec<String> = object_ids.iter().map(encode_collab_key).collect();
    let mut conn = self.connection_manager.clone();
    let mut pipeline = pipe();
    for key in &cache_keys {
      pipeline.get(key);
    }

    let raw_results = pipeline
      .query_async::<Vec<Option<Vec<u8>>>>(&mut conn)
      .await
      .map_err(|err| AppError::Internal(anyhow!("Failed to batch get from Redis: {}", err)))?;

    let results: Vec<(Uuid, Vec<u8>)> = self
      .thread_pool
      .install(|| {
        raw_results
          .into_par_iter()
          .enumerate()
          .filter_map(|(i, raw_data)| {
            raw_data.and_then(|data| {
              let object_id = object_ids[i];
              self
                .extract_timestamp_and_payload_with_metrics(&data, Some(&object_id))
                .map(|(_, payload)| (object_id, payload))
            })
          })
          .collect()
      })
      .map_err(|err| {
        AppError::Internal(anyhow!(
          "Thread pool panic during batch processing: {}",
          err
        ))
      })?;

    Ok(results)
  }

  /// Batch retrieves multiple encoded collaborations efficiently using Redis pipeline.
  ///
  /// # Arguments
  /// * `object_ids` - A slice of object IDs to retrieve
  ///
  /// # Returns
  /// A vector of results in the same order as input, where each result is:
  /// - `Some((Rid, EncodedCollab))` if found and successfully decoded
  /// - `None` if not found or decoding failed
  ///
  /// # Performance
  /// Uses Redis pipelining to minimize network roundtrips and processes
  /// decoding tasks efficiently based on data size.
  #[instrument(level = "trace", skip_all, fields(count = object_ids.len()))]
  pub async fn batch_get_encode_collab(&self, object_ids: &[Uuid]) -> Vec<(Rid, EncodedCollab)> {
    if object_ids.is_empty() {
      return Vec::new();
    }

    trace!("Batch getting {} encoded collabs", object_ids.len());
    // Build cache keys
    let cache_keys: Vec<String> = object_ids.iter().map(encode_collab_key).collect();
    let mut conn = self.connection_manager.clone();
    let mut pipeline = pipe();
    for key in &cache_keys {
      pipeline.get(key);
    }

    // Execute pipeline
    let raw_results = match pipeline
      .query_async::<Vec<Option<Vec<u8>>>>(&mut conn)
      .await
    {
      Ok(results) => results,
      Err(err) => {
        error!("Failed to batch get from Redis: {}", err);
        return vec![];
      },
    };

    let final_results = self
      .thread_pool
      .install(|| {
        raw_results
          .into_par_iter()
          .enumerate()
          .filter_map(|(i, raw_data)| {
            let data = raw_data?;
            let object_id = object_ids.get(i);
            let (timestamp, encoded_collab) = self.extract_and_decode_collab(&data, object_id)?;
            let rid = Rid::new(timestamp, 0);
            Some((rid, encoded_collab))
          })
          .collect::<Vec<(Rid, EncodedCollab)>>()
      })
      .unwrap_or_else(|err| {
        error!(
          "Thread pool panic during batch encode collab processing: {}",
          err
        );
        vec![]
      });

    final_results
  }

  /// Retrieves a range of updates from the Redis stream for a given workspace ID.
  pub async fn get_workspace_updates(
    &self,
    workspace_id: &Uuid,
    object_id: Option<&Uuid>,
    from: Option<Rid>,
    to: Option<Rid>,
  ) -> Result<Vec<UpdateStreamMessage>, AppError> {
    let key = UpdateStreamMessage::stream_key(workspace_id);
    let from = from
      .map(|rid| rid.to_string())
      .unwrap_or_else(|| "-".into());
    let to = to.map(|rid| rid.to_string()).unwrap_or_else(|| "+".into());
    let mut conn = self.connection_manager.clone();
    let updates: StreamRangeReply = conn
      .xrange(key, from, to)
      .await
      .map_err(|err| AppError::Internal(err.into()))?;
    let mut result = Vec::with_capacity(updates.ids.len());
    for stream_id in updates.ids {
      if let Some(object_id) = object_id {
        let msg_oid = stream_id
          .map
          .get("oid")
          .and_then(|v| Uuid::from_redis_value(v).ok())
          .unwrap_or_default();
        if &msg_oid != object_id {
          continue; // this is not the object we are looking for
        }
      }
      let message = UpdateStreamMessage::from_redis_stream(&stream_id.id, &stream_id.map)?;
      result.push(message);
    }
    Ok(result)
  }

  #[instrument(level = "trace", skip_all, fields(object_id=%object_id))]
  pub async fn insert_encode_collab(
    &self,
    object_id: &Uuid,
    encoded_collab: EncodedCollab,
    mills_secs: MillisSeconds,
    expiration_seconds: u64,
  ) {
    trace!(
      "insert encode collab: {} updated_at: {}",
      object_id,
      mills_secs
    );
    // Estimate the size of the encoded data to decide whether to spawn a blocking task
    let estimated_size = encoded_collab.state_vector.len() + encoded_collab.doc_state.len();
    let bytes_result = if estimated_size <= ENCODE_SPAWN_THRESHOLD {
      // For small data, encode on current thread for efficiency
      encoded_collab.encode_to_bytes()
    } else {
      // For large data, spawn a blocking task to avoid blocking current thread
      match tokio::task::spawn_blocking(move || encoded_collab.encode_to_bytes()).await {
        Ok(result) => result,
        Err(e) => {
          error!("Failed to spawn blocking task for encoding: {}", e);
          return;
        },
      }
    };

    match bytes_result {
      Ok(bytes) => {
        if let Err(err) = self
          .insert_data_with_timestamp(object_id, &bytes, mills_secs, Some(expiration_seconds))
          .await
        {
          error!("Failed to cache encoded collab: {}", err);
        }
      },
      Err(err) => {
        error!("Failed to encode collab to bytes: {}", err);
      },
    }
  }

  /// Inserts data into Redis with a conditional timestamp.
  /// if the expiration_seconds is None, the data will be expired after 7 days.
  pub async fn insert_encode_collab_data(
    &self,
    object_id: &Uuid,
    data: &[u8],
    millis_secs: MillisSeconds,
    expiration_seconds: Option<u64>,
  ) -> redis::RedisResult<()> {
    self
      .insert_data_with_timestamp(object_id, data, millis_secs, expiration_seconds)
      .await
  }

  /// Batch inserts multiple raw data items efficiently using Redis pipeline.
  ///
  /// **Note: This function will override any existing values without timestamp comparison.**
  /// Use the single insert methods if you need conditional insertion based on timestamps.
  #[instrument(level = "trace", skip_all, fields(count = items.len()))]
  pub async fn batch_insert_raw_data(
    &self,
    items: &[(Uuid, Bytes, MillisSeconds, Option<u64>)],
  ) -> redis::RedisResult<()> {
    if items.is_empty() {
      return Ok(());
    }

    let mut conn = self.connection_manager.clone();
    let mut pipeline = pipe();
    pipeline.atomic();

    // Prepare all data with timestamps and add to pipeline
    for (object_id, data, timestamp, expiration_seconds) in items {
      trace!("insert collab: {} updated_at: {}", object_id, timestamp);

      let cache_key = encode_collab_key(object_id);
      let mut timestamped_data = Vec::with_capacity(8 + data.len());
      timestamped_data.extend_from_slice(&timestamp.0.to_be_bytes());
      timestamped_data.extend_from_slice(data);
      pipeline.set(&cache_key, timestamped_data).ignore();

      let expiration = expiration_seconds.unwrap_or(SEVEN_DAYS);
      pipeline.expire(&cache_key, expiration as i64).ignore();
    }

    // Execute the batch insert
    match pipeline.query_async::<()>(&mut conn).await {
      Ok(()) => {
        self
          .metrics
          .redis_write_collab_count
          .inc_by(items.len() as u64);
        Ok(())
      },
      Err(err) => {
        error!("Failed to execute batch insert pipeline: {}", err);
        Err(redis::RedisError::from((
          redis::ErrorKind::IoError,
          "Failed to execute batch insert pipeline",
        )))
      },
    }
  }

  /// Inserts data into Redis with a conditional timestamp.
  ///
  /// inserts data associated with an `object_id` into Redis only if the new timestamp is greater than the timestamp
  /// currently stored in Redis for the same `object_id`. It uses Redis transactions to ensure that the operation is atomic.
  /// the data will be expired after 7 days.
  ///
  /// For large data (bigger than small_collab_size), the insertion is spawned as an async task to avoid blocking the caller.
  ///
  /// # Arguments
  /// * `object_id` - A string identifier for the data object.
  /// * `data` - The binary data to be stored.
  /// * `timestamp` - A unix timestamp indicating the creation time of the data.
  ///
  /// # Returns
  /// A Redis result indicating the success or failure of the operation.
  #[instrument(level = "trace", skip_all)]
  async fn insert_data_with_timestamp(
    &self,
    object_id: &Uuid,
    data: &[u8],
    millis_secs: MillisSeconds,
    expiration_seconds: Option<u64>,
  ) -> redis::RedisResult<()> {
    trace!("insert collab: {} updated_at: {}", object_id, millis_secs,);
    // Check if data size is larger than threshold
    if data.len() > self.small_collab_size {
      // For large data, spawn an async task to avoid blocking the caller
      let cache = self.clone();
      let object_id = *object_id;
      let data = data.to_vec();
      tokio::spawn(async move {
        if let Err(err) = cache
          ._insert_data_with_timestamp(&object_id, &data, millis_secs, expiration_seconds)
          .await
        {
          error!("Failed to insert large data into cache: {}", err);
        }
      });
      // Return immediately for large data
      return Ok(());
    }

    self
      ._insert_data_with_timestamp(object_id, data, millis_secs, expiration_seconds)
      .await
  }

  /// Internal implementation of data insertion with timestamp.
  async fn _insert_data_with_timestamp(
    &self,
    object_id: &Uuid,
    data: &[u8],
    seconds: MillisSeconds,
    expiration_seconds: Option<u64>,
  ) -> redis::RedisResult<()> {
    let cache_object_id = encode_collab_key(object_id);
    let mut conn = self.connection_manager.clone();
    let key_exists: bool = conn.exists(&cache_object_id).await?;
    // Start a watch on the object_id to monitor for changes during this transaction
    if key_exists {
      // WATCH command is used to monitor one or more keys for modifications, establishing a condition
      // for executing a subsequent transaction (with MULTI/EXEC). If any of the watched keys are
      // altered by another client before the current client executes EXEC, the transaction will be
      // aborted by Redis (the EXEC will return nil indicating the transaction was not processed).
      let _: redis::Value = redis::cmd("WATCH")
        .arg(&cache_object_id)
        .query_async(&mut conn)
        .await?;
    }

    let result = async {
      // Retrieve the current data, if exists
      let current_value: Option<(u64, Vec<u8>)> = if key_exists {
        let val: Option<Vec<u8>> = conn.get(&cache_object_id).await?;
        val.and_then(|data| Self::extract_timestamp_and_payload(&data).ok())
      } else {
        None
      };

      // Perform update only if the new timestamp is greater than the existing one
      if current_value
        .as_ref()
        .is_none_or(|(ts, _)| seconds.0 >= *ts)
      {
        let mut pipeline = pipe();
        let mut timestamped_data = Vec::with_capacity(8 + data.len());
        timestamped_data.extend_from_slice(&seconds.0.to_be_bytes());
        timestamped_data.extend_from_slice(data);
        pipeline
            .atomic()
            .set(&cache_object_id, timestamped_data)
            .ignore()
            .expire(&cache_object_id, expiration_seconds.unwrap_or(SEVEN_DAYS) as i64) // Setting the expiration to 7 days
            .ignore();
        let () = pipeline.query_async(&mut conn).await?;
      }
      Ok::<(), redis::RedisError>(())
    }
    .await;

    // Always reset Watch State
    let _: redis::Value = redis::cmd("UNWATCH").query_async(&mut conn).await?;
    self.metrics.redis_write_collab_count.inc();
    result
  }

  /// Retrieves data and its associated timestamp from Redis for a given object identifier.
  ///
  /// # Arguments
  /// * `object_id` - A unique identifier for the data.
  ///
  /// # Returns
  /// A `RedisResult<Option<(i64, Vec<u8>)>>` where:
  /// - `i64` is the timestamp of the data.
  /// - `Vec<u8>` is the binary data.
  ///
  /// The function returns `Ok(None)` if no data is found for the given `object_id`.
  pub async fn get_data_with_timestamp(
    &self,
    object_id: &Uuid,
  ) -> redis::RedisResult<Option<(u64, Vec<u8>)>> {
    let cache_object_id = encode_collab_key(object_id);
    let mut conn = self.connection_manager.clone();

    // Attempt to retrieve the data from Redis
    if let Some(data) = conn.get::<_, Option<Vec<u8>>>(&cache_object_id).await? {
      // Use helper function to extract timestamp and payload with metrics
      match self.extract_timestamp_and_payload_with_metrics(&data, Some(object_id)) {
        Some((timestamp, payload)) => Ok(Some((timestamp, payload))),
        None => Err(redis::RedisError::from((
          redis::ErrorKind::TypeError,
          "Failed to extract timestamp and payload",
        ))),
      }
    } else {
      // No data found for the provided object_id
      Ok(None)
    }
  }

  /// Helper function to extract timestamp and payload from raw Redis data.
  ///
  /// # Arguments
  /// * `data` - Raw bytes from Redis containing timestamp (first 8 bytes) + payload
  ///
  /// # Returns
  /// * `Ok((timestamp, payload))` - Successfully extracted timestamp and payload
  /// * `Err(redis::RedisError)` - If data is corrupted or too short
  fn extract_timestamp_and_payload(data: &[u8]) -> redis::RedisResult<(u64, Vec<u8>)> {
    if data.len() < 8 {
      return Err(redis::RedisError::from((
        redis::ErrorKind::TypeError,
        "Data corruption: stored data is too short to contain a valid timestamp.",
      )));
    }

    match data[0..8].try_into() {
      Ok(ts_bytes) => {
        let timestamp = u64::from_be_bytes(ts_bytes);
        let payload = data[8..].to_vec();
        Ok((timestamp, payload))
      },
      Err(_) => Err(redis::RedisError::from((
        redis::ErrorKind::TypeError,
        "Failed to decode timestamp",
      ))),
    }
  }

  /// Helper function to extract timestamp and payload with metrics tracking.
  ///
  /// # Arguments
  /// * `data` - Raw bytes from Redis
  ///
  /// # Returns
  /// * `Some((timestamp, payload))` - Successfully extracted
  /// * `None` - If extraction failed (errors are logged)
  fn extract_timestamp_and_payload_with_metrics(
    &self,
    data: &[u8],
    object_id: Option<&Uuid>,
  ) -> Option<(u64, Vec<u8>)> {
    match Self::extract_timestamp_and_payload(data) {
      Ok((timestamp, payload)) => {
        self.metrics.redis_read_collab_count.inc();
        Some((timestamp, payload))
      },
      Err(err) => {
        if let Some(oid) = object_id {
          error!("Failed to extract timestamp/payload for {}: {}", oid, err);
        } else {
          error!("Failed to extract timestamp/payload: {}", err);
        }
        None
      },
    }
  }

  fn extract_and_decode_collab(
    &self,
    data: &[u8],
    object_id: Option<&Uuid>,
  ) -> Option<(u64, EncodedCollab)> {
    // Extract timestamp and payload with metrics
    let (timestamp, payload) = self.extract_timestamp_and_payload_with_metrics(data, object_id)?;

    // Decode the collaboration data
    match encode_collab_from_bytes(payload) {
      Ok(encoded_collab) => Some((timestamp, encoded_collab)),
      Err(err) => {
        if let Some(oid) = object_id {
          error!("Failed to decode collab data for {}: {}", oid, err);
        } else {
          error!("Failed to decode collab data: {}", err);
        }
        None
      },
    }
  }
}

/// Generates a cache-specific key for an object ID by prepending a fixed prefix.
/// This method ensures that any updates to the object's data involve merely
/// changing the prefix, allowing the old data to expire naturally.
#[inline]
fn encode_collab_key(object_id: &Uuid) -> String {
  let mut key = String::with_capacity(52); // "encode_collab_v0:".len() + 36 (UUID length)
  key.push_str("encode_collab_v0:");
  key.push_str(&object_id.to_string());
  key
}

#[inline]
pub fn cache_exp_secs_from_collab_type(collab_type: &CollabType) -> u64 {
  match collab_type {
    CollabType::Document => SEVEN_DAYS * 2,
    CollabType::Database => SEVEN_DAYS * 2,
    CollabType::WorkspaceDatabase => ONE_MONTH,
    CollabType::Folder => SEVEN_DAYS,
    CollabType::DatabaseRow => SEVEN_DAYS,
    CollabType::UserAwareness => SEVEN_DAYS * 2,
    CollabType::Unknown => SEVEN_DAYS,
  }
}
