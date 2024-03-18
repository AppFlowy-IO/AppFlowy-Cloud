use crate::af_spawn;
use crate::collab_sync::sink_config::SinkConfig;
use crate::collab_sync::{
  CollabSink, CollabSinkRunner, SinkSignal, SinkState, SyncError, SyncObject,
};
use bytes::Bytes;
use collab::core::awareness::Awareness;
use collab::core::collab::MutexCollab;
use collab::core::collab_state::SyncState;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;

use futures_util::{SinkExt, StreamExt};
use realtime_entity::collab_msg::{
  AckCode, BroadcastSync, ClientCollabMessage, InitSync, ServerCollabMessage, ServerInit,
  UpdateSync,
};
use realtime_protocol::{handle_collab_message, ClientSyncProtocol, CollabSyncProtocol};
use realtime_protocol::{Message, MessageReader, SyncMessage};
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex};
use tokio_stream::wrappers::WatchStream;
use tracing::{error, info, trace, warn};
use yrs::encoding::read::Cursor;
use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::{Encoder, EncoderV1};

pub const DEFAULT_SYNC_TIMEOUT: u64 = 10;
pub const NUMBER_OF_UPDATE_TRIGGER_INIT_SYNC: u32 = 1;

const DEBOUNCE_DURATION: Duration = Duration::from_secs(10);

pub struct SyncControl<Sink, Stream> {
  object: SyncObject,
  origin: CollabOrigin,
  /// The [CollabSink] is used to send the updates to the remote. It will send the current
  /// update periodically if the timeout is reached or it will send the next update if
  /// it receive previous ack from the remote.
  sink: Arc<CollabSink<Sink, ClientCollabMessage>>,
  /// The [ObserveCollab] will be spawned in a separate task It continuously receive
  /// the updates from the remote.
  #[allow(dead_code)]
  observe_collab: ObserveCollab<Sink, Stream>,
  sync_state: Arc<watch::Sender<SyncState>>,
}

