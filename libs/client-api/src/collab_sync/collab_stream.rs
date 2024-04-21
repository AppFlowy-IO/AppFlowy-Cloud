use crate::af_spawn;
use crate::collab_sync::{start_sync, CollabSink, SyncError, SyncObject, SyncReason};

use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab_rt_entity::{AckCode, ClientCollabMessage, ServerCollabMessage, ServerInit, UpdateSync};
use collab_rt_protocol::{
  handle_message_follow_protocol, ClientSyncProtocol, Message, MessageReader, SyncMessage,
};
use futures_util::{SinkExt, StreamExt};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};

use tracing::{error, instrument, trace, warn};
use yrs::encoding::read::Cursor;
use yrs::updates::decoder::DecoderV1;

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
    #[cfg(feature = "sync_verbose_log")]
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
          warn!("Stream error:{}, stop receive incoming changes", err.into());
          break;
        },
      };

      if let Err(error) = ObserveCollab::<Sink, Stream>::process_message(
        &object,
        &collab,
        &sink,
        msg,
        &seq_num_counter,
      )
      .await
      {
        match error {
          SyncError::MissUpdates {
            state_vector_v1,
            reason,
          } => {
            Self::pull_missing_updates(&origin, &object, &collab, &sink, state_vector_v1, reason)
              .await;
          },
          SyncError::CannotApplyUpdate => {
            if let Some(lock_guard) = collab.try_lock() {
              if let Err(err) = start_sync(
                origin.clone(),
                &object,
                &lock_guard,
                &sink,
                SyncReason::ServerCannotApplyUpdate,
              ) {
                error!("Error while start sync: {}", err);
              }
            }
          },
          _ => {
            error!("Error while processing message: {}", error);
          },
        }
      }
    }
  }

  /// Continuously handle messages from the remote doc
  async fn process_message(
    object: &SyncObject,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink>>,
    msg: ServerCollabMessage,
    seq_num_counter: &Arc<SeqNumCounter>,
  ) -> Result<(), SyncError> {
    if let ServerCollabMessage::ClientAck(ack) = &msg {
      let ack_code = ack.get_code();
      // if the server can not apply the update, we start the init sync.
      if ack_code == AckCode::CannotApplyUpdate {
        return Err(SyncError::CannotApplyUpdate);
      }

      if ack_code == AckCode::MissUpdate {
        return Err(SyncError::MissUpdates {
          state_vector_v1: Some(ack.payload.to_vec()),
          reason: "server miss updates".to_string(),
        });
      }
    }

    // msg_id will be None for [ServerBroadcast] or [ServerAwareness].
    match msg.msg_id() {
      None => {
        if let ServerCollabMessage::ServerBroadcast(ref data) = msg {
          seq_num_counter.check_broadcast_contiguous(&object.object_id, data.seq_num)?;
          seq_num_counter.store_broadcast_seq_num(data.seq_num);
        }
        Self::process_message_follow_protocol(&object.object_id, &msg, collab, sink).await?;
        sink.notify_next();
        Ok(())
      },
      Some(msg_id) => {
        let is_valid = sink
          .validate_response(msg_id, &msg, seq_num_counter)
          .await?;

        if is_valid {
          Self::process_message_follow_protocol(&object.object_id, &msg, collab, sink).await?;
        }
        sink.notify_next();
        Ok(())
      },
    }
  }

  #[instrument(level = "trace", skip_all)]
  async fn pull_missing_updates(
    origin: &CollabOrigin,
    object: &SyncObject,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink>>,
    state_vector_v1: Option<Vec<u8>>,
    reason: String,
  ) {
    if let Some(lock_guard) = collab.try_lock() {
      let reason = SyncReason::MissUpdates {
        state_vector_v1,
        reason,
      };
      if let Err(err) = start_sync(origin.clone(), object, &lock_guard, sink, reason) {
        error!("Error while start sync: {}", err);
      }
    }
  }

  async fn process_message_follow_protocol(
    object_id: &str,
    msg: &ServerCollabMessage,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink>>,
  ) -> Result<(), SyncError> {
    if msg.payload().is_empty() {
      return Ok(());
    }

    let payload = msg.payload().clone();
    let message_origin = msg.origin().clone();
    let sink = sink.clone();
    let object_id = object_id.to_string();
    let collab = collab.clone();

    // workaround for panic when applying updates. It can be removed in the future
    let result = tokio::spawn(async move {
      if let Some(mut collab) = collab.try_lock() {
        let mut decoder = DecoderV1::new(Cursor::new(&payload));
        let reader = MessageReader::new(&mut decoder);
        for yrs_message in reader {
          let msg = yrs_message?;
          let is_server_sync_step_1 = matches!(msg, Message::Sync(SyncMessage::SyncStep1(_)));
          if let Some(return_payload) =
            handle_message_follow_protocol(&message_origin, &ClientSyncProtocol, &mut collab, msg)?
          {
            let object_id = object_id.to_string();
            sink.queue_msg(|msg_id| {
              if is_server_sync_step_1 {
                ClientCollabMessage::new_server_init_sync(ServerInit::new(
                  message_origin.clone(),
                  object_id,
                  return_payload,
                  msg_id,
                ))
              } else {
                ClientCollabMessage::new_update_sync(UpdateSync::new(
                  message_origin.clone(),
                  object_id,
                  return_payload,
                  msg_id,
                ))
              }
            });
          }
        }
      }
      Ok::<_, SyncError>(())
    })
    .await;

    result.unwrap_or_else(|err| {
      error!("Panic while processing message: {:?}", err);
      Err(SyncError::Internal(anyhow::anyhow!(
        "Panic while processing message"
      )))
    })
  }
}

