use super::server::{Join, Leave, WsOutput};
use super::session::{InputMessage, WsInput, WsSession};
use crate::collab::collab_store::CollabStore;
use crate::collab::snapshot_scheduler::SnapshotScheduler;
use crate::ws2::{BroadcastPermissionChanges, PublishUpdate, UpdateUserPermissions};
use actix::ActorFutureExt;
use actix::{
  fut, Actor, ActorContext, Addr, AsyncContext, AtomicResponse, Handler, Recipient,
  ResponseActFuture, Running, SpawnHandle, StreamHandler, WrapFuture,
};
use anyhow::anyhow;
use app_error::AppError;
use appflowy_proto::{AccessChangedReason, ObjectId, Rid, ServerMessage, UpdateFlags, WorkspaceId};
use chrono::DateTime;
use collab::core::origin::CollabOrigin;
use collab::entity::EncoderVersion;
use collab_entity::CollabType;
use collab_stream::model::{AwarenessStreamUpdate, UpdateStreamMessage};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use yrs::block::ClientID;
use yrs::updates::encoder::Encode;
use yrs::{StateVector, Update};

pub struct Workspace {
  server: Recipient<Terminate>,
  workspace_id: WorkspaceId,
  last_message_id: Rid,
  store: Arc<CollabStore>,
  snapshot_scheduler: SnapshotScheduler,
  sessions_by_client_id: HashMap<ClientID, WorkspaceSessionHandle>,
  updates_handle: Option<SpawnHandle>,
  awareness_handle: Option<SpawnHandle>,
  snapshot_handle: Option<SpawnHandle>,
  termination_handle: Option<SpawnHandle>,
  permission_cache_cleanup_handle: Option<SpawnHandle>,
}

impl Workspace {
  pub const SNAPSHOT_INTERVAL: Duration = Duration::from_secs(60);
  pub const PERMISSION_CACHE_CLEANUP_INTERVAL: Duration = Duration::from_secs(180); // 3 minutes
  pub const PUBLISH_COLLAB_LIMIT: usize = 500;

  pub fn new(
    server: Recipient<Terminate>,
    workspace_id: WorkspaceId,
    store: Arc<CollabStore>,
    snapshot_scheduler: SnapshotScheduler,
  ) -> Self {
    Self {
      server,
      workspace_id,
      store,
      snapshot_scheduler,
      last_message_id: Rid::default(),
      sessions_by_client_id: HashMap::new(),
      updates_handle: None,
      awareness_handle: None,
      snapshot_handle: None,
      termination_handle: None,
      permission_cache_cleanup_handle: None,
    }
  }

  async fn publish_update(store: Arc<CollabStore>, workspace_id: WorkspaceId, msg: PublishUpdate) {
    let result = store
      .publish_update(
        workspace_id,
        msg.object_id,
        msg.collab_type,
        &msg.sender,
        msg.update_v1,
      )
      .await;
    let _ = msg.ack.send(result);
  }

