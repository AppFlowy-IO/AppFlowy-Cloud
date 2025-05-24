use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::client::client_msg_router::ClientMessageRouter;
use crate::command::{spawn_collaboration_command, CLCommandReceiver};
use crate::config::get_env_var;
use crate::connect_state::ConnectState;
use crate::error::{CreateGroupFailedReason, RealtimeError};
use crate::group::cmd::{GroupCommand, GroupCommandRunner, GroupCommandSender};
use crate::group::manager::GroupManager;
use crate::rt_server::collaboration_runtime::COLLAB_RUNTIME;
use access_control::collab::RealtimeAccessControl;
use anyhow::{anyhow, Result};
use app_error::AppError;
use collab_rt_entity::user::{RealtimeUser, UserDevice};
use collab_rt_entity::MessageByObjectId;
use collab_stream::awareness_gossip::AwarenessGossip;
use collab_stream::client::CollabRedisStream;
use collab_stream::stream_router::StreamRouter;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use database::collab::CollabStorage;
use indexer::scheduler::IndexerScheduler;
use redis::aio::ConnectionManager;
use tokio::sync::mpsc::Sender;
use tokio::task::yield_now;
use tokio::time::interval;
use tracing::{error, info, trace, warn};
use uuid::Uuid;
use yrs::updates::decoder::Decode;
use yrs::StateVector;

use crate::actix_ws::entities::{ClientGenerateEmbeddingMessage, ClientHttpUpdateMessage};
use crate::{CollabRealtimeMetrics, RealtimeClientWebsocketSink};

#[derive(Clone)]
pub struct CollaborationServer<S> {
  /// Keep track of all collab groups
  group_manager: Arc<GroupManager<S>>,
  connect_state: ConnectState,
  group_sender_by_object_id: Arc<DashMap<Uuid, GroupCommandSender>>,
  #[allow(dead_code)]
  metrics: Arc<CollabRealtimeMetrics>,
  enable_custom_runtime: bool,
}

