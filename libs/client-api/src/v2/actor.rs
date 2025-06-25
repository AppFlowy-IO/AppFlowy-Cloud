use crate::v2::compactor::ChannelReceiverCompactor;
use crate::v2::controller::{ConnectionStatus, DisconnectedReason, Options};
use crate::v2::db::Db;
use crate::v2::ObjectId;
use crate::{sync_debug, sync_error, sync_info, sync_trace, sync_warn};
use app_error::AppError;
use appflowy_proto::{
  AccessChangedReason, ClientMessage, Rid, ServerMessage, UpdateFlags, WorkspaceNotification,
};
use arc_swap::ArcSwap;
use bytes::BytesMut;
use client_api_entity::CollabType;
use collab::core::collab_state::{InitState, State, SyncState};
use collab::preclude::Collab;
use collab_rt_protocol::{CollabRef, WeakCollabRef};
use dashmap::DashMap;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use shared_entity::response::AppResponseError;
use std::collections::HashMap;
use std::fmt::{Display, Formatter, Write};
use std::hash::{Hash, Hasher};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::select;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;
use tokio::time::MissedTickBehavior;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream};
use tokio_util::sync::CancellationToken;
use tracing::{error, instrument};
use uuid::Uuid;
use yrs::block::ClientID;
use yrs::sync::{Awareness, AwarenessUpdate};
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{ReadTxn, StateVector, Transact, Transaction, Update, WriteTxn};

pub(super) struct WorkspaceControllerActor {
  options: Options,
  status_rx: tokio::sync::watch::Receiver<ConnectionStatus>,
  status_tx: tokio::sync::watch::Sender<ConnectionStatus>,
  mailbox: WorkspaceControllerMailbox,
  last_message_id: Arc<ArcSwap<Rid>>,
  /// Cache for collabs actually existing in the memory, along with their type.
  cache: Arc<DashMap<ObjectId, CachedCollab>>,
  /// Persistent database handle.
  db: Db,
  notification_tx: tokio::sync::broadcast::Sender<WorkspaceNotification>,
  /// Used to record recently changed collabs
  changed_collab_sender: tokio::sync::broadcast::Sender<ChangedCollab>,
  #[cfg(debug_assertions)]
  pub skip_realtime_message: AtomicBool,
}

impl WorkspaceControllerActor {
  const PING_INTERVAL: Duration = Duration::from_secs(4);
  const REMOTE_ORIGIN: &'static str = "af";

  pub fn new(db: Db, options: Options, last_message_id: Rid) -> Arc<Self> {
    let (changed_collab_sender, _) = tokio::sync::broadcast::channel(10);
    let (status_tx, status_rx) = tokio::sync::watch::channel(ConnectionStatus::default());
    let (message_tx, message_rx) = tokio::sync::mpsc::unbounded_channel();
    let (notification_tx, _) = tokio::sync::broadcast::channel(100);
    let actor = Arc::new(WorkspaceControllerActor {
      options,
      status_rx,
      status_tx,
      mailbox: message_tx,
      last_message_id: Arc::new(ArcSwap::new(last_message_id.into())),
      cache: Arc::new(DashMap::new()),
      db,
      #[cfg(debug_assertions)]
      skip_realtime_message: AtomicBool::new(false),
      notification_tx,
      changed_collab_sender,
    });
    tokio::spawn(Self::actor_loop(
      Arc::downgrade(&actor),
      ChannelReceiverCompactor::new(message_rx),
    ));
    actor
  }

