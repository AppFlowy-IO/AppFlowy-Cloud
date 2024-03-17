use crate::ws::ConnectStateNotify;
use client_websocket::Message;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Sender;
use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;
#[allow(dead_code)]
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
    // TODO(nathan): implement the ping for wasm
  }
}
