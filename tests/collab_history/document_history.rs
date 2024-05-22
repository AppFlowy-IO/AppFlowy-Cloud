use client_api_test::TestClient;
use collab_entity::CollabType;
use tokio::time::sleep;

#[tokio::test]
async fn realtime_write_single_collab_test() {
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
    .get_snapshot2(&workspace_id, &object_id, collab_type.clone())
    .await
    .unwrap()
    .items;
  assert!(snapshots.is_empty());

  for i in 0..15 {
    test_client
      .collabs
      .get_mut(&object_id)
      .unwrap()
      .mutex_collab
      .lock()
      .insert(&i.to_string(), i.to_string());
    sleep(std::time::Duration::from_millis(500)).await;
  }
  sleep(std::time::Duration::from_secs(20)).await;
  let snapshots = test_client
    .api_client
    .get_snapshot2(&workspace_id, &object_id, collab_type)
    .await
    .unwrap()
    .items;
  assert!(!snapshots.is_empty());
}