  async fn hande_ws_input(store: Arc<CollabStore>, sender: WorkspaceSessionHandle, msg: WsInput) {
    match msg.message {
      InputMessage::Manifest(collab_type, rid, state_vector) => {
        match store
          .get_latest_state(
            msg.workspace_id,
            msg.object_id,
            collab_type,
            sender.collab_origin.client_user_id().unwrap(),
            state_vector,
          )
          .await
        {
          Ok(state) => {
            tracing::trace!(
              "replying to {} manifest (client msg id: {}, server msg id: {})",
              msg.object_id,
              rid,
              state.rid
            );
            sender.conn.do_send(WsOutput {
              message: ServerMessage::Update {
                object_id: msg.object_id,
                collab_type,
                flags: state.flags,
                last_message_id: state.rid,
                update: state.update,
              },
            });
            tracing::trace!(
              "sending manifest for {}, sv:{:?}",
              msg.object_id,
              state.state_vector
            );
            sender.conn.do_send(WsOutput {
              message: ServerMessage::Manifest {
                object_id: msg.object_id,
                collab_type,
                last_message_id: Rid::default(),
                state_vector: state.state_vector,
              },
            });

            //TODO: fetch all documents that have been updated since `rid` and send their manifests to the client
          },
          Err(AppError::RecordNotFound(_)) => {
            tracing::trace!("sending manifest for new collab {}", msg.object_id);
            sender.conn.do_send(WsOutput {
              message: ServerMessage::Manifest {
                object_id: msg.object_id,
                collab_type,
                last_message_id: Rid::default(),
                state_vector: StateVector::default().encode_v1(),
              },
            });
          },
          Err(AppError::RecordDeleted(_)) => {
            tracing::trace!("sending manifest for new collab {}", msg.object_id);
            sender.conn.do_send(WsOutput {
              message: ServerMessage::AccessChanges {
                object_id: msg.object_id,
                collab_type,
                can_read: false,
                can_write: false,
                reason: AccessChangedReason::ObjectDeleted,
              },
            });
          },
          Err(err) => {
            tracing::error!(
              "failed to resolve state of {}/{}/{}: {}",
              msg.workspace_id,
              msg.client_id,
              msg.object_id,
              err
            );
          },
        };
      },
      InputMessage::Update(collab_type, flags, update) => {
        if is_empty_update(&update, &flags) {
          tracing::trace!("skipping empty update {}", msg.object_id);
          return;
        }

        // Check if the user has permission to write to the collab
        if sender
          .can_write_collab(&store, &msg.object_id)
          .await
          .is_err()
        {
          tracing::trace!(
            "user {} lack of permission to write to collab {}",
            sender.uid,
            msg.object_id,
          );
          return;
        }

        if let Err(err) = store
          .publish_update(
            msg.workspace_id,
            msg.object_id,
            collab_type,
            &msg.sender,
            update,
          )
          .await
        {
          tracing::error!("failed to publish update: {:?}", err);
        }
      },
      InputMessage::AwarenessUpdate(update) => {
        if let Err(err) = store
          .publish_awareness_update(
            msg.workspace_id,
            msg.object_id,
            sender.collab_origin.clone(),
            update,
          )
          .await
        {
          tracing::error!("failed to publish awareness update: {:?}", err);
        }
      },
    }
  }

  async fn publish_collabs_created_since(
    store: Arc<CollabStore>,
    session_handle: WorkspaceSessionHandle,
    client_id: ClientID,
    workspace_id: WorkspaceId,
    last_message_id: Rid,
    reply_to: Addr<WsSession>,
    limit: usize,
  ) -> Result<(), AppError> {
    let since =
      DateTime::from_timestamp_millis(last_message_id.timestamp as i64).ok_or_else(|| {
        AppError::Internal(anyhow!(
          "last message id timestamp is invalid: {}",
          last_message_id
        ))
      })?;
    {
      let new_collabs = store
        .get_collabs_created_since(workspace_id, since, limit)
        .await?;
      tracing::trace!(
        "{} collabs created in workspace {} since {}",
        new_collabs.len(),
        workspace_id,
        since
      );
      for collab in new_collabs {
        if session_handle
          .can_read_collab(&store, &collab.object_id)
          .await
          .is_err()
        {
          tracing::trace!(
            "user {} lack of permission. skip publish new collab {}",
            session_handle.uid,
            collab.object_id,
          );
          continue;
        };

        // [0,0] is an empty Yrs document update encoded in v1 encoding
        if !collab.encoded_collab.doc_state.is_empty() && *collab.encoded_collab.doc_state != [0, 0]
        {
          tracing::trace!(
            "sending new collab {} state ({} bytes)",
            collab.object_id,
            collab.encoded_collab.doc_state.len()
          );
          let rid = match collab.updated_at {
            None => Rid::default(),
            Some(updated_at) => Rid::new(updated_at.timestamp_millis() as u64, 0),
          };
          reply_to.do_send(WsOutput {
            message: ServerMessage::Update {
              object_id: collab.object_id,
              collab_type: collab.collab_type,
              flags: match collab.encoded_collab.version {
                EncoderVersion::V1 => UpdateFlags::Lib0v1,
                EncoderVersion::V2 => UpdateFlags::Lib0v2,
              },
              last_message_id: rid,
              update: collab.encoded_collab.doc_state,
            },
          });
        }
      }
    }
    let updates = store
      .get_workspace_updates(&workspace_id, last_message_id.into())
      .await?;
    for update in updates {
      tracing::trace!(
        "sending collab {} update ({} bytes)",
        update.object_id,
        update.update.len()
      );

      if session_handle
        .can_read_collab(&store, &update.object_id)
        .await
        .is_err()
      {
        tracing::trace!(
          "user {} lack of permission. skip publish collab {}, client_id: {}",
          session_handle.uid,
          update.object_id,
          client_id,
        );
        continue;
      };
      reply_to.do_send(WsOutput {
        message: ServerMessage::Update {
          object_id: update.object_id,
          collab_type: update.collab_type,
          flags: update.update_flags,
          last_message_id: update.last_message_id,
          update: update.update,
        },
      });
    }

    Ok(())
  }

