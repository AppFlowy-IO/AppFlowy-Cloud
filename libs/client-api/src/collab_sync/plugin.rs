use collab::core::awareness::{AwarenessUpdate, Event};
use std::sync::{Arc, Weak};

use collab::core::collab::MutexCollab;
use collab::core::collab_state::SyncState;
use collab::core::origin::CollabOrigin;
use collab::preclude::{Collab, CollabPlugin};
use collab_entity::{CollabObject, CollabType};
use collab_rt_entity::collab_msg::{ClientCollabMessage, ServerCollabMessage, UpdateSync};
use collab_rt_protocol::{Message, SyncMessage};
use futures_util::SinkExt;
use tokio_stream::StreamExt;

use crate::collab_sync::SyncControl;
use tokio_stream::wrappers::WatchStream;
use tracing::trace;

use crate::af_spawn;
use crate::collab_sync::sink_config::SinkConfig;
use crate::ws::{ConnectState, WSConnectStateReceiver};
use yrs::updates::encoder::Encode;

pub struct SyncPlugin<Sink, Stream, C> {
  object: SyncObject,
  sync_queue: Arc<SyncControl<Sink, Stream>>,
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
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<ServerCollabMessage, E>> + Send + Sync + Unpin + 'static,
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
    let sync_queue = SyncControl::new(
      object.clone(),
      origin,
      sink,
      sink_config,
      stream,
      collab.clone(),
      pause,
    );

    let mut sync_state_stream = WatchStream::new(sync_queue.subscribe_sync_state());
    af_spawn(async move {
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
    af_spawn(async move {
      while let Ok(connect_state) = ws_connect_state.recv().await {
        match connect_state {
          ConnectState::Connected => {
            // If the websocket is connected, initialize a new init sync
            if let (Some(local_collab), Some(sync_queue)) =
              (weak_local_collab.upgrade(), weak_sync_queue.upgrade())
            {
              if let Some(local_collab) = local_collab.try_lock() {
                sync_queue.resume();
                sync_queue.init_sync(&local_collab);
              }
            } else {
              break;
            }
          },
          ConnectState::Unauthorized | ConnectState::Lost => {
            if let Some(sync_queue) = weak_sync_queue.upgrade() {
              // Stop sync if the websocket is unauthorized or disconnected
              sync_queue.pause();
            } else {
              break;
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
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<ServerCollabMessage, E>> + Send + Sync + Unpin + 'static,
  C: Send + Sync + 'static,
{
  fn did_init(&self, collab: &Collab, _object_id: &str, _last_sync_at: i64) {
    self.sync_queue.init_sync(collab);
  }

  fn receive_local_update(&self, origin: &CollabOrigin, _object_id: &str, update: &[u8]) {
    let update = update.to_vec();
    let payload = Message::Sync(SyncMessage::Update(update)).encode_v1();
    self.sync_queue.queue_msg(|msg_id| {
      let update_sync = UpdateSync::new(
        origin.clone(),
        self.object.object_id.clone(),
        payload,
        msg_id,
      );
      ClientCollabMessage::new_update_sync(update_sync)
    });
  }

  fn receive_local_state(
    &self,
    origin: &CollabOrigin,
    object_id: &str,
    _event: &Event,
    update: &AwarenessUpdate,
  ) {
    let payload = Message::Awareness(update.clone()).encode_v1();
    self.sync_queue.queue_msg(|msg_id| {
      let update_sync = UpdateSync::new(origin.clone(), object_id.to_string(), payload, msg_id);
      trace!("queue local state: {}", update_sync);
      ClientCollabMessage::new_update_sync(update_sync)
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
