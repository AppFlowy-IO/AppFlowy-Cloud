use crate::collab_sync::{CollabSyncState, SinkConfig, SyncControl, SyncReason};

use crate::af_spawn;
use crate::ws::{ConnectState, WSConnectStateReceiver};
use anyhow::anyhow;
use collab::core::awareness::{AwarenessUpdate, Event};
use collab::core::collab::MutexCollab;

use collab::core::collab_state::SyncState;
use collab::core::origin::CollabOrigin;
use collab::preclude::{Collab, CollabPlugin};
use collab_entity::{CollabObject, CollabType};
use collab_rt_entity::{ClientCollabMessage, ServerCollabMessage, UpdateSync};
use collab_rt_protocol::{Message, SyncMessage};

use futures_util::SinkExt;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::time::sleep;
use tokio_retry::strategy::FixedInterval;
use tokio_retry::{Action, Condition, RetryIf};
use tokio_stream::StreamExt;
use tracing::{error, trace};
use yrs::updates::encoder::Encode;

pub struct SyncPlugin<Sink, Stream, C> {
  object: SyncObject,
  sync_queue: Arc<SyncControl<Sink, Stream>>,
  // Used to keep the lifetime of the channel
  #[allow(dead_code)]
  channel: Option<Arc<C>>,
  collab: Weak<MutexCollab>,
  is_destroyed: Arc<AtomicBool>,
}

impl<Sink, Stream, C> Drop for SyncPlugin<Sink, Stream, C> {
  fn drop(&mut self) {
    #[cfg(feature = "sync_verbose_log")]
    trace!("Drop sync plugin: {}", self.object.object_id);

    // when the plugin is dropped, set the is_destroyed flag to true
    self
      .is_destroyed
      .store(true, std::sync::atomic::Ordering::SeqCst);
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
    mut ws_connect_state: WSConnectStateReceiver,
  ) -> Self {
    let sync_queue = SyncControl::new(
      object.clone(),
      origin,
      sink,
      sink_config,
      stream,
      collab.clone(),
    );

    if let Some(local_collab) = collab.upgrade() {
      let mut sync_state_stream = sync_queue.subscribe_sync_state();
      let weak_state = Arc::downgrade(local_collab.lock().get_state());
      af_spawn(async move {
        while let Ok(sink_state) = sync_state_stream.recv().await {
          if let Some(state) = weak_state.upgrade() {
            let sync_state = match sink_state {
              CollabSyncState::Syncing => SyncState::Syncing,
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
              sync_queue.resume();
              if let Some(local_collab) = local_collab.try_lock() {
                let _ = sync_queue.init_sync(&local_collab, SyncReason::NetworkResume);
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
      is_destroyed: Arc::new(Default::default()),
    }
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
      if cfg!(feature = "sync_verbose_log") {
        trace!("queue awareness: {:?}", update);
      }

      ClientCollabMessage::new_update_sync(update_sync)
    });
  }

  fn start_init_sync(&self) {
    let object_id = self.object.object_id.clone();
    let collab = self.collab.clone();
    let sync_queue = self.sync_queue.clone();

    tokio::spawn(async move {
      if let Some(collab) = collab.upgrade() {
        const MAX_RETRY: usize = 3;
        const RETRY_INTERVAL: Duration = Duration::from_millis(300);

        for attempt in 0..MAX_RETRY {
          if let Some(collab) = collab.clone().try_lock() {
            if let Err(err) = sync_queue.init_sync(&collab, SyncReason::CollabInitialize) {
              error!("Failed to start init sync: {}", err);
            }
            return;
          }

          trace!(
            "Attempt {} failed to lock collab for init sync: {}",
            attempt + 1,
            object_id
          );
          if attempt < MAX_RETRY - 1 {
            sleep(RETRY_INTERVAL).await;
          }
        }

        trace!(
          "Failed to start init sync after {} attempts, object_id: {}",
          MAX_RETRY,
          object_id
        );
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

pub(crate) struct InitSyncAction<Sink, Stream> {
  sync_queue: Weak<SyncControl<Sink, Stream>>,
  collab: Weak<MutexCollab>,
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
        if let Some(collab) = collab.try_lock() {
          if queue.did_queue_init_sync() {
            return Ok(());
          }
          let is_queue = queue.init_sync(&collab, SyncReason::CollabInitialize)?;
          if is_queue {
            return Ok(());
          } else {
            return Err(anyhow!("Failed to queue init sync"));
          }
        } else {
          // If failed to lock collab, return Err. it will start a new retry in the next iteration base
          // on the retry strategy
          return Err(anyhow!("Failed to lock collab"));
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
