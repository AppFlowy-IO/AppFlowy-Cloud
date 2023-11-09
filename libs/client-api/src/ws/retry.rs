use std::future::Future;
use std::pin::Pin;

use crate::ws::WSError;
use tokio::net::TcpStream;
use tokio_retry::Action;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tracing::info;

pub(crate) struct ConnectAction {
  addr: String,
}

impl ConnectAction {
  pub fn new(addr: String) -> Self {
    Self { addr }
  }
}

impl Action for ConnectAction {
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + Send + Sync>>;
  type Item = WebSocketStream<MaybeTlsStream<TcpStream>>;
  type Error = WSError;

  fn run(&mut self) -> Self::Future {
    let cloned_addr = self.addr.clone();
    Box::pin(async move {
      info!("🔵websocket start connecting: {}", cloned_addr);
      match connect_async(&cloned_addr).await {
        Ok((stream, _response)) => {
          info!("🟢websocket connect success");
          Ok(stream)
        },
        Err(e) => Err(e.into()),
      }
    })
  }
}