  /// Schedules a termination signal for the workspace in 1min.
  /// If there was a previous termination handle, it will be canceled.
  fn schedule_terminate(&mut self, ctx: &mut actix::Context<Self>) {
    if let Some(handle) = self.termination_handle.take() {
      ctx.cancel_future(handle);
    }
    let handle = ctx.notify_later(
      Terminate {
        workspace_id: self.workspace_id,
      },
      std::time::Duration::from_secs(60),
    );
    self.termination_handle = Some(handle);
  }
}

impl Actor for Workspace {
  type Context = actix::Context<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    tracing::info!("initializing workspace: {}", self.workspace_id);
    let update_streams_key = UpdateStreamMessage::stream_key(&self.workspace_id);
    let stream = self
      .store
      .updates()
      .observe::<UpdateStreamMessage>(update_streams_key, None);
    self.updates_handle = Some(ctx.add_stream(stream));
    let stream = self
      .store
      .awareness()
      .workspace_awareness_stream(&self.workspace_id);
    self.awareness_handle = Some(ctx.add_stream(stream));
    self.snapshot_handle = Some(ctx.notify_later(Snapshot, Self::SNAPSHOT_INTERVAL));
    self.permission_cache_cleanup_handle = Some(ctx.notify_later(
      CleanupPermissionCaches,
      Self::PERMISSION_CACHE_CLEANUP_INTERVAL,
    ));
  }

  fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
    tracing::info!("workspace {} stopping", self.workspace_id);
    self.server.do_send(Terminate {
      workspace_id: self.workspace_id,
    });
    if let Some(handle) = self.updates_handle.take() {
      ctx.cancel_future(handle);
    }
    if let Some(handle) = self.awareness_handle.take() {
      ctx.cancel_future(handle);
    }
    if let Some(handle) = self.snapshot_handle.take() {
      ctx.cancel_future(handle);

      // when closing the workspace, we should schedule a snapshot
      self
        .snapshot_scheduler
        .schedule_snapshot(self.workspace_id, self.last_message_id);
    }
    if let Some(handle) = self.termination_handle.take() {
      ctx.cancel_future(handle);
    }
    if let Some(handle) = self.permission_cache_cleanup_handle.take() {
      ctx.cancel_future(handle);
    }
    Running::Stop
  }
}

impl StreamHandler<anyhow::Result<UpdateStreamMessage>> for Workspace {
  fn handle(&mut self, item: anyhow::Result<UpdateStreamMessage>, ctx: &mut Self::Context) {
    let msg = match item {
      Ok(msg) => msg,
      Err(err) => {
        tracing::error!(
          "failed to read update stream message for workspace {}: {:?}",
          self.workspace_id,
          err
        );
        ctx.stop();
        return;
      },
    };

    let update_flags = msg.update_flags;
    if is_empty_update(&msg.update, &update_flags) {
      tracing::trace!("Receive empty update {}, skipping", msg.object_id);
      return;
    }

    self.last_message_id = self.last_message_id.max(msg.last_message_id);
    let store = self.store.clone();
    let object_id = msg.object_id;
    let collab_type = msg.collab_type;
    let last_message_id = msg.last_message_id;
    let update = msg.update.clone();
    store.mark_as_dirty(object_id, msg.last_message_id.timestamp);

    let sessions: Vec<WorkspaceSessionHandle> = self
      .sessions_by_client_id
      .values()
      .filter(|s| s.collab_origin != msg.sender)
      .cloned()
      .collect();

    ctx.spawn(
      async move {
        // For each session, check permission then send if allowed
        let send_tasks = sessions.into_iter().map(|session| {
          let store = Arc::clone(&store);
          let update = update.clone();
          async move {
            if session.can_read_collab(&store, &object_id).await.is_ok() {
              session.conn.do_send(WsOutput {
                message: ServerMessage::Update {
                  object_id,
                  collab_type,
                  flags: update_flags,
                  last_message_id,
                  update,
                },
              });
            } else {
              tracing::trace!(
                "user {} lack of permission. skip publish collab {}",
                session.uid,
                object_id,
              );
            }
          }
        });
        join_all(send_tasks).await;
      }
      .into_actor(self)
      .map(|_, _, _| ()),
    );
  }
}

