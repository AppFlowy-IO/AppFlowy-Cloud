use super::session::{WsInput, WsSession};
use super::workspace::{Terminate, Workspace};
use crate::ws2::collab_store::CollabStore;
use crate::ws2::{BulkPermissionUpdate, PermissionUpdate};
use actix::{Actor, Addr, AsyncContext, Handler, Recipient};
use appflowy_proto::{Rid, ServerMessage, WorkspaceId};
use collab::core::origin::CollabOrigin;
use std::collections::HashMap;
use std::sync::Arc;
use yrs::block::ClientID;

pub struct WsServer {
  store: Arc<CollabStore>,
  workspaces: HashMap<WorkspaceId, Addr<Workspace>>,
}

impl WsServer {
  pub fn new(store: Arc<CollabStore>) -> Self {
    Self {
      store,
      workspaces: HashMap::new(),
    }
  }

  fn init_workspace(
    server: Recipient<Terminate>,
    workspace_id: WorkspaceId,
    store: Arc<CollabStore>,
  ) -> Addr<Workspace> {
    Workspace::new(server, workspace_id, store).start()
  }
}

impl Actor for WsServer {
  type Context = actix::Context<Self>;
}

impl Handler<Join> for WsServer {
  type Result = ();

  fn handle(&mut self, msg: Join, ctx: &mut Self::Context) -> Self::Result {
    let server = ctx.address().recipient();
    let workspace = self
      .workspaces
      .entry(msg.workspace_id)
      .or_insert_with(|| Self::init_workspace(server, msg.workspace_id, self.store.clone()));
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
    let workspace = self
      .workspaces
      .entry(msg.workspace_id)
      .or_insert_with(|| Self::init_workspace(server, msg.workspace_id, self.store.clone()));
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
