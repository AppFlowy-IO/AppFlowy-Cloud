use std::fmt::Display;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

use collab::core::awareness::Awareness;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, watch};
use tracing::{error, instrument, trace};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{ReadTxn, StateVector};

use collab_rt_entity::{ClientCollabMessage, InitSync, ServerCollabMessage, UpdateSync};
use collab_rt_protocol::{ClientSyncProtocol, CollabSyncProtocol, Message, SyncMessage};

use crate::af_spawn;
use crate::collab_sync::collab_stream::{CollabRef, ObserveCollab};
use crate::collab_sync::{
  CollabSink, CollabSinkRunner, CollabSyncState, MissUpdateReason, SinkSignal, SyncError,
  SyncObject,
};

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
    collab: CollabRef,
    periodic_sync: Option<Duration>,
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
      periodic_sync,
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
    if cfg!(feature = "sync_verbose_log") {
      trace!("pause {} sync", self.object.object_id);
    }
    self.sink.pause();
  }

  pub fn resume(&self) {
    if cfg!(feature = "sync_verbose_log") {
      trace!("resume {} sync", self.object.object_id);
    }
    self.sink.resume();
  }

  pub fn subscribe_sync_state(&self) -> broadcast::Receiver<CollabSyncState> {
    self.sync_state_tx.subscribe()
  }

  /// Returns bool indicating whether the init sync is queued.
  pub fn init_sync(
    &self,
    collab: &collab::preclude::Collab,
    reason: SyncReason,
  ) -> Result<bool, SyncError> {
    start_sync(
      self.origin.clone(),
      &self.object,
      collab,
      &self.sink,
      reason,
    )
  }
}

pub enum SyncReason {
  CollabInitialize,
  ServerMissUpdates {
    state_vector_v1: Vec<u8>,
    reason: MissUpdateReason,
  },
  ClientMissUpdates {
    reason: MissUpdateReason,
  },
  ServerCannotApplyUpdate,
  NetworkResume,
}

impl Display for SyncReason {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      SyncReason::CollabInitialize => write!(f, "CollabInitialize"),
      SyncReason::ServerMissUpdates { reason, .. } => write!(f, "ServerMissUpdates: {}", reason),
      SyncReason::ClientMissUpdates { reason } => write!(f, "ClientMissUpdates: {}", reason),
      SyncReason::ServerCannotApplyUpdate => write!(f, "ServerCannotApplyUpdate"),
      SyncReason::NetworkResume => write!(f, "NetworkResume"),
    }
  }
}

fn gen_sync_state<P: CollabSyncProtocol>(
  awareness: &Awareness,
  protocol: &P,
) -> Result<Vec<u8>, SyncError> {
  let mut encoder = EncoderV1::new();
  protocol.start(awareness, &mut encoder)?;
  Ok(encoder.to_vec())
}

fn gen_missing_updates(collab: &Collab, sv: StateVector) -> Result<Vec<u8>, SyncError> {
  let update = {
    let txn = collab.transact();
    txn.encode_state_as_update_v1(&sv)
  };

  let mut encoder = EncoderV1::new();
  Message::Sync(SyncMessage::Update(update)).encode(&mut encoder);
  Ok(encoder.to_vec())
}

#[instrument(level = "trace", skip_all)]
pub fn start_sync<E, Sink>(
  origin: CollabOrigin,
  sync_object: &SyncObject,
  collab: &Collab,
  sink: &Arc<CollabSink<Sink>>,
  reason: SyncReason,
) -> Result<bool, SyncError>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
{
  if let Err(err) = sync_object.collab_type.validate_require_data(collab) {
    return Err(SyncError::Internal(err.into()));
  }

  match reason {
    SyncReason::ClientMissUpdates { reason } => {
      if !sink.should_queue_init_sync() {
        return Ok(false);
      }

      trace!("🔥{} start sync, reason:{}", &sync_object.object_id, reason);
      let awareness = collab.get_awareness();
      let payload = gen_sync_state(awareness, &ClientSyncProtocol)?;
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
    },
    SyncReason::ServerMissUpdates {
      state_vector_v1,
      reason,
    } => match StateVector::decode_v1(&state_vector_v1) {
      Ok(sv) => {
        trace!("🔥{} start sync, reason:{}", &sync_object.object_id, reason);
        let update = gen_missing_updates(collab, sv)?;
        sink.queue_msg(|msg_id| {
          let update_sync = UpdateSync::new(
            origin.clone(),
            sync_object.object_id.clone(),
            update,
            msg_id,
          );
          ClientCollabMessage::new_update_sync(update_sync)
        });
      },
      Err(err) => error!("fail to decode server state vector: {}", err),
    },
    SyncReason::CollabInitialize
    | SyncReason::ServerCannotApplyUpdate
    | SyncReason::NetworkResume => {
      trace!(
        "🔥{} start sync, reason: {}",
        &sync_object.object_id,
        reason
      );
      let awareness = collab.get_awareness();
      let payload = gen_sync_state(awareness, &ClientSyncProtocol)?;
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
    },
  };

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
