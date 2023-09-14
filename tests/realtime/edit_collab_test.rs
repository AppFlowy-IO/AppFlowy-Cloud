use crate::client_api_client;
use serde_json::json;

use collab_define::CollabType;

use crate::realtime::test_client::{assert_collab_json, TestClient};
use std::time::Duration;

#[tokio::test]
async fn realtime_write_collab_test() {
  let mut client_api = client_api_client();
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;
  let test_client = TestClient::new(&mut client_api, &object_id, collab_type.clone()).await;

  // Edit the collab
  for i in 0..=5 {
    test_client
      .collab
      .lock()
      .insert(&i.to_string(), i.to_string());
  }

  // Wait for the messages to be sent
  tokio::time::sleep(Duration::from_secs(2)).await;
  test_client.disconnect().await;

  assert_collab_json(
    &client_api,
    &object_id,
    &collab_type,
    5,
    json!( {
      "0": "0",
      "1": "1",
      "2": "2",
      "3": "3",
      "4": "4",
      "5": "5",
    }),
  )
  .await;
}
