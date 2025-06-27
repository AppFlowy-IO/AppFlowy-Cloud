use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use assert_json_diff::assert_json_eq;
use client_api::entity::AFRole;
use collab::core::origin::CollabOrigin;
use collab_entity::CollabType;
use serde_json::json;
use tokio::time::sleep;
use uuid::Uuid;

use crate::collab::util::{
  generate_random_bytes, generate_random_string, make_collab_with_key_value,
};
use client_api_test::*;
use collab_rt_entity::{CollabMessage, RealtimeMessage, UpdateSync, MAXIMUM_REALTIME_MESSAGE_SIZE};
#[tokio::test]
async fn realtime_write_single_collab_test() {
  let collab_type = CollabType::Unknown;
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = test_client
    .create_and_edit_collab(workspace_id, collab_type)
    .await;

  // Edit the collab
  for i in 0..=5 {
    test_client
      .insert_into(&object_id, &i.to_string(), i.to_string())
      .await;
  }

  let expected_json = json!( {
    "0": "0",
    "1": "1",
    "2": "2",
    "3": "3",
  });
  test_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

  assert_server_collab(
    workspace_id,
    &mut test_client.api_client,
    object_id,
    &collab_type,
    10,
    expected_json,
  )
  .await
  .unwrap();
}
#[tokio::test]
async fn collab_write_small_chunk_of_data_test() {
  let collab_type = CollabType::Unknown;
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4();

  // Calling the open_collab function directly will create the collab object in the plugin.
  // The [CollabStoragePlugin] plugin try to get the collab object from the database, but it doesn't exist.
  // So the plugin will create the collab object.
  test_client
    .open_collab(workspace_id, object_id, collab_type)
    .await;
  let mut expected_json = HashMap::new();

  // Edit the collab
  for i in 0..=20 {
    test_client
      .insert_into(&object_id, &i.to_string(), i.to_string())
      .await;
    expected_json.insert(i.to_string(), i.to_string());
  }

  assert_server_collab(
    workspace_id,
    &mut test_client.api_client,
    object_id,
    &collab_type,
    10,
    json!(expected_json),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn collab_write_big_chunk_of_data_test() {
  let collab_type = CollabType::Unknown;
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4();

  test_client
    .open_collab(workspace_id, object_id, collab_type)
    .await;
  let s = generate_random_string(1000);
  test_client.insert_into(&object_id, "text", s.clone()).await;

  test_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();
  assert_server_collab(
    workspace_id,
    &mut test_client.api_client,
    object_id,
    &collab_type,
    10,
    json!({
      "text": s
    }),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn write_big_chunk_data_init_sync_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4();
  let big_text = generate_random_string(MAXIMUM_REALTIME_MESSAGE_SIZE / 2);
  let collab_type = CollabType::Unknown;
  let doc_state = make_collab_with_key_value(&object_id, "text", big_text.clone());

  // the big doc_state will force the init_sync using the http request.
  // It will trigger the POST_REALTIME_MESSAGE_STREAM_HANDLER to handle the request.
  test_client
    .open_collab_with_doc_state(workspace_id, object_id, collab_type, doc_state)
    .await;
  test_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

  assert_server_collab(
    workspace_id,
    &mut test_client.api_client,
    object_id,
    &collab_type,
    10,
    json!({
      "text": big_text
    }),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn realtime_write_multiple_collab_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let mut object_ids = vec![];
  for _ in 0..5 {
    let collab_type = CollabType::Unknown;

    let object_id = test_client
      .create_and_edit_collab(workspace_id, collab_type)
      .await;
    for i in 0..=5 {
      test_client
        .insert_into(&object_id, &i.to_string(), i.to_string())
        .await;
    }

    test_client
      .wait_object_sync_complete(&object_id)
      .await
      .unwrap();
    object_ids.push(object_id);
  }

  // Wait for the messages to be sent
  for object_id in object_ids {
    assert_server_collab(
      workspace_id,
      &mut test_client.api_client,
      object_id,
      &CollabType::Document,
      10,
      json!( {
        "0": "0",
        "1": "1",
        "2": "2",
        "3": "3",
        "4": "4",
        "5": "5",
      }),
    )
    .await
    .unwrap();
  }
}

#[tokio::test]
async fn second_connect_override_first_connect_test() {
  // Different TestClient with same device connect, the last one will
  // take over the connection.
  let collab_type = CollabType::Unknown;
  let mut client = TestClient::new_user().await;
  let workspace_id = client.workspace_id().await;

  let object_id = client
    .create_and_edit_collab(workspace_id, collab_type)
    .await;

  client.insert_into(&object_id, "1", "a").await;

  // Sleep one second for the doc observer the update. Otherwise, the
  // sync complete might be called before the update being schedule
  sleep(Duration::from_secs(1)).await;
  client.wait_object_sync_complete(&object_id).await.unwrap();

  // the new_client connect with same device_id, so it will replace the existing client
  // in the server. Which means the old client will not receive updates.
  let mut new_client =
    TestClient::new_with_device_id(&client.device_id, client.user.clone(), true).await;
  new_client
    .open_collab(workspace_id, object_id, collab_type)
    .await;
  new_client.insert_into(&object_id, "2", "b").await;
  new_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

  assert_client_collab_include_value(
    &mut new_client,
    &object_id,
    json!({
      "1": "a",
      "2": "b"
    }),
  )
  .await
  .unwrap();

  assert_server_collab(
    workspace_id,
    &mut new_client.api_client,
    object_id,
    &collab_type,
    60,
    json!({
      "1": "a",
      "2": "b",
    }),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn same_device_multiple_connect_in_order_test() {
  let collab_type = CollabType::Unknown;
  let mut old_client = TestClient::new_user().await;
  let workspace_id = old_client.workspace_id().await;

  let object_id = old_client
    .create_and_edit_collab(workspace_id, collab_type)
    .await;
  // simulate client try to connect the websocket server by three times
  // each connect alter the document
  for i in 0..3 {
    let mut new_client =
      TestClient::new_with_device_id(&old_client.device_id, old_client.user.clone(), true).await;
    new_client
      .open_collab(workspace_id, object_id, collab_type)
      .await;
    new_client.insert_into(&object_id, &i.to_string(), i).await;
    sleep(Duration::from_millis(500)).await;
    new_client
      .wait_object_sync_complete(&object_id)
      .await
      .unwrap();
  }

  assert_server_collab(
    workspace_id,
    &mut old_client.api_client,
    object_id,
    &collab_type,
    10,
    json!({"0":0,"1":1,"2":2}),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn two_direction_peer_sync_test() {
  let collab_type = CollabType::Unknown;

  let mut client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(workspace_id, collab_type)
    .await;

  let mut client_2 = TestClient::new_user().await;
  // Before the client_2 want to edit the collab object, it needs to become a member of the collab
  // Otherwise, the server will reject the edit request
  client_1
    .invite_and_accepted_workspace_member(&workspace_id, &client_2, AFRole::Member)
    .await
    .unwrap();

  client_2
    .open_collab(workspace_id, object_id, collab_type)
    .await;

  client_1.insert_into(&object_id, "name", "AppFlowy").await;
  client_1
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

  client_2
    .insert_into(
      &object_id,
      "support platform",
      "macOS, Windows, Linux, iOS, Android",
    )
    .await;
  client_2
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

  let expected_json = json!({
    "name": "AppFlowy",
    "support platform": "macOS, Windows, Linux, iOS, Android"
  });
  assert_client_collab_include_value(&mut client_1, &object_id, expected_json.clone())
    .await
    .unwrap();
  assert_client_collab_include_value(&mut client_2, &object_id, expected_json.clone())
    .await
    .unwrap();
}

#[tokio::test]
async fn multiple_collab_edit_test() {
  let collab_type = CollabType::Unknown;
  let mut client_1 = TestClient::new_user().await;
  let workspace_id_1 = client_1.workspace_id().await;
  let object_id_1 = client_1
    .create_and_edit_collab(workspace_id_1, collab_type)
    .await;

  let mut client_2 = TestClient::new_user().await;
  let workspace_id_2 = client_2.workspace_id().await;
  let object_id_2 = client_2
    .create_and_edit_collab(workspace_id_2, collab_type)
    .await;

  client_1
    .insert_into(&object_id_1, "title", "I am client 1")
    .await;
  client_1
    .wait_object_sync_complete(&object_id_1)
    .await
    .unwrap();

  client_2
    .insert_into(&object_id_2, "title", "I am client 2")
    .await;
  client_2
    .wait_object_sync_complete(&object_id_2)
    .await
    .unwrap();

  assert_server_collab(
    workspace_id_1,
    &mut client_1.api_client,
    object_id_1,
    &collab_type,
    10,
    json!( {
      "title": "I am client 1"
    }),
  )
  .await
  .unwrap();

  assert_server_collab(
    workspace_id_2,
    &mut client_2.api_client,
    object_id_2,
    &collab_type,
    10,
    json!( {
      "title": "I am client 2"
    }),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn simulate_multiple_user_edit_collab_test() {
  let mut tasks = Vec::new();
  for _i in 0..5 {
    let task = tokio::spawn(async move {
      let mut new_user = TestClient::new_user().await;
      let collab_type = CollabType::Unknown;
      let workspace_id = new_user.workspace_id().await;
      let object_id = Uuid::new_v4();

      new_user
        .open_collab(workspace_id, object_id, collab_type)
        .await;

      let random_str = generate_random_string(200);
      new_user
        .insert_into(&object_id, "string", random_str.clone())
        .await;
      let expected_json = json!({
        "string": random_str
      });

      new_user
        .wait_object_sync_complete(&object_id)
        .await
        .unwrap();

      let json = (*new_user
        .collabs
        .get(&object_id)
        .unwrap()
        .collab
        .read()
        .await)
        .to_json_value();

      (expected_json, json)
    });
    tasks.push(task);
  }

  let results = futures::future::join_all(tasks).await;
  for result in results {
    let (expected_json, json) = result.unwrap();
    assert_json_eq!(expected_json, json);
  }
}

#[tokio::test]
async fn post_realtime_message_test() {
  let mut tasks = Vec::new();
  let big_text = generate_random_string(64 * 1024);

  for _i in 0..5 {
    let cloned_text = big_text.clone();
    let task = tokio::spawn(async move {
      let mut new_user = TestClient::new_user().await;
      // sleep 2 secs to make sure it do not trigger register user too fast in gotrue
      sleep(Duration::from_secs(2)).await;

      let object_id = Uuid::new_v4();
      let workspace_id = new_user.workspace_id().await;
      let doc_state = make_collab_with_key_value(&object_id, "text", cloned_text);
      // the big doc_state will force the init_sync using the http request.
      // It will trigger the POST_REALTIME_MESSAGE_STREAM_HANDLER to handle the request.
      new_user
        .open_collab_with_doc_state(workspace_id, object_id, CollabType::Unknown, doc_state)
        .await;

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
      workspace_id,
      &mut client.api_client,
      object_id,
      &CollabType::Document,
      10,
      json!({
        "text": big_text
      }),
    )
    .await
    .unwrap();

    drop(client);
  }
}

#[tokio::test]
async fn post_realtime_message_without_ws_connect_test() {
  let client = Arc::new(TestClient::new_user_without_ws_conn().await);
  let mut handles = vec![];

  // try to post 10 realtime message without connect to the websocket server.
  for _ in 0..10 {
    let cloned_client = client.clone();
    let handle = tokio::spawn(async move {
      let message = RealtimeMessage::Collab(CollabMessage::ClientUpdateSync(UpdateSync::new(
        CollabOrigin::Empty,
        uuid::Uuid::new_v4().to_string(),
        generate_random_bytes(1024),
        1,
      )))
      .encode()
      .unwrap();
      cloned_client.post_realtime_binary(message).await.unwrap();
    });
    handles.push(handle);
  }
  for result in futures::future::join_all(handles).await {
    result.unwrap();
  }
}

#[tokio::test]
async fn post_realtime_message_with_ws_connect_test() {
  let client = Arc::new(TestClient::new_user().await);
  let message = RealtimeMessage::Collab(CollabMessage::ClientUpdateSync(UpdateSync::new(
    CollabOrigin::Empty,
    uuid::Uuid::new_v4().to_string(),
    generate_random_bytes(1024),
    1,
  )))
  .encode()
  .unwrap();
  client.post_realtime_binary(message).await.unwrap();
}

#[tokio::test]
async fn simulate_5_offline_user_connect_and_then_sync_document_test() {
  let text = generate_random_string(1024);
  let mut tasks = Vec::new();
  for i in 0..5 {
    let cloned_text = text.clone();
    let task = tokio::spawn(async move {
      let mut new_user = TestClient::new_user_without_ws_conn().await;
      // sleep to make sure it do not trigger register user too fast in gotrue
      sleep(Duration::from_secs(i % 5)).await;

      let object_id = Uuid::new_v4();
      let workspace_id = new_user.workspace_id().await;
      let doc_state = make_collab_with_key_value(&object_id, "text", cloned_text);
      new_user
        .open_collab_with_doc_state(workspace_id, object_id, CollabType::Unknown, doc_state)
        .await;
      (new_user, object_id)
    });
    tasks.push(task);
  }

  let results = futures::future::join_all(tasks).await;
  let mut tasks = Vec::new();
  for result in results.into_iter() {
    let task = tokio::spawn(async move {
      let (client, object_id) = result.unwrap();
      client.reconnect().await;
      client.wait_object_sync_complete(&object_id).await.unwrap();

      for i in 0..100 {
        client
          .insert_into(&object_id, &i.to_string(), i.to_string())
          .await;
        sleep(Duration::from_millis(60)).await;
      }
      client.wait_object_sync_complete(&object_id).await.unwrap();
    });
    tasks.push(task);
  }
  let results = futures::future::join_all(tasks).await;
  for result in results {
    result.unwrap()
  }
}

#[tokio::test]
async fn offline_and_then_sync_through_http_request() {
  let mut test_client = TestClient::new_user().await;
  let object_id = Uuid::new_v4();
  let workspace_id = test_client.workspace_id().await;
  let doc_state = make_collab_with_key_value(&object_id, "1", "".to_string());
  test_client
    .open_collab_with_doc_state(workspace_id, object_id, CollabType::Unknown, doc_state)
    .await;

  // Verify server hasn't received small text update while offline
  assert_server_collab(
    workspace_id,
    &mut test_client.api_client,
    object_id,
    &CollabType::Unknown,
    10,
    json!({"1":""}),
  )
  .await
  .unwrap();

  test_client.disconnect().await;

  // First insertion - small text
  let small_text = generate_random_string(100);
  test_client
    .insert_into(&object_id, "1", small_text.clone())
    .await;

  // Sync small text changes
  let encode_collab = test_client
    .collabs
    .get(&object_id)
    .unwrap()
    .encode_collab()
    .await;
  test_client
    .api_client
    .collab_full_sync(
      &workspace_id,
      &object_id,
      CollabType::Unknown,
      encode_collab.doc_state.to_vec(),
      encode_collab.state_vector.to_vec(),
    )
    .await
    .unwrap();

  // Verify server still has only small text
  assert_server_collab(
    workspace_id,
    &mut test_client.api_client,
    object_id,
    &CollabType::Unknown,
    10,
    json!({"1": small_text.clone()}),
  )
  .await
  .unwrap();

  // Second insertion - medium text
  let medium_text = generate_random_string(200);
  test_client
    .insert_into(&object_id, "2", medium_text.clone())
    .await;

  // Sync medium text changes
  let encode_collab = test_client
    .collabs
    .get(&object_id)
    .unwrap()
    .encode_collab()
    .await;
  test_client
    .api_client
    .collab_full_sync(
      &workspace_id,
      &object_id,
      CollabType::Unknown,
      encode_collab.doc_state.to_vec(),
      encode_collab.state_vector.to_vec(),
    )
    .await
    .unwrap();

  assert_client_collab_value(
    &mut test_client,
    &object_id,
    json!({"1": small_text, "2": medium_text}),
  )
  .await
  .unwrap();

  // Verify medium text was synced
  assert_server_collab(
    workspace_id,
    &mut test_client.api_client,
    object_id,
    &CollabType::Unknown,
    10,
    json!({"1": small_text, "2": medium_text}),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn insert_text_through_http_post_request() {
  let mut test_client = TestClient::new_user().await;
  let object_id = Uuid::new_v4();
  let workspace_id = test_client.workspace_id().await;
  let doc_state = make_collab_with_key_value(&object_id, "1", "".to_string());
  test_client
    .open_collab_with_doc_state(workspace_id, object_id, CollabType::Unknown, doc_state)
    .await;
  test_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();
  test_client.disconnect().await;

  let mut final_text = HashMap::new();
  for i in 0..1000 {
    let key = i.to_string();
    let text = generate_random_string(10);
    test_client
      .insert_into(&object_id, &key, text.clone())
      .await;
    final_text.insert(key, text);
    if i % 100 == 0 {
      tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    }
  }

  let encode_collab = test_client
    .collabs
    .get(&object_id)
    .unwrap()
    .encode_collab()
    .await;
  test_client
    .api_client
    .collab_full_sync(
      &workspace_id,
      &object_id,
      CollabType::Unknown,
      encode_collab.doc_state.to_vec(),
      encode_collab.state_vector.to_vec(),
    )
    .await
    .unwrap();

  assert_server_collab(
    workspace_id,
    &mut test_client.api_client,
    object_id,
    &CollabType::Unknown,
    10,
    json!(final_text),
  )
  .await
  .unwrap();
}
