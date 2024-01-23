use std::future::Future;
use std::pin::Pin;

use crate::ws::WSError;
use tokio_retry::Action;
use tracing::info;
use websocket::{connect_async, WebSocketStream};

pub(crate) struct ConnectAction {
  addr: String,
}

impl ConnectAction {
  pub fn new(addr: String) -> Self {
    Self { addr }
  }
}

impl Action for ConnectAction {
  #[cfg(not(target_arch = "wasm32"))]
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + Send + Sync>>;

  #[cfg(target_arch = "wasm32")]
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>>>>;
  type Item = WebSocketStream;
  type Error = WSError;

  fn run(&mut self) -> Self::Future {
    let cloned_addr = self.addr.clone();
    Box::pin(async move {
      info!("🔵websocket start connecting");
      match connect_async(&cloned_addr).await {
        Ok(stream) => {
          info!("🟢websocket connect success");
          Ok(stream)
        },
        Err(e) => Err(e.into()),
      }
    })
  }
}
