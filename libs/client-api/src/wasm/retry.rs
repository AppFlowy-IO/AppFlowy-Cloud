use crate::ws::{CurrentAddr, StateNotify, WSError};
use reqwest::header::HeaderMap;
use std::sync::Weak;
use websocket::{connect_async, WebSocketStream};

pub async fn retry_connect(
  addr: &str,
  _state_notify: Weak<StateNotify>,
  _current_addr: Weak<CurrentAddr>,
) -> Result<WebSocketStream, WSError> {
  let stream = connect_async(addr, HeaderMap::new()).await?;
  Ok(stream)
}
