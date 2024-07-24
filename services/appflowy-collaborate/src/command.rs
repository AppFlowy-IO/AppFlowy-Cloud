use crate::group::cmd::{GroupCommand, GroupCommandSender};
use bytes::Bytes;
use collab::entity::EncodedCollab;
use collab_folder::CollabOrigin;
use collab_rt_entity::{user::RealtimeUser, ClientCollabMessage, UpdateSync};
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
  SendEncodeCollab {
    uid: i64,
    object_id: String,
    encoded_v1_bytes: Bytes,
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
        CollaborationCommand::SendEncodeCollab {
          uid,
          object_id,
          encoded_v1_bytes,
        } => {
          if let Some(sender) = group_sender_by_object_id.get(&object_id) {
            let ts_now_milli = chrono::Utc::now().timestamp_millis();
            let (tx, mut rx) = tokio::sync::oneshot::channel();
            match sender
              .send(GroupCommand::HandleClientCollabMessage {
                user: RealtimeUser {
                  uid,
                  device_id: uuid::Uuid::new_v4().to_string(),
                  connect_at: ts_now_milli,
                  session_id: uuid::Uuid::new_v4().to_string(),
                  app_version: "".to_string(),
                },
                object_id: object_id.clone(),
                collab_messages: vec![ClientCollabMessage::ClientUpdateSync {
                  data: UpdateSync {
                    origin: CollabOrigin::Server,
                    object_id,
                    msg_id: ts_now_milli as u64,
                    payload: encoded_v1_bytes,
                  },
                }],
                ret: tx,
              })
              .await
            {
              Ok(()) => {
                if let Err(err) = rx.try_recv() {
                  tracing::error!("Send encode collab fail: {:?}", err);
                }
              },
              Err(err) => tracing::error!("Send group command error: {}", err),
            }
          }
        },
      }
    }
  });
}
