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
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = uuid::Uuid::new_v4().to_string();

  // Using [CollabType::Unknown] for testing purposes.
  let collab_type = CollabType::Unknown;
  test_client
    .create_and_edit_collab_with_data(&object_id, &workspace_id, collab_type, None)
    .await;
  test_client
    .open_collab(&workspace_id, &object_id, collab_type)
    .await;

  // from the beginning, there should be no snapshots
  let snapshots = test_client
    .api_client
    .get_snapshots(&workspace_id, &object_id, collab_type)
    .await
    .unwrap()
    .items;
  assert!(snapshots.is_empty());

  // Simulate the client editing the collaboration object. A snapshot is generated if the number of edits
  // exceeds a specific threshold. By default, [CollabType::Unknown] has a threshold of 10 edits in debug mode.
  for i in 0..10 {
    let mut lock = test_client
      .collabs
      .get(&object_id)
      .unwrap()
      .collab
      .write()
      .await;
    lock.borrow_mut().insert(&i.to_string(), i.to_string());
    sleep(std::time::Duration::from_millis(1000)).await;
  }
  // Wait for the snapshot to be generated.
  sleep(std::time::Duration::from_secs(10)).await;
  let snapshots = test_client
    .api_client
    .get_snapshots(&workspace_id, &object_id, collab_type)
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
  let snapshot_collab = get_snapshot_collab(&full_collab, &snapshot, &object_id);
  let snapshot_json = snapshot_collab.to_json_value();
  assert_json_include!(
    actual: snapshot_json,
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

pub fn get_snapshot_collab(collab: &Collab, snapshot: &Snapshot, object_id: &str) -> Collab {
  let txn = collab.transact();
  let mut encoder = EncoderV2::new();
  collab
    .transact()
    .encode_state_from_snapshot(snapshot, &mut encoder)
    .unwrap();
  let update = Update::decode_v2(&encoder.to_vec()).unwrap();
  drop(txn);

  let mut collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
  let mut txn = collab.transact_mut();
  txn.apply_update(update).unwrap();
  drop(txn);
  collab
}