impl<Sink, Stream> Drop for SyncControl<Sink, Stream> {
  fn drop(&mut self) {
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
    let sync_state = Arc::new(watch::channel(SyncState::InitSyncBegin).0);
    let (sync_state_tx, sink_state_rx) = watch::channel(SinkState::Init);
    debug_assert!(origin.client_user_id().is_some());

    // Create the sink and start the sink runner.
    let sink = Arc::new(CollabSink::new(
      origin.client_user_id().unwrap_or(0),
      object.clone(),
      sink,
      notifier,
      sync_state_tx,
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

    let weak_sync_state = Arc::downgrade(&sync_state);
    let mut sink_state_stream = WatchStream::new(sink_state_rx);
    // Subscribe the sink state stream and update the sync state in the background.
    af_spawn(async move {
      while let Some(collab_state) = sink_state_stream.next().await {
        if let Some(sync_state) = weak_sync_state.upgrade() {
          match collab_state {
            SinkState::Syncing => {
              let _ = sync_state.send(SyncState::Syncing);
            },
            SinkState::Finished => {
              let _ = sync_state.send(SyncState::SyncFinished);
            },
            SinkState::Init => {
              let _ = sync_state.send(SyncState::InitSyncBegin);
            },
            SinkState::Pause => {},
          }
        }
      }
    });

    Self {
      object,
      origin,
      sink,
      observe_collab: stream,
      sync_state,
    }
  }

  pub fn pause(&self) {
    trace!("pause {} sync", self.object.object_id);
    self.sink.pause();
  }

  pub fn resume(&self) {
    trace!("resume {} sync", self.object.object_id);
    self.sink.resume();
  }

  pub fn subscribe_sync_state(&self) -> watch::Receiver<SyncState> {
    self.sync_state.subscribe()
  }

  pub fn init_sync(&self, collab: &Collab) {
    _init_sync(self.origin.clone(), &self.object, collab, &self.sink);
  }

  /// Remove all the messages in the sink queue
  pub fn clear(&self) {
    self.sink.clear();
  }
}

fn doc_init_state<P: CollabSyncProtocol>(awareness: &Awareness, protocol: &P) -> Option<Vec<u8>> {
  let payload = {
    let mut encoder = EncoderV1::new();
    protocol.start(awareness, &mut encoder).ok()?;
    encoder.to_vec()
  };
  if payload.is_empty() {
    None
  } else {
    Some(payload)
  }
}

pub fn _init_sync<E, Sink>(
  origin: CollabOrigin,
  sync_object: &SyncObject,
  collab: &Collab,
  sink: &Arc<CollabSink<Sink, ClientCollabMessage>>,
) where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
{
  let awareness = collab.get_awareness();
  if let Some(payload) = doc_init_state(awareness, &ClientSyncProtocol) {
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
  } else {
    sink.notify();
  }
}

impl<Sink, Stream> Deref for SyncControl<Sink, Stream> {
  type Target = Arc<CollabSink<Sink, ClientCollabMessage>>;

  fn deref(&self) -> &Self::Target {
    &self.sink
  }
}

/// Use to continuously receive updates from remote.
struct ObserveCollab<Sink, Stream> {
  object_id: String,
  #[allow(dead_code)]
  weak_collab: Weak<MutexCollab>,
  phantom_sink: PhantomData<Sink>,
  phantom_stream: PhantomData<Stream>,
}

impl<Sink, Stream> Drop for ObserveCollab<Sink, Stream> {
  fn drop(&mut self) {
    trace!("Drop SyncStream {}", self.object_id);
  }
}

impl<E, Sink, Stream> ObserveCollab<Sink, Stream>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<ServerCollabMessage, E>> + Send + Sync + Unpin + 'static,
{
  pub fn new(
    origin: CollabOrigin,
    object: SyncObject,
    stream: Stream,
    weak_collab: Weak<MutexCollab>,
    sink: Weak<CollabSink<Sink, ClientCollabMessage>>,
  ) -> Self {
    let seq_num = Arc::new(AtomicU32::new(0));
    let last_init_sync = LastSyncTime::new();
    let object_id = object.object_id.clone();
    let cloned_weak_collab = weak_collab.clone();
    af_spawn(ObserveCollab::<Sink, Stream>::observer_collab_message(
      origin,
      object,
      stream,
      cloned_weak_collab,
      sink,
      seq_num,
      last_init_sync,
    ));
    Self {
      object_id,
      weak_collab,
      phantom_sink: Default::default(),
      phantom_stream: Default::default(),
    }
  }

  // Spawn the stream that continuously reads the doc's updates from remote.
  async fn observer_collab_message(
    origin: CollabOrigin,
    object: SyncObject,
    mut stream: Stream,
    weak_collab: Weak<MutexCollab>,
    weak_sink: Weak<CollabSink<Sink, ClientCollabMessage>>,
    broadcast_seq_num: Arc<AtomicU32>,
    last_init_sync: LastSyncTime,
  ) {
    while let Some(collab_message_result) = stream.next().await {
      let collab = match weak_collab.upgrade() {
        Some(collab) => collab,
        None => break, // Collab dropped, stop the stream.
      };

      let sink = match weak_sink.upgrade() {
        Some(sink) => sink,
        None => break, // Sink dropped, stop the stream.
      };

      let msg = match collab_message_result {
        Ok(msg) => msg,
        Err(err) => {
          warn!(
            "Stream error: {}, stop receive incoming changes",
            err.into()
          );
          break;
        },
      };

      if let Err(error) = ObserveCollab::<Sink, Stream>::process_message(
        &origin,
        &object,
        &collab,
        &sink,
        msg,
        &broadcast_seq_num,
        &last_init_sync,
      )
      .await
      {
        if error.is_cannot_apply_update() {
          // TODO(nathan): ask the client to resolve the conflict.
          error!(
            "collab:{} can not be synced because of error: {}",
            object.object_id, error
          );
          break;
        } else {
          error!("Error while processing message: {}", error);
        }
      }
    }
  }

  /// Continuously handle messages from the remote doc
  async fn process_message(
    origin: &CollabOrigin,
    object: &SyncObject,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink, ClientCollabMessage>>,
    msg: ServerCollabMessage,
    broadcast_seq_num: &Arc<AtomicU32>,
    last_sync_time: &LastSyncTime,
  ) -> Result<(), SyncError> {
    // If server return the AckCode::ApplyInternalError, which means the server can not apply the
    // update
    if let ServerCollabMessage::ClientAck(ref ack) = msg {
      if ack.code == AckCode::CannotApplyUpdate {
        return Err(SyncError::CannotApplyUpdate(object.object_id.clone()));
      }
    }

    if let ServerCollabMessage::ServerBroadcast(ref data) = msg {
      if let Err(err) = Self::validate_broadcast(object, data, broadcast_seq_num).await {
        info!("{}", err);
        Self::try_init_sync(origin, object, collab, sink, last_sync_time).await;
      }
    }

    // Check if the message is acknowledged by the sink. If not, return.
    let is_valid = sink.validate_response(&msg).await;
    // If there's no payload or the payload is empty, return.
    if is_valid && !msg.payload().is_empty() {
      let msg_origin = msg.origin();
      let origin = ObserveCollab::<Sink, Stream>::process_payload(
        msg_origin,
        msg.payload(),
        &object.object_id,
        collab,
        sink,
      )
      .await?;
    }

    if is_valid {
      sink.notify();
    }
    Ok(())
  }

  async fn try_init_sync(
    origin: &CollabOrigin,
    object: &SyncObject,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink, ClientCollabMessage>>,
    last_sync_time: &LastSyncTime,
  ) {
    let debounce_duration = if cfg!(debug_assertions) {
      Duration::from_secs(2)
    } else {
      DEBOUNCE_DURATION
    };
    if sink.can_queue_init_sync() && last_sync_time.should_sync(debounce_duration).await {
      if let Some(lock_guard) = collab.try_lock() {
        _init_sync(origin.clone(), object, &lock_guard, sink);
      }
    }
  }

  async fn validate_broadcast(
    object: &SyncObject,
    broadcast_sync: &BroadcastSync,
    broadcast_seq_num: &Arc<AtomicU32>,
  ) -> Result<(), SyncError> {
    let prev_seq_num = broadcast_seq_num.load(Ordering::SeqCst);
    broadcast_seq_num.store(broadcast_sync.seq_num, Ordering::SeqCst);
    trace!(
      "receive {} broadcast data, current: {}, prev: {}",
      object.object_id,
      broadcast_sync.seq_num,
      prev_seq_num
    );

    // Check if the received seq_num indicates missing updates.
    if broadcast_sync.seq_num > prev_seq_num + NUMBER_OF_UPDATE_TRIGGER_INIT_SYNC {
      return Err(SyncError::MissingBroadcast(format!(
        "{} missing updates len: {}, start init sync",
        object.object_id,
        broadcast_sync.seq_num - prev_seq_num,
      )));
    }

    Ok(())
  }

  async fn process_payload(
    origin: &CollabOrigin,
    payload: &Bytes,
    object_id: &str,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink, ClientCollabMessage>>,
  ) -> Result<(), SyncError> {
    if let Some(mut collab) = collab.try_lock() {
      let mut decoder = DecoderV1::new(Cursor::new(payload));
      let reader = MessageReader::new(&mut decoder);
      for msg in reader {
        let msg = msg?;
        let is_server_sync_step_1 = matches!(msg, Message::Sync(SyncMessage::SyncStep1(_)));
        if let Some(payload) = handle_collab_message(origin, &ClientSyncProtocol, &mut collab, msg)?
        {
          let object_id = object_id.to_string();
          sink.queue_msg(|msg_id| {
            if is_server_sync_step_1 {
              ClientCollabMessage::new_server_init_sync(ServerInit::new(
                origin.clone(),
                object_id,
                payload,
                msg_id,
              ))
            } else {
              ClientCollabMessage::new_update_sync(UpdateSync::new(
                origin.clone(),
                object_id,
                payload,
                msg_id,
              ))
            }
          });
        }
      }
    }
    Ok(())
  }
}

struct LastSyncTime {
  last_sync: Mutex<Instant>,
}

impl LastSyncTime {
  fn new() -> Self {
    let now = Instant::now();
    let one_hour = Duration::from_secs(3600);
    // Use checked_sub to safely attempt subtraction, falling back to 'now' if underflow would occur
    let one_hour_ago = now.checked_sub(one_hour).unwrap_or(now);

    LastSyncTime {
      last_sync: Mutex::new(one_hour_ago),
    }
  }

  async fn should_sync(&self, debounce_duration: Duration) -> bool {
    let now = Instant::now();
    let mut last_sync_locked = self.last_sync.lock().await;
    if now.duration_since(*last_sync_locked) > debounce_duration {
      *last_sync_locked = now;
      true
    } else {
      false
    }
  }
}
