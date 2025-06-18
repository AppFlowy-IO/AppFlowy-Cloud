use super::session::{WsInput, WsSession};
use super::workspace::{Terminate, Workspace};
use crate::collab::collab_store::CollabStore;
use crate::collab::snapshot_scheduler::SnapshotScheduler;
use crate::ws2::{BulkPermissionUpdate, PermissionUpdate};
use actix::{Actor, Addr, AsyncContext, Handler, Recipient};
use appflowy_proto::{ObjectId, Rid, ServerMessage, WorkspaceId};
use collab::core::origin::CollabOrigin;
use collab_entity::CollabType;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use tracing::info;
use yrs::block::ClientID;

pub struct WsServer {
  store: Arc<CollabStore>,
  snapshot_scheduler: SnapshotScheduler,
  workspaces: HashMap<WorkspaceId, Addr<Workspace>>,
}

impl WsServer {
  pub fn new(store: Arc<CollabStore>) -> Self {
    let snapshot_scheduler = SnapshotScheduler::new(store.clone());
    Self {
      store,
      snapshot_scheduler,
      workspaces: HashMap::new(),
    }
  }

  fn init_workspace(
    server: Recipient<Terminate>,
    workspace_id: WorkspaceId,
    store: Arc<CollabStore>,
    snapshot_scheduler: SnapshotScheduler,
  ) -> Addr<Workspace> {
    Workspace::new(server, workspace_id, store, snapshot_scheduler).start()
  }
}

impl Actor for WsServer {
  type Context = actix::Context<Self>;
}

impl Handler<Join> for WsServer {
  type Result = ();

  fn handle(&mut self, msg: Join, ctx: &mut Self::Context) -> Self::Result {
    let server = ctx.address().recipient();
    let workspace = self.workspaces.entry(msg.workspace_id).or_insert_with(|| {
      Self::init_workspace(
        server,
        msg.workspace_id,
        self.store.clone(),
        self.snapshot_scheduler.clone(),
      )
    });
    info!("{} joined", msg);
    workspace.do_send(msg);
  }
}

impl Handler<Leave> for WsServer {
  type Result = ();

  fn handle(&mut self, msg: Leave, _ctx: &mut Self::Context) -> Self::Result {
    if let Some(workspace) = self.workspaces.get(&msg.workspace_id) {
      workspace.do_send(msg);
    }
  }
}

impl Handler<WsInput> for WsServer {
  type Result = ();

  fn handle(&mut self, msg: WsInput, ctx: &mut Self::Context) -> Self::Result {
    let server = ctx.address().recipient();
    let workspace = self.workspaces.entry(msg.workspace_id).or_insert_with(|| {
      Self::init_workspace(
        server,
        msg.workspace_id,
        self.store.clone(),
        self.snapshot_scheduler.clone(),
      )
    });
    workspace.do_send(msg);
  }
}

impl Handler<Terminate> for WsServer {
  type Result = ();

  fn handle(&mut self, msg: Terminate, _ctx: &mut Self::Context) -> Self::Result {
    self.workspaces.remove(&msg.workspace_id);
    tracing::debug!("workspace {} closed", msg.workspace_id);
  }
}

impl Handler<PublishUpdate> for WsServer {
  type Result = ();

  fn handle(&mut self, msg: PublishUpdate, ctx: &mut Self::Context) -> Self::Result {
    let server = ctx.address().recipient();
    let workspace = self.workspaces.entry(msg.workspace_id).or_insert_with(|| {
      Self::init_workspace(
        server,
        msg.workspace_id,
        self.store.clone(),
        self.snapshot_scheduler.clone(),
      )
    });
    workspace.do_send(msg);
  }
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct Join {
  pub uid: i64,
  /// Current client session identifier.
  pub session_id: ClientID,
  pub collab_origin: CollabOrigin,
  pub last_message_id: Option<Rid>,
  /// Actix WebSocket session actor address.
  pub addr: Addr<WsSession>,
  /// Workspace to join.
  pub workspace_id: WorkspaceId,
}

impl Display for Join {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "Join(uid: {}, session_id: {}, workspace_id: {})",
      self.uid, self.session_id, self.workspace_id
    )
  }
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct Leave {
  /// Current client session identifier.
  pub session_id: ClientID,
  /// Workspace to leave.
  pub workspace_id: WorkspaceId,
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct WsOutput {
  pub message: ServerMessage,
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct UpdateUserPermissions {
  pub uid: i64,
  pub updates: Vec<PermissionUpdate>,
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct BroadcastPermissionChanges {
  pub changes: BulkPermissionUpdate,
  pub exclude_uid: Option<i64>, // Don't send to the user who made the change
}

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct PublishUpdate {
  pub workspace_id: WorkspaceId,
  pub object_id: ObjectId,
  pub collab_type: CollabType,
  pub sender: CollabOrigin,
  pub update_v1: Vec<u8>,
  pub ack: tokio::sync::oneshot::Sender<anyhow::Result<Rid>>,
}

#[async_trait::async_trait]
pub trait CollabUpdatePublisher {
  async fn publish_update(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    sender: &CollabOrigin,
    update_v1: Vec<u8>,
  ) -> anyhow::Result<Rid>;
}

#[async_trait::async_trait]
impl CollabUpdatePublisher for Addr<WsServer> {
  async fn publish_update(
    &self,
    workspace_id: WorkspaceId,
    object_id: ObjectId,
    collab_type: CollabType,
    sender: &CollabOrigin,
    update_v1: Vec<u8>,
  ) -> anyhow::Result<Rid> {
    let (ack, rx) = tokio::sync::oneshot::channel();
    self.do_send(PublishUpdate {
      workspace_id,
      object_id,
      collab_type,
      sender: sender.clone(),
      update_v1,
      ack,
    });
    rx.await?
  }
}
