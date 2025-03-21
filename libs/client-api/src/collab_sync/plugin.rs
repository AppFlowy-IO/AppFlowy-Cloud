use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Weak};
use std::time::Duration;

use anyhow::anyhow;
use collab::core::awareness::{AwarenessUpdate, Event};
use collab::core::collab_plugin::CollabPluginType;
use collab::core::collab_state::SyncState;
use collab::core::origin::CollabOrigin;
use collab::preclude::{Collab, CollabPlugin};
use futures_util::SinkExt;
use tokio_retry::strategy::FixedInterval;
use tokio_retry::{Action, Condition, RetryIf};
use tokio_stream::StreamExt;
use tracing::{error, trace};
use uuid::Uuid;
use yrs::updates::encoder::Encode;

use client_api_entity::{CollabObject, CollabType};
use collab_rt_entity::{ClientCollabMessage, ServerCollabMessage, UpdateSync};
use collab_rt_protocol::{Message, SyncMessage};

use crate::collab_sync::collab_stream::CollabRef;
use crate::collab_sync::{CollabSyncState, SinkConfig, SyncControl, SyncReason};
use crate::ws::{ConnectState, WSConnectStateReceiver};

pub struct SyncPlugin<Sink, Stream, Channel> {
  object: SyncObject,
  sync_queue: Arc<SyncControl<Sink, Stream>>,
  // Used to keep the lifetime of the channel
  #[allow(dead_code)]
  channel: Option<Arc<Channel>>,
  collab: CollabRef,
  is_destroyed: Arc<AtomicBool>,
}

impl<Sink, Stream, Channel> Drop for SyncPlugin<Sink, Stream, Channel> {
  fn drop(&mut self) {
    #[cfg(feature = "sync_verbose_log")]
    trace!("Drop sync plugin: {}", self.object.object_id);

    // when the plugin is dropped, set the is_destroyed flag to true
    self
      .is_destroyed
      .store(true, std::sync::atomic::Ordering::SeqCst);
  }
}

