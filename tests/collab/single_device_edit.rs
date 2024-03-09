use crate::collab::util::{generate_random_string, make_big_collab_doc_state};
use assert_json_diff::assert_json_eq;
use client_api_test_util::*;
use collab_entity::CollabType;
use database_entity::dto::AFAccessLevel;
use serde_json::json;

use std::time::Duration;
use tokio::time::sleep;

use realtime_entity::message::MAXIMUM_REALTIME_MESSAGE_SIZE;
use uuid::Uuid;

#[tokio::test]
async fn collab_write_small_chunk_of_data_test() {
  let collab_type = CollabType::Document;
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4().to_string();

  // Calling the open_collab function directly will create the collab object in the plugin.
  // The [CollabStoragePlugin] plugin try to get the collab object from the database, but it doesn't exist.
  // So the plugin will create the collab object.
  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  // Edit the collab
  for i in 0..=5 {
    test_client
      .collabs
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert(&i.to_string(), i.to_string());
  }
  test_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();
  test_client.disconnect().await;

  assert_server_collab(
    &workspace_id,
    &mut test_client.api_client,
    &object_id,
    &collab_type,
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

#[tokio::test]
async fn collab_write_big_chunk_of_data_test() {
  let collab_type = CollabType::Document;
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4().to_string();

  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  let s = generate_random_string(10000);
  test_client
    .collabs
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("big_text", s.clone());

  test_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();
  assert_server_collab(
    &workspace_id,
    &mut test_client.api_client,
    &object_id,
    &collab_type,
    10,
    json!({
      "big_text": s
    }),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn write_big_chunk_data_init_sync_test() {
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4().to_string();
  let big_text = generate_random_string((MAXIMUM_REALTIME_MESSAGE_SIZE / 2) as usize);
  let collab_type = CollabType::Document;
  let doc_state = make_big_collab_doc_state(&object_id, "big_text", big_text.clone());

  // the big doc_state will force the init_sync using the http request.
  // It will trigger the POST_REALTIME_MESSAGE_STREAM_HANDLER to handle the request.
  test_client
    .open_collab_with_doc_state(&workspace_id, &object_id, collab_type.clone(), doc_state)
    .await;
  test_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

  assert_server_collab(
    &workspace_id,
    &mut test_client.api_client,
    &object_id,
    &collab_type,
    10,
    json!({
      "big_text": big_text
    }),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn realtime_write_single_collab_test() {
  let collab_type = CollabType::Document;
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = test_client
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;
  test_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  // Edit the collab
  for i in 0..=5 {
    test_client
      .collabs
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert(&i.to_string(), i.to_string());
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
    &workspace_id,
    &mut test_client.api_client,
    &object_id,
    &collab_type,
    10,
    expected_json,
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
    let collab_type = CollabType::Document;

    let object_id = test_client
      .create_and_edit_collab(&workspace_id, collab_type.clone())
      .await;

    test_client
      .open_collab(&workspace_id, &object_id, collab_type.clone())
      .await;
    for i in 0..=5 {
      test_client
        .collabs
        .get_mut(&object_id)
        .unwrap()
        .collab
        .lock()
        .insert(&i.to_string(), i.to_string());
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
      &workspace_id,
      &mut test_client.api_client,
      &object_id,
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
  let collab_type = CollabType::Document;
  let mut client = TestClient::new_user().await;
  let workspace_id = client.workspace_id().await;

  let object_id = client
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  client
    .collabs
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("1", "a");

  // Sleep one second for the doc observer the update. Otherwise, the
  // sync complete might be called before the update being schedule
  sleep(Duration::from_secs(1)).await;
  client.wait_object_sync_complete(&object_id).await.unwrap();

  // the new_client connect with same device_id, so it will replace the existing client
  // in the server. Which means the old client will not receive updates.
  let mut new_client =
    TestClient::new_with_device_id(&client.device_id, client.user.clone(), true).await;
  new_client
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;
  new_client
    .collabs
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("2", "b");
  new_client
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

  assert_client_collab_include_value_within_30_secs(
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
    &workspace_id,
    &mut new_client.api_client,
    &object_id,
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
  let collab_type = CollabType::Document;
  let mut old_client = TestClient::new_user().await;
  let workspace_id = old_client.workspace_id().await;

  let object_id = old_client
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;
  // simulate client try to connect the websocket server by three times
  // each connect alter the document
  for i in 0..3 {
    let mut new_client =
      TestClient::new_with_device_id(&old_client.device_id, old_client.user.clone(), true).await;
    new_client
      .open_collab(&workspace_id, &object_id, collab_type.clone())
      .await;
    new_client
      .collabs
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert(&i.to_string(), i);
    sleep(Duration::from_millis(500)).await;
    new_client
      .wait_object_sync_complete(&object_id)
      .await
      .unwrap();
  }

  assert_server_collab(
    &workspace_id,
    &mut old_client.api_client,
    &object_id,
    &collab_type,
    10,
    json!({"0":0.0,"1":1.0,"2":2.0}),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn two_direction_peer_sync_test() {
  let collab_type = CollabType::Document;

  let mut client_1 = TestClient::new_user().await;
  let workspace_id = client_1.workspace_id().await;
  let object_id = client_1
    .create_and_edit_collab(&workspace_id, collab_type.clone())
    .await;

  let mut client_2 = TestClient::new_user().await;
  // Before the client_2 want to edit the collab object, it needs to become a member of the collab
  // Otherwise, the server will reject the edit request
  client_1
    .add_collab_member(
      &workspace_id,
      &object_id,
      &client_2,
      AFAccessLevel::FullAccess,
    )
    .await;

  client_2
    .open_collab(&workspace_id, &object_id, collab_type.clone())
    .await;

  client_1
    .collabs
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("name", "AppFlowy");
  client_1
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

  client_2
    .collabs
    .get_mut(&object_id)
    .unwrap()
    .collab
    .lock()
    .insert("support platform", "macOS, Windows, Linux, iOS, Android");
  client_2
    .wait_object_sync_complete(&object_id)
    .await
    .unwrap();

  let expected_json = json!({
    "name": "AppFlowy",
    "support platform": "macOS, Windows, Linux, iOS, Android"
  });
  assert_client_collab_include_value_within_30_secs(
    &mut client_1,
    &object_id,
    expected_json.clone(),
  )
  .await
  .unwrap();
  assert_client_collab_include_value_within_30_secs(
    &mut client_2,
    &object_id,
    expected_json.clone(),
  )
  .await
  .unwrap();
}

#[tokio::test]
async fn multiple_collab_edit_test() {
  let collab_type = CollabType::Document;
  let mut client_1 = TestClient::new_user().await;
  let workspace_id_1 = client_1.workspace_id().await;
  let object_id_1 = client_1
    .create_and_edit_collab(&workspace_id_1, collab_type.clone())
    .await;
  client_1
    .open_collab(&workspace_id_1, &object_id_1, collab_type.clone())
    .await;

  let mut client_2 = TestClient::new_user().await;
  let workspace_id_2 = client_2.workspace_id().await;
  let object_id_2 = client_2
    .create_and_edit_collab(&workspace_id_2, collab_type.clone())
    .await;
  client_2
    .open_collab(&workspace_id_2, &object_id_2, collab_type.clone())
    .await;

  client_1
    .collabs
    .get_mut(&object_id_1)
    .unwrap()
    .collab
    .lock()
    .insert("title", "I am client 1");
  client_1
    .wait_object_sync_complete(&object_id_1)
    .await
    .unwrap();

  client_2
    .collabs
    .get_mut(&object_id_2)
    .unwrap()
    .collab
    .lock()
    .insert("title", "I am client 2");
  client_2
    .wait_object_sync_complete(&object_id_2)
    .await
    .unwrap();

  assert_server_collab(
    &workspace_id_1,
    &mut client_1.api_client,
    &object_id_1,
    &collab_type,
    10,
    json!( {
      "title": "I am client 1"
    }),
  )
  .await
  .unwrap();

  assert_server_collab(
    &workspace_id_2,
    &mut client_2.api_client,
    &object_id_2,
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
  for _i in 0..20 {
    let task = tokio::spawn(async move {
      let mut new_user = TestClient::new_user().await;
      let collab_type = CollabType::Document;
      let workspace_id = new_user.workspace_id().await;
      let object_id = Uuid::new_v4().to_string();

      new_user
        .open_collab(&workspace_id, &object_id, collab_type.clone())
        .await;

      let random_str = generate_random_string(200);
      new_user
        .collabs
        .get_mut(&object_id)
        .unwrap()
        .collab
        .lock()
        .insert("string", random_str.clone());
      let expected_json = json!({
        "string": random_str
      });

      new_user
        .wait_object_sync_complete(&object_id)
        .await
        .unwrap();
      (
        expected_json,
        new_user
          .collabs
          .get(&object_id)
          .unwrap()
          .collab
          .to_json_value(),
      )
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
  let big_text = generate_random_string((MAXIMUM_REALTIME_MESSAGE_SIZE / 2) as usize);

  for i in 0..20 {
    let cloned_text = big_text.clone();
    let task = tokio::spawn(async move {
      let mut new_user = TestClient::new_user().await;
      // sleep 2 secs to make sure it do not trigger register user too fast in gotrue
      sleep(Duration::from_secs(i % 5)).await;

      let object_id = Uuid::new_v4().to_string();
      let workspace_id = new_user.workspace_id().await;
      let doc_state = make_big_collab_doc_state(&object_id, "text", cloned_text);
      // the big doc_state will force the init_sync using the http request.
      // It will trigger the POST_REALTIME_MESSAGE_STREAM_HANDLER to handle the request.
      new_user
        .open_collab_with_doc_state(&workspace_id, &object_id, CollabType::Document, doc_state)
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
      &workspace_id,
      &mut client.api_client,
      &object_id,
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
async fn collab_flush_test() {
  let mut new_user = TestClient::new_user().await;
  let object_id = Uuid::new_v4().to_string();
  let workspace_id = new_user.workspace_id().await;
  new_user
    .open_collab(&workspace_id, &object_id, CollabType::Document)
    .await;

  // the default flush_per_update is 100 that defined in [WriteConfig]
  // so we need to write 200 times to trigger the flush
  for i in 0..200 {
    new_user
      .collabs
      .get_mut(&object_id)
      .unwrap()
      .collab
      .lock()
      .insert(&i.to_string(), i.to_string());
    sleep(Duration::from_millis(300)).await;
  }
  // TODO(nathan): assert the collab content in disk
}

#[tokio::test]
async fn simulate_50_offline_user_connect_and_then_sync_document_test() {
  let text = generate_random_string(1024 * 1024 * 3);
  let mut tasks = Vec::new();
  for i in 0..50 {
    let cloned_text = text.clone();
    let task = tokio::spawn(async move {
      let mut new_user = TestClient::new_user_without_ws_conn().await;
      // sleep to make sure it do not trigger register user too fast in gotrue
      sleep(Duration::from_secs(i % 5)).await;

      let object_id = Uuid::new_v4().to_string();
      let workspace_id = new_user.workspace_id().await;
      let doc_state = make_big_collab_doc_state(&object_id, "text", cloned_text);
      new_user
        .open_collab_with_doc_state(&workspace_id, &object_id, CollabType::Document, doc_state)
        .await;
      (new_user, object_id)
    });
    tasks.push(task);
  }

  let results = futures::future::join_all(tasks).await;
  let mut tasks = Vec::new();
  for result in results.into_iter() {
    let task = tokio::spawn(async move {
      let (mut client, object_id) = result.unwrap();
      client.reconnect().await;
      client.wait_object_sync_complete(&object_id).await.unwrap();

      for i in 0..100 {
        client
          .collabs
          .get_mut(&object_id)
          .unwrap()
          .collab
          .lock()
          .insert(&i.to_string(), i.to_string());
        sleep(Duration::from_millis(30)).await;
      }
    });
    tasks.push(task);
  }
  let _results = futures::future::join_all(tasks).await;
}
