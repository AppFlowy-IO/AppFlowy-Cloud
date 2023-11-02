use std::future::Future;
use std::pin::Pin;

use crate::ws::WSError;
use tokio::net::TcpStream;
use tokio_retry::Action;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{connect_async_with_config, MaybeTlsStream, WebSocketStream};
use tracing::{error, info};

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
      match connect_async_with_config(
        &cloned_addr,
        Some(WebSocketConfig {
          max_message_size: Some(65_536), // 64KB
          max_frame_size: Some(65_536),   // 64KB
          ..WebSocketConfig::default()
        }),
        false,
      )
      .await
      {
        Ok((stream, _response)) => {
          info!("🟢websocket connect success");
          Ok(stream)
        },
        Err(e) => {
          error!("connect failed: {:?}", e.to_string());
          Err(e.into())
        },
      }
    })
  }
}
