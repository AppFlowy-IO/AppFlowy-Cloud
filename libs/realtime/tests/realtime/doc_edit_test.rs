use crate::realtime::script::CollabTest;
use serde_json::json;

#[actix_rt::test]
async fn single_client_write_collab_test() {
  let mut test = CollabTest::new().await;
  test.create_client(0).await;
  test.open_object(0, "1").await;
  test
    .modify_object(0, "1", |collab| {
      collab.insert("1", "a");
    })
    .await;
  test.wait(1).await;

  let client = test.get_client_doc(0, "1").await;
  let server = test.get_server_doc("1").await;

  assert_json_diff::assert_json_eq!(
    client.clone(),
    json!({
      "1": "a"
    })
  );

  assert_eq!(client, server);
  assert_json_diff::assert_json_eq!(client, server);
}

#[actix_rt::test]
async fn client_receive_changes_test() {
  let mut test = CollabTest::new().await;
  test.create_client(0).await;
  test.create_client(1).await;
  test.open_object(0, "1").await;
  test.open_object(1, "1").await;
  test
    .modify_object(0, "1", |collab| {
      collab.insert("1", "a");
    })
    .await;
  test.wait(2).await;

  let client_1 = test.get_client_doc(0, "1").await;
  let client_2 = test.get_client_doc(1, "1").await;
  let server = test.get_server_doc("1").await;

  assert_eq!(client_1, client_2);
  assert_eq!(client_1, server);
  assert_json_diff::assert_json_eq!(client_1, server);
}

#[actix_rt::test]
async fn two_client_write_same_collab_test() {
  let mut test = CollabTest::new().await;
  test.create_client(0).await;
  test.create_client(1).await;
  test.open_object(0, "1").await;
  test.open_object(1, "1").await;
  test
    .modify_object(0, "1", |collab| {
      collab.insert("1", "a");
    })
    .await;
  test.wait(1).await;

  test
    .modify_object(1, "1", |collab| {
      collab.insert("2", "b");
    })
    .await;
  test.wait(1).await;

  let client_1 = test.get_client_doc(0, "1").await;
  let client_2 = test.get_client_doc(1, "1").await;
  let server = test.get_server_doc("1").await;

  assert_json_diff::assert_json_eq!(
    client_1,
    json!({
      "1": "a",
      "2": "b"
    })
  );
  assert_eq!(client_1, client_2);
  assert_eq!(client_1, server);
  assert_json_diff::assert_json_eq!(client_1, server);
}

#[actix_rt::test]
async fn two_client_write_two_collab_test() {
  let mut test = CollabTest::new().await;
  test.create_client(0).await;
  test.create_client(1).await;
  test.open_object(0, "1").await;
  test.open_object(1, "2").await;
  test
    .modify_object(0, "1", |collab| {
      collab.insert("1", "a");
    })
    .await;
  test.wait(1).await;

  test
    .modify_object(1, "2", |collab| {
      collab.insert("2", "b");
    })
    .await;
  test.wait(1).await;

  let client_1 = test.get_client_doc(0, "1").await;
  let server = test.get_server_doc("1").await;
  assert_eq!(client_1, server);

  assert_json_diff::assert_json_eq!(
    client_1,
    json!({
      "1": "a",
    })
  );

  let client_2 = test.get_client_doc(1, "2").await;
  let server = test.get_server_doc("2").await;
  assert_eq!(client_2, server);
  assert_json_diff::assert_json_eq!(
    client_2,
    json!({
      "2": "b",
    })
  );
}
