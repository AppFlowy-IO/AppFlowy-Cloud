use crate::collaborate::all_group::AllGroup;
use crate::error::RealtimeError;
use crate::{CollabClientStream, RealtimeAccessControl};

use async_stream::stream;
use collab::core::collab_plugin::EncodedCollab;
use collab_rt_entity::collab_msg::{ClientCollabMessage, CollabMessage, CollabSinkMessage};
use dashmap::DashMap;
use database::collab::CollabStorage;
use futures_util::StreamExt;

use collab_rt_entity::message::RealtimeMessage;
use collab_rt_entity::user::{Editing, RealtimeUser};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{error, instrument, trace, warn};

pub enum GroupCommand {
  HandleClientCollabMessage {
    user: RealtimeUser,
    object_id: String,
    collab_messages: Vec<ClientCollabMessage>,
  },
  EncodeCollab {
    object_id: String,
    ret: tokio::sync::oneshot::Sender<Option<EncodedCollab>>,
  },
}

pub type GroupCommandSender = tokio::sync::mpsc::Sender<GroupCommand>;
pub type GroupCommandReceiver = tokio::sync::mpsc::Receiver<GroupCommand>;

pub struct GroupCommandRunner<S, AC> {
  pub all_groups: Arc<AllGroup<S, AC>>,
  pub client_stream_by_user: Arc<DashMap<RealtimeUser, CollabClientStream>>,
  pub edit_collab_by_user: Arc<DashMap<RealtimeUser, HashSet<Editing>>>,
  pub access_control: Arc<AC>,
  pub recv: Option<GroupCommandReceiver>,
}

impl<S, AC> GroupCommandRunner<S, AC>
where
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  pub async fn run(mut self, object_id: String) {
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
          } => {
            if let Err(err) = self
              .handle_client_collab_message(&user, object_id, collab_messages)
              .await
            {
              error!("handle client message error: {}", err);
            }
          },
          GroupCommand::EncodeCollab { object_id, ret } => {
            let group = self.all_groups.get_group(&object_id).await;
            if let Err(_err) = match group {
              None => ret.send(None),
              Some(group) => ret.send(Some(group.encode_collab().await)),
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
    if self.client_stream_by_user.get(user).is_none() {
      // 1. **Client Not Connected**: This case occurs when there is an attempt to interact with a
      // WebSocket server, but the client has not established a connection with the server. The action
      // or message intended for the server cannot proceed because there is no active connection.
      // 2. **Duplicate Connections from the Same Device**: When a client from the same device attempts
      // to establish a new WebSocket connection while a previous connection from that device already
      // exists, the new connection will supersede and replace the old one.
      trace!("The client stream: {} is not found, it should be created when the client is connected with this websocket server", user);
      return Ok(());
    }

    let is_group_exist = self.all_groups.contains_group(&object_id).await;
    if is_group_exist {
      // subscribe the user to the group. then the user will receive the changes from the group
      let is_user_subscribed = self.all_groups.contains_user(&object_id, user).await;
      if !is_user_subscribed {
        // safety: messages is not empty because we have checked it before
        let first_message = messages.first().unwrap();
        self.subscribe_group(user, first_message).await?;
      }
      forward_message_to_group(user, object_id, messages, &self.client_stream_by_user).await;
    } else {
      let first_message = messages.first().unwrap();
      // If there is no existing group for the given object_id and the message is an 'init message',
      // then create a new group and add the user as a subscriber to this group.
      if first_message.is_client_init_sync() {
        self.create_group(first_message).await?;
        self.subscribe_group(user, first_message).await?;
        forward_message_to_group(user, object_id, messages, &self.client_stream_by_user).await;
      } else {
        warn!(
          "The group:{} is not found, the client:{} should send the init message first",
          first_message.object_id(),
          user
        );
        // TODO(nathan): ask the client to send the init message first
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
    let origin = collab_message.origin();
    if let Some(mut client_stream) = self.client_stream_by_user.get_mut(user) {
      if let Some(collab_group) = self.all_groups.get_group(object_id).await {
        if !collab_group.contains_user(user) {
          trace!(
            "[realtime]: {} subscribe group:{}",
            user,
            collab_message.object_id()
          );

          self
            .edit_collab_by_user
            .entry((*user).clone())
            .or_default()
            .insert(Editing {
              object_id: object_id.to_string(),
              origin: origin.clone(),
            });

          let (sink, stream) = client_stream
            .value_mut()
            .client_channel::<CollabMessage, _>(
              &collab_group.workspace_id,
              user,
              object_id,
              self.access_control.clone(),
            );

          collab_group
            .subscribe(user, origin.clone(), sink, stream)
            .await;
        }
      }
    } else {
      warn!("The client stream: {} is not found", user);
    }

    Ok(())
  }
  async fn create_group(&self, collab_message: &ClientCollabMessage) -> Result<(), RealtimeError> {
    let object_id = collab_message.object_id();
    match collab_message {
      ClientCollabMessage::ClientInitSync { data, .. } => {
        let uid = data
          .origin
          .client_user_id()
          .ok_or(RealtimeError::ExpectInitSync(
            "The client user id is empty".to_string(),
          ))?;

        self
          .all_groups
          .create_group(uid, &data.workspace_id, object_id, data.collab_type.clone())
          .await;

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
  client_streams: &Arc<DashMap<RealtimeUser, CollabClientStream>>,
) {
  if let Some(client_stream) = client_streams.get(user) {
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
      warn!("Send user:{} message to group error: {}", user.uid, err,);
      client_streams.remove(user);
    }
  }
}
