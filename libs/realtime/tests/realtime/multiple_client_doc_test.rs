use crate::realtime::script::{ScriptTest, TestScript::*};
use serde_json::json;

#[actix_rt::test]
async fn client_with_multiple_objects_test() {
  let mut test = ScriptTest::new().await;
  test
    .run_scripts(vec![
      CreateClient { uid: 0 },
      OpenObject {
        uid: 0,
        object_id: "1".to_string(),
      },
      OpenObject {
        uid: 0,
        object_id: "2".to_string(),
      },
      ModifyClientCollab {
        uid: 0,
        object_id: "1".to_string(),
        f: |collab| {
          collab.insert("1", "a");
        },
      },
      ModifyClientCollab {
        uid: 0,
        object_id: "2".to_string(),
        f: |collab| {
          collab.insert("2", "b");
        },
      },
      Wait { secs: 2 },
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
      AssertClientContent {
        uid: 0,
        object_id: "2".to_string(),
        expected: json!({
          "2": "b"
        }),
      },
      AssertClientEqualToServer {
        uid: 0,
        object_id: "2".to_string(),
      },
    ])
    .await;
}
