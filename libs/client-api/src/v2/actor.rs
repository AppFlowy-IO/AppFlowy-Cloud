use crate::v2::compactor::ChannelReceiverCompactor;
use crate::v2::controller::{ConnectionStatus, DisconnectedReason, Options};
use crate::v2::db::Db;
use crate::v2::ObjectId;
use app_error::AppError;
use appflowy_proto::{ClientMessage, Rid, ServerMessage, UpdateFlags, WorkspaceNotification};
use arc_swap::ArcSwap;
use bytes::BytesMut;
use client_api_entity::CollabType;
use collab::core::collab_state::{InitState, SyncState};
use collab::preclude::Collab;
use collab_rt_protocol::{CollabRef, WeakCollabRef};
use dashmap::DashMap;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use shared_entity::response::AppResponseError;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Weak};
use tokio::select;
use tokio::sync::Mutex;
use tokio::time::{Duration, MissedTickBehavior};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, trace};
use uuid::Uuid;
use yrs::block::ClientID;
use yrs::sync::AwarenessUpdate;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{ReadTxn, StateVector, Transact, Transaction, Update};

pub(super) struct WorkspaceControllerActor {
  options: Options,
  status_rx: tokio::sync::watch::Receiver<ConnectionStatus>,
  status_tx: tokio::sync::watch::Sender<ConnectionStatus>,
  mailbox: WorkspaceControllerMailbox,
  last_message_id: Arc<ArcSwap<Rid>>,
  /// Cache for collabs actually existing in the memory.
  cache: DashMap<ObjectId, WeakCollabRef>,
  /// Persistent database handle.
  db: Db,
  notification_tx: tokio::sync::broadcast::Sender<WorkspaceNotification>,
  #[cfg(debug_assertions)]
  pub skip_realtime_message: AtomicBool,
}

impl WorkspaceControllerActor {
  const PING_INTERVAL: Duration = Duration::from_secs(4);
  const REMOTE_ORIGIN: &'static str = "af";

  pub fn new(db: Db, options: Options, last_message_id: Rid) -> Arc<Self> {
    let (status_tx, status_rx) = tokio::sync::watch::channel(ConnectionStatus::default());
    let (message_tx, message_rx) = tokio::sync::mpsc::unbounded_channel();
    let (notification_tx, _) = tokio::sync::broadcast::channel(100);
    let actor = Arc::new(WorkspaceControllerActor {
      options,
      status_rx,
      status_tx,
      mailbox: message_tx,
      last_message_id: Arc::new(ArcSwap::new(last_message_id.into())),
      cache: DashMap::new(),
      db,
      #[cfg(debug_assertions)]
      skip_realtime_message: AtomicBool::new(false),
      notification_tx,
    });
    tokio::spawn(Self::actor_loop(
      Arc::downgrade(&actor),
      ChannelReceiverCompactor::new(message_rx),
    ));
    actor
  }

  pub fn subscribe_notification(&self) -> tokio::sync::broadcast::Receiver<WorkspaceNotification> {
    self.notification_tx.subscribe()
  }

  pub fn client_id(&self) -> ClientID {
    self.db.client_id()
  }

  pub fn workspace_id(&self) -> &Uuid {
    &self.options.workspace_id
  }

  pub fn trigger(&self, action: WorkspaceAction) {
    if let Err(err) = self.mailbox.send(action) {
      error!("failed to send action to actor: {}", err);
    }
  }

  pub fn status_channel(&self) -> &tokio::sync::watch::Receiver<ConnectionStatus> {
    &self.status_rx
  }

  pub fn get_collab(&self, object_id: &ObjectId) -> Option<CollabRef> {
    self.cache.get(object_id)?.upgrade()
  }

  pub fn remove_collab(&self, object_id: &ObjectId) -> anyhow::Result<()> {
    self.cache.remove(object_id);
    self.db.remove_doc(object_id)?;
    Ok(())
  }

  pub fn last_message_id(&self) -> Rid {
    *self.last_message_id.load_full()
  }

