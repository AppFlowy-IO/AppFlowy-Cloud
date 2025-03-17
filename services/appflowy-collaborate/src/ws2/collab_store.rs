use crate::collab::cache::CollabCache;
use anyhow::anyhow;
use appflowy_proto::{ObjectId, Rid, UpdateFlags, WorkspaceId};
use bytes::Bytes;
use collab_stream::stream_router::StreamRouter;
use redis::aio::ConnectionManager;
use redis::{cmd, AsyncCommands, Client};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinSet;
use yrs::block::ClientID;
use yrs::sync::AwarenessUpdate;
use yrs::updates::decoder::Decode;
use yrs::{Doc, ReadTxn, StateVector, Transact, TransactionMut, Update};

pub struct CollabStore {
  collab_cache: CollabCache,
  update_streams: StreamRouter,
  awareness_broadcast: AwarenessGossip,
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

  pub async fn new(
    s3: Operator,
    client: Client,
    metrics: Arc<CollabStreamMetrics>,
  ) -> crate::Result<Arc<Self>> {
    let connection_manager = ConnectionManager::new(client.clone()).await?;
    let update_streams = StreamRouter::new(&client, metrics.clone())?;
    let awareness_broadcast = AwarenessGossip::new(client);
    Ok(Arc::new(Self {
      s3,
      update_streams,
      awareness_broadcast,
      connection_manager,
    }))
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
  ) -> crate::Result<Option<Bytes>> {
    match self
      .s3
      .read(&format!("{}/{}/.state", workspace_id, object_id))
      .await
    {
      Ok(buffer) => Ok(Some(buffer.to_bytes())),
      Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
      Err(err) => Err(err.into()),
    }
  }

  /// Get the difference between the current state and the state at the given state vector.
  /// This WILL NOT include the updates that are not yet stored in the database.
  pub async fn get_snapshot_diff(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    state_vector: &StateVector,
    rid: Rid,
  ) -> crate::Result<CollabState> {
    let bytes = self
      .get_snapshot(workspace_id, object_id)
      .await?
      .unwrap_or_else(|| Bytes::from_static(Self::EMPTY_SNAPSHOT));
    let (rid_bytes, update) = bytes.split_at(10);
    let snapshot_rid = Rid::from_bytes(rid_bytes)?;

    // If the requested snapshot is newer than the current state, return an empty update.
    if snapshot_rid <= rid {
      tracing::trace!("the requested snapshot has no new data, returning an empty update");
      return Ok(CollabState {
        rid: snapshot_rid,
        flags: UpdateFlags::Lib0v1,
        update: Bytes::copy_from_slice(&[0, 0]), // empty lib0 v1 update
      });
    }

    // If state is fairly small, it's cheaper to return it as is instead of computing diff.
    if update.len() < Self::UPDATE_DIFF_THRESHOLD {
      tracing::trace!(
        "returning snapshot state as is (rid: {}, len: {})",
        snapshot_rid,
        update.len()
      );
      return Ok(CollabState {
        rid: snapshot_rid,
        flags: UpdateFlags::Lib0v2,
        update: Bytes::copy_from_slice(update),
      });
    }

    let doc = Doc::new();
    {
      let mut tx = doc.transact_mut();
      tx.apply_update(Update::decode_v2(update)?)?;
    }
    let update = doc.transact().encode_state_as_update_v2(&state_vector);
    tracing::trace!("returning snapshot state (rid: {})", rid);

    Ok(CollabState {
      rid,
      flags: UpdateFlags::Lib0v2,
      update: update.into(),
    })
  }

