use std::borrow::BorrowMut;
use std::sync::{Arc, Weak};

use anyhow::anyhow;
use bytes::Bytes;
use collab::core::origin::CollabOrigin;
use collab::lock::RwLock;
use collab::preclude::Collab;
use futures_util::{SinkExt, StreamExt};
use tokio::select;
use tokio::sync::broadcast::{channel, Sender};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{error, trace, warn};
use yrs::encoding::write::Write;
use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::Subscription as YrsSubscription;

use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::MessageByObjectId;
use collab_rt_entity::{AckCode, MsgId};
use collab_rt_entity::{
  AwarenessSync, BroadcastSync, ClientCollabMessage, CollabAck, CollabMessage,
};
use collab_rt_protocol::RTProtocolError;
use collab_rt_protocol::{CollabSyncProtocol, Message, MessageReader, MSG_SYNC, MSG_SYNC_UPDATE};

use crate::error::RealtimeError;
use crate::group::group_init::EditState;
use crate::group::protocol::ServerSyncProtocol;
use crate::metrics::CollabRealtimeMetrics;
/// A broadcast can be used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
/// to subscribes. One broadcast can be used to propagate updates for a single document with
/// object_id.
///
pub struct CollabBroadcast {
  object_id: String,
  broadcast_sender: Sender<CollabMessage>,
  awareness_sub: Option<YrsSubscription>,
  /// Keep the lifetime of the document observer subscription. The subscription will be stopped
  /// when the broadcast is dropped.
  doc_subscription: Option<YrsSubscription>,
  edit_state: Arc<EditState>,
  /// The last modified time of the document.
  pub modified_at: Arc<parking_lot::Mutex<Instant>>,
}

unsafe impl Send for CollabBroadcast {}
unsafe impl Sync for CollabBroadcast {}

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
  pub fn new(
    object_id: &str,
    buffer_capacity: usize,
    edit_state: Arc<EditState>,
    collab: &Collab,
  ) -> Self {
    let object_id = object_id.to_owned();
    // broadcast channel
    let (sender, _) = channel(buffer_capacity);
    let mut this = CollabBroadcast {
      object_id,
      broadcast_sender: sender,
      awareness_sub: Default::default(),
      doc_subscription: Default::default(),
      edit_state,
      modified_at: Arc::new(parking_lot::Mutex::new(Instant::now())),
    };
    this.observe_collab_changes(collab);
    this
  }

  fn observe_collab_changes(&mut self, collab: &Collab) {
    let (doc_sub, awareness_sub) = {
      // Observer the document's update and broadcast it to all subscribers.
      let cloned_oid = self.object_id.clone();
      let broadcast_sink = self.broadcast_sender.clone();
      let modified_at = self.modified_at.clone();
      let edit_state = self.edit_state.clone();

      // Observer the document's update and broadcast it to all subscribers. When one of the clients
      // sends an update to the document that alters its state, the document observer will trigger
      // an update event. This event is then broadcast to all connected clients. After broadcasting, all
      // connected clients will receive the update and apply it to their local document state.
      let doc_sub = collab
        .get_awareness()
        .doc()
        .observe_update_v1(move |txn, event| {
          let seq_num = edit_state.increment_edit_count() + 1;
          let origin = CollabOrigin::from(txn);
          trace!(
            "observe update with len:{}, origin: {}",
            event.update.len(),
            origin
          );

          let payload = gen_update_message(&event.update);
          let msg = BroadcastSync::new(origin, cloned_oid.clone(), payload, seq_num);
          if let Err(err) = broadcast_sink.send(msg.into()) {
            trace!("fail to broadcast updates:{}", err);
          }
          *modified_at.lock() = Instant::now();
        })
        .unwrap();

      let broadcast_sink = self.broadcast_sender.clone();
      let cloned_oid = self.object_id.clone();

      // Observer the awareness's update and broadcast it to all subscribers.
      let awareness_sub = collab
        .get_awareness()
        .on_update(move |awareness, event, _origin| {
          if let Ok(awareness_update) = awareness.update_with_clients(event.all_changes()) {
            let payload = Message::Awareness(awareness_update).encode_v1();
            let msg = AwarenessSync::new(cloned_oid.clone(), payload, CollabOrigin::Empty);
            if let Err(err) = broadcast_sink.send(msg.into()) {
              trace!("fail to broadcast awareness:{}", err);
            }
          }
        });
      (doc_sub, awareness_sub)
    };

    self.doc_subscription = Some(doc_sub);
    self.awareness_sub = Some(awareness_sub);
  }

  /// Subscribes a new connection to a broadcast group
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
  #[allow(clippy::too_many_arguments)]
  pub fn subscribe<Sink, Stream>(
    &self,
    user: &RealtimeUser,
    subscriber_origin: CollabOrigin,
    mut sink: Sink,
    mut stream: Stream,
    collab: Weak<RwLock<Collab>>,
    metrics_calculate: Arc<CollabRealtimeMetrics>,
    cancel: CancellationToken,
  ) -> Subscription
  where
    Sink: SinkExt<CollabMessage> + Clone + Send + Sync + Unpin + 'static,
    Stream: StreamExt<Item = MessageByObjectId> + Send + Sync + Unpin + 'static,
    <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error + Send + Sync,
  {
    // the receiver will continue to receive updates from the document observer and forward the update to
    // connected subscriber using its Sink. The loop will break if the stop_rx receives a message.
    let mut receiver = self.broadcast_sender.subscribe();
    let cloned_user = user.clone();
    let cancel_sink = cancel.clone();
    let mut sink2 = sink.clone();
    tokio::spawn(async move {
      loop {
        select! {
          _ = cancel_sink.cancelled() => break,
          result = receiver.recv() => {
            match result {
              Ok(message) => {

                // No need to broadcast the message back to the originator
                if message.origin() == &subscriber_origin {
                  continue;
                }

                trace!("[realtime]: send {} => {}", message, cloned_user.user_device());
                if let Err(err) = sink2.send(message).await {
                  warn!("fail to broadcast message:{}", err);
                }
              }
              Err(_) => {
                // Err(RecvError::Closed) is returned when all Sender halves have dropped,
                // indicating that no further values can be sent on the channel.
                break;
              },
            }
          },
        }
      }
    });

    let user = user.clone();
    let cancel_stream = cancel.clone();
    let object_id = self.object_id.clone();
    let edit_state = self.edit_state.clone();

    // the stream will continue to receive messages from the client and it will stop if the stop_rx
    // receives a message. If the client's message alter the document state, it will trigger the
    // document observer and broadcast the update to all connected subscribers. Check out the [observe_update_v1] and [sink_task] above.
    tokio::spawn(async move {
      loop {
        select! {
          _ = cancel_stream.cancelled() => {
           trace!("stop receiving {} stream from user:{} connect at:{}", object_id, user.uid, user.connect_at);
            break
          },
          result = stream.next() => {
            if result.is_none() {
              trace!("{} stop receiving user:{} messages", object_id, user.user_device());
              break
            }
            let message_map = result.unwrap();
            match collab.upgrade() {
              None => {
                trace!("{} stop receiving user:{} messages because of collab is drop", user.user_device(), object_id);
                // break the loop if the collab is dropped
                break
              },
              Some(collab) => {
                handle_client_messages(&object_id, message_map, &mut sink, collab, &metrics_calculate, &edit_state).await;
              }
            }
          }
        }
      }
    });

    Subscription { cancel }
  }
}

