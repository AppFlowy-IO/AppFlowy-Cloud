use crate::util::{spawn_server, TestUser};

use collab_client_ws::{WSClient, WSClientConfig};

#[actix_rt::test]
async fn ws_retry_connect() {
  let server = spawn_server().await;
  let test_user = TestUser::generate();
  let token = test_user.register(&server).await;
  let address = format!("{}/{}", server.ws_addr, token);

  let ws_client = WSClient::new(
    address,
    WSClientConfig {
      buffer_capacity: 100,
      ping_per_secs: 2,
      retry_connect_per_pings: 5,
    },
  );
  let _addr = ws_client.connect().await.unwrap().unwrap();
  // wait(20).await;
}
