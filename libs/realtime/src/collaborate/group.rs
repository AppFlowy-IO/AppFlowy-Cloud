use crate::collaborate::{CollabBroadcast, CollabStoragePlugin, Subscription};
use crate::error::RealtimeError;
use bytes::{Bytes, BytesMut};
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_define::CollabType;
use collab_sync_protocol::CollabMessage;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use storage::collab::CollabStorage;

use crate::entities::RealtimeUser;
use tokio::task::spawn_blocking;
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

pub struct CollabGroupCache<S, U> {
  group_by_object_id: RwLock<HashMap<String, Arc<CollabGroup<U>>>>,
  storage: S,
}

impl<S, U> CollabGroupCache<S, U>
where
  S: CollabStorage + Clone,
  U: RealtimeUser,
{
  pub fn new(storage: S) -> Self {
    Self {
      group_by_object_id: RwLock::new(HashMap::new()),
      storage,
    }
  }

  pub async fn create_group(
    &self,
    uid: i64,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) {
    if self.group_by_object_id.read().contains_key(object_id) {
      return;
    }

    let group = self
      .init_group(uid, workspace_id, object_id, collab_type)
      .await;
    self
      .group_by_object_id
      .write()
      .insert(object_id.to_string(), group);
  }

  async fn init_group(
    &self,
    uid: i64,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Arc<CollabGroup<U>> {
    tracing::trace!("Create new group for object_id:{}", object_id);
    let collab = MutexCollab::new(CollabOrigin::Server, object_id, vec![]);
    let broadcast = CollabBroadcast::new(object_id, collab.clone(), 10);

    let collab = Arc::new(collab.clone());

    // The lifecycle of the collab is managed by the group.
    let group = Arc::new(CollabGroup {
      collab: collab.clone(),
      broadcast,
      subscribers: Default::default(),
    });

    let plugin = CollabStoragePlugin::new(
      uid,
      workspace_id,
      collab_type,
      self.storage.clone(),
      Arc::downgrade(&group),
    );
    collab.lock().add_plugin(Arc::new(plugin));
    collab.async_initialize().await;

    self
      .storage
      .cache_memory_collab(object_id, Arc::downgrade(&collab))
      .await;
    group
  }
}

impl<S, U> Deref for CollabGroupCache<S, U>
where
  S: CollabStorage,
  U: RealtimeUser,
{
  type Target = RwLock<HashMap<String, Arc<CollabGroup<U>>>>;

  fn deref(&self) -> &Self::Target {
    &self.group_by_object_id
  }
}

impl<S, U> DerefMut for CollabGroupCache<S, U>
where
  S: CollabStorage,
  U: RealtimeUser,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.group_by_object_id
  }
}

/// A group used to manage a single [Collab] object
pub struct CollabGroup<U> {
  pub collab: Arc<MutexCollab>,

  /// A broadcast used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
  /// to subscribes.
  pub broadcast: CollabBroadcast,

  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  pub subscribers: RwLock<HashMap<U, Subscription>>,
}

impl<U> CollabGroup<U>
where
  U: RealtimeUser,
{
  /// Mutate the [Collab] by the given closure
  pub fn get_mut_collab<F>(&self, f: F)
  where
    F: FnOnce(&Collab),
  {
    let collab = self.collab.lock();
    f(&collab);
  }

  pub fn is_empty(&self) -> bool {
    self.subscribers.read().is_empty()
  }

  /// Flush the [Collab] to the storage.
  /// When there is no subscriber, perform the flush in a blocking task.
  pub fn flush_collab(&self) {
    let collab = self.collab.clone();
    spawn_blocking(move || {
      collab.lock().flush();
    });
  }
}

#[derive(Debug, Default)]
pub struct CollabMsgCodec(LengthDelimitedCodec);

impl Encoder<CollabMessage> for CollabMsgCodec {
  type Error = RealtimeError;

  fn encode(&mut self, item: CollabMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
    let bytes = item.to_vec();
    self.0.encode(Bytes::from(bytes), dst)?;
    Ok(())
  }
}

impl Decoder for CollabMsgCodec {
  type Item = CollabMessage;
  type Error = RealtimeError;

  fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    if let Some(bytes) = self.0.decode(src)? {
      let bytes = bytes.freeze().to_vec();
      let msg = CollabMessage::from_vec(&bytes).ok();
      Ok(msg)
    } else {
      Ok(None)
    }
  }
}

// pub type CollabStream = FramedRead<OwnedReadHalf, CollabMsgCodec>;
// pub type CollabSink = FramedWrite<OwnedWriteHalf, CollabMsgCodec>;
