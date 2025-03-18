use super::server::{Join, Leave, WsOutput};
use super::session::{InputMessage, WsInput, WsSession};
use crate::ws2::collab_store::CollabStore;
use crate::ws2::messages::UpdateStreamMessage;
use actix::{
  fut, Actor, ActorContext, Addr, AsyncContext, AtomicResponse, Handler, Recipient,
  ResponseActFuture, Running, SpawnHandle, StreamHandler, WrapFuture,
};
use appflowy_proto::{ObjectId, Rid, ServerMessage, WorkspaceId};
use collab::core::origin::CollabOrigin;
use collab_stream::error::StreamError;
use collab_stream::model::AwarenessStreamUpdate;
use std::collections::HashMap;
use std::sync::Arc;
use yrs::block::ClientID;
use yrs::updates::encoder::Encode;

struct SessionHandle {
  collab_origin: CollabOrigin,
  conn: Addr<WsSession>,
}

impl SessionHandle {
  fn new(collab_origin: CollabOrigin, conn: Addr<WsSession>) -> Self {
    Self {
      collab_origin,
      conn,
    }
  }
}

pub struct Workspace {
  server: Recipient<Terminate>,
  workspace_id: WorkspaceId,
  last_message_id: Rid,
  store: Arc<CollabStore>,
  sessions_by_client_id: HashMap<ClientID, SessionHandle>,
  updates_handle: Option<SpawnHandle>,
  awareness_handle: Option<SpawnHandle>,
  snapshot_handle: Option<SpawnHandle>,
}

impl Workspace {
  pub const SNAPSHOT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

  pub fn new(
    server: Recipient<Terminate>,
    workspace_id: WorkspaceId,
    store: Arc<CollabStore>,
  ) -> Self {
    Self {
      server,
      workspace_id,
      store,
      last_message_id: Rid::default(),
      sessions_by_client_id: HashMap::new(),
      updates_handle: None,
      awareness_handle: None,
      snapshot_handle: None,
    }
  }

  async fn hande_ws_input(store: Arc<CollabStore>, sender: Addr<WsSession>, msg: WsInput) {
    match msg.message {
      InputMessage::Manifest(rid, state_vector) => {
        match store
          .get_latest_state(
            msg.workspace_id,
            msg.object_id,
            msg.collab_type,
            &state_vector,
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
            sender.do_send(WsOutput {
              message: ServerMessage::Update {
                object_id: msg.object_id,
                collab_type: msg.collab_type,
                flags: state.flags,
                last_message_id: state.rid,
                update: state.update.into(),
              },
            });
          },
          Err(err) => {
            tracing::error!(
              "failed to resolve state of {}/{}: {}",
              msg.workspace_id,
              msg.object_id,
              err
            );
          },
        };
      },
      InputMessage::Update(update) => {
        if let Err(err) = store
          .publish_update(
            msg.workspace_id,
            msg.object_id,
            msg.sender,
            update.encode_v1(),
          )
          .await
        {
          tracing::error!("failed to publish update: {:?}", err);
        }
      },
      InputMessage::AwarenessUpdate(update) => {
        if let Err(err) = store
          .publish_awareness_update(msg.workspace_id, msg.object_id, msg.sender, update)
          .await
        {
          tracing::error!("failed to publish awareness update: {:?}", err);
        }
      },
    }
  }
}

impl Actor for Workspace {
  type Context = actix::Context<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    tracing::info!("initializing workspace: {}", self.workspace_id);
    let update_streams_key = format!("af:u:{}", self.workspace_id);
    let stream = self
      .store
      .updates()
      .observe::<UpdateStreamMessage>(update_streams_key, None);
    self.updates_handle = Some(ctx.add_stream(stream));
    let stream = self
      .store
      .awareness()
      .awareness_workspace_stream(&self.workspace_id);
    self.awareness_handle = Some(ctx.add_stream(stream));
    self.snapshot_handle = Some(ctx.notify_later(Snapshot, Self::SNAPSHOT_INTERVAL));
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
    }
    Running::Stop
  }
}

impl StreamHandler<anyhow::Result<UpdateStreamMessage>> for Workspace {
  fn handle(&mut self, item: anyhow::Result<UpdateStreamMessage>, ctx: &mut Self::Context) {
    match item {
      Ok(msg) => {
        tracing::trace!(
          "received update for {}/{}",
          self.workspace_id,
          msg.object_id
        );
        self.last_message_id = msg.last_message_id.max(msg.last_message_id);
        for (session_id, sender) in self.sessions_by_client_id.iter() {
          if session_id == &msg.sender {
            continue; // skip the sender
          }
          sender.conn.do_send(WsOutput {
            message: ServerMessage::Update {
              object_id: msg.object_id,
              collab_type: msg.collab_type,
              flags: msg.update_flags,
              last_message_id: msg.last_message_id,
              update: msg.update.clone(),
            },
          });
        }
      },
      Err(err) => {
        tracing::error!(
          "failed to read update stream message for workpsace {}: {:?}",
          self.workspace_id,
          err
        );
        ctx.stop();
      },
    }
  }
}

impl StreamHandler<Result<(ObjectId, AwarenessStreamUpdate), StreamError>> for Workspace {
  fn handle(
    &mut self,
    item: Result<(ObjectId, AwarenessStreamUpdate), StreamError>,
    ctx: &mut Self::Context,
  ) {
    match item {
      Ok((object_id, msg)) => {
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
              collab_type: msg.collab_type,
              awareness: msg.data.encode_v1().into(),
            },
          });
        }
      },
      Err(err) => {
        tracing::error!(
          "failed to read awareness stream message for workpsace {}: {:?}",
          self.workspace_id,
          err
        );
        ctx.stop();
      },
    }
  }
}

impl Handler<Join> for Workspace {
  type Result = ();

  fn handle(&mut self, msg: Join, _ctx: &mut Self::Context) -> Self::Result {
    if msg.workspace_id == self.workspace_id {
      let handle = SessionHandle::new(msg.collab_origin, msg.addr);
      self.sessions_by_client_id.insert(msg.session_id, handle);
      tracing::trace!(
        "attached session `{}` to workspace {}",
        msg.session_id,
        msg.workspace_id
      );
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
        ctx.notify_later(
          Terminate {
            workspace_id: self.workspace_id,
          },
          std::time::Duration::from_secs(60),
        );
      }
    }
  }
}

impl Handler<Terminate> for Workspace {
  type Result = ();

  fn handle(&mut self, msg: Terminate, ctx: &mut Self::Context) -> Self::Result {
    if msg.workspace_id == self.workspace_id {
      if self.sessions_by_client_id.is_empty() {
        ctx.stop();
      }
    }
  }
}

impl Handler<WsInput> for Workspace {
  type Result = AtomicResponse<Self, ()>;
  fn handle(&mut self, msg: WsInput, _: &mut Self::Context) -> Self::Result {
    let store = self.store.clone();
    if let Some(sender) = self.sessions_by_client_id.get(&msg.sender) {
      let sender = sender.conn.clone();
      AtomicResponse::new(Box::pin(
        Self::hande_ws_input(store, sender, msg).into_actor(self),
      ))
    } else {
      AtomicResponse::new(Box::pin(fut::ready(())))
    }
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

#[derive(actix::Message)]
#[rtype(result = "()")]
pub struct Terminate {
  pub workspace_id: WorkspaceId,
}

#[derive(actix::Message)]
#[rtype(result = "()")]
struct Snapshot;
