use crate::{
  error::RealtimeError,
  group::cmd::{GroupCommand, GroupCommandSender},
};
use collab::entity::EncodedCollab;
use collab_rt_entity::ClientCollabMessage;
use dashmap::DashMap;
use std::sync::Arc;
use tracing::error;

pub type CLCommandSender = tokio::sync::mpsc::Sender<CollaborationCommand>;
pub type CLCommandReceiver = tokio::sync::mpsc::Receiver<CollaborationCommand>;

pub type EncodeCollabSender = tokio::sync::oneshot::Sender<Option<EncodedCollab>>;
pub enum CollaborationCommand {
  GetEncodeCollab {
    object_id: String,
    ret: EncodeCollabSender,
  },
  ServerSendCollabMessage {
    object_id: String,
    collab_messages: Vec<ClientCollabMessage>,
    ret: tokio::sync::oneshot::Sender<Result<(), RealtimeError>>,
  },
}

pub(crate) fn spawn_collaboration_command(
  mut command_recv: CLCommandReceiver,
  group_sender_by_object_id: &Arc<DashMap<String, GroupCommandSender>>,
) {
  let group_sender_by_object_id = group_sender_by_object_id.clone();
  tokio::spawn(async move {
    while let Some(cmd) = command_recv.recv().await {
      match cmd {
        CollaborationCommand::GetEncodeCollab { object_id, ret } => {
          match group_sender_by_object_id.get(&object_id) {
            Some(sender) => {
              if let Err(err) = sender
                .send(GroupCommand::EncodeCollab {
                  object_id: object_id.clone(),
                  ret,
                })
                .await
              {
                error!("Send group command error: {}", err);
              }
            },
            None => {
              let _ = ret.send(None);
            },
          }
        },
        CollaborationCommand::ServerSendCollabMessage {
          object_id,
          collab_messages,
          ret,
        } => {
          if let Some(sender) = group_sender_by_object_id.get(&object_id) {
            if let Err(err) = sender
              .send(GroupCommand::HandleServerCollabMessage {
                object_id,
                collab_messages,
                ret,
              })
              .await
            {
              tracing::error!("Send group command error: {}", err);
            };
          }
        },
      }
    }
  });
}
