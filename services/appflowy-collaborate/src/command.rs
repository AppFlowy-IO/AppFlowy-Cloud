use crate::{
  error::RealtimeError,
  group::{
    cmd::{GroupCommand, GroupCommandSender},
    manager::GroupManager,
  },
};
use collab::entity::EncodedCollab;
use collab_rt_entity::ClientCollabMessage;
use dashmap::DashMap;
use database::collab::CollabStorage;
use futures::StreamExt;
use std::{
  collections::HashMap,
  sync::{Arc, Weak},
};
use tracing::error;
use uuid::Uuid;

pub type CLCommandSender = tokio::sync::mpsc::Sender<CollaborationCommand>;
pub type CLCommandReceiver = tokio::sync::mpsc::Receiver<CollaborationCommand>;

pub type EncodeCollabSender = tokio::sync::oneshot::Sender<Option<EncodedCollab>>;
pub type BatchEncodeCollabSender = tokio::sync::oneshot::Sender<HashMap<Uuid, EncodedCollab>>;
pub enum CollaborationCommand {
  GetEncodeCollab {
    object_id: Uuid,
    ret: EncodeCollabSender,
  },
  BatchGetEncodeCollab {
    object_ids: Vec<Uuid>,
    ret: BatchEncodeCollabSender,
  },
  ServerSendCollabMessage {
    object_id: Uuid,
    collab_messages: Vec<ClientCollabMessage>,
    ret: tokio::sync::oneshot::Sender<Result<(), RealtimeError>>,
  },
}

const BATCH_GET_ENCODE_COLLAB_CONCURRENCY: usize = 10;

pub(crate) fn spawn_collaboration_command<S>(
  mut command_recv: CLCommandReceiver,
  group_sender_by_object_id: &Arc<DashMap<Uuid, GroupCommandSender>>,
  weak_groups: Weak<GroupManager<S>>,
) where
  S: CollabStorage,
{
  let group_sender_by_object_id = group_sender_by_object_id.clone();
  tokio::spawn(async move {
    while let Some(cmd) = command_recv.recv().await {
      match cmd {
        CollaborationCommand::GetEncodeCollab { object_id, ret } => {
          match group_sender_by_object_id.get(&object_id) {
            Some(sender) => {
              if let Err(err) = sender
                .send(GroupCommand::EncodeCollab { object_id, ret })
                .await
              {
                error!("Send group command error: {}", err);
              }
            },
            None => {
              tracing::trace!("no active collab group for {} has been found", object_id);
              let _ = ret.send(None);
            },
          }
        },
        CollaborationCommand::BatchGetEncodeCollab { object_ids, ret } => {
          if let Some(group_manager) = weak_groups.upgrade() {
            let tasks = futures::stream::iter(object_ids)
              .map(|object_id| {
                let cloned_group_manager = group_manager.clone();
                tokio::task::spawn(async move {
                  let group = cloned_group_manager.get_group(&object_id).await;
                  if let Some(group) = group {
                    (object_id, group.encode_collab().await.ok())
                  } else {
                    (object_id, None)
                  }
                })
              })
              .buffer_unordered(BATCH_GET_ENCODE_COLLAB_CONCURRENCY)
              .collect::<Vec<_>>()
              .await;

            let mut outputs: HashMap<_, EncodedCollab> = HashMap::new();
            for (object_id, encoded_collab) in tasks.into_iter().flatten() {
              if let Some(encoded_collab) = encoded_collab {
                outputs.insert(object_id, encoded_collab);
              }
            }
            let _ = ret.send(outputs);
          } else {
            let _ = ret.send(HashMap::new());
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
