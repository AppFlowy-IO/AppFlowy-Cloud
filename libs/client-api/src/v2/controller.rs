use super::db::Db;
use super::{ObjectId, WorkspaceId};
use crate::entity::CollabType;
use anyhow::bail;
use appflowy_proto::{ClientMessage, Rid, ServerMessage, UpdateFlags};
use arc_swap::{ArcSwap, ArcSwapOption};
use bytes::BytesMut;
use collab::core::collab_state::{InitState, SyncState};
use collab::preclude::Collab;
use collab_rt_protocol::{CollabRef, WeakCollabRef};
use dashmap::DashMap;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use std::borrow::BorrowMut;
use std::fmt::{Display, Formatter};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{Mutex, Notify};
use tokio::time::MissedTickBehavior;
use tokio_stream::wrappers::WatchStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream};
use tokio_util::sync::CancellationToken;
use yrs::block::ClientID;
use yrs::sync::AwarenessUpdate;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{merge_updates_v1, ReadTxn, StateVector, Transact, Transaction, Update};

type WsConn = tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

type ControllerSender = tokio::sync::mpsc::UnboundedSender<(ClientMessage, Option<Rid>)>;
type ControllerReceiver = tokio::sync::mpsc::UnboundedReceiver<(ClientMessage, Option<Rid>)>;

#[derive(Clone)]
pub struct WorkspaceController {
  inner: Arc<Inner>,
}

impl Drop for WorkspaceController {
  fn drop(&mut self) {
    self.inner.signal_closed();
  }
}

impl WorkspaceController {
  const RECONNECT_DELAY: Duration = Duration::from_secs(5);
  const PING_INTERVAL: Duration = Duration::from_secs(4);

  pub fn new(options: Options) -> anyhow::Result<Self> {
    let (status_tx, status_rx) = tokio::sync::watch::channel(ConnectionStatus::default());
    let shutdown = CancellationToken::new();
    let (message_tx, message_rx) = tokio::sync::mpsc::unbounded_channel();
    let db = Db::open(
      options.workspace_id,
      options.uid,
      &options.workspace_db_path,
    )?;
    let last_message_id = db.last_message_id()?;
    let inner = Arc::new(Inner {
      options,
      status_rx,
      status_tx,
      message_tx: ArcSwapOption::new(Some(Arc::new(message_tx))),
      last_message_id: Arc::new(ArcSwap::new(last_message_id.into())),
      cache: DashMap::new(),
      db,
      shutdown,
      closed: Notify::new(),
      #[cfg(debug_assertions)]
      skip_realtime_message: AtomicBool::new(false),
    });
    tokio::spawn(Self::local_receiver_loop(
      Arc::downgrade(&inner),
      message_rx,
    ));
    tokio::spawn(Self::remote_receiver_loop(
      Arc::downgrade(&inner),
      inner.status_rx.clone(),
    ));
    Ok(Self { inner })
  }

  pub fn is_connected(&self) -> bool {
    matches!(
      &*self.inner.status_rx.borrow(),
      ConnectionStatus::Connected { .. }
    )
  }

  pub fn is_disconnected(&self) -> bool {
    matches!(
      &*self.inner.status_rx.borrow(),
      ConnectionStatus::Disconnected { .. }
    )
  }

  pub async fn connect(&self) -> anyhow::Result<()> {
    if self.is_disconnected() {
      tracing::trace!("requesting connection");
      self.inner.request_reconnect();
    }
    let mut status_rx = self.inner.status_rx.clone();
    let status = status_rx
      .wait_for(|status| match status {
        ConnectionStatus::Disconnected { .. } => true,
        ConnectionStatus::Connecting { .. } => false,
        ConnectionStatus::Connected { .. } => true,
      })
      .await?;
    match &*status {
      ConnectionStatus::Disconnected { reason } => bail!(
        "failed to connect: {}",
        reason.as_deref().unwrap_or("disconnect requested")
      ),
      ConnectionStatus::Connecting { .. } => unreachable!(),
      ConnectionStatus::Connected { .. } => Ok(()),
    }
  }