impl StreamHandler<(ObjectId, Arc<AwarenessStreamUpdate>)> for Workspace {
  fn handle(
    &mut self,
    (object_id, msg): (ObjectId, Arc<AwarenessStreamUpdate>),
    _: &mut Self::Context,
  ) {
    tracing::trace!(
      "received awareness update for {}/{}",
      self.workspace_id,
      object_id
    );
    for (session_id, sender) in self.sessions_by_client_id.iter() {
      if sender.collab_origin == msg.sender {
        continue; // skip the sender
      }
      tracing::trace!("sending awareness update to {}", session_id);
      sender.conn.do_send(WsOutput {
        message: ServerMessage::AwarenessUpdate {
          object_id,
          collab_type: CollabType::Unknown,
          awareness: msg.data.encode_v1().into(),
        },
      });
    }
  }
}

impl Handler<Join> for Workspace {
  type Result = ();

  fn handle(&mut self, msg: Join, ctx: &mut Self::Context) -> Self::Result {
    if msg.workspace_id == self.workspace_id {
      let handle = WorkspaceSessionHandle::new(
        msg.uid,
        msg.workspace_id,
        msg.collab_origin,
        msg.addr.clone(),
      );
      self
        .sessions_by_client_id
        .insert(msg.session_id, handle.clone());
      tracing::trace!(
        "attached session `{}` to workspace {}",
        msg.session_id,
        msg.workspace_id
      );
      let store = self.store.clone();
      let workspace_id = self.workspace_id;
      let session = msg.addr;
      if let Some(last_message_id) = msg.last_message_id {
        ctx.spawn(
          async move {
            if let Err(err) = Self::publish_collabs_created_since(
              store,
              handle,
              msg.session_id,
              workspace_id,
              last_message_id,
              session,
              Self::PUBLISH_COLLAB_LIMIT,
            )
            .await
            {
              tracing::error!(
                "failed to send missing collabs for workspace {}: {}",
                workspace_id,
                err
              );
            }
          }
          .into_actor(self),
        );
      }
    }
  }
}

impl Handler<Leave> for Workspace {
  type Result = ();

  fn handle(&mut self, msg: Leave, ctx: &mut Self::Context) -> Self::Result {
    if msg.workspace_id == self.workspace_id {
      self.sessions_by_client_id.remove(&msg.session_id);
      tracing::trace!(
        "detached session `{}` from workspace {}",
        msg.session_id,
        msg.workspace_id
      );

      if self.sessions_by_client_id.is_empty() {
        self.schedule_terminate(ctx);
      }
    }
  }
}

impl Handler<Terminate> for Workspace {
  type Result = ();

  fn handle(&mut self, msg: Terminate, ctx: &mut Self::Context) -> Self::Result {
    if msg.workspace_id == self.workspace_id && self.sessions_by_client_id.is_empty() {
      ctx.stop();
    }
  }
}

impl Handler<WsInput> for Workspace {
  type Result = AtomicResponse<Self, ()>;
  fn handle(&mut self, msg: WsInput, _: &mut Self::Context) -> Self::Result {
    let store = self.store.clone();
    if let Some(sender) = self.sessions_by_client_id.get(&msg.client_id) {
      AtomicResponse::new(Box::pin(
        Self::hande_ws_input(store, sender.clone(), msg).into_actor(self),
      ))
    } else {
      AtomicResponse::new(Box::pin(fut::ready(())))
    }
  }
}

impl Handler<PublishUpdate> for Workspace {
  type Result = AtomicResponse<Self, ()>;
  fn handle(&mut self, msg: PublishUpdate, ctx: &mut Self::Context) -> Self::Result {
    let store = self.store.clone();
    let workspace_id = self.workspace_id;
    if self.sessions_by_client_id.is_empty() {
      // this is a single call message (i.e. called from a non-session context like HTTP request)
      // so if there are no active sessions, we should schedule a termination
      self.schedule_terminate(ctx);
    }
    AtomicResponse::new(Box::pin(
      Self::publish_update(store, workspace_id, msg).into_actor(self),
    ))
  }
}

impl Handler<Snapshot> for Workspace {
  type Result = ResponseActFuture<Self, ()>;

