use crate::ws::{ConnectInfo, CurrentConnInfo, StateNotify, WSError};
use again::Condition;
use app_error::gotrue::GoTrueError;
use client_websocket::{connect_async, WebSocketStream};
use reqwest::header::HeaderMap;
use std::sync::Weak;

pub(crate) struct RefreshTokenRetryCondition;

impl Condition<GoTrueError> for RefreshTokenRetryCondition {
  fn is_retryable(&mut self, error: &GoTrueError) -> bool {
    error.is_network_error()
  }
}
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
