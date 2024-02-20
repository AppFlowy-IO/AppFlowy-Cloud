use futures_util::{SinkExt, StreamExt};
use governor::clock::DefaultClock;
use governor::middleware::NoOpMiddleware;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use parking_lot::RwLock;
use std::borrow::Cow;
use std::collections::HashMap;

use futures_util::FutureExt;
use std::num::NonZeroU32;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::broadcast::{channel, Receiver, Sender};

use crate::ws::{ConnectState, ConnectStateNotify, WSError, WebSocketChannel};
use crate::ServerFixIntervalPing;
use crate::{platform_spawn, retry_connect};
use realtime_entity::collab_msg::CollabMessage;
use realtime_entity::message::{RealtimeMessage, SystemMessage};
use realtime_entity::user::UserMessage;

use tokio::sync::{oneshot, Mutex};
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

pub(crate) type StateNotify = parking_lot::Mutex<ConnectStateNotify>;
pub(crate) type CurrentAddr = parking_lot::Mutex<Option<String>>;
pub struct WSClient {
  addr: Arc<CurrentAddr>,
  config: WSClientConfig,
  state_notify: Arc<StateNotify>,
  /// Sender used to send messages to the websocket.
  sender: Sender<Message>,
  http_sender: Arc<dyn WSClientHttpSender>,
  user_channel: Arc<Sender<UserMessage>>,
  collab_channels: Arc<RwLock<ChannelByObjectId>>,
  ping: Arc<Mutex<Option<ServerFixIntervalPing>>>,
  stop_tx: Mutex<Option<oneshot::Sender<()>>>,
  rate_limiter:
    Arc<tokio::sync::RwLock<RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>>>,
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
    let rate_limiter = gen_rate_limiter(10);
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
      rate_limiter: Arc::new(tokio::sync::RwLock::new(rate_limiter)),
    }
  }

  pub async fn connect(&self, addr: String, device_id: &str) -> Result<(), WSError> {
    self.set_state(ConnectState::Connecting).await;

    // stop receiving message from client
    let (stop_tx, mut stop_rx) = oneshot::channel();
    if let Some(old_stop_tx) = self.stop_tx.lock().await.take() {
      let _ = old_stop_tx.send(());
    }
    *self.stop_tx.lock().await = Some(stop_tx);

    // stop pinging
    *self.addr.lock() = Some(addr.clone());
    if let Some(old_ping) = self.ping.lock().await.as_ref() {
      old_ping.stop().await;
    }

    // start connecting
    let conn_result = retry_connect(
      &addr,
      Arc::downgrade(&self.state_notify),
      Arc::downgrade(&self.addr),
    )
    .await;

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
    let rate_limiter = self.rate_limiter.clone();
    // Receive messages from the websocket, and send them to the channels.
    platform_spawn(async move {
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
                  RealtimeMessage::System(sys_message) => match sys_message {
                    SystemMessage::RateLimit(limit) => {
                      *rate_limiter.write().await = gen_rate_limiter(limit);
                    },
                    SystemMessage::KickOff => {
                      //
                    },
                  },
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
            Err(_e) => {
              // if the sender returns an error, it means the receiver has been dropped
              break;
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
    let rate_limiter = self.rate_limiter.clone();
    let device_id = device_id.to_string();
    platform_spawn(async move {
      loop {
        tokio::select! {
          _ = &mut stop_rx => break,
         Ok(msg) = rx.recv() => {
            rate_limiter.read().await.until_ready().fuse().await;

            let len = msg.len();
            // The maximum size allowed for a WebSocket message is 65,536 bytes. If the message exceeds
            // 40,960 bytes (to avoid occupying the entire space), it should be sent over HTTP instead.
            if  msg.is_binary() && len > 40960 {
              trace!("send ws message via http, message len: :{}", len);
              if let Some(http_sender) = weak_http_sender.upgrade() {
                let cloned_device_id = device_id.clone();
                // Spawn a task here in case of blocking the current loop task.
                platform_spawn(async move {
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
      info!("exit websocket send loop");
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

fn gen_rate_limiter(
  mut times_per_sec: u32,
) -> RateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware> {
  // make sure the rate limiter is not zero
  if times_per_sec == 0 {
    times_per_sec = 1;
  }
  let quota = Quota::per_second(NonZeroU32::new(times_per_sec).unwrap());
  RateLimiter::direct(quota)
}
