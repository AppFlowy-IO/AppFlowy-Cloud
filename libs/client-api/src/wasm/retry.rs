use crate::ws::{ConnectInfo, CurrentConnInfo, StateNotify, WSError};
use reqwest::header::HeaderMap;
use std::sync::Weak;
use websocket::{connect_async, WebSocketStream};

pub async fn retry_connect(
  url: String,
  info: ConnectInfo,
  _state_notify: Weak<StateNotify>,
  _current_addr: Weak<CurrentConnInfo>,
) -> Result<WebSocketStream, WSError> {
  let headers: HeaderMap = info.into();
  let stream = connect_async(url, headers).await?;
  Ok(stream)
}
