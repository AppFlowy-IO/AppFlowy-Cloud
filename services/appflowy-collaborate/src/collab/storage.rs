#![allow(unused_imports)]

use crate::command::{CLCommandSender, CollaborationCommand};
use anyhow::{anyhow, Context};
use app_error::AppError;
use async_trait::async_trait;
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use collab_rt_entity::ClientCollabMessage;
use database::collab::{
  insert_into_af_collab_bulk_for_user, AppResult, CollabMetadata, CollabStorage,
  CollabStorageAccessControl, GetCollabOrigin,
};
use database_entity::dto::{
  AFAccessLevel, AFSnapshotMeta, AFSnapshotMetas, CollabParams, InsertSnapshotParams,
  PendingCollabWrite, QueryCollab, QueryCollabParams, QueryCollabResult, SnapshotData,
};
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
use yrs::Update;

use crate::collab::access_control::CollabStorageAccessControlImpl;
use crate::collab::cache::CollabCache;
use crate::collab::validator::CollabValidator;
use crate::metrics::CollabMetrics;
use crate::snapshot::SnapshotControl;

pub type CollabAccessControlStorage = CollabStorageImpl<CollabStorageAccessControlImpl>;

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
    tokio::spawn(Self::periodic_write_task(cache.clone(), reader));
    Self {
      cache,
      access_control,
      snapshot_control,
      rt_cmd_sender,
      queue,
    }
  }

  pub fn metrics(&self) -> &CollabMetrics {
    self.cache.metrics()
  }

  const PENDING_WRITE_BUF_CAPACITY: usize = 20;
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
      if let Err(e) = cache.batch_insert_collab(pending.collect()).await {
        error!("failed to persist {} collabs: {}", n, e);
      }
    }
  }

  async fn insert_collab(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    params: CollabParams,
  ) -> AppResult<()> {
    self
      .cache
      .insert_encode_collab_to_disk(workspace_id, uid, params)
      .await?;
    Ok(())
  }

  async fn check_write_workspace_permission(
    &self,
    workspace_id: &Uuid,
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
    workspace_id: &Uuid,
    uid: &i64,
    object_id: &Uuid,
  ) -> Result<(), AppError> {
    // If the collab already exists, check if the user has enough permissions to update collab
    self
      .access_control
      .enforce_write_collab(workspace_id, uid, object_id)
      .await?;
    Ok(())
  }
  async fn get_encode_collab_from_editing(&self, object_id: Uuid) -> Option<EncodedCollab> {
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
        trace!("Editing collab not found: `{}`", object_id);
        None
      },
      Ok(Err(err)) => {
        error!(
          "Failed to get collab from realtime server `{}`: {}",
          object_id, err
        );
        None
      },
      Err(_) => {
        error!(
          "Timeout trying to read collab `{}` from realtime server",
          object_id
        );
        None
      },
    }
  }

  async fn batch_get_encode_collab_from_editing(
    &self,
    object_ids: Vec<Uuid>,
  ) -> HashMap<Uuid, EncodedCollab> {
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
    workspace_id: Uuid,
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

    let pending = PendingCollabWrite::new(workspace_id, *uid, params);
    if let Err(e) = self.queue.send(pending).await {
      error!("Failed to queue insert collab doc state: {}", e);
    }
    Ok(())
  }

  async fn batch_insert_collabs(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params_list: Vec<CollabParams>,
  ) -> Result<(), AppError> {
    self
      .cache
      .bulk_insert_collab(workspace_id, uid, params_list)
      .await
  }

  /// Sends a collab message to all connected clients.
  /// # Arguments
  /// * `object_id` - The ID of the collaboration object.
  /// * `collab_messages` - The list of collab messages to broadcast.
  pub async fn broadcast_encode_collab(
    &self,
    object_id: Uuid,
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

#[async_trait]
impl<AC> CollabStorage for CollabStorageImpl<AC>
where
  AC: CollabStorageAccessControl,
{
  async fn queue_insert_or_update_collab(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params: CollabParams,
    flush_to_disk: bool,
  ) -> AppResult<()> {
    params.validate()?;
    let is_exist = self
      .cache
      .is_exist(&workspace_id, &params.object_id)
      .await?;
    // If the collab already exists, check if the user has enough permissions to update collab
    // Otherwise, check if the user has enough permissions to create collab.
    if is_exist {
      self
        .check_write_collab_permission(&workspace_id, uid, &params.object_id)
        .await?;
    } else {
      self
        .check_write_workspace_permission(&workspace_id, uid)
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
    if flush_to_disk {
      self.insert_collab(&workspace_id, uid, params).await?;
    } else {
      self.queue_insert_collab(workspace_id, uid, params).await?;
    }
    Ok(())
  }

  async fn batch_insert_new_collab(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params_list: Vec<CollabParams>,
  ) -> AppResult<()> {
    self
      .check_write_workspace_permission(&workspace_id, uid)
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
  async fn upsert_new_collab_with_transaction(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    action_description: &str,
  ) -> AppResult<()> {
    params.validate()?;
    self
      .check_write_workspace_permission(&workspace_id, uid)
      .await?;
    self
      .access_control
      .update_policy(uid, &params.object_id, AFAccessLevel::FullAccess)
      .await?;

    match tokio::time::timeout(
      Duration::from_secs(120),
      self
        .cache
        .insert_encode_collab_data(&workspace_id, uid, params, transaction),
    )
    .await
    {
      Ok(Ok(())) => Ok(()),
      Ok(Err(err)) => Err(err),
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
      if let Some(value) = self.get_encode_collab_from_editing(params.object_id).await {
        trace!(
          "Did get encode collab {} from editing collab",
          params.object_id
        );
        return Ok(value);
      }
    }

    let (_, encode_collab) = self
      .cache
      .get_encode_collab(&params.workspace_id, params.inner)
      .await?;
    Ok(encode_collab)
  }

  async fn batch_get_collab(
    &self,
    _uid: &i64,
    workspace_id: Uuid,
    queries: Vec<QueryCollab>,
    from_editing_collab: bool,
  ) -> HashMap<Uuid, QueryCollabResult> {
    if queries.is_empty() {
      return HashMap::new();
    }

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
        .batch_get_encode_collab_from_editing(editing_queries.iter().map(|q| q.object_id).collect())
        .await;
      let editing_query_collab_results: HashMap<Uuid, QueryCollabResult> =
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

              (object_id, query_collab_result)
            })
            .collect()
        })
        .await
        .unwrap();
      let editing_object_ids: Vec<_> = editing_query_collab_results.keys().cloned().collect();
      results.extend(editing_query_collab_results);
      valid_queries
        .into_iter()
        .filter(|q| !editing_object_ids.contains(&q.object_id))
        .collect()
    } else {
      valid_queries
    };

    results.extend(
      self
        .cache
        .batch_get_encode_collab(&workspace_id, cache_queries)
        .await,
    );
    results
  }

  async fn delete_collab(&self, workspace_id: &Uuid, uid: &i64, object_id: &Uuid) -> AppResult<()> {
    self
      .access_control
      .enforce_delete(workspace_id, uid, object_id)
      .await?;
    self.cache.delete_collab(workspace_id, object_id).await?;
    Ok(())
  }

  async fn should_create_snapshot(
    &self,
    workspace_id: &Uuid,
    oid: &Uuid,
  ) -> Result<bool, AppError> {
    self
      .snapshot_control
      .should_create_snapshot(workspace_id, oid)
      .await
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> AppResult<AFSnapshotMeta> {
    self.snapshot_control.create_snapshot(params).await
  }

  async fn queue_snapshot(&self, params: InsertSnapshotParams) -> AppResult<()> {
    self.snapshot_control.queue_snapshot(params).await
  }

  async fn get_collab_snapshot(
    &self,
    workspace_id: Uuid,
    object_id: Uuid,
    snapshot_id: &i64,
  ) -> AppResult<SnapshotData> {
    self
      .snapshot_control
      .get_snapshot(workspace_id, object_id, snapshot_id)
      .await
  }

  async fn get_latest_snapshot(
    &self,
    workspace_id: Uuid,
    object_id: Uuid,
    collab_type: CollabType,
  ) -> AppResult<Option<SnapshotData>> {
    self
      .snapshot_control
      .get_latest_snapshot(workspace_id, object_id, collab_type)
      .await
  }

  async fn get_collab_snapshot_list(
    &self,
    workspace_id: &Uuid,
    oid: &Uuid,
  ) -> AppResult<AFSnapshotMetas> {
    self
      .snapshot_control
      .get_collab_snapshot_list(workspace_id, oid)
      .await
  }
}
