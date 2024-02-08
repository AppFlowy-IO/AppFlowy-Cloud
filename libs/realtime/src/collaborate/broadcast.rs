use collab::core::awareness;
use std::sync::Arc;

use crate::collaborate::sync_protocol::ServerSyncProtocol;
use collab::core::awareness::{Awareness, AwarenessUpdate};
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use futures_util::{SinkExt, StreamExt};
use realtime_protocol::handle_collab_message;
use realtime_protocol::{Message, MessageReader, MSG_SYNC, MSG_SYNC_UPDATE};
use tokio::select;
use tokio::sync::broadcast::error::SendError;
use tokio::sync::broadcast::{channel, Sender};
use tokio::sync::Mutex;
use tokio::time::Instant;
use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::UpdateSubscription;

use crate::collaborate::retry::SinkCollabMessageAction;
use crate::error::RealtimeError;
use realtime_entity::collab_msg::{CollabAck, CollabAwareness, CollabBroadcastData, CollabMessage};
use tracing::{error, trace, warn};
use yrs::encoding::write::Write;

/// A broadcast can be used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
/// to subscribes. One broadcast can be used to propagate updates for a single document with
/// object_id.
pub struct CollabBroadcast {
  object_id: String,
  collab: MutexCollab,
  sender: Sender<CollabMessage>,
  awareness_sub: Mutex<Option<awareness::UpdateSubscription>>,
  doc_subscription: Mutex<Option<UpdateSubscription>>,
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
    // broadcast channel
    let (sender, _) = channel(buffer_capacity);
    CollabBroadcast {
      object_id,
      collab,
      sender,
      awareness_sub: Default::default(),
      doc_subscription: Default::default(),
    }
  }

  pub async fn observe_collab_changes(&self) {
    let (doc_sub, awareness_sub) = {
      let mut mutex_collab = self.collab.lock();

      // Observer the document's update and broadcast it to all subscribers.
      let cloned_oid = self.object_id.clone();
      let broadcast_sink = self.sender.clone();
      let doc_sub = mutex_collab
        .get_mut_awareness()
        .doc_mut()
        .observe_update_v1(move |txn, event| {
          let update_len = event.update.len();
          let origin = CollabOrigin::from(txn);
          let payload = gen_update_message(&event.update);
          let msg = CollabBroadcastData::new(origin, cloned_oid.clone(), payload);

          match broadcast_sink.send(msg.into()) {
            Ok(_) => trace!("observe doc update with len:{}", update_len),
            Err(e) => error!(
              "observe doc update with len:{} - broadcast sink fail: {}",
              update_len, e
            ),
          }
        })
        .unwrap();

      let broadcast_sink = self.sender.clone();
      let cloned_oid = self.object_id.clone();

      // Observer the awareness's update and broadcast it to all subscribers.
      let awareness_sub = mutex_collab
        .get_mut_awareness()
        .on_update(move |awareness, event| {
          if let Ok(awareness_update) = gen_awareness_update_message(awareness, event) {
            let payload = Message::Awareness(awareness_update).encode_v1();
            let msg = CollabAwareness::new(cloned_oid.clone(), payload);
            if let Err(_e) = broadcast_sink.send(msg.into()) {
              trace!("Broadcast group is closed");
            }
          }
        });
      (doc_sub, awareness_sub)
    };

    *self.doc_subscription.lock().await = Some(doc_sub);
    *self.awareness_sub.lock().await = Some(awareness_sub);
  }

  /// Returns a reference to an underlying [MutexCollab] instance.
  pub fn collab(&self) -> &MutexCollab {
    &self.collab
  }

  /// Broadcasts user message to all active subscribers. Returns error if message could not have
  /// been broadcast.
  #[allow(clippy::result_large_err)]
  pub fn broadcast_awareness(&self, msg: CollabAwareness) -> Result<(), SendError<CollabMessage>> {
    self.sender.send(msg.into())?;
    Ok(())
  }

  /// Subscribes a new connection - represented by `sink`/`stream` pair implementing a futures
  /// Sink and Stream protocols - to a current broadcast group.
  ///
  /// Returns a subscription structure, which can be dropped in order to unsubscribe or awaited
  /// via [Subscription::stop] method in order to complete of its own volition (due to
  /// an internal connection error or closed connection).
  pub fn subscribe<Sink, Stream, E>(
    &self,
    subscriber_origin: CollabOrigin,
    sink: Sink,
    mut stream: Stream,
    modified_at: Arc<Mutex<Instant>>,
  ) -> Subscription
  where
    Sink: SinkExt<CollabMessage> + Send + Sync + Unpin + 'static,
    Stream: StreamExt<Item = Result<CollabMessage, E>> + Send + Sync + Unpin + 'static,
    <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error + Send + Sync,
    E: Into<anyhow::Error> + Send + Sync + 'static,
  {
    let cloned_origin = subscriber_origin.clone();
    trace!("[realtime]: new subscriber: {}", subscriber_origin);
    let sink = Arc::new(Mutex::new(sink));
    // Receive a update from the document observer and forward the  update to all
    // connected subscribers using its Sink.
    let sink_stop_tx = {
      let sink = sink.clone();
      let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);
      let mut receiver = self.sender.subscribe();
      tokio::spawn(async move {
        loop {
          select! {
            _ = stop_rx.recv() => break,
            Ok(message) = receiver.recv() => {
              if let Some(msg_origin) = message.origin() {
                if msg_origin == &subscriber_origin {
                  continue;
                }
              }

              trace!("[realtime]: broadcast collab message: {}", message);
              let action = SinkCollabMessageAction {
                sink: &sink,
                message,
              };
              if let Err(err) = action.run().await {
                error!("fail to broadcast message:{}", err);
              }
            },
          }
        }
      });
      stop_tx
    };

    // Receive messages from clients and reply with the response. The message may alter the
    // document that the current broadcast group is associated with. If the message alter
    // the document state then the document observer will be triggered and the update will be
    // broadcast to all connected subscribers. Check out the [observe_update_v1] and [sink_task]
    // above.
    let stream_stop_tx = {
      let collab = self.collab().clone();
      let object_id = self.object_id.clone();
      let (stream_stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);
      tokio::spawn(async move {
        loop {
          select! {
             _ = stop_rx.recv() => break,
            Some(Ok(collab_msg)) = stream.next() => {
              // Continue if the message is empty
              let payload = collab_msg.payload();
              if payload.is_none() {
                warn!("Unexpected empty payload of collab message:{}", collab_msg);
                continue;
              }

              let _collab_msg_origin = collab_msg.origin();
              if object_id != collab_msg.object_id() {
                error!("Incoming message's object id does not match the broadcast group's object id");
                continue;
              }

              handle_user_collab_message(&object_id, &sink, &collab_msg, &collab).await;

              if let Ok(mut modified_at) = modified_at.try_lock() {
               *modified_at = Instant::now();
              }
            }
          }
        }
      });
      stream_stop_tx
    };

    Subscription {
      origin: cloned_origin,
      sink_stop_tx: Some(sink_stop_tx),
      stream_stop_tx: Some(stream_stop_tx),
    }
  }
}

