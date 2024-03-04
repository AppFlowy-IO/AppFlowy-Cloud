use collab::core::awareness;
use collab::core::awareness::{Awareness, AwarenessUpdate};
use std::rc::{Rc, Weak};

use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use futures_util::{SinkExt, StreamExt};
use realtime_protocol::{handle_collab_message, Error};
use realtime_protocol::{Message, MessageReader, MSG_SYNC, MSG_SYNC_UPDATE};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::select;
use tokio::sync::broadcast::error::SendError;
use tokio::sync::broadcast::{channel, Sender};
use tokio::sync::Mutex;
use tokio::time::Instant;

use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::UpdateSubscription;

use crate::error::RealtimeError;
use crate::server::collaborate::sync_protocol::ServerSyncProtocol;
use realtime_entity::collab_msg::{
  AckCode, AwarenessSync, BroadcastSync, ClientCollabMessage, CollabAck, CollabMessage,
};
use tracing::{error, trace, warn};
use yrs::encoding::write::Write;

/// A broadcast can be used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
/// to subscribes. One broadcast can be used to propagate updates for a single document with
/// object_id.
///
pub struct CollabBroadcast {
  object_id: String,
  sender: Sender<CollabMessage>,
  awareness_sub: Mutex<Option<awareness::UpdateSubscription>>,
  /// Keep the lifetime of the document observer subscription. The subscription will be stopped
  /// when the broadcast is dropped.
  doc_subscription: Mutex<Option<UpdateSubscription>>,
  broadcast_seq_num_counter: Rc<AtomicU32>,
  /// The last modified time of the document.
  pub modified_at: Rc<parking_lot::Mutex<Instant>>,
}

impl Drop for CollabBroadcast {
  fn drop(&mut self) {
    trace!("Drop collab broadcast:{}", self.object_id);
  }
}

impl CollabBroadcast {
  /// Creates a new [CollabBroadcast] over a provided `collab` instance. All changes triggered
  /// by this collab will be propagated to all subscribers which have been registered via
  /// [CollabBroadcast::subscribe] method.
  ///
  /// The overflow of the incoming events that needs to be propagates will be buffered up to a
  /// provided `buffer_capacity` size.
  pub fn new(object_id: &str, buffer_capacity: usize) -> Self {
    let object_id = object_id.to_owned();
    // broadcast channel
    let (sender, _) = channel(buffer_capacity);
    CollabBroadcast {
      object_id,
      sender,
      awareness_sub: Default::default(),
      doc_subscription: Default::default(),
      broadcast_seq_num_counter: Rc::new(Default::default()),
      modified_at: Rc::new(parking_lot::Mutex::new(Instant::now())),
    }
  }