async fn handle_client_messages<Sink>(
  object_id: &str,
  message_by_object_id: MessageByObjectId,
  sink: &mut Sink,
  collab: Arc<RwLock<dyn BorrowMut<Collab> + Send + Sync + 'static>>,
  metrics_calculate: &Arc<CollabRealtimeMetrics>,
  edit_state: &Arc<EditState>,
) where
  Sink: SinkExt<CollabMessage> + Unpin + 'static,
  <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error,
{
  for (message_object_id, collab_messages) in message_by_object_id.into_inner() {
    // Ignore messages where the object_id does not match. This situation should not occur, as
    // [ClientMessageRouter::init_client_communication] is expected to filter out such messages. However,
    // as a precautionary measure, we perform this check to handle any unexpected cases.
    if object_id != message_object_id {
      error!(
        "Expect object id:{} but got:{}",
        object_id, message_object_id
      );
      continue;
    }
    if collab_messages.is_empty() {
      warn!("{} collab messages is empty", object_id);
    }

    for collab_message in collab_messages {
      match handle_one_client_message(
        object_id,
        &collab_message,
        &collab,
        metrics_calculate,
        edit_state,
      )
      .await
      {
        Ok(response) => {
          trace!("[realtime]: sending response: {}", response);
          match sink.send(response.into()).await {
            Ok(()) => {},
            Err(err) => {
              trace!("[realtime]: send failed: {}", err);
              break;
            },
          }
        },
        Err(err) => {
          error!(
            "Error handling collab message for object_id: {}: {}",
            message_object_id, err
          );
          break;
        },
      }
    }
  }
}

