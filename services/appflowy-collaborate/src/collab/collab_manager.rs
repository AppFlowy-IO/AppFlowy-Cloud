use crate::collab::cache::mem_cache::MillisSeconds;
use crate::collab::cache::CollabCache;
use access_control::act::Action;
use access_control::collab::CollabAccessControl;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_proto::{ObjectId, Rid, TimestampedEncodedCollab, UpdateFlags, WorkspaceId};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use collab::core::collab::{default_client_id, CollabOptions};
use collab::core::origin::CollabOrigin;
use collab::entity::{EncodedCollab, EncoderVersion};
use collab::preclude::Collab;
use collab_document::document::DocumentBody;
use collab_entity::CollabType;
use collab_folder::Folder;
use collab_stream::awareness_gossip::AwarenessGossip;
use collab_stream::lease::Lease;
use collab_stream::model::{AwarenessStreamUpdate, MessageId, UpdateStreamMessage};
use collab_stream::stream_router::StreamRouter;
use database::collab::AppResult;
use database_entity::dto::{CollabParams, CollabUpdateData, QueryCollab};
use indexer::scheduler::{IndexerScheduler, UnindexedCollabTask, UnindexedData};
use infra::thread_pool::ThreadPoolNoAbort;
use itertools::Itertools;
use rayon::prelude::*;
use redis::aio::ConnectionManager;
use redis::streams::{StreamTrimOptions, StreamTrimmingMode};
use redis::AsyncCommands;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
/// Represents a snapshot task to be processed
use tracing::{instrument, trace, warn};
use uuid::Uuid;
use yrs::block::ClientID;
use yrs::sync::AwarenessUpdate;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{ReadTxn, StateVector, Update};

pub struct CollabManager {
  collab_cache: Arc<CollabCache>,
  access_control: Arc<dyn CollabAccessControl>,
  update_streams: Arc<StreamRouter>,
  awareness_broadcast: Arc<AwarenessGossip>,
  connection_manager: ConnectionManager,
  indexer_scheduler: Arc<IndexerScheduler>,
  snapshot_thread_pool: Arc<ThreadPoolNoAbort>,
}

