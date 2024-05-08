use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Display;

use futures_util::stream::{SplitSink, SplitStream};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use semver::Version;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::broadcast::{channel, Receiver, Sender};

use crate::ws::msg_queue::{AggregateMessageQueue, AggregateMessagesReceiver};
use crate::ws::{ConnectState, ConnectStateNotify, WSError, WebSocketChannel};
use crate::ServerFixIntervalPing;
use crate::{af_spawn, retry_connect};
use client_websocket::{CloseCode, CloseFrame, Message, WebSocketStream};
use collab_rt_entity::user::UserMessage;
use collab_rt_entity::ClientCollabMessage;
use collab_rt_entity::ServerCollabMessage;
use collab_rt_entity::{RealtimeMessage, SystemMessage};
use tokio::sync::{oneshot, Mutex};
use tracing::{error, info, trace, warn};

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
      ping_per_secs: 5,
      retry_connect_per_pings: 6,
    }
  }
}

#[async_trait::async_trait]
pub trait WSClientHttpSender: Send + Sync {
  async fn send_ws_msg(&self, device_id: &str, message: Message) -> Result<(), WSError>;
}

#[async_trait::async_trait]
pub trait WSClientConnectURLProvider: Send + Sync {
  fn connect_ws_url(&self) -> String;
  async fn connect_info(&self) -> Result<ConnectInfo, WSError>;
}

type WeakChannel = Weak<WebSocketChannel<ServerCollabMessage>>;
type ChannelByObjectId = HashMap<String, Vec<WeakChannel>>;
pub type WSConnectStateReceiver = Receiver<ConnectState>;

pub(crate) type StateNotify = parking_lot::Mutex<ConnectStateNotify>;

/// The maximum size allowed for a WebSocket message is 65,536 bytes. If the message exceeds
/// 50960 bytes (to avoid occupying the entire space), it should be sent over HTTP instead.
const MAXIMUM_MESSAGE_SIZE: usize = 40960;
const MAXIMUM_BATCH_MESSAGE_SIZE: usize = 20480;

pub struct WSClient {
  config: WSClientConfig,
  state_notify: Arc<StateNotify>,
  /// Sender used to send messages to the websocket.
  ws_msg_sender: Sender<Message>,
  rt_msg_sender: Sender<Vec<ClientCollabMessage>>,
  http_sender: Arc<dyn WSClientHttpSender>,
  user_channel: Arc<Sender<UserMessage>>,
  channels: Arc<RwLock<ChannelByObjectId>>,
  ping: Arc<Mutex<Option<ServerFixIntervalPing>>>,
  stop_ws_msg_loop_tx: Mutex<Option<oneshot::Sender<()>>>,
  aggregate_queue: Arc<AggregateMessageQueue>,

  #[cfg(debug_assertions)]
  skip_realtime_message: Arc<std::sync::atomic::AtomicBool>,
  connect_provider: Arc<dyn WSClientConnectURLProvider>,
}
impl WSClient {
  pub fn new<H, C>(config: WSClientConfig, http_sender: H, connect_provider: C) -> Self
  where
    H: WSClientHttpSender + 'static,
    C: WSClientConnectURLProvider + 'static,
  {
    let (ws_msg_sender, _) = channel(config.buffer_capacity);
    let state_notify = Arc::new(parking_lot::Mutex::new(ConnectStateNotify::new()));
    let channels = Arc::new(RwLock::new(HashMap::new()));
    let ping = Arc::new(Mutex::new(None));
    let http_sender = Arc::new(http_sender);
    let (user_channel, _) = channel(1);
    let (rt_msg_sender, _) = channel(config.buffer_capacity);
    let connect_provider = Arc::new(connect_provider);
    let aggregate_queue = Arc::new(AggregateMessageQueue::new(MAXIMUM_BATCH_MESSAGE_SIZE));
    WSClient {
      config,
      state_notify,
      ws_msg_sender,
      rt_msg_sender,
      http_sender,
      user_channel: Arc::new(user_channel),
      channels,
      ping,
      stop_ws_msg_loop_tx: Mutex::new(None),
      aggregate_queue,

      #[cfg(debug_assertions)]
      skip_realtime_message: Default::default(),
      connect_provider,
    }
  }

