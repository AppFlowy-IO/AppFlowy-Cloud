use crate::state::RedisConnectionManager;
use collab::core::collab_plugin::EncodedCollab;
use redis::{pipe, AsyncCommands};
use std::ops::DerefMut;

use anyhow::anyhow;
use app_error::AppError;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, instrument, trace};

#[derive(Clone)]
pub struct CollabMemCache {
  connection_manager: Arc<Mutex<RedisConnectionManager>>,
}

impl CollabMemCache {
  pub fn new(redis_client: RedisConnectionManager) -> Self {
    Self {
      connection_manager: Arc::new(Mutex::new(redis_client)),
    }
  }

  pub async fn remove_encode_collab(&self, object_id: &str) -> Result<(), AppError> {
    self
      .connection_manager
      .lock()
      .await
      .del::<&str, ()>(object_id)
      .await
      .map_err(|err| {
        AppError::Internal(anyhow!(
          "Failed to remove encoded collab from redis: {:?}",
          err
        ))
      })
  }

  pub async fn get_encode_collab_data(&self, object_id: &str) -> Option<Vec<u8>> {
    match self.get_data_with_timestamp(object_id).await {
      Ok(data) => data.map(|(_, bytes)| bytes),
      Err(err) => {
        error!("Failed to get encoded collab from redis: {:?}", err);
        None
      },
    }
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn get_encode_collab(&self, object_id: &str) -> Option<EncodedCollab> {
    match self.get_encode_collab_data(object_id).await {
      Some(bytes) => match EncodedCollab::decode_from_bytes(&bytes) {
        Ok(encoded_collab) => Some(encoded_collab),
        Err(err) => {
          error!("Failed to decode collab from redis cache bytes: {:?}", err);
          None
        },
      },
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
    object_id: String,
    encoded_collab: EncodedCollab,
    timestamp: i64,
  ) {
    trace!("Inserting encode collab into cache: {}", object_id);
    let result = tokio::task::spawn_blocking(move || encoded_collab.encode_to_bytes()).await;
    match result {
      Ok(Ok(bytes)) => {
        if let Err(err) = self
          .insert_data_with_timestamp(object_id, bytes, timestamp)
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

  pub async fn insert_encode_collab_data(&self, object_id: String, data: Vec<u8>, timestamp: i64) {
    if let Err(err) = self
      .insert_data_with_timestamp(object_id, data, timestamp)
      .await
    {
      error!("Failed to cache encoded collab bytes: {:?}", err);
    }
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
    object_id: String,
    data: Vec<u8>,
    timestamp: i64,
  ) -> redis::RedisResult<()> {
    let mut conn = self.connection_manager.lock().await;
    let key_exists: bool = conn.exists(&object_id).await?;
    // Start a watch on the object_id to monitor for changes during this transaction
    if key_exists {
      // WATCH command is used to monitor one or more keys for modifications, establishing a condition
      // for executing a subsequent transaction (with MULTI/EXEC). If any of the watched keys are
      // altered by another client before the current client executes EXEC, the transaction will be
      // aborted by Redis (the EXEC will return nil indicating the transaction was not processed).
      redis::cmd("WATCH")
        .arg(&object_id)
        .query_async::<_, ()>(&mut *conn)
        .await?;
    }

    let result = async {
      // Retrieve the current data, if exists
      let current_value: Option<(i64, Vec<u8>)> = if key_exists {
        let val: Option<Vec<u8>> = conn.get(&object_id).await?;
        val.map(|data| {
          let ts = i64::from_be_bytes(data[0..8].try_into().unwrap());
          (ts, data[8..].to_vec())
        })
      } else {
        None
      };

      // Perform update only if the new timestamp is greater than the existing one
      if current_value
        .as_ref()
        .map_or(true, |(ts, _)| timestamp > *ts)
      {
        let mut pipeline = pipe();
        let data = [timestamp.to_be_bytes().as_ref(), data.as_slice()].concat();
        pipeline
            .atomic()
            .set(&object_id, data)
            .ignore()
            .expire(&object_id, 604800) // Setting the expiration to 7 days
            .ignore();
        pipeline.query_async(conn.deref_mut()).await?;
      }
      Ok::<(), redis::RedisError>(())
    }
    .await;

    // Always reset Watch State
    redis::cmd("UNWATCH")
      .query_async::<_, ()>(&mut *conn)
      .await?;

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
  /// The function returns `Ok(None)` if no data is found for the given `object_id`.
  async fn get_data_with_timestamp(
    &self,
    object_id: &str,
  ) -> redis::RedisResult<Option<(i64, Vec<u8>)>> {
    let mut conn = self.connection_manager.lock().await;
    // Attempt to retrieve the data from Redis
    if let Some(data) = conn.get::<_, Option<Vec<u8>>>(object_id).await? {
      if data.len() < 8 {
        // Data is too short to contain a valid timestamp and payload
        Err(redis::RedisError::from((
          redis::ErrorKind::TypeError,
          "Data corruption: stored data is too short to contain a valid timestamp.",
        )))
      } else {
        // Extract timestamp and payload from the retrieved data
        let timestamp =
          i64::from_be_bytes(data[0..8].try_into().expect("Failed to decode timestamp"));
        let payload = data[8..].to_vec();
        Ok(Some((timestamp, payload)))
      }
    } else {
      // No data found for the provided object_id
      Ok(None)
    }
  }
}
