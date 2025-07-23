#![allow(unused_imports)]

use crate::collab::cache::mem_cache::MillisSeconds;
use crate::collab::cache::CollabCache;
use crate::metrics::CollabMetrics;
use crate::ws2::CollabManager;
use access_control::act::Action;
use access_control::collab::CollabAccessControl;
use access_control::workspace::WorkspaceAccessControl;
use anyhow::{anyhow, Context};
use app_error::AppError;
use appflowy_proto::{ObjectId, Rid, TimestampedEncodedCollab, UpdateFlags, WorkspaceId};
use async_trait::async_trait;
use chrono::Timelike;
use collab::entity::{EncodedCollab, EncoderVersion};
use collab_entity::CollabType;
use collab_rt_entity::ClientCollabMessage;
use collab_rt_protocol::validate_encode_collab;
use database::collab::{
  insert_into_af_collab_bulk_for_user, AppResult, CollabMetadata, CollabStore, GetCollabOrigin,
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
use yrs::{StateVector, Update};

/// A wrapper around the actual storage implementation that provides access control and caching.
#[derive(Clone)]
pub struct CollabStoreImpl {
  cache: Arc<CollabCache>,
  /// access control for collab object. Including read/write
  access_control: Arc<dyn CollabAccessControl>,
  workspace_access_control: Arc<dyn WorkspaceAccessControl>,
  queue: Sender<PendingCollabWrite>,
}

impl CollabStoreImpl {
  pub fn new(
    cache: Arc<CollabCache>,
    access_control: Arc<dyn CollabAccessControl>,
    workspace_access_control: Arc<dyn WorkspaceAccessControl>,
  ) -> Self {
    let (queue, reader) = channel(1000);
    tokio::spawn(Self::periodic_write_task(cache.clone(), reader));
    Self {
      cache,
      access_control,
      workspace_access_control,
      queue,
    }
  }

  async fn check_or_update_permission(
    &self,
    uid: &i64,
    workspace_id: &WorkspaceId,
    object_id: &ObjectId,
  ) -> AppResult<()> {
    let is_exist = self.cache.is_exist(workspace_id, object_id).await?;
    // If the collab already exists, check if the user has enough permissions to update collab
    // Otherwise, check if the user has enough permissions to create collab.
    if is_exist {
      self
        .access_control
        .enforce_action(workspace_id, uid, object_id, Action::Write)
        .await?;
    } else {
      self
        .check_write_workspace_permission(workspace_id, uid)
        .await?;
      trace!(
        "Update policy for user:{} to create collab:{}",
        uid,
        object_id
      );
      self
        .access_control
        .update_access_level_policy(uid, object_id, AFAccessLevel::FullAccess)
        .await?;
    }

    Ok(())
  }

  pub fn metrics(&self) -> &CollabMetrics {
    self.cache.metrics()
  }

  const PENDING_WRITE_BUF_CAPACITY: usize = 20;
  async fn periodic_write_task(cache: Arc<CollabCache>, mut reader: Receiver<PendingCollabWrite>) {
    let mut buf = Vec::with_capacity(Self::PENDING_WRITE_BUF_CAPACITY);
    loop {
      let n = reader
        .recv_many(&mut buf, Self::PENDING_WRITE_BUF_CAPACITY)
        .await;
      if n == 0 {
        break;
      }
      let pending = buf.drain(..n);
      trace!("Persisting {} collabs to disk", n);
      if let Err(e) = cache.batch_insert_collab(pending.collect()).await {
        error!("failed to persist {} collabs: {}", n, e);
      }
    }
  }

  async fn check_write_workspace_permission(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
  ) -> Result<(), AppError> {
    // If the collab doesn't exist, check if the user has enough permissions to create collab.
    // If the user is the owner or member of the workspace, the user can create collab.
    self
      .workspace_access_control
      .enforce_action(uid, workspace_id, Action::Write)
      .await?;
    Ok(())
  }

  /// **Note: This function will override any existing values without timestamp comparison.**
  /// Use the single insert methods if you need conditional insertion based on timestamps.
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
}

#[async_trait]
impl CollabStore for CollabStoreImpl {
  async fn upsert_collab(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params: CollabParams,
  ) -> AppResult<()> {
    self
      .check_or_update_permission(uid, &workspace_id, &params.object_id)
      .await?;
    self
      .cache
      .insert_encode_collab_to_disk(&workspace_id, uid, params)
      .await?;
    Ok(())
  }

  async fn upsert_collab_background(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params: CollabParams,
  ) -> AppResult<()> {
    self
      .check_or_update_permission(uid, &workspace_id, &params.object_id)
      .await?;
    trace!(
      "Queue insert collab:{}:{}",
      params.object_id,
      params.collab_type
    );

    if let Err(err) = validate_encode_collab(
      &params.object_id,
      &params.encoded_collab_v1,
      &params.collab_type,
    )
    .await
    .map_err(|err| AppError::NoRequiredData(err.to_string()))
    {
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
      .update_access_level_policy(uid, &params.object_id, AFAccessLevel::FullAccess)
      .await?;

    match tokio::time::timeout(
      Duration::from_secs(30),
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

  async fn batch_insert_new_collab(
    &self,
    workspace_id: Uuid,
    uid: &i64,
    params_list: Vec<CollabParams>,
  ) -> AppResult<()> {
    self
      .check_write_workspace_permission(&workspace_id, uid)
      .await?;

    for params in &params_list {
      self
        .access_control
        .update_access_level_policy(uid, &params.object_id, AFAccessLevel::FullAccess)
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

  #[instrument(level = "trace", skip_all)]
  async fn get_full_encode_collab(
    &self,
    origin: GetCollabOrigin,
    workspace_id: &Uuid,
    object_id: &Uuid,
    collab_type: CollabType,
  ) -> AppResult<TimestampedEncodedCollab> {
    if let GetCollabOrigin::User { uid } = origin {
      // Check if the user has enough permissions to access the collab
      trace!(
        "enforce read collab for user: {}, object_id: {}",
        uid,
        object_id
      );
      let collab_exists = self.cache.is_exist(workspace_id, object_id).await?;
      if collab_exists {
        self
          .access_control
          .enforce_action(workspace_id, &uid, object_id, Action::Read)
          .await?;
      }
    }

    self
      .cache
      .get_full_collab(
        workspace_id,
        QueryCollab::new(*object_id, collab_type),
        None,
        EncoderVersion::V1,
      )
      .await
  }

  async fn batch_get_collab(
    &self,
    _uid: &i64,
    workspace_id: Uuid,
    queries: Vec<QueryCollab>,
  ) -> HashMap<Uuid, QueryCollabResult> {
    if queries.is_empty() {
      return HashMap::new();
    }
    self
      .cache
      .batch_get_full_collab(&workspace_id, queries, None, EncoderVersion::V1)
      .await
  }

  async fn delete_collab(&self, workspace_id: &Uuid, uid: &i64, object_id: &Uuid) -> AppResult<()> {
    self
      .access_control
      .enforce_access_level(workspace_id, uid, object_id, AFAccessLevel::FullAccess)
      .await?;
    self.cache.delete_collab(workspace_id, object_id).await?;
    Ok(())
  }

  #[instrument(level = "trace", skip_all)]
  fn mark_as_editing(&self, oid: Uuid) {
    self.cache.mark_as_dirty(oid, MillisSeconds::now());
  }
}
