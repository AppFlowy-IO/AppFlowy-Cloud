use std::sync::Arc;
use std::time::Duration;

use collab_entity::CollabType;
use serde_json::json;
use tokio::time::sleep;
use uuid::Uuid;

use super::util::TestScenario;
use client_api_test::{assert_server_collab, TestClient};
use database_entity::dto::AFRole;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_multiple_text_edits() {
  const READER_COUNT: usize = 1;
  let test_scenario = Arc::new(TestScenario::open(
    "./tests/collab/asset/automerge-paper.json.gz",
  ));
  // create writer
  let mut writer = TestClient::new_user().await;
  sleep(Duration::from_secs(2)).await; // sleep 2 secs to make sure it do not trigger register user too fast in gotrue

  let object_id = Uuid::new_v4().to_string();
  let workspace_id = writer.workspace_id().await;

  writer
    .open_collab(&workspace_id, &object_id, CollabType::Unknown)
    .await;

  // create readers and invite them into the same workspace
  let mut readers = Vec::with_capacity(READER_COUNT);
  for _ in 0..READER_COUNT {
    let mut reader = TestClient::new_user().await;
    sleep(Duration::from_secs(2)).await; // sleep 2 secs to make sure it do not trigger register user too fast in gotrue
    writer
      .invite_and_accepted_workspace_member(&workspace_id, &reader, AFRole::Member)
      .await
      .unwrap();

    reader
      .open_collab(&workspace_id, &object_id, CollabType::Unknown)
      .await;

    readers.push(reader);
  }

  // run test scenario
  let collab = writer.collabs.get(&object_id).unwrap().collab.clone();
  let expected = test_scenario.execute(collab, 25_000).await;

  // wait for the writer to complete sync
  writer.wait_object_sync_complete(&object_id).await.unwrap();

  // wait for the readers to complete sync
  let mut tasks = Vec::with_capacity(READER_COUNT);
  for reader in readers.iter() {
    let fut = reader.wait_object_sync_complete(&object_id);
    tasks.push(fut);
  }
  let results = futures::future::join_all(tasks).await;

  // make sure that the readers are in correct state
  for res in results {
    res.unwrap();
  }

  for mut reader in readers.drain(..) {
    assert_server_collab(
      &workspace_id,
      &mut reader.api_client,
      &object_id,
      &CollabType::Unknown,
      10,
      json!({
        "text-id": &expected,
      }),
    )
    .await
    .unwrap();
  }
}
