use crate::collab::cache::mem_cache::MillisSeconds;
use crate::collab::cache::CollabCache;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_proto::{ObjectId, Rid, UpdateFlags, WorkspaceId};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use collab::core::collab::{default_client_id, CollabOptions};
use collab::core::origin::CollabOrigin;
use collab::entity::{EncodedCollab, EncoderVersion};
use collab::preclude::Collab;
use collab_document::document::DocumentBody;
use collab_entity::CollabType;
use collab_stream::awareness_gossip::AwarenessGossip;
use collab_stream::lease::Lease;
use collab_stream::model::{AwarenessStreamUpdate, MessageId, UpdateStreamMessage};
use collab_stream::stream_router::StreamRouter;
use database::collab::{AppResult, CollabStorageAccessControl};
use database_entity::dto::{CollabParams, CollabUpdateData, QueryCollab};
use indexer::scheduler::{IndexerScheduler, UnindexedCollabTask, UnindexedData};
use infra::thread_pool::ThreadPoolNoAbort;
use itertools::Itertools;
use rayon::prelude::*;
use redis::aio::ConnectionManager;
use redis::streams::{StreamTrimOptions, StreamTrimmingMode};
use redis::AsyncCommands;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tracing::{instrument, trace, warn};
use uuid::Uuid;
use yrs::block::ClientID;
use yrs::sync::AwarenessUpdate;
use yrs::updates::decoder::Decode;
use yrs::{ReadTxn, StateVector, Update};

pub struct CollabStore {
  collab_cache: Arc<CollabCache>,
  access_control: Arc<dyn CollabStorageAccessControl + Send + Sync + 'static>,
  update_streams: Arc<StreamRouter>,
  awareness_broadcast: Arc<AwarenessGossip>,
  connection_manager: ConnectionManager,
  indexer_scheduler: Arc<IndexerScheduler>,
  snapshot_thread_pool: Arc<ThreadPoolNoAbort>,
}