  ///
  /// Binds a collaboration object to the actor and loads its data if needed.
  /// This function sets up the necessary callbacks and observers to handle
  /// collaboration updates and awareness changes.
  ///
  /// # Arguments
  ///
  /// * `actor`: Reference to the workspace controller actor managing the collaboration
  /// * `collab_ref`: Reference to the collaboration object to be bound
  /// * `collab_type`: The type of the collaboration (document, folder, etc.)
  ///
  pub async fn bind(
    actor: &Arc<Self>,
    collab_ref: &CollabRef,
    collab_type: CollabType,
  ) -> anyhow::Result<()> {
    let mut collab = collab_ref.write().await;
    let collab = (*collab).borrow_mut();
    let object_id: ObjectId = collab.object_id().parse()?;
    trace!("binding collab {}/{}", actor.workspace_id(), object_id);

    let sync_state = collab.get_state().clone();
    let last_message_id = actor.last_message_id.clone();
    sync_state.set_init_state(InitState::Loading);

    trace!("init collab {}/{}", actor.workspace_id(), object_id);
    if !actor.db.init_collab(collab)? {
      tracing::debug!("loading collab {} from local db", object_id);
      actor.db.load(collab)?;
    }
    sync_state.set_init_state(InitState::Initialized);
    // Register callback on this collab to observe incoming updates
    let weak_inner = Arc::downgrade(actor);
    let client_id = actor.db.client_id();
    sync_state.set_sync_state(SyncState::InitSyncBegin);
    let awareness = collab.get_awareness();
    awareness.doc().observe_update_v1_with("af", move |tx, e| {
      if let Some(inner) = weak_inner.upgrade() {
        let rid: ActionSource = tx
          .origin()
          .and_then(|origin| Rid::from_bytes(origin.as_ref()).ok())
          .into();
        trace!(
          "[{}] emit collab update {:?} {:#?} ",
          client_id,
          rid,
          Update::decode_v1(&e.update).unwrap()
        );
        if let ActionSource::Remote(rid) = rid {
          trace!("[{}] {} received collab from remote", client_id, object_id);
          last_message_id.rcu(|old| {
            if rid > **old {
              Arc::new(rid)
            } else {
              old.clone()
            }
          });
        } else {
          sync_state.set_sync_state(SyncState::Syncing);
        }
        inner.publish_update(object_id, collab_type, rid, e.update.clone());
      }
    })?;
    let weak_inner = Arc::downgrade(actor);
    awareness.on_change_with("af", move |awareness, e, origin| {
      if let Some(inner) = weak_inner.upgrade() {
        if origin.map(|o| o.as_ref()) != Some(Self::REMOTE_ORIGIN.as_bytes()) {
          match awareness.update_with_clients(e.all_changes()) {
            Ok(update) => inner.publish_awareness(object_id, collab_type, update),
            Err(err) => error!(
              "[{}] failed to prepare awareness update for {}: {}",
              client_id, object_id, err
            ),
          }
        }
      }
    });
    actor.publish_manifest(collab, collab_type);
    actor.publish_awareness(object_id, collab_type, awareness.update()?);
    actor.cache.insert(object_id, Arc::downgrade(collab_ref));
    Ok(())
  }

  pub(crate) fn set_connection_status(&self, status: ConnectionStatus) {
    trace!("set connection status: {:?}", status);
    self.status_tx.send_replace(status);
  }

  async fn ping(&self) -> anyhow::Result<()> {
    if let Some(conn) = self.ws_sink() {
      let mut lock = conn.lock().await;
      lock.send(Message::Ping(Vec::new())).await?;
      lock.flush().await?;
    }
    Ok(())
  }

