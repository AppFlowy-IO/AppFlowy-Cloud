use super::disk_cache::CollabDiskCache;
use super::mem_cache::{cache_exp_secs_from_collab_type, CollabMemCache, MillisSeconds};
use crate::collab::cache::DECODE_SPAWN_THRESHOLD;
use crate::config::get_env_var;
use crate::CollabMetrics;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_proto::{Rid, TimestampedEncodedCollab, UpdateFlags};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use collab::core::collab::{default_client_id, CollabOptions, DataSource};
use collab::core::origin::CollabOrigin;
use collab::entity::{EncodedCollab, EncoderVersion};
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_stream::model::UpdateStreamMessage;
use dashmap::DashMap;
use database::file::s3_client_impl::AwsS3BucketClientImpl;
use database_entity::dto::{
  CollabParams, CollabUpdateData, PendingCollabWrite, QueryCollab, QueryCollabResult,
};
use futures_util::{stream, StreamExt};
use infra::thread_pool::ThreadPoolNoAbort;
use itertools::{Either, Itertools};
use rayon::prelude::*;
use sqlx::{PgPool, Transaction};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, instrument, trace, warn};
use uuid::Uuid;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{ReadTxn, StateVector, Update};

pub struct CollabCache {
  thread_pool: Arc<ThreadPoolNoAbort>,
  disk_cache: CollabDiskCache,
  mem_cache: CollabMemCache,
  s3_collab_threshold: usize,
  /// Threshold for spawning background tasks for memory cache operations.
  /// Data smaller than this will be processed on the current thread.
  /// Data larger than this will be spawned to avoid blocking.
  small_collab_size: usize,
  metrics: Arc<CollabMetrics>,
  dirty_collabs: DashMap<Uuid, u64>,
}

impl CollabCache {
  pub fn new(
    thread_pool: Arc<ThreadPoolNoAbort>,
    redis_conn_manager: redis::aio::ConnectionManager,
    pg_pool: PgPool,
    s3: AwsS3BucketClientImpl,
    metrics: Arc<CollabMetrics>,
    s3_collab_threshold: usize,
  ) -> Arc<Self> {
    let mem_cache = CollabMemCache::new(
      thread_pool.clone(),
      redis_conn_manager.clone(),
      metrics.clone(),
    );
    let disk_cache = CollabDiskCache::new(
      thread_pool.clone(),
      pg_pool.clone(),
      s3,
      s3_collab_threshold,
      metrics.clone(),
    );

    let small_collab_size = get_env_var("APPFLOWY_SMALL_COLLAB_SIZE", "4096")
      .parse::<usize>()
      .unwrap_or(DECODE_SPAWN_THRESHOLD);
    Arc::new(Self {
      thread_pool,
      disk_cache,
      mem_cache,
      s3_collab_threshold,
      small_collab_size,
      metrics,
      dirty_collabs: DashMap::new(),
    })
  }

  #[instrument(level = "trace", skip_all)]
  pub fn mark_as_dirty(&self, object_id: Uuid, millis_secs: MillisSeconds) {
    let millis_secs = millis_secs.into_inner();
    match self.dirty_collabs.entry(object_id) {
      dashmap::mapref::entry::Entry::Occupied(mut entry) => {
        // Only update if the new timestamp is newer
        if millis_secs > *entry.get() {
          tracing::trace!(
            "marking collab {} as dirty at timestamp {}",
            object_id,
            millis_secs
          );
          entry.insert(millis_secs);
        }
      },
      dashmap::mapref::entry::Entry::Vacant(entry) => {
        tracing::trace!(
          "marking collab {} as dirty at timestamp {}",
          object_id,
          millis_secs
        );
        entry.insert(millis_secs);
      },
    }
  }

  pub fn is_dirty_since(&self, object_id: &Uuid, millis_seconds: MillisSeconds) -> bool {
    let millis_secs = millis_seconds.into_inner();
    if let Some(value) = self.dirty_collabs.get(object_id) {
      let is_dirty = *value > millis_secs;
      trace!(
        "collab {} is dirty:{}, dirty ts:{}, get ts:{}, ",
        object_id,
        is_dirty,
        *value,
        millis_secs,
      );
      is_dirty
    } else {
      // Mark the collab as dirty if it is not found in the cache. Any upcoming read with millis_secs
      // less than the current timestamp will be considered dirty. When snapshot schedule write a new snapshot.
      // The snapshot rid will bigger then stored time. then it will return false.
      self.mark_as_dirty(*object_id, millis_secs.into());
      true
    }
  }