  fn handle(&mut self, _: Snapshot, _: &mut Self::Context) -> Self::Result {
    use actix::ActorFutureExt;
    let store = self.store.clone();
    let workspace_id = self.workspace_id;
    let up_to = self.last_message_id;
    Box::pin(
      async move { store.snapshot_workspace(workspace_id, up_to).await }
        .into_actor(self)
        .map(move |res, act, ctx| match res {
          Ok(_) => {
            tracing::trace!("workspace {} snapshot complete", workspace_id);
            act.snapshot_handle = Some(ctx.notify_later(Snapshot, Self::SNAPSHOT_INTERVAL));
          },
          Err(err) => {
            tracing::error!("failed to snapshot workspace {}: {}", workspace_id, err)
          },
        }),
    )
  }
}

impl Handler<CleanupPermissionCaches> for Workspace {
  type Result = ();

  fn handle(&mut self, _: CleanupPermissionCaches, ctx: &mut Self::Context) -> Self::Result {
    // Schedule the next cleanup
    self.permission_cache_cleanup_handle = Some(ctx.notify_later(
      CleanupPermissionCaches,
      Self::PERMISSION_CACHE_CLEANUP_INTERVAL,
    ));

    // Collect sessions to avoid borrowing issues
    let sessions: Vec<WorkspaceSessionHandle> =
      self.sessions_by_client_id.values().cloned().collect();

    // Execute cleanup asynchronously
    ctx.spawn(
      async move {
        for session in sessions {
          session.cleanup_expired_cache().await;
        }
      }
      .into_actor(self)
      .map(|_, _, _| ()),
    );
  }
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct Terminate {
  pub workspace_id: WorkspaceId,
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct Snapshot;

#[derive(actix::Message)]
#[rtype(result = "()")]
struct CleanupPermissionCaches;

impl Handler<UpdateUserPermissions> for Workspace {
  type Result = ();

  fn handle(&mut self, msg: UpdateUserPermissions, _: &mut Self::Context) -> Self::Result {
    // TODO(nathan): send message when documents permission changed
    // Find sessions for the specific user
    let user_sessions: Vec<WorkspaceSessionHandle> = self
      .sessions_by_client_id
      .values()
      .filter(|session| session.uid == msg.uid)
      .cloned()
      .collect();

    // Apply permission updates to user's sessions
    if !user_sessions.is_empty() {
      tokio::spawn(async move {
        for session in user_sessions {
          session.apply_permission_updates(msg.updates.clone()).await;
        }
      });
    }
  }
}

impl Handler<BroadcastPermissionChanges> for Workspace {
  type Result = ();

  fn handle(&mut self, msg: BroadcastPermissionChanges, _: &mut Self::Context) -> Self::Result {
    // TODO(nathan): send message when list of documents permission changed
    let sessions: Vec<WorkspaceSessionHandle> = self
      .sessions_by_client_id
      .values()
      .filter(|session| {
        // Exclude the user who made the change (if specified)
        if let Some(exclude_uid) = msg.exclude_uid {
          session.uid != exclude_uid
        } else {
          true
        }
      })
      .cloned()
      .collect();

    let changes = msg.changes;
    tokio::spawn(async move {
      for session in sessions {
        if changes.workspace_level_change {
          // Workspace-level change: clear all permissions for this user
          session.clear_permission_cache().await;
        } else {
          session
            .apply_permission_updates(changes.updates.clone())
            .await;
        }
      }
    });
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionUpdate {
  pub object_id: ObjectId,
  pub permission_type: PermissionType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum PermissionType {
  NoAccess,
  Read,
  Write,
}

impl PermissionType {
  pub fn can_read(&self) -> bool {
    matches!(self, PermissionType::Read | PermissionType::Write)
  }
  pub fn can_write(&self) -> bool {
    matches!(self, PermissionType::Write)
  }

  /// Check if this permission is higher than another permission
  pub fn is_higher_than(&self, other: &PermissionType) -> bool {
    self > other
  }

  /// Check if this permission is at least as high as another permission
  pub fn is_at_least(&self, other: &PermissionType) -> bool {
    self >= other
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkPermissionUpdate {
  pub updates: Vec<PermissionUpdate>,
  pub workspace_level_change: bool, // true if workspace role changed
}

#[derive(Clone)]
struct WorkspaceSessionHandle {
  uid: i64,
  workspace_id: WorkspaceId,
  collab_origin: CollabOrigin,
  conn: Addr<WsSession>,
  // Permission cache with expiration
  permission_cache: Arc<RwLock<HashMap<ObjectId, (PermissionType, Instant)>>>,
  cache_ttl: Duration,
}

impl WorkspaceSessionHandle {
  fn new(
    uid: i64,
    workspace_id: WorkspaceId,
    collab_origin: CollabOrigin,
    conn: Addr<WsSession>,
  ) -> Self {
    Self::new_with_cache_ttl(
      uid,
      workspace_id,
      collab_origin,
      conn,
      Duration::from_secs(300), // 5 minutes cache TTL
    )
  }

  fn new_with_cache_ttl(
    uid: i64,
    workspace_id: WorkspaceId,
    collab_origin: CollabOrigin,
    conn: Addr<WsSession>,
    cache_ttl: Duration,
  ) -> Self {
    Self {
      uid,
      workspace_id,
      collab_origin,
      conn,
      permission_cache: Arc::new(RwLock::new(HashMap::new())),
      cache_ttl,
    }
  }

  async fn can_write_collab(
    &self,
    store: &Arc<CollabStore>,
    object_id: &ObjectId,
  ) -> Result<bool, AppError> {
    let now = Instant::now();
    if let Some((permission, cached_at)) = self.permission_cache.read().await.get(object_id) {
      if now.duration_since(*cached_at) < self.cache_ttl {
        return Ok(permission.can_write());
      }
    }

    // Cache miss or expired, check permission and update cache
    let has_permission = store
      .enforce_write_collab(&self.workspace_id, &self.uid, object_id)
      .await
      .is_ok();

    // Update cache with the actual permission state
    let permission_type = if has_permission {
      PermissionType::Write
    } else {
      PermissionType::NoAccess
    };

    self
      .permission_cache
      .write()
      .await
      .insert(*object_id, (permission_type, now));

    Ok(has_permission)
  }

  async fn can_read_collab(
    &self,
    store: &Arc<CollabStore>,
    object_id: &ObjectId,
  ) -> Result<bool, AppError> {
    let now = Instant::now();
    if let Some((permission, cached_at)) = self.permission_cache.read().await.get(object_id) {
      if now.duration_since(*cached_at) < self.cache_ttl {
        return Ok(permission.can_read());
      }
    }

    let has_permission = store
      .enforce_read_collab(&self.workspace_id, &self.uid, object_id)
      .await
      .is_ok();

    let mut cache = self.permission_cache.write().await;
    if has_permission {
      // Check if there's already a higher permission cached
      if let Some((existing_permission, _)) = cache.get(object_id) {
        if existing_permission.is_higher_than(&PermissionType::Read) {
          return Ok(true);
        }
      }
      cache.insert(*object_id, (PermissionType::Read, now));
    } else {
      cache.insert(*object_id, (PermissionType::NoAccess, now));
    }

    Ok(has_permission)
  }

  /// Apply permission updates and send WebSocket notification
  async fn apply_permission_updates(&self, updates: Vec<PermissionUpdate>) {
    let mut cache = self.permission_cache.write().await;
    let now = Instant::now();
    for update in updates {
      // Always allow updates from NoAccess, but prevent downgrades from higher permissions
      if let Some((existing_permission, _)) = cache.get(&update.object_id) {
        // Allow update if:
        // 1. Current permission is NoAccess (always allow upgrade from no access)
        // 2. New permission is at least as high as existing permission
        if *existing_permission == PermissionType::NoAccess
          || update.permission_type.is_at_least(existing_permission)
        {
          cache.insert(update.object_id, (update.permission_type, now));
        }
      } else {
        // No existing permission, insert the new one
        cache.insert(update.object_id, (update.permission_type, now));
      }
    }
  }

  /// Clear all cached permissions (useful when user's workspace role changes)
  async fn clear_permission_cache(&self) {
    let mut cache = self.permission_cache.write().await;
    cache.clear();
  }

  /// Remove expired entries from cache
  async fn cleanup_expired_cache(&self) {
    let now = Instant::now();
    let mut cache = self.permission_cache.write().await;
    cache.retain(|_, (_, cached_at)| now.duration_since(*cached_at) < self.cache_ttl);
  }
}

#[inline]
fn is_empty_update(update: &[u8], flag: &UpdateFlags) -> bool {
  (flag == &UpdateFlags::Lib0v1 && update == Update::EMPTY_V1)
    || (flag == &UpdateFlags::Lib0v2 && update == Update::EMPTY_V2)
}