#[derive(Default)]
pub struct SeqNumCounter {
  /// The sequence number of the last update broadcast by the server.
  /// This counter is incremented by 1 each time the server applies an update.
  pub broadcast_seq_counter: AtomicU32,
  /// The sequence number of the last update acknowledged by a client.
  /// This is set to the sequence number contained in the `CollabMessage::ClientAck` received from a client.
  /// If this number is greater than `broadcast_seq_counter`, it indicates that some updates are missing on the client side,
  /// prompting an initialization sync to rectify missing updates.
  pub ack_seq_counter: AtomicU32,
  pub miss_update_counter: AtomicU32,
}

impl SeqNumCounter {
  pub fn store_ack_seq_num(&self, seq_num: u32) -> u32 {
    // If the broadcast sequence counter is 0, set it to the current sequence number.
    if self.broadcast_seq_counter.load(Ordering::SeqCst) == 0 {
      self.broadcast_seq_counter.store(seq_num, Ordering::SeqCst);
    }

    match self
      .ack_seq_counter
      .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
        // Check if the sequence number is less than the current one. A lower sequence number can indicate
        // that the server has been restarted, or the collaboration group has been reinitialized.
        if seq_num >= current {
          Some(seq_num)
        } else {
          None
        }
      }) {
      Ok(prev) => prev,
      Err(prev) => {
        self.ack_seq_counter.store(seq_num, Ordering::SeqCst);
        prev
      },
    }
  }

  pub fn store_broadcast_seq_num(&self, broadcast_seq_num: u32) -> u32 {
    match self
      .broadcast_seq_counter
      .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
        // Check if the sequence number is less than the current one. A lower sequence number can indicate
        // that the server has been restarted, or the collaboration group has been reinitialized.
        if broadcast_seq_num >= current {
          Some(broadcast_seq_num)
        } else {
          None
        }
      }) {
      Ok(prev) => prev,
      Err(prev) => {
        self
          .broadcast_seq_counter
          .store(broadcast_seq_num, Ordering::SeqCst);
        prev
      },
    }
  }

  /// Checks if the given broadcast sequence number is contiguous with the current sequence.
  ///
  /// Verifies that the broadcast sequence number provided (`broadcast_seq_num`) follows directly after
  /// the last known sequence number stored in the system (`current`).
  ///
  /// If there is a gap between the `broadcast_seq_num` and `current`, it indicates that some
  /// messages may have been missed, and an error is returned.
  pub fn check_broadcast_contiguous(
    &self,
    object_id: &str,
    broadcast_seq_num: u32,
  ) -> Result<(), SyncError> {
    let current = self.broadcast_seq_counter.load(Ordering::SeqCst);
    if current > 0 && broadcast_seq_num > current + 1 {
      return Err(SyncError::MissUpdates {
        state_vector_v1: None,
        reason: format!(
          "{} broadcast is not contiguous, current:{}, broadcast:{}",
          object_id, current, broadcast_seq_num,
        ),
      });
    }

    Ok(())
  }

  pub fn check_ack_broadcast_contiguous(&self, object_id: &str) -> Result<(), SyncError> {
    let ack_seq_num = self.ack_seq_counter.load(Ordering::SeqCst);
    let broadcast_seq_num = self.broadcast_seq_counter.load(Ordering::SeqCst);
    log_ack_and_broadcast(object_id, ack_seq_num, broadcast_seq_num);

    if ack_seq_num > broadcast_seq_num {
      // calculate the number of times the ack is greater than the broadcast. We don't do return MissingUpdates
      // immediately, because the ack may be greater than the broadcast for a short time.
      let old = self.miss_update_counter.fetch_add(1, Ordering::SeqCst);

      if old + 1 >= 2 {
        self.miss_update_counter.store(0, Ordering::SeqCst);
        // Mark the broadcast sequence number as ack seq_num because a MissUpdates error triggers
        // an initialization synchronization. After this initial sync, the ack and broadcast sequence
        // numbers are expected to align, ensuring that all updates are synchronized.
        self
          .broadcast_seq_counter
          .store(ack_seq_num, Ordering::SeqCst);

        return Err(SyncError::MissUpdates {
          state_vector_v1: None,
          reason: format!(
            "ack is not equal to broadcast, ack:{}, broadcast:{}",
            ack_seq_num, broadcast_seq_num,
          ),
        });
      }
    }

    Ok(())
  }
}

#[cfg(feature = "sync_verbose_log")]
fn log_ack_and_broadcast(object_id: &str, ack_seq_num: u32, broadcast_seq_num: u32) {
  trace!(
    "receive {} seq_num, ack:{}, broadcast:{}",
    object_id,
    ack_seq_num,
    broadcast_seq_num,
  );
}
