use crate::af_spawn;
use crate::collab_sync::{
  start_sync, CollabSink, SyncError, SyncObject, NUMBER_OF_UPDATE_TRIGGER_INIT_SYNC,
};
use anyhow::anyhow;
use bytes::Bytes;
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab_rt_entity::{
  AckCode, BroadcastSync, ClientCollabMessage, ServerCollabMessage, ServerInit, UpdateSync,
};
use collab_rt_protocol::{handle_message, ClientSyncProtocol, Message, MessageReader, SyncMessage};
use futures_util::{SinkExt, StreamExt};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{error, instrument, trace, warn};
use yrs::encoding::read::Cursor;
use yrs::updates::decoder::DecoderV1;

const DEBOUNCE_DURATION: Duration = Duration::from_secs(10);

/// Use to continuously receive updates from remote.
pub struct ObserveCollab<Sink, Stream> {
  object_id: String,
  #[allow(dead_code)]
  weak_collab: Weak<MutexCollab>,
  phantom_sink: PhantomData<Sink>,
  phantom_stream: PhantomData<Stream>,
  // Use sequence number to check if the received updates/broadcasts are continuous.
  #[allow(dead_code)]
  seq_num_counter: Arc<SeqNumCounter>,
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
    sink: Weak<CollabSink<Sink>>,
  ) -> Self {
    let last_init_sync = LastSyncTime::new();
    let object_id = object.object_id.clone();
    let cloned_weak_collab = weak_collab.clone();
    let seq_num_counter = Arc::new(SeqNumCounter::default());
    let cloned_seq_num_counter = seq_num_counter.clone();
    af_spawn(ObserveCollab::<Sink, Stream>::observer_collab_message(
      origin,
      object,
      stream,
      cloned_weak_collab,
      sink,
      cloned_seq_num_counter,
      last_init_sync,
    ));
    Self {
      object_id,
      weak_collab,
      phantom_sink: Default::default(),
      phantom_stream: Default::default(),
      seq_num_counter,
    }
  }

  // Spawn the stream that continuously reads the doc's updates from remote.
  async fn observer_collab_message(
    origin: CollabOrigin,
    object: SyncObject,
    mut stream: Stream,
    weak_collab: Weak<MutexCollab>,
    weak_sink: Weak<CollabSink<Sink>>,
    seq_num_counter: Arc<SeqNumCounter>,
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
        &seq_num_counter,
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
    sink: &Arc<CollabSink<Sink>>,
    msg: ServerCollabMessage,
    seq_num_counter: &Arc<SeqNumCounter>,
    last_init_time: &LastSyncTime,
  ) -> Result<(), SyncError> {
    // If server return the AckCode::ApplyInternalError, which means the server can not apply the
    // update
    if let ServerCollabMessage::ClientAck(ref ack) = msg {
      if ack.code == AckCode::CannotApplyUpdate {
        return Err(SyncError::CannotApplyUpdate(object.object_id.clone()));
      }
    }

    // msg_id will be None for [ServerBroadcast] or [ServerAwareness].
    match msg.msg_id() {
      None => {
        if let ServerCollabMessage::ServerBroadcast(ref data) = msg {
          if let Err(err) = Self::validate_broadcast(object, data, seq_num_counter).await {
            if err.is_missing_updates() {
              Self::pull_missing_updates(origin, object, collab, sink, last_init_time).await;
              return Ok(());
            }
          }
        }
        Self::process_message_payload(&object.object_id, msg, collab, sink).await?;
        sink.notify();
        Ok(())
      },
      Some(msg_id) => {
        // Check if the message is acknowledged by the sink.
        match sink.validate_response(msg_id, &msg, seq_num_counter).await {
          Ok(is_valid) => {
            if is_valid {
              Self::process_message_payload(&object.object_id, msg, collab, sink).await?;
            }
            sink.notify();
          },
          Err(err) => {
            // Update the last sync time if the message is valid.
            if err.is_missing_updates() {
              Self::pull_missing_updates(origin, object, collab, sink, last_init_time).await;
            } else {
              error!("Error while validating response: {}", err);
            }
          },
        }
        Ok(())
      },
    }
  }

  async fn process_message_payload(
    object_id: &str,
    msg: ServerCollabMessage,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink>>,
  ) -> Result<(), SyncError> {
    if !msg.payload().is_empty() {
      let msg_origin = msg.origin();
      ObserveCollab::<Sink, Stream>::process_payload(
        msg_origin,
        msg.payload(),
        object_id,
        collab,
        sink,
      )
      .await?;
    }
    Ok(())
  }

  #[instrument(level = "trace", skip_all)]
  async fn pull_missing_updates(
    origin: &CollabOrigin,
    object: &SyncObject,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink>>,
    last_sync_time: &LastSyncTime,
  ) {
    let debounce_duration = if cfg!(debug_assertions) {
      Duration::from_secs(2)
    } else {
      DEBOUNCE_DURATION
    };

    if sink.can_queue_init_sync()
      && last_sync_time
        .can_proceed_with_sync(debounce_duration)
        .await
    {
      if let Some(lock_guard) = collab.try_lock() {
        trace!("Start pull missing updates for {}", object.object_id);
        start_sync(origin.clone(), object, &lock_guard, sink);
      }
    }
  }

  async fn validate_broadcast(
    object: &SyncObject,
    broadcast_sync: &BroadcastSync,
    seq_num_counter: &Arc<SeqNumCounter>,
  ) -> Result<(), SyncError> {
    check_update_contiguous(&object.object_id, broadcast_sync.seq_num, seq_num_counter)?;
    Ok(())
  }

  async fn process_payload(
    origin: &CollabOrigin,
    payload: &Bytes,
    object_id: &str,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink>>,
  ) -> Result<(), SyncError> {
    if let Some(mut collab) = collab.try_lock() {
      let mut decoder = DecoderV1::new(Cursor::new(payload));
      let reader = MessageReader::new(&mut decoder);
      for msg in reader {
        let msg = msg?;
        let is_server_sync_step_1 = matches!(msg, Message::Sync(SyncMessage::SyncStep1(_)));
        if let Some(payload) = handle_message(origin, &ClientSyncProtocol, &mut collab, msg)? {
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

  async fn can_proceed_with_sync(&self, debounce_duration: Duration) -> bool {
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

/// Check if the update is contiguous.
///
/// when client send updates to the server, the seq_num should be increased otherwise which means the
/// sever might lack of some updates for given client.
pub(crate) fn check_update_contiguous(
  object_id: &str,
  current_seq_num: u32,
  seq_num_counter: &Arc<SeqNumCounter>,
) -> Result<(), SyncError> {
  let prev_seq_num = seq_num_counter.fetch_update(current_seq_num);
  trace!(
    "receive {} seq_num, prev:{}, current:{}",
    object_id,
    prev_seq_num,
    current_seq_num,
  );

  // if the seq_num is 0, it means the client is just connected to the server.
  if prev_seq_num == 0 && current_seq_num == 0 {
    return Ok(());
  }

  if current_seq_num < prev_seq_num {
    return Err(SyncError::Internal(anyhow!(
      "{} invalid seq_num, prev:{}, current:{}",
      object_id,
      prev_seq_num,
      current_seq_num,
    )));
  }

  if current_seq_num > prev_seq_num + NUMBER_OF_UPDATE_TRIGGER_INIT_SYNC
    || seq_num_counter.should_init_sync()
  {
    seq_num_counter.reset_counter();
    return Err(SyncError::MissingUpdates(format!(
      "{} missing {} updates, should start init sync",
      object_id,
      current_seq_num - prev_seq_num,
    )));
  }
  Ok(())
}

#[derive(Default)]
pub struct SeqNumCounter {
  pub counter: AtomicU32,
  pub equal_counter: AtomicU32,
}

impl SeqNumCounter {
  pub fn fetch_update(&self, seq_num: u32) -> u32 {
    match self
      .counter
      .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
        if seq_num >= current {
          Some(seq_num)
        } else {
          None
        }
      }) {
      Ok(prev) => {
        if prev == seq_num {
          self.equal_counter.fetch_add(1, Ordering::SeqCst);
        } else {
          self.equal_counter.store(0, Ordering::SeqCst);
        }
        prev
      },
      Err(prev) => prev,
    }
  }

  pub fn should_init_sync(&self) -> bool {
    // when receive 8 continuous equal seq_num, we should start the init sync.
    self.equal_counter.load(Ordering::SeqCst) >= 8
  }

  pub fn reset_counter(&self) {
    self.equal_counter.store(0, Ordering::SeqCst);
  }
}