impl<S> CollaborationServer<S>
where
  S: CollabStorage,
{
  #[allow(clippy::too_many_arguments)]
  pub async fn new(
    storage: Arc<S>,
    access_control: Arc<dyn RealtimeAccessControl>,
    metrics: Arc<CollabRealtimeMetrics>,
    command_recv: CLCommandReceiver,
    redis_stream_router: Arc<StreamRouter>,
    awareness_gossip: Arc<AwarenessGossip>,
    redis_connection_manager: ConnectionManager,
    group_persistence_interval: Duration,
    indexer_scheduler: Arc<IndexerScheduler>,
  ) -> Result<Self, RealtimeError> {
    let enable_custom_runtime = get_env_var("APPFLOWY_COLLABORATE_MULTI_THREAD", "false")
      .parse::<bool>()
      .unwrap_or(false);

    if enable_custom_runtime {
      info!("CollaborationServer with custom runtime");
    } else {
      info!("CollaborationServer with actix-web runtime");
    }

    let connect_state = ConnectState::new();
    let collab_stream = CollabRedisStream::new_with_connection_manager(
      redis_connection_manager,
      redis_stream_router,
      awareness_gossip,
    );
    let group_manager = Arc::new(
      GroupManager::new(
        storage.clone(),
        access_control.clone(),
        metrics.clone(),
        collab_stream,
        group_persistence_interval,
        indexer_scheduler.clone(),
      )
      .await?,
    );
    let group_sender_by_object_id: Arc<DashMap<_, GroupCommandSender>> =
      Arc::new(Default::default());

    spawn_period_check_inactive_group(Arc::downgrade(&group_manager), &group_sender_by_object_id);

    spawn_collaboration_command(
      command_recv,
      &group_sender_by_object_id,
      Arc::downgrade(&group_manager),
    );

    Ok(Self {
      group_manager,
      connect_state,
      group_sender_by_object_id,
      metrics,
      enable_custom_runtime,
    })
  }

  /// Handles a new user connection, replacing any existing connection for the same user.
  ///
  /// - Creates a new client stream for the connected user.
  /// - Replaces any existing user connection with the new one, signaling the old connection
  ///   if it's replaced.
  /// - Removes the old user connection from all collaboration groups.
  ///
  pub fn handle_new_connection(
    &self,
    connected_user: RealtimeUser,
    conn_sink: impl RealtimeClientWebsocketSink,
  ) -> Result<(), RealtimeError> {
    let new_client_router = ClientMessageRouter::new(conn_sink);
    if let Some(old_user) = self
      .connect_state
      .handle_user_connect(connected_user, new_client_router)
    {
      // Remove the old user from all collaboration groups.
      self.group_manager.remove_user(&old_user);
    }
    self
      .metrics
      .connected_users
      .set(self.connect_state.number_of_connected_users() as i64);
    Ok(())
  }

  /// Handles a user's disconnection from the collaboration server.
  ///
  /// Steps:
  /// 1. Checks if the disconnecting user's session matches the stored session.
  ///    - If yes, proceeds with removal.
  ///    - If not, exits without action.
  /// 2. Removes the user from collaboration groups and client streams.
  pub fn handle_disconnect(&self, disconnect_user: RealtimeUser) -> Result<(), RealtimeError> {
    trace!("[realtime]: disconnect => {}", disconnect_user);
    let was_removed = self.connect_state.handle_user_disconnect(&disconnect_user);
    if was_removed.is_some() {
      self
        .metrics
        .connected_users
        .set(self.connect_state.number_of_connected_users() as i64);

      self.group_manager.remove_user(&disconnect_user);
    }

    Ok(())
  }

  #[inline]
  pub fn handle_client_message(
    &self,
    user: RealtimeUser,
    message_by_oid: MessageByObjectId,
  ) -> Result<(), RealtimeError> {
    for (object_id, collab_messages) in message_by_oid.into_inner() {
      let object_id = Uuid::parse_str(&object_id)?;
      let group_cmd_sender = self.create_group_if_not_exist(object_id);
      let cloned_user = user.clone();
      // Create a new task to send a message to the group command runner without waiting for the
      // result. This approach is used to prevent potential issues with the actor's mailbox in
      // single-threaded runtimes (like actix-web actors). By spawning a task, the actor can
      // immediately proceed to process the next message.
      tokio::spawn(async move {
        let (tx, rx) = tokio::sync::oneshot::channel();
        match group_cmd_sender
          .send(GroupCommand::HandleClientCollabMessage {
            user: cloned_user,
            object_id,
            collab_messages,
            ret: tx,
          })
          .await
        {
          Ok(_) => {
            if let Ok(Err(err)) = rx.await {
              if !matches!(
                err,
                RealtimeError::CreateGroupFailed(
                  CreateGroupFailedReason::CollabWorkspaceIdNotMatch { .. }
                )
              ) {
                error!("Handle client collab message fail: {}", err);
              }
            }
          },
          Err(err) => {
            // it should not happen. Because the receiver is always running before acquiring the sender.
            // Otherwise, the GroupCommandRunner might not be ready to handle the message.
            error!("Send message to group fail: {}", err);
          },
        }
      });
    }

    Ok(())
  }

  #[inline]
  pub fn handle_client_http_update(
    &self,
    message: ClientHttpUpdateMessage,
  ) -> Result<(), RealtimeError> {
    let group_cmd_sender = self.create_group_if_not_exist(message.object_id);
    tokio::spawn(async move {
      let object_id = message.object_id;
      let (tx, rx) = tokio::sync::oneshot::channel();
      let result = group_cmd_sender
        .send(GroupCommand::HandleClientHttpUpdate {
          user: message.user,
          workspace_id: message.workspace_id,
          object_id: message.object_id,
          update: message.update,
          collab_type: message.collab_type,
          ret: tx,
        })
        .await;

      let return_tx = message.return_tx;
      if let Err(err) = result {
        if let Some(return_rx) = return_tx {
          let _ = return_rx.send(Err(AppError::Internal(anyhow!(
            "send update to group fail: {}",
            err
          ))));
          return;
        } else {
          error!("send http update to group fail: {}", err);
        }
      }

      match rx.await {
        Ok(Ok(())) => {
          if message.state_vector.is_some() && return_tx.is_none() {
            warn!(
              "state_vector is not None, but return_tx is None, object_id: {}",
              object_id
            );
          }

          if let Some(return_rx) = return_tx {
            if let Some(state_vector) = message
              .state_vector
              .and_then(|data| StateVector::decode_v1(&data).ok())
            {
              // yield
              yield_now().await;

              // Calculate missing update
              let (tx, rx) = tokio::sync::oneshot::channel();
              let _ = group_cmd_sender
                .send(GroupCommand::CalculateMissingUpdate {
                  object_id,
                  state_vector,
                  ret: tx,
                })
                .await;
              match rx.await {
                Ok(missing_update_result) => {
                  let _ = group_cmd_sender
                    .send(GroupCommand::GenerateCollabEmbedding { object_id })
                    .await;

                  let result = missing_update_result
                    .map_err(|err| {
                      AppError::Internal(anyhow!("fail to calculate missing update: {}", err))
                    })
                    .map(Some);
                  let _ = return_rx.send(result);
                },
                Err(err) => {
                  let _ = return_rx.send(Err(AppError::Internal(anyhow!(
                    "fail to calculate missing update: {}",
                    err
                  ))));
                },
              }
            } else {
              let _ = return_rx.send(Ok(None));
            }
          }
        },
        Ok(Err(err)) => {
          if let Some(return_rx) = return_tx {
            let _ = return_rx.send(Err(AppError::Internal(anyhow!(
              "apply http update to group fail: {}",
              err
            ))));
          } else {
            error!("apply http update to group fail: {}", err);
          }
        },
        Err(err) => {
          if let Some(return_rx) = return_tx {
            let _ = return_rx.send(Err(AppError::Internal(anyhow!(
              "fail to receive applied result: {}",
              err
            ))));
          } else {
            error!("fail to receive applied result: {}", err);
          }
        },
      }
    });

    Ok(())
  }

  #[inline]
  fn create_group_if_not_exist(&self, object_id: Uuid) -> Sender<GroupCommand> {
    let old_sender = self
      .group_sender_by_object_id
      .get(&object_id)
      .map(|entry| entry.value().clone());

    let sender = match old_sender {
      Some(sender) => sender,
      None => match self.group_sender_by_object_id.entry(object_id) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
          let (new_sender, recv) = tokio::sync::mpsc::channel(2000);
          let runner = GroupCommandRunner {
            group_manager: self.group_manager.clone(),
            msg_router_by_user: self.connect_state.client_message_routers.clone(),
            recv: Some(recv),
          };

          let object_id = *entry.key();
          if self.enable_custom_runtime {
            COLLAB_RUNTIME.spawn(runner.run(object_id));
          } else {
            tokio::spawn(runner.run(object_id));
          }

          entry.insert(new_sender.clone());
          new_sender
        },
      },
    };
    sender
  }

  #[inline]
  pub fn handle_client_generate_embedding_request(
    &self,
    message: ClientGenerateEmbeddingMessage,
  ) -> Result<(), RealtimeError> {
    let group_cmd_sender = self.create_group_if_not_exist(message.object_id);
    tokio::spawn(async move {
      let result = group_cmd_sender
        .send(GroupCommand::GenerateCollabEmbedding {
          object_id: message.object_id,
        })
        .await;

      if let Err(err) = result {
        if let Some(return_tx) = message.return_tx {
          let _ = return_tx.send(Err(AppError::Internal(anyhow!(
            "send generate embedding to group fail: {}",
            err
          ))));
        } else {
          error!("send generate embedding to group fail: {}", err);
        }
      }
    });
    Ok(())
  }

  pub fn get_user_by_device(&self, user_device: &UserDevice) -> Option<RealtimeUser> {
    self
      .connect_state
      .user_by_device
      .get(user_device)
      .map(|entry| entry.value().clone())
  }
}

