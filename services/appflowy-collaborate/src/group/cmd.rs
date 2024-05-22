use std::sync::Arc;

use async_stream::stream;
use collab::entity::EncodedCollab;
use dashmap::DashMap;
use futures_util::StreamExt;
use tracing::{instrument, trace, warn};

use access_control::collab::RealtimeAccessControl;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::{ClientCollabMessage, ServerCollabMessage, SinkMessage};
use collab_rt_entity::{CollabAck, RealtimeMessage};
use database::collab::CollabStorage;

use crate::client::client_msg_router::ClientMessageRouter;
use crate::error::RealtimeError;
use crate::group::manager::GroupManager;

/// Using [GroupCommand] to interact with the group
/// - HandleClientCollabMessage: Handle the client message
/// - EncodeCollab: Encode the collab
pub enum GroupCommand {
  HandleClientCollabMessage {
    user: RealtimeUser,
    object_id: String,
    collab_messages: Vec<ClientCollabMessage>,
    ret: tokio::sync::oneshot::Sender<Result<(), RealtimeError>>,
  },
  EncodeCollab {
    object_id: String,
    ret: tokio::sync::oneshot::Sender<Option<EncodedCollab>>,
  },
}

pub type GroupCommandSender = tokio::sync::mpsc::Sender<GroupCommand>;
pub type GroupCommandReceiver = tokio::sync::mpsc::Receiver<GroupCommand>;

/// Each group has a command runner to handle the group command. GroupCommandRunner is designed to run
/// in tokio multi-thread runtime. It will receive the group command from the receiver and handle the
/// command.
///
pub struct GroupCommandRunner<S, AC>
where
  AC: RealtimeAccessControl,
  S: CollabStorage,
{
  pub group_manager: Arc<GroupManager<S, AC>>,
  pub msg_router_by_user: Arc<DashMap<RealtimeUser, ClientMessageRouter>>,
  pub access_control: Arc<AC>,
  pub recv: Option<GroupCommandReceiver>,
}

impl<S, AC> GroupCommandRunner<S, AC>
where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  pub async fn run(mut self, object_id: String, notify: Arc<tokio::sync::Notify>) {
    let mut receiver = self.recv.take().expect("Only take once");
    let stream = stream! {
      while let Some(msg) = receiver.recv().await {
         yield msg;
      }
      trace!("Collab group:{} command runner is stopped", object_id);
    };
    notify.notify_one();
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
    object_id: String,
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

    let is_group_exist = self.group_manager.contains_group(&object_id).await;
    if is_group_exist {
      // subscribe the user to the group. then the user will receive the changes from the group
      let is_user_subscribed = self.group_manager.contains_user(&object_id, user).await;
      if !is_user_subscribed {
        // safety: messages is not empty because we have checked it before
        let first_message = messages.first().unwrap();
        self.subscribe_group(user, first_message).await?;
      }
      forward_message_to_group(user, object_id, messages, &self.msg_router_by_user).await;
    } else {
      let first_message = messages.first().unwrap();
      // If there is no existing group for the given object_id and the message is an 'init message',
      // then create a new group and add the user as a subscriber to this group.
      if first_message.is_client_init_sync() {
        self.create_group(user, first_message).await?;
        self.subscribe_group(user, first_message).await?;
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
        let ack = CollabAck::new(origin, object_id, msg_id, 0);
        entry
          .value()
          .send_message(ServerCollabMessage::ClientAck(ack).into())
          .await;
      }
    }
    Ok(())
  }

  async fn subscribe_group(
    &self,
    user: &RealtimeUser,
    collab_message: &ClientCollabMessage,
  ) -> Result<(), RealtimeError> {
    let object_id = collab_message.object_id();
    let message_origin = collab_message.origin();
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
            message_origin,
            client_msg_router.value_mut(),
          )
          .await
      },
    }
  }

  #[instrument(level = "info", skip_all)]
  async fn create_group(
    &self,
    user: &RealtimeUser,
    collab_message: &ClientCollabMessage,
  ) -> Result<(), RealtimeError> {
    let object_id = collab_message.object_id();
    match collab_message {
      ClientCollabMessage::ClientInitSync { data, .. } => {
        self
          .group_manager
          .create_group(
            user,
            &data.workspace_id,
            object_id,
            data.collab_type.clone(),
          )
          .await?;

        Ok(())
      },
      _ => Err(RealtimeError::ExpectInitSync(collab_message.to_string())),
    }
  }
}

/// Forward the message to the group.
/// When the group receives the message, it will broadcast the message to all the users in the group.
#[inline]
pub async fn forward_message_to_group(
  user: &RealtimeUser,
  object_id: String,
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
    let pair = (object_id, collab_messages);
    let err = client_stream
      .stream_tx
      .send(RealtimeMessage::ClientCollabV2([pair].into()));
    if let Err(err) = err {
      warn!("Send user:{} message to group:{}", user.uid, err);
      client_msg_router.remove(user);
    }
  }
}
