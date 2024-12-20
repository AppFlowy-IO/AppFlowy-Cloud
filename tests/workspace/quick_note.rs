use std::time::Duration;

use client_api_test::TestClient;
use serde_json::json;
use tokio::time;
use uuid::Uuid;

#[tokio::test]
async fn quick_note_crud_test() {
  let client = TestClient::new_user_without_ws_conn().await;
  let workspace_id = client.workspace_id().await;
  let workspace_uuid = Uuid::parse_str(&workspace_id).unwrap();
  let mut quick_note_ids: Vec<Uuid> = vec![];
  for _ in 0..2 {
    let quick_note = client
      .api_client
      .create_quick_note(workspace_uuid, None)
      .await
      .expect("create quick note");
    quick_note_ids.push(quick_note.id);
    // To ensure that the creation time is different
    time::sleep(Duration::from_millis(1)).await;
  }
  let quick_notes = client
    .api_client
    .list_quick_notes(workspace_uuid, None, None, None)
    .await
    .expect("list quick notes");
  assert_eq!(quick_notes.quick_notes.len(), 2);
  assert!(!quick_notes.has_more);

  let quick_note_id_1 = quick_note_ids[0];
  let quick_note_id_2 = quick_note_ids[1];
  let data_1 = json!([
    {
      "type": "paragraph",
      "delta": {
        "insert": "orange",
        "attributes": {
          "bold": true
        },
      },
    },
    {
      "type": "heading",
      "data": {
        "level": 1
      },
      "delta": {
        "insert": "apple",
        "attributes": {
          "bold": true
        },
      },
    },
  ]);
  let data_2 = json!([
    {
      "type": "paragraph",
      "delta": {
        "insert": "banana",
        "attributes": {
          "bold": true
        },
      },
    },
    {
      "type": "heading",
      "data": {
        "level": 1
      },
      "delta": {
        "insert": "melon",
        "attributes": {
          "bold": true
        },
      },
    },
  ]);
  client
    .api_client
    .update_quick_note(workspace_uuid, quick_note_id_1, data_1)
    .await
    .expect("update quick note");
  client
    .api_client
    .update_quick_note(workspace_uuid, quick_note_id_2, data_2)
    .await
    .expect("update quick note");
  let quick_notes = client
    .api_client
    .list_quick_notes(workspace_uuid, None, None, None)
    .await
    .expect("list quick notes");
  assert_eq!(quick_notes.quick_notes.len(), 2);
  let quick_notes = client
    .api_client
    .list_quick_notes(workspace_uuid, Some("".to_string()), None, None)
    .await
    .expect("list quick notes with empty search term");
  assert_eq!(quick_notes.quick_notes.len(), 2);
  let quick_notes_with_offset_and_limit = client
    .api_client
    .list_quick_notes(workspace_uuid, None, Some(1), Some(1))
    .await
    .expect("list quick notes with offset and limit");
  assert_eq!(quick_notes_with_offset_and_limit.quick_notes.len(), 1);
  assert!(!quick_notes_with_offset_and_limit.has_more);
  assert_eq!(
    quick_notes_with_offset_and_limit.quick_notes[0].id,
    quick_note_id_1
  );
  let quick_notes_with_offset_and_limit = client
    .api_client
    .list_quick_notes(workspace_uuid, None, Some(0), Some(1))
    .await
    .expect("list quick notes with offset and limit");
  assert_eq!(quick_notes_with_offset_and_limit.quick_notes.len(), 1);
  assert!(quick_notes_with_offset_and_limit.has_more);
  assert_eq!(
    quick_notes_with_offset_and_limit.quick_notes[0].id,
    quick_note_id_2
  );
  let filtered_quick_notes = client
    .api_client
    .list_quick_notes(workspace_uuid, Some("pple".to_string()), None, None)
    .await
    .expect("list quick notes with filter");
  assert_eq!(filtered_quick_notes.quick_notes.len(), 1);
  assert_eq!(filtered_quick_notes.quick_notes[0].id, quick_note_id_1);
  client
    .api_client
    .delete_quick_note(workspace_uuid, quick_note_id_1)
    .await
    .expect("delete quick note");
  let quick_notes = client
    .api_client
    .list_quick_notes(workspace_uuid, None, None, None)
    .await
    .expect("list quick notes");
  assert_eq!(quick_notes.quick_notes.len(), 1);
  assert_eq!(quick_notes.quick_notes[0].id, quick_note_id_2);
}
