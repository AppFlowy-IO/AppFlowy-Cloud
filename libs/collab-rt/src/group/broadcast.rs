use crate::error::RealtimeError;
use crate::group::group_init::{EditState, MutexCollab, WeakMutexCollab};
use crate::group::protocol::ServerSyncProtocol;
use crate::metrics::CollabMetricsCalculate;
use crate::rt_server::COLLAB_RUNTIME;
use anyhow::anyhow;
use collab::core::awareness::{gen_awareness_update_message, AwarenessUpdateSubscription};
use collab::core::origin::CollabOrigin;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::MessageByObjectId;
use collab_rt_entity::{AckCode, MsgId};
use collab_rt_entity::{
  AwarenessSync, BroadcastSync, ClientCollabMessage, CollabAck, CollabMessage,
};
use collab_rt_protocol::{handle_message, RTProtocolError};
use collab_rt_protocol::{Message, MessageReader, MSG_SYNC, MSG_SYNC_UPDATE};
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::select;
use tokio::sync::broadcast::error::SendError;
use tokio::sync::broadcast::{channel, Sender};
use tokio::time::Instant;
use tracing::{error, trace, warn};
use yrs::encoding::write::Write;
use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::UpdateSubscription;

/// A broadcast can be used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
/// to subscribes. One broadcast can be used to propagate updates for a single document with
/// object_id.
///
pub struct CollabBroadcast {
  object_id: String,
  sender: Sender<CollabMessage>,
  awareness_sub: Option<AwarenessUpdateSubscription>,
  /// Keep the lifetime of the document observer subscription. The subscription will be stopped
  /// when the broadcast is dropped.
  doc_subscription: Option<UpdateSubscription>,
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
  pub async fn new(
    object_id: &str,
    buffer_capacity: usize,
    edit_state: Arc<EditState>,
    collab: &MutexCollab,
  ) -> Self {
    let object_id = object_id.to_owned();
    // broadcast channel
    let (sender, _) = channel(buffer_capacity);
    let mut this = CollabBroadcast {
      object_id,
      sender,
      awareness_sub: Default::default(),
      doc_subscription: Default::default(),
      edit_state,
      modified_at: Arc::new(parking_lot::Mutex::new(Instant::now())),
    };
    this.observe_collab_changes(collab).await;
    this
  }

