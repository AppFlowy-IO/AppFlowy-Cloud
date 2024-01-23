use collab::core::collab_plugin::EncodedCollab;
use collab::preclude::Collab;
use collab_entity::CollabType;
use serde_json::{json, Value};
use std::time::Duration;

use client_api_test_util::*;
use database::collab::COLLAB_SNAPSHOT_LIMIT;
use uuid::Uuid;

#[tokio::test]
async fn create_snapshot_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let collab_type = CollabType::Document;
  let object_id = Uuid::new_v4().to_string();
  let (data, expected) = test_collab_data(test_client.uid().await, &object_id);

  test_client
    .create_and_edit_collab_with_data(
      object_id.clone(),
      &workspace_id,
      collab_type.clone(),
      Some(data),
    )
    .await;

  let meta = test_client
    .create_snapshot(&workspace_id, &object_id, collab_type)
    .await
    .unwrap();

  assert_server_snapshot(
    &test_client.api_client,
    &workspace_id,
    &object_id,
    &meta.snapshot_id,
    expected,
  )
  .await;
}

#[tokio::test]
async fn get_snapshot_list_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let collab_type = CollabType::Document;
  let object_id = Uuid::new_v4().to_string();
  let (data, _) = test_collab_data(test_client.uid().await, &object_id);

  test_client
    .create_and_edit_collab_with_data(
      object_id.clone(),
      &workspace_id,
      collab_type.clone(),
      Some(data),
    )
    .await;

  // By default, when create a collab, a snapshot will be created.
  // wait for the snapshot to be saved to disk
  tokio::time::sleep(Duration::from_secs(2)).await;
  let list = test_client
    .get_snapshot_list(&workspace_id, &object_id)
    .await
    .unwrap()
    .0;
  assert_eq!(list.len(), 1);

  let meta_1 = test_client
    .create_snapshot(&workspace_id, &object_id, collab_type.clone())
    .await
    .unwrap();
  let meta_2 = test_client
    .create_snapshot(&workspace_id, &object_id, collab_type.clone())
    .await
    .unwrap();
  let list = test_client
    .get_snapshot_list(&workspace_id, &object_id)
    .await
    .unwrap()
    .0;
  assert_eq!(list.len(), 3);
  assert_eq!(list[0].snapshot_id, meta_2.snapshot_id);
  assert_eq!(list[1].snapshot_id, meta_1.snapshot_id);
}

#[tokio::test]
async fn snapshot_limit_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let collab_type = CollabType::Document;
  let object_id = Uuid::new_v4().to_string();
  let (data, _) = test_collab_data(test_client.uid().await, &object_id);

  test_client
    .create_and_edit_collab_with_data(
      object_id.clone(),
      &workspace_id,
      collab_type.clone(),
      Some(data),
    )
    .await;

  // When a new snapshot is created that surpasses the preset limit, older snapshots
  // will be deleted to maintain the limit
  for _ in 0..(2 * COLLAB_SNAPSHOT_LIMIT) {
    let _ = test_client
      .create_snapshot(&workspace_id, &object_id, collab_type.clone())
      .await
      .unwrap();
  }

  let list = test_client
    .get_snapshot_list(&workspace_id, &object_id)
    .await
    .unwrap()
    .0;
  assert_eq!(list.len() as i64, COLLAB_SNAPSHOT_LIMIT);
}

fn test_collab_data(uid: i64, oid: &str) -> (EncodedCollab, Value) {
  let collab = Collab::new(uid, oid, "fake_device_id", vec![]);
  collab.with_origin_transact_mut(|txn| {
    collab.insert_with_txn(txn, "0", "a");
    collab.insert_with_txn(txn, "1", "b");
    collab.insert_with_txn(txn, "2", "c");
  });
  (
    collab.encode_collab_v1(),
    json!({
      "0": "a",
      "1": "b",
      "2": "c",
    }),
  )
}
