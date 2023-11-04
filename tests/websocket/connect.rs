use crate::user::utils::generate_unique_registered_user_client;
use client_api::ws::{ConnectState, WSClient, WSClientConfig};

#[tokio::test]
async fn realtime_connect_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let ws_client = WSClient::new(WSClientConfig {
    buffer_capacity: 100,
    ping_per_secs: 6,
    retry_connect_per_pings: 5,
  });
  let mut state = ws_client.subscribe_connect_state();

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
  let (c, _user) = generate_unique_registered_user_client().await;
  let ws_client = WSClient::new(WSClientConfig {
    buffer_capacity: 100,
    ping_per_secs: 6,
    retry_connect_per_pings: 5,
  });
  ws_client
    .connect(c.ws_url("fake_device_id").unwrap())
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

// #[tokio::test]
// async fn max_frame_size() {
//   let (c, _user) = generate_unique_registered_user_client().await;
//   let ws_client = WSClient::new(WSClientConfig {
//     buffer_capacity: 100,
//     ping_per_secs: 6,
//     retry_connect_per_pings: 5,
//   });
//   ws_client
//     .connect(c.ws_url("fake_device_id").unwrap())
//     .await
//     .unwrap();
//
//   for _ in 0..10 {
//     let sender = ws_client.sender();
//     tokio::spawn(async move {
//       sender.send(Message::Binary(vec![0; 65536])).unwrap();
//     });
//   }
//
//   tokio::time::sleep(Duration::from_secs(5)).await;
// }
