use crate::ws::{StateNotify, WSClientConnectURLProvider, WSError};
use again::Condition;
use app_error::gotrue::GoTrueError;
use client_websocket::{connect_async, WebSocketStream};
use reqwest::header::HeaderMap;
use std::sync::{Arc, Weak};

pub(crate) struct RefreshTokenRetryCondition;

impl Condition<GoTrueError> for RefreshTokenRetryCondition {
  fn is_retryable(&mut self, error: &GoTrueError) -> bool {
    error.is_network_error()
  }
}
pub async fn retry_connect(
  connect_provider: Arc<dyn WSClientConnectURLProvider>,
  _state_notify: Weak<StateNotify>,
) -> Result<WebSocketStream, WSError> {
  let url = connect_provider.connect_ws_url();
  let connect_info = connect_provider.connect_info().await?;
  let headers: HeaderMap = connect_info.into();
  let stream = connect_async(url, headers).await?;
  Ok(stream)
}