  pub async fn disconnect(&self) -> anyhow::Result<()> {
    match &*self.inner.status_rx.borrow() {
      ConnectionStatus::Disconnected { .. } => return Ok(()),
      ConnectionStatus::Connecting { cancel } => {
        tracing::trace!("cancelling connection on request");
        cancel.cancel();
      },
      ConnectionStatus::Connected { sink, cancel } => {
        tracing::trace!("disconnecting on request");
        cancel.cancel();
        let mut lock = sink.lock().await;
        lock.close().await?;
      },
    }
    let mut status_rx = self.inner.status_rx.clone();
    status_rx
      .wait_for(|status| match status {
        ConnectionStatus::Disconnected { .. } => true,
        ConnectionStatus::Connecting { .. } => false,
        ConnectionStatus::Connected { .. } => false,
      })
      .await?;
    Ok(())
  }

  pub async fn close(&mut self) -> anyhow::Result<()> {
    self.inner.message_tx.swap(None); // close the message channel
    self.inner.shutdown.cancel();
    self.inner.closed.notified().await;
    tracing::trace!("close signaled");
    if let Some(conn) = self.inner.ws_sink() {
      let mut conn = conn.lock().await;
      conn.close().await?;
      tracing::trace!("web socket connection closed");
    }
    tracing::trace!("controller closed");
    Ok(())
  }

  pub fn client_id(&self) -> ClientID {
    self.inner.db.client_id()
  }

  pub fn workspace_id(&self) -> WorkspaceId {
    self.inner.options.workspace_id
  }

  pub fn last_message_id(&self) -> Rid {
    self.inner.last_message_id()
  }

  pub async fn bind(&self, collab_ref: &CollabRef, collab_type: CollabType) -> anyhow::Result<()> {
    let mut collab = collab_ref.write().await;
    let collab = (*collab).borrow_mut();
    let object_id: ObjectId = collab.object_id().parse()?;

    tracing::trace!("binding collab {}/{}", self.workspace_id(), object_id);

    let sync_state = collab.get_state().clone();
    let last_message_id = self.inner.last_message_id.clone();
    sync_state.set_init_state(InitState::Loading);
    if !self.inner.db.init_collab(collab)? {
      tracing::debug!("loading collab {} from local db", object_id);
      self.inner.db.load(collab)?;
    }
    sync_state.set_init_state(InitState::Initialized);
    // Register callback on this collab to observe incoming updates
    let weak_inner = Arc::downgrade(&self.inner);
    let client_id = self.inner.db.client_id();
    sync_state.set_sync_state(SyncState::InitSyncBegin);
    collab
      .get_awareness()
      .doc()
      .observe_update_v1_with("af", move |tx, e| {
        if let Some(inner) = weak_inner.upgrade() {
          let rid = tx
            .origin()
            .and_then(|origin| Rid::from_bytes(origin.as_ref()).ok());
          if let Some(rid) = rid {
            tracing::trace!("[{}] {} received collab from remote", client_id, object_id);
            last_message_id.rcu(|old| {
              if rid > **old {
                Arc::new(rid)
              } else {
                old.clone()
              }
            });
          }
          sync_state.set_sync_state(SyncState::Syncing);
          inner.publish_update(object_id, collab_type, rid, e.update.clone());
        }
      })
      .unwrap();

    self.inner.publish_manifest(collab, collab_type);
    self
      .inner
      .cache
      .insert(object_id, Arc::downgrade(collab_ref));
    Ok(())
  }

