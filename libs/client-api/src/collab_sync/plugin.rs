use std::sync::{Arc, Weak};

use collab::core::collab::MutexCollab;
use collab::core::collab_state::SyncState;
use collab::core::origin::CollabOrigin;
use collab::preclude::CollabPlugin;
use collab::sync_protocol::awareness::Awareness;
use collab::sync_protocol::message::{Message, SyncMessage};
use collab_entity::{CollabObject, CollabType};
use futures_util::SinkExt;
use realtime_entity::collab_msg::{CollabMessage, UpdateSync};
use tokio_stream::StreamExt;

use crate::collab_sync::{SinkConfig, SyncQueue};
use crate::ws::{ConnectState, WSConnectStateReceiver};
use tokio_stream::wrappers::WatchStream;
use tracing::trace;

use yrs::updates::encoder::Encode;

pub struct SyncPlugin<Sink, Stream, C> {
  object: SyncObject,
  sync_queue: Arc<SyncQueue<Sink, Stream>>,
  // Used to keep the lifetime of the channel
  #[allow(dead_code)]
  channel: Option<Arc<C>>,
}

impl<Sink, Stream, C> Drop for SyncPlugin<Sink, Stream, C> {
  fn drop(&mut self) {
    trace!("Drop sync plugin: {}", self.object.object_id);
  }
}

impl<E, Sink, Stream, C> SyncPlugin<Sink, Stream, C>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<CollabMessage, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<CollabMessage, E>> + Send + Sync + Unpin + 'static,
  C: Send + Sync + 'static,
{
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    origin: CollabOrigin,
    object: SyncObject,
    collab: Weak<MutexCollab>,
    sink: Sink,
    sink_config: SinkConfig,
    stream: Stream,
    channel: Option<Arc<C>>,
    pause: bool,
    mut ws_connect_state: WSConnectStateReceiver,
  ) -> Self {
    let weak_local_collab = collab.clone();
    let sync_queue = SyncQueue::new(
      object.clone(),
      origin,
      sink,
      stream,
      collab.clone(),
      sink_config,
      pause,
    );

    let mut sync_state_stream = WatchStream::new(sync_queue.subscribe_sync_state());
    tokio::spawn(async move {
      while let Some(new_state) = sync_state_stream.next().await {
        if let Some(local_collab) = weak_local_collab.upgrade() {
          if let Some(local_collab) = local_collab.try_lock() {
            local_collab.set_sync_state(new_state);
          }
        }
      }
    });

    let sync_queue = Arc::new(sync_queue);
    let weak_local_collab = collab;
    let weak_sync_queue = Arc::downgrade(&sync_queue);
    tokio::spawn(async move {
      while let Ok(connect_state) = ws_connect_state.recv().await {
        match connect_state {
          ConnectState::Connected => {
            // If the websocket is connected, initialize a new init sync
            if let (Some(local_collab), Some(sync_queue)) =
              (weak_local_collab.upgrade(), weak_sync_queue.upgrade())
            {
              if let Some(local_collab) = local_collab.try_lock() {
                sync_queue.resume();
                sync_queue.init_sync(local_collab.get_awareness());
              }
            }
          },
          ConnectState::Unauthorized | ConnectState::Closed => {
            if let Some(sync_queue) = weak_sync_queue.upgrade() {
              // Stop sync if the websocket is unauthorized or disconnected
              sync_queue.pause();
            }
          },
          _ => {},
        }
      }
    });

    Self {
      sync_queue,
      object,
      channel,
    }
  }

  pub fn subscribe_sync_state(&self) -> WatchStream<SyncState> {
    let rx = self.sync_queue.subscribe_sync_state();
    WatchStream::new(rx)
  }
}

impl<E, Sink, Stream, C> CollabPlugin for SyncPlugin<Sink, Stream, C>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<CollabMessage, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<CollabMessage, E>> + Send + Sync + Unpin + 'static,
  C: Send + Sync + 'static,
{
  fn did_init(&self, _awareness: &Awareness, _object_id: &str) {
    self.sync_queue.init_sync(_awareness);
  }

  fn receive_local_update(&self, origin: &CollabOrigin, _object_id: &str, update: &[u8]) {
    let weak_sync_queue = Arc::downgrade(&self.sync_queue);
    let update = update.to_vec();
    let object_id = self.object.object_id.clone();
    let cloned_origin = origin.clone();

    tokio::spawn(async move {
      if let Some(sync_queue) = weak_sync_queue.upgrade() {
        let payload = Message::Sync(SyncMessage::Update(update)).encode_v1();
        sync_queue
          .queue_msg(|msg_id| UpdateSync::new(cloned_origin, object_id, payload, msg_id).into());
      }
    });
  }

  fn reset(&self, _object_id: &str) {
    self.sync_queue.clear();
  }
}

#[derive(Clone, Debug)]
pub struct SyncObject {
  pub object_id: String,
  pub workspace_id: String,
  pub collab_type: CollabType,
  pub device_id: String,
}

impl SyncObject {
  pub fn new(
    object_id: &str,
    workspace_id: &str,
    collab_type: CollabType,
    device_id: &str,
  ) -> Self {
    Self {
      object_id: object_id.to_string(),
      workspace_id: workspace_id.to_string(),
      collab_type,
      device_id: device_id.to_string(),
    }
  }
}

impl From<CollabObject> for SyncObject {
  fn from(collab_object: CollabObject) -> Self {
    Self {
      object_id: collab_object.object_id,
      workspace_id: collab_object.workspace_id,
      collab_type: collab_object.collab_type,
      device_id: collab_object.device_id,
    }
  }
}
