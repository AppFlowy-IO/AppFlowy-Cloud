use assert_json_diff::{assert_json_eq, assert_json_include};
use client_api_test::TestClient;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use serde_json::json;
use tokio::time::sleep;

#[tokio::test]
async fn realtime_write_single_collab_test() {
  // Set up all the required data
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Unknown;
  test_client
    .create_and_edit_collab_with_data(&object_id, &workspace_id, collab_type.clone(), None)
    .await;
  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  // from the beginning, there should be no snapshots
  let snapshots = test_client
    .api_client
    .get_snapshots(&workspace_id, &object_id, collab_type.clone())
    .await
    .unwrap()
    .items;
  assert!(snapshots.is_empty());

  // Simulate the client editing the collaboration object. A snapshot is generated if the number of edits
  // exceeds a specific threshold. By default, [CollabType::Unknown] has a threshold of 10 edits in debug mode.
  for i in 0..20 {
    test_client
      .collabs
      .get_mut(&object_id)
      .unwrap()
      .mutex_collab
      .lock()
      .insert(&i.to_string(), i.to_string());
    sleep(std::time::Duration::from_millis(500)).await;
  }
  // Wait for the snapshot to be generated.
  sleep(std::time::Duration::from_secs(20)).await;

  // For now, the snapshot should be generated.
  let snapshots = test_client
    .api_client
    .get_snapshots(&workspace_id, &object_id, collab_type.clone())
    .await
    .unwrap()
    .items;
  assert!(!snapshots.is_empty());

  // Get the latest history
  let history = test_client
    .api_client
    .get_latest_history(&workspace_id, &object_id, collab_type)
    .await
    .unwrap();
  assert_eq!(history.doc_state_version, 1);

  let collab = Collab::new_with_source(
    CollabOrigin::Empty,
    &object_id,
    DataSource::DocStateV1(history.doc_state),
    vec![],
    false,
  )
  .unwrap();
  let json = collab.to_json_value();
  assert_json_eq!(
    json,
    json!( {
      "0": "0",
      "1": "1",
      "10": "10",
      "11": "11",
      "12": "12",
      "13": "13",
      "14": "14",
      "15": "15",
      "16": "16",
      "17": "17",
      "18": "18",
      "19": "19",
      "2": "2",
      "3": "3",
      "4": "4",
      "5": "5",
      "6": "6",
      "7": "7",
      "8": "8",
      "9": "9"
    })
  );
}