impl CollabStore {
  #[allow(clippy::too_many_arguments)]
  pub fn new<AC: CollabStorageAccessControl + Send + Sync + 'static>(
    thread_pool: Arc<ThreadPoolNoAbort>,
    access_control: AC,
    collab_cache: Arc<CollabCache>,
    connection_manager: ConnectionManager,
    update_streams: Arc<StreamRouter>,
    awareness_broadcast: Arc<AwarenessGossip>,
    indexer_scheduler: Arc<IndexerScheduler>,
  ) -> Arc<Self> {
    Arc::new(Self {
      access_control: Arc::new(access_control),
      collab_cache,
      update_streams,
      awareness_broadcast,
      connection_manager,
      indexer_scheduler,
      snapshot_thread_pool: thread_pool,
    })
  }

  pub fn updates(&self) -> &StreamRouter {
    &self.update_streams
  }

  pub fn awareness(&self) -> &AwarenessGossip {
    &self.awareness_broadcast
  }

  pub async fn publish_create(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    sender: &CollabOrigin,
    update: Vec<u8>,
  ) -> anyhow::Result<Rid> {
    self
      .publish_update(workspace_id, object_id, collab_type, sender, update)
      .await
  }

  pub async fn prune_updates(&self, workspace_id: WorkspaceId, up_to: Rid) -> anyhow::Result<()> {
    let key = UpdateStreamMessage::stream_key(&workspace_id);
    let mut conn = self.connection_manager.clone();
    let options = StreamTrimOptions::minid(StreamTrimmingMode::Exact, up_to.to_string());
    let _: redis::Value = conn.xtrim_options(key, &options).await?;
    tracing::info!("pruned updates from workspace {}", workspace_id);
    Ok(())
  }

  async fn get_snapshot(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
  ) -> AppResult<(Rid, Bytes)> {
    match self
      .collab_cache
      .get_snapshot_collab(&workspace_id, QueryCollab::new(object_id, collab_type))
      .await
    {
      Ok((rid, collab)) => Ok((rid, collab.doc_state)),
      Err(AppError::RecordNotFound(_)) => Ok((Rid::default(), Bytes::from_static(&[0, 0]))),
      Err(err) => Err(err),
    }
  }

  pub async fn enforce_read_collab(
    &self,
    workspace_id: &WorkspaceId,
    uid: &i64,
    object_id: &ObjectId,
  ) -> AppResult<()> {
    self
      .access_control
      .enforce_read_collab(workspace_id, uid, object_id)
      .await
  }

  pub async fn enforce_write_collab(
    &self,
    workspace_id: &WorkspaceId,
    uid: &i64,
    object_id: &ObjectId,
  ) -> AppResult<()> {
    self
      .access_control
      .enforce_write_collab(workspace_id, uid, object_id)
      .await
  }

  pub async fn get_collabs_created_since(
    &self,
    workspace_id: Uuid,
    since: DateTime<Utc>,
    limit: usize,
  ) -> Result<Vec<CollabUpdateData>, AppError> {
    self
      .collab_cache
      .get_collabs_created_since(workspace_id, since, limit)
      .await
  }

  /// Returns the latest full state of an object (including all updates).
  pub async fn get_latest_state(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    user_id: i64,
    state_vector: StateVector,
  ) -> AppResult<CollabState> {
    let params = QueryCollab::new(object_id, collab_type);
    self
      .access_control
      .enforce_read_collab(&workspace_id, &user_id, &object_id)
      .await?;

    let (rid, encoded_collab) = self
      .collab_cache
      .get_full_collab(
        &workspace_id,
        params,
        Some(state_vector),
        EncoderVersion::V1,
      )
      .await?;

    Ok(CollabState {
      rid,
      flags: UpdateFlags::Lib0v1,
      update: encoded_collab.doc_state,
      state_vector: encoded_collab.state_vector.into(),
    })
  }

  pub async fn get_workspace_updates(
    &self,
    workspace_id: &WorkspaceId,
    since: MessageId,
  ) -> AppResult<Vec<UpdateStreamMessage>> {
    self
      .collab_cache
      .get_workspace_updates(workspace_id, None, Some(since.into()), None)
      .await
  }

  #[instrument(level = "trace", skip_all, err)]
  pub async fn publish_update(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    sender: &CollabOrigin,
    update: Vec<u8>,
  ) -> anyhow::Result<Rid> {
    trace!(
      "publish_update({:?}, {}/{}, update: {:#?})",
      workspace_id,
      object_id,
      collab_type,
      yrs::Update::decode_v1(&update)
    );

    let key = UpdateStreamMessage::stream_key(&workspace_id);
    let mut conn = self.connection_manager.clone();
    let millis_seconds = MillisSeconds::now();
    let rid: String = UpdateStreamMessage::prepare_command(
      &key,
      &object_id,
      collab_type,
      sender,
      update,
      UpdateFlags::Lib0v1.into(),
    )
    .query_async(&mut conn)
    .await?;
    self.collab_cache.mark_as_dirty(object_id, millis_seconds);
    let rid = Rid::from_str(&rid).map_err(|err| anyhow!("failed to parse rid: {}", err))?;
    trace!(
      "published update to '{}' (object id: {}), rid:{}",
      key,
      object_id,
      rid
    );
    Ok(rid)
  }

  pub fn mark_as_dirty(&self, object_id: ObjectId) {
    self
      .collab_cache
      .mark_as_dirty(object_id, MillisSeconds::now());
  }

  #[instrument(level = "trace", skip_all, err)]
  pub async fn publish_awareness_update(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    sender: CollabOrigin,
    update: AwarenessUpdate,
  ) -> anyhow::Result<()> {
    let msg = AwarenessStreamUpdate {
      data: update,
      sender,
    };
    trace!("broadcast {} awareness update", object_id);
    self
      .awareness_broadcast
      .send(&workspace_id.to_string(), &object_id.to_string(), &msg)
      .await?;
    Ok(())
  }

  async fn save_snapshot(
    collab_cache: Arc<CollabCache>,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    uid: i64,
    last_message_id: Rid,
    update: Bytes,
  ) -> anyhow::Result<()> {
    let encoded_collab = EncodedCollab::new_v1(Bytes::default(), update);
    let updated_at = DateTime::<Utc>::from_timestamp_millis(last_message_id.timestamp as i64);
    let params = CollabParams {
      object_id,
      encoded_collab_v1: encoded_collab.encode_to_bytes()?.into(),
      collab_type,
      updated_at,
    };
    collab_cache
      .insert_encode_collab_to_disk(&workspace_id, &uid, params)
      .await?;
    Ok(())
  }

  pub async fn snapshot_workspace(
    &self,
    workspace_id: WorkspaceId,
    mut up_to: Rid,
  ) -> anyhow::Result<()> {
    let all_object_updates = self
      .collab_cache
      .get_workspace_updates(&workspace_id, None, None, Some(up_to))
      .await?
      .into_iter()
      .into_group_map_by(|msg| msg.object_id);

    if all_object_updates.is_empty() {
      return Ok(());
    }

    let key = format!("af:lease:{}", workspace_id);
    // Acquire distributed lock for this workspace using Redis.
    // This ensures only one server can snapshot a workspace at a time, preventing:
    // - Race conditions between multiple servers
    // - Duplicate processing work
    // - Conflicts when pruning Redis streams
    //
    // The lock expires after 120 seconds if not released (fault tolerance).
    if let Some(_lease) = self
      .connection_manager
      .lease(key, Duration::from_secs(120))
      .await?
    {
      tracing::info!(
        "snapshotting {} collabs for workspace {}",
        all_object_updates.len(),
        workspace_id
      );

      // Prepare workspace for processing
      let mut snapshot_tasks = Vec::new();

      // 1: Collect all necessary data
      for (object_id, updates) in all_object_updates {
        if updates.is_empty() {
          continue;
        }

        let (collab_type, user_id) = {
          let update = &updates[0];
          (
            update.collab_type,
            update.sender.client_user_id().unwrap_or(0),
          )
        };

        // Fetch the snapshot from database
        let (rid_snapshot, update_snapshot) = self
          .get_snapshot(workspace_id, object_id, collab_type)
          .await?;

        snapshot_tasks.push((
          workspace_id,
          object_id,
          collab_type,
          user_id,
          rid_snapshot,
          update_snapshot,
          updates,
        ));
      }

      if snapshot_tasks.is_empty() {
        tracing::info!("no collabs to snapshot for workspace {}", workspace_id);
        return Ok(());
      }

      // 2: Process all collabs in parallel using the thread pool
      let thread_pool = self.snapshot_thread_pool.clone();
      let client_id = default_client_id();
      let processing_results = thread_pool.install(|| {
        snapshot_tasks
          .into_par_iter()
          .filter_map(
            |(
              workspace_id,
              object_id,
              collab_type,
              user_id,
              rid_snapshot,
              update_snapshot,
              updates,
            )| {
              match apply_updates_to_snapshot(
                client_id,
                object_id,
                collab_type,
                rid_snapshot,
                update_snapshot,
                updates,
              ) {
                Ok((rid, full_state, paragraphs)) => Some((
                  workspace_id,
                  object_id,
                  collab_type,
                  user_id,
                  rid,
                  full_state,
                  paragraphs,
                )),
                Err(err) => {
                  warn!("Failed to process collab {} snapshot: {}", object_id, err);
                  None
                },
              }
            },
          )
          .collect::<Vec<_>>()
      })?;

      // 3: Save snapshots and schedule indexing in batches
      const BATCH_SIZE: usize = 20;
      let mut indexed_collabs = vec![];
      for chunk in processing_results.chunks(BATCH_SIZE) {
        trace!(
          "processing batch of {} collabs when snapshotting workspace {}",
          chunk.len(),
          workspace_id
        );
        let mut join_set = JoinSet::new();
        for (workspace_id, object_id, collab_type, user_id, rid, full_state, paragraphs) in chunk {
          // Save snapshot asynchronously
          join_set.spawn(Self::save_snapshot(
            self.collab_cache.clone(),
            *workspace_id,
            *object_id,
            *collab_type,
            *user_id,
            *rid,
            full_state.clone(),
          ));

          // Schedule indexing if needed
          if !paragraphs.is_empty() {
            let indexed_collab = UnindexedCollabTask::new(
              *workspace_id,
              *object_id,
              *collab_type,
              UnindexedData::Paragraphs(paragraphs.clone()),
            );
            indexed_collabs.push(indexed_collab);
          }
        }

        trace!(
          "batch indexing {} collabs when snapshotting workspace{}",
          indexed_collabs.len(),
          workspace_id
        );
        if let Err(err) = self
          .indexer_scheduler
          .index_pending_collabs(std::mem::take(&mut indexed_collabs))
        {
          warn!("failed to batch index {}, err: {}", workspace_id, err);
        }

        // Wait for this batch to complete before processing the next
        while let Some(result) = join_set.join_next().await {
          result??;
        }
      }

      // drop the messages from redis stream
      up_to.seq_no += 1; // we want to include the last message in pruned updates
      self.prune_updates(workspace_id, up_to).await?;
    }
    Ok(())
  }
}