  async fn observe_collab_changes(&mut self, collab: &MutexCollab) {
    let (doc_sub, awareness_sub) = {
      // Observer the document's update and broadcast it to all subscribers.
      let cloned_oid = self.object_id.clone();
      let broadcast_sink = self.sender.clone();
      let modified_at = self.modified_at.clone();
      let edit_state = self.edit_state.clone();

      // Observer the document's update and broadcast it to all subscribers. When one of the clients
      // sends an update to the document that alters its state, the document observer will trigger
      // an update event. This event is then broadcast to all connected clients. After broadcasting, all
      // connected clients will receive the update and apply it to their local document state.
      let doc_sub = collab
        .lock()
        .get_doc()
        .observe_update_v1(move |txn, event| {
          let seq_num = edit_state.increment_edit_count();
          let update_len = event.update.len();
          let origin = CollabOrigin::from(txn);

          let payload = gen_update_message(&event.update);
          let msg = BroadcastSync::new(origin, cloned_oid.clone(), payload, seq_num);

          trace!("collab update with len:{}", update_len);
          if let Err(err) = broadcast_sink.send(msg.into()) {
            trace!("fail to broadcast updates:{}", err);
          }
          *modified_at.lock() = Instant::now();
        })
        .unwrap();

      let broadcast_sink = self.sender.clone();
      let cloned_oid = self.object_id.clone();

      // Observer the awareness's update and broadcast it to all subscribers.
      let awareness_sub = collab.lock().observe_awareness(move |awareness, event| {
        if let Ok(awareness_update) = gen_awareness_update_message(awareness, event) {
          trace!("awareness update:{}", awareness_update);
          let payload = Message::Awareness(awareness_update).encode_v1();
          // TODO(nathan): replace the origin from awareness transaction
          let msg = AwarenessSync::new(cloned_oid.clone(), payload);
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
  pub fn subscribe<Sink, Stream>(
    &self,
    user: &RealtimeUser,
    subscriber_origin: CollabOrigin,
    mut sink: Sink,
    mut stream: Stream,
    collab: WeakMutexCollab,
    metrics_calculate: CollabMetricsCalculate,
  ) -> Subscription
  where
    Sink: SinkExt<CollabMessage> + Clone + Send + Sync + Unpin + 'static,
    Stream: StreamExt<Item = MessageByObjectId> + Send + Sync + Unpin + 'static,
    <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error + Send + Sync,
  {
    let cloned_origin = subscriber_origin.clone();
    let sink_stop_tx = {
      let mut sink = sink.clone();
      let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);

      // the receiver will continue to receive updates from the document observer and forward the update to
      // connected subscriber using its Sink. The loop will break if the stop_rx receives a message.
      let mut receiver = self.sender.subscribe();
      let cloned_user = user.clone();
      COLLAB_RUNTIME.spawn(async move {
        loop {
          select! {
            _ = stop_rx.recv() => break,
            result = receiver.recv() => {
              match result {
                Ok(message) => {
                  if message.origin() == &subscriber_origin {
                    continue;
                  }

                  trace!("[realtime]: send {} => {}", message, cloned_user.user_device());
                  if let Err(err) = sink.send(message).await {
                    error!("fail to broadcast message:{}", err);
                  }
                }
                Err(_) => break,
              }
            },
          }
        }
      });
      stop_tx
    };

    let user = user.clone();
    let stream_stop_tx = {
      let (stream_stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(1);
      let object_id = self.object_id.clone();
      let edit_state = self.edit_state.clone();

      // the stream will continue to receive messages from the client and it will stop if the stop_rx
      // receives a message. If the client's message alter the document state, it will trigger the
      // document observer and broadcast the update to all connected subscribers. Check out the [observe_update_v1] and [sink_task] above.
      COLLAB_RUNTIME.spawn(async move {
        loop {
          select! {
            _ = stop_rx.recv() => {
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

      stream_stop_tx
    };

    Subscription {
      origin: cloned_origin,
      sink_stop_tx: Some(sink_stop_tx),
      stream_stop_tx: Some(stream_stop_tx),
    }
  }
}

async fn handle_client_messages<Sink>(
  object_id: &str,
  message_map: MessageByObjectId,
  sink: &mut Sink,
  collab: MutexCollab,
  metrics_calculate: &CollabMetricsCalculate,
  edit_state: &Arc<EditState>,
) where
  Sink: SinkExt<CollabMessage> + Unpin + 'static,
  <Sink as futures_util::Sink<CollabMessage>>::Error: std::error::Error,
{
  for (message_object_id, collab_messages) in message_map {
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
            Ok(_) => {},
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
  collab: &MutexCollab,
  metrics_calculate: &CollabMetricsCalculate,
  edit_state: &Arc<EditState>,
) -> Result<CollabAck, RealtimeError> {
  let msg_id = collab_msg.msg_id();
  let message_origin = collab_msg.origin().clone();
  let seq_num = edit_state.edit_count();

  // If the payload is empty, we don't need to apply any updates to the document.
  // Currently, only the ping message should has an empty payload.
  if collab_msg.payload().is_empty() {
    if !matches!(collab_msg, ClientCollabMessage::ClientPingSync(_)) {
      error!("receive unexpected empty payload message:{}", collab_msg);
    }
    let resp = CollabAck::new(message_origin, object_id.to_string(), msg_id, seq_num);
    Ok(resp)
  } else {
    trace!(
      "Applying client updates: {}, origin:{}",
      collab_msg,
      message_origin
    );
    let ack = handle_one_message_payload(
      object_id,
      message_origin,
      msg_id,
      collab_msg.payload(),
      collab,
      metrics_calculate,
      seq_num,
    )
    .await?;

    update_last_sync_at(collab);
    Ok(ack)
  }
}

/// Handle the message sent from the client
async fn handle_one_message_payload(
  object_id: &str,
  message_origin: CollabOrigin,
  msg_id: MsgId,
  payload: &[u8],
  collab: &MutexCollab,
  metrics_calculate: &CollabMetricsCalculate,
  seq_num: u32,
) -> Result<CollabAck, RealtimeError> {
  let mut decoder = DecoderV1::from(payload);
  let reader = MessageReader::new(&mut decoder);
  let mut ack_response = None;

  metrics_calculate
    .acquire_collab_lock_count
    .fetch_add(1, Ordering::Relaxed);

  let mut collab_lock = match collab.try_lock() {
    Some(collab) => collab,
    None => {
      metrics_calculate
        .acquire_collab_lock_fail_count
        .fetch_add(1, Ordering::Relaxed);

      return Err(RealtimeError::Internal(anyhow!(
        "fail to lock collab:{}",
        object_id,
      )));
    },
  };

  for msg in reader {
    match msg {
      Ok(msg) => {
        let result = handle_message(&message_origin, &ServerSyncProtocol, &mut collab_lock, msg);
        match result {
          Ok(payload) => {
            metrics_calculate
              .apply_update_count
              .fetch_add(1, Ordering::Relaxed);
            // One ClientCollabMessage can have multiple Yrs [Message] in it, but we only need to
            // send one ack back to the client.
            if ack_response.is_none() {
              let resp = CollabAck::new(
                message_origin.clone(),
                object_id.to_string(),
                msg_id,
                seq_num,
              )
              .with_payload(payload.unwrap_or_default());
              ack_response = Some(resp);
            }
          },
          Err(err) => {
            metrics_calculate
              .apply_update_failed_count
              .fetch_add(1, Ordering::Relaxed);
            if ack_response.is_none() {
              let resp = CollabAck::new(
                message_origin.clone(),
                object_id.to_string(),
                msg_id,
                seq_num,
              )
              .with_code(ack_code_from_error(&err));
              ack_response = Some(resp);
            }
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
  let response = ack_response.ok_or_else(|| RealtimeError::UnexpectedData("No ack response"))?;
  Ok(response)
}

#[inline]
fn ack_code_from_error(error: &RTProtocolError) -> AckCode {
  match error {
    RTProtocolError::YrsTransaction(_) => AckCode::Retry,
    RTProtocolError::YrsApplyUpdate(_) => AckCode::CannotApplyUpdate,
    RTProtocolError::YrsEncodeState(_) => AckCode::EncodeState,
    RTProtocolError::MissUpdates(_) => AckCode::RequireInitSync,
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
        warn!(
          "fail to stop sink:{}, the stream might be already stop",
          err
        );
      }
    }
    if let Some(stream_stop_tx) = self.stream_stop_tx.take() {
      if let Err(err) = stream_stop_tx.send(()).await {
        warn!(
          "fail to stop stream:{}, the stream might be already stop",
          err
        );
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
fn update_last_sync_at(collab: &MutexCollab) {
  if let Some(collab) = collab.try_lock() {
    collab.set_last_sync_at(chrono::Utc::now().timestamp());
  }
}
