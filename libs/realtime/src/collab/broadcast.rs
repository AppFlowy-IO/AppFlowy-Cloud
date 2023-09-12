use std::sync::Arc;

use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use futures_util::{SinkExt, StreamExt};
use lib0::encoding::Write;
use tokio::select;
use tokio::sync::broadcast::error::SendError;
use tokio::sync::broadcast::{channel, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use y_sync::awareness;
use y_sync::awareness::{Awareness, AwarenessUpdate};
use y_sync::sync::{Message, MessageReader, SyncMessage, MSG_SYNC, MSG_SYNC_UPDATE};
use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{ReadTxn, UpdateSubscription};

use crate::error::{internal_error, RealtimeError};
use collab_sync_protocol::{handle_msg, DefaultSyncProtocol};
use collab_sync_protocol::{
  CSAwarenessUpdate, CollabMessage, CollabServerAck, CollabServerBroadcast, CollabServerResponse,
};

/// A broadcast can be used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
/// to subscribes. One broadcast can be used to propagate updates for a single document with
/// object_id.
pub struct CollabBroadcast {
  object_id: String,
  collab: MutexCollab,
  sender: Sender<CollabMessage>,

  #[allow(dead_code)]
  awareness_sub: awareness::UpdateSubscription,
  #[allow(dead_code)]
  doc_sub: UpdateSubscription,
}

impl CollabBroadcast {
  /// Creates a new [CollabBroadcast] over a provided `collab` instance. All changes triggered
  /// by this collab will be propagated to all subscribers which have been registered via
  /// [CollabBroadcast::subscribe] method.
  ///
  /// The overflow of the incoming events that needs to be propagates will be buffered up to a
  /// provided `buffer_capacity` size.
  pub fn new(object_id: &str, collab: MutexCollab, buffer_capacity: usize) -> Self {
    let object_id = object_id.to_owned();
    let (sender, _) = channel(buffer_capacity);
    let (doc_sub, awareness_sub) = {
      let mut mutex_collab = collab.lock();

      // Observer the document's update and broadcast it to all subscribers.
      let cloned_oid = object_id.clone();
      let sink = sender.clone();
      let doc_sub = mutex_collab
        .get_mut_awareness()
        .doc_mut()
        .observe_update_v1(move |txn, event| {
          let origin = CollabOrigin::from(txn);
          let payload = gen_update_message(&event.update);
          let msg = CollabServerBroadcast::new(origin, cloned_oid.clone(), payload);
          if let Err(_e) = sink.send(msg.into()) {
            tracing::trace!("Broadcast group is closed");
          }
        })
        .unwrap();

      let sink = sender.clone();
      let cloned_oid = object_id.clone();

      // Observer the awareness's update and broadcast it to all subscribers.
      let awareness_sub = mutex_collab
        .get_mut_awareness()
        .on_update(move |awareness, event| {
          if let Ok(awareness_update) = gen_awareness_update_message(awareness, event) {
            let payload = Message::Awareness(awareness_update).encode_v1();
            let msg = CSAwarenessUpdate::new(cloned_oid.clone(), payload);
            if let Err(_e) = sink.send(msg.into()) {
              tracing::trace!("Broadcast group is closed");
            }
          }
        });
      (doc_sub, awareness_sub)
    };
    CollabBroadcast {
      object_id,
      collab,
      sender,
      awareness_sub,
      doc_sub,
    }
  }

  /// Returns a reference to an underlying [MutexCollab] instance.
  pub fn collab(&self) -> &MutexCollab {
    &self.collab
  }

  /// Broadcasts user message to all active subscribers. Returns error if message could not have
  /// been broadcast.
  #[allow(clippy::result_large_err)]
  pub fn broadcast_awareness(
    &self,
    msg: CSAwarenessUpdate,
  ) -> Result<(), SendError<CollabMessage>> {
    self.sender.send(msg.into())?;
    Ok(())
  }

  /// Subscribes a new connection - represented by `sink`/`stream` pair implementing a futures
  /// Sink and Stream protocols - to a current broadcast group.
  ///
  /// Returns a subscription structure, which can be dropped in order to unsubscribe or awaited
  /// via [Subscription::completed] method in order to complete of its own volition (due to
  /// an internal connection error or closed connection).
  pub fn subscribe<Sink, Stream, E>(
    &self,
    origin: CollabOrigin,
    sink: Sink,
    mut stream: Stream,
  ) -> Subscription
  where
    Sink: SinkExt<CollabMessage> + Send + Sync + Unpin + 'static,
    Stream: StreamExt<Item = Result<CollabMessage, E>> + Send + Sync + Unpin + 'static,
    <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error + Send + Sync,
    E: std::error::Error + Send + Sync + 'static,
  {
    tracing::trace!("[💭Server]: new subscriber: {}", origin);
    let sink = Arc::new(Mutex::new(sink));
    // Receive a update from the document observer and forward the applied update to all
    // connected subscribers using its Sink.
    let sink_task = {
      let sink = sink.clone();
      let mut receiver = self.sender.subscribe();
      tokio::spawn(async move {
        while let Ok(msg) = receiver.recv().await {
          // No need to broadcast the message back to the origin.
          if let Some(msg_origin) = msg.origin() {
            if msg_origin == &origin {
              continue;
            }
          }

          tracing::trace!("[💭Server]: {}", msg);
          let mut sink = sink.lock().await;
          if let Err(e) = sink.send(msg).await {
            tracing::error!("[💭Server]: broadcast client message failed: {:?}", e);
            return Err(RealtimeError::Internal(anyhow::Error::from(e)));
          }
        }
        Ok(())
      })
    };

    // Receive messages from clients and reply with the response. The message may alter the
    // document that the current broadcast group is associated with. If the message alter
    // the document state then the document observer will be triggered and the update will be
    // broadcast to all connected subscribers. Check out the [observe_update_v1] and [sink_task]
    // above.
    let stream_task = {
      let collab = self.collab().clone();
      let object_id = self.object_id.clone();
      tokio::spawn(async move {
        while let Some(res) = stream.next().await {
          let collab_msg = res.map_err(internal_error)?;
          // Continue if the message is empty
          if collab_msg.is_empty() {
            continue;
          }

          let origin = collab_msg.origin();
          let is_client_init = collab_msg.is_init();

          if object_id != collab_msg.object_id() {
            tracing::error!("[🔴Server]: Incoming message's object id does not match the broadcast group's object id");
            continue;
          }
          tracing::debug!("[💭Server]: {}", collab_msg,);
          let payload = collab_msg.payload().unwrap();
          let mut decoder = DecoderV1::from(payload.as_ref());
          let mut sink = sink.lock().await;
          let reader = MessageReader::new(&mut decoder);
          for msg in reader {
            match msg {
              Ok(msg) => {
                let resp = handle_msg(&origin, &DefaultSyncProtocol, &collab, msg).await?;
                // Send the response to the corresponding client
                if let Some(resp) = resp {
                  let msg =
                    CollabServerResponse::new(origin.cloned(), object_id.clone(), resp.encode_v1());
                  sink.send(msg.into()).await.map_err(internal_error)?;
                }
              },
              _ => break,
            }
          }

          if let Some(msg_id) = collab_msg.msg_id() {
            // Send the server's state vector to the client. The client will calculate the missing
            // updates and send them as a single update back to the server.
            let payload = if is_client_init {
              Some(encode_server_sv(&collab))
            } else {
              None
            };

            // Send the ack message to the client
            let ack = CollabServerAck::new(object_id.clone(), msg_id, payload);
            let _ = sink.send(ack.into()).await;
          }
        }
        Ok(())
      })
    };

    Subscription {
      sink_task,
      stream_task,
    }
  }
}

fn encode_server_sv(collab: &MutexCollab) -> Vec<u8> {
  let mut encoder = EncoderV1::new();
  let sv = collab.lock().transact().state_vector();
  Message::Sync(SyncMessage::SyncStep1(sv)).encode(&mut encoder);
  encoder.to_vec()
}

/// A subscription structure returned from [CollabBroadcast::subscribe], which represents a
/// subscribed connection. It can be dropped in order to unsubscribe or awaited via
/// [Subscription::completed] method in order to complete of its own volition (due to an internal
/// connection error or closed connection).
#[derive(Debug)]
pub struct Subscription {
  sink_task: JoinHandle<Result<(), RealtimeError>>,
  stream_task: JoinHandle<Result<(), RealtimeError>>,
}

impl Subscription {
  /// Consumes current subscription, waiting for it to complete. If an underlying connection was
  /// closed because of failure, an error which caused it to happen will be returned.
  ///
  /// This method doesn't invoke close procedure. If you need that, drop current subscription instead.
  pub async fn completed(self) -> Result<(), RealtimeError> {
    let res = select! {
        r1 = self.sink_task => r1?,
        r2 = self.stream_task => r2?,
    };
    res
  }
}

fn gen_update_message(update: &[u8]) -> Vec<u8> {
  let mut encoder = EncoderV1::new();
  encoder.write_var(MSG_SYNC);
  encoder.write_var(MSG_SYNC_UPDATE);
  encoder.write_buf(update);
  encoder.to_vec()
}

fn gen_awareness_update_message(
  awareness: &Awareness,
  event: &awareness::Event,
) -> Result<AwarenessUpdate, RealtimeError> {
  let added = event.added();
  let updated = event.updated();
  let removed = event.removed();
  let mut changed = Vec::with_capacity(added.len() + updated.len() + removed.len());
  changed.extend_from_slice(added);
  changed.extend_from_slice(updated);
  changed.extend_from_slice(removed);
  let update = awareness.update_with_clients(changed)?;
  Ok(update)
}
