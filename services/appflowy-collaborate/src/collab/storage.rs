#![allow(unused_imports)]

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use collab_rt_entity::ClientCollabMessage;
use database::collab::cache::CollabCache;
use itertools::{Either, Itertools};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use sqlx::Transaction;
use std::collections::HashMap;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::time::timeout;
use tracing::warn;
use tracing::{error, instrument, trace};
use uuid::Uuid;
use validator::Validate;

use crate::command::{CLCommandSender, CollaborationCommand};
use app_error::AppError;
use database::collab::{
  insert_into_af_collab_bulk_for_user, AppResult, CollabMetadata, CollabStorage,
  CollabStorageAccessControl, GetCollabOrigin,
};
use database_entity::dto::{
  AFAccessLevel, AFSnapshotMeta, AFSnapshotMetas, CollabParams, InsertSnapshotParams, QueryCollab,
  QueryCollabParams, QueryCollabResult, SnapshotData,
};

use crate::collab::access_control::CollabStorageAccessControlImpl;
use crate::collab::validator::CollabValidator;
use crate::metrics::CollabMetrics;
use crate::snapshot::SnapshotControl;

pub type CollabAccessControlStorage = CollabStorageImpl<CollabStorageAccessControlImpl>;

struct PendingCollabWrite {
  workspace_id: String,
  uid: i64,
  params: CollabParams,
}

impl PendingCollabWrite {
  fn new(workspace_id: String, uid: i64, params: CollabParams) -> Self {
    PendingCollabWrite {
      workspace_id,
      uid,
      params,
    }
  }
}

/// A wrapper around the actual storage implementation that provides access control and caching.
#[derive(Clone)]
pub struct CollabStorageImpl<AC> {
  cache: CollabCache,
  /// access control for collab object. Including read/write
  access_control: AC,
  snapshot_control: SnapshotControl,
  rt_cmd_sender: CLCommandSender,
  queue: Sender<PendingCollabWrite>,
}

