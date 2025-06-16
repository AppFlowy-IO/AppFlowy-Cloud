use crate::collab::cache::mem_cache::MillisSeconds;
use crate::collab::cache::CollabCache;
use anyhow::anyhow;
use appflowy_proto::{ObjectId, Rid, UpdateFlags, WorkspaceId};
use collab::core::origin::CollabOrigin;
use collab_entity::CollabType;
use collab_stream::model::UpdateStreamMessage;
use redis::aio::ConnectionManager;
use redis::streams::{StreamTrimOptions, StreamTrimmingMode};
use redis::AsyncCommands;
use std::str::FromStr;
use std::sync::Arc;
use tracing::trace;
use yrs::updates::decoder::Decode;

pub struct CollabUpdateWriter {
  connection_manager: ConnectionManager,
  collab_cache: Arc<CollabCache>,
}

impl CollabUpdateWriter {
  pub fn new(connection_manager: ConnectionManager, collab_cache: Arc<CollabCache>) -> Self {
    Self {
      connection_manager,
      collab_cache,
    }
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
    let items: String = UpdateStreamMessage::prepare_command(
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
    let rid = Rid::from_str(&items).map_err(|err| anyhow!("failed to parse rid: {}", err))?;
    trace!(
      "publishing update to '{}' (object id: {}), rid:{}",
      key,
      object_id,
      rid
    );
    Ok(rid)
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
