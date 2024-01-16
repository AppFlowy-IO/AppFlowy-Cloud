use crate::{ConnectState, ConnectStateNotify};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Sender;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

pub(crate) struct ServerFixIntervalPing {
  duration: Duration,
  ping_sender: Option<Sender<Message>>,
  pong_recv: Option<Receiver<()>>,
  #[allow(dead_code)]
  stop_tx: tokio::sync::mpsc::Sender<()>,
  stop_rx: Option<Receiver<()>>,
  state: Arc<parking_lot::Mutex<ConnectStateNotify>>,
  ping_count: Arc<Mutex<u32>>,
  maximum_ping_count: u32,
}

impl ServerFixIntervalPing {
  pub(crate) fn new(
    duration: Duration,
    state: Arc<parking_lot::Mutex<ConnectStateNotify>>,
    ping_sender: Sender<Message>,
    pong_recv: Receiver<()>,
    maximum_ping_count: u32,
  ) -> Self {
    let (tx, rx) = tokio::sync::mpsc::channel(1000);
    Self {
      duration,
      stop_tx: tx,
      stop_rx: Some(rx),
      state,
      ping_sender: Some(ping_sender),
      pong_recv: Some(pong_recv),
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
    let ping_sender = self.ping_sender.take().expect("Only take once");
    let mut pong_recv = self.pong_recv.take().expect("Only take once");
    let weak_ping_count = Arc::downgrade(&self.ping_count);
    let weak_state = Arc::downgrade(&self.state);
    let reconnect_per_ping = self.maximum_ping_count;
    tokio::spawn(async move {
      loop {
        tokio::select! {
          _ = interval.tick() => {
            // send ping to server
            if let Err(err) = ping_sender.send(Message::Ping(vec![])) {
              tracing::error!("ping send error: {}", err);
            }
            if let Some(ping_count) = weak_ping_count.upgrade() {
              let mut lock = ping_count.lock().await;
              if *lock >= reconnect_per_ping {
                if let Some(state) =weak_state.upgrade() {
                  state.lock().set_state(ConnectState::PingTimeout);
                }
              } else {
                if *lock > 1 {
                 tracing::trace!("ping count: {}", *lock);
                }
                *lock +=1;
              }
            }
          },
          // pong from server
          result = pong_recv.recv() => {
            if result.is_none() {
              continue;
            }
            if let Some(ping_count) = weak_ping_count.upgrade() {
              let mut lock = ping_count.lock().await;
              *lock = 0;

              if let Some(state) =weak_state.upgrade() {
                state.lock().set_state(ConnectState::Connected);
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
