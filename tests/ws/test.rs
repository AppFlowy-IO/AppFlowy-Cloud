use crate::util::{spawn_server, TestUser};
use crate::ws::client::spawn_client;
use serde_json::json;
use std::time::Duration;

#[actix_rt::test]
async fn ws_conn_test() {
  let server = spawn_server().await;
  let test_user = TestUser::generate();
  let token = test_user.register(&server).await;
  let address = format!("{}/{}", server.ws_addr, token);
  let client = spawn_client(1, "1", address).await.unwrap();

  wait_a_sec().await;
  {
    let collab = client.lock();
    collab.insert("1", "a");
  }
  wait_a_sec().await;

  let value = server.get_doc("1");
  assert_json_diff::assert_json_eq!(
    value,
    json!({
      "1": "a"
    })
  );
}

async fn wait_a_sec() {
  tokio::time::sleep(Duration::from_secs(2)).await;
}
