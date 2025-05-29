use std::borrow::BorrowMut;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use arc_swap::ArcSwap;
use collab::core::origin::CollabOrigin;
use collab::lock::RwLock;
use collab::preclude::Collab;
use futures_util::{SinkExt, StreamExt};
use tokio::select;
use tokio_util::sync::CancellationToken;
use tracing::{error, instrument, trace, warn};
use uuid::Uuid;
use yrs::encoding::read::Cursor;
use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::Encode;
use yrs::ReadTxn;

use client_api_entity::{validate_data_for_folder, CollabType};
use collab_rt_entity::{AckCode, ClientCollabMessage, ServerCollabMessage, ServerInit, UpdateSync};
use collab_rt_protocol::{
  ClientSyncProtocol, CollabSyncProtocol, Message, MessageReader, SyncMessage,
};

use crate::collab_sync::{
  start_sync, CollabSink, MissUpdateReason, SyncError, SyncObject, SyncReason,
};

pub type CollabRef = Weak<RwLock<dyn BorrowMut<Collab> + Send + Sync + 'static>>;

/// Use to continuously receive updates from remote.
pub struct ObserveCollab<Sink, Stream> {
  #[allow(dead_code)]
  object_id: Uuid,
  #[allow(dead_code)]
  weak_collab: CollabRef,
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
    weak_collab: CollabRef,
    sink: Weak<CollabSink<Sink>>,
    periodic_sync_interval: Option<Duration>,
  ) -> Self {
    let object_id = object.object_id;
    let cloned_weak_collab = weak_collab.clone() as CollabRef;
    let seq_num_counter = Arc::new(SeqNumCounter::default());
    let cloned_seq_num_counter = seq_num_counter.clone();
    let init_sync_cancel_token = ArcSwap::new(Arc::new(CancellationToken::new()));
    let arc_object = Arc::new(object);

    if let Some(interval) = periodic_sync_interval {
      tracing::trace!("setting periodic sync step 1 for {}", object_id);
      tokio::spawn(ObserveCollab::<Sink, Stream>::periodic_sync_step_1(
        origin.clone(),
        sink.clone(),
        cloned_weak_collab.clone(),
        interval,
        object_id.to_string(),
      ));
    }
    tokio::spawn(ObserveCollab::<Sink, Stream>::observer_collab_message(
      origin,
      arc_object,
      stream,
      cloned_weak_collab,
      sink,
      cloned_seq_num_counter,
      init_sync_cancel_token,
    ));
    Self {
      object_id,
      weak_collab,
      phantom_sink: Default::default(),
      phantom_stream: Default::default(),
      seq_num_counter,
    }
  }

  /// Periodically run sync step 1 to make sure that there are no missing updates from other clients.
  async fn periodic_sync_step_1(
    origin: CollabOrigin,
    weak_sink: Weak<CollabSink<Sink>>,
    weak_collab: CollabRef,
    interval: Duration,
    object_id: String,
  ) {
    loop {
      tokio::time::sleep(interval).await;
      let sink = match weak_sink.upgrade() {
        Some(sink) => sink,
        None => break,
      };

      let collab = match weak_collab.upgrade() {
        Some(collab) => collab,
        None => break,
      };

      let sv = {
        let lock = collab.read().await;
        let sv = (*lock).borrow().transact().state_vector();
        sv
      };
      let msg = Message::Sync(SyncMessage::SyncStep1(sv)).encode_v1();
      trace!("Periodic sync step 1 for {}", object_id);
      sink.queue_msg(|msg_id| {
        ClientCollabMessage::new_update_sync(UpdateSync::new(
          origin.clone(),
          object_id.clone(),
          msg,
          msg_id,
        ))
      });
    }
  }

  // Spawn the stream that continuously reads the doc's updates from remote.
  async fn observer_collab_message(
    origin: CollabOrigin,
    object: Arc<SyncObject>,
    mut stream: Stream,
    weak_collab: CollabRef,
    weak_sink: Weak<CollabSink<Sink>>,
    seq_num_counter: Arc<SeqNumCounter>,
    cancel_token: ArcSwap<CancellationToken>,
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
            "{} stream error:{}, stop receive incoming changes",
            object.object_id,
            err.into()
          );
          break;
        },
      };

      if let Err(error) = ObserveCollab::<Sink, Stream>::process_remote_message(
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
            let new_cancel_token = Arc::new(CancellationToken::new());
            let old_cancel_token = cancel_token.swap(new_cancel_token.clone());
            old_cancel_token.cancel();

            let cloned_origin = origin.clone();
            let cloned_object = object.clone();
            let collab = collab.clone();
            let sink = sink.clone();
            let sync_reason = match state_vector_v1 {
              None => SyncReason::ClientMissUpdates { reason },
              Some(sv) => SyncReason::ServerMissUpdates {
                state_vector_v1: sv,
                reason,
              },
            };
            tokio::spawn(async move {
              select! {
                _ = new_cancel_token.cancelled() => {
                    trace!("{} cancel pull missing updates", cloned_object.object_id);
                },
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
                   Self::pull_missing_updates(&cloned_origin, &cloned_object, &collab, &sink, sync_reason)
                   .await;
                }
              }
            });
          },
          SyncError::CannotApplyUpdate => {
            let lock = collab.read().await;
            if let Err(err) = start_sync(
              origin.clone(),
              &object,
              (*lock).borrow(),
              &sink,
              SyncReason::ServerCannotApplyUpdate,
            ) {
              error!("Error while start sync: {}", err);
            }
          },
          SyncError::OverrideWithIncorrectData(_) => {
            error!("Error while processing message: {}", error);
            break;
          },
          _ => {
            error!("Error while processing message: {}", error);
          },
        }
      }
    }
  }

  /// Continuously handle messages from the remote doc
  async fn process_remote_message(
    object: &SyncObject,
    collab: &Arc<RwLock<dyn BorrowMut<Collab> + Send + Sync + 'static>>,
    sink: &Arc<CollabSink<Sink>>,
    msg: ServerCollabMessage,
    seq_num_counter: &Arc<SeqNumCounter>,
  ) -> Result<(), SyncError> {
    if cfg!(feature = "sync_verbose_log") {
      trace!("handle server: {}", msg);
    }

    if let ServerCollabMessage::ClientAck(ack) = &msg {
      let ack_code = ack.get_code();
      // if the server can not apply the update, we start the init sync.
      if ack_code == AckCode::CannotApplyUpdate {
        return Err(SyncError::CannotApplyUpdate);
      }

      if ack_code == AckCode::MissUpdate {
        // if the ack code is MissUpdate, it means the server has missed some updates. Client need to
        // use the payload of the current message to calculate missing update. So any existing pending
        // updates are no long needed.
        sink.clear();

        return Err(SyncError::MissUpdates {
          state_vector_v1: Some(ack.payload.to_vec()),
          reason: MissUpdateReason::ServerMissUpdates,
        });
      }
    }

    // msg_id will be None for [ServerBroadcast] or [ServerAwareness].
    match msg.msg_id() {
      None => {
        // apply the broadcast data and then check the continuity of the broadcast sequence number.
        Self::process_message_follow_protocol(object, &msg, collab, sink).await?;
        sink.notify_next();

        if let ServerCollabMessage::ServerBroadcast(ref data) = msg {
          seq_num_counter.check_broadcast_contiguous(&object.object_id, data.seq_num)?;
          seq_num_counter.store_broadcast_seq_num(data.seq_num);
        }
        Ok(())
      },
      Some(msg_id) => {
        let is_valid = sink
          .validate_response(msg_id, &msg, seq_num_counter)
          .await?;

        if is_valid {
          Self::process_message_follow_protocol(object, &msg, collab, sink).await?;
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
    collab: &Arc<RwLock<dyn BorrowMut<Collab> + Send + Sync + 'static>>,
    sink: &Arc<CollabSink<Sink>>,
    reason: SyncReason,
  ) {
    let lock = collab.read().await;
    if let Err(err) = start_sync(origin.clone(), object, (*lock).borrow(), sink, reason) {
      error!("Error while start sync: {}", err);
    }
  }

  async fn process_message_follow_protocol(
    sync_object: &SyncObject,
    msg: &ServerCollabMessage,
    collab: &Arc<RwLock<dyn BorrowMut<Collab> + Send + Sync + 'static>>,
    sink: &Arc<CollabSink<Sink>>,
  ) -> Result<(), SyncError> {
    if msg.payload().is_empty() {
      return Ok(());
    }

    let payload = msg.payload().clone();
    let message_origin = msg.origin().clone();
    let sink = sink.clone();
    let sync_object = sync_object.clone();
    let collab = collab.clone();

    // workaround for panic when applying updates. It can be removed in the future
    let result = tokio::spawn(async move {
      let mut decoder = DecoderV1::new(Cursor::new(&payload));
      let reader = MessageReader::new(&mut decoder);
      for yrs_message in reader {
        let msg = yrs_message?;

        // When the client receives a SyncStep1 message, it indicates that the server is requesting
        // the client to send updates that the server is missing. This typically occurs when the client
        // has been editing offline, resulting in the client's version of the collaboration object
        // being ahead of the server's version. In response, the client prepares to send the missing updates.
        let is_server_sync_step_1 = matches!(msg, Message::Sync(SyncMessage::SyncStep1(_)));

        // If the collaboration object is of type [CollabType::Folder], data validation is required
        // before sending the SyncStep1 to the server.
        if is_server_sync_step_1 && sync_object.collab_type == CollabType::Folder {
          let lock = collab.read().await;
          validate_data_for_folder((*lock).borrow(), &sync_object.workspace_id.to_string())
            .map_err(|err| SyncError::OverrideWithIncorrectData(err.to_string()))?;
        }

        if let Some(return_payload) = ClientSyncProtocol
          .handle_message(&message_origin, &collab, msg)
          .await?
        {
          let object_id = sync_object.object_id;
          sink.queue_msg(|msg_id| {
            if is_server_sync_step_1 {
              ClientCollabMessage::new_server_init_sync(ServerInit::new(
                message_origin.clone(),
                object_id.to_string(),
                return_payload,
                msg_id,
              ))
            } else {
              ClientCollabMessage::new_update_sync(UpdateSync::new(
                message_origin.clone(),
                object_id.to_string(),
                return_payload,
                msg_id,
              ))
            }
          });
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
    _object_id: &Uuid,
    broadcast_seq_num: u32,
  ) -> Result<(), SyncError> {
    let current = self.broadcast_seq_counter.load(Ordering::SeqCst);
    if current > 0 && broadcast_seq_num > current + 1 {
      return Err(SyncError::MissUpdates {
        state_vector_v1: None,
        reason: MissUpdateReason::BroadcastSeqNotContinuous {
          current,
          expected: broadcast_seq_num,
        },
      });
    }

    Ok(())
  }

  pub fn check_ack_broadcast_contiguous(&self, object_id: &Uuid) -> Result<(), SyncError> {
    let ack_seq_num = self.ack_seq_counter.load(Ordering::SeqCst);
    let broadcast_seq_num = self.broadcast_seq_counter.load(Ordering::SeqCst);
    if cfg!(feature = "sync_verbose_log") {
      trace!(
        "receive {} seq_num, ack:{}, broadcast:{}",
        object_id,
        ack_seq_num,
        broadcast_seq_num,
      );
    }

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
          reason: MissUpdateReason::AckSeqAdvanceBroadcastSeq {
            ack_seq: ack_seq_num,
            broadcast_seq: broadcast_seq_num,
          },
        });
      }
    }

    Ok(())
  }
}
