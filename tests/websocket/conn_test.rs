use std::time::{Duration, SystemTime};
use tokio::time::timeout;

use client_api::ws::{ConnectState, WSClient, WSClientConfig};
use client_api_test::generate_unique_registered_user_client;

#[tokio::test]
async fn realtime_connect_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let ws_client = WSClient::new(WSClientConfig::default(), c.clone(), c.clone());
  let mut state = ws_client.subscribe_connect_state();
  tokio::spawn(async move { ws_client.connect().await });
  let connect_future = async {
    loop {
      match state.recv().await {
        Ok(ConnectState::Connected) => {
          break;
        },
        Ok(_) => {},
        Err(err) => panic!("Receiver Error: {:?}", err),
      }
    }
  };

  // Apply the timeout
  match timeout(Duration::from_secs(10), connect_future).await {
    Ok(_) => {},
    Err(_) => panic!("Connection timeout."),
  }
}

#[tokio::test]
async fn realtime_connect_after_token_exp_test() {
  let (c, _user) = generate_unique_registered_user_client().await;

  // Set the token to be expired
  c.token().write().as_mut().unwrap().expires_at = SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64;

  let ws_client = WSClient::new(WSClientConfig::default(), c.clone(), c.clone());
  let mut state = ws_client.subscribe_connect_state();
  tokio::spawn(async move { ws_client.connect().await });
  let connect_future = async {
    loop {
      match state.recv().await {
        Ok(ConnectState::Connected) => {
          break;
        },
        Ok(_) => {},
        Err(err) => panic!("Receiver Error: {:?}", err),
      }
    }
  };

  // Apply the timeout
  match timeout(Duration::from_secs(10), connect_future).await {
    Ok(_) => {},
    Err(_) => panic!("Connection timeout."),
  }
}

#[tokio::test]
async fn realtime_disconnect_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let ws_client = WSClient::new(WSClientConfig::default(), c.clone(), c.clone());
  ws_client.connect().await.unwrap();

  let mut state = ws_client.subscribe_connect_state();
  loop {
    tokio::select! {
        _ = ws_client.disconnect() => {},
       value = state.recv() => {
        let new_state = value.unwrap();
        if new_state == ConnectState::Lost {
          break;
        }
      },
    }
  }
}