  /// Returns the latest full state of an object (including all updates).
  pub async fn get_latest_state(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    state_vector: &StateVector,
  ) -> crate::Result<CollabState> {
    let bytes = self
      .get_snapshot(workspace_id, object_id)
      .await?
      .unwrap_or_else(|| Bytes::from_static(Self::EMPTY_SNAPSHOT));
    let (rid_bytes, update) = bytes.split_at(10);
    let snapshot_rid = Rid::from_bytes(rid_bytes)?;
    let mut rid = snapshot_rid;

    let doc = Doc::new();
    {
      let mut tx = doc.transact_mut();
      tx.apply_update(Update::decode_v2(update)?)?;
      drop(bytes);

      rid = self
        .replay_updates(&mut tx, workspace_id, object_id, rid)
        .await?;
    }
    let update: Bytes = doc
      .transact()
      .encode_state_as_update_v2(&state_vector)
      .into();

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
        self.s3.clone(),
        self.connection_manager.clone(),
        workspace_id,
        object_id,
        rid,
        full_state,
      ));
    }

    Ok(CollabState {
      rid,
      flags: UpdateFlags::Lib0v2,
      update,
    })
  }

  async fn replay_updates(
    &self,
    tx: &mut TransactionMut<'_>,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    snapshot_rid: Rid,
  ) -> crate::Result<Rid> {
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
  ) -> crate::Result<Vec<UpdateStreamMessage>> {
    let key = format!("af:u:{}", workspace_id);
    let from = from
      .map(|rid| rid.to_string())
      .unwrap_or_else(|| "-".into());
    let to = to.map(|rid| rid.to_string()).unwrap_or_else(|| "+".into());
    let mut conn = self.connection_manager.clone();
    let updates: StreamRangeReply = conn.xrange(key, from, to).await?;
    let mut result = Vec::with_capacity(updates.ids.len());
    for stream_id in updates.ids {
      let map = stream_id.map;
      let id = stream_id.id;
      let message = UpdateStreamMessage::from_redis_stream(id, map)?;
      result.push(message);
    }
    Ok(result)
  }

  pub async fn publish_update(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    sender: ClientID,
    update: Vec<u8>,
  ) -> crate::Result<Rid> {
    let key = format!("af:u:{}", workspace_id);
    tracing::trace!("publishing update to {} - object id: {}", key, object_id);
    let mut conn = self.connection_manager.clone();
    let items: String = cmd("XADD")
      .arg(&key)
      .arg("*")
      .arg("oid")
      .arg(object_id)
      .arg("sender")
      .arg(sender)
      .arg("data")
      .arg(update)
      .query_async(&mut conn)
      .await?;
    Rid::from_str(&items).map_err(|err| anyhow!("failed to parse rid: {}", err))
  }

  pub async fn publish_awareness_update(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    sender: ClientID,
    update: AwarenessUpdate,
  ) -> crate::Result<()> {
    self
      .awareness_broadcast
      .patch_awareness_state(workspace_id, object_id, sender, update)
      .await
  }

  async fn save_snapshot_task(
    s3: Operator,
    redis: ConnectionManager,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    last_message_id: Rid,
    update: Bytes,
  ) {
    let key = format!("af:lease:{}", workspace_id);
    if let Ok(Some(_lease)) = redis.lease(key, Duration::from_secs(60)).await {
      if let Err(err) =
        Self::save_snapshot(s3, workspace_id, object_id, last_message_id, update).await
      {
        tracing::error!("failed to save snapshot: {}", err);
      }
    } else {
      tracing::info!("failed to acquire lease for {}", workspace_id);
    }
  }

  async fn save_snapshot(
    s3: Operator,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    last_message_id: Rid,
    update: Bytes,
  ) -> crate::Result<()> {
    let buf = Buffer::from(vec![
      Bytes::copy_from_slice(&last_message_id.into_bytes()),
      update,
    ]);
    let s3_key = format!("{}/{}/.state", workspace_id, object_id);
    s3.write(&s3_key, buf).await?;
    Ok(())
  }

  pub async fn snapshot_workspace(
    &self,
    workspace_id: WorkspaceId,
    mut up_to: Rid,
  ) -> crate::Result<()> {
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
        let doc = Doc::new();
        let mut tx = doc.transact_mut();
        let mut rid = Rid::default();
        if let Some(bytes) = self.get_snapshot(workspace_id, object_id).await? {
          let (rid_bytes, update) = bytes.split_at(10);
          rid = Rid::from_bytes(rid_bytes)?;
          tx.apply_update(Update::decode_v2(&update)?)?;
        }
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
          self.s3.clone(),
          workspace_id,
          object_id,
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

  pub async fn prune_updates(&self, workspace_id: WorkspaceId, up_to: Rid) -> crate::Result<()> {
    let key = format!("af:u:{}", workspace_id);
    let mut conn = self.connection_manager.clone();
    let options = StreamTrimOptions::minid(StreamTrimmingMode::Exact, up_to.to_string());
    conn.xtrim_options(key, &options).await?;
    tracing::info!("pruned updates from workspace {}", workspace_id);
    Ok(())
  }
}

pub struct CollabState {
  pub rid: Rid,
  pub flags: UpdateFlags,
  pub update: Bytes,
}

#[cfg(test)]
mod test {
  use crate::collab_store::CollabStore;
  use crate::metrics::CollabStreamMetrics;
  use af_proto::{ObjectId, Rid, WorkspaceId};
  use opendal::services::S3;
  use yrs::updates::decoder::Decode;
  use yrs::{Doc, GetString, Text, Transact, WriteTxn};

  #[tokio::test]
  async fn snapshot_compression() {
    let s3 = S3::default()
      .bucket("appflowy")
      .region("eu-west-1")
      .endpoint("http://localhost:9000")
      .secret_access_key("minioadmin")
      .access_key_id("minioadmin");
    let operator = opendal::Operator::new(s3).unwrap().finish();
    let store = CollabStore::new(
      operator,
      redis::Client::open("redis://localhost:6379").unwrap(),
      CollabStreamMetrics::default().into(),
    )
    .await
    .unwrap();
    let workspace_id: WorkspaceId = WorkspaceId::new_v4();
    let mut objects: Vec<_> = (0..10)
      .into_iter()
      .map(|_| (ObjectId::new_v4(), Rid::default()))
      .collect();

    // create initial state of the updates in Redis
    let mut last_rid = Rid::default();
    for (object_id, rid) in objects.iter_mut() {
      let doc = Doc::new();
      let txt = doc.get_or_insert_text("test");
      for c in "Hello, World!".chars() {
        let mut tx = doc.transact_mut();
        txt.push(&mut tx, &c.to_string());
        let update = tx.encode_update_v1();
        *rid = store
          .publish_update(workspace_id, *object_id, 0, update)
          .await
          .unwrap();
        last_rid = *rid;
      }
    }

    // snapshot the workspace
    store
      .snapshot_workspace(workspace_id, last_rid)
      .await
      .unwrap();

    // check the state of s3
    for (object_id, _) in objects {
      let data = store
        .get_snapshot(workspace_id, object_id)
        .await
        .unwrap()
        .unwrap();
      let doc = Doc::new();
      let mut tx = doc.transact_mut();
      let txt = tx.get_or_insert_text("test");
      tx.apply_update(yrs::Update::decode_v2(&data[10..]).unwrap())
        .unwrap();
      assert_eq!(txt.get_string(&tx), "Hello, World!");
    }

    let updates = store
      .get_current_updates(workspace_id, None, None)
      .await
      .unwrap();
    assert_eq!(updates, vec![], "Redis stream should be empty");
  }
}
