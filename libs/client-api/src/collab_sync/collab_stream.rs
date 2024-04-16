use crate::af_spawn;
use crate::collab_sync::{start_sync, CollabSink, InitSyncReason, SyncError, SyncObject};

use bytes::Bytes;
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
        &origin,
        &object,
        &collab,
        &sink,
        msg,
        &seq_num_counter,
      )
      .await
      {
        match error {
          SyncError::MissUpdates(reason) => {
            Self::pull_missing_updates(&origin, &object, &collab, &sink, &seq_num_counter, reason)
              .await;
          },
          SyncError::RequireInitSync => {
            if let Some(lock_guard) = collab.try_lock() {
              if let Err(err) = start_sync(
                origin.clone(),
                &object,
                &lock_guard,
                &sink,
                InitSyncReason::RequireInitSync,
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
    _origin: &CollabOrigin,
    object: &SyncObject,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink>>,
    msg: ServerCollabMessage,
    seq_num_counter: &Arc<SeqNumCounter>,
  ) -> Result<(), SyncError> {
    // If server return the AckCode::ApplyInternalError, which means the server can not apply the
    // update
    if let ServerCollabMessage::ClientAck(ref ack) = msg {
      if ack.get_code() == AckCode::CannotApplyUpdate {
        return Err(SyncError::RequireInitSync);
      }
    }

    // msg_id will be None for [ServerBroadcast] or [ServerAwareness].
    match msg.msg_id() {
      None => {
        if let ServerCollabMessage::ServerBroadcast(ref data) = msg {
          seq_num_counter.store_broadcast_seq_num(data.seq_num);
        }
        Self::process_message_payload(&object.object_id, msg, collab, sink).await?;
        sink.notify();
        Ok(())
      },
      Some(msg_id) => {
        let is_valid = sink
          .validate_response(msg_id, &msg, seq_num_counter)
          .await?;

        if let ServerCollabMessage::ClientAck(ack) = &msg {
          if matches!(ack.get_code(), AckCode::RequireInitSync) {
            return Err(SyncError::RequireInitSync);
          }
        }

        if is_valid {
          Self::process_message_payload(&object.object_id, msg, collab, sink).await?;
        }
        sink.notify();
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
    if msg.payload().is_empty() {
      return Ok(());
    }
    ObserveCollab::<Sink, Stream>::process_payload(
      msg.origin(),
      msg.payload(),
      object_id,
      collab,
      sink,
    )
    .await
  }

  #[instrument(level = "trace", skip_all)]
  async fn pull_missing_updates(
    origin: &CollabOrigin,
    object: &SyncObject,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink>>,
    _seq_num_counter: &Arc<SeqNumCounter>,
    reason: String,
  ) {
    if let Some(lock_guard) = collab.try_lock() {
      if let Err(err) = start_sync(
        origin.clone(),
        object,
        &lock_guard,
        sink,
        InitSyncReason::MissUpdates(reason),
      ) {
        error!("Error while start sync: {}", err);
      }
    }
  }

  async fn process_payload(
    message_origin: &CollabOrigin,
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
        match handle_message_follow_protocol(message_origin, &ClientSyncProtocol, &mut collab, msg)?
        {
          Some(payload) => {
            let object_id = object_id.to_string();
            sink.queue_msg(|msg_id| {
              if is_server_sync_step_1 {
                ClientCollabMessage::new_server_init_sync(ServerInit::new(
                  message_origin.clone(),
                  object_id,
                  payload,
                  msg_id,
                ))
              } else {
                ClientCollabMessage::new_update_sync(UpdateSync::new(
                  message_origin.clone(),
                  object_id,
                  payload,
                  msg_id,
                ))
              }
            });
          },
          None => {
            // do nothing
          },
        }
      }
    }
    Ok(())
  }
}

#[derive(Default)]
pub struct SeqNumCounter {
  pub broadcast_seq_counter: AtomicU32,
  pub ack_seq_counter: AtomicU32,
  pub equal_counter: AtomicU32,
}

impl SeqNumCounter {
  pub fn store_ack_seq_num(&self, seq_num: u32) -> u32 {
    match self
      .ack_seq_counter
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
      Err(prev) => {
        // If the seq_num is less than the current seq_num, we should reset the equal_counter.
        // Because the server might be restarted and the seq_num is reset to 0.
        self.equal_counter.store(0, Ordering::SeqCst);
        self.ack_seq_counter.store(seq_num, Ordering::SeqCst);
        prev
      },
    }
  }

  pub fn store_broadcast_seq_num(&self, seq_num: u32) -> u32 {
    self
      .broadcast_seq_counter
      .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |_current| Some(seq_num))
      .unwrap()
  }

  pub fn get_ack_seq_num(&self) -> u32 {
    self.ack_seq_counter.load(Ordering::SeqCst)
  }

  pub fn get_broadcast_seq_num(&self) -> u32 {
    self.broadcast_seq_counter.load(Ordering::SeqCst)
  }

  pub fn check_ack_broadcast_contiguous(&self, object_id: &str) -> Result<(), SyncError> {
    let ack_seq_num = self.ack_seq_counter.load(Ordering::SeqCst);
    let broadcast_seq_num = self.broadcast_seq_counter.load(Ordering::SeqCst);

    #[cfg(feature = "sync_verbose_log")]
    trace!(
      "receive {} seq_num, ack:{}, broadcast:{}",
      object_id,
      ack_seq_num,
      broadcast_seq_num,
    );

    if ack_seq_num > broadcast_seq_num + 3 {
      self.store_broadcast_seq_num(ack_seq_num);
      return Err(SyncError::MissUpdates(format!(
        "missing {} updates, start init sync",
        ack_seq_num - broadcast_seq_num,
      )));
    }

    if self.equal_counter.load(Ordering::SeqCst) >= 5 {
      self.equal_counter.store(0, Ordering::SeqCst);
      return Err(SyncError::MissUpdates(
        "ping exceeds, start init sync".to_string(),
      ));
    }
    Ok(())
  }
}