impl<E, Sink, Stream, Channel> SyncPlugin<Sink, Stream, Channel>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<ServerCollabMessage, E>> + Send + Sync + Unpin + 'static,
  Channel: Send + Sync + 'static,
{
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    origin: CollabOrigin,
    object: SyncObject,
    collab: CollabRef,
    sink: Sink,
    sink_config: SinkConfig,
    stream: Stream,
    channel: Option<Arc<Channel>>,
    mut ws_connect_state: WSConnectStateReceiver,
    periodic_sync: Option<Duration>,
  ) -> Self {
    let sync_queue = SyncControl::new(
      object.clone(),
      origin,
      sink,
      sink_config,
      stream,
      collab.clone(),
      periodic_sync,
    );

    let mut sync_state_stream = sync_queue.subscribe_sync_state();
    let sync_state_collab = collab.clone();
    tokio::spawn(async move {
      while let Ok(sink_state) = sync_state_stream.recv().await {
        if let Some(collab) = sync_state_collab.upgrade() {
          let sync_state = match sink_state {
            CollabSyncState::Syncing => SyncState::Syncing,
            _ => SyncState::SyncFinished,
          };
          let lock = collab.read().await;
          lock.borrow().get_state().set_sync_state(sync_state);
        } else {
          break;
        }
      }
    });

    let sync_queue = Arc::new(sync_queue);
    let weak_local_collab = collab.clone();
    let weak_sync_queue = Arc::downgrade(&sync_queue);
    tokio::spawn(async move {
      while let Ok(connect_state) = ws_connect_state.recv().await {
        match connect_state {
          ConnectState::Connected => {
            // If the websocket is connected, initialize a new init sync
            if let (Some(local_collab), Some(sync_queue)) =
              (weak_local_collab.upgrade(), weak_sync_queue.upgrade())
            {
              sync_queue.resume();
              let lock = local_collab.read().await;
              let _ = sync_queue.init_sync(lock.borrow(), SyncReason::NetworkResume);
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
      is_destroyed: Arc::new(Default::default()),
    }
  }
}

impl<E, Sink, Stream, Channel> CollabPlugin for SyncPlugin<Sink, Stream, Channel>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<ServerCollabMessage, E>> + Send + Sync + Unpin + 'static,
  Channel: Send + Sync + 'static,
{
  fn plugin_type(&self) -> CollabPluginType {
    CollabPluginType::CloudStorage
  }

  fn did_init(&self, _collab: &Collab, _object_id: &str) {
    // Most of the time, it should be successful to queue init sync by 1st time.
    let retry_strategy = FixedInterval::new(Duration::from_secs(1)).take(10);
    let action = InitSyncAction {
      sync_queue: Arc::downgrade(&self.sync_queue),
      collab: self.collab.clone(),
    };

    let condition = InitSyncRetryCondition {
      is_plugin_destroyed: self.is_destroyed.clone(),
    };

    tokio::spawn(async move {
      if let Err(err) = RetryIf::spawn(retry_strategy, action, condition).await {
        error!("Failed to start init sync: {}", err);
      }
    });
  }

  fn receive_local_update(&self, origin: &CollabOrigin, _object_id: &str, update: &[u8]) {
    let update = update.to_vec();
    let payload = Message::Sync(SyncMessage::Update(update)).encode_v1();
    self.sync_queue.queue_msg(|msg_id| {
      let update_sync = UpdateSync::new(
        origin.clone(),
        self.object.object_id.to_string(),
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
    let payload = Message::Awareness(update.encode_v1()).encode_v1();
    self.sync_queue.queue_msg(|msg_id| {
      let update_sync = UpdateSync::new(origin.clone(), object_id.to_string(), payload, msg_id);
      if cfg!(feature = "sync_verbose_log") {
        trace!("queue awareness: {:?}", update);
      }

      ClientCollabMessage::new_awareness_sync(update_sync)
    });
  }

  fn start_init_sync(&self) {
    let collab = self.collab.clone();
    let sync_queue = self.sync_queue.clone();

    tokio::spawn(async move {
      if let Some(collab) = collab.upgrade() {
        let lock = collab.read().await;
        if let Err(err) = sync_queue.init_sync(lock.borrow(), SyncReason::CollabInitialize) {
          error!("Failed to start init sync: {}", err);
        }
      }
    });
  }

  fn destroy(&self) {
    self
      .is_destroyed
      .store(true, std::sync::atomic::Ordering::SeqCst);
  }
}

#[derive(Clone, Debug)]
pub struct SyncObject {
  pub object_id: Uuid,
  pub workspace_id: Uuid,
  pub collab_type: CollabType,
  pub device_id: String,
}

impl SyncObject {
  pub fn new(
    object_id: Uuid,
    workspace_id: Uuid,
    collab_type: CollabType,
    device_id: &str,
  ) -> Self {
    Self {
      object_id,
      workspace_id,
      collab_type,
      device_id: device_id.to_string(),
    }
  }
}

impl TryFrom<CollabObject> for SyncObject {
  type Error = anyhow::Error;
  fn try_from(collab_object: CollabObject) -> Result<Self, Self::Error> {
    Ok(Self {
      object_id: Uuid::parse_str(&collab_object.object_id)?,
      workspace_id: Uuid::parse_str(&collab_object.workspace_id)?,
      collab_type: collab_object.collab_type,
      device_id: collab_object.device_id,
    })
  }
}

pub(crate) struct InitSyncAction<Sink, Stream> {
  sync_queue: Weak<SyncControl<Sink, Stream>>,
  collab: CollabRef,
}

impl<E, Sink, Stream> Action for InitSyncAction<Sink, Stream>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<ServerCollabMessage, E>> + Send + Sync + Unpin + 'static,
{
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + Send + Sync>>;
  type Item = ();
  type Error = anyhow::Error;

  fn run(&mut self) -> Self::Future {
    let weak_queue = self.sync_queue.clone();
    let weak_collab = self.collab.clone();
    Box::pin(async move {
      if let (Some(queue), Some(collab)) = (weak_queue.upgrade(), weak_collab.upgrade()) {
        if queue.did_queue_init_sync() {
          return Ok(());
        }
        let lock = collab.read().await;
        let collab = (*lock).borrow();
        let is_queue = queue.init_sync(collab, SyncReason::CollabInitialize)?;
        if is_queue {
          return Ok(());
        } else {
          return Err(anyhow!("Failed to queue init sync"));
        }
      }

      // If the queue or collab is dropped, return Ok to stop retrying.
      Ok(())
    })
  }
}

pub(crate) struct InitSyncRetryCondition {
  is_plugin_destroyed: Arc<AtomicBool>,
}
impl Condition<anyhow::Error> for InitSyncRetryCondition {
  fn should_retry(&mut self, _error: &anyhow::Error) -> bool {
    // Only retry if the plugin is not destroyed
    !self
      .is_plugin_destroyed
      .load(std::sync::atomic::Ordering::SeqCst)
  }
}
