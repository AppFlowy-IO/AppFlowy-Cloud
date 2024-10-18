use std::sync::Arc;
use std::time::Duration;

use collab_entity::CollabType;
use serde_json::json;
use tokio::time::sleep;
use uuid::Uuid;

use client_api_test::{assert_server_collab, TestClient};

use super::util::TestScenario;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn run_multiple_text_edits() {
  let test_scenario = Arc::new(TestScenario::open(
    "./tests/collab/asset/automerge-paper.json.gz",
  ));
  let mut tasks = Vec::new();
  for _i in 0..1 {
    let test_scenario = test_scenario.clone();
    let task = tokio::spawn(async move {
      let mut new_user = TestClient::new_user().await;
      // sleep 2 secs to make sure it do not trigger register user too fast in gotrue
      sleep(Duration::from_secs(2)).await;

      let object_id = Uuid::new_v4().to_string();
      let workspace_id = new_user.workspace_id().await;
      // the big doc_state will force the init_sync using the http request.
      // It will trigger the POST_REALTIME_MESSAGE_STREAM_HANDLER to handle the request.
      new_user
        .open_collab(&workspace_id, &object_id, CollabType::Unknown)
        .await;

      let collab = new_user.collabs.get(&object_id).unwrap().collab.clone();
      test_scenario.execute(collab).await;

      new_user
        .wait_object_sync_complete(&object_id)
        .await
        .unwrap();
      (new_user, object_id, workspace_id)
    });
    tasks.push(task);
  }

  let results = futures::future::join_all(tasks).await;
  for result in results.into_iter() {
    let (mut client, object_id, workspace_id) = result.unwrap();
    assert_server_collab(
      &workspace_id,
      &mut client.api_client,
      &object_id,
      &CollabType::Document,
      10,
      json!({
        "text-id": &test_scenario.end_content,
      }),
    )
    .await
    .unwrap();

    drop(client);
  }
}