impl CollabManager {
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    thread_pool: Arc<ThreadPoolNoAbort>,
    access_control: Arc<dyn CollabAccessControl>,
    collab_cache: Arc<CollabCache>,
    connection_manager: ConnectionManager,
    update_streams: Arc<StreamRouter>,
    awareness_broadcast: Arc<AwarenessGossip>,
    indexer_scheduler: Arc<IndexerScheduler>,
  ) -> Arc<Self> {
    Arc::new(Self {
      access_control,
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

  async fn prune_updates(&self, workspace_id: WorkspaceId, up_to: Rid) -> anyhow::Result<()> {
    let key = UpdateStreamMessage::stream_key(&workspace_id);
    let mut conn = self.connection_manager.clone();
    let options = StreamTrimOptions::minid(StreamTrimmingMode::Exact, up_to.to_string());
    let _: redis::Value = conn.xtrim_options(key, &options).await?;
    tracing::info!("pruned updates from workspace {}", workspace_id);
    Ok(())
  }

  pub async fn build_folder(&self, workspace_id: WorkspaceId) -> AppResult<Folder> {
    let query = QueryCollab::new(workspace_id, CollabType::Folder);
    let encoded_collab = self
      .collab_cache
      .get_full_collab(&workspace_id, query, None, EncoderVersion::V1)
      .await?
      .encoded_collab;

    let folder = tokio::task::spawn_blocking(move || {
      Folder::from_collab_doc_state(
        CollabOrigin::Server,
        encoded_collab.into(),
        &workspace_id.to_string(),
        default_client_id(),
      )
      .map_err(|e| {
        AppError::Internal(anyhow::anyhow!(
          "Unable to decode workspace folder {}: {}",
          workspace_id,
          e
        ))
      })
    })
    .await??;
    Ok(folder)
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
    let collab_exists = self.collab_cache.is_exist(workspace_id, object_id).await?;
    if !collab_exists {
      // If the collab does not exist, we should not enforce the access control. We consider the user
      // has the permission to read the collab
      return Ok(());
    }
    self
      .access_control
      .enforce_action(workspace_id, uid, object_id, Action::Read)
      .await
  }

  pub async fn enforce_write_collab(
    &self,
    workspace_id: &WorkspaceId,
    uid: &i64,
    object_id: &ObjectId,
  ) -> AppResult<()> {
    let collab_exists = self.collab_cache.is_exist(workspace_id, object_id).await?;
    if !collab_exists {
      // If the collab does not exist, we should not enforce the access control. we consider the user
      // has the permission to write the collab
      return Ok(());
    }
    self
      .access_control
      .enforce_action(workspace_id, uid, object_id, Action::Write)
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
  #[instrument(level = "trace", skip_all)]
  pub async fn get_latest_state(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    user_id: i64,
    state_vector: StateVector,
  ) -> AppResult<CollabState> {
    let params = QueryCollab::new(object_id, collab_type);
    let collab_exists = self
      .collab_cache
      .is_exist(&workspace_id, &object_id)
      .await?;
    if collab_exists {
      self
        .access_control
        .enforce_action(&workspace_id, &user_id, &object_id, Action::Read)
        .await?;
    }

    let TimestampedEncodedCollab {
      encoded_collab,
      rid,
    } = self
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
    let rid = Rid::from_str(&rid).map_err(|err| anyhow!("failed to parse rid: {}", err))?;
    self
      .collab_cache
      .mark_as_dirty(object_id, MillisSeconds::from(rid.timestamp));
    trace!(
      "published update to '{}' (object id: {}), rid:{}",
      key,
      object_id,
      rid
    );
    Ok(rid)
  }

  pub fn mark_as_dirty(&self, object_id: ObjectId, millis_secs: u64) {
    self
      .collab_cache
      .mark_as_dirty(object_id, MillisSeconds::from(millis_secs));
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

  /// Snapshots all pending updates for a workspace into persistent storage.
  ///
  /// This operation:
  /// - Acquires a distributed lock to prevent concurrent snapshots
  /// - Processes collabs in parallel using the thread pool
  /// - Groups and batch-inserts snapshots by user ID
  /// - Schedules updated documents for search indexing
  /// - Prunes processed updates from Redis streams
  #[instrument(level = "trace", skip_all)]
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
      trace!(
        "no updates to snapshot for workspace {} up to rid {}",
        workspace_id,
        up_to
      );
      return Ok(());
    }

    if let Some(_lease) = self.acquire_workspace_lock(workspace_id).await? {
      let snapshot_start = std::time::Instant::now();

      // Count total updates for metrics
      let total_updates: usize = all_object_updates
        .values()
        .map(|updates| updates.len())
        .sum();

      tracing::debug!(
        "snapshotting {} collabs ({} total updates) for workspace {}",
        all_object_updates.len(),
        total_updates,
        workspace_id
      );

      let snapshot_tasks = self
        .collect_snapshot_tasks(workspace_id, all_object_updates)
        .await?;
      if snapshot_tasks.is_empty() {
        tracing::info!("no collabs to snapshot for workspace {}", workspace_id);
        return Ok(());
      }

      let processing_results = self.process_snapshot_tasks(snapshot_tasks)?;
      self
        .encode_and_save_snapshots(workspace_id, processing_results)
        .await?;

      // Cleanup: prune processed updates from Redis stream
      up_to.seq_no += 1;
      self.prune_updates(workspace_id, up_to).await?;

      // Record metrics for the completed snapshot operation
      let snapshot_duration = snapshot_start.elapsed();
      self
        .collab_cache
        .metrics()
        .observe_snapshot_duration(snapshot_duration);
      self
        .collab_cache
        .metrics()
        .observe_snapshot_updates_count(total_updates);

      tracing::debug!(
        "completed snapshot for workspace {} in {:?} (processed {} updates)",
        workspace_id,
        snapshot_duration,
        total_updates
      );
    }
    Ok(())
  }

  /// Acquires a distributed lock for workspace snapshotting
  async fn acquire_workspace_lock(
    &self,
    workspace_id: WorkspaceId,
  ) -> anyhow::Result<Option<impl Drop>> {
    let key = format!("af:lease:{}", workspace_id);
    // Acquire distributed lock for this workspace using Redis.
    // This ensures only one server can snapshot a workspace at a time, preventing:
    // - Race conditions between multiple servers
    // - Duplicate processing work
    // - Conflicts when pruning Redis streams
    //
    // The lock expires after 120 seconds if not released (fault tolerance).
    self
      .connection_manager
      .lease(key, Duration::from_secs(120))
      .await
      .map_err(|e| anyhow!("Failed to acquire workspace lock: {}", e))
  }

  /// Collects all snapshot tasks for the workspace
  async fn collect_snapshot_tasks(
    &self,
    workspace_id: WorkspaceId,
    all_object_updates: HashMap<Uuid, Vec<UpdateStreamMessage>>,
  ) -> anyhow::Result<Vec<SnapshotTask>> {
    let mut snapshot_tasks = Vec::new();
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
      let (rid, update_snapshot) = self
        .get_snapshot(workspace_id, object_id, collab_type)
        .await?;

      snapshot_tasks.push(SnapshotTask {
        workspace_id,
        object_id,
        collab_type,
        user_id,
        rid,
        update_snapshot,
        updates,
      });
    }

    Ok(snapshot_tasks)
  }

  /// Processes snapshot tasks in parallel to generate full states
  fn process_snapshot_tasks(
    &self,
    snapshot_tasks: Vec<SnapshotTask>,
  ) -> anyhow::Result<Vec<ProcessedSnapshot>> {
    let thread_pool = self.snapshot_thread_pool.clone();
    let client_id = default_client_id();

    thread_pool
      .install(|| {
        snapshot_tasks
          .into_par_iter()
          .filter_map(|task| {
            match apply_updates_to_snapshot(
              client_id,
              task.object_id,
              task.collab_type,
              task.rid,
              task.update_snapshot,
              task.updates,
            ) {
              Ok((rid, full_state, state_vector, paragraphs)) => Some(ProcessedSnapshot {
                workspace_id: task.workspace_id,
                object_id: task.object_id,
                collab_type: task.collab_type,
                user_id: task.user_id,
                rid,
                full_state,
                state_vector: state_vector.encode_v1().into(),
                paragraphs,
              }),
              Err(err) => {
                warn!(
                  "Failed to process collab {} snapshot: {}",
                  task.object_id, err
                );
                None
              },
            }
          })
          .collect::<Vec<_>>()
      })
      .map_err(|err| anyhow!("Thread pool panic during snapshot processing: {}", err))
  }

  /// Encodes collabs in parallel and saves them in batches grouped by user ID
  async fn encode_and_save_snapshots(
    &self,
    workspace_id: WorkspaceId,
    processing_results: Vec<ProcessedSnapshot>,
  ) -> anyhow::Result<()> {
    const BATCH_SIZE: usize = 20;
    let mut indexed_collabs = vec![];

    for chunk in processing_results.chunks(BATCH_SIZE) {
      trace!(
        "processing batch of {} collabs when snapshotting workspace {}",
        chunk.len(),
        workspace_id
      );

      let encoded_result = self.encode_collab_chunk(chunk)?;

      // Collect indexing tasks
      for task in encoded_result.indexing_tasks {
        indexed_collabs.push(task);
      }

      // Batch insert snapshots for each user
      self
        .batch_insert_by_user(workspace_id, encoded_result.params_by_uid)
        .await;

      // Batch index collabs periodically
      if !indexed_collabs.is_empty() {
        self.batch_index_collabs(workspace_id, &mut indexed_collabs);
      }
    }

    Ok(())
  }

  /// Encodes a chunk of collabs in parallel
  fn encode_collab_chunk(&self, chunk: &[ProcessedSnapshot]) -> anyhow::Result<EncodedChunkResult> {
    // Prepare data for parallel encoding
    let encode_data: Vec<_> = chunk
      .iter()
      .map(|snapshot| {
        (
          snapshot.workspace_id,
          snapshot.object_id,
          snapshot.collab_type,
          snapshot.user_id,
          snapshot.rid,
          snapshot.full_state.clone(),
          snapshot.state_vector.clone(),
          snapshot.paragraphs.clone(),
        )
      })
      .collect();

    // Use thread pool to encode all collabs in parallel
    let batch_results = self
      .snapshot_thread_pool
      .install(|| {
        encode_data
          .into_par_iter()
          .map(
            |(ws_id, object_id, collab_type, uid, rid, full_state, state_vector, paragraphs)| {
              // Create EncodedCollab and encode to bytes in parallel
              let encoded_collab = EncodedCollab::new_v1(state_vector, full_state);
              let updated_at = DateTime::<Utc>::from_timestamp_millis(rid.timestamp as i64);
              let encoded_bytes = encoded_collab
                .encode_to_bytes()
                .map_err(|e| anyhow!("Failed to encode collab {}: {}", object_id, e))?;

              let params = CollabParams {
                object_id,
                encoded_collab_v1: encoded_bytes.into(),
                collab_type,
                updated_at,
              };

              // Return both params and indexing data
              let indexing_data = if !paragraphs.is_empty() {
                Some(UnindexedCollabTask::new(
                  ws_id,
                  object_id,
                  collab_type,
                  UnindexedData::Paragraphs(paragraphs),
                ))
              } else {
                None
              };

              Ok(((uid, params), indexing_data))
            },
          )
          .collect::<Result<Vec<_>, anyhow::Error>>()
      })
      .map_err(|err| anyhow!("Thread pool panic during encoding: {}", err))??;

    // Separate params with uid and indexing data
    let (params_with_uid, indexing_tasks): (Vec<_>, Vec<_>) = batch_results.into_iter().unzip();

    // Group batch params by uid
    let mut batch_params_by_uid: HashMap<i64, Vec<CollabParams>> = HashMap::new();
    for (uid, params) in params_with_uid {
      batch_params_by_uid.entry(uid).or_default().push(params);
    }

    // Collect non-None indexing tasks
    let indexing_tasks: Vec<UnindexedCollabTask> = indexing_tasks.into_iter().flatten().collect();
    Ok(EncodedChunkResult {
      params_by_uid: batch_params_by_uid,
      indexing_tasks,
    })
  }

  /// Batch inserts snapshots grouped by user ID
  async fn batch_insert_by_user(
    &self,
    workspace_id: WorkspaceId,
    batch_params_by_uid: HashMap<i64, Vec<CollabParams>>,
  ) {
    for (uid, batch_params) in batch_params_by_uid {
      if !batch_params.is_empty() {
        let batch_len = batch_params.len();
        if let Err(err) = self
          .collab_cache
          .bulk_insert_collab(workspace_id, &uid, batch_params)
          .await
        {
          warn!(
            "Failed to batch insert {} snapshots for user {}: {}",
            batch_len, uid, err
          );
        } else {
          trace!(
            "Successfully batch inserted {} snapshots for user {}",
            batch_len,
            uid
          );
        }
      }
    }
  }

  /// Batch indexes collabs for search
  fn batch_index_collabs(
    &self,
    workspace_id: WorkspaceId,
    indexed_collabs: &mut Vec<UnindexedCollabTask>,
  ) {
    if !indexed_collabs.is_empty() {
      trace!(
        "batch indexing {} collabs when snapshotting workspace {}",
        indexed_collabs.len(),
        workspace_id
      );
      if let Err(err) = self
        .indexer_scheduler
        .index_pending_collabs(std::mem::take(indexed_collabs))
      {
        warn!("failed to batch index {}, err: {}", workspace_id, err);
      }
    }
  }
}

pub struct CollabState {
  pub rid: Rid,
  pub flags: UpdateFlags,
  pub update: Bytes,
  pub state_vector: Vec<u8>,
}

fn apply_updates_to_snapshot(
  client_id: ClientID,
  object_id: ObjectId,
  collab_type: CollabType,
  rid_snapshot: Rid,
  update_snapshot: Bytes,
  updates: Vec<UpdateStreamMessage>,
) -> anyhow::Result<(Rid, Bytes, StateVector, Vec<String>)> {
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
  let tx = collab.transact();
  let full_state = tx.encode_diff_v1(&StateVector::default());
  let state_vector = tx.state_vector();
  let paragraphs = if collab_type == CollabType::Document {
    DocumentBody::from_collab(&collab)
      .map(|body| body.to_plain_text(tx))
      .unwrap_or_default()
  } else {
    vec![]
  };

  Ok((rid, full_state.into(), state_vector, paragraphs))
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

struct SnapshotTask {
  workspace_id: WorkspaceId,
  object_id: ObjectId,
  collab_type: CollabType,
  user_id: i64,
  rid: Rid,
  update_snapshot: Bytes,
  updates: Vec<UpdateStreamMessage>,
}

struct ProcessedSnapshot {
  workspace_id: WorkspaceId,
  object_id: ObjectId,
  collab_type: CollabType,
  user_id: i64,
  rid: Rid,
  full_state: Bytes,
  state_vector: Bytes,
  paragraphs: Vec<String>,
}
struct EncodedChunkResult {
  /// Collab parameters grouped by user ID for batch insertion
  params_by_uid: HashMap<i64, Vec<CollabParams>>,
  /// Indexing tasks for search engine updates
  indexing_tasks: Vec<UnindexedCollabTask>,
}
