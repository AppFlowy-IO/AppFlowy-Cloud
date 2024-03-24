use crate::util::channel_ext::UnboundedSenderSink;
use crate::RealtimeAccessControl;
use async_trait::async_trait;
use collab_rt_entity::collab_msg::{ClientCollabMessage, CollabSinkMessage};
use collab_rt_entity::message::{MessageByObjectId, RealtimeMessage};
use collab_rt_entity::user::RealtimeUser;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::StreamExt;
use tracing::{error, trace};

#[async_trait]
pub trait RealtimeClientWebsocketSink: Send + Sync + 'static {
  fn do_send(&self, message: RealtimeMessage);
}

/// Manages message routing for client connections in a collaborative environment.
///
/// acts as an intermediary that receives messages from individual client sessions and
/// forwards them to the appropriate destination, either by broadcasting to all connected
/// clients or directing them to a specific client. It leverages a websocket sink for outgoing
/// messages and a broadcast channel to receive incoming messages from the collab server.
pub struct ClientMessageRouter {
  pub(crate) sink: Arc<dyn RealtimeClientWebsocketSink>,
  /// Used to receive messages from the collab server. The message will forward to the [CollabBroadcast] which
  /// will broadcast the message to all connected clients.
  ///
  /// The message flow:
  /// ClientSession(websocket) -> [CollabRealtimeServer] -> [ClientMessageRouter] -> [CollabBroadcast] 1->* websocket(client)
  pub(crate) stream_tx: tokio::sync::broadcast::Sender<RealtimeMessage>,
}

impl ClientMessageRouter {
  pub fn new(sink: impl RealtimeClientWebsocketSink) -> Self {
    // When receive a new connection, create a new [ClientStream] that holds the connection's websocket
    let (stream_tx, _) = tokio::sync::broadcast::channel(1000);
    Self {
      sink: Arc::new(sink),
      stream_tx,
    }
  }

  /// Initializes a communication channel for a client and a specific collaboration object.
  ///
  /// sets up a two-way communication channel between the client and the collaboration server,
  /// tailored for a specific object identified by `object_id`. It establishes a mechanism for sending changes to
  /// and receiving updates from the collaboration object, ensuring that only authorized changes are communicated.
  ///
  /// - An `UnboundedSenderSink<T>`, which is used to send updates to the connected client.
  /// - A `ReceiverStream<MessageByObjectId>`, which is used to receive authorized updates from the connected client.
  ///
  pub fn init_client_communication<T, AC>(
    &mut self,
    workspace_id: &str,
    user: &RealtimeUser,
    object_id: &str,
    access_control: Arc<AC>,
  ) -> (UnboundedSenderSink<T>, ReceiverStream<MessageByObjectId>)
  where
    T: Into<RealtimeMessage> + Send + Sync + 'static,
    AC: RealtimeAccessControl,
  {
    let client_ws_sink = self.sink.clone();
    let mut stream_rx = BroadcastStream::new(self.stream_tx.subscribe());
    let cloned_object_id = object_id.to_string();

    // Send the message to the connected websocket client. When the client receive the message,
    // it will apply the changes.
    let (client_sink_tx, mut client_sink_rx) = tokio::sync::mpsc::unbounded_channel::<T>();
    let sink_access_control = access_control.clone();
    let sink_workspace_id = workspace_id.to_string();
    let uid = user.uid;
    let client_sink = UnboundedSenderSink::<T>::new(client_sink_tx);
    tokio::spawn(async move {
      while let Some(msg) = client_sink_rx.recv().await {
        let result = sink_access_control
          .can_read_collab(&sink_workspace_id, &uid, &cloned_object_id)
          .await;
        match result {
          Ok(is_allowed) => {
            if is_allowed {
              let rt_msg = msg.into();
              client_ws_sink.do_send(rt_msg);
            } else {
              trace!(
                "user:{} is not allow to observe {} changes",
                uid,
                cloned_object_id
              );
              // when then client is not allowed to receive messages
              tokio::time::sleep(Duration::from_secs(2)).await;
            }
          },
          Err(err) => {
            error!("user:{} fail to receive updates: {}", uid, err);
            tokio::time::sleep(Duration::from_secs(1)).await;
          },
        }
      }
    });
    let cloned_object_id = object_id.to_string();
    let stream_workspace_id = workspace_id.to_string();
    let user = user.clone();
    // stream_rx continuously receive messages from the websocket client and then
    // forward the message to the subscriber which is the broadcast channel [CollabBroadcast].
    let (recv_client_msg, rx) = tokio::sync::mpsc::channel(100);
    let client_stream = ReceiverStream::new(rx);
    tokio::spawn(async move {
      while let Some(Ok(realtime_msg)) = stream_rx.next().await {
        match realtime_msg.transform() {
          Ok(messages_by_oid) => {
            for (msg_oid, original_messages) in messages_by_oid {
              if cloned_object_id != msg_oid {
                continue;
              }

              let (valid_messages, invalid_message) = Self::access_control(
                &stream_workspace_id,
                &user.uid,
                &msg_oid,
                &access_control,
                original_messages,
              )
              .await;
              trace!(
                "{} receive client:{}, device:{}, message: valid:{} invalid:{}",
                msg_oid,
                user.uid,
                user.device_id,
                valid_messages.len(),
                invalid_message.len()
              );

              if valid_messages.is_empty() {
                continue;
              }

              // if tx.send return error, it means the client is disconnected from the group
              if let Err(err) = recv_client_msg
                .send([(msg_oid, valid_messages)].into())
                .await
              {
                trace!(
                  "{} send message to user:{} stream fail with error: {}, break the loop",
                  cloned_object_id,
                  user.user_device(),
                  err,
                );
                return;
              }
            }
          },
          Err(err) => {
            if cfg!(debug_assertions) {
              error!("parse client message error: {}", err);
            }
          },
        }
      }
    });
    (client_sink, client_stream)
  }

  #[inline]
  async fn access_control<AC>(
    workspace_id: &str,
    uid: &i64,
    object_id: &str,
    access_control: &Arc<AC>,
    messages: Vec<ClientCollabMessage>,
  ) -> (Vec<ClientCollabMessage>, Vec<ClientCollabMessage>)
  where
    AC: RealtimeAccessControl,
  {
    let can_write = access_control
      .can_write_collab(workspace_id, uid, object_id)
      .await
      .unwrap_or(false);

    let mut valid_messages = Vec::with_capacity(messages.len());
    let mut invalid_messages = Vec::with_capacity(messages.len());
    let can_read = access_control
      .can_read_collab(workspace_id, uid, object_id)
      .await
      .unwrap_or(false);

    for message in messages {
      if message.is_client_init_sync() && can_read {
        valid_messages.push(message);
        continue;
      }

      if can_write {
        valid_messages.push(message);
      } else {
        invalid_messages.push(message);
      }
    }
    (valid_messages, invalid_messages)
  }
}
