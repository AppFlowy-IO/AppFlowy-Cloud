use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::broadcast::{channel, Receiver, Sender};

use crate::spawn;
use crate::ws::ping::ServerFixIntervalPing;
use crate::ws::retry::ConnectAction;
use crate::ws::{ConnectState, ConnectStateNotify, WSError, WebSocketChannel};
use realtime_entity::collab_msg::CollabMessage;
use realtime_entity::message::RealtimeMessage;
use realtime_entity::user::UserMessage;
use tokio::sync::{oneshot, Mutex};
use tokio_retry::strategy::FixedInterval;
use tokio_retry::{Condition, RetryIf};
use tracing::{debug, error, info, trace, warn};
use websocket::{CloseCode, CloseFrame, Message};

pub struct WSClientConfig {
  /// specifies the number of messages that the channel can hold at any given
  /// time. It is used to set the initial size of the channel's internal buffer
  pub buffer_capacity: usize,
  /// specifies the number of seconds between each ping message
  pub ping_per_secs: u64,
  /// specifies the number of pings that the client will start reconnecting
  pub retry_connect_per_pings: u32,
}

impl Default for WSClientConfig {
  fn default() -> Self {
    Self {
      buffer_capacity: 2000,
      ping_per_secs: 6,
      retry_connect_per_pings: 10,
    }
  }
}

#[async_trait::async_trait]
pub trait WSClientHttpSender: Send + Sync {
  async fn send_ws_msg(&self, device_id: &str, message: Message) -> Result<(), WSError>;
}

type WeakChannel = Weak<WebSocketChannel<CollabMessage>>;
type ChannelByObjectId = HashMap<String, Vec<WeakChannel>>;
pub type WSConnectStateReceiver = Receiver<ConnectState>;