fn spawn_period_check_inactive_group<S>(
  weak_groups: Weak<GroupManager<S>>,
  group_sender_by_object_id: &Arc<DashMap<Uuid, GroupCommandSender>>,
) where
  S: CollabStorage,
{
  let mut interval = interval(Duration::from_secs(20));
  let cloned_group_sender_by_object_id = group_sender_by_object_id.clone();
  tokio::spawn(async move {
    // when appflowy-collaborate start, wait for 60 seconds to start the check. Since no groups will
    // be inactive in the first 60 seconds.
    tokio::time::sleep(Duration::from_secs(60)).await;

    loop {
      interval.tick().await;
      if let Some(groups) = weak_groups.upgrade() {
        let inactive_group_ids = groups.get_inactive_groups();
        for id in inactive_group_ids {
          cloned_group_sender_by_object_id.remove(&id);
        }
      } else {
        break;
      }
    }
  });
}

/// When the CollaborationServer operates within an actix-web actor, utilizing tokio::spawn for
/// task execution confines all tasks to the same thread, attributable to the actor's reliance on a
/// single-threaded Tokio runtime. To circumvent this limitation and enable task execution across
/// multiple threads, we've incorporated a multi-thread feature.
///
/// When appflowy-collaborate is deployed as a standalone service, we can use tokio multi-thread.
mod collaboration_runtime {
  use std::io;

  use lazy_static::lazy_static;
  use tokio::runtime;
  use tokio::runtime::Runtime;

  lazy_static! {
    pub(crate) static ref COLLAB_RUNTIME: Runtime = default_tokio_runtime().unwrap();
  }

  pub fn default_tokio_runtime() -> io::Result<Runtime> {
    runtime::Builder::new_multi_thread()
      .thread_name("collab-rt")
      .enable_io()
      .enable_time()
      .build()
  }
}