async fn handle_user_collab_message<Sink>(
  object_id: &str,
  sink: &Arc<Mutex<Sink>>,
  collab_msg: &CollabMessage,
  collab: &MutexCollab,
) where
  Sink: SinkExt<CollabMessage> + Send + Sync + Unpin + 'static,
  <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error + Send + Sync,
{
  match collab_msg.payload() {
    None => {},
    Some(payload) => {
      let object_id = object_id.to_string();
      let mut decoder = DecoderV1::from(payload.as_ref());
      let origin = Arc::new(collab_msg.origin().cloned());
      let reader = MessageReader::new(&mut decoder);
      for msg in reader {
        match msg {
          Ok(msg) => {
            let cloned_collab = collab.clone();
            let cloned_origin = origin.clone();
            let result = tokio::task::spawn_blocking(move || {
              handle_collab_message(&cloned_origin, &ServerSyncProtocol, &cloned_collab, msg)
            })
            .await;

            match result {
              Ok(Ok(payload)) => match origin.as_ref() {
                None => warn!("Client message does not have a origin"),
                Some(origin) => {
                  if let Some(msg_id) = collab_msg.msg_id() {
                    let resp = CollabAck::new(
                      origin.clone(),
                      object_id.clone(),
                      payload.unwrap_or_default(),
                      msg_id,
                      collab_msg.type_str(),
                    );

                    trace!("Send response to client: {}", resp);
                    match sink.try_lock() {
                      Ok(mut sink) => {
                        if let Err(err) = sink.send(resp.into()).await {
                          trace!("fail to send response to client: {}", err);
                        }
                      },
                      Err(err) => error!("Requires sink lock failed: {:?}", err),
                    }
                  }
                },
              },
              Ok(Err(err)) => {
                error!("object id:{} =>{}", object_id, err);
              },
              Err(err) => {
                error!("internal error when handle user ws message: {}", err);
              },
            }
          },
          Err(e) => {
            error!(
              "object id:{} => parser sync message failed: {:?}",
              object_id, e
            );
            break;
          },
        }
      }
    },
  }
}

/// A subscription structure returned from [CollabBroadcast::subscribe], which represents a
/// subscribed connection. It can be dropped in order to unsubscribe or awaited via
/// [Subscription::stop] method in order to complete of its own volition (due to an internal
/// connection error or closed connection).
#[derive(Debug)]
pub struct Subscription {
  pub origin: CollabOrigin,
  sink_stop_tx: Option<tokio::sync::mpsc::Sender<()>>,
  stream_stop_tx: Option<tokio::sync::mpsc::Sender<()>>,
}

impl Subscription {
  pub async fn stop(&mut self) {
    if let Some(sink_stop_tx) = self.sink_stop_tx.take() {
      let _ = sink_stop_tx.send(()).await;
    }
    if let Some(stream_stop_tx) = self.stream_stop_tx.take() {
      let _ = stream_stop_tx.send(()).await;
    }
  }
}

impl Drop for Subscription {
  fn drop(&mut self) {
    if self.stream_stop_tx.is_some() || self.stream_stop_tx.is_some() {
      error!("Subscription is not stopped before dropping");
    }
  }
}

/// Generates a message: Message::Sync::(SyncMessage::Update(update))
#[inline]
fn gen_update_message(update: &[u8]) -> Vec<u8> {
  let mut encoder = EncoderV1::new();
  // write the tag for Message::Sync
  encoder.write_var(MSG_SYNC);
  // write the tag for SyncMessage::Update
  encoder.write_var(MSG_SYNC_UPDATE);
  encoder.write_buf(update);
  encoder.to_vec()
}

#[inline]
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