  pub async fn observe_collab_changes(&self, collab: &mut Collab) {
    let (doc_sub, awareness_sub) = {
      // Observer the document's update and broadcast it to all subscribers.
      let cloned_oid = self.object_id.clone();
      let broadcast_sink = self.sender.clone();
      let seq_num_counter = self.broadcast_seq_num_counter.clone();
      let modified_at = self.modified_at.clone();

      // Observer the document's update and broadcast it to all subscribers. When one of the clients
      // sends an update to the document that alters its state, the document observer will trigger
      // an update event. This event is then broadcast to all connected clients. After broadcasting, all
      // connected clients will receive the update and apply it to their local document state.
      let doc_sub = collab
        .get_mut_awareness()
        .doc_mut()
        .observe_update_v1(move |txn, event| {
          let value = seq_num_counter.fetch_add(1, Ordering::SeqCst);

          let update_len = event.update.len();
          let origin = CollabOrigin::from(txn);
          let payload = gen_update_message(&event.update);
          let msg = BroadcastSync::new(origin, cloned_oid.clone(), payload, value + 1);

          match broadcast_sink.send(msg.into()) {
            Ok(_) => trace!("observe doc update with len:{}", update_len),
            Err(e) => error!(
              "observe doc update with len:{} - broadcast sink fail: {}",
              update_len, e
            ),
          }

          *modified_at.lock() = Instant::now();
        })
        .unwrap();

      let broadcast_sink = self.sender.clone();
      let cloned_oid = self.object_id.clone();

      // Observer the awareness's update and broadcast it to all subscribers.
      let awareness_sub = collab
        .get_mut_awareness()
        .on_update(move |awareness, event| {
          if let Ok(awareness_update) = gen_awareness_update_message(awareness, event) {
            let payload = Message::Awareness(awareness_update).encode_v1();
            let msg = AwarenessSync::new(cloned_oid.clone(), payload);
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

  /// Broadcasts user message to all active subscribers. Returns error if message could not have
  /// been broadcast.
  #[allow(clippy::result_large_err)]
  #[allow(dead_code)]
  pub fn broadcast_awareness(&self, msg: AwarenessSync) -> Result<(), SendError<CollabMessage>> {
    self.sender.send(msg.into())?;
    Ok(())
  }

  /// Subscribes a new connection to a broadcast group, enabling real-time collaboration.
  ///
  /// This function takes a `sink`/`stream` pair representing the connection to a subscriber. The `sink`
  /// is used to send messages to the subscriber, while the `stream` receives messages from the subscriber.
  ///
  /// # Arguments
  /// - `subscriber_origin`: Identifies the subscriber's origin to avoid echoing messages back.
  /// - `sink`: A `Sink` implementation for sending messages to the subscriber(Each connected client).
  /// - `stream`: A `Stream` implementation for receiving messages from the subscriber((Each connected client)).
  ///
  /// # Behavior
  /// - [Sink] Forwards updates received from the document observer to all subscribers through 'sink', excluding the originator
  ///   of the message, to prevent echoing back the same message.
  /// - [Stream] Processes incoming messages from the `stream` associated with the subscriber. If a message alters
  ///   the document's state, it triggers an update broadcast to all subscribers.
  ///
  /// - Utilizes two asynchronous tasks: one for broadcasting updates to the `sink`, and another for
  ///   processing messages from the `stream`.
  ///
  /// # Termination
  /// - The subscription can be manually stopped by dropping the returned `Subscription` structure or
  ///   by awaiting its `stop` method. This action will terminate both the sink and stream tasks.
  /// - Internal errors or disconnection will also terminate the tasks, ending the subscription.
  ///
  /// # Returns
  /// A `Subscription` instance that represents the active subscription. Dropping this structure or
  /// calling its `stop` method will unsubscribe the connection and cease all related activities.
  ///
  pub fn subscribe<Sink, Stream, E>(
    &self,
    subscriber_origin: CollabOrigin,
    mut sink: Sink,
    mut stream: Stream,
    collab: Weak<Mutex<Collab>>,
  ) -> Subscription
  where
    Sink: SinkExt<CollabMessage> + Clone + Send + Sync + Unpin + 'static,
    Stream: StreamExt<Item = Result<ClientCollabMessage, E>> + Send + Sync + Unpin + 'static,
    <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error + Send + Sync,
    E: Into<anyhow::Error> + Send + Sync + 'static,
  {
    let cloned_origin = subscriber_origin.clone();
    trace!("[realtime]: new subscriber: {}", subscriber_origin);
    let sink_stop_tx = {
      let mut sink = sink.clone();
      let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);

      // the receiver will continue to receive updates from the document observer and forward the update to
      // connected subscriber using its Sink. The loop will break if the stop_rx receives a message.
      let mut receiver = self.sender.subscribe();
      tokio::spawn(async move {
        loop {
          select! {
            _ = stop_rx.recv() => break,
            result = receiver.recv() => {
              match result {
                Ok(message) => {
                  if message.origin() == &subscriber_origin {
                    continue;
                  }

                  trace!("[realtime]: send {}", message);
                  if let Err(err) = sink.send(message).await {
                    error!("fail to broadcast message:{}", err);
                  }
                }
                Err( _) => break,
              }
            },
          }
        }
      });
      stop_tx
    };

    let stream_stop_tx = {
      let (stream_stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);
      let object_id = self.object_id.clone();

      // the stream will continue to receive messages from the client and it will stop if the stop_rx
      // receives a message. If the client's message alter the document state, it will trigger the
      // document observer and broadcast the update to all connected subscribers. Check out the [observe_update_v1] and [sink_task] above.
      tokio::task::spawn_local(async move {
        loop {
          select! {
            _ = stop_rx.recv() => break,
            result = stream.next() => {
              match result {
                Some(Ok(collab_msg)) => {
                  match collab.upgrade() {
                    None => break, // break the loop if the collab is dropped
                    Some(collab) => {
                      // The message is valid if it has a payload and the object_id matches the broadcast's object_id.
                      if object_id == collab_msg.object_id() {
                        handle_client_collab_message(&object_id, &mut sink, &collab_msg, &collab).await;
                      } else {
                        warn!("Invalid collab message: {:?}", collab_msg);
                      }
                    }
                  }
                },
                Some(Err(e)) => error!("Error receiving collab message: {:?}", e.into()),
                None => break,
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

/// Handle the message sent from the client
async fn handle_client_collab_message<Sink>(
  object_id: &str,
  sink: &mut Sink,
  collab_msg: &ClientCollabMessage,
  collab: &Mutex<Collab>,
) where
  Sink: SinkExt<CollabMessage> + Unpin + 'static,
  <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error,
{
  let mut decoder = DecoderV1::from(collab_msg.payload().as_ref());
  let origin = collab_msg.origin().clone();
  let reader = MessageReader::new(&mut decoder);
  let mut resps = vec![];
  for msg in reader {
    match msg {
      Ok(msg) => {
        if let Ok(mut collab) = collab.try_lock() {
          let result = handle_collab_message(&origin, &ServerSyncProtocol, &mut collab, msg);
          match result {
            Ok(payload) => {
              let resp = CollabAck::new(origin.clone(), object_id.to_string(), collab_msg.msg_id())
                .with_payload(payload.unwrap_or_default());

              // One ClientCollabMessage can have multiple Yrs [Message] in it, but we only need to
              // send one ack back to the client.
              if resps.is_empty() {
                resps.push(resp);
              }
            },
            Err(err) => {
              error!("handle collab:{} message error:{}", object_id, err);
              let resp = CollabAck::new(origin.clone(), object_id.to_string(), collab_msg.msg_id())
                .with_code(ack_code_from_error(&err));
              resps.push(resp);
            },
          }
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
  for resp in resps {
    trace!("[realtime]: send {}", resp);
    if let Err(err) = sink.send(resp.into()).await {
      trace!("fail to send response to client: {}", err);
    }
  }
}

#[inline]
fn ack_code_from_error(error: &Error) -> AckCode {
  match error {
    Error::YrsTransaction(_) => AckCode::Retry,
    Error::YrsApplyUpdate(_) => AckCode::CannotApplyUpdate,
    _ => AckCode::Internal,
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
      if let Err(err) = sink_stop_tx.send(()).await {
        error!("fail to stop sink:{}", err);
      }
    }
    if let Some(stream_stop_tx) = self.stream_stop_tx.take() {
      if let Err(err) = stream_stop_tx.send(()).await {
        error!("fail to stop stream:{}", err);
      }
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
