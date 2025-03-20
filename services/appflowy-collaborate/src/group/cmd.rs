use crate::client::client_msg_router::ClientMessageRouter;
use crate::error::RealtimeError;
use crate::group::manager::GroupManager;
use crate::group::null_sender::NullSender;
use async_stream::stream;
use bytes::Bytes;
use collab::core::origin::{CollabClient, CollabOrigin};
use collab::entity::EncodedCollab;
use dashmap::DashMap;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;

use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::CollabAck;
use collab_rt_entity::{
  AckCode, ClientCollabMessage, MessageByObjectId, ServerCollabMessage, SinkMessage, UpdateSync,
};
use collab_rt_protocol::{Message, SyncMessage};
use database::collab::CollabStorage;
use tracing::{error, instrument, trace, warn};
use uuid::Uuid;
use yrs::updates::encoder::Encode;
use yrs::StateVector;

/// Using [GroupCommand] to interact with the group
/// - HandleClientCollabMessage: Handle the client message
/// - EncodeCollab: Encode the collab
/// - HandleServerCollabMessage: Handle the server message
pub enum GroupCommand {
  HandleClientCollabMessage {
    user: RealtimeUser,
    object_id: Uuid,
    collab_messages: Vec<ClientCollabMessage>,
    ret: tokio::sync::oneshot::Sender<Result<(), RealtimeError>>,
  },
  HandleClientHttpUpdate {
    user: RealtimeUser,
    workspace_id: Uuid,
    object_id: Uuid,
    update: Bytes,
    collab_type: CollabType,
    ret: tokio::sync::oneshot::Sender<Result<(), RealtimeError>>,
  },
  EncodeCollab {
    object_id: Uuid,
    ret: tokio::sync::oneshot::Sender<Option<EncodedCollab>>,
  },
  HandleServerCollabMessage {
    object_id: Uuid,
    collab_messages: Vec<ClientCollabMessage>,
    ret: tokio::sync::oneshot::Sender<Result<(), RealtimeError>>,
  },
  GenerateCollabEmbedding {
    object_id: Uuid,
  },
  CalculateMissingUpdate {
    object_id: Uuid,
    state_vector: StateVector,
    ret: tokio::sync::oneshot::Sender<Result<Vec<u8>, RealtimeError>>,
  },
}

pub type GroupCommandSender = tokio::sync::mpsc::Sender<GroupCommand>;
pub type GroupCommandReceiver = tokio::sync::mpsc::Receiver<GroupCommand>;

/// Each group has a command runner to handle the group command. GroupCommandRunner is designed to run
/// in tokio multi-thread runtime. It will receive the group command from the receiver and handle the
/// command.
///
pub struct GroupCommandRunner<S>
where
  S: CollabStorage,
{
  pub group_manager: Arc<GroupManager<S>>,
  pub msg_router_by_user: Arc<DashMap<RealtimeUser, ClientMessageRouter>>,
  pub recv: Option<GroupCommandReceiver>,
}

