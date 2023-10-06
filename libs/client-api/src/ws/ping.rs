use crate::ws::state::{ConnectState, ConnectStateNotify};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Sender;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

pub(crate) struct ServerFixIntervalPing {
  duration: Duration,
  sender: Option<Sender<Message>>,
  #[allow(dead_code)]
  stop_tx: tokio::sync::mpsc::Sender<()>,
  stop_rx: Option<tokio::sync::mpsc::Receiver<()>>,
  state: Arc<parking_lot::Mutex<ConnectStateNotify>>,
  ping_count: Arc<Mutex<u32>>,
  maximum_ping_count: u32,
}

impl ServerFixIntervalPing {
  pub(crate) fn new(
    duration: Duration,
    state: Arc<parking_lot::Mutex<ConnectStateNotify>>,
    sender: Sender<Message>,
    maximum_ping_count: u32,
  ) -> Self {
    let (tx, rx) = tokio::sync::mpsc::channel(1000);
    Self {
      duration,
      stop_tx: tx,
      stop_rx: Some(rx),
      state,
      sender: Some(sender),
      ping_count: Arc::new(Mutex::new(0)),
      maximum_ping_count,
    }
  }

  pub(crate) async fn stop(&self) {
    let _ = self.stop_tx.send(()).await;
  }

  pub(crate) fn run(&mut self) {
    let mut stop_rx = self.stop_rx.take().expect("Only take once");
    let mut interval = tokio::time::interval(self.duration);
    let sender = self.sender.take().expect("Only take once");
    let mut receiver = sender.subscribe();
    let weak_ping_count = Arc::downgrade(&self.ping_count);
    let weak_state = Arc::downgrade(&self.state);
    let reconnect_per_ping = self.maximum_ping_count;
    tokio::spawn(async move {
      loop {
        tokio::select! {
          _ = interval.tick() => {
            let _ = sender.send(Message::Ping(vec![]));
            if let Some(ping_count) = weak_ping_count.upgrade() {
              let mut lock = ping_count.lock().await;
              if *lock >= reconnect_per_ping {
                if let Some(state) =weak_state.upgrade() {
                  state.lock().set_state(ConnectState::PingTimeout);
                }
              } else {
                *lock +=1;
              }
            }
          },
          msg = receiver.recv() => {
            if let Ok(Message::Pong(_)) = msg {
              if let Some(ping_count) = weak_ping_count.upgrade() {
                let mut lock = ping_count.lock().await;
                *lock = 0;

                if let Some(state) =weak_state.upgrade() {
                  state.lock().set_state(ConnectState::Connected);
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
