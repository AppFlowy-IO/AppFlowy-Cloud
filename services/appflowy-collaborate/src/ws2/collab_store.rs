use crate::collab::cache::CollabCache;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_proto::{ObjectId, Rid, UpdateFlags, WorkspaceId};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use collab_stream::awareness_gossip::AwarenessGossip;
use collab_stream::lease::Lease;
use collab_stream::model::{AwarenessStreamUpdate, UpdateStreamMessage};
use collab_stream::stream_router::{FromRedisStream, StreamRouter};
use database_entity::dto::{CollabParams, QueryCollab};
use redis::aio::ConnectionManager;
use redis::streams::{StreamRangeReply, StreamTrimOptions, StreamTrimmingMode};
use redis::{cmd, AsyncCommands, Client};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use yrs::sync::AwarenessUpdate;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{Doc, ReadTxn, StateVector, Transact, TransactionMut, Update};

pub struct CollabStore {
  collab_cache: Arc<CollabCache>,
  update_streams: Arc<StreamRouter>,
  awareness_broadcast: Arc<AwarenessGossip>,
  connection_manager: ConnectionManager,
}

impl CollabStore {
  /// IF updates are smaller than this threshold, we will not compute the diff from state vectors.
  /// Instead, we will return the updates as is.
  const UPDATE_DIFF_THRESHOLD: usize = 16 * 1024;

