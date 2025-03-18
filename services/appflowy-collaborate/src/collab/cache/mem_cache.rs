use crate::collab::cache::encode_collab_from_bytes;
use crate::CollabMetrics;
use anyhow::anyhow;
use app_error::AppError;
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use database::collab::CollabMetadata;
use redis::{pipe, AsyncCommands};
use std::sync::Arc;
use tracing::{error, instrument, trace};
use uuid::Uuid;

const SEVEN_DAYS: u64 = 604800;
const ONE_MONTH: u64 = 2592000;
#[derive(Clone)]
pub struct CollabMemCache {
  connection_manager: redis::aio::ConnectionManager,
  metrics: Arc<CollabMetrics>,
}

impl CollabMemCache {
  pub fn new(
    connection_manager: redis::aio::ConnectionManager,
    metrics: Arc<CollabMetrics>,
  ) -> Self {
    Self {
      connection_manager,
      metrics,
    }
  }

  pub async fn insert_collab_meta(&self, meta: CollabMetadata) -> Result<(), AppError> {
    let key = collab_meta_key(&meta.object_id);
    let value = serde_json::to_string(&meta)?;
    let () = self
      .connection_manager
      .clone()
      .set_ex(key, value, ONE_MONTH)
      .await
      .map_err(|err| {
        AppError::Internal(anyhow!("Failed to save collab meta to redis: {:?}", err))
      })?;
    Ok(())
  }

