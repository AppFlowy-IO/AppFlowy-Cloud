use crate::collaborate::group_control::CollabGroupControl;
use crate::collaborate::group_sub::{CollabUserMessage, SubscribeGroup};
use crate::collaborate::{broadcast_message, CollabAccessControl, CollabClientStream};
use crate::entities::{Editing, RealtimeUser};
use crate::error::RealtimeError;
use anyhow::anyhow;
use async_stream::stream;
use dashmap::DashMap;
use database::collab::CollabStorage;
use futures_util::StreamExt;
use realtime_entity::collab_msg::{CollabMessage, CollabSinkMessage};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{error, trace};

pub enum GroupCommand<U> {
  HandleCollabMessage {
    user: U,
    collab_message: CollabMessage,
  },
}

pub type GroupControlCommandSender<U> = tokio::sync::mpsc::Sender<GroupCommand<U>>;
pub type GroupControlCommandReceiver<U> = tokio::sync::mpsc::Receiver<GroupCommand<U>>;

pub struct GroupCommandRunner<S, U, AC> {
  pub group_control: Arc<CollabGroupControl<S, U, AC>>,
  pub client_stream_by_user: Arc<DashMap<U, CollabClientStream>>,
  pub edit_collab_by_user: Arc<DashMap<U, HashSet<Editing>>>,
  pub access_control: Arc<AC>,
  pub recv: Option<GroupControlCommandReceiver<U>>,
}

impl<S, U, AC> GroupCommandRunner<S, U, AC>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  pub async fn run(mut self) {
    let mut receiver = self.recv.take().expect("Only take once");
    let stream = stream! {
      while let Some(msg) = receiver.recv().await {
         yield msg;
      }
      trace!("The group command runner is stopped");
    };

    stream
      .for_each(|command| async {
        match command {
          GroupCommand::HandleCollabMessage {
            user,
            collab_message,
          } => {
            if let Err(err) = self.handle_collab_message(user, collab_message).await {
              error!("Failed to handle collab message: {}", err);
            }
          },
        }
      })
      .await;
  }

  /// When processing a message from a client, the server performs the following actions:
  /// 1. Check if the client is connected to the websocket server.
  /// 2. Handle message sent by the client if the message is [CollabMessage]
  ///   2.1 If the message is init sync and the group exists, remove the old subscriber and subscribe the user to the group.
  ///   2.2 If the message is init sync and the group is not exist, create a new group.
  ///   2.3 If the message is not init sync, send the message to the group and then broadcast the message to all connected clients.
  ///   2.4 If the message is not init sync and the group is not exist. Ask client to send the init message first.
  async fn handle_collab_message(
    &self,
    user: U,
    collab_message: CollabMessage,
  ) -> Result<(), RealtimeError> {
    // 1.Check the client is connected with the websocket server
    if self.client_stream_by_user.get(&user).is_none() {
      let msg = anyhow!("The client stream: {} is not found, it should be created when the client is connected with this websocket server", user);
      return Err(RealtimeError::Internal(msg));
    }

    let is_group_exist = self
      .group_control
      .contains_group(collab_message.object_id())
      .await;
    if is_group_exist {
      // 2.1 If a group exists for the specified object_id and the message is an 'init sync',
      // then remove any existing subscriber from that group and add the new user as a subscriber to the group.
      if collab_message.is_init_msg() {
        self
          .group_control
          .remove_user(collab_message.object_id(), &user)
          .await?;
      }

      // 2.1.1 subscribe the user to the group. then the user will receive the changes from the
      // group
      let is_user_subscribed = self
        .group_control
        .contains_user(collab_message.object_id(), &user)
        .await;
      if !is_user_subscribed {
        self.subscribe_group(&user, &collab_message).await?;
      }
      // 2.3 If the message is not init sync, send the message to the group and then broadcast
      // the message to all connected clients.
      broadcast_message(&user, collab_message, &self.client_stream_by_user).await;
    } else {
      // 2.2 If there is no existing group for the given object_id and the message is an 'init message',
      // then create a new group and add the user as a subscriber to this group.
      if collab_message.is_init_msg() {
        // 2.2.1 create group
        self.create_group(&collab_message).await?;

        // 2.2.2 subscribe the user to the group
        self.subscribe_group(&user, &collab_message).await?;
      } else {
        // 2.4 If the group is not exist and the message is not init sync, then the server should
        // ask the client to send the init message first
        // TODO(nathan): ask the client to send the init message first
      }
    }
    Ok(())
  }

  async fn subscribe_group(
    &self,
    user: &U,
    collab_message: &CollabMessage,
  ) -> Result<(), RealtimeError> {
    SubscribeGroup {
      message: &CollabUserMessage {
        user,
        collab_message,
      },
      groups: &self.group_control,
      edit_collab_by_user: &self.edit_collab_by_user,
      client_stream_by_user: &self.client_stream_by_user,
      access_control: &self.access_control,
    }
    .run()
    .await;
    Ok(())
  }
  async fn create_group(&self, collab_message: &CollabMessage) -> Result<(), RealtimeError> {
    let object_id = collab_message.object_id();
    match collab_message {
      CollabMessage::ClientInitSync(client_init) => {
        let uid = client_init
          .origin
          .client_user_id()
          .ok_or(RealtimeError::ExpectInitSync(
            "The client user id is empty".to_string(),
          ))?;

        self
          .group_control
          .create_group(
            uid,
            &client_init.workspace_id,
            object_id,
            client_init.collab_type.clone(),
          )
          .await;

        Ok(())
      },
      _ => Err(RealtimeError::ExpectInitSync(collab_message.to_string())),
    }
  }
}
