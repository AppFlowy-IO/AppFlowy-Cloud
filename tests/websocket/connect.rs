use crate::user::utils::generate_unique_registered_user_client;
use client_api::ws::{ConnectState, WSClient, WSClientConfig};

#[tokio::test]
async fn realtime_connect_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let ws_client = WSClient::new(WSClientConfig::default(), c.clone());
  let mut state = ws_client.subscribe_connect_state();
  let device_id = "fake_device_id";
  loop {
    tokio::select! {
        _ = ws_client.connect(c.ws_url(device_id).unwrap(), device_id) => {},
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
  let (c, _user) = generate_unique_registered_user_client().await;
  let ws_client = WSClient::new(WSClientConfig::default(), c.clone());
  let device_id = "fake_device_id";
  ws_client
    .connect(c.ws_url(device_id).unwrap(), device_id)
    .await
    .unwrap();

  let mut state = ws_client.subscribe_connect_state();
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

// use std::time::Duration;
// use tokio_tungstenite::tungstenite::Message;
// #[tokio::test]
// async fn max_frame_size() {
//   let (c, _user) = generate_unique_registered_user_client().await;
//   let ws_client = WSClient::new(WSClientConfig::default(), c.clone());
//   let device_id = "fake_device_id";
//   ws_client
//     .connect(c.ws_url(device_id).unwrap(), device_id)
//     .await
//     .unwrap();
//
//   ws_client.send(Message::Binary(vec![0; 65536])).unwrap();
//   tokio::time::sleep(Duration::from_secs(5)).await;
// }