  async fn remote_receiver_loop(
    inner: Weak<Inner>,
    status_rx: tokio::sync::watch::Receiver<ConnectionStatus>,
  ) {
    let mut stream = WatchStream::new(status_rx);
    while let Some(status) = stream.next().await {
      tracing::trace!("status changed: {}", status);
      let inner = match inner.upgrade() {
        Some(inner) => inner,
        None => {
          tracing::debug!("connection manager has been dropped");
          break;
        },
      };
      // if status was changed to connecting, we retry creating connection
      // otherwise skip the loop iteration and wait for the next status change until we received
      // connecting signal
      let cancel = if let ConnectionStatus::Connecting { cancel } = status {
        cancel
      } else if inner.shutdown.is_cancelled() {
        tracing::debug!("connection manager has been closed");
        inner.signal_closed();
        break;
      } else {
        continue;
      };
      let client_id = inner.db.client_id();
      // connection status changed to connecting => try to establish connection
      let reconnect =
        match Self::establish_connection(&inner.options, client_id, cancel.clone()).await {
          Ok(Some(conn)) => {
            // successfully made a connection
            tracing::debug!("successfully connected to {}", inner.options.url);
            let (sink, stream) = conn.split();
            inner.set_connected(sink, cancel.clone());
            let receive_messages_loop = tokio::spawn(Self::receive_server_messages_loop(
              Arc::downgrade(&inner),
              stream,
              cancel,
            ));
            // wait for loop to complete, if it completed with failure it's a connection
            // failure, and we need to reconnect
            match receive_messages_loop.await.unwrap() {
              Ok(()) => false, // connection closed gracefully
              Err(err) => {
                // error while sending messages
                tracing::error!("failed to handle messages: {}", err);
                true
              },
            }
          },
          Ok(None) => {
            inner.set_disconnected(None); // connection establishing has been cancelled midway
            false
          },
          Err(err) => {
            // failed to make a connection, wait and retry
            tracing::error!("failed to establish WebSocket v2 connection: {}", err);
            inner.set_disconnected(Some(err.to_string()));
            tokio::time::sleep(Self::RECONNECT_DELAY).await;
            true
          },
        };

      if inner.shutdown.is_cancelled() {
        tracing::debug!("connection manager has been closed");
        inner.signal_closed();
        break;
      }

      if reconnect {
        tracing::trace!("reconnecting");
        inner.request_reconnect(); // go to the next loop iteration and retry
      }
    }
    tracing::debug!("remote receiver loop finished");
  }

  async fn establish_connection(
    options: &Options,
    client_id: ClientID,
    cancel: CancellationToken,
  ) -> anyhow::Result<Option<WsConn>> {
    let url = format!("{}/{}", options.url, options.workspace_id);
    tracing::info!("establishing WebScoket connection to: {}", url);
    let mut req = url.into_client_request()?;
    let headers = req.headers_mut();
    headers.insert("X-AF-DeviceID", HeaderValue::from_str(&options.device_id)?);
    headers.insert(
      "X-AF-ClientID",
      HeaderValue::from_str(&client_id.to_string())?,
    );
    headers.insert(
      "Authorization",
      HeaderValue::from_str(&options.access_token)?,
    );
    let config = WebSocketConfig {
      max_frame_size: None,
      ..WebSocketConfig::default()
    };
    let fut = connect_async_with_config(req, Some(config), false);
    tokio::select! {
      res = fut => {
        let (stream, _resp) = res?;
        Ok(Some(stream))
      }
      _ = cancel.cancelled() => {
        tracing::debug!("connection cancelled");
        Ok(None)
      }
    }
  }

