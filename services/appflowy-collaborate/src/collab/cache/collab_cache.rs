use super::disk_cache::CollabDiskCache;
use super::mem_cache::{cache_exp_secs_from_collab_type, CollabMemCache};
use crate::collab::cache::DECODE_SPAWN_THRESHOLD;
use crate::config::get_env_var;
use crate::CollabMetrics;
use app_error::AppError;
use appflowy_proto::{Rid, UpdateFlags};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use collab::core::collab::{default_client_id, CollabOptions, DataSource};
use collab::core::origin::CollabOrigin;
use collab::entity::{EncodedCollab, EncoderVersion};
use collab::preclude::Collab;
use collab_entity::CollabType;
use collab_stream::model::UpdateStreamMessage;
use dashmap::DashSet;
use database::file::s3_client_impl::AwsS3BucketClientImpl;
use database_entity::dto::{
  CollabParams, CollabUpdateData, PendingCollabWrite, QueryCollab, QueryCollabResult,
};
use futures_util::{stream, StreamExt};
use infra::thread_pool::ThreadPoolNoAbort;
use itertools::{Either, Itertools};
use sqlx::{PgPool, Transaction};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, instrument};
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
  /// List of dirty collabs that have pending updates in Redis and need to be flushed to disk.
  dirty_collabs: DashSet<Uuid>,
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
      dirty_collabs: DashSet::new(),
    })
  }

  pub fn mark_as_dirty(&self, object_id: Uuid) {
    tracing::trace!("marking collab {} as dirty", object_id);
    self.dirty_collabs.insert(object_id);
  }

  pub fn mark_as_clean(&self, object_id: &Uuid) {
    tracing::trace!("marking collab {} as clean", object_id);
    self.dirty_collabs.remove(object_id);
  }

  pub fn metrics(&self) -> &CollabMetrics {
    &self.metrics
  }

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

    // Separate small and large data for different processing strategies
    let (small_params, large_params): (Vec<_>, Vec<_>) = params_list
      .into_iter()
      .partition(|params| params.encoded_collab_v1.len() <= self.small_collab_size);

    let timestamp = chrono::Utc::now().timestamp();
    for params in small_params {
      if let Err(err) = self
        .mem_cache
        .insert_encode_collab_data(
          &params.object_id,
          &params.encoded_collab_v1,
          timestamp,
          Some(cache_exp_secs_from_collab_type(&params.collab_type)),
        )
        .await
        .map_err(|err| AppError::Internal(err.into()))
      {
        tracing::warn!(
          "Failed to insert collab `{}` into memory cache: {}",
          params.object_id,
          err
        );
      }
    }

    if !large_params.is_empty() {
      let mem_cache = self.mem_cache.clone();
      tokio::spawn(async move {
        for params in large_params {
          if let Err(err) = mem_cache
            .insert_encode_collab_data(
              &params.object_id,
              &params.encoded_collab_v1,
              timestamp,
              Some(cache_exp_secs_from_collab_type(&params.collab_type)),
            )
            .await
            .map_err(|err| AppError::Internal(err.into()))
          {
            tracing::warn!(
              "Failed to insert collab `{}` into memory cache: {}",
              params.object_id,
              err
            );
          }
        }
      });
    }

    Ok(())
  }

  pub async fn get_encode_collab(
    &self,
    workspace_id: &Uuid,
    query: QueryCollab,
  ) -> Result<(Rid, EncodedCollab), AppError> {
    // Attempt to retrieve encoded collab from memory cache, falling back to disk cache if necessary.
    if let Some(encoded_collab) = self.mem_cache.get_encode_collab(&query.object_id).await {
      tracing::debug!(
        "Did get encode collab: {} from cache at {}",
        query.object_id,
        encoded_collab.0
      );
      return Ok(encoded_collab);
    }

    // Retrieve from disk cache as fallback. After retrieval, the value is inserted into the memory cache.
    let object_id = query.object_id;
    let expiration_secs = cache_exp_secs_from_collab_type(&query.collab_type);
    let (rid, encode_collab) = self
      .disk_cache
      .get_collab_encoded_from_disk(workspace_id, query)
      .await?;

    // spawn a task to insert the encoded collab into the memory cache
    let cloned_encode_collab = encode_collab.clone();
    let mem_cache = self.mem_cache.clone();
    let data_size = cloned_encode_collab.doc_state.len();

    if data_size <= self.small_collab_size {
      // For small data, process on current thread for efficiency
      mem_cache
        .insert_encode_collab(
          &object_id,
          cloned_encode_collab,
          rid.timestamp,
          expiration_secs,
        )
        .await;
    } else {
      // For large data, spawn a task to avoid blocking current thread
      tokio::spawn(async move {
        mem_cache
          .insert_encode_collab(
            &object_id,
            cloned_encode_collab,
            rid.timestamp,
            expiration_secs,
          )
          .await;
      });
    }
    Ok((rid, encode_collab))
  }

  #[instrument(level = "debug", skip_all)]
  pub async fn get_full_collab(
    &self,
    workspace_id: &Uuid,
    query: QueryCollab,
    from: Option<StateVector>,
    encoding: EncoderVersion,
  ) -> Result<(Rid, EncodedCollab), AppError> {
    let object_id = query.object_id;
    let (rid, mut encoded_collab) = match self.get_encode_collab(workspace_id, query).await {
      Ok((rid, encoded_collab)) => (rid, Some(encoded_collab)),
      Err(AppError::RecordNotFound(_)) => (Rid::default(), None),
      Err(err) => return Err(err),
    };

    let from = from.unwrap_or_default();
    if !self.dirty_collabs.contains(&object_id) {
      // there are no pending updates for this collab, so we can return the cached value directly
      tracing::trace!("no pending updates for collab: {}", object_id);
      match encoded_collab {
        Some(encoded_collab) if encoded_collab.doc_state.len() <= self.small_collab_size => {
          return Ok((rid, encoded_collab));
        },
        Some(encoded_collab) => {
          // If the collab is large, we do not replay updates and return the snapshot only.
          let options = CollabOptions::new(object_id.to_string(), default_client_id())
            .with_data_source(match encoded_collab.version {
              EncoderVersion::V1 => DataSource::DocStateV1(encoded_collab.doc_state.into()),
              EncoderVersion::V2 => DataSource::DocStateV2(encoded_collab.doc_state.into()),
            });
          let collab = Collab::new_with_options(CollabOrigin::Server, options)
            .map_err(|err| AppError::Internal(err.into()))?;
          let tx = collab.transact();
          let doc_state: Bytes = match encoded_collab.version {
            EncoderVersion::V1 => tx.encode_diff_v1(&from),
            EncoderVersion::V2 => tx.encode_diff_v2(&from),
          }
          .into();
          return Ok((
            rid,
            EncodedCollab {
              version: encoded_collab.version,
              state_vector: encoded_collab.state_vector,
              doc_state,
            },
          ));
        },
        None => {
          return Err(AppError::RecordNotFound(format!(
            "Collab not found for object_id: {}",
            object_id
          )));
        },
      }
    }

    let updates = self
      .get_workspace_updates(workspace_id, Some(&object_id), Some(rid), None)
      .await?;

    let size = encoded_collab
      .as_ref()
      .map(|v| v.doc_state.len())
      .unwrap_or(0)
      + updates.iter().map(|u| u.update.len()).sum::<usize>();

    if !updates.is_empty() {
      encoded_collab = if size <= self.small_collab_size {
        // For small collab, replaying updates on the current thread
        replaying_updates(encoding, object_id, encoded_collab, updates, &from)?
      } else {
        self
          .thread_pool
          .install(|| replaying_updates(encoding, object_id, encoded_collab, updates, &from))
          .map_err(|err| AppError::Internal(err.into()))??
      }
    }

    match encoded_collab {
      Some(encoded_collab) => Ok((rid, encoded_collab)),
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

  /// Batch get the encoded collab data from the cache.
  /// returns a hashmap of the object_id to the encoded collab data.
  pub async fn batch_get_encode_collab<T: Into<QueryCollab>>(
    &self,
    workspace_id: &Uuid,
    queries: Vec<T>,
  ) -> HashMap<Uuid, QueryCollabResult> {
    let queries = queries.into_iter().map(Into::into).collect::<Vec<_>>();
    let mut results = HashMap::new();
    // 1. Processes valid queries against the in-memory cache to retrieve cached values.
    //    - Queries not found in the cache are earmarked for disk retrieval.
    let (disk_queries, values_from_mem_cache): (Vec<_>, HashMap<_, _>) = stream::iter(queries)
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
    results.extend(values_from_mem_cache);

    // 2. Retrieves remaining values from the disk cache for queries not satisfied by the memory cache.
    //    - These values are then merged into the final result set.
    let values_from_disk_cache = self
      .disk_cache
      .batch_get_collab(workspace_id, disk_queries)
      .await;
    results.extend(values_from_disk_cache);
    results
  }

  /// Insert the encoded collab data into the cache.
  /// The data is inserted into both the memory and disk cache.
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
    let timestamp = params
      .updated_at
      .unwrap_or_else(chrono::Utc::now)
      .timestamp_millis();
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

    // when the data is written to the disk cache but fails to be written to the memory cache
    // we log the error and continue.
    self
      .cache_collab(object_id, collab_type, encode_collab_data, timestamp)
      .await;
    Ok(())
  }

  async fn cache_collab(
    &self,
    object_id: Uuid,
    collab_type: CollabType,
    encode_collab_data: Bytes,
    timestamp: i64,
  ) {
    let data_size = encode_collab_data.len();
    let mem_cache = self.mem_cache.clone();
    let expiration_secs = cache_exp_secs_from_collab_type(&collab_type);
    if data_size <= self.small_collab_size {
      if let Err(err) = mem_cache
        .insert_encode_collab_data(
          &object_id,
          &encode_collab_data,
          timestamp,
          Some(expiration_secs),
        )
        .await
      {
        error!("Failed to insert encode collab into memory cache: {}", err);
      }
    } else {
      tokio::spawn(async move {
        if let Err(err) = mem_cache
          .insert_encode_collab_data(
            &object_id,
            &encode_collab_data,
            timestamp,
            Some(expiration_secs),
          )
          .await
        {
          error!("Failed to insert encode collab into memory cache: {}", err);
        }
      });
    }
    self.mark_as_clean(&object_id);
  }

  pub async fn insert_encode_collab_to_disk(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    params: CollabParams,
  ) -> Result<(), AppError> {
    let p = params.clone();
    self
      .disk_cache
      .upsert_collab(workspace_id, uid, params)
      .await?;
    let timestamp = p
      .updated_at
      .unwrap_or_else(chrono::Utc::now)
      .timestamp_millis();
    self
      .cache_collab(p.object_id, p.collab_type, p.encoded_collab_v1, timestamp)
      .await;
    Ok(())
  }

  pub async fn delete_collab(&self, workspace_id: &Uuid, object_id: &Uuid) -> Result<(), AppError> {
    self.mem_cache.remove_encode_collab(object_id).await?;
    self
      .disk_cache
      .delete_collab(workspace_id, object_id)
      .await?;
    self.mark_as_clean(object_id);
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
    let (small_params, large_params): (Vec<_>, Vec<_>) = mem_cache_params
      .into_iter()
      .partition(|(_, data, _)| data.len() <= self.small_collab_size);

    let now = chrono::Utc::now().timestamp();
    for (oid, data, expire) in small_params {
      if let Err(err) = self
        .mem_cache
        .insert_encode_collab_data(&oid, &data, now, Some(expire))
        .await
      {
        error!(
          "Failed to insert collab `{}` into memory cache: {}",
          oid, err
        );
      }
    }

    if !large_params.is_empty() {
      let mem_cache = self.mem_cache.clone();
      tokio::spawn(async move {
        for (oid, data, expire) in large_params {
          if let Err(err) = mem_cache
            .insert_encode_collab_data(&oid, &data, now, Some(expire))
            .await
          {
            error!(
              "Failed to insert collab `{}` into memory cache: {}",
              oid, err
            );
          }
        }
      });
    }

    Ok(())
  }
}

#[inline]
fn replaying_updates(
  encoding: EncoderVersion,
  object_id: Uuid,
  mut encoded_collab: Option<EncodedCollab>,
  updates: Vec<UpdateStreamMessage>,
  from: &StateVector,
) -> Result<Option<EncodedCollab>, AppError> {
  tracing::trace!("replaying {} updates for {}", updates.len(), object_id);
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