  pub fn subscribe_changed_collab(&self) -> tokio::sync::broadcast::Receiver<ChangedCollab> {
    self.changed_collab_sender.subscribe()
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

  /// Remove colla from the cache
  pub fn remove_collab(&self, object_id: &ObjectId) {
    self.cache.remove(object_id);
  }

  /// Remove collab from cache and delete the object from db
  pub fn delete_collab(&self, object_id: &ObjectId) -> anyhow::Result<()> {
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
  pub async fn bind_and_cache_collab_ref(
    actor: &Arc<Self>,
    collab_ref: &CollabRef,
    collab_type: CollabType,
  ) -> anyhow::Result<()> {
    let mut collab = collab_ref.write().await;
    let collab = (*collab).borrow_mut();
    let object_id: ObjectId = collab.object_id().parse()?;

    let entry = actor.cache.entry(object_id);
    Self::bind(actor, collab, collab_type)?;
    entry.insert(CachedCollab::new(Arc::downgrade(collab_ref), collab_type));
    Ok(())
  }

  pub fn cache_collab_ref(
    &self,
    object_id: ObjectId,
    collab_ref: &CollabRef,
    collab_type: CollabType,
  ) {
    self.cache.insert(
      object_id,
      CachedCollab::new(Arc::downgrade(collab_ref), collab_type),
    );
  }

  pub async fn unbind(&self, object_id: &ObjectId) {
    if let Some(collab) = self.get_collab(object_id) {
      sync_trace!("unbind collab {}/{}", self.workspace_id(), object_id);
      let mut guard = collab.write().await;
      let collab = (*guard).borrow_mut();
      let awareness = collab.get_awareness();
      if let Err(err) = unobserve_update(object_id, awareness) {
        sync_error!(
          "failed to unobserve {}/{} update: {}",
          self.workspace_id(),
          object_id,
          err
        );
      }

      sync_trace!(
        "unobserve awareness for collab {}/{}",
        self.workspace_id(),
        object_id
      );
      unobserve_awareness(awareness);
    }
  }

  #[instrument(level = "trace", skip_all, err)]
  pub fn bind(
    actor: &Arc<Self>,
    collab: &mut Collab,
    collab_type: CollabType,
  ) -> anyhow::Result<()> {
    let object_id: ObjectId = collab.object_id().parse()?;
    let client_id = actor.db.client_id();
    collab_type.validate_require_data(collab)?;
    sync_info!(
      "binding collab {}/{}/{} for client:{}",
      actor.workspace_id(),
      object_id,
      collab_type,
      client_id,
    );

    let sync_state = collab.get_state().clone();
    let last_message_id = actor.last_message_id.clone();
    sync_state.set_init_state(InitState::Loading);

    sync_trace!("init collab {}/{}", actor.workspace_id(), object_id);
    if !actor.db.init_collab(&object_id, collab, &collab_type)? {
      tracing::debug!("loading collab {} from local db", object_id);
      actor.db.load(collab, true)?;
    }
    sync_state.set_init_state(InitState::Initialized);
    let client_id = actor.db.client_id();
    sync_state.set_sync_state(SyncState::InitSyncBegin);

    // Register callback on this collab to observe incoming updates
    observe_update(
      collab_type,
      object_id,
      sync_state,
      last_message_id,
      Arc::downgrade(actor),
      client_id,
      collab.get_awareness(),
    )?;

    // Only observe awareness changed if the collab supports it
    let sync_awareness = collab_type.awareness_enabled() || cfg!(debug_assertions);
    if sync_awareness {
      let awareness = collab.get_awareness();
      observe_awareness(actor, collab_type, object_id, client_id, awareness);
      actor.publish_awareness(object_id, collab_type, awareness.update()?);
    }

    actor.publish_manifest(object_id, collab, collab_type);
    Ok(())
  }

  #[instrument(level = "trace", skip_all)]
  pub(crate) fn set_connection_status(&self, status: ConnectionStatus) {
    sync_debug!("set connection status: {:?}", status);
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
            sync_error!("failed to send ping: {}", err);
            actor.set_connection_status(ConnectionStatus::Disconnected {
              reason: Some(DisconnectedReason::Unexpected( err.to_string().into())),
            });
          }
        }
      }
    }
  }

  #[instrument(level = "trace", skip_all)]
  async fn handle_action(actor: &Arc<Self>, action: WorkspaceAction) {
    let id = actor.db.client_id();
    sync_trace!("[{}] action {:?}", id, action);
    match action {
      WorkspaceAction::Connect { ack, access_token } => {
        sync_debug!(
          "[{}] start websocket connect with token: {}",
          id,
          access_token
        );
        match Self::handle_connect(actor, access_token).await {
          Ok(_) => {
            let _ = ack.send(Ok(()));
          },
          Err(err) => {
            sync_error!("[{}] failed to connect: {}", id, err);
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
          sync_trace!("[{}] disconnected", id);
          actor.set_connection_status(ConnectionStatus::Disconnected {
            reason: Some(DisconnectedReason::UserDisconnect(
              "User disconnect successfully".into(),
            )),
          });
          let _ = ack.send(Ok(()));
        },
        Err(err) => {
          sync_warn!("[{}] failed to disconnect: {}", id, err);
          actor.set_connection_status(ConnectionStatus::Disconnected {
            reason: Some(DisconnectedReason::UserDisconnect(err.to_string().into())),
          });
          let _ = ack.send(Err(err));
        },
      },
      WorkspaceAction::Send(msg, source) => {
        if let Err(err) = actor.handle_send(msg, source).await {
          sync_error!("[{}] failed to send client message: {}", id, err);
          actor.set_connection_status(ConnectionStatus::Disconnected {
            reason: Some(DisconnectedReason::Unexpected(err.to_string().into())),
          });
        }
      },
    }
  }

  #[instrument(level = "trace", skip_all)]
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
        UpdateFlags::Lib0v1 => self.db.save_update(object_id, rid, update, source),
        UpdateFlags::Lib0v2 => {
          let update_v1 = Update::decode_v2(update)?.encode_v1();
          self.db.save_update(object_id, rid, &update_v1, source)
        },
      }?;
    };
    if let ActionSource::Local = source {
      if let Err(err) = self.send_message(msg).await {
        sync_error!("Failed to send message: {}", err);
        self.set_connection_status(ConnectionStatus::Disconnected {
          reason: Some(DisconnectedReason::Unexpected(err.to_string().into())),
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
      sync_debug!("[{}] sending message: {:?}", self.db.client_id(), msg);
      {
        let bytes = msg.into_bytes()?;
        let mut sink = sink.lock().await;
        sink.send(Message::Binary(bytes)).await?;
      }
      if let Some((object_id, sync_state)) = sync_state {
        self.set_collab_sync_state(&object_id, sync_state).await;
      }
    } else {
      // When the connection is disconnected, we need to check if the reason is retriable.
      // If the current disconnection status is retriable, we trigger a reconnection attempt.
      // Once reconnection is initiated, additional reconnection triggers will be ignored
      // until the current reconnection process completes.
      let should_retry = self
        .status_rx
        .borrow()
        .disconnected_reason()
        .as_ref()
        .map(|v| v.retriable_when_editing())
        .unwrap_or(false);

      if should_retry {
        self.set_connection_status(ConnectionStatus::StartReconnect);
      }
      sync_trace!("Skip sending message: sink");
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

  #[instrument(level = "debug", skip_all, err)]
  pub(crate) async fn handle_connect(
    actor: &Arc<Self>,
    access_token: String,
  ) -> Result<(), AppResponseError> {
    match &*actor.status_rx.borrow() {
      ConnectionStatus::Connecting { .. } => {
        sync_info!(
          "[{}] websocket already connecting, skipping connect",
          actor.db.client_id()
        );
        return Ok(());
      },
      ConnectionStatus::Connected { .. } => {
        sync_info!(
          "[{}] websocket already connected, skipping connect",
          actor.db.client_id()
        );
        return Ok(());
      },
      ConnectionStatus::Disconnected { .. } => {},
      ConnectionStatus::StartReconnect => {},
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
        sync_info!("[{}] connection established failed", actor.db.client_id());
        actor.set_connection_status(ConnectionStatus::Disconnected {
          reason: Some(DisconnectedReason::Unexpected(
            "Establish connect failed".into(),
          )),
        });
      },
      Some(connection) => {
        sync_debug!("[{}] connected to {}", client_id, actor.options.url);
        let (sink, stream) = connection.split();
        let sink = Arc::new(Mutex::new(sink));
        actor.set_connection_status(ConnectionStatus::Connected {
          sink,
          cancel: cancel.clone(),
        });

        if let Err(err) = actor.publish_pending_collabs().await {
          sync_error!("failed to publish pending collabs: {}", err);
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
    sync_debug!("websocket receiver task started");
    let reason = Self::remote_receiver_loop(weak_actor.clone(), stream, cancel.clone())
      .await
      .err();

    if let Some(actor) = weak_actor.upgrade() {
      if reason.is_some() {
        sync_error!("failed to receive messages from server: {:?}", reason);
      } else {
        sync_debug!("websocket receiver task ended");
      }
      actor.set_connection_status(ConnectionStatus::Disconnected { reason });
    }
  }

  async fn remote_receiver_loop(
    weak_actor: Weak<WorkspaceControllerActor>,
    mut stream: SplitStream<WsConn>,
    cancel: CancellationToken,
  ) -> Result<(), DisconnectedReason> {
    let mut buf = BytesMut::new();
    while let Some(res) = stream.next().await {
      if cancel.is_cancelled() {
        sync_trace!("remote receiver loop cancelled");
        return Err(DisconnectedReason::UserDisconnect("User disconnect".into()));
      }
      let actor = match weak_actor.upgrade() {
        Some(inner) => inner,
        None => {
          sync_trace!("remote receiver loop ended - actor dropped");
          break;
        },
      };
      let msg = res?;
      #[cfg(debug_assertions)]
      {
        if actor
          .skip_realtime_message
          .load(std::sync::atomic::Ordering::SeqCst)
        {
          sync_trace!("skipping realtime message");
          continue;
        }
      }
      match msg {
        Message::Binary(bytes) => {
          sync_trace!("[WsMessage] received binary: len:{}", bytes.len());
          let msg = ServerMessage::from_bytes(&bytes)?;
          actor.handle_receive(msg).await.map_err(|err| {
            DisconnectedReason::CannotHandleReceiveMessage(err.to_string().into())
          })?;
        },
        Message::Text(_) => {
          sync_error!("text messages are not supported");
        },
        Message::Ping(_) => { /* do nothing */ },
        Message::Pong(_) => { /* do nothing */ },
        Message::Frame(frame) => {
          buf.extend_from_slice(frame.payload());
          if frame.header().is_final {
            let bytes = std::mem::take(&mut buf);
            sync_trace!(
              "[WsMessage] received final frame, len:{}, total:{}",
              frame.len(),
              bytes.len()
            );
            let msg = ServerMessage::from_bytes(&bytes)?;
            actor.handle_receive(msg).await.map_err(|err| {
              DisconnectedReason::CannotHandleReceiveMessage(err.to_string().into())
            })?;
          } else {
            sync_trace!("[WsMessage] received frame: len:{}", frame.len());
          }
        },
        Message::Close(close) => {
          match close {
            None => sync_info!("received close request from server"),
            Some(frame) => sync_info!(
              "received close request from server: {} - {}",
              frame.code,
              frame.reason
            ),
          }
          return Err(DisconnectedReason::ServerForceClose);
        },
      }
    }
    sync_debug!("websocket receiver loop ended");
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
        sync_trace!(
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
            let update = tx.encode_diff_v1(&sv); // encode state without pending updates
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
          if let Some(missing) = missing {
            self.send_message(missing).await?;
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
          UpdateFlags::Lib0v1 if update != Update::EMPTY_V1 => Update::decode_v1(&update)?,
          UpdateFlags::Lib0v2 if update != Update::EMPTY_V2 => Update::decode_v2(&update)?,
          _ => return Ok(()),
        };
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
        self.delete_collab(&object_id)?;
      },
      ServerMessage::Notification { notification } => {
        sync_info!("received notification: {:?}", notification);
        self.send_notification(notification).await;
      },
    }
    Ok(())
  }

  async fn send_notification(&self, notification: WorkspaceNotification) {
    sync_trace!("Receive server notification: {:?}", notification);
    match &notification {
      WorkspaceNotification::UserProfileChange { .. } => {},
      WorkspaceNotification::ObjectAccessChanged { object_id, reason } => {
        if matches!(reason, AccessChangedReason::ObjectDeleted) {
          self.unbind(object_id).await;
        }
      },
    }

    if let Err(err) = self.notification_tx.send(notification) {
      sync_error!("Failed to send server notification, error: {:?}", err);
    }
  }

  async fn publish_pending_collabs(&self) -> anyhow::Result<()> {
    let last_message_id = self.last_message_id();
    let mut pending: Vec<_> = self
      .cache
      .iter()
      .map(|e| (*e.key(), e.value().collab_type))
      .collect();

    if pending.is_empty() {
      return Ok(());
    }

    sync_info!(
      "[{}] Publishing pending collabs: {}",
      self.client_id(),
      pending.len()
    );
    sync_debug!("[{}] has pending collabs: {:?}", self.client_id(), pending);
    // Sort by collab_type: Document > Database > Folder
    pending.sort_by_key(|(_, collab_type)| match collab_type {
      CollabType::Document => 0,
      CollabType::Database => 1,
      CollabType::Folder => 2,
      _ => 3,
    });

    let mut inactive_collab = vec![];
    for (object_id, collab_type) in pending {
      if let Some(collab_ref) = self.get_collab(&object_id) {
        let state_vector = collab_ref.read().await.borrow().transact().state_vector();
        let manifest = ClientMessage::Manifest {
          object_id,
          collab_type,
          last_message_id,
          state_vector: state_vector.encode_v1(),
        };
        sync_trace!("publish pending collab: {:#?}", manifest);
        self.trigger(WorkspaceAction::Send(manifest, ActionSource::Local));
      } else {
        // remove collab that already dropped
        self.remove_collab(&object_id);

        // Collabs not in memory are considered inactive. We need to sync these when
        // connection is established to handle changes made while offline.
        sync_debug!(
          "[{}] pending collab {}/{} is inactive",
          self.client_id(),
          object_id,
          collab_type
        );
        inactive_collab.push((object_id, collab_type));
      }
    }

    let cancel_token = CancellationToken::new();
    let cancel_token_clone = cancel_token.clone();
    let mut connect_status = self.status_tx.subscribe();
    tokio::spawn(async move {
      select! {
        _ = cancel_token_clone.cancelled() => {
          sync_info!("Deferred sync finished");
        }
        _ = connect_status.changed() => {
          if !matches!(*connect_status.borrow(), ConnectionStatus::Connected { .. }) {
            cancel_token_clone.cancel();
            sync_info!("Connection disconnect, cancel publishing inactive collabs");
          }
        }
      }
    });

    self.spawn_publish_inactive_collabs(inactive_collab, cancel_token);
    Ok(())
  }

  /// Publish inactive collabs in the background.
  pub fn spawn_publish_inactive_collabs(
    &self,
    collabs: Vec<(ObjectId, CollabType)>,
    cancel_token: CancellationToken,
  ) {
    if collabs.is_empty() {
      return;
    }

    sync_info!("Publishing inactive collabs: {}", collabs.len());
    let last_message_id = self.last_message_id();
    let sender = self.mailbox.clone();
    let db = self.db.clone();
    let cache = Arc::downgrade(&self.cache);
    tokio::spawn(async move {
      for chunk in collabs.chunks(10) {
        if cancel_token.is_cancelled() {
          sync_info!("Deferred sync cancelled due to disconnection");
          break;
        }

        loop {
          let num_of_unsynced_collab = match cache.upgrade() {
            None => break,
            Some(cache) => num_of_unsynced_collab(cache),
          };
          if num_of_unsynced_collab >= 10 {
            sync_trace!(
              "Too many unsynced collabs ({}), delaying inactive collab sync",
              num_of_unsynced_collab
            );
            tokio::time::sleep(Duration::from_secs(5)).await;
            if cancel_token.is_cancelled() {
              sync_info!("Deferred sync cancelled during wait");
              return;
            }
          } else {
            break;
          }
        }

        let object_ids: Vec<_> = chunk.iter().map(|(object_id, _)| object_id).collect();
        match db.batch_get_state_vector(&object_ids) {
          Ok(vectors) => {
            if let Err(err) = publish_inactive_collab(last_message_id, &sender, chunk, vectors) {
              sync_error!("Failed to publish inactive collab: {}", err);
              return;
            }
          },
          Err(err) => {
            sync_error!("Failed to get state vectors for batch: {}", err);
          },
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
      }

      cancel_token.cancel();
    });
  }

  fn check_missing_updates(
    tx: Transaction,
    object_id: ObjectId,
    collab_type: CollabType,
    local_message_id: Rid,
  ) -> anyhow::Result<Option<ClientMessage>> {
    if tx.store().pending_update().is_some() || tx.store().pending_ds().is_some() {
      let sv = tx.state_vector();
      sync_trace!("collab {} detected missing updates: {:?}", object_id, sv);
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

  /// Applies or persists remote updates for collaborative objects.
  ///
  /// # Arguments
  /// * `object_id` - The identifier of the collaborative object
  /// * `collab_type` - The type of collaboration
  /// * `rid` - The message identifier for this update
  /// * `update` - The update data to apply
  ///
  /// # Behavior
  /// - For active collabs (in memory): Directly applies the update and checks for missing updates
  /// - For inactive collabs: Encodes and persists the update to the database
  ///
  /// Sets the sync state to SyncFinished when successful or triggers manifest publication
  /// if missing updates are detected.
  async fn save_remote_update(
    &self,
    object_id: ObjectId,
    collab_type: CollabType,
    rid: Rid,
    update: Update,
  ) -> anyhow::Result<()> {
    if let Some(collab_ref) = self.get_collab(&object_id) {
      sync_debug!(
        "[{}] applying remote update for collab {}/{} active: {:#?}",
        self.db.client_id(),
        object_id,
        collab_type,
        update
      );

      let mut lock = collab_ref.write().await;
      let collab = (*lock).borrow_mut();
      let doc = collab.get_awareness().doc();
      let mut tx = doc.transact_mut_with(rid.into_bytes().as_ref());
      tx.apply_update(update)?;

      // We try to prune missing updates. If there were any, return true.
      // Server, when requested, will resend "continuous" missing updates
      // (with no holes inside) so on the second resend we either get all
      // missing updates or none at all.
      let missing = tx.prune_pending();
      drop(tx);

      // If there are no missing updates, we can set the sync state to SyncFinished
      // If there are missing updates, we will send a manifest to request them
      match missing {
        None => {
          collab.set_sync_state(SyncState::SyncFinished);
        },
        Some(update) => {
          sync_debug!(
            "[{}] found missing update:{:#?} for {} - sending manifest",
            self.db.client_id(),
            update,
            object_id
          );
          self.publish_manifest(object_id, collab, collab_type);
        },
      }
    } else {
      sync_trace!(
        "storing remote update for collab {}/{} inactive: {:#?}",
        object_id,
        collab_type,
        update
      );
      let bytes = update.encode_v1();
      self
        .persist_update(
          object_id,
          collab_type,
          Some(rid),
          bytes,
          ActionSource::Remote(rid),
        )
        .await?;
    }
    Ok(())
  }

  /// Saves the provided update to the persistent database and checks if there are any missing
  /// updates in the collaboration sequence. If gaps are detected in the update history, it
  /// automatically triggers a manifest message to request the missing updates.
  ///
  /// It is primarily used when receiving updates for collaborative objects that
  /// aren't currently active in memory. Active objects handle their updates through the
  /// in-memory collaboration object directly.
  async fn persist_update(
    &self,
    object_id: ObjectId,
    collab_type: CollabType,
    last_message_id: Option<Rid>,
    update_bytes: Vec<u8>,
    action_source: ActionSource,
  ) -> anyhow::Result<()> {
    if matches!(action_source, ActionSource::Remote(_)) {
      let _ = self.changed_collab_sender.send(ChangedCollab {
        id: object_id,
        collab_type,
      });
    }

    let missing = self
      .db
      .save_update(&object_id, last_message_id, &update_bytes, action_source)?;
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

  async fn save_awareness_update(
    &self,
    object_id: ObjectId,
    update: AwarenessUpdate,
  ) -> anyhow::Result<()> {
    if let Some(collab_ref) = self.get_collab(&object_id) {
      let mut lock = collab_ref.write().await;
      let collab = (*lock).borrow_mut();

      sync_trace!(
        "applying awareness update for active collab {}: {:#?}",
        object_id,
        update
      );
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
        sync_trace!("[{}] cancelling connection", self.db.client_id());
        cancel.cancel();
        Ok(())
      },
      ConnectionStatus::Disconnected { .. } => Ok(()),
      ConnectionStatus::StartReconnect => Ok(()),
    }
  }

  fn publish_manifest(&self, object_id: ObjectId, collab: &Collab, collab_type: CollabType) {
    let last_message_id = self.last_message_id();
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
      object_id,
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
    tracing::trace!(
      "[{}] publishing awareness update for {}: {:#?}",
      self.db.client_id(),
      object_id,
      update
    );
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
    let mut url = format!(
      "{}/{}?clientId={}&deviceId={}",
      options.url, options.workspace_id, client_id, options.device_id
    );
    sync_info!("establishing WebSocket connection to: {}", url);
    // don't include auth token in the log message (or maybe it doesn't matter?)
    write!(url, "&token={}", access_token).unwrap();
    if options.sync_eagerly {
      write!(url, "&lastMessageId={}", last_message_id).unwrap();
    }
    let req = url.into_client_request()?;
    let config = WebSocketConfig {
      max_frame_size: None,
      ..WebSocketConfig::default()
    };
    let fut = connect_async_with_config(req, Some(config), false);
    tokio::select! {
      res = fut => {
        match res {
          Ok((stream, _resp)) => {
            sync_info!("establishing WebSocket successfully");
            Ok(Some(stream))
          },
          Err(err) => {
            sync_error!("establishing WebSocket failed");
            Err(AppError::from(err).into())
          }
        }
      }
      _ = cancel.cancelled() => {
        sync_debug!("establishing connection cancelled");
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

impl Display for ActionSource {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      ActionSource::Local => f.write_str("local"),
      ActionSource::Remote(_) => f.write_str("remote"),
    }
  }
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

/// Contains cached information about a collab document
#[derive(Clone)]
struct CachedCollab {
  collab_ref: WeakCollabRef,
  collab_type: CollabType,
}

impl CachedCollab {
  fn new(collab_ref: WeakCollabRef, collab_type: CollabType) -> Self {
    Self {
      collab_ref,
      collab_type,
    }
  }

  fn upgrade(&self) -> Option<CollabRef> {
    self.collab_ref.upgrade()
  }
}

#[derive(Debug, Clone)]
pub struct ChangedCollab {
  pub id: ObjectId,
  pub collab_type: CollabType,
}

impl PartialEq for ChangedCollab {
  fn eq(&self, other: &Self) -> bool {
    self.id == other.id
  }
}
impl Eq for ChangedCollab {}

impl Hash for ChangedCollab {
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.id.hash(state);
  }
}
impl std::borrow::Borrow<ObjectId> for ChangedCollab {
  fn borrow(&self) -> &ObjectId {
    &self.id
  }
}

const OBSERVER_KEY: &str = "af";

fn unobserve_awareness(awareness: &Awareness) {
  awareness.unobserve_change(OBSERVER_KEY);
}

#[inline]
fn observe_awareness(
  actor: &Arc<WorkspaceControllerActor>,
  collab_type: CollabType,
  object_id: ObjectId,
  client_id: ClientID,
  awareness: &Awareness,
) {
  let weak_inner = Arc::downgrade(actor);
  awareness.on_change_with(OBSERVER_KEY, move |awareness, e, origin| {
    if let Some(inner) = weak_inner.upgrade() {
      if origin.map(|o| o.as_ref()) != Some(WorkspaceControllerActor::REMOTE_ORIGIN.as_bytes()) {
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
}

fn unobserve_update(object_id: &ObjectId, awareness: &Awareness) -> anyhow::Result<()> {
  awareness.doc().unobserve_update_v1(OBSERVER_KEY)?;
  sync_trace!("unobserve update for {}", object_id);
  Ok(())
}

#[inline]
fn observe_update(
  collab_type: CollabType,
  object_id: ObjectId,
  sync_state: Arc<State>,
  last_message_id: Arc<ArcSwap<Rid>>,
  weak_inner: Weak<WorkspaceControllerActor>,
  client_id: ClientID,
  awareness: &Awareness,
) -> anyhow::Result<()> {
  awareness
    .doc()
    .observe_update_v1_with(OBSERVER_KEY, move |tx, e| {
      if let Some(inner) = weak_inner.upgrade() {
        let rid: ActionSource = tx
          .origin()
          .and_then(|origin| Rid::from_bytes(origin.as_ref()).ok())
          .into();
        sync_trace!(
          "[{}] emit collab update {:?} {:#?} ",
          client_id,
          rid,
          Update::decode_v1(&e.update).unwrap()
        );
        if let ActionSource::Remote(rid) = rid {
          sync_trace!("[{}] {} received collab from remote", client_id, object_id);
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
  Ok(())
}

fn publish_inactive_collab(
  last_message_id: Rid,
  sender: &UnboundedSender<WorkspaceAction>,
  chunk: &[(ObjectId, CollabType)],
  vectors: Vec<(ObjectId, StateVector)>,
) -> anyhow::Result<()> {
  let mut collab_types = chunk
    .iter()
    .map(|(object_id, collab_type)| (object_id, *collab_type))
    .collect::<HashMap<_, _>>();

  for (object_id, state_vector) in vectors {
    if let Some(collab_type) = collab_types.remove(&object_id) {
      let manifest = ClientMessage::Manifest {
        object_id,
        collab_type,
        last_message_id,
        state_vector: state_vector.encode_v1(),
      };

      sender.send(WorkspaceAction::Send(manifest, ActionSource::Local))?;
    }
  }
  Ok(())
}

fn num_of_unsynced_collab(cache: Arc<DashMap<ObjectId, CachedCollab>>) -> usize {
  cache
    .iter()
    .filter(|entry| {
      if let Some(collab) = entry.value().upgrade() {
        collab
          .try_read()
          .map(|guard| guard.borrow().get_state().sync_state() != SyncState::SyncFinished)
          .unwrap_or(true)
      } else {
        false
      }
    })
    .count()
}
