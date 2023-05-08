use crate::util::{spawn_server, TestUser};
use collab_client_ws::WSClient;

#[actix_rt::test]
async fn ws_conn_test() {
  let server = spawn_server().await;
  let test_user = TestUser::generate();
  let token = test_user.register(&server).await;

  let address = format!("{}/{}", server.ws_addr, token);
  let client = WSClient::new(address, 100);
  let _ = client.connect().await;
}
