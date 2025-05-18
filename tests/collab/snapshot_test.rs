use client_api_test::{assert_server_collab, TestClient};
use collab::core::collab::CollabOptions;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::{Collab, JsonValue};
use collab_entity::CollabType;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn read_write_snapshot() {
  let mut c = TestClient::new_user().await;

  // prepare initial document
  let wid = c.workspace_id().await;
  let oid = c.create_and_edit_collab(wid, CollabType::Unknown).await;
  c.open_collab(wid, oid, CollabType::Unknown).await;
  c.insert_into(&oid, "title", "t1").await;
  c.wait_object_sync_complete(&oid).await.unwrap();
  assert_server_collab(
    wid,
    &mut c.api_client,
    oid,
    &CollabType::Unknown,
    10,
    json!({"title": "t1"}),
  )
  .await
  .unwrap();
  // create the 1st snapshot
  let m1 = c
    .create_snapshot(&wid, &oid, CollabType::Unknown)
    .await
    .unwrap();

  c.insert_into(&oid, "title", "t2").await;
  c.wait_object_sync_complete(&oid).await.unwrap();
  assert_server_collab(
    wid,
    &mut c.api_client,
    oid,
    &CollabType::Unknown,
    10,
    json!({"title": "t2"}),
  )
  .await
  .unwrap();
  // create the 2nd snapshot
  let m2 = c
    .create_snapshot(&wid, &oid, CollabType::Unknown)
    .await
    .unwrap();

  let snapshots = c.get_snapshot_list(&wid, &oid).await.unwrap();
  assert_eq!(snapshots.0.len(), 2, "expecting 2 snapshots");

  // retrieve state
  verify_snapshot_state(&c, &wid, &oid, &m1.snapshot_id, json!({"title": "t1"})).await;
  verify_snapshot_state(&c, &wid, &oid, &m2.snapshot_id, json!({"title": "t2"})).await;
}

async fn verify_snapshot_state(
  c: &TestClient,
  workspace_id: &Uuid,
  oid: &Uuid,
  snapshot_id: &i64,
  expected: JsonValue,
) {
  let snapshot = c
    .get_snapshot(workspace_id, oid, snapshot_id)
    .await
    .unwrap();

  // retrieve state
  let encoded_collab = EncodedCollab::decode_from_bytes(&snapshot.encoded_collab_v1).unwrap();
  let options = CollabOptions::new(oid.to_string())
    .with_data_source(DataSource::DocStateV1(encoded_collab.doc_state.into()));
  let collab = Collab::new_with_options(CollabOrigin::Empty, options).unwrap();
  let actual = collab.to_json_value();
  assert_eq!(actual, expected);
}