  const EMPTY_SNAPSHOT: &'static [u8] = &[
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 10 bytes of Rid (Redis Stream ID)
    0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, // empty lib0 v2 update
  ];

  /// Maximum number of concurrent snapshots that can be sent to S3 at the same time.
  const MAX_CONCURRENT_SNAPSHOTS: usize = 200;

  pub fn new(
    collab_cache: Arc<CollabCache>,
    connection_manager: ConnectionManager,
    update_streams: Arc<StreamRouter>,
    awareness_broadcast: Arc<AwarenessGossip>,
  ) -> Arc<Self> {
    Arc::new(Self {
      collab_cache,
      update_streams,
      awareness_broadcast,
      connection_manager,
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
  ) -> anyhow::Result<(Rid, Bytes)> {
    match self
      .collab_cache
      .get_encode_collab(&workspace_id, QueryCollab::new(object_id, collab_type))
      .await
    {
      Ok((rid, collab)) => Ok((rid, collab.doc_state)),
      Err(AppError::RecordNotFound(_)) => Ok((Rid::default(), Bytes::from_static(&[0, 0]))),
      Err(err) => Err(err.into()),
    }
  }

  /// Returns the latest full state of an object (including all updates).
  pub async fn get_latest_state(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    user_id: i64,
    state_vector: &StateVector,
  ) -> anyhow::Result<CollabState> {
    let (snapshot_rid, update) = self
      .get_snapshot(workspace_id, object_id, collab_type)
      .await?;
    let mut rid = snapshot_rid;
    tracing::trace!(
      "received snapshot for {}/{} (last message id: {})",
      workspace_id,
      object_id,
      rid
    );

    let doc = Doc::new();
    {
      let mut tx = doc.transact_mut();
      let update = Update::decode_v2(&update).or_else(|_| Update::decode_v1(&update))?;
      tx.apply_update(update)?;

      rid = self
        .replay_updates(&mut tx, workspace_id, object_id, rid)
        .await?;
    }
    let tx = doc.transact();
    let update: Bytes = tx.encode_state_as_update_v2(state_vector).into();
    let server_sv = if Self::has_missing_updates(&tx) {
      let sv = tx.state_vector().encode_v1();
      Some(sv)
    } else {
      None
    };

    if rid > snapshot_rid {
      tracing::trace!("document changed while replaying updates, saving it back");
      let full_state = if state_vector == &StateVector::default() {
        update.clone()
      } else {
        doc
          .transact()
          .encode_state_as_update_v2(&StateVector::default())
          .into()
      };
      tokio::spawn(Self::save_snapshot_task(
        self.collab_cache.clone(),
        self.connection_manager.clone(),
        workspace_id,
        object_id,
        collab_type,
        user_id,
        rid,
        full_state,
      ));
    }

    Ok(CollabState {
      rid,
      flags: UpdateFlags::Lib0v2,
      update,
      state_vector: server_sv,
    })
  }

  fn has_missing_updates<T: ReadTxn>(tx: &T) -> bool {
    let store = tx.store();
    store.pending_update().is_some() || store.pending_ds().is_some()
  }

  async fn replay_updates(
    &self,
    tx: &mut TransactionMut<'_>,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    snapshot_rid: Rid,
  ) -> anyhow::Result<Rid> {
    let updates = self
      .get_current_updates(workspace_id, Some(snapshot_rid), None)
      .await?;
    let mut last_rid = snapshot_rid;
    let mut i = 0;
    for message in updates.iter() {
      if message.object_id == object_id {
        let update = Update::decode_v1(&message.update)?;
        tx.apply_update(update)?;
        last_rid = message.last_message_id;
        i += 1;
      }
    }
    tracing::debug!(
      "replayed {} updates (out of {} total) for {}/{}",
      i,
      updates.len(),
      workspace_id,
      object_id
    );
    Ok(last_rid)
  }

  async fn get_current_updates(
    &self,
    workspace_id: WorkspaceId,
    from: Option<Rid>,
    to: Option<Rid>,
  ) -> anyhow::Result<Vec<UpdateStreamMessage>> {
    let key = UpdateStreamMessage::stream_key(&workspace_id);
    let from = from
      .map(|rid| rid.to_string())
      .unwrap_or_else(|| "-".into());
    let to = to.map(|rid| rid.to_string()).unwrap_or_else(|| "+".into());
    let mut conn = self.connection_manager.clone();
    let updates: StreamRangeReply = conn.xrange(key, from, to).await?;
    let mut result = Vec::with_capacity(updates.ids.len());
    for stream_id in updates.ids {
      let message = UpdateStreamMessage::from_redis_stream(&stream_id.id, &stream_id.map)?;
      result.push(message);
    }
    Ok(result)
  }

  pub async fn publish_update(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    sender: &CollabOrigin,
    update: Vec<u8>,
  ) -> anyhow::Result<Rid> {
    let key = UpdateStreamMessage::stream_key(&workspace_id);
    tracing::trace!("publishing update to {} - object id: {}", key, object_id);
    let mut conn = self.connection_manager.clone();
    let items: String =
      UpdateStreamMessage::prepare_command(&key, &object_id, collab_type, sender, update)
        .query_async(&mut conn)
        .await?;
    Rid::from_str(&items).map_err(|err| anyhow!("failed to parse rid: {}", err))
  }

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
    self
      .awareness_broadcast
      .send(&workspace_id.to_string(), &object_id.to_string(), &msg)
      .await?;
    Ok(())
  }

  #[allow(clippy::too_many_arguments)]
  async fn save_snapshot_task(
    collab_cache: Arc<CollabCache>,
    redis: ConnectionManager,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    uid: i64,
    last_message_id: Rid,
    update: Bytes,
  ) {
    let key = format!("af:lease:{}", workspace_id);
    if let Ok(Some(_lease)) = redis.lease(key, Duration::from_secs(60)).await {
      if let Err(err) = Self::save_snapshot(
        collab_cache,
        workspace_id,
        object_id,
        collab_type,
        uid,
        last_message_id,
        update,
      )
      .await
      {
        tracing::error!("failed to save snapshot: {}", err);
      }
    } else {
      tracing::info!("failed to acquire lease for {}", workspace_id);
    }
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
      .get_current_updates(workspace_id, None, Some(up_to))
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
        let doc = Doc::new();
        let mut tx = doc.transact_mut();
        let (mut rid, update) = self
          .get_snapshot(workspace_id, object_id, collab_type)
          .await?;
        tx.apply_update(Update::decode_v2(&update)?)?;

        for update in updates {
          rid = rid.max(update.last_message_id);
          let update = match update.update_flags {
            UpdateFlags::Lib0v1 => Update::decode_v1(&update.update),
            UpdateFlags::Lib0v2 => Update::decode_v2(&update.update),
          }?;
          tx.apply_update(update)?;
        }
        drop(tx); // commit the transaction
        let full_state = doc
          .transact()
          .encode_state_as_update_v2(&StateVector::default());

        join_set.spawn(Self::save_snapshot(
          self.collab_cache.clone(),
          workspace_id,
          object_id,
          collab_type,
          user_id,
          rid,
          full_state.into(),
        ));

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
  pub state_vector: Option<Vec<u8>>,
}
