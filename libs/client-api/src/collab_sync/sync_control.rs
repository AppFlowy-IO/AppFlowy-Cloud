use crate::af_spawn;
use crate::collab_sync::collab_stream::ObserveCollab;
use crate::collab_sync::{
  CollabSink, CollabSinkRunner, CollabSyncState, SinkSignal, SyncError, SyncObject,
};
use anyhow::anyhow;
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

pub struct SyncControl<Sink, Stream> {
  object: SyncObject,
  pub(crate) origin: CollabOrigin,
  /// The [CollabSink] is used to send the updates to the remote. It will send the current
  /// update periodically if the timeout is reached or it will send the next update if
  /// it receive previous ack from the remote.
  sink: Arc<CollabSink<Sink>>,
  /// The [ObserveCollab] will be spawned in a separate task It continuously receive
  /// the updates from the remote.
  #[allow(dead_code)]
  observe_collab: ObserveCollab<Sink, Stream>,
  sync_state_tx: broadcast::Sender<CollabSyncState>,
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

  pub fn subscribe_sync_state(&self) -> broadcast::Receiver<CollabSyncState> {
    self.sync_state_tx.subscribe()
  }

  /// Returns bool indicating whether the init sync is queued.
  pub fn init_sync(&self, collab: &Collab, reason: InitSyncReason) -> Result<bool, SyncError> {
    start_sync(
      self.origin.clone(),
      &self.object,
      collab,
      &self.sink,
      reason,
    )
  }

  /// Remove all the messages in the sink queue
  pub fn clear(&self) {
    self.sink.clear();
  }
}

pub enum InitSyncReason {
  CollabDidInit,
  MissUpdates(String),
  RequireInitSync,
  NetworkResume,
}

fn gen_sync_state<P: CollabSyncProtocol>(
  awareness: &Awareness,
  protocol: &P,
  sync_before: bool,
) -> Result<Vec<u8>, SyncError> {
  let mut encoder = EncoderV1::new();
  protocol.start(awareness, &mut encoder, sync_before)?;
  Ok(encoder.to_vec())
}

pub fn start_sync<E, Sink>(
  origin: CollabOrigin,
  sync_object: &SyncObject,
  collab: &Collab,
  sink: &Arc<CollabSink<Sink>>,
  reason: InitSyncReason,
) -> Result<bool, SyncError>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
{
  if !sink.should_queue_init_sync() {
    return Ok(false);
  }

  if let Err(err) = sync_object.collab_type.validate(collab) {
    #[cfg(feature = "sync_verbose_log")]
    trace!(
      "{}: skip queue init sync. error: {}",
      sync_object.object_id,
      err
    );
    return Err(SyncError::Internal(anyhow!("Lack of required data")));
  }

  let sync_before = collab.get_last_sync_at() > 0;
  let awareness = collab.get_awareness();
  let payload = gen_sync_state(awareness, &ClientSyncProtocol, sync_before)?;

  #[cfg(feature = "sync_verbose_log")]
  match reason {
    InitSyncReason::CollabDidInit => {
      trace!(
        "{} collab did init and then try init sync",
        &sync_object.object_id,
      );
    },
    InitSyncReason::MissUpdates(reason) => {
      trace!(
        "🔥🔥🔥{} start pull missing updates, reason:{}",
        &sync_object.object_id,
        reason
      );
    },
    InitSyncReason::RequireInitSync => {
      trace!("{} retry init sync", &sync_object.object_id,);
    },
    InitSyncReason::NetworkResume => {
      trace!("{} network resume, retry init sync", &sync_object.object_id,);
    },
  }

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
  });
  Ok(true)
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
