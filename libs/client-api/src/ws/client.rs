use futures_util::{SinkExt, StreamExt};

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::ws::retry::ConnectAction;
use crate::ws::{BusinessID, ClientRealtimeMessage, WSError, WebSocketChannel};
use tokio::sync::broadcast::{channel, Receiver, Sender};
use tokio::sync::{Mutex, RwLock};
use tokio_retry::strategy::FixedInterval;
use tokio_retry::Retry;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::MaybeTlsStream;

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
      retry_connect_per_pings: 10,
    }
  }
}

type ChannelByObjectId = HashMap<String, Weak<WebSocketChannel>>;

pub struct WSClient {
  addr: Mutex<Option<String>>,
  state: Arc<Mutex<ConnectStateNotify>>,
  sender: Sender<Message>,
  channels: Arc<RwLock<HashMap<BusinessID, ChannelByObjectId>>>,
  ping: Arc<Mutex<ServerFixIntervalPing>>,
}

impl WSClient {
  pub fn new(config: WSClientConfig) -> Self {
    let (sender, _) = channel(config.buffer_capacity);
    let state = Arc::new(Mutex::new(ConnectStateNotify::new()));
    let channels = Arc::new(RwLock::new(HashMap::new()));
    let ping = Arc::new(Mutex::new(ServerFixIntervalPing::new(
      Duration::from_secs(config.ping_per_secs),
      state.clone(),
      sender.clone(),
      config.retry_connect_per_pings,
    )));
    WSClient {
      addr: Mutex::new(None),
      state,
      sender,
      channels,
      ping,
    }
  }

  pub async fn connect(&self, addr: String) -> Result<Option<SocketAddr>, WSError> {
    *self.addr.lock().await = Some(addr.clone());

    self.set_state(ConnectState::Connecting).await;
    let retry_strategy = FixedInterval::new(Duration::from_secs(2)).take(3);
    let action = ConnectAction::new(addr);
    let stream = Retry::spawn(retry_strategy, action).await?;
    let addr = match stream.get_ref() {
      MaybeTlsStream::Plain(s) => s.local_addr().ok(),
      _ => None,
    };

    let (mut sink, mut stream) = stream.split();
    self.set_state(ConnectState::Connected).await;
    let weak_channels = Arc::downgrade(&self.channels);
    let sender = self.sender.clone();
    self.ping.lock().await.run();
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
                  .and_then(|channel| channel.upgrade())
                {
                  channel.recv_msg(&msg);
                }
              }
            } else {
              tracing::error!("🔴Invalid message from websocket");
            }
          },
          Message::Ping(_) => match sender.send(Message::Pong(vec![])) {
            Ok(_) => {},
            Err(e) => {
              tracing::error!("🔴Failed to send pong message to websocket: {:?}", e);
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
    self.state.lock().await.subscribe()
  }

  pub async fn is_connected(&self) -> bool {
    self.state.lock().await.state.is_connected()
  }

  pub async fn disconnect(&self) {
    let _ = self.sender.send(Message::Close(None));
  }

  async fn set_state(&self, state: ConnectState) {
    self.state.lock().await.set_state(state);
  }
}

struct ServerFixIntervalPing {
  duration: Duration,
  sender: Option<Sender<Message>>,
  #[allow(dead_code)]
  stop_tx: tokio::sync::mpsc::Sender<()>,
  stop_rx: Option<tokio::sync::mpsc::Receiver<()>>,
  state: Arc<Mutex<ConnectStateNotify>>,
  ping_count: Arc<Mutex<u32>>,
  retry_connect_per_pings: u32,
}

impl ServerFixIntervalPing {
  fn new(
    duration: Duration,
    state: Arc<Mutex<ConnectStateNotify>>,
    sender: Sender<Message>,
    retry_connect_per_pings: u32,
  ) -> Self {
    let (tx, rx) = tokio::sync::mpsc::channel(1000);
    Self {
      duration,
      stop_tx: tx,
      stop_rx: Some(rx),
      state,
      sender: Some(sender),
      ping_count: Arc::new(Mutex::new(0)),
      retry_connect_per_pings,
    }
  }

  fn run(&mut self) {
    let mut stop_rx = self.stop_rx.take().expect("Only take once");
    let mut interval = tokio::time::interval(self.duration);
    let sender = self.sender.take().expect("Only take once");
    let mut receiver = sender.subscribe();
    let weak_ping_count = Arc::downgrade(&self.ping_count);
    let weak_state = Arc::downgrade(&self.state);
    let reconnect_per_ping = self.retry_connect_per_pings;
    tokio::spawn(async move {
      loop {
        tokio::select! {
          _ = interval.tick() => {
            // Send the ping
            tracing::trace!("🙂ping");
            let _ = sender.send(Message::Ping(vec![]));
            if let Some(ping_count) = weak_ping_count.upgrade() {
              let mut lock = ping_count.lock().await;
              // After ten ping were sent, mark the connection as disconnected
              if *lock >= reconnect_per_ping {
                if let Some(state) =weak_state.upgrade() {
                  state.lock().await.set_state(ConnectState::Disconnected);
                }
              } else {
                *lock +=1;
              }
            }
          },
          msg = receiver.recv() => {
            if let Ok(Message::Pong(_)) = msg {
              tracing::trace!("🟢Receive pong from server");
              if let Some(ping_count) = weak_ping_count.upgrade() {
                let mut lock = ping_count.lock().await;
                *lock = 0;

                if let Some(state) =weak_state.upgrade() {
                  state.lock().await.set_state(ConnectState::Connected);
                }
              }
            }
          },
          _ = stop_rx.recv() => {
            break;
          }
        }
      }
    });
  }
}

pub struct ConnectStateNotify {
  state: ConnectState,
  sender: Sender<ConnectState>,
}

impl ConnectStateNotify {
  fn new() -> Self {
    let (sender, _) = channel(100);
    Self {
      state: ConnectState::Disconnected,
      sender,
    }
  }

  fn set_state(&mut self, state: ConnectState) {
    if self.state != state {
      tracing::trace!("[🙂Client]: connect state changed to {:?}", state);
      self.state = state.clone();
      let _ = self.sender.send(state);
    }
  }

  fn subscribe(&self) -> Receiver<ConnectState> {
    self.sender.subscribe()
  }
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum ConnectState {
  Connecting,
  Connected,
  Disconnected,
}

impl ConnectState {
  #[allow(dead_code)]
  fn is_connecting(&self) -> bool {
    matches!(self, ConnectState::Connecting)
  }

  #[allow(dead_code)]
  fn is_connected(&self) -> bool {
    matches!(self, ConnectState::Connected)
  }

  #[allow(dead_code)]
  fn is_disconnected(&self) -> bool {
    matches!(self, ConnectState::Disconnected)
  }
}