impl<AC> CollabStorageImpl<AC>
where
  AC: CollabStorageAccessControl,
{
  pub fn new(
    cache: CollabCache,
    access_control: AC,
    snapshot_control: SnapshotControl,
    rt_cmd_sender: CLCommandSender,
  ) -> Self {
    let (queue, reader) = channel(1000);
    {
      let cache = cache.clone();
      tokio::spawn(Self::periodic_write_task(cache, reader));
    }
    Self {
      cache,
      access_control,
      snapshot_control,
      rt_cmd_sender,
      queue,
    }
  }

  const PENDING_WRITE_BUF_CAPACITY: usize = 100;
  async fn periodic_write_task(cache: CollabCache, mut reader: Receiver<PendingCollabWrite>) {
    let mut buf = Vec::with_capacity(Self::PENDING_WRITE_BUF_CAPACITY);
    loop {
      let n = reader
        .recv_many(&mut buf, Self::PENDING_WRITE_BUF_CAPACITY)
        .await;
      if n == 0 {
        break;
      }
      let pending = buf.drain(..n);
      if let Err(e) = Self::persist(&cache, pending).await {
        tracing::error!("failed to persist {} collabs: {}", n, e);
      }
    }
  }

  async fn persist(
    cache: &CollabCache,
    records: impl ExactSizeIterator<Item = PendingCollabWrite>,
  ) -> Result<(), AppError> {
    // Start a database transaction
    let mut transaction = cache
      .pg_pool()
      .begin()
      .await
      .context("Failed to acquire transaction for writing pending collaboration data")
      .map_err(AppError::from)?;

    tracing::trace!("persisting {} collabs", records.len());
    // Insert each record into the database within the transaction context
    let mut action_description = String::new();
    for (index, record) in records.into_iter().enumerate() {
      let params = record.params;
      action_description = format!("{}", params);
      let savepoint_name = format!("sp_{}", index);

      // using savepoint to rollback the transaction if the insert fails
      sqlx::query(&format!("SAVEPOINT {}", savepoint_name))
        .execute(transaction.deref_mut())
        .await?;
      if let Err(_err) = cache
        .insert_encode_collab_to_disk(&record.workspace_id, &record.uid, params, &mut transaction)
        .await
      {
        sqlx::query(&format!("ROLLBACK TO SAVEPOINT {}", savepoint_name))
          .execute(transaction.deref_mut())
          .await?;
      }
    }

    // Commit the transaction to finalize all writes
    match tokio::time::timeout(Duration::from_secs(10), transaction.commit()).await {
      Ok(result) => {
        result.map_err(AppError::from)?;
        Ok(())
      },
      Err(_) => {
        error!(
          "Timeout waiting for committing the transaction for pending write:{}",
          action_description
        );
        Err(AppError::Internal(anyhow!(
          "Timeout when committing the transaction for pending collaboration data"
        )))
      },
    }
  }

  async fn insert_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
  ) -> AppResult<()> {
    // Start a database transaction
    let mut transaction = self
      .cache
      .pg_pool()
      .begin()
      .await
      .context("Failed to acquire transaction for writing pending collaboration data")
      .map_err(AppError::from)?;
    self
      .cache
      .insert_encode_collab_to_disk(workspace_id, uid, params, &mut transaction)
      .await?;
    tokio::time::timeout(Duration::from_secs(10), transaction.commit())
      .await
      .map_err(|_| {
        AppError::Internal(anyhow!(
          "Timeout when committing the transaction for pending collaboration data"
        ))
      })??;
    Ok(())
  }

  async fn check_write_workspace_permission(
    &self,
    workspace_id: &str,
    uid: &i64,
  ) -> Result<(), AppError> {
    // If the collab doesn't exist, check if the user has enough permissions to create collab.
    // If the user is the owner or member of the workspace, the user can create collab.
    self
      .access_control
      .enforce_write_workspace(uid, workspace_id)
      .await?;
    Ok(())
  }

  async fn check_write_collab_permission(
    &self,
    workspace_id: &str,
    uid: &i64,
    object_id: &str,
  ) -> Result<(), AppError> {
    // If the collab already exists, check if the user has enough permissions to update collab
    self
      .access_control
      .enforce_write_collab(workspace_id, uid, object_id)
      .await?;
    Ok(())
  }

  async fn get_encode_collab_from_editing(&self, object_id: &str) -> Option<EncodedCollab> {
    let object_id = object_id.to_string();
    let (ret, rx) = tokio::sync::oneshot::channel();
    let timeout_duration = Duration::from_secs(5);

    // Attempt to send the command to the realtime server
    if let Err(err) = self
      .rt_cmd_sender
      .send(CollaborationCommand::GetEncodeCollab { object_id, ret })
      .await
    {
      error!(
        "Failed to send get encode collab command to realtime server: {}",
        err
      );
      return None;
    }

    // Await the response from the realtime server with a timeout
    match timeout(timeout_duration, rx).await {
      Ok(Ok(Some(encode_collab))) => Some(encode_collab),
      Ok(Ok(None)) => {
        trace!("No encode collab found in editing collab");
        None
      },
      Ok(Err(err)) => {
        error!("Failed to get encode collab from realtime server: {}", err);
        None
      },
      Err(_) => {
        error!("Timeout waiting for encode collab from realtime server");
        None
      },
    }
  }

  async fn batch_get_encode_collab_from_editing(
    &self,
    object_ids: Vec<String>,
  ) -> HashMap<String, EncodedCollab> {
    let (ret, rx) = tokio::sync::oneshot::channel();
    let timeout_duration = Duration::from_secs(10);

    // Attempt to send the command to the realtime server
    if let Err(err) = self
      .rt_cmd_sender
      .send(CollaborationCommand::BatchGetEncodeCollab { object_ids, ret })
      .await
    {
      error!(
        "Failed to send get encode collab command to realtime server: {}",
        err
      );
      return HashMap::new();
    }

    // Await the response from the realtime server with a timeout
    match timeout(timeout_duration, rx).await {
      Ok(Ok(batch_encoded_collab)) => batch_encoded_collab,
      Ok(Err(err)) => {
        error!("Failed to get encode collab from realtime server: {}", err);
        HashMap::new()
      },
      Err(_) => {
        error!("Timeout waiting for batch encode collab from realtime server");
        HashMap::new()
      },
    }
  }

  async fn queue_insert_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
  ) -> Result<(), AppError> {
    trace!(
      "Queue insert collab:{}:{}",
      params.object_id,
      params.collab_type
    );
    if let Err(err) = params.check_encode_collab().await {
      return Err(AppError::NoRequiredData(format!(
        "Invalid collab doc state detected for workspace_id: {}, uid: {}, object_id: {} collab_type:{}. Error details: {}",
        workspace_id, uid, params.object_id, params.collab_type, err
      )));
    }

    let pending = PendingCollabWrite::new(workspace_id.into(), *uid, params);
    if let Err(e) = self.queue.send(pending).await {
      tracing::error!("Failed to queue insert collab doc state: {}", e);
    }
    Ok(())
  }

  async fn batch_insert_collabs(
    &self,
    workspace_id: &str,
    uid: &i64,
    params_list: Vec<CollabParams>,
  ) -> Result<(), AppError> {
    let mut transaction = self.cache.pg_pool().begin().await?;
    insert_into_af_collab_bulk_for_user(&mut transaction, uid, workspace_id, &params_list).await?;
    transaction.commit().await?;

    // update the mem cache without blocking the current task
    let cache = self.cache.clone();
    tokio::spawn(async move {
      for params in params_list {
        let _ = cache.insert_encode_collab_to_mem(&params).await;
      }
    });
    Ok(())
  }
}