/// Handle the message sent from the client
async fn handle_one_client_message(
  object_id: &str,
  collab_msg: &ClientCollabMessage,
  collab: &Arc<RwLock<dyn BorrowMut<Collab> + Send + Sync + 'static>>,
  metrics_calculate: &Arc<CollabRealtimeMetrics>,
  edit_state: &Arc<EditState>,
) -> Result<CollabAck, RealtimeError> {
  let msg_id = collab_msg.msg_id();
  let message_origin = collab_msg.origin().clone();

  // If the payload is empty, we don't need to apply any updates .
  // Currently, only the ping message should has an empty payload.
  if collab_msg.payload().is_empty() {
    return Ok(CollabAck::new(
      message_origin,
      object_id.to_string(),
      msg_id,
      edit_state.edit_count(),
    ));
  }

  trace!(
    "Applying client updates: {}, origin:{}",
    collab_msg,
    message_origin
  );

  handle_one_message_payload(
    object_id,
    message_origin.clone(),
    msg_id,
    collab_msg.payload(),
    collab,
    metrics_calculate,
    edit_state,
  )
  .await
}

/// Handle the message sent from the client
async fn handle_one_message_payload(
  object_id: &str,
  message_origin: CollabOrigin,
  msg_id: MsgId,
  payload: &Bytes,
  collab: &Arc<RwLock<dyn BorrowMut<Collab> + Send + Sync + 'static>>,
  metrics_calculate: &Arc<CollabRealtimeMetrics>,
  edit_state: &Arc<EditState>,
) -> Result<CollabAck, RealtimeError> {
  let payload = payload.clone();
  metrics_calculate.acquire_collab_lock_count.inc();

  // Spawn a blocking task to handle the message
  let result = handle_message(
    &payload,
    &message_origin,
    collab,
    metrics_calculate,
    object_id,
    msg_id,
    edit_state,
  )
  .await;

  match result {
    Ok(inner_result) => match inner_result {
      Some(response) => Ok(response),
      None => Err(RealtimeError::UnexpectedData("No ack response")),
    },
    Err(err) => Err(RealtimeError::Internal(anyhow!(
      "fail to handle message:{}",
      err
    ))),
  }
}

async fn handle_message(
  payload: &Bytes,
  message_origin: &CollabOrigin,
  collab: &Arc<RwLock<dyn BorrowMut<Collab> + Send + Sync + 'static>>,
  metrics_calculate: &Arc<CollabRealtimeMetrics>,
  object_id: &str,
  msg_id: MsgId,
  edit_state: &Arc<EditState>,
) -> Result<Option<CollabAck>, RealtimeError> {
  let mut decoder = DecoderV1::from(payload.as_ref());
  let reader = MessageReader::new(&mut decoder);
  let seq_num = edit_state.edit_count();
  let mut ack_response = None;
  for msg in reader {
    match msg {
      Ok(msg) => {
        match ServerSyncProtocol::new(metrics_calculate.clone())
          .handle_message(message_origin, collab, msg)
          .await
        {
          Ok(payload) => {
            metrics_calculate.apply_update_count.inc();
            // One ClientCollabMessage can have multiple Yrs [Message] in it, but we only need to
            // send one ack back to the client.
            if ack_response.is_none() {
              ack_response = Some(
                CollabAck::new(CollabOrigin::Server, object_id.to_string(), msg_id, seq_num)
                  .with_payload(payload.unwrap_or_default()),
              );
            }
          },
          Err(err) => {
            metrics_calculate.apply_update_failed_count.inc();
            let code = ack_code_from_error(&err);
            let payload = match err {
              RTProtocolError::MissUpdates {
                state_vector_v1,
                reason: _,
              } => state_vector_v1.unwrap_or_default(),
              _ => vec![],
            };

            ack_response = Some(
              CollabAck::new(CollabOrigin::Server, object_id.to_string(), msg_id, seq_num)
                .with_code(code)
                .with_payload(payload),
            );

            break;
          },
        }
      },
      Err(e) => {
        error!("{} => parse sync message failed: {:?}", object_id, e);
        break;
      },
    }
  }

  Ok(ack_response)
}

#[inline]
fn ack_code_from_error(error: &RTProtocolError) -> AckCode {
  match error {
    RTProtocolError::YrsTransaction(_) => AckCode::Retry,
    RTProtocolError::YrsApplyUpdate(_) => AckCode::CannotApplyUpdate,
    RTProtocolError::YrsEncodeState(_) => AckCode::EncodeStateAsUpdateFail,
    RTProtocolError::MissUpdates { .. } => AckCode::MissUpdate,
    _ => AckCode::Internal,
  }
}

/// A subscription structure returned from [CollabBroadcast::subscribe], which represents a
/// subscribed connection. It can be dropped in order to unsubscribe or awaited via
/// [Subscription::stop] method in order to complete of its own volition (due to an internal
/// connection error or closed connection).
#[derive(Debug)]
pub struct Subscription {
  cancel: CancellationToken,
}

impl Drop for Subscription {
  fn drop(&mut self) {
    self.cancel.cancel();
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
