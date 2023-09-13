use crate::client::utils::{REGISTERED_EMAIL, REGISTERED_PASSWORD};
use crate::client_api_client;
use collab_ws::{ConnectState, WSClient, WSClientConfig};

#[tokio::test]
async fn realtime_connect_test() {
  let mut c = client_api_client();
  c.sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap();

  let ws_client = WSClient::new(
    c.ws_url().unwrap(),
    WSClientConfig {
      buffer_capacity: 100,
      ping_per_secs: 2,
      retry_connect_per_pings: 5,
    },
  );
  let mut state = ws_client.subscribe_connect_state().await;

  loop {
    tokio::select! {
        _ = ws_client.connect() => {},
       value = state.recv() => {
        let new_state = value.unwrap();
        if new_state == ConnectState::Connected {
          break;
        }
      },
    }
  }
}

// #[tokio::test]
// async fn realtime_write_test() {
//   let mut c = client_api_client();
//   c.sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
//     .await
//     .unwrap();
//
//   let ws_client = WSClient::new(
//     c.ws_url().unwrap(),
//     WSClientConfig {
//       buffer_capacity: 100,
//       ping_per_secs: 2,
//       retry_connect_per_pings: 5,
//     },
//   );
//   ws_client.connect().await.unwrap();
//
//   let handler = ws_client.subscribe(1, "test".to_string()).await.unwrap();
//   let mut sink = handler.sink::<TestMessage>();
//   let mut stream = handler.stream::<TestMessage>();
//   let msg = TestMessage {
//     object_id: "test".to_string(),
//   };
//   sink.send(msg.clone()).await.unwrap();
//
//   tokio::time::sleep(Duration::from_secs(3)).await;
//   // loop {
//   //   tokio::select! {
//   //       _ = sink.send(msg.clone()) => {},
//   //      msg = stream.next() => {
//   //        break;
//   //     },
//   //   }
//   // }
// }

// #[derive(Clone, Debug)]
// struct TestMessage {
//   object_id: String,
// }
//
// impl From<TestMessage> for WSMessage {
//   fn from(value: TestMessage) -> Self {
//     WSMessage {
//       business_id: 0,
//       object_id: value.object_id,
//       payload: vec![],
//     }
//   }
// }
//
// impl From<WSMessage> for TestMessage {
//   fn from(value: WSMessage) -> Self {
//     TestMessage {
//       object_id: value.object_id,
//     }
//   }
// }
