use assert_json_diff::assert_json_include;
use client_api_test::TestClient;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::preclude::updates::decoder::Decode;
use collab::preclude::updates::encoder::{Encoder, EncoderV2};
use collab::preclude::{Collab, ReadTxn, Snapshot, Update};
use collab_entity::CollabType;
use serde_json::json;
use tokio::time::sleep;

#[tokio::test]
async fn collab_history_and_snapshot_test() {
  // Set up all the required data
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = uuid::Uuid::new_v4().to_string();

  // Using [CollabType::Unknown] for testing purposes.
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
  for i in 0..10 {
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
  sleep(std::time::Duration::from_secs(10)).await;
  let snapshots = test_client
    .api_client
    .get_snapshots(&workspace_id, &object_id, collab_type.clone())
    .await
    .unwrap()
    .items;
  assert!(!snapshots.is_empty());

  // Get the latest history
  let snapshot_info = test_client
    .api_client
    .get_latest_history(&workspace_id, &object_id, collab_type)
    .await
    .unwrap();

  let full_collab = Collab::new_with_source(
    CollabOrigin::Empty,
    &object_id,
    DataSource::DocStateV2(snapshot_info.history.doc_state),
    vec![],
    true,
  )
  .unwrap();

  // Collab restored from the history data may not contain all the data. So just compare part of the data.
  assert_json_include!(
    actual: full_collab.to_json_value(),
    expected: json!({
      "0": "0",
      "1": "1",
      "2": "2",
      "3": "3",
      "4": "4",
      "5": "5",
    })
  );

  //  Collab restored from snapshot data might equal to full_collab or be a subset of full_collab.
  let snapshot = Snapshot::decode_v1(snapshot_info.snapshot_meta.snapshot.as_slice()).unwrap();
  let json_snapshot = json_from_snapshot(&full_collab, &object_id, &snapshot);
  assert_json_include!(
    actual: json_snapshot,
    expected: json!({
      "0": "0",
      "1": "1",
      "2": "2",
      "3": "3",
      "4": "4",
      "5": "5",
    })
  );
}

#[tokio::test]
async fn multiple_snapshots_test() {
  // Set up all the required data
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = uuid::Uuid::new_v4().to_string();

  // Using [CollabType::Unknown] for testing purposes.
  let collab_type = CollabType::Unknown;
  test_client
    .create_and_edit_collab_with_data(&object_id, &workspace_id, collab_type.clone(), None)
    .await;
  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  for i in 1..11 {
    test_client
      .collabs
      .get_mut(&object_id)
      .unwrap()
      .mutex_collab
      .lock()
      .insert(&i.to_string(), i.to_string());
    if i % 5 == 0 {
      sleep(std::time::Duration::from_secs(10)).await;
    } else {
      // simulate delay between edits
      sleep(std::time::Duration::from_millis(500)).await;
    }
  }
  let snapshot_metas = test_client
    .api_client
    .get_snapshots(&workspace_id, &object_id, collab_type.clone())
    .await
    .unwrap()
    .items;

  // Get the latest history
  let snapshot_info = test_client
    .api_client
    .get_latest_history(&workspace_id, &object_id, collab_type)
    .await
    .unwrap();

  let full_collab = Collab::new_with_source(
    CollabOrigin::Empty,
    &object_id,
    DataSource::DocStateV2(snapshot_info.history.doc_state),
    vec![],
    true,
  )
  .unwrap();
  // full_collab is the latest state of the collaboration object. so it should be able to restore
  // all snapshots.
  assert!(!snapshot_metas.is_empty());
  for meta in snapshot_metas.iter() {
    let snapshot = Snapshot::decode_v1(meta.snapshot.as_slice()).unwrap();
    let json_snapshot = json_from_snapshot(&full_collab, &object_id, &snapshot);
    println!("json_snapshot: {}", json_snapshot);
  }
}

fn json_from_snapshot(
  full_collab: &Collab,
  object_id: &str,
  snapshot: &Snapshot,
) -> serde_json::Value {
  let update = doc_state_v2_from_snapshot(full_collab, snapshot);
  let update = Update::decode_v2(update.as_slice()).unwrap();

  let snapshot_collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], true);
  snapshot_collab.with_origin_transact_mut(|txn| {
    txn.apply_update(update);
  });

  snapshot_collab.to_json_value()
}
fn doc_state_v2_from_snapshot(full_collab: &Collab, snapshot: &Snapshot) -> Vec<u8> {
  let txn = full_collab.try_transaction().unwrap();
  let mut encoder = EncoderV2::new();
  txn
    .encode_state_from_snapshot(snapshot, &mut encoder)
    .unwrap();
  encoder.to_vec()
}
