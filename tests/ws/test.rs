use crate::util::{spawn_server, TestUser};
use crate::ws::client::spawn_client;

#[actix_rt::test]
async fn ws_conn_test() {
  let server = spawn_server().await;
  let test_user = TestUser::generate();
  let token = test_user.register(&server).await;
  let address = format!("{}/{}", server.ws_addr, token);
  let client = spawn_client(1, "1", address).await.unwrap();
}