  pub fn metrics(&self) -> &CollabMetrics {
    &self.metrics
  }

  /// **Note: This function will override any existing values without timestamp comparison.**
  /// Use the single insert methods if you need conditional insertion based on timestamps.
  #[instrument(level = "trace", skip_all)]
  pub async fn bulk_insert_collab(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params_list: Vec<CollabParams>,
  ) -> Result<(), AppError> {
    self
      .disk_cache
      .bulk_insert_collab(workspace_id, uid, params_list.clone())
      .await?;

    // Group params by collab type for different batch sizes
    let mut database_rows = Vec::new();
    let mut other_types = Vec::new();
    for params in params_list {
      let mills_secs = params
        .updated_at
        .as_ref()
        .map(|v| MillisSeconds::from(v.timestamp_millis() as u64))
        .unwrap_or_else(|| {
          warn!(
            "CollabParams updated_at should not be None for object_id: {}",
            params.object_id
          );
          MillisSeconds::now()
        });

      let batch_item = (
        params.object_id,
        params.encoded_collab_v1,
        mills_secs,
        Some(cache_exp_secs_from_collab_type(&params.collab_type)),
      );

      match params.collab_type {
        CollabType::DatabaseRow => database_rows.push(batch_item),
        _ => other_types.push(batch_item),
      }
    }

    let batch_row_size = get_env_var("APPFLOWY_REDIS_BATCH_DB_ROW_COLLAB_SIZE", "50")
      .parse::<usize>()
      .unwrap_or(50);

    // Process database rows in batches of 50
    for batch in database_rows.chunks(batch_row_size) {
      if let Err(err) = self
        .mem_cache
        .batch_insert_raw_data(batch)
        .await
        .map_err(|err| AppError::Internal(err.into()))
      {
        tracing::warn!(
          "Failed to batch insert {} database row collabs into memory cache: {}",
          batch.len(),
          err
        );
      }
    }

    let size = get_env_var("APPFLOWY_REDIS_BATCH_COLLAB_SIZE", "10")
      .parse::<usize>()
      .unwrap_or(10);
    for batch in other_types.chunks(size) {
      if let Err(err) = self
        .mem_cache
        .batch_insert_raw_data(batch)
        .await
        .map_err(|err| AppError::Internal(err.into()))
      {
        tracing::warn!(
          "Failed to batch insert {} collabs into memory cache: {}",
          batch.len(),
          err
        );
      }
    }

    Ok(())
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn get_snapshot_collab(
    &self,
    workspace_id: &Uuid,
    query: QueryCollab,
  ) -> Result<(Rid, EncodedCollab), AppError> {
    // Attempt to retrieve encoded collab from memory cache, falling back to disk cache if necessary.
    if let Some((rid, encoded_collab)) = self.mem_cache.get_encode_collab(&query.object_id).await {
      debug!(
        "Did get encode collab: {} from cache at {}",
        query.object_id, rid,
      );
      return Ok((rid, encoded_collab));
    }

    // Retrieve from disk cache as fallback. After retrieval, the value is inserted into the memory cache.
    let object_id = query.object_id;
    let expiration_secs = cache_exp_secs_from_collab_type(&query.collab_type);
    let (rid, encode_collab) = self
      .disk_cache
      .get_encoded_collab_from_disk(workspace_id, query)
      .await?;

    self
      .mem_cache
      .insert_encode_collab(
        &object_id,
        encode_collab.clone(),
        MillisSeconds::from(&rid),
        expiration_secs,
      )
      .await;
    Ok((rid, encode_collab))
  }

  #[instrument(level = "debug", skip_all)]
  pub async fn get_full_collab(
    &self,
    workspace_id: &Uuid,
    query: QueryCollab,
    from: Option<StateVector>,
    encoding: EncoderVersion,
  ) -> Result<TimestampedEncodedCollab, AppError> {
    let object_id = query.object_id;
    let collab_type = query.collab_type;
    let (rid, mut encoded_collab) = match self.get_snapshot_collab(workspace_id, query).await {
      Ok((rid, encoded_collab)) => {
        debug!(
          "Snapshot Collab:{} at {} found in cache",
          object_id, rid.timestamp
        );
        (rid, Some(encoded_collab))
      },
      Err(AppError::RecordNotFound(_)) => {
        debug!(
          "Snapshot Collab:{} not found in cache, returning default state",
          object_id
        );
        (Rid::default(), None)
      },
      Err(err) => return Err(err),
    };

    let from = from.unwrap_or_default();
    if !self.is_dirty_since(&object_id, MillisSeconds::from(&rid)) {
      // there are no pending updates for this collab, so we can return the cached value directly
      trace!("no pending updates for collab: {}", object_id);
      return match encoded_collab {
        Some(encoded_collab) if encoded_collab.doc_state.len() <= self.small_collab_size => {
          Ok(TimestampedEncodedCollab {
            encoded_collab,
            rid,
          })
        },
        Some(encoded_collab) => {
          // If the collab is large, we do not replay updates and return the snapshot only.
          let diff_encoded = self.encode_diff_for_large_collab(object_id, encoded_collab, &from)?;
          Ok(TimestampedEncodedCollab {
            encoded_collab: diff_encoded,
            rid,
          })
        },
        None => Err(AppError::RecordNotFound(format!(
          "Collab not found for object_id: {}",
          object_id
        ))),
      };
    }

    let updates = self
      .get_workspace_updates(workspace_id, Some(&object_id), Some(rid), None)
      .await?;

    let size = encoded_collab
      .as_ref()
      .map(|v| v.doc_state.len())
      .unwrap_or(0)
      + updates.iter().map(|u| u.update.len()).sum::<usize>();

    trace!(
      "found {} pending updates for collab: {}/{}",
      updates.len(),
      object_id,
      collab_type
    );
    if !updates.is_empty() {
      encoded_collab = if size <= self.small_collab_size {
        // For small collab, replaying updates on the current thread
        replaying_updates(
          encoding,
          object_id,
          collab_type,
          encoded_collab,
          updates,
          &from,
        )?
      } else {
        self
          .thread_pool
          .install(|| {
            replaying_updates(
              encoding,
              object_id,
              collab_type,
              encoded_collab,
              updates,
              &from,
            )
          })
          .map_err(|err| AppError::Internal(err.into()))??
      }
    }

    match encoded_collab {
      Some(encoded_collab) => Ok(TimestampedEncodedCollab {
        encoded_collab,
        rid,
      }),
      None => Err(AppError::RecordNotFound(format!(
        "Collab not found for object_id: {}",
        object_id
      ))),
    }
  }

  pub async fn get_collabs_created_since(
    &self,
    workspace_id: Uuid,
    since: DateTime<Utc>,
    limit: usize,
  ) -> Result<Vec<CollabUpdateData>, AppError> {
    self
      .disk_cache
      .get_collabs_created_since(workspace_id, since, limit)
      .await
  }

  pub async fn get_workspace_updates(
    &self,
    workspace_id: &Uuid,
    object_id: Option<&Uuid>,
    from: Option<Rid>,
    to: Option<Rid>,
  ) -> Result<Vec<UpdateStreamMessage>, AppError> {
    self
      .mem_cache
      .get_workspace_updates(workspace_id, object_id, from, to)
      .await
  }

  /// Batch get the encoded collab **SNAPSHOT** data from the cache.
  /// This function only returns cached/stored data and does NOT apply pending updates.
  /// Use batch_get_full_collab() if you need current state with updates applied.
  /// Returns a hashmap of the object_id to the encoded collab data.
  #[instrument(level = "debug", skip_all)]
  pub async fn batch_get_snapshot_collab<T: Into<QueryCollab>>(
    &self,
    workspace_id: &Uuid,
    queries: Vec<T>,
  ) -> HashMap<Uuid, QueryCollabResult> {
    let queries = queries.into_iter().map(Into::into).collect::<Vec<_>>();
    let mut results = HashMap::new();

    // Group queries by collab_type for optimal batch sizing
    let mut db_row_queries = Vec::new();
    let mut other_queries = Vec::new();

    for query in queries {
      if matches!(query.collab_type, CollabType::DatabaseRow) {
        db_row_queries.push(query);
      } else {
        other_queries.push(query);
      }
    }

    if !db_row_queries.is_empty() {
      let size = get_env_var("APPFLOWY_REDIS_BATCH_DB_ROW_COLLAB_SIZE", "50")
        .parse::<usize>()
        .unwrap_or(50);
      for chunk in db_row_queries.chunks(size) {
        let batch_results = self.process_query_batch(workspace_id, chunk.to_vec()).await;
        results.extend(batch_results);
      }
    }

    // Process other queries in batches of 20
    if !other_queries.is_empty() {
      let size = get_env_var("APPFLOWY_REDIS_BATCH_COLLAB_SIZE", "10")
        .parse::<usize>()
        .unwrap_or(10);
      for chunk in other_queries.chunks(size) {
        let batch_results = self.process_query_batch(workspace_id, chunk.to_vec()).await;
        results.extend(batch_results);
      }
    }

    results
  }

  async fn process_query_batch(
    &self,
    workspace_id: &Uuid,
    queries: Vec<QueryCollab>,
  ) -> HashMap<Uuid, QueryCollabResult> {
    let mut results = HashMap::new();
    let object_ids: Vec<Uuid> = queries.iter().map(|q| q.object_id).collect();
    let (disk_queries, values_from_mem_cache) =
      match self.mem_cache.batch_get_data(&object_ids).await {
        Ok(mem_cache_results) => {
          let mem_cache_map: HashMap<Uuid, QueryCollabResult> = mem_cache_results
            .into_iter()
            .map(|(object_id, data)| {
              (
                object_id,
                QueryCollabResult::Success {
                  encode_collab_v1: data,
                },
              )
            })
            .collect();

          let found_in_cache: std::collections::HashSet<Uuid> =
            mem_cache_map.keys().copied().collect();
          let disk_queries: Vec<QueryCollab> = queries
            .into_iter()
            .filter(|q| !found_in_cache.contains(&q.object_id))
            .collect();

          (disk_queries, mem_cache_map)
        },
        Err(err) => {
          error!(
            "Batch get from memory cache failed: {}, falling back to individual calls",
            err
          );
          let (disk_queries, values_from_mem_cache): (Vec<_>, HashMap<_, _>) =
            stream::iter(queries)
              .then(|params| async move {
                match self
                  .mem_cache
                  .get_data_with_timestamp(&params.object_id)
                  .await
                {
                  Ok(Some((_ts, data))) => Either::Right((
                    params.object_id,
                    QueryCollabResult::Success {
                      encode_collab_v1: data,
                    },
                  )),
                  _ => Either::Left(params),
                }
              })
              .collect::<Vec<_>>()
              .await
              .into_iter()
              .partition_map(|either| either);
          (disk_queries, values_from_mem_cache)
        },
      };

    results.extend(values_from_mem_cache);

    // 2. Retrieves remaining values from the disk cache for queries not satisfied by the memory cache.
    let values_from_disk_cache = self
      .disk_cache
      .batch_get_collab(workspace_id, disk_queries)
      .await;
    results.extend(values_from_disk_cache);
    results
  }

  /// Batch get the FULL/CURRENT collab data (applying pending updates for dirty collabs).
  /// This function is consistent with get_full_collab - it applies pending updates.
  /// Returns a hashmap of the object_id to the encoded collab data.
  #[instrument(level = "debug", skip_all)]
  pub async fn batch_get_full_collab<T: Into<QueryCollab>>(
    &self,
    workspace_id: &Uuid,
    queries: Vec<T>,
    from: Option<StateVector>,
    encoding: EncoderVersion,
  ) -> HashMap<Uuid, QueryCollabResult> {
    let queries = queries.into_iter().map(Into::into).collect::<Vec<_>>();
    let mut results = HashMap::new();

    // For batch efficiency, separate dirty and clean collabs
    let mut clean_queries = Vec::new();
    let mut dirty_queries = Vec::new();

    for query in queries {
      if self.is_dirty_since(&query.object_id, MillisSeconds::now()) {
        dirty_queries.push(query);
      } else {
        clean_queries.push(query);
      }
    }

    debug!(
      "Batch processing collabs: {} clean, {} dirty",
      clean_queries.len(),
      dirty_queries.len()
    );

    if !clean_queries.is_empty() {
      let clean_results = self
        .batch_get_snapshot_collab(workspace_id, clean_queries)
        .await;
      results.extend(clean_results);
    }

    let mut encoded_collab_by_object_id: HashMap<Uuid, EncodedCollab> =
      HashMap::with_capacity(dirty_queries.len());
    for query in dirty_queries {
      let object_id = query.object_id;
      match self
        .get_full_collab(workspace_id, query, from.clone(), encoding.clone())
        .await
      {
        Ok(value) => {
          encoded_collab_by_object_id.insert(object_id, value.encoded_collab);
        },
        Err(err) => {
          results.insert(
            object_id,
            QueryCollabResult::Failed {
              error: err.to_string(),
            },
          );
        },
      }
    }

    if let Ok(entries) = self.thread_pool.install(|| {
      encoded_collab_by_object_id
        .into_par_iter()
        .map(|(object_id, encoded_collab)| {
          (
            object_id,
            QueryCollabResult::Success {
              encode_collab_v1: encoded_collab.encode_to_bytes().unwrap(),
            },
          )
        })
        .collect::<HashMap<_, _>>()
    }) {
      results.extend(entries);
    }
    results
  }

  /// Insert the encoded collab data into the cache.
  /// The data is inserted into both the memory and disk cache.
  #[instrument(level = "trace", skip_all)]
  pub async fn insert_encode_collab_data(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
  ) -> Result<(), AppError> {
    let collab_type = params.collab_type;
    let object_id = params.object_id;
    let encode_collab_data = params.encoded_collab_v1.clone();
    let s3 = self.disk_cache.s3_client();
    CollabDiskCache::upsert_collab_with_transaction(
      workspace_id,
      uid,
      params,
      transaction,
      s3,
      self.s3_collab_threshold,
      &self.metrics,
    )
    .await?;

    self
      .cache_collab(
        object_id,
        collab_type,
        encode_collab_data,
        MillisSeconds::now(),
      )
      .await;
    Ok(())
  }

  #[instrument(level = "trace", skip_all)]
  async fn cache_collab(
    &self,
    object_id: Uuid,
    collab_type: CollabType,
    encode_collab_data: Bytes,
    mills_secs: MillisSeconds,
  ) {
    let mem_cache = self.mem_cache.clone();
    let expiration_secs = cache_exp_secs_from_collab_type(&collab_type);
    if let Err(err) = mem_cache
      .insert_encode_collab_data(
        &object_id,
        &encode_collab_data,
        mills_secs,
        Some(expiration_secs),
      )
      .await
    {
      error!("Failed to insert encode collab into memory cache: {}", err);
    }
  }

  #[instrument(level = "trace", skip_all)]
  pub async fn insert_encode_collab_to_disk(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    params: CollabParams,
  ) -> Result<(), AppError> {
    let mills_secs = params
      .updated_at
      .as_ref()
      .map(|v| v.timestamp_millis() as u64)
      .ok_or_else(|| AppError::Internal(anyhow!("Update_at should not be None")))?;

    let p = params.clone();
    self
      .disk_cache
      .upsert_collab(workspace_id, uid, params)
      .await?;

    self
      .cache_collab(
        p.object_id,
        p.collab_type,
        p.encoded_collab_v1,
        MillisSeconds::from(mills_secs),
      )
      .await;
    Ok(())
  }

  pub async fn delete_collab(&self, workspace_id: &Uuid, object_id: &Uuid) -> Result<(), AppError> {
    self.mem_cache.remove_encode_collab(object_id).await?;
    self
      .disk_cache
      .delete_collab(workspace_id, object_id)
      .await?;
    Ok(())
  }

  pub async fn is_exist(&self, workspace_id: &Uuid, oid: &Uuid) -> Result<bool, AppError> {
    if let Ok(value) = self.mem_cache.is_exist(oid).await {
      if value {
        return Ok(value);
      }
    }

    let is_exist = self.disk_cache.is_exist(workspace_id, oid).await?;
    Ok(is_exist)
  }

  #[instrument(level = "debug", skip_all)]
  pub async fn batch_insert_collab(
    &self,
    records: Vec<PendingCollabWrite>,
  ) -> Result<(), AppError> {
    let mem_cache_params: Vec<_> = records
      .iter()
      .map(|r| {
        (
          r.params.object_id,
          r.params.encoded_collab_v1.clone(),
          cache_exp_secs_from_collab_type(&r.params.collab_type),
        )
      })
      .collect();

    self.disk_cache.batch_insert_collab(records).await?;
    for (oid, data, expire) in mem_cache_params {
      if let Err(err) = self
        .mem_cache
        .insert_encode_collab_data(&oid, &data, MillisSeconds::now(), Some(expire))
        .await
      {
        error!(
          "Failed to insert collab `{}` into memory cache: {}",
          oid, err
        );
      }
    }

    Ok(())
  }

  /// Encode diff for a large collab without full replay (optimization for clean large collabs)
  fn encode_diff_for_large_collab(
    &self,
    object_id: Uuid,
    encoded_collab: EncodedCollab,
    from: &StateVector,
  ) -> Result<EncodedCollab, AppError> {
    let options = CollabOptions::new(object_id.to_string(), default_client_id()).with_data_source(
      match encoded_collab.version {
        EncoderVersion::V1 => DataSource::DocStateV1(encoded_collab.doc_state.into()),
        EncoderVersion::V2 => DataSource::DocStateV2(encoded_collab.doc_state.into()),
      },
    );

    let collab = Collab::new_with_options(CollabOrigin::Server, options)
      .map_err(|err| AppError::Internal(err.into()))?;

    let tx = collab.transact();
    let doc_state: Bytes = match encoded_collab.version {
      EncoderVersion::V1 => tx.encode_diff_v1(from),
      EncoderVersion::V2 => tx.encode_diff_v2(from),
    }
    .into();

    Ok(EncodedCollab {
      version: encoded_collab.version,
      state_vector: encoded_collab.state_vector,
      doc_state,
    })
  }
}

#[inline]
fn replaying_updates(
  encoding: EncoderVersion,
  object_id: Uuid,
  collab_type: CollabType,
  mut encoded_collab: Option<EncodedCollab>,
  updates: Vec<UpdateStreamMessage>,
  from: &StateVector,
) -> Result<Option<EncodedCollab>, AppError> {
  tracing::trace!(
    "replaying {} updates for {}/{}, from: {:#?}",
    updates.len(),
    object_id,
    collab_type,
    from,
  );
  let mut collab = match encoded_collab {
    Some(encoded_collab) => {
      let options = CollabOptions::new(object_id.to_string(), default_client_id())
        .with_data_source(match encoded_collab.version {
          EncoderVersion::V1 => DataSource::DocStateV1(encoded_collab.doc_state.into()),
          EncoderVersion::V2 => DataSource::DocStateV2(encoded_collab.doc_state.into()),
        });
      Collab::new_with_options(CollabOrigin::Server, options)
        .map_err(|err| AppError::Internal(err.into()))?
    },
    None => {
      let options = CollabOptions::new(object_id.to_string(), default_client_id());
      Collab::new_with_options(CollabOrigin::Server, options)
        .map_err(|err| AppError::Internal(err.into()))?
    },
  };
  {
    let mut tx = collab.transact_mut();
    for msg in updates {
      if msg.object_id == object_id {
        let update = match msg.update_flags {
          UpdateFlags::Lib0v1 => Update::decode_v1(&msg.update),
          UpdateFlags::Lib0v2 => Update::decode_v2(&msg.update),
        }
        .map_err(|err| AppError::DecodeUpdateError(err.to_string()))?;

        tracing::trace!(
          "replaying {} update for {}/{}: {:#?}",
          msg.last_message_id,
          object_id,
          collab_type,
          update,
        );
        tx.apply_update(update)
          .map_err(|err| AppError::ApplyUpdateError(err.to_string()))?;
      }
    }
  }
  let tx = collab.transact();
  encoded_collab = Some(match encoding {
    EncoderVersion::V1 => {
      EncodedCollab::new_v1(tx.state_vector().encode_v1(), tx.encode_diff_v1(from))
    },
    EncoderVersion::V2 => {
      EncodedCollab::new_v2(tx.state_vector().encode_v2(), tx.encode_diff_v2(from))
    },
  });

  Ok(encoded_collab)
}
