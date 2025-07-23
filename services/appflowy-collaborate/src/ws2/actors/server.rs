use super::session::{WsInput, WsSession};
use super::workspace::{Terminate, Workspace};
use crate::collab::collab_manager::CollabManager;
use crate::collab::snapshot_scheduler::SnapshotScheduler;
use crate::ws2::{BulkPermissionUpdate, PermissionUpdate};
use actix::{Actor, Addr, Arbiter, AsyncContext, Handler, Recipient};
use app_error::AppError;
use appflowy_proto::{ObjectId, Rid, ServerMessage, WorkspaceId};
use collab::core::origin::CollabOrigin;
use collab_entity::CollabType;
use collab_folder::Folder;
use database::collab::AppResult;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tracing::info;
use yrs::block::ClientID;

pub struct WsServer {
  manager: Arc<CollabManager>,
  snapshot_scheduler: SnapshotScheduler,
  workspaces: HashMap<WorkspaceId, Addr<Workspace>>,
  arbiter_pool: ArbiterPool,
}

impl WsServer {
  pub fn new(manager: Arc<CollabManager>) -> Self {
    let snapshot_scheduler = SnapshotScheduler::new(manager.clone());
    let arbiter_pool = ArbiterPool::default();
    Self {
      manager,
      snapshot_scheduler,
      workspaces: HashMap::new(),
      arbiter_pool,
    }
  }

  fn init_workspace(
    server: Recipient<Terminate>,
    workspace_id: WorkspaceId,
    manager: Arc<CollabManager>,
    snapshot_scheduler: SnapshotScheduler,
    pool: &ArbiterPool,
  ) -> Addr<Workspace> {
    let arbiter = pool.next();
    Workspace::start_in_arbiter(&arbiter.handle(), move |_ctx| {
      Workspace::new(server, workspace_id, manager, snapshot_scheduler)
    })
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
        self.manager.clone(),
        self.snapshot_scheduler.clone(),
        &self.arbiter_pool,
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
        self.manager.clone(),
        self.snapshot_scheduler.clone(),
        &self.arbiter_pool,
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
        self.manager.clone(),
        self.snapshot_scheduler.clone(),
        &self.arbiter_pool,
      )
    });
    workspace.do_send(msg);
  }
}

impl Handler<WorkspaceFolder> for WsServer {
  type Result = ();

  fn handle(&mut self, msg: WorkspaceFolder, ctx: &mut Self::Context) -> Self::Result {
    let server = ctx.address().recipient();
    let workspace = self.workspaces.entry(msg.workspace_id).or_insert_with(|| {
      Self::init_workspace(
        server,
        msg.workspace_id,
        self.manager.clone(),
        self.snapshot_scheduler.clone(),
        &self.arbiter_pool,
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

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct WorkspaceFolder {
  pub workspace_id: WorkspaceId,
  pub ack: tokio::sync::oneshot::Sender<AppResult<Folder>>,
}

#[async_trait::async_trait]
pub trait WorkspaceCollabInstanceCache {
  async fn get_folder(&self, workspace_id: WorkspaceId) -> AppResult<Folder>;
}

#[async_trait::async_trait]
impl WorkspaceCollabInstanceCache for Addr<WsServer> {
  async fn get_folder(&self, workspace_id: WorkspaceId) -> AppResult<Folder> {
    let (ack, rx) = tokio::sync::oneshot::channel();
    self.do_send(WorkspaceFolder { workspace_id, ack });
    rx.await.map_err(|err| AppError::Internal(err.into()))?
  }
}

pub struct ArbiterPool {
  arbiters: Vec<Arbiter>,
  next: AtomicU64,
}

impl Default for ArbiterPool {
  fn default() -> Self {
    let parallelism = match std::thread::available_parallelism() {
      Ok(max_cpus) => max_cpus.get(),
      Err(_) => 4, // Fallback to a default value if unable to determine
    };
    Self::new(parallelism)
  }
}

impl ArbiterPool {
  pub fn new(size: usize) -> Self {
    tracing::info!("creating arbiter pool on {} threads", size);
    let mut arbiters = Vec::with_capacity(size);
    for _ in 0..size {
      arbiters.push(Arbiter::new());
    }
    Self {
      arbiters,
      next: AtomicU64::new(0),
    }
  }

  pub fn next(&self) -> &Arbiter {
    let index =
      self.next.fetch_add(1, std::sync::atomic::Ordering::SeqCst) % (self.arbiters.len() as u64);
    &self.arbiters[index as usize]
  }
}
