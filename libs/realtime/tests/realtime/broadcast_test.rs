use crate::realtime::script::CollabTest;
use serde_json::json;

#[actix_rt::test]
async fn client_receive_changes_test() {
  // Client 0 edit the collab and send the changes to server. Server broadcast the changes to other clients.
  // In the end, the collab should be the same for all clients.
  let mut test = CollabTest::new().await;
  test.create_client(0).await;
  test.create_client(1).await;
  test.create_client(2).await;
  test.open_object(0, "1").await;
  test.open_object(1, "1").await;
  test.open_object(2, "1").await;

  // edit
  test
    .modify_object(0, "1", |collab| {
      collab.insert("1", "a");
    })
    .await;

  // wait for server to receive the update and broadcast to other clients
  test.wait(2).await;

  let server = test.get_server_collab("1").await;
  let client_0 = test.get_client_collab(0, "1").await;
  let client_1 = test.get_client_collab(0, "1").await;
  let client_2 = test.get_client_collab(1, "1").await;

  assert_json_diff::assert_json_eq!(
    client_0,
    json!({
      "1": "a"
    })
  );
  assert_eq!(client_0, server);
  assert_eq!(client_0, client_1);
  assert_eq!(client_0, client_2);
}