#[async_trait]
impl<AC> CollabStorage for CollabStorageImpl<AC>
where
  AC: CollabStorageAccessControl,
{
  fn encode_collab_redis_query_state(&self) -> (u64, u64) {
    let state = self.cache.query_state();
    (state.total_attempts, state.success_attempts)
  }

  async fn queue_insert_or_update_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    write_immediately: bool,
  ) -> AppResult<()> {
    params.validate()?;
    let is_exist = self.cache.is_exist(&params.object_id).await?;
    // If the collab already exists, check if the user has enough permissions to update collab
    // Otherwise, check if the user has enough permissions to create collab.
    if is_exist {
      self
        .check_write_collab_permission(workspace_id, uid, &params.object_id)
        .await?;
    } else {
      self
        .check_write_workspace_permission(workspace_id, uid)
        .await?;
      trace!(
        "Update policy for user:{} to create collab:{}",
        uid,
        params.object_id
      );
      self
        .access_control
        .update_policy(uid, &params.object_id, AFAccessLevel::FullAccess)
        .await?;
    }
    if write_immediately {
      self.insert_collab(workspace_id, uid, params).await?;
    } else {
      self.queue_insert_collab(workspace_id, uid, params).await?;
    }
    Ok(())
  }

  async fn batch_insert_new_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params_list: Vec<CollabParams>,
  ) -> AppResult<()> {
    self
      .check_write_workspace_permission(workspace_id, uid)
      .await?;

    // TODO(nathan): batch insert permission
    for params in &params_list {
      self
        .access_control
        .update_policy(uid, &params.object_id, AFAccessLevel::FullAccess)
        .await?;
    }

    match tokio::time::timeout(
      Duration::from_secs(60),
      self.batch_insert_collabs(workspace_id, uid, params_list),
    )
    .await
    {
      Ok(result) => result,
      Err(_) => {
        error!("Timeout waiting for action completed",);
        Err(AppError::RequestTimeout("".to_string()))
      },
    }
  }

  #[instrument(level = "trace", skip(self, params), oid = %params.oid, ty = %params.collab_type, err)]
  #[allow(clippy::blocks_in_conditions)]
  async fn insert_new_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    action_description: &str,
  ) -> AppResult<()> {
    params.validate()?;
    self
      .check_write_workspace_permission(workspace_id, uid)
      .await?;
    self
      .access_control
      .update_policy(uid, &params.object_id, AFAccessLevel::FullAccess)
      .await?;

    match tokio::time::timeout(
      Duration::from_secs(120),
      self
        .cache
        .insert_encode_collab_data(workspace_id, uid, &params, transaction),
    )
    .await
    {
      Ok(result) => result,
      Err(_) => {
        error!(
          "Timeout waiting for action completed: {}",
          action_description
        );
        Err(AppError::RequestTimeout(action_description.to_string()))
      },
    }
  }

  #[instrument(level = "trace", skip_all, fields(oid = %params.object_id, from_editing_collab = %from_editing_collab))]
  async fn get_encode_collab(
    &self,
    origin: GetCollabOrigin,
    params: QueryCollabParams,
    from_editing_collab: bool,
  ) -> AppResult<EncodedCollab> {
    params.validate()?;
    match origin {
      GetCollabOrigin::User { uid } => {
        // Check if the user has enough permissions to access the collab
        self
          .access_control
          .enforce_read_collab(&params.workspace_id, &uid, &params.object_id)
          .await?;
      },
      GetCollabOrigin::Server => {},
    }

    // Early return if editing collab is initialized, as it indicates no need to query further.
    if from_editing_collab {
      // Attempt to retrieve encoded collab from the editing collab
      if let Some(value) = self.get_encode_collab_from_editing(&params.object_id).await {
        trace!(
          "Did get encode collab {} from editing collab",
          params.object_id
        );
        return Ok(value);
      }
    }

    let encode_collab = self.cache.get_encode_collab(params.inner).await?;
    Ok(encode_collab)
  }

  async fn batch_get_collab(
    &self,
    _uid: &i64,
    queries: Vec<QueryCollab>,
    from_editing_collab: bool,
  ) -> HashMap<String, QueryCollabResult> {
    // Partition queries based on validation into valid queries and errors (with associated error messages).
    let (valid_queries, mut results): (Vec<_>, HashMap<_, _>) =
      queries
        .into_iter()
        .partition_map(|params| match params.validate() {
          Ok(_) => Either::Left(params),
          Err(err) => Either::Right((
            params.object_id,
            QueryCollabResult::Failed {
              error: err.to_string(),
            },
          )),
        });
    let cache_queries = if from_editing_collab {
      let editing_queries = valid_queries.clone();
      let editing_results = self
        .batch_get_encode_collab_from_editing(
          editing_queries
            .iter()
            .map(|q| q.object_id.clone())
            .collect(),
        )
        .await;
      let editing_query_collab_results: HashMap<String, QueryCollabResult> =
        tokio::task::spawn_blocking(move || {
          let par_iter = editing_results.into_par_iter();
          par_iter
            .map(|(object_id, encoded_collab)| {
              let encoding_result = encoded_collab.encode_to_bytes();
              let query_collab_result = match encoding_result {
                Ok(encoded_collab_bytes) => QueryCollabResult::Success {
                  encode_collab_v1: encoded_collab_bytes,
                },
                Err(err) => QueryCollabResult::Failed {
                  error: err.to_string(),
                },
              };

              (object_id.clone(), query_collab_result)
            })
            .collect()
        })
        .await
        .unwrap();
      let editing_object_ids: Vec<String> = editing_query_collab_results.keys().cloned().collect();
      results.extend(editing_query_collab_results);
      valid_queries
        .into_iter()
        .filter(|q| !editing_object_ids.contains(&q.object_id))
        .collect()
    } else {
      valid_queries
    };

    results.extend(self.cache.batch_get_encode_collab(cache_queries).await);
    results
  }

  async fn delete_collab(&self, workspace_id: &str, uid: &i64, object_id: &str) -> AppResult<()> {
    self
      .access_control
      .enforce_delete(workspace_id, uid, object_id)
      .await?;
    self.cache.delete_collab(object_id).await?;
    Ok(())
  }

  async fn query_collab_meta(
    &self,
    object_id: &str,
    collab_type: &CollabType,
  ) -> AppResult<CollabMetadata> {
    self.cache.get_collab_meta(object_id, collab_type).await
  }

  async fn should_create_snapshot(&self, oid: &str) -> Result<bool, AppError> {
    self.snapshot_control.should_create_snapshot(oid).await
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> AppResult<AFSnapshotMeta> {
    self.snapshot_control.create_snapshot(params).await
  }

  async fn queue_snapshot(&self, params: InsertSnapshotParams) -> AppResult<()> {
    self.snapshot_control.queue_snapshot(params).await
  }

  async fn get_collab_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    snapshot_id: &i64,
  ) -> AppResult<SnapshotData> {
    self
      .snapshot_control
      .get_snapshot(workspace_id, object_id, snapshot_id)
      .await
  }

  async fn get_collab_snapshot_list(&self, oid: &str) -> AppResult<AFSnapshotMetas> {
    self.snapshot_control.get_collab_snapshot_list(oid).await
  }

  async fn broadcast_encode_collab(
    &self,
    object_id: String,
    collab_messages: Vec<ClientCollabMessage>,
  ) -> Result<(), AppError> {
    let (sender, recv) = tokio::sync::oneshot::channel();

    self
      .rt_cmd_sender
      .send(CollaborationCommand::ServerSendCollabMessage {
        object_id,
        collab_messages,
        ret: sender,
      })
      .await
      .map_err(|err| {
        AppError::Unhandled(format!(
          "Failed to send encode collab command to realtime server: {}",
          err
        ))
      })?;

    match recv.await {
      Ok(res) =>
        if let Err(err) = res {
          error!("Failed to broadcast encode collab: {}", err);
        }
      ,
      // caller may have dropped the receiver
      Err(err) => warn!("Failed to receive response from realtime server: {}", err),
    }

    Ok(())
  }
}
