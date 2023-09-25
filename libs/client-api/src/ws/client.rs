use futures_util::{SinkExt, StreamExt};

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::ws::ping::ServerFixIntervalPing;
use crate::ws::retry::ConnectAction;
use crate::ws::state::{ConnectState, ConnectStateNotify};
use crate::ws::{BusinessID, ClientRealtimeMessage, WSError, WebSocketChannel};
use tokio::sync::broadcast::{channel, Receiver, Sender};
use tokio::sync::{Mutex, RwLock};
use tokio_retry::strategy::FibonacciBackoff;
use tokio_retry::{Condition, RetryIf};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::MaybeTlsStream;
use tracing::{error, trace, warn};

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

pub struct WSClient {
  addr: Arc<parking_lot::Mutex<Option<String>>>,
  config: WSClientConfig,
  state_notify: Arc<Mutex<ConnectStateNotify>>,
  /// Sender used to send messages to the websocket.
  sender: Sender<Message>,
  channels: Arc<RwLock<HashMap<BusinessID, ChannelByObjectId>>>,
  ping: Arc<Mutex<Option<ServerFixIntervalPing>>>,
}

impl WSClient {
  pub fn new(config: WSClientConfig) -> Self {
    let (sender, _) = channel(config.buffer_capacity);
    let state = Arc::new(Mutex::new(ConnectStateNotify::new()));
    let channels = Arc::new(RwLock::new(HashMap::new()));
    let ping = Arc::new(Mutex::new(None));
    WSClient {
      addr: Arc::new(parking_lot::Mutex::new(None)),
      config,
      state_notify: state,
      sender,
      channels,
      ping,
    }
  }

  pub async fn connect(&self, addr: String) -> Result<Option<SocketAddr>, WSError> {
    *self.addr.lock() = Some(addr.clone());
    if let Some(old_ping) = self.ping.lock().await.as_ref() {
      old_ping.stop().await;
    }
    self.set_state(ConnectState::Connecting).await;

    let retry_strategy = FibonacciBackoff::from_millis(2000).max_delay(Duration::from_secs(5 * 60));
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

    let (mut sink, mut stream) = stream.split();
    self.set_state(ConnectState::Connected).await;
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
                  .await
                  .get(&msg.business_id)
                  .and_then(|map| map.get(&msg.object_id))
                {
                  match channel.upgrade() {
                    None => {
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
          Message::Close(_) => {},
          Message::Frame(_) => {},
        }
      }
    });

    let mut sink_rx = self.sender.subscribe();
    tokio::spawn(async move {
      while let Ok(msg) = sink_rx.recv().await {
        match sink.send(msg).await {
          Ok(_) => {},
          Err(e) => {
            tracing::error!("🔴Failed to send message via websocket: {:?}", e);
          },
        }
      }
    });

    Ok(addr)
  }

  /// Return a [WebSocketChannel] that can be used to send messages to the websocket. Caller should
  /// keep the channel alive as long as it wants to receive messages from the websocket.
  pub async fn subscribe(
    &self,
    business_id: BusinessID,
    object_id: String,
  ) -> Result<Arc<WebSocketChannel>, WSError> {
    let channel = Arc::new(WebSocketChannel::new(business_id, self.sender.clone()));
    self
      .channels
      .write()
      .await
      .entry(business_id)
      .or_insert_with(HashMap::new)
      .insert(object_id, Arc::downgrade(&channel));
    Ok(channel)
  }

  pub async fn subscribe_connect_state(&self) -> Receiver<ConnectState> {
    self.state_notify.lock().await.subscribe()
  }

  pub async fn is_connected(&self) -> bool {
    self.state_notify.lock().await.state.is_connected()
  }

  pub async fn disconnect(&self) {
    *self.addr.lock() = None;
    let _ = self.sender.send(Message::Close(None));
    self.set_state(ConnectState::Disconnected).await;
  }

  async fn set_state(&self, state: ConnectState) {
    trace!("websocket state: {:?}", state);
    self.state_notify.lock().await.set_state(state);
  }
}

struct RetryCondition {
  connecting_addr: String,
  addr: Weak<parking_lot::Mutex<Option<String>>>,
}
impl Condition<WSError> for RetryCondition {
  fn should_retry(&mut self, _error: &WSError) -> bool {
    self
      .addr
      .upgrade()
      .map(|addr| match addr.lock().as_ref() {
        None => false,
        Some(addr) => addr == &self.connecting_addr,
      })
      .unwrap_or(false)
  }
}
