use futures_util::{SinkExt, StreamExt};
use std::borrow::Cow;

use parking_lot::RwLock;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::ws::ping::ServerFixIntervalPing;
use crate::ws::retry::ConnectAction;
use crate::ws::state::{ConnectState, ConnectStateNotify};
use crate::ws::{BusinessID, ClientRealtimeMessage, WSError, WebSocketChannel};
use tokio::sync::broadcast::{channel, Receiver, Sender};

use tokio::sync::{oneshot, Mutex};
use tokio_retry::strategy::FixedInterval;
use tokio_retry::{Condition, RetryIf};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_tungstenite::MaybeTlsStream;
use tracing::{debug, error, info, trace, warn};

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
      buffer_capacity: 1000,
      ping_per_secs: 8,
      retry_connect_per_pings: 20,
    }
  }
}

type ChannelByObjectId = HashMap<String, Weak<WebSocketChannel>>;
pub type WSConnectStateReceiver = Receiver<ConnectState>;

pub struct WSClient {
  addr: Arc<parking_lot::Mutex<Option<String>>>,
  config: WSClientConfig,
  state_notify: Arc<parking_lot::Mutex<ConnectStateNotify>>,
  /// Sender used to send messages to the websocket.
  sender: Sender<Message>,
  channels: Arc<RwLock<HashMap<BusinessID, ChannelByObjectId>>>,
  ping: Arc<Mutex<Option<ServerFixIntervalPing>>>,
  stop_tx: Mutex<Option<oneshot::Sender<()>>>,
}

impl WSClient {
  pub fn new(config: WSClientConfig) -> Self {
    let (sender, _) = channel(config.buffer_capacity);
    let state_notify = Arc::new(parking_lot::Mutex::new(ConnectStateNotify::new()));
    let channels = Arc::new(RwLock::new(HashMap::new()));
    let ping = Arc::new(Mutex::new(None));
    WSClient {
      addr: Arc::new(parking_lot::Mutex::new(None)),
      config,
      state_notify,
      sender,
      channels,
      ping,
      stop_tx: Mutex::new(None),
    }
  }

  pub async fn connect(&self, addr: String) -> Result<Option<SocketAddr>, WSError> {
    let (stop_tx, mut stop_rx) = oneshot::channel();
    *self.stop_tx.lock().await = Some(stop_tx);

    self.set_state(ConnectState::Connecting).await;
    *self.addr.lock() = Some(addr.clone());
    if let Some(old_ping) = self.ping.lock().await.as_ref() {
      old_ping.stop().await;
    }

    // let retry_strategy = FibonacciBackoff::from_millis(2000).max_delay(Duration::from_secs(10 * 60));
    let retry_strategy = FixedInterval::new(Duration::from_secs(6));
    let action = ConnectAction::new(addr.clone());
    let cond = RetryCondition {
      connecting_addr: addr,
      addr: Arc::downgrade(&self.addr),
    };
    let stream = RetryIf::spawn(retry_strategy, action, cond).await?;
    let addr = match stream.get_ref() {
      MaybeTlsStream::Plain(s) => s.local_addr().ok(),
      _ => None,
    };

    self.set_state(ConnectState::Connected).await;
    let (mut sink, mut stream) = stream.split();
    let weak_channels = Arc::downgrade(&self.channels);
    let sender = self.sender.clone();

    let mut ping = ServerFixIntervalPing::new(
      Duration::from_secs(self.config.ping_per_secs),
      self.state_notify.clone(),
      sender.clone(),
      self.config.retry_connect_per_pings,
    );
    ping.run();
    *self.ping.lock().await = Some(ping);

    // Receive messages from the websocket, and send them to the channels.
    tokio::spawn(async move {
      while let Some(Ok(msg)) = stream.next().await {
        match msg {
          Message::Text(_) => {},
          Message::Binary(_) => {
            if let Ok(msg) = ClientRealtimeMessage::try_from(&msg) {
              if let Some(channels) = weak_channels.upgrade() {
                if let Some(channel) = channels
                  .read()
                  .get(&msg.business_id)
                  .and_then(|map| map.get(&msg.object_id))
                {
                  match channel.upgrade() {
                    None => {
                      // when calling [WSClient::subscribe], the caller is responsible for keeping
                      // the channel alive as long as it wants to receive messages from the websocket.
                      trace!("channel is dropped");
                    },
                    Some(channel) => {
                      channel.recv_msg(&msg);
                    },
                  }
                }
              } else {
                warn!("channels are closed");
              }
            } else {
              error!("🔴Parser ClientRealtimeMessage failed");
            }
          },
          Message::Ping(_) => match sender.send(Message::Pong(vec![])) {
            Ok(_) => {},
            Err(e) => {
              error!("🔴Failed to send pong message to websocket: {:?}", e);
            },
          },
          Message::Pong(_) => {},
          Message::Close(close) => {
            info!("{:?}", close);
          },
          Message::Frame(_) => {},
        }
      }
    });

    let mut sink_rx = self.sender.subscribe();

    let weak_state_notify = Arc::downgrade(&self.state_notify);
    let handle_ws_error = move |error: &Error| {
      error!("websocket error: {:?}", error);
      match weak_state_notify.upgrade() {
        None => {
          error!("ws state_notify is dropped");
        },
        Some(state_notify) => match &error {
          Error::ConnectionClosed | Error::AlreadyClosed => {
            state_notify.lock().set_state(ConnectState::Disconnected);
          },
          _ => {},
        },
      }
    };

    tokio::spawn(async move {
      loop {
        tokio::select! {
          _ = &mut stop_rx => {
            info!("Client stop sending message using websocket");
            break;
          },
         Ok(msg) = sink_rx.recv() => {
           if let Err(err) = sink.send(msg).await {
              handle_ws_error(&err);
              break;
            }
          }
        }
      }
    });

    Ok(addr)
  }

  /// Return a [WebSocketChannel] that can be used to send messages to the websocket. Caller should
  /// keep the channel alive as long as it wants to receive messages from the websocket.
  pub fn subscribe(
    &self,
    business_id: BusinessID,
    object_id: String,
  ) -> Result<Arc<WebSocketChannel>, WSError> {
    let channel = Arc::new(WebSocketChannel::new(business_id, self.sender.clone()));
    self
      .channels
      .write()
      .entry(business_id)
      .or_default()
      .insert(object_id, Arc::downgrade(&channel));
    Ok(channel)
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
      self.set_state(ConnectState::Disconnected).await;
    }
  }

  pub fn send<M: Into<Message>>(&self, msg: M) -> Result<(), WSError> {
    self.sender.send(msg.into()).unwrap();
    Ok(())
  }

  pub fn sender(&self) -> Sender<Message> {
    self.sender.clone()
  }

  async fn set_state(&self, state: ConnectState) {
    self.state_notify.lock().set_state(state);
  }
}

struct RetryCondition {
  connecting_addr: String,
  addr: Weak<parking_lot::Mutex<Option<String>>>,
}
impl Condition<WSError> for RetryCondition {
  fn should_retry(&mut self, error: &WSError) -> bool {
    if let WSError::AuthError(err) = error {
      debug!("WSClient auth error: {}, stop retry connect", err);
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