pub struct WSClient {
  addr: Arc<parking_lot::Mutex<Option<String>>>,
  config: WSClientConfig,
  state_notify: Arc<parking_lot::Mutex<ConnectStateNotify>>,
  /// Sender used to send messages to the websocket.
  sender: Sender<Message>,
  http_sender: Arc<dyn WSClientHttpSender>,
  user_channel: Arc<Sender<UserMessage>>,
  collab_channels: Arc<RwLock<ChannelByObjectId>>,
  ping: Arc<Mutex<Option<ServerFixIntervalPing>>>,
  stop_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl WSClient {
  pub fn new<H>(config: WSClientConfig, http_sender: H) -> Self
  where
    H: WSClientHttpSender + 'static,
  {
    let (sender, _) = channel(config.buffer_capacity);
    let state_notify = Arc::new(parking_lot::Mutex::new(ConnectStateNotify::new()));
    let collab_channels = Arc::new(RwLock::new(HashMap::new()));
    let ping = Arc::new(Mutex::new(None));
    let http_sender = Arc::new(http_sender);
    let (user_channel, _) = channel(1);
    WSClient {
      addr: Arc::new(parking_lot::Mutex::new(None)),
      config,
      state_notify,
      sender,
      http_sender,
      user_channel: Arc::new(user_channel),
      collab_channels,
      ping,
      stop_tx: Mutex::new(None),
    }
  }

  pub async fn connect(&self, addr: String, device_id: &str) -> Result<(), WSError> {
    self.set_state(ConnectState::Connecting).await;

    let (stop_tx, mut stop_rx) = oneshot::channel();
    if let Some(old_stop_tx) = self.stop_tx.lock().await.take() {
      let _ = old_stop_tx.send(());
    }
    *self.stop_tx.lock().await = Some(stop_tx);

    *self.addr.lock() = Some(addr.clone());
    if let Some(old_ping) = self.ping.lock().await.as_ref() {
      old_ping.stop().await;
    }

    let retry_strategy = FixedInterval::new(Duration::from_secs(6));
    let action = ConnectAction::new(addr.clone());
    let cond = RetryCondition {
      connecting_addr: addr,
      addr: Arc::downgrade(&self.addr),
      state_notify: Arc::downgrade(&self.state_notify),
    };

    // handle websocket error when connecting or sending message
    let weak_state_notify = Arc::downgrade(&self.state_notify);
    let handle_ws_error = move |error: &WSError| {
      error!("websocket error: {:?}", error);
      match weak_state_notify.upgrade() {
        None => error!("websocket state_notify is dropped"),
        Some(state_notify) => match &error {
          WSError::TungsteniteError(_) => {},
          WSError::LostConnection(_) => state_notify.lock().set_state(ConnectState::Closed),
          WSError::AuthError(_) => state_notify.lock().set_state(ConnectState::Unauthorized),
          WSError::Internal(_) => {},
        },
      }
    };

    let conn_result = RetryIf::spawn(retry_strategy, action, cond).await;
    if let Err(err) = &conn_result {
      handle_ws_error(err);
    }

    let ws_stream = conn_result?;
    self.set_state(ConnectState::Connected).await;
    let (mut sink, mut stream) = ws_stream.split();
    let weak_collab_channels = Arc::downgrade(&self.collab_channels);
    let sender = self.sender.clone();

    let ping_sender = sender.clone();
    let (pong_tx, pong_recv) = tokio::sync::mpsc::channel(1);
    let mut ping = ServerFixIntervalPing::new(
      Duration::from_secs(self.config.ping_per_secs),
      self.state_notify.clone(),
      ping_sender,
      pong_recv,
      self.config.retry_connect_per_pings,
    );
    ping.run();
    *self.ping.lock().await = Some(ping);

    let user_message_tx = self.user_channel.as_ref().clone();
    // Receive messages from the websocket, and send them to the channels.
    spawn(async move {
      while let Some(Ok(ws_msg)) = stream.next().await {
        match ws_msg {
          Message::Binary(_) => {
            match RealtimeMessage::try_from(&ws_msg) {
              Ok(msg) => {
                match msg {
                  RealtimeMessage::Collab(collab_msg) => {
                    if let Some(collab_channels) = weak_collab_channels.upgrade() {
                      let object_id = collab_msg.object_id().to_owned();

                      // Iterate all channels and send the message to them.
                      if let Some(channels) = collab_channels.read().get(&object_id) {
                        for channel in channels.iter() {
                          match channel.upgrade() {
                            None => {
                              // when calling [WSClient::subscribe], the caller is responsible for keeping
                              // the channel alive as long as it wants to receive messages from the websocket.
                              warn!("channel is dropped");
                            },
                            Some(channel) => {
                              trace!("receive remote message: {}", collab_msg);
                              channel.forward_to_stream(collab_msg.clone());
                            },
                          }
                        }
                      }
                    } else {
                      warn!("channels are closed");
                    }
                  },
                  RealtimeMessage::User(user_message) => {
                    let _ = user_message_tx.send(user_message);
                  },
                  RealtimeMessage::ServerKickedOff => {},
                }
              },
              Err(err) => {
                error!("parser RealtimeMessage failed: {:?}", err);
              },
            }
          },
          // ping from server
          Message::Ping(_) => match sender.send(Message::Pong(vec![])) {
            Ok(_) => {},
            Err(e) => {
              error!("failed to send pong message to websocket: {}", e);
            },
          },
          Message::Close(close) => {
            info!("websocket close: {:?}", close);
          },
          Message::Pong(_) => {
            if let Err(err) = pong_tx.send(()).await {
              error!("failed to receive server pong: {}", err);
            }
          },
          _ => warn!("received unexpected message from websocket: {:?}", ws_msg),
        }
      }
    });

    let mut rx = self.sender.subscribe();
    let weak_http_sender = Arc::downgrade(&self.http_sender);
    let device_id = device_id.to_string();
    spawn(async move {
      loop {
        tokio::select! {
          _ = &mut stop_rx => break,
         Ok(msg) = rx.recv() => {
            let len = msg.len();
            // The maximum size allowed for a WebSocket message is 65,536 bytes. If the message exceeds
            // 40,960 bytes (to avoid occupying the entire space), it should be sent over HTTP instead.
            if  msg.is_binary() && len > 40960 {
              trace!("send ws message via http, message len: :{}", len);
              if let Some(http_sender) = weak_http_sender.upgrade() {
                let cloned_device_id = device_id.clone();
                // Spawn a task here in case of blocking the current loop task.
                tokio::spawn(async move {
                  if let Err(err) = http_sender.send_ws_msg(&cloned_device_id, msg).await {
                    error!("Failed to send WebSocket message over HTTP: {}", err);
                  }
                });
              } else {
                 error!("The HTTP sender has been dropped, unable to send message.");
                 continue;
              }
            } else if let Err(err) = sink.send(msg).await.map_err(WSError::from){
              handle_ws_error(&err);
              break;
            }
          }
        }
      }
    });

    Ok(())
  }

  /// Return a [WebSocketChannel] that can be used to send messages to the websocket. Caller should
  /// keep the channel alive as long as it wants to receive messages from the websocket.
  pub fn subscribe_collab(
    &self,
    object_id: String,
  ) -> Result<Arc<WebSocketChannel<CollabMessage>>, WSError> {
    let channel = Arc::new(WebSocketChannel::new(&object_id, self.sender.clone()));
    let mut collab_channels_guard = self.collab_channels.write();

    // remove the dropped channels
    if let Some(channels) = collab_channels_guard.get_mut(&object_id) {
      channels.retain(|channel| channel.upgrade().is_some());
    }

    collab_channels_guard
      .entry(object_id)
      .or_default()
      .push(Arc::downgrade(&channel));

    Ok(channel)
  }

  pub fn subscribe_user_changed(&self) -> Receiver<UserMessage> {
    self.user_channel.subscribe()
  }

  pub fn subscribe_connect_state(&self) -> WSConnectStateReceiver {
    self.state_notify.lock().subscribe()
  }

  pub fn is_connected(&self) -> bool {
    self.state_notify.lock().state.is_connected()
  }

  pub async fn disconnect(&self) {
    if let Some(stop_tx) = self.stop_tx.lock().await.take() {
      debug!("client disconnect");

      let _ = stop_tx.send(());
      let _ = self.sender.send(Message::Close(Some(CloseFrame {
        code: CloseCode::Normal,
        reason: Cow::from("client disconnect"),
      })));

      *self.addr.lock() = None;
      self.set_state(ConnectState::Closed).await;
    }
  }

  pub fn send<M: Into<Message>>(&self, msg: M) -> Result<(), WSError> {
    self.sender.send(msg.into()).unwrap();
    Ok(())
  }

  pub fn get_state(&self) -> ConnectState {
    self.state_notify.lock().state.clone()
  }

  async fn set_state(&self, state: ConnectState) {
    self.state_notify.lock().set_state(state);
  }
}

struct RetryCondition {
  connecting_addr: String,
  addr: Weak<parking_lot::Mutex<Option<String>>>,
  state_notify: Weak<parking_lot::Mutex<ConnectStateNotify>>,
}
impl Condition<WSError> for RetryCondition {
  fn should_retry(&mut self, error: &WSError) -> bool {
    if let WSError::AuthError(err) = error {
      debug!("{}, stop retry connect", err);

      if let Some(state_notify) = self.state_notify.upgrade() {
        state_notify.lock().set_state(ConnectState::Unauthorized);
      }

      return false;
    }

    let should_retry = self
      .addr
      .upgrade()
      .map(|addr| match addr.try_lock() {
        None => false,
        Some(addr) => match &*addr {
          None => false,
          Some(addr) => addr == &self.connecting_addr,
        },
      })
      .unwrap_or(false);

    debug!("WSClient should_retry: {}", should_retry);
    should_retry
  }
}
