use crate::client::utils::REGISTERED_USERS_MUTEX;
use crate::user_1_signed_in;
use client_api::ws::{ConnectState, WSClient, WSClientConfig};

#[tokio::test]
async fn realtime_connect_test() {
  let _guard = REGISTERED_USERS_MUTEX.lock().await;

  let c = user_1_signed_in().await;
  let ws_client = WSClient::new(WSClientConfig {
    buffer_capacity: 100,
    ping_per_secs: 2,
    retry_connect_per_pings: 5,
  });
  let mut state = ws_client.subscribe_connect_state().await;

  loop {
    tokio::select! {
        _ = ws_client.connect(c.ws_url("fake_device_id").unwrap()) => {},
       value = state.recv() => {
        let new_state = value.unwrap();
        if new_state == ConnectState::Connected {
          break;
        }
      },
    }
  }
}

#[tokio::test]
async fn realtime_disconnect_test() {
  let _guard = REGISTERED_USERS_MUTEX.lock().await;

  let c = user_1_signed_in().await;

  let ws_client = WSClient::new(WSClientConfig {
    buffer_capacity: 100,
    ping_per_secs: 2,
    retry_connect_per_pings: 5,
  });
  ws_client
    .connect(c.ws_url("fake_device_id").unwrap())
    .await
    .unwrap();

  let mut state = ws_client.subscribe_connect_state().await;
  loop {
    tokio::select! {
        _ = ws_client.disconnect() => {},
       value = state.recv() => {
        let new_state = value.unwrap();
        if new_state == ConnectState::Disconnected {
          break;
        }
      },
    }
  }
}