  pub async fn connect(&self) -> Result<(), WSError> {
    let connect_info = self.connect_provider.connect_info().await?;
    let device_id = connect_info.device_id.clone();

    if self.get_state().is_connecting() {
      info!("websocket is connecting, skip connect request");
      return Ok(());
    }
    // 1. clean any previous connection
    self.clean().await;

    self.set_state(ConnectState::Connecting).await;
    let (stop_ws_msg_loop_tx, stop_ws_msg_loop_rx) = oneshot::channel();
    *self.stop_ws_msg_loop_tx.lock().await = Some(stop_ws_msg_loop_tx);

    // 2. start connecting
    let conn_result = retry_connect(
      self.connect_provider.clone(),
      Arc::downgrade(&self.state_notify),
    )
    .await;

    // 3. handle websocket error when connecting or sending message
    if let Err(err) = &conn_result {
      match err {
        WSError::AuthError(_) => self
          .state_notify
          .lock()
          .set_state(ConnectState::Unauthorized),
        _ => self.state_notify.lock().set_state(ConnectState::Lost),
      }
    }

    // 4. after the connection is established, the client will start sending ping messages to the server
    // at regular intervals to detect the connection status.
    let (sink, stream) = conn_result?.split();
    self.set_state(ConnectState::Connected).await;

    // 5. start pinging
    let pong_tx = self.start_ping().await;

    // 6. spawn a task that continuously receives messages from the server
    self.spawn_recv_server_message(
      stream,
      Arc::downgrade(&self.channels),
      self.ws_msg_sender.clone(),
      pong_tx,
    );

    let (tx, rx) = tokio::sync::mpsc::channel(100);
    self.aggregate_queue.set_sender(tx).await;

    // 7. spawn a task that continuously sending client message.
    self.spawn_send_client_message(sink, &device_id, stop_ws_msg_loop_rx, rx);

    // 8. start aggregating messages
    // combine multiple messages into one message to reduce the number of messages sent over the network
    self.spawn_aggregate_message();
    Ok(())
  }

  fn spawn_aggregate_message(&self) {
    let mut rx = self.rt_msg_sender.subscribe();
    let weak_aggregate_queue = Arc::downgrade(&self.aggregate_queue);
    af_spawn(async move {
      while let Ok(msg) = rx.recv().await {
        if let Some(aggregate_queue) = weak_aggregate_queue.upgrade() {
          aggregate_queue.push(msg).await;
        }
      }
    });
  }

  async fn start_ping(&self) -> tokio::sync::mpsc::Sender<()> {
    let ping_sender = self.ws_msg_sender.clone();
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
    pong_tx
  }

  // When
  fn spawn_send_client_message(
    &self,
    mut sink: SplitSink<WebSocketStream, Message>,
    device_id: &str,
    mut stop_ws_msg_loop_rx: oneshot::Receiver<()>,
    mut aggregate_msg_rx: AggregateMessagesReceiver,
  ) {
    let mut ws_msg_rx = self.ws_msg_sender.subscribe();
    let weak_state_notify = Arc::downgrade(&self.state_notify);
    let weak_http_sender = Arc::downgrade(&self.http_sender);
    let device_id = device_id.to_string();

    let handle_error = move |err| {
      error!("{}", err);
      match weak_state_notify.upgrade() {
        None => error!("websocket state_notify is dropped"),
        Some(state_notify) => match &err {
          WSError::LostConnection(_) => state_notify.lock().set_state(ConnectState::Lost),
          WSError::AuthError(_) => state_notify.lock().set_state(ConnectState::Unauthorized),
          _ => {},
        },
      }
    };

    af_spawn(async move {
      loop {
        tokio::select! {
           _ = &mut stop_ws_msg_loop_rx => break,
           Ok(msg) = ws_msg_rx.recv() => {
              if let Err(err) = send_message(&mut sink, &device_id, msg, &weak_http_sender).await {
                if err.should_stop() {
                  break;
                }
                handle_error(err);
              }
           }
           Some(msg) = aggregate_msg_rx.recv() => {
              if let Err(err) = send_message(&mut sink, &device_id, msg, &weak_http_sender).await {
                if err.should_stop() {
                  break;
                }
                handle_error(err);
              }
          }
        }
      }
    });
  }

