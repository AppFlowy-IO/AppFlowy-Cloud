use crate::client::utils::{REGISTERED_EMAIL, REGISTERED_PASSWORD, REGISTERED_USER_MUTEX};
use crate::client_api_client;
use client_api::ws::{ConnectState, WSClient, WSClientConfig};

#[tokio::test]
async fn realtime_connect_test() {
  let _guard = REGISTERED_USER_MUTEX.lock().await;

  let mut c = client_api_client();
  c.sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
    .await
    .unwrap();

  let ws_client = WSClient::new(
    c.ws_url("fake_device_id").unwrap(),
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
