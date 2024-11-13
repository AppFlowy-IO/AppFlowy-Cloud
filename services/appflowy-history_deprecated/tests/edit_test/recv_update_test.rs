use crate::edit_test::mock::mock_test_data;
use crate::util::{check_doc_state_json, redis_stream, run_test_server};
use collab_entity::CollabType;
use tonic_proto::history::SnapshotRequestPb;

#[tokio::test]
async fn apply_update_stream_updates_test() {
  let redis_stream = redis_stream().await;
  let workspace_id = uuid::Uuid::new_v4().to_string();
  let object_id = uuid::Uuid::new_v4().to_string();
  let mock = mock_test_data(&workspace_id, &object_id, 30).await;

  // use a random control stream key
  let control_stream_key = uuid::Uuid::new_v4().to_string();
  let client = run_test_server(control_stream_key).await;

  let control_stream_key = client.config.stream_settings.control_key.clone();
  let mut control_group = redis_stream
    .collab_control_stream(&control_stream_key, "appflowy_cloud")
    .await
    .unwrap();

  // apply open event
  control_group
    .insert_message(mock.open_event.clone())
    .await
    .unwrap();

  let mut update_group = redis_stream
    .collab_update_stream(&workspace_id, &object_id, "appflowy_cloud")
    .await
    .unwrap();

  // apply updates
  for update_event in &mock.update_events {
    update_group
      .insert_message(update_event.clone())
      .await
      .unwrap();
  }

  let request = SnapshotRequestPb {
    workspace_id: workspace_id.to_string(),
    object_id: object_id.to_string(),
    collab_type: CollabType::Unknown.value(),
    num_snapshot: 1,
  };

  check_doc_state_json(&object_id, 60, mock.expected_json.clone(), move || {
    let mut cloned_client = client.clone();
    let cloned_request = request.clone();
    Box::pin(async move {
      cloned_client
        .get_latest_snapshot(cloned_request)
        .await
        .map(|r| r.into_inner().history_state.unwrap())
    })
  })
  .await
  .unwrap();
}

// #[tokio::test]
// async fn apply_missing_updates_test() {
//   let redis_stream = redis_stream().await;
//   let workspace_id = uuid::Uuid::new_v4().to_string();
//   let object_id = uuid::Uuid::new_v4().to_string();
//   let mock = mock_test_data(&workspace_id, &object_id, 30).await;
//   let client = run_test_server(uuid::Uuid::new_v4().to_string()).await;
//
//   let control_stream_key = client.config.stream_settings.control_key.clone();
//   let mut control_group = redis_stream
//     .collab_control_stream(&control_stream_key, "appflowy_cloud")
//     .await
//     .unwrap();
//
//   // apply open event
//   control_group
//     .insert_message(mock.open_event.clone())
//     .await
//     .unwrap();
//
//   let mut update_group = redis_stream
//     .collab_update_stream(&workspace_id, &object_id, "appflowy_cloud")
//     .await
//     .unwrap();
//
//   let mut missing_updates = vec![];
//   // apply updates
//   for (index, update_event) in mock.update_events.iter().enumerate() {
//     if index % 2 == 0 {
//       missing_updates.push(update_event.clone());
//     } else {
//       update_group
//         .insert_message(update_event.clone())
//         .await
//         .unwrap();
//     }
//   }
//
//   for update_event in missing_updates {
//     update_group.insert_message(update_event).await.unwrap();
//   }
//
//   let request = SnapshotRequestPb {
//     workspace_id: workspace_id.to_string(),
//     object_id: object_id.to_string(),
//     collab_type: CollabType::Unknown.value(),
//   };
//
//   check_doc_state_json(&object_id, 120, mock.expected_json.clone(), move || {
//     let mut cloned_client = client.clone();
//     let cloned_request = request.clone();
//     Box::pin(async move {
//       cloned_client
//         .get_in_memory_history(cloned_request)
//         .await
//         .map(|r| r.into_inner())
//     })
//   })
//   .await
//   .unwrap();
// }