  fn spawn_recv_server_message(
    &self,
    mut stream: SplitStream<WebSocketStream>,
    weak_collab_channels: Weak<RwLock<ChannelByObjectId>>,
    sender: Sender<Message>,
    pong_tx: tokio::sync::mpsc::Sender<()>,
  ) {
    #[cfg(debug_assertions)]
    let cloned_skip_realtime_message = self.skip_realtime_message.clone();
    let user_message_tx = self.user_channel.as_ref().clone();
    af_spawn(async move {
      while let Some(Ok(ws_msg)) = stream.next().await {
        match ws_msg {
          Message::Binary(data) => {
            #[cfg(debug_assertions)]
            {
              if cloned_skip_realtime_message.load(std::sync::atomic::Ordering::SeqCst) {
                continue;
              }
            }
            match RealtimeMessage::decode(&data) {
              Ok(msg) => match msg {
                RealtimeMessage::Collab(collab_msg) => {
                  match ServerCollabMessage::try_from(collab_msg) {
                    Ok(collab_message) => {
                      handle_collab_message(&weak_collab_channels, vec![collab_message]);
                    },
                    Err(err) => {
                      error!("parser ServerCollabMessage failed: {:?}", err);
                    },
                  }
                },
                RealtimeMessage::User(user_message) => {
                  let _ = user_message_tx.send(user_message);
                },
                RealtimeMessage::System(sys_message) => match sys_message {
                  SystemMessage::RateLimit(_limit) => {},
                  SystemMessage::KickOff => {
                    break;
                  },
                  SystemMessage::DuplicateConnection => {
                    trace!("detect same ws connect from this device, closing the connection");
                    break;
                  },
                },
                RealtimeMessage::ServerCollabV1(collab_messages) => {
                  handle_collab_message(&weak_collab_channels, collab_messages);
                },
                RealtimeMessage::ClientCollabV1(_) | RealtimeMessage::ClientCollabV2(_) => {
                  // The message from server should not be collab message.
                  error!(
                    "received unexpected collab message from websocket: {:?}",
                    msg
                  );
                },
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
            break;
          },
          Message::Pong(_) => {
            if let Err(err) = pong_tx.send(()).await {
              error!("failed to receive server pong: {}", err);
              break;
            }
          },
          _ => warn!("received unexpected message from websocket: {:?}", ws_msg),
        }
      }
    });
  }

  /// Return a [WebSocketChannel] that can be used to send messages to the websocket. Caller should
  /// keep the channel alive as long as it wants to receive messages from the websocket.
  pub fn subscribe_collab(
    &self,
    object_id: String,
  ) -> Result<Arc<WebSocketChannel<ServerCollabMessage>>, WSError> {
    let channel = Arc::new(WebSocketChannel::new(
      &object_id,
      self.rt_msg_sender.clone(),
    ));
    let mut collab_channels_guard = self.channels.write();

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
    self.clean().await;

    let _ = self.ws_msg_sender.send(Message::Close(Some(CloseFrame {
      code: CloseCode::Normal,
      reason: Cow::from("client disconnect"),
    })));

    self.set_state(ConnectState::Lost).await;
  }

  async fn clean(&self) {
    if let Some(old_stop_ws_tx) = self.stop_ws_msg_loop_tx.lock().await.take() {
      let _ = old_stop_ws_tx.send(());
    }

    if let Some(old_ping) = self.ping.lock().await.as_ref() {
      old_ping.stop().await;
    }

    self.aggregate_queue.clear().await;
  }

  pub fn send<M: Into<Message>>(&self, msg: M) -> Result<(), WSError> {
    self.ws_msg_sender.send(msg.into()).unwrap();
    Ok(())
  }

  pub fn get_state(&self) -> ConnectState {
    self.state_notify.lock().state.clone()
  }

  async fn set_state(&self, state: ConnectState) {
    self.state_notify.lock().set_state(state);
  }
}

#[inline]
fn handle_collab_message(
  weak_collab_channels: &Weak<RwLock<ChannelByObjectId>>,
  collab_messages: Vec<ServerCollabMessage>,
) {
  if let Some(collab_channels) = weak_collab_channels.upgrade() {
    for collab_msg in collab_messages {
      let object_id = collab_msg.object_id().to_owned();
      // Iterate all channels and send the message to them.
      if let Some(channels) = collab_channels.read().get(&object_id) {
        for channel in channels.iter() {
          if let Some(channel) = channel.upgrade() {
            if cfg!(feature = "sync_verbose_log") {
              trace!("receive server: {}", collab_msg);
            }
            channel.forward_to_stream(collab_msg.clone());
          }
        }
      }
    }
  } else {
    warn!("channels are closed");
  }
}
#[cfg(debug_assertions)]
impl WSClient {
  pub fn disable_receive_message(&self) {
    self
      .skip_realtime_message
      .store(true, std::sync::atomic::Ordering::SeqCst);
  }

  pub fn enable_receive_message(&self) {
    self
      .skip_realtime_message
      .store(false, std::sync::atomic::Ordering::SeqCst);
  }
}

async fn send_message(
  sink: &mut SplitSink<WebSocketStream, Message>,
  device_id: &str,
  message: Message,
  http_sender: &Weak<dyn WSClientHttpSender>,
) -> Result<(), WSError> {
  if message.is_binary() && message.len() > MAXIMUM_MESSAGE_SIZE {
    if let Some(http_sender) = http_sender.upgrade() {
      let cloned_device_id = device_id.to_string();
      af_spawn(async move {
        if let Err(err) = http_sender.send_ws_msg(&cloned_device_id, message).await {
          error!("Failed to send WebSocket message over HTTP: {}", err);
        }
      });
    } else {
      error!("The HTTP sender has been dropped, unable to send message.");
    }
  } else {
    sink.send(message).await.map_err(WSError::from)?;
  }

  Ok(())
}

#[derive(Clone, Eq, PartialEq)]
pub struct ConnectInfo {
  pub access_token: String,
  pub client_version: Version,
  pub device_id: String,
}

impl Display for ConnectInfo {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "access_token: xx, client_version: {}, device_id: {}",
      self.client_version, self.device_id
    ))
  }
}

impl From<ConnectInfo> for HeaderMap {
  fn from(info: ConnectInfo) -> Self {
    let mut headers = HeaderMap::new();
    headers.insert(
      "device-id",
      HeaderValue::from_str(&info.device_id).unwrap_or(HeaderValue::from_static("")),
    );
    headers.insert(
      "client-version",
      HeaderValue::from_str(&info.client_version.to_string())
        .unwrap_or(HeaderValue::from_static("unknown_client")),
    );
    headers.insert(
      AUTHORIZATION,
      HeaderValue::from_str(&info.access_token).unwrap_or(HeaderValue::from_static("")),
    );
    headers.insert(
      "connect-at",
      HeaderValue::from(chrono::Utc::now().timestamp()),
    );
    headers
  }
}
