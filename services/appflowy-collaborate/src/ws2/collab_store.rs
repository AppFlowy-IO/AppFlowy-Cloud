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
use redis::aio::ConnectionManager;
use redis::streams::{StreamTrimOptions, StreamTrimmingMode};
use redis::AsyncCommands;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use tracing::{instrument, trace, warn};
use uuid::Uuid;
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
}

impl CollabStore {
  /// Maximum number of concurrent snapshots that can be sent to S3 at the same time.
  const MAX_CONCURRENT_SNAPSHOTS: usize = 200;

  pub fn new<AC: CollabStorageAccessControl + Send + Sync + 'static>(
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
    })
  }

  pub fn updates(&self) -> &StreamRouter {
    &self.update_streams
  }

  pub fn awareness(&self) -> &AwarenessGossip {
    &self.awareness_broadcast
  }

  async fn get_snapshot(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
  ) -> AppResult<(Rid, Bytes)> {
    match self
      .collab_cache
      .get_encode_collab(&workspace_id, QueryCollab::new(object_id, collab_type))
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
    state_vector: &StateVector,
  ) -> AppResult<CollabState> {
    let params = QueryCollab::new(object_id, collab_type);
    self
      .access_control
      .enforce_read_collab(&workspace_id, &user_id, &object_id)
      .await?;

    let (rid, encoded_collab) = self
      .collab_cache
      .get_full_collab(&workspace_id, params, state_vector, EncoderVersion::V1)
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
    let uid = sender.client_user_id().unwrap_or(0);
    self
      .access_control
      .enforce_write_collab(&workspace_id, &uid, &object_id)
      .await?;

    let key = UpdateStreamMessage::stream_key(&workspace_id);
    let mut conn = self.connection_manager.clone();
    let items: String =
      UpdateStreamMessage::prepare_command(&key, &object_id, collab_type, sender, update)
        .query_async(&mut conn)
        .await?;
    let rid = Rid::from_str(&items).map_err(|err| anyhow!("failed to parse rid: {}", err))?;
    trace!(
      "publishing update to '{}' (object id: {}), rid:{}",
      key,
      object_id,
      rid
    );
    Ok(rid)
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
    use itertools::Itertools;
    let updates = self
      .collab_cache
      .get_workspace_updates(&workspace_id, None, None, Some(up_to))
      .await?
      .into_iter()
      .into_group_map_by(|msg| msg.object_id);

    if updates.is_empty() {
      return Ok(());
    }

    let key = format!("af:lease:{}", workspace_id);
    if let Some(_lease) = self
      .connection_manager
      .lease(key, Duration::from_secs(120))
      .await?
    {
      tracing::info!(
        "snapshotting {} collabs for workspace {}",
        updates.len(),
        workspace_id
      );
      //TODO: use rayon to parallelize the loop below?
      let mut join_set = JoinSet::new();
      let mut i = 0;
      for (object_id, updates) in updates {
        let (collab_type, user_id) = if updates.is_empty() {
          continue;
        } else {
          let update = &updates[0];
          (
            update.collab_type,
            update.sender.client_user_id().unwrap_or(0),
          )
        };
        let options = CollabOptions::new(object_id.to_string(), default_client_id());
        let mut collab = Collab::new_with_options(CollabOrigin::Server, options)
          .map_err(|err| anyhow!("failed to create collab: {}", err))?;
        // apply all known updates
        let rid = {
          let mut tx = collab.transact_mut();

          // apply latest snapshot
          let (mut rid, update) = self
            .get_snapshot(workspace_id, object_id, collab_type)
            .await?;
          tx.apply_update(decode_update(&update)?)?;

          // apply updates from redis stream targeting this collab
          tracing::trace!(
            "snapshotting {} updates for {}:{}",
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
          rid
        }; // commit the transaction
        let full_state = collab.transact().encode_diff_v1(&StateVector::default());
        let paragraphs = if collab_type == CollabType::Document {
          let tx = collab.transact();
          DocumentBody::from_collab(&collab)
            .map(|body| body.paragraphs(tx))
            .unwrap_or_default()
        } else {
          vec![]
        };

        join_set.spawn(Self::save_snapshot(
          self.collab_cache.clone(),
          workspace_id,
          object_id,
          collab_type,
          user_id,
          rid,
          full_state.into(),
        ));

        if !paragraphs.is_empty() {
          self.index_collab_content(workspace_id, object_id, collab_type, paragraphs);
        }

        i += 1;
        if i >= Self::MAX_CONCURRENT_SNAPSHOTS {
          // wait for some snapshots to finish before starting new ones
          while let Some(result) = join_set.join_next().await {
            result??;
          }
          i = 0;
        }
      }

      // consume remaining tasks
      while let Some(result) = join_set.join_next().await {
        result??;
      }

      // drop the messages from redis stream
      up_to.seq_no += 1; // we want to include the last message in pruned updates
      self.prune_updates(workspace_id, up_to).await?;
    }
    Ok(())
  }

  fn index_collab_content(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    paragraphs: Vec<String>,
  ) {
    tracing::debug!("scheduling {} {} for text indexing", collab_type, object_id);
    let indexed_collab = UnindexedCollabTask::new(
      workspace_id,
      object_id,
      collab_type,
      UnindexedData::Paragraphs(paragraphs),
    );
    if let Err(err) = self
      .indexer_scheduler
      .index_pending_collab_one(indexed_collab, false)
    {
      warn!(
        "failed to index collab `{}/{}`: {}",
        workspace_id, object_id, err
      );
    }
  }

  pub async fn prune_updates(&self, workspace_id: WorkspaceId, up_to: Rid) -> anyhow::Result<()> {
    let key = UpdateStreamMessage::stream_key(&workspace_id);
    let mut conn = self.connection_manager.clone();
    let options = StreamTrimOptions::minid(StreamTrimmingMode::Exact, up_to.to_string());
    let _: redis::Value = conn.xtrim_options(key, &options).await?;
    tracing::info!("pruned updates from workspace {}", workspace_id);
    Ok(())
  }
}

pub struct CollabState {
  pub rid: Rid,
  pub flags: UpdateFlags,
  pub update: Bytes,
  pub state_vector: Vec<u8>,
}

fn decode_update(update: &[u8]) -> AppResult<Update> {
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
