use crate::group::cmd::{GroupCommand, GroupCommandSender};
use crate::rt_server::rt_spawn;
use collab::core::collab_plugin::EncodedCollab;
use dashmap::DashMap;
use std::sync::Arc;
use tracing::error;

pub type RTCommandSender = tokio::sync::mpsc::Sender<RTCommand>;
pub type RTCommandReceiver = tokio::sync::mpsc::Receiver<RTCommand>;

pub type EncodeCollabSender = tokio::sync::oneshot::Sender<Option<EncodedCollab>>;
pub enum RTCommand {
  GetEncodeCollab {
    object_id: String,
    ret: EncodeCollabSender,
  },
}

pub(crate) fn spawn_rt_command(
  mut command_recv: RTCommandReceiver,
  group_sender_by_object_id: &Arc<DashMap<String, GroupCommandSender>>,
) {
  let group_sender_by_object_id = group_sender_by_object_id.clone();
  rt_spawn(async move {
    while let Some(cmd) = command_recv.recv().await {
      match cmd {
        RTCommand::GetEncodeCollab { object_id, ret } => {
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
      }
    }
  });
}