pub struct CollabState {
  pub rid: Rid,
  pub flags: UpdateFlags,
  pub update: Bytes,
  pub state_vector: Vec<u8>,
}

pub fn apply_updates_to_snapshot(
  client_id: ClientID,
  object_id: ObjectId,
  collab_type: CollabType,
  rid_snapshot: Rid,
  update_snapshot: Bytes,
  updates: Vec<UpdateStreamMessage>,
) -> anyhow::Result<(Rid, Bytes, Vec<String>)> {
  let options = CollabOptions::new(object_id.to_string(), client_id);
  let mut collab = Collab::new_with_options(CollabOrigin::Server, options)
    .map_err(|err| anyhow!("failed to create collab: {}", err))?;

  // Apply all updates to build the full state
  let mut rid = rid_snapshot;
  {
    let mut tx = collab.transact_mut();
    // First apply the snapshot
    if !update_snapshot.is_empty() {
      tx.apply_update(decode_update(&update_snapshot)?)?;
    }

    // Then apply updates from redis stream
    trace!(
      "processing {} updates for {}:{}",
      updates.len(),
      object_id,
      collab_type
    );

    for update in updates {
      if update.object_id == object_id {
        rid = rid.max(update.last_message_id);
        let update = match update.update_flags {
          UpdateFlags::Lib0v1 => Update::decode_v1(&update.update),
          UpdateFlags::Lib0v2 => Update::decode_v2(&update.update),
        }?;
        tx.apply_update(update)?;
      }
    }
    drop(tx); // commit the transaction
  }
  let full_state = collab.transact().encode_diff_v1(&StateVector::default());
  let paragraphs = if collab_type == CollabType::Document {
    let tx = collab.transact();
    DocumentBody::from_collab(&collab)
      .map(|body| body.to_plain_text(tx))
      .unwrap_or_default()
  } else {
    vec![]
  };

  Ok((rid, full_state.into(), paragraphs))
}

pub fn decode_update(update: &[u8]) -> AppResult<Update> {
  if update.len() < 2 {
    return Err(AppError::DecodeUpdateError("invalid update".to_string()));
  }
  let encoding_hint = update[0];
  let res = match encoding_hint {
    // lib0 v2 update always starts with 0 (but v1 can too on delete-only updates)
    0 => Update::decode_v2(update),
    _ => Update::decode_v1(update),
  };
  match res {
    Ok(res) => Ok(res),
    Err(_) if encoding_hint == 0 => {
      // we assumed that this was v2 encoded update, but it was not
      Update::decode_v1(update).map_err(|e| AppError::DecodeUpdateError(e.to_string()))
    },
    Err(e) => Err(AppError::DecodeUpdateError(e.to_string())),
  }
}