impl<S> GroupCommandRunner<S>
where
  S: CollabStorage,
{
  pub async fn run(mut self, object_id: Uuid) {
    let mut receiver = self.recv.take().expect("Only take once");
    let stream = stream! {
      while let Some(msg) = receiver.recv().await {
         yield msg;
      }
      trace!("Collab group:{} command runner is stopped", object_id);
    };
    stream
      .for_each(|command| async {
        match command {
          GroupCommand::HandleClientCollabMessage {
            user,
            object_id,
            collab_messages,
            ret,
          } => {
            let result = self
              .handle_client_collab_message(&user, object_id, collab_messages)
              .await;
            if let Err(err) = ret.send(result) {
              warn!("Send handle client collab message result fail: {:?}", err);
            }
          },
          GroupCommand::EncodeCollab { object_id, ret } => {
            let group = self.group_manager.get_group(&object_id).await;
            if let Err(_err) = match group {
              None => ret.send(None),
              Some(group) => ret.send(group.encode_collab().await.ok()),
            } {
              warn!("Send encode collab fail");
            }
          },
          GroupCommand::HandleServerCollabMessage {
            object_id,
            collab_messages,
            ret,
          } => {
            let res = self
              .handle_server_collab_messages(object_id, collab_messages)
              .await;
            if let Err(err) = ret.send(res) {
              warn!("Send handle server collab message result fail: {:?}", err);
            }
          },
          GroupCommand::HandleClientHttpUpdate {
            user,
            workspace_id,
            object_id,
            update,
            collab_type,
            ret,
          } => {
            let result = self
              .handle_client_posted_http_update(&user, workspace_id, object_id, collab_type, update)
              .await;
            if let Err(err) = ret.send(result) {
              warn!("Send handle client update message result fail: {:?}", err);
            }
          },
          GroupCommand::GenerateCollabEmbedding { object_id } => {
            if let Some(group) = self.group_manager.get_group(&object_id).await {
              match group.generate_embeddings().await {
                Ok(_) => trace!("successfully created embeddings for {}", object_id),
                Err(err) => trace!("failed to create embeddings for {}: {}", object_id, err),
              }
            }
          },
          GroupCommand::CalculateMissingUpdate {
            object_id,
            state_vector,
            ret,
          } => {
            let group = self.group_manager.get_group(&object_id).await;
            match group {
              None => {
                let _ = ret.send(Err(RealtimeError::GroupNotFound(object_id.to_string())));
              },
              Some(group) => {
                let result = group.calculate_missing_update(state_vector).await;
                let _ = ret.send(result);
              },
            }
          },
        }
      })
      .await;
  }

  /// Processes a client message with the following logic:
  /// 1. Verifies client connection to the websocket server.
  /// 2. Processes [CollabMessage] messages as follows:
  ///    2.1 For 'init sync' messages:
  ///      - If the group exists: Removes the old subscription and re-subscribes the user.
  ///      - If the group does not exist: Creates a new group.
  ///      In both cases, the message is then sent to the group for synchronization according to [CollabSyncProtocol],
  ///      which includes broadcasting to all connected clients.
  ///    2.2 For non-'init sync' messages:
  ///      - If the group exists: The message is sent to the group for synchronization as per [CollabSyncProtocol].
  ///      - If the group does not exist: The client is prompted to send an 'init sync' message first.
  #[instrument(level = "trace", skip_all)]
  async fn handle_client_collab_message(
    &self,
    user: &RealtimeUser,
    object_id: Uuid,
    messages: Vec<ClientCollabMessage>,
  ) -> Result<(), RealtimeError> {
    if messages.is_empty() {
      warn!("Unexpected empty collab messages sent from client");
      return Ok(());
    }
    // 1.Check the client is connected with the websocket server.
    if self.msg_router_by_user.get(user).is_none() {
      // 1. **Client Not Connected**: This case occurs when there is an attempt to interact with a
      // WebSocket server, but the client has not established a connection with the server. The action
      // or message intended for the server cannot proceed because there is no active connection.
      // 2. **Duplicate Connections from the Same Device**: When a client from the same device attempts
      // to establish a new WebSocket connection while a previous connection from that device already
      // exists, the new connection will supersede and replace the old one.
      trace!("The client stream: {} is not found, it should be created when the client is connected with this websocket server", user);
      return Ok(());
    }

    let is_group_exist = self.group_manager.contains_group(&object_id);
    if is_group_exist {
      // subscribe the user to the group. then the user will receive the changes from the group
      let is_user_subscribed = self.group_manager.contains_user(&object_id, user);
      if !is_user_subscribed {
        // safety: messages is not empty because we have checked it before
        let first_message = messages.first().unwrap();
        self
          .subscribe_group_with_message(user, first_message)
          .await?;
      }
      forward_message_to_group(user, object_id, messages, &self.msg_router_by_user).await;
    } else {
      let first_message = messages.first().unwrap();
      // If there is no existing group for the given object_id and the message is an 'init message',
      // then create a new group and add the user as a subscriber to this group.
      if first_message.is_client_init_sync() {
        self.create_group_with_message(user, first_message).await?;
        self
          .subscribe_group_with_message(user, first_message)
          .await?;
        forward_message_to_group(user, object_id, messages, &self.msg_router_by_user).await;
      } else if let Some(entry) = self.msg_router_by_user.get(user) {
        warn!(
          "The group:{} is not found, the client:{} should send the init message first",
          first_message.object_id(),
          user
        );
        let origin = first_message.origin().clone();
        let msg_id = first_message.msg_id();
        let object_id = first_message.object_id().to_string();

        // when the group with given id is not found and the the first message is not init sync.
        // Return AckCode::CannotApplyUpdate to the client and then client will send the init sync message.
        let ack =
          CollabAck::new(origin, object_id, msg_id, 0).with_code(AckCode::CannotApplyUpdate);
        entry
          .value()
          .send_message(ServerCollabMessage::ClientAck(ack).into())
          .await;
      }
    }
    Ok(())
  }

  /// This functions will be called when client post update via http requset
  #[instrument(level = "trace", skip_all)]
  async fn handle_client_posted_http_update(
    &self,
    user: &RealtimeUser,
    workspace_id: Uuid,
    object_id: Uuid,
    collab_type: collab_entity::CollabType,
    update: Bytes,
  ) -> Result<(), RealtimeError> {
    let origin = CollabOrigin::Client(CollabClient {
      uid: user.uid,
      device_id: user.device_id.clone(),
    });

    // Create message router for user if it's not exist
    let is_router_exists = self.msg_router_by_user.get(user).is_some();
    if !is_router_exists {
      trace!("create a new client message router for user:{}", user);
      let new_client_router = ClientMessageRouter::new(NullSender::<()>::default());
      self
        .msg_router_by_user
        .insert(user.clone(), new_client_router);
    }

    // Create group if it's not exist
    let is_group_exist = self.group_manager.contains_group(&object_id);
    if !is_group_exist {
      trace!("The group:{} is not found, create a new group", object_id);
      self
        .create_group(user, workspace_id, object_id, collab_type)
        .await?;
    }

    // Only subscribe when the user is not subscribed to the group
    if !self.group_manager.contains_user(&object_id, user) {
      self.subscribe_group(user, object_id, &origin).await?;
    }
    if let Some(client_stream) = self.msg_router_by_user.get(user) {
      let payload = Message::Sync(SyncMessage::Update(update.to_vec())).encode_v1();
      let msg = ClientCollabMessage::ClientUpdateSync {
        data: UpdateSync {
          origin,
          object_id: object_id.to_string(),
          msg_id: chrono::Utc::now().timestamp_millis() as u64,
          payload: payload.into(),
        },
      };
      let message = MessageByObjectId::new_with_message(object_id.to_string(), vec![msg]);
      let err = client_stream.stream_tx.send(message);
      if let Err(err) = err {
        warn!("Send user:{} http update message to group:{}", user, err);
        self.msg_router_by_user.remove(user);
      }
    } else {
      warn!(
        "The client stream: {} is not found when applying client update",
        user
      );
    }
    Ok(())
  }

  /// similar to `handle_client_collab_message`, but the messages are sent from the server instead.
  #[instrument(level = "trace", skip_all)]
  async fn handle_server_collab_messages(
    &self,
    object_id: Uuid,
    messages: Vec<ClientCollabMessage>,
  ) -> Result<(), RealtimeError> {
    if messages.is_empty() {
      warn!("Unexpected empty collab messages sent from server");
      return Ok(());
    }

    let server_rt_user = RealtimeUser {
      uid: 0,
      device_id: "server".to_string(),
      connect_at: chrono::Utc::now().timestamp_millis(),
      session_id: uuid::Uuid::new_v4().to_string(),
      app_version: "".to_string(),
    };

    if let Some(group) = self.group_manager.get_group(&object_id).await {
      let (mut message_by_oid_sender, message_by_oid_receiver) = futures::channel::mpsc::channel(1);
      group.subscribe(
        &server_rt_user,
        CollabOrigin::Server,
        NullSender::default(),
        message_by_oid_receiver,
      );
      let message = HashMap::from([(object_id.to_string(), messages)]);
      if let Err(err) = message_by_oid_sender.try_send(MessageByObjectId(message)) {
        error!(
          "failed to send message to group: {}, object_id: {}",
          err, object_id
        );
      }
    };

    Ok(())
  }

  async fn subscribe_group_with_message(
    &self,
    user: &RealtimeUser,
    collab_message: &ClientCollabMessage,
  ) -> Result<(), RealtimeError> {
    let object_id = Uuid::parse_str(collab_message.object_id())?;
    let message_origin = collab_message.origin();
    self.subscribe_group(user, object_id, message_origin).await
  }

  async fn subscribe_group(
    &self,
    user: &RealtimeUser,
    object_id: Uuid,
    collab_origin: &CollabOrigin,
  ) -> Result<(), RealtimeError> {
    match self.msg_router_by_user.get_mut(user) {
      None => {
        warn!("The client stream: {} is not found", user);
        Ok(())
      },
      Some(mut client_msg_router) => {
        self
          .group_manager
          .subscribe_group(
            user,
            object_id,
            collab_origin,
            client_msg_router.value_mut(),
          )
          .await
      },
    }
  }

  #[instrument(level = "debug", skip_all)]
  async fn create_group_with_message(
    &self,
    user: &RealtimeUser,
    collab_message: &ClientCollabMessage,
  ) -> Result<(), RealtimeError> {
    let object_id = Uuid::parse_str(collab_message.object_id())?;
    match collab_message {
      ClientCollabMessage::ClientInitSync { data, .. } => {
        let workspace_id = Uuid::parse_str(&data.workspace_id)?;
        self
          .create_group(user, workspace_id, object_id, data.collab_type.clone())
          .await?;
        Ok(())
      },
      _ => Err(RealtimeError::ExpectInitSync(collab_message.to_string())),
    }
  }

  #[instrument(level = "debug", skip_all)]
  async fn create_group(
    &self,
    user: &RealtimeUser,
    workspace_id: Uuid,
    object_id: Uuid,
    collab_type: collab_entity::CollabType,
  ) -> Result<(), RealtimeError> {
    self
      .group_manager
      .create_group(user, workspace_id, object_id, collab_type)
      .await?;

    Ok(())
  }
}

/// Forward the message to the group.
/// When the group receives the message, it will broadcast the message to all the users in the group.
#[inline]
pub async fn forward_message_to_group(
  user: &RealtimeUser,
  object_id: Uuid,
  collab_messages: Vec<ClientCollabMessage>,
  client_msg_router: &Arc<DashMap<RealtimeUser, ClientMessageRouter>>,
) {
  if let Some(client_stream) = client_msg_router.get(user) {
    trace!(
      "[realtime]: receive client:{} device:{} oid:{} msg ids: {:?}",
      user.uid,
      user.device_id,
      object_id,
      collab_messages
        .iter()
        .map(|v| v.msg_id())
        .collect::<Vec<_>>()
    );
    let message = MessageByObjectId::new_with_message(object_id.to_string(), collab_messages);
    let err = client_stream.stream_tx.send(message);
    if let Err(err) = err {
      warn!("Send user:{} message to group:{}", user.uid, err);
      client_msg_router.remove(user);
    }
  }
}
