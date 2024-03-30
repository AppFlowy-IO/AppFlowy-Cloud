use crate::af_spawn;
use crate::collab_sync::collab_stream::ObserveCollab;
use crate::collab_sync::{CollabSink, CollabSinkRunner, SinkSignal, SinkState, SyncObject};
use collab::core::awareness::Awareness;
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_rt_entity::{ClientCollabMessage, InitSync, ServerCollabMessage};
use collab_rt_protocol::{ClientSyncProtocol, CollabSyncProtocol};
use futures_util::{SinkExt, StreamExt};
use std::ops::Deref;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{broadcast, watch};

use tracing::trace;
use yrs::updates::encoder::{Encoder, EncoderV1};

pub const DEFAULT_SYNC_TIMEOUT: u64 = 10;
pub const NUMBER_OF_UPDATE_TRIGGER_INIT_SYNC: u32 = 1;

pub struct SyncControl<Sink, Stream> {
  object: SyncObject,
  origin: CollabOrigin,
  /// The [CollabSink] is used to send the updates to the remote. It will send the current
  /// update periodically if the timeout is reached or it will send the next update if
  /// it receive previous ack from the remote.
  sink: Arc<CollabSink<Sink>>,
  /// The [ObserveCollab] will be spawned in a separate task It continuously receive
  /// the updates from the remote.
  #[allow(dead_code)]
  observe_collab: ObserveCollab<Sink, Stream>,
  sync_state_tx: broadcast::Sender<SinkState>,
}

impl<Sink, Stream> Drop for SyncControl<Sink, Stream> {
  fn drop(&mut self) {
    #[cfg(feature = "sync_verbose_log")]
    trace!("Drop SyncQueue {}", self.object.object_id);
  }
}

impl<E, Sink, Stream> SyncControl<Sink, Stream>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<ServerCollabMessage, E>> + Send + Sync + Unpin + 'static,
{
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    object: SyncObject,
    origin: CollabOrigin,
    sink: Sink,
    sink_config: SinkConfig,
    stream: Stream,
    collab: Weak<MutexCollab>,
    pause: bool,
  ) -> Self {
    let protocol = ClientSyncProtocol;
    let (notifier, notifier_rx) = watch::channel(SinkSignal::Proceed);
    let (sync_state_tx, _) = broadcast::channel(10);
    debug_assert!(origin.client_user_id().is_some());

    // Create the sink and start the sink runner.
    let sink = Arc::new(CollabSink::new(
      origin.client_user_id().unwrap_or(0),
      object.clone(),
      sink,
      notifier,
      sync_state_tx.clone(),
      sink_config,
      pause,
    ));
    af_spawn(CollabSinkRunner::run(Arc::downgrade(&sink), notifier_rx));

    // Create the observe collab stream.
    let _cloned_protocol = protocol.clone();
    let _object_id = object.object_id.clone();
    let stream = ObserveCollab::new(
      origin.clone(),
      object.clone(),
      stream,
      collab.clone(),
      Arc::downgrade(&sink),
    );

    // let weak_sync_state = Arc::downgrade(&sync_state);
    // let mut sink_state_stream = WatchStream::new(sink_state_rx);
    // // Subscribe the sink state stream and update the sync state in the background.
    // af_spawn(async move {
    //   while let Some(collab_state) = sink_state_stream.next().await {
    //     if let Some(sync_state) = weak_sync_state.upgrade() {
    //       match collab_state {
    //         SinkState::Syncing => {
    //           let _ = sync_state.send(SyncState::Syncing);
    //         },
    //         SinkState::Finished => {
    //           let _ = sync_state.send(SyncState::SyncFinished);
    //         },
    //         SinkState::Init => {
    //           let _ = sync_state.send(SyncState::InitSyncBegin);
    //         },
    //         SinkState::Pause => {},
    //       }
    //     }
    //   }
    // });

    Self {
      object,
      origin,
      sink,
      observe_collab: stream,
      sync_state_tx,
    }
  }

  pub fn pause(&self) {
    #[cfg(feature = "sync_verbose_log")]
    trace!("pause {} sync", self.object.object_id);
    self.sink.pause();
  }

  pub fn resume(&self) {
    #[cfg(feature = "sync_verbose_log")]
    trace!("resume {} sync", self.object.object_id);
    self.sink.resume();
  }

  pub fn subscribe_sync_state(&self) -> broadcast::Receiver<SinkState> {
    self.sync_state_tx.subscribe()
  }

  pub fn init_sync(&self, collab: &Collab) {
    start_sync(self.origin.clone(), &self.object, collab, &self.sink);
  }

  /// Remove all the messages in the sink queue
  pub fn clear(&self) {
    self.sink.clear();
  }
}

fn gen_sync_state<P: CollabSyncProtocol>(
  awareness: &Awareness,
  protocol: &P,
  sync_before: bool,
) -> Option<Vec<u8>> {
  let mut encoder = EncoderV1::new();
  protocol.start(awareness, &mut encoder, sync_before).ok()?;
  Some(encoder.to_vec())
}

pub fn start_sync<E, Sink>(
  origin: CollabOrigin,
  sync_object: &SyncObject,
  collab: &Collab,
  sink: &Arc<CollabSink<Sink>>,
) where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
{
  let sync_before = collab.get_last_sync_at() > 0;
  let awareness = collab.get_awareness();
  if let Some(payload) = gen_sync_state(awareness, &ClientSyncProtocol, sync_before) {
    sink.queue_init_sync(|msg_id| {
      let init_sync = InitSync::new(
        origin,
        sync_object.object_id.clone(),
        sync_object.collab_type.clone(),
        sync_object.workspace_id.clone(),
        msg_id,
        payload,
      );

      ClientCollabMessage::new_init_sync(init_sync)
    })
  }
}

impl<Sink, Stream> Deref for SyncControl<Sink, Stream> {
  type Target = Arc<CollabSink<Sink>>;

  fn deref(&self) -> &Self::Target {
    &self.sink
  }
}

pub struct SinkConfig {
  /// `timeout` is the time to wait for the remote to ack the message. If the remote
  /// does not ack the message in time, the message will be sent again.
  pub send_timeout: Duration,
  /// `maximum_payload_size` is the maximum size of the messages to be merged.
  pub maximum_payload_size: usize,
}

impl SinkConfig {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn send_timeout(mut self, secs: u64) -> Self {
    self.send_timeout = Duration::from_secs(secs);
    self
  }
}

impl Default for SinkConfig {
  fn default() -> Self {
    Self {
      send_timeout: Duration::from_secs(DEFAULT_SYNC_TIMEOUT),
      maximum_payload_size: 1024 * 10,
    }
  }
}
