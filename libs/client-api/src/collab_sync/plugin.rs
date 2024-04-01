use collab::core::awareness::{AwarenessUpdate, Event};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::collab_sync::{InitSyncReason, SinkConfig, SinkState, SyncControl};
use collab::core::collab::MutexCollab;
use collab::core::collab_state::SyncState;
use collab::core::origin::CollabOrigin;
use collab::preclude::{Collab, CollabPlugin};
use collab_entity::{CollabObject, CollabType};
use collab_rt_entity::{ClientCollabMessage, ServerCollabMessage, UpdateSync};
use collab_rt_protocol::{Message, SyncMessage};
use futures_util::SinkExt;
use tokio::time::sleep;
use tokio_stream::StreamExt;
use tracing::trace;

use crate::af_spawn;
use crate::ws::{ConnectState, WSConnectStateReceiver};
use yrs::updates::encoder::Encode;

pub struct SyncPlugin<Sink, Stream, C> {
  object: SyncObject,
  sync_queue: Arc<SyncControl<Sink, Stream>>,
  // Used to keep the lifetime of the channel
  #[allow(dead_code)]
  channel: Option<Arc<C>>,
  collab: Weak<MutexCollab>,
  did_init_sync: Arc<AtomicBool>,
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
    let sync_queue = SyncControl::new(
      object.clone(),
      origin,
      sink,
      sink_config,
      stream,
      collab.clone(),
      pause,
    );

    if let Some(local_collab) = collab.upgrade() {
      let mut sync_state_stream = sync_queue.subscribe_sync_state();
      let weak_state = Arc::downgrade(local_collab.lock().get_state());
      af_spawn(async move {
        while let Ok(sink_state) = sync_state_stream.recv().await {
          if let Some(state) = weak_state.upgrade() {
            let sync_state = match sink_state {
              SinkState::Syncing => SyncState::Syncing,
              _ => SyncState::SyncFinished,
            };
            state.set_sync_state(sync_state);
          } else {
            break;
          }
        }
      });
    }

    let sync_queue = Arc::new(sync_queue);
    let weak_local_collab = collab.clone();
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
                let _ = sync_queue.init_sync(&local_collab, InitSyncReason::NetworkResume);
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
      collab,
      did_init_sync: Arc::new(AtomicBool::new(false)),
    }
  }

  fn start_init_sync(&self) {
    if self.did_init_sync.load(Ordering::SeqCst) {
      return;
    }
    let weak_queue = Arc::downgrade(&self.sync_queue);
    let weak_collab = self.collab.clone();
    let weak_did_init_sync = self.did_init_sync.clone();
    tokio::spawn(async move {
      sleep(Duration::from_secs(2)).await;
      if let (Some(queue), Some(collab)) = (weak_queue.upgrade(), weak_collab.upgrade()) {
        if let Some(collab) = collab.try_lock() {
          if queue.can_queue_init_sync()
            && queue
              .init_sync(&collab, InitSyncReason::CollabDidInit)
              .is_ok()
          {
            #[cfg(feature = "sync_verbose_log")]
            trace!("finish init sync: {}", queue.origin);
            weak_did_init_sync.store(true, Ordering::SeqCst);
          }
        }
      }
    });
  }
}

impl<E, Sink, Stream, C> CollabPlugin for SyncPlugin<Sink, Stream, C>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<ServerCollabMessage, E>> + Send + Sync + Unpin + 'static,
  C: Send + Sync + 'static,
{
  fn did_init(&self, _collab: &Collab, _object_id: &str, _last_sync_at: i64) {
    self.start_init_sync();
  }

  fn receive_local_update(&self, origin: &CollabOrigin, _object_id: &str, update: &[u8]) {
    if self.did_init_sync.load(Ordering::SeqCst) {
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
    } else {
      self.start_init_sync();
    }
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
      #[cfg(feature = "sync_verbose_log")]
      trace!("queue awareness: {:?}", update);
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