  async fn actor_loop(
    weak_ref: Weak<WorkspaceControllerActor>,
    mut receiver: ChannelReceiverCompactor,
  ) {
    let mut keep_alive = tokio::time::interval(Self::PING_INTERVAL);
    keep_alive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
      select! {
        action = receiver.recv() => {
          match action {
            None => break,
            Some(action) => {
              if let Some(actor) = weak_ref.upgrade() {
                Self::handle_action(&actor, action).await;
              }
            }
          }
        }
        _ = keep_alive.tick() => {
          // we didn't receive any message for some time, so we send a ping to keep connection alive
          let actor = match weak_ref.upgrade() {
            Some(actor) => actor,
            None => break, // controller dropped
          };
          if let Err(err) = actor.ping().await {
            error!("failed to send ping: {}", err);
            actor.set_connection_status(ConnectionStatus::Disconnected {
              reason: Some(DisconnectedReason::Other( err.to_string().into())),
            });
          }
        }
      }
    }
  }

  async fn handle_action(actor: &Arc<Self>, action: WorkspaceAction) {
    let id = actor.db.client_id();
    trace!("[{}] action {:?}", id, action);
    match action {
      WorkspaceAction::Connect { ack, access_token } => {
        trace!(
          "[{}] start websocket connect with token: {}",
          id,
          access_token
        );
        match Self::handle_connect(actor, access_token).await {
          Ok(_) => {
            let _ = ack.send(Ok(()));
          },
          Err(err) => {
            error!("[{}] failed to connect: {}", id, err);
            let reason = DisconnectedReason::from(err.clone());
            actor.set_connection_status(ConnectionStatus::Disconnected {
              reason: Some(reason),
            });
            let _ = ack.send(Err(err));
          },
        }
      },
      WorkspaceAction::Disconnect(ack) => match actor.handle_disconnect().await {
        Ok(_) => {
          trace!("[{}] disconnected", id);
          actor.set_connection_status(ConnectionStatus::Disconnected {
            reason: Some(DisconnectedReason::UserDisconnect(
              "User disconnect successfully".into(),
            )),
          });
          let _ = ack.send(Ok(()));
        },
        Err(err) => {
          tracing::warn!("[{}] failed to disconnect: {}", id, err);
          actor.set_connection_status(ConnectionStatus::Disconnected {
            reason: Some(DisconnectedReason::UserDisconnect(err.to_string().into())),
          });
          let _ = ack.send(Err(err));
        },
      },
      WorkspaceAction::Send(msg, source) => {
        if let Err(err) = actor.handle_send(msg, source).await {
          error!("[{}] failed to send client message: {}", id, err);
          actor.set_connection_status(ConnectionStatus::Disconnected {
            reason: Some(DisconnectedReason::Other(err.to_string().into())),
          });
        }
      },
    }
  }

  async fn handle_send(&self, msg: ClientMessage, source: ActionSource) -> anyhow::Result<()> {
    if let ClientMessage::Update {
      object_id,
      flags,
      update,
      ..
    } = &msg
    {
      let rid = source.into();
      // persist
      match flags {
        UpdateFlags::Lib0v1 => self.db.save_update(object_id, rid, update),
        UpdateFlags::Lib0v2 => {
          let update_v1 = Update::decode_v2(update)?.encode_v1();
          self.db.save_update(object_id, rid, &update_v1)
        },
      }?;
    };
    if let ActionSource::Local = source {
      if let Err(err) = self.send_message(msg).await {
        error!("Failed to send message: {}", err);
        self.set_connection_status(ConnectionStatus::Disconnected {
          reason: Some(DisconnectedReason::Other(err.to_string().into())),
        });
        return Err(err);
      }
    }
    Ok(())
  }

  async fn send_message(&self, msg: ClientMessage) -> anyhow::Result<()> {
    let sync_state = match &msg {
      ClientMessage::Manifest { object_id, .. } => Some((*object_id, SyncState::InitSyncBegin)),
      ClientMessage::Update { object_id, .. } => Some((*object_id, SyncState::SyncFinished)),
      ClientMessage::AwarenessUpdate { .. } => None,
    };
    if let Some(sink) = self.ws_sink() {
      trace!("sending message: {:?}", msg);
      {
        let bytes = msg.into_bytes()?;
        let mut sink = sink.lock().await;
        sink.send(Message::Binary(bytes)).await?;
      }
      if let Some((object_id, sync_state)) = sync_state {
        self.set_collab_sync_state(&object_id, sync_state).await;
      }
    } else {
      trace!("Skip sending message: no sink");
    }
    Ok(())
  }

  async fn set_collab_sync_state(&self, object_id: &ObjectId, sync_state: SyncState) {
    if let Some(collab_ref) = self.get_collab(object_id) {
      let lock = collab_ref.read().await;
      let collab = lock.borrow();
      collab.set_sync_state(sync_state);
    }
  }

  pub(crate) async fn handle_connect(
    actor: &Arc<Self>,
    access_token: String,
  ) -> Result<(), AppResponseError> {
    match &*actor.status_rx.borrow() {
      ConnectionStatus::Connecting { .. } | ConnectionStatus::Connected { .. } => return Ok(()),
      ConnectionStatus::Disconnected { .. } => {},
    }

    let cancel = CancellationToken::new();
    actor.set_connection_status(ConnectionStatus::Connecting {
      cancel: cancel.clone(),
    });

    let last_message_id = actor.last_message_id.load_full();
    let client_id = actor.db.client_id();
    let result = Self::establish_connection(
      &actor.options,
      client_id,
      &last_message_id,
      cancel.clone(),
      access_token,
    )
    .await?;

    match result {
      None => {
        actor.set_connection_status(ConnectionStatus::Disconnected { reason: None });
      },
      Some(connection) => {
        trace!("[{}] connected to {}", client_id, actor.options.url);
        let (sink, stream) = connection.split();
        let sink = Arc::new(Mutex::new(sink));
        actor.set_connection_status(ConnectionStatus::Connected {
          sink,
          cancel: cancel.clone(),
        });

        if let Err(err) = actor.publish_pending_collabs().await {
          error!("failed to publish pending collabs: {}", err);
        }
        tokio::spawn(Self::remote_receiver_task(
          Arc::downgrade(actor),
          stream,
          cancel,
        ));
      },
    }
    Ok(())
  }

  async fn remote_receiver_task(
    weak_actor: Weak<WorkspaceControllerActor>,
    stream: SplitStream<WsConn>,
    cancel: CancellationToken,
  ) {
    let reason: Option<Arc<str>> =
      match Self::remote_receiver_loop(weak_actor.clone(), stream, cancel.clone()).await {
        Ok(_) => None,
        Err(err) => Some(Arc::from(err.to_string())),
      };
    if let Some(actor) = weak_actor.upgrade() {
      error!("failed to receive messages from server: {:?}", reason);
      actor.set_connection_status(ConnectionStatus::Disconnected {
        reason: reason.map(DisconnectedReason::MessageLoopEnd),
      });
    }
  }

  async fn remote_receiver_loop(
    weak_actor: Weak<WorkspaceControllerActor>,
    mut stream: SplitStream<WsConn>,
    cancel: CancellationToken,
  ) -> anyhow::Result<()> {
    let mut buf = BytesMut::new();
    while let Some(res) = stream.next().await {
      if cancel.is_cancelled() {
        break;
      }
      let actor = match weak_actor.upgrade() {
        Some(inner) => inner,
        None => break,
      };
      let msg = res?;
      #[cfg(debug_assertions)]
      {
        if actor
          .skip_realtime_message
          .load(std::sync::atomic::Ordering::SeqCst)
        {
          trace!("skipping realtime message");
          continue;
        }
      }
      match msg {
        Message::Binary(bytes) => {
          #[cfg(feature = "message_verbose_log")]
          trace!("[WsMessage] received binary: len:{}", bytes.len());
          let msg = ServerMessage::from_bytes(&bytes)?;
          actor.handle_receive(msg).await?;
        },
        Message::Text(_) => {
          error!("text messages are not supported")
        },
        Message::Ping(_) => { /* do nothing */ },
        Message::Pong(_) => { /* do nothing */ },
        Message::Frame(frame) => {
          buf.extend_from_slice(frame.payload());
          if frame.header().is_final {
            let bytes = std::mem::take(&mut buf);
            #[cfg(feature = "message_verbose_log")]
            trace!(
              "[WsMessage] received final frame, len:{}, total:{}",
              frame.len(),
              bytes.len()
            );
            let msg = ServerMessage::from_bytes(&bytes)?;
            actor.handle_receive(msg).await?;
          } else {
            #[cfg(feature = "message_verbose_log")]
            trace!("[WsMessage] received frame: len:{}", frame.len());
          }
        },
        Message::Close(close) => {
          match close {
            None => tracing::info!("received close request from server"),
            Some(frame) => tracing::info!(
              "received close request from server: {} - {}",
              frame.code,
              frame.reason
            ),
          }
          cancel.cancel();
          break;
        },
      }
    }
    Ok(())
  }

  async fn handle_receive(&self, msg: ServerMessage) -> anyhow::Result<()> {
    match msg {
      ServerMessage::Manifest {
        object_id,
        collab_type,
        last_message_id,
        state_vector,
      } => {
        #[cfg(feature = "message_verbose_log")]
        trace!(
          "received manifest message for {} (rid: {}), sv:{:?}",
          object_id,
          last_message_id,
          state_vector
        );

        let sv = if state_vector.is_empty() {
          // If no inserts or other operations have ever been applied, the sv will be empty (i.e. []).
          StateVector::default()
        } else {
          StateVector::decode_v1(&state_vector)?
        };

        let local_message_id = self.last_message_id();
        if let Some(collab_ref) = self.get_collab(&object_id) {
          let (msg, missing) = {
            let lock = collab_ref.read().await;
            let collab = lock.borrow();
            let tx = collab.get_awareness().doc().transact();
            let update = tx.encode_state_as_update_v1(&sv);
            let msg = ClientMessage::Update {
              object_id,
              collab_type,
              flags: UpdateFlags::Lib0v1,
              update,
            };
            let missing =
              Self::check_missing_updates(tx, object_id, collab_type, local_message_id)?;
            if missing.is_some() {
              collab.set_sync_state(SyncState::Syncing);
            }
            (msg, missing)
          };
          self.send_message(msg).await?;
          if let Some(msg) = missing {
            self.send_message(msg).await?;
          }
        }
      },
      ServerMessage::Update {
        object_id,
        flags,
        last_message_id,
        update,
        collab_type,
      } => {
        // we don't need to decode update for every use case, but do so anyway to confirm
        // that it isn't malformed
        let update = match flags {
          UpdateFlags::Lib0v1 => Update::decode_v1(&update)?,
          UpdateFlags::Lib0v2 => Update::decode_v2(&update)?,
        };
        #[cfg(feature = "message_verbose_log")]
        trace!(
          "received update for {} (rid: {})",
          object_id,
          last_message_id
        );
        self
          .save_remote_update(object_id, collab_type, last_message_id, update)
          .await?;
      },
      ServerMessage::AwarenessUpdate {
        object_id,
        awareness,
        ..
      } => {
        // we don't need to decode update for every use case, but do so anyway to confirm
        // that it isn't malformed
        let update = AwarenessUpdate::decode_v1(&awareness)?;

        #[cfg(feature = "message_verbose_log")]
        trace!("received awareness update for {}", object_id);
        self.save_awareness_update(object_id, update).await?;
      },
      ServerMessage::AccessChanges {
        object_id, reason, ..
      } => {
        tracing::warn!(
          "received permission denied for {} - reason: {}",
          object_id,
          reason
        );
        self.remove_collab(&object_id)?;
      },
      ServerMessage::UserProfileChange { uid } => {
        self.send_notification(WorkspaceNotification::UserProfileChange {
          uid,
          workspace_id: self.options.workspace_id,
        });
      },
    }
    Ok(())
  }

  fn send_notification(&self, notification: WorkspaceNotification) {
    if self.notification_tx.is_empty() {
      return;
    }

    trace!("send server notification: {:?}", notification);
    if let Err(err) = self.notification_tx.send(notification) {
      error!("Failed to send server notification, error: {:?}", err)
    }
  }

  async fn publish_pending_collabs(&self) -> anyhow::Result<()> {
    let last_message_id = self.last_message_id();
    let pending: Vec<_> = self.cache.iter().map(|e| *e.key()).collect();
    for object_id in pending {
      if let Some(collab_ref) = self.get_collab(&object_id) {
        let state_vector = collab_ref.read().await.borrow().transact().state_vector();
        let manifest = ClientMessage::Manifest {
          object_id,
          collab_type: CollabType::Unknown,
          last_message_id,
          state_vector: state_vector.encode_v1(),
        };
        self.trigger(WorkspaceAction::Send(manifest, ActionSource::Local));
      }
    }
    Ok(())
  }

  fn check_missing_updates(
    tx: Transaction,
    object_id: ObjectId,
    collab_type: CollabType,
    local_message_id: Rid,
  ) -> anyhow::Result<Option<ClientMessage>> {
    if Self::has_missing_updates(&tx) {
      let sv = tx.state_vector();
      trace!("collab {} detected missing updates: {:?}", object_id, sv);
      let reply = ClientMessage::Manifest {
        object_id,
        collab_type,
        last_message_id: local_message_id,
        state_vector: sv.encode_v1(),
      };
      Ok(Some(reply))
    } else {
      Ok(None)
    }
  }

  async fn save_remote_update(
    &self,
    object_id: ObjectId,
    collab_type: CollabType,
    rid: Rid,
    update: Update,
  ) -> anyhow::Result<()> {
    if let Some(collab_ref) = self.get_collab(&object_id) {
      #[cfg(feature = "verbose_log")]
      trace!(
        "applying remote update for active collab {}: {:#?}",
        object_id,
        update
      );
      let mut lock = collab_ref.write().await;
      let collab = (*lock).borrow_mut();
      let doc = collab.get_awareness().doc();
      let mut tx = doc.transact_mut_with(rid.into_bytes().as_ref());
      tx.apply_update(update)?;
      if Self::has_missing_updates(&tx) {
        drop(tx);
        trace!("found missing updates for {} - sending manifest", object_id);
        self.publish_manifest(collab, collab_type);
      } else {
        drop(tx);
        collab.set_sync_state(SyncState::SyncFinished);
      }
    } else {
      trace!(
        "storing remote update for inactive collab {}: {:#?}",
        object_id,
        update
      );
      let bytes = update.encode_v1();
      self
        .persist_update(object_id, collab_type, Some(rid), bytes)
        .await?;
    }
    Ok(())
  }

  async fn persist_update(
    &self,
    object_id: ObjectId,
    collab_type: CollabType,
    last_message_id: Option<Rid>,
    update_bytes: Vec<u8>,
  ) -> anyhow::Result<()> {
    let missing = self
      .db
      .save_update(&object_id, last_message_id, &update_bytes)?;
    if let Some(state_vector) = missing {
      let msg = ClientMessage::Manifest {
        object_id,
        collab_type,
        last_message_id: last_message_id.unwrap_or_default(),
        state_vector: state_vector.encode_v1(),
      };
      // we received that manifest from the local client
      self.trigger(WorkspaceAction::Send(msg, ActionSource::Local));
    }
    Ok(())
  }

  fn has_missing_updates<T: ReadTxn>(tx: &T) -> bool {
    let store = tx.store();
    store.pending_update().is_some() || store.pending_ds().is_some()
  }

  async fn save_awareness_update(
    &self,
    object_id: ObjectId,
    update: AwarenessUpdate,
  ) -> anyhow::Result<()> {
    if let Some(collab_ref) = self.get_collab(&object_id) {
      let mut lock = collab_ref.write().await;
      let collab = (*lock).borrow_mut();
      collab
        .get_awareness()
        .apply_update_with(update, Self::REMOTE_ORIGIN)?;
    }
    Ok(())
  }

  async fn handle_disconnect(&self) -> anyhow::Result<()> {
    let previous_status = self
      .status_tx
      .send_replace(ConnectionStatus::Disconnected { reason: None });
    match previous_status {
      ConnectionStatus::Connected { sink, cancel } => {
        cancel.cancel();
        {
          let mut sink = sink.lock().await;
          sink.flush().await?;
          sink.close().await?;
        }
        Ok(())
      },
      ConnectionStatus::Connecting { cancel } => {
        trace!("[{}] cancelling connection", self.db.client_id());
        cancel.cancel();
        Ok(())
      },
      ConnectionStatus::Disconnected { .. } => Ok(()),
    }
  }

  fn publish_manifest(&self, collab: &Collab, collab_type: CollabType) {
    let last_message_id = self.last_message_id();
    let object_id = collab.object_id();
    let awareness = collab.get_awareness();
    let doc = awareness.doc();
    let state_vector = doc.transact().state_vector();
    tracing::debug!(
      "publishing manifest for {} (last msg id: {}): {:?}",
      object_id,
      last_message_id,
      state_vector
    );
    let msg = ClientMessage::Manifest {
      object_id: object_id.parse().unwrap(),
      collab_type,
      last_message_id,
      state_vector: state_vector.encode_v1(),
    };
    // we received that update from the local client
    self.trigger(WorkspaceAction::Send(msg, ActionSource::Local));
  }

  fn publish_update(
    &self,
    object_id: ObjectId,
    collab_type: CollabType,
    source: ActionSource,
    update_v1: Vec<u8>,
  ) {
    let msg = ClientMessage::Update {
      object_id,
      collab_type,
      flags: UpdateFlags::Lib0v1,
      update: update_v1,
    };
    // we received that update from the local client
    self.trigger(WorkspaceAction::Send(msg, source));
  }

  fn publish_awareness(
    &self,
    object_id: ObjectId,
    collab_type: CollabType,
    update: AwarenessUpdate,
  ) {
    let awareness = update.encode_v1();
    let msg = ClientMessage::AwarenessUpdate {
      object_id,
      collab_type,
      awareness,
    };
    self.trigger(WorkspaceAction::Send(msg, ActionSource::Local));
  }

  fn ws_sink(&self) -> Option<Arc<Mutex<SplitSink<WsConn, Message>>>> {
    match &*self.status_rx.borrow() {
      ConnectionStatus::Connected { sink, .. } => Some(sink.clone()),
      _ => None,
    }
  }

  async fn establish_connection(
    options: &Options,
    client_id: ClientID,
    last_message_id: &Rid,
    cancel: CancellationToken,
    access_token: String,
  ) -> Result<Option<WsConn>, AppResponseError> {
    let url = format!("{}/{}", options.url, options.workspace_id);
    info!("establishing WebSocket connection to: {}", url);
    let mut req = url.into_client_request()?;
    let headers = req.headers_mut();
    headers.insert("X-AF-Device-ID", HeaderValue::from_str(&options.device_id)?);
    headers.insert(
      "X-AF-Client-ID",
      HeaderValue::from_str(&client_id.to_string())?,
    );
    if options.sync_eagerly {
      headers.insert(
        "X-AF-Last-Message-ID",
        HeaderValue::from_str(&last_message_id.to_string())?,
      );
    }
    headers.insert("Authorization", HeaderValue::from_str(&access_token)?);
    let config = WebSocketConfig {
      max_frame_size: None,
      ..WebSocketConfig::default()
    };
    let fut = connect_async_with_config(req, Some(config), false);
    tokio::select! {
      res = fut => {
        match res {
          Ok((stream, _resp)) => {
            info!("establishing WebSocket successfully");
            Ok(Some(stream))
          },
          Err(err) => {
            error!("establishing WebSocket failed");
            Err(AppError::from(err).into())
          }
        }
      }
      _ = cancel.cancelled() => {
        tracing::debug!("connection cancelled");
        Ok(None)
      }
    }
  }
}

#[derive(Debug)]
pub(super) enum WorkspaceAction {
  Connect {
    ack: tokio::sync::oneshot::Sender<Result<(), AppResponseError>>,
    access_token: String,
  },
  Disconnect(tokio::sync::oneshot::Sender<anyhow::Result<()>>),
  Send(ClientMessage, ActionSource),
}

#[derive(Debug, Copy, Clone)]
pub(super) enum ActionSource {
  Local,
  Remote(Rid),
}

impl From<ActionSource> for Option<Rid> {
  fn from(value: ActionSource) -> Self {
    match value {
      ActionSource::Local => None,
      ActionSource::Remote(rid) => Some(rid),
    }
  }
}

impl From<Option<Rid>> for ActionSource {
  fn from(value: Option<Rid>) -> Self {
    match value {
      None => ActionSource::Local,
      Some(rid) => ActionSource::Remote(rid),
    }
  }
}

pub(super) type WsConn = tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub(super) type WorkspaceControllerMailbox = tokio::sync::mpsc::UnboundedSender<WorkspaceAction>;