  pub async fn get_collab_meta(&self, object_id: &Uuid) -> Result<CollabMetadata, AppError> {
    let key = collab_meta_key(object_id);
    let value: Option<String> = self
      .connection_manager
      .clone()
      .get(key)
      .await
      .map_err(|err| {
        AppError::Internal(anyhow!("Failed to get collab meta from redis: {:?}", err))
      })?;
    match value {
      Some(value) => {
        let meta: CollabMetadata = serde_json::from_str(&value)?;
        Ok(meta)
      },
      None => Err(AppError::RecordNotFound(format!(
        "Collab meta not found for object_id: {}",
        object_id
      ))),
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
    let cache_object_id = encode_collab_key(object_id);
    self
      .connection_manager
      .clone()
      .del::<&str, ()>(&cache_object_id)
      .await
      .map_err(|err| {
        AppError::Internal(anyhow!(
          "Failed to remove encoded collab from redis: {:?}",
          err
        ))
      })
  }

  pub async fn get_encode_collab_data(&self, object_id: &Uuid) -> Option<Vec<u8>> {
    match self.get_data_with_timestamp(object_id).await {
      Ok(None) => None,
      Ok(Some((_, bytes))) => Some(bytes),
      Err(err) => {
        error!("Failed to get encoded collab from redis: {:?}", err);
        None
      },
    }
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn get_encode_collab(&self, object_id: &Uuid) -> Option<EncodedCollab> {
    match self.get_encode_collab_data(object_id).await {
      Some(bytes) => encode_collab_from_bytes(bytes).await.ok(),
      None => {
        trace!(
          "No encoded collab found in cache for object_id: {}",
          object_id
        );
        None
      },
    }
  }

  #[instrument(level = "trace", skip_all, fields(object_id=%object_id))]
  pub async fn insert_encode_collab(
    &self,
    object_id: &Uuid,
    encoded_collab: EncodedCollab,
    timestamp: i64,
    expiration_seconds: u64,
  ) {
    trace!("Inserting encode collab into cache: {}", object_id);
    let result = tokio::task::spawn_blocking(move || encoded_collab.encode_to_bytes()).await;
    match result {
      Ok(Ok(bytes)) => {
        if let Err(err) = self
          .insert_data_with_timestamp(object_id, &bytes, timestamp, Some(expiration_seconds))
          .await
        {
          error!("Failed to cache encoded collab: {:?}", err);
        }
      },
      Ok(Err(err)) => {
        error!("Failed to encode collab to bytes: {:?}", err);
      },
      Err(e) => {
        error!("Failed to encode collab to bytes: {:?}", e);
      },
    }
  }

  /// Inserts data into Redis with a conditional timestamp.
  /// if the expiration_seconds is None, the data will be expired after 7 days.
  pub async fn insert_encode_collab_data(
    &self,
    object_id: &Uuid,
    data: &[u8],
    timestamp: i64,
    expiration_seconds: Option<u64>,
  ) -> redis::RedisResult<()> {
    tracing::trace!("insert collab {} to memory cache", object_id);
    self
      .insert_data_with_timestamp(object_id, data, timestamp, expiration_seconds)
      .await
  }

  /// Inserts data into Redis with a conditional timestamp.
  ///
  /// inserts data associated with an `object_id` into Redis only if the new timestamp is greater than the timestamp
  /// currently stored in Redis for the same `object_id`. It uses Redis transactions to ensure that the operation is atomic.
  /// the data will be expired after 7 days.
  ///
  /// # Arguments
  /// * `object_id` - A string identifier for the data object.
  /// * `data` - The binary data to be stored.
  /// * `timestamp` - A unix timestamp indicating the creation time of the data.
  ///
  /// # Returns
  /// A Redis result indicating the success or failure of the operation.
  async fn insert_data_with_timestamp(
    &self,
    object_id: &Uuid,
    data: &[u8],
    timestamp: i64,
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
      let current_value: Option<(i64, Vec<u8>)> = if key_exists {
        let val: Option<Vec<u8>> = conn.get(&cache_object_id).await?;
        val.and_then(|data| {
          if data.len() < 8 {
            None
          } else {
            match data[0..8].try_into() {
              Ok(ts_bytes) => {
                let ts = i64::from_be_bytes(ts_bytes);
                Some((ts, data[8..].to_vec()))
              },
              Err(_) => None,
            }
          }
        })
      } else {
        None
      };

      // Perform update only if the new timestamp is greater than the existing one
      if current_value
        .as_ref()
        .map_or(true, |(ts, _)| timestamp >= *ts)
      {
        let mut pipeline = pipe();
        let data = [timestamp.to_be_bytes().as_ref(), data].concat();
        pipeline
            .atomic()
            .set(&cache_object_id, data)
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
  async fn get_data_with_timestamp(
    &self,
    object_id: &Uuid,
  ) -> redis::RedisResult<Option<(i64, Vec<u8>)>> {
    let cache_object_id = encode_collab_key(object_id);
    let mut conn = self.connection_manager.clone();
    // Attempt to retrieve the data from Redis
    if let Some(data) = conn.get::<_, Option<Vec<u8>>>(&cache_object_id).await? {
      if data.len() < 8 {
        // Data is too short to contain a valid timestamp and payload
        Err(redis::RedisError::from((
          redis::ErrorKind::TypeError,
          "Data corruption: stored data is too short to contain a valid timestamp.",
        )))
      } else {
        // Extract timestamp and payload from the retrieved data
        match data[0..8].try_into() {
          Ok(ts_bytes) => {
            self.metrics.redis_read_collab_count.inc();
            let timestamp = i64::from_be_bytes(ts_bytes);
            let payload = data[8..].to_vec();
            Ok(Some((timestamp, payload)))
          },
          Err(_) => Err(redis::RedisError::from((
            redis::ErrorKind::TypeError,
            "Failed to decode timestamp",
          ))),
        }
      }
    } else {
      // No data found for the provided object_id
      Ok(None)
    }
  }
}

/// Generates a cache-specific key for an object ID by prepending a fixed prefix.
/// This method ensures that any updates to the object's data involve merely
/// changing the prefix, allowing the old data to expire naturally.
///
#[inline]
fn encode_collab_key(object_id: &Uuid) -> String {
  format!("encode_collab_v0:{}", object_id)
}

#[inline]
fn collab_meta_key(object_id: &Uuid) -> String {
  format!("collab_meta_v0:{}", object_id)
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
