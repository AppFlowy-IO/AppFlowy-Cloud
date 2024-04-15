use anyhow::Error;
use assert_json_diff::assert_json_eq;
use collab::core::collab::{DataSource, MutexCollab};
use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_entity::CollabType;
use serde_json::{json, Value};

use client_api_test_util::*;
use database::collab::COLLAB_SNAPSHOT_LIMIT;
use uuid::Uuid;

#[tokio::test]
async fn create_snapshot_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let collab_type = CollabType::Unknown;
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
async fn get_snapshot_data_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let collab_type = CollabType::Unknown;
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

  let metas = test_client
    .get_snapshot_list_until(
      &workspace_id,
      &object_id,
      |metas| metas.0.len() == 1,
      10 * 60,
    )
    .await
    .unwrap()
    .0;

  // when create a new collab, it will create a snapshot
  assert_eq!(metas.len(), 1);
  let meta = &metas[0];

  let data = test_client
    .get_snapshot(&workspace_id, &object_id, &meta.snapshot_id)
    .await
    .unwrap();

  let encoded_collab = EncodedCollab::decode_from_bytes(&data.encoded_collab_v1).unwrap();
  let collab = MutexCollab::new_with_source(
    CollabOrigin::Empty,
    &object_id,
    DataSource::DocStateV1(encoded_collab.doc_state.to_vec()),
    vec![],
    false,
  )
  .unwrap();
  let json = collab.to_json_value();
  assert_json_eq!(
    json,
    json!({
      "0": "a",
      "1": "b",
      "2": "c"
    })
  );
}

#[tokio::test]
async fn snapshot_limit_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;

  let collab_type = CollabType::Unknown;
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
  let collab = Collab::new(uid, oid, "fake_device_id", vec![], false);
  collab.with_origin_transact_mut(|txn| {
    collab.insert_with_txn(txn, "0", "a");
    collab.insert_with_txn(txn, "1", "b");
    collab.insert_with_txn(txn, "2", "c");
  });
  (
    collab.encode_collab_v1(|_| Ok::<(), Error>(())).unwrap(),
    json!({
      "0": "a",
      "1": "b",
      "2": "c",
    }),
  )
}