  /// Loop task used to handle messages received from the server.
  async fn receive_server_messages_loop(
    inner: Weak<Inner>,
    mut stream: SplitStream<WsConn>,
    cancel: CancellationToken,
  ) -> anyhow::Result<()> {
    let mut buf = BytesMut::new();
    while let Some(res) = stream.next().await {
      let msg = res?;
      let inner = match inner.upgrade() {
        Some(inner) => inner,
        None => break,
      };
      #[cfg(debug_assertions)]
      {
        if inner
          .skip_realtime_message
          .load(std::sync::atomic::Ordering::SeqCst)
        {
          tracing::trace!("skipping realtime message");
          continue;
        }
      }
      match msg {
        Message::Binary(bytes) => {
          inner.handle_server_message(bytes).await?;
        },
        Message::Text(_) => {
          tracing::error!("text messages are not supported")
        },
        Message::Ping(_) => { /* do nothing */ },
        Message::Pong(_) => { /* do nothing */ },
        Message::Frame(frame) => {
          buf.extend_from_slice(frame.payload());
          if frame.header().is_final {
            let bytes = std::mem::take(&mut buf);
            inner.handle_server_message(bytes.to_vec()).await?;
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
    tracing::trace!("receive server messages loop stopped");
    Ok(())
  }

  /// Loop task used to handle messages to be sent to the server.
  async fn local_receiver_loop(inner: Weak<Inner>, mut rx: ControllerReceiver) {
    let mut keep_alive = tokio::time::interval(Self::PING_INTERVAL);
    keep_alive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let cancel = match inner.upgrade() {
      None => return,
      Some(inner) => inner.shutdown.clone(),
    };
    loop {
      tokio::select! {
        res = rx.recv() => {
          let inner = match inner.upgrade() {
            Some(inner) => inner,
            None => break, // controller dropped
          };
          if let Some((msg, last_message_id)) = res {
            if let Err(err) = inner.handle_messages(&mut rx, msg, last_message_id).await {
              tracing::error!("failed to handle message: {}", err);
              inner.set_disconnected(Some(err.to_string()));
            }
            keep_alive.reset();
          } else {
            tracing::trace!("local input channel, closed");
            inner.signal_closed();
            break;
          }
        },
        _ = keep_alive.tick() => {
          // we didn't receive any message for some time, so we send a ping to keep connection alive
          let inner = match inner.upgrade() {
            Some(inner) => inner,
            None => break, // controller dropped
          };
          if let Err(err) = inner.try_send(Message::Ping(Vec::new())).await {
              tracing::error!("failed to send ping: {}", err);
              inner.set_disconnected(Some(err.to_string()));
          }
        }
        _ = cancel.cancelled() => {
          if rx.is_empty() {
              tracing::trace!("controller closing");
              if let Some(inner) = inner.upgrade() {
                  inner.signal_closed();
              }
              break;
          }
        }
      }
    }
    tracing::debug!("local receiver loop finished");
  }
}

struct Inner {
  options: Options,
  status_rx: tokio::sync::watch::Receiver<ConnectionStatus>,
  status_tx: tokio::sync::watch::Sender<ConnectionStatus>,
  message_tx: ArcSwapOption<ControllerSender>,
  last_message_id: Arc<ArcSwap<Rid>>,
  /// Cache for collabs actually existing in the memory.
  cache: DashMap<ObjectId, WeakCollabRef>,
  /// Persistent database handle.
  db: Db,
  shutdown: CancellationToken,
  closed: Notify,

  #[cfg(debug_assertions)]
  skip_realtime_message: AtomicBool,
}

impl Inner {
  async fn handle_server_message(&self, data: Vec<u8>) -> anyhow::Result<()> {
    let msg = ServerMessage::from_bytes(&data)?;
    match msg {
      ServerMessage::Manifest {
        object_id,
        collab_type,
        last_message_id,
        state_vector,
      } => {
        tracing::trace!(
          "received manifest message for {} (rid: {})",
          object_id,
          last_message_id
        );
        let sv = StateVector::decode_v1(&state_vector)?;
        let local_message_id = self.last_message_id();
        if let Some(collab_ref) = self.get_collab(object_id) {
          let (msg, missing) = {
            let lock = collab_ref.read().await;
            let collab = lock.borrow();
            let tx = collab.get_awareness().doc().transact();
            let update = tx.encode_state_as_update_v2(&sv);
            let msg = ClientMessage::Update {
              object_id,
              collab_type,
              flags: UpdateFlags::Lib0v2,
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
        } else if !sv.is_empty() {
          // we haven't seen this collab yet, so we need to send manifest ourselves
          tracing::trace!("sending manifest for {}", object_id);
          let reply = ClientMessage::Manifest {
            object_id,
            collab_type,
            last_message_id: local_message_id,
            state_vector: StateVector::default().encode_v1(),
          };
          self.send_message(reply).await?;
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
        tracing::trace!(
          "received update for {} (rid: {})",
          object_id,
          last_message_id
        );
        self
          .save_update(object_id, Some(last_message_id), update, collab_type)
          .await?;
      },
      ServerMessage::AwarenessUpdate {
        object_id,
        awareness,
      } => {
        // we don't need to decode update for every use case, but do so anyway to confirm
        // that it isn't malformed
        let update = AwarenessUpdate::decode_v1(&awareness)?;
        tracing::trace!("received awareness update for {}", object_id);
        self.save_awareness_update(object_id, update).await?;
      },
      ServerMessage::PermissionDenied {
        object_id, reason, ..
      } => {
        tracing::warn!(
          "received permission denied for {} - reason: {}",
          object_id,
          reason
        );
        self.remove_collab(&object_id)?;
      },
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

  fn ws_sink(&self) -> Option<Arc<Mutex<SplitSink<WsConn, Message>>>> {
    match &*self.status_rx.borrow() {
      ConnectionStatus::Connected { sink, .. } => Some(sink.clone()),
      _ => None,
    }
  }

  async fn send_message(&self, msg: ClientMessage) -> anyhow::Result<()> {
    tracing::trace!("sending message: {:?}", msg);
    let sync_state = match &msg {
      ClientMessage::Manifest { object_id, .. } => Some((*object_id, SyncState::InitSyncBegin)),
      ClientMessage::Update { object_id, .. } => Some((*object_id, SyncState::SyncFinished)),
      ClientMessage::AwarenessUpdate { .. } => None,
    };
    let bytes = msg.into_bytes()?;
    if !self.try_send(Message::Binary(bytes)).await? {
      bail!("WebSocket client not connected");
    } else if let Some((object_id, sync_state)) = sync_state {
      if let Some(collab_ref) = self.get_collab(object_id) {
        let lock = collab_ref.read().await;
        let collab = lock.borrow();
        collab.set_sync_state(sync_state);
      }
    }
    Ok(())
  }

  async fn try_send(&self, msg: Message) -> anyhow::Result<bool> {
    if let Some(sink) = self.ws_sink() {
      let mut sink = sink.lock().await;
      sink.send(msg).await?;
      sink.flush().await?;
      Ok(true)
    } else {
      Ok(false)
    }
  }

  async fn handle_messages(
    &self,
    rx: &mut ControllerReceiver,
    msg: ClientMessage,
    last_message_id: Option<Rid>,
  ) -> anyhow::Result<()> {
    if let ClientMessage::Update {
      object_id,
      collab_type,
      flags: UpdateFlags::Lib0v1,
      update,
    } = msg
    {
      // try to eagerly fetch more updates if possible: this way we can merge multiple
      // updates and send them as one bigger message
      let (m1, m2) = Self::eager_prefetch(rx, object_id, collab_type, update)?;
      self.persist_and_send_message(m1, last_message_id).await?;
      if let Some(msg) = m2 {
        self.persist_and_send_message(msg, last_message_id).await?;
      }
    } else {
      self.persist_and_send_message(msg, last_message_id).await?;
    }
    Ok(())
  }

  /// Given initial [ClientMessage::Update] payload, try to prefetch (without blocking or awaiting) as
  /// many bending messages from the collab stream as possible.
  ///
  /// This is so that we can even the difference between frequent updates coming from the user with
  /// slower responding server by merging a lot of small updates together into a bigger one.
  ///
  /// Returns a compacted update message and (optionally) the first message after it which couldn't
  /// be compacted because it's of different type or doesn't belong to the same collab.
  fn eager_prefetch(
    rx: &mut ControllerReceiver,
    current_oid: ObjectId,
    collab_type: CollabType,
    buf: Vec<u8>,
  ) -> anyhow::Result<(ClientMessage, Option<ClientMessage>)> {
    const SIZE_THRESHOLD: usize = 64 * 1024;
    let mut size_hint = buf.len();
    let mut updates: Vec<Vec<u8>> = vec![buf];
    let mut other = None;
    // try to eagerly fetch more updates if they are already in the queue
    while let Ok((msg, None)) = rx.try_recv() {
      match msg {
        ClientMessage::Update {
          object_id,
          flags: UpdateFlags::Lib0v1,
          update,
          ..
        } if object_id == current_oid => {
          size_hint += update.len();
          // we stack updates together until we reach a non-update message
          updates.push(update);

          if size_hint >= SIZE_THRESHOLD {
            break; // potential size of the update may be over threshold, stop here and send what we have
          }
        },
        _ => {
          // other type of message, we cannot compact updates anymore,
          // so we just prepend the update message and then add new one and send them
          // all together
          other = Some(msg);
          break;
        },
      }
    }
    let compacted = if updates.len() == 1 {
      std::mem::take(&mut updates[0])
    } else {
      tracing::debug!("Compacting {} updates ({} bytes)", updates.len(), size_hint);
      merge_updates_v1(updates)? // try to compact updates together
    };
    let compacted = ClientMessage::Update {
      object_id: current_oid,
      collab_type,
      flags: UpdateFlags::Lib0v1,
      update: compacted,
    };
    Ok((compacted, other))
  }

  async fn persist_and_send_message(
    &self,
    msg: ClientMessage,
    last_message_id: Option<Rid>,
  ) -> anyhow::Result<()> {
    if let ClientMessage::Update {
      object_id,
      flags,
      update,
      collab_type,
    } = &msg
    {
      // persist
      let update = match flags {
        UpdateFlags::Lib0v1 => Update::decode_v1(update),
        UpdateFlags::Lib0v2 => Update::decode_v2(update),
      }?;
      self
        .save_update(*object_id, last_message_id, update, *collab_type)
        .await?;
    }
    if last_message_id.is_none() {
      // we only send updates generated locally
      self.send_message(msg).await?;
    }
    Ok(())
  }

  fn signal_closed(&self) {
    self.shutdown.cancel();
    self.closed.notify_waiters();
  }

  /// Connection status change: requesting connection but not yet connected.
  /// Note: DO NOT call it outside the given context - it doesn't cancel current connection.
  fn request_reconnect(&self) {
    let cancel = self.shutdown.child_token();
    let status_tx = self.status_tx.clone();

    status_tx
      .send(ConnectionStatus::Connecting { cancel })
      .unwrap();
    tracing::debug!("requesting reconnection");
  }

  /// Connection status change: connected to server and ready to operate.
  fn set_connected(&self, sink: SplitSink<WsConn, Message>, cancel: CancellationToken) {
    let sink = Arc::new(Mutex::new(sink));
    let status = ConnectionStatus::Connected { sink, cancel };
    self.status_tx.send(status).unwrap();
  }

  /// Connection status change: disconnected from the server, either on demand (reason `None`) or
  /// due to some failure (reason provided).
  fn set_disconnected(&self, reason: Option<String>) {
    let status = ConnectionStatus::Disconnected {
      reason: reason.map(Into::into),
    };
    self.status_tx.send(status).unwrap();
  }

  fn publish_manifest(&self, collab: &Collab, collab_type: CollabType) {
    let messages = self.message_tx.load();
    if let Some(channel) = &*messages {
      let last_message_id = self.last_message_id();
      let object_id = collab.object_id();
      let doc = collab.get_awareness().doc();
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
      let _ = channel.send((msg, None));
    }
  }

  fn publish_update(
    &self,
    object_id: ObjectId,
    collab_type: CollabType,
    last_message_id: Option<Rid>,
    update_v1: Vec<u8>,
  ) {
    let messages = self.message_tx.load();
    if let Some(channel) = &*messages {
      let msg = ClientMessage::Update {
        object_id,
        collab_type,
        flags: UpdateFlags::Lib0v1,
        update: update_v1,
      };
      // we received that update from the local client
      let _ = channel.send((msg, last_message_id));
    }
  }

  fn get_collab(&self, object_id: ObjectId) -> Option<CollabRef> {
    self.cache.get(&object_id)?.upgrade()
  }

  fn remove_collab(&self, object_id: &ObjectId) -> anyhow::Result<()> {
    self.cache.remove(object_id);
    self.db.remove_doc(object_id)?;
    Ok(())
  }

  fn last_message_id(&self) -> Rid {
    *self.last_message_id.load_full()
  }

  async fn save_update(
    &self,
    object_id: ObjectId,
    last_message_id: Option<Rid>,
    update: Update,
    collab_type: CollabType,
  ) -> anyhow::Result<()> {
    let update_bytes = update.encode_v1();
    if let Some(rid) = last_message_id {
      if let Some(collab_ref) = self.get_collab(object_id) {
        let mut lock = collab_ref.write().await;
        let collab = (*lock).borrow_mut();
        let sync_state = collab.get_state().sync_state();
        let doc = collab.get_awareness().doc();
        let mut tx = doc.transact_mut_with(rid.into_bytes().as_ref());
        tx.apply_update(update)?;
        if Self::has_missing_updates(&tx) {
          drop(tx);
          tracing::trace!("found missing updates for {} - sending manifest", object_id);
          self.publish_manifest(collab, collab_type);
        } else if sync_state == SyncState::InitSyncBegin {
          drop(tx);
          collab.set_sync_state(SyncState::InitSyncEnd);
        }
      }
    }
    tracing::trace!(
      "persisting update for {} ({} bytes)",
      object_id,
      update_bytes.len()
    );
    self
      .db
      .save_update(&object_id, last_message_id, update_bytes)?;
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
    if let Some(collab_ref) = self.get_collab(object_id) {
      let mut lock = collab_ref.write().await;
      let collab = (*lock).borrow_mut();
      collab.borrow_mut().get_awareness().apply_update(update)?;
    }
    Ok(())
  }
}

#[derive(Debug, Clone)]
pub enum ConnectionStatus {
  Disconnected {
    reason: Option<Arc<str>>,
  },
  Connecting {
    cancel: CancellationToken,
  },
  Connected {
    sink: Arc<Mutex<SplitSink<WsConn, Message>>>,
    cancel: CancellationToken,
  },
}

impl Default for ConnectionStatus {
  fn default() -> Self {
    ConnectionStatus::Disconnected { reason: None }
  }
}

impl Display for ConnectionStatus {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      ConnectionStatus::Disconnected { reason: None } => write!(f, "disconnected"),
      ConnectionStatus::Disconnected {
        reason: Some(reason),
      } => write!(f, "disconnected: {}", reason),
      ConnectionStatus::Connecting { .. } => write!(f, "connecting"),
      ConnectionStatus::Connected { .. } => write!(f, "connected"),
    }
  }
}

#[derive(Debug, Clone)]
pub struct Options {
  pub url: String,
  pub workspace_id: WorkspaceId,
  pub uid: i64,
  pub workspace_db_path: String,
  pub device_id: String,
  pub access_token: String,
}

#[cfg(debug_assertions)]
impl WorkspaceController {
  pub fn enable_receive_message(&self) {
    self
      .inner
      .skip_realtime_message
      .store(false, std::sync::atomic::Ordering::SeqCst);
  }

  pub fn disable_receive_message(&self) {
    self
      .inner
      .skip_realtime_message
      .store(true, std::sync::atomic::Ordering::SeqCst);
  }
}
