use crate::realtime::script::{ScriptTest, TestScript::*};
use serde_json::json;

#[actix_rt::test]
async fn single_client_connect_test() {
  let mut test = ScriptTest::new().await;
  test
    .run_scripts(vec![
      CreateClient { uid: 0 },
      OpenObject {
        uid: 0,
        object_id: "1".to_string(),
      },
      ModifyClientCollab {
        uid: 0,
        object_id: "1".to_string(),
        f: |collab| {
          collab.insert("1", "a");
        },
      },
      Wait { secs: 1 },
      AssertClientContent {
        uid: 0,
        object_id: "1".to_string(),
        expected: json!({
          "1": "a"
        }),
      },
      AssertClientEqualToServer {
        uid: 0,
        object_id: "1".to_string(),
      },
    ])
    .await;
}

#[actix_rt::test]
async fn client_single_write_test() {
  let mut test = ScriptTest::new().await;
  test
    .run_scripts(vec![
      CreateClient { uid: 0 },
      CreateClient { uid: 1 },
      OpenObject {
        uid: 0,
        object_id: "1".to_string(),
      },
      OpenObject {
        uid: 1,
        object_id: "1".to_string(),
      },
      ModifyClientCollab {
        uid: 0,
        object_id: "1".to_string(),
        f: |collab| {
          collab.insert("1", "a");
        },
      },
      Wait { secs: 2 },
      AssertClientEqualToServer {
        uid: 0,
        object_id: "1".to_string(),
      },
      AssertClientEqualToServer {
        uid: 1,
        object_id: "1".to_string(),
      },
    ])
    .await;
}

#[actix_rt::test]
async fn client_multiple_write_test() {
  let mut test = ScriptTest::new().await;
  test
    .run_scripts(vec![
      CreateClient { uid: 0 },
      CreateClient { uid: 1 },
      OpenObject {
        uid: 0,
        object_id: "1".to_string(),
      },
      OpenObject {
        uid: 1,
        object_id: "1".to_string(),
      },
      ModifyClientCollab {
        uid: 0,
        object_id: "1".to_string(),
        f: |collab| {
          collab.insert("1", "a");
        },
      },
      Wait { secs: 1 },
      ModifyClientCollab {
        uid: 1,
        object_id: "1".to_string(),
        f: |collab| {
          collab.insert("2", "b");
        },
      },
      Wait { secs: 1 },
      AssertServerContent {
        object_id: "1".to_string(),
        expected: json!({
          "1": "a",
          "2": "b"
        }),
      },
      AssertClientEqualToServer {
        uid: 0,
        object_id: "1".to_string(),
      },
      AssertClientEqualToServer {
        uid: 1,
        object_id: "1".to_string(),
      },
    ])
    .await;
}
