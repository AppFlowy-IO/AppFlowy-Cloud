use std::collections::HashMap;

use client_api_test::{generate_unique_registered_user_client, workspace_id_from_client};
use collab_database::entity::FieldType;
use serde_json::json;
use shared_entity::dto::workspace_dto::AFInsertDatabaseField;

#[tokio::test]
async fn database_row_upsert() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let databases = c.list_databases(&workspace_id).await.unwrap();
  assert_eq!(databases.len(), 1);

  let todo_db = &databases[0];
  _ = todo_db;
  // TODO: test for upserting a row
  // c.upsert_database_item
}

#[tokio::test]
async fn database_fields_crud() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let databases = c.list_databases(&workspace_id).await.unwrap();
  assert_eq!(databases.len(), 1);
  let todo_db = &databases[0];

  let my_num_field_id = {
    c.add_database_field(
      &workspace_id,
      &todo_db.id,
      &AFInsertDatabaseField {
        name: "MyNumberColumn".to_string(),
        field_type: FieldType::Number.into(),
        ..Default::default()
      },
    )
    .await
    .unwrap()
  };
  let my_datetime_field_id = {
    c.add_database_field(
      &workspace_id,
      &todo_db.id,
      &AFInsertDatabaseField {
        name: "MyDateTimeColumn".to_string(),
        field_type: FieldType::DateTime.into(),
        ..Default::default()
      },
    )
    .await
    .unwrap()
  };
  let my_url_field_id = {
    c.add_database_field(
      &workspace_id,
      &todo_db.id,
      &AFInsertDatabaseField {
        name: "MyUrlField".to_string(),
        field_type: FieldType::URL.into(),
        ..Default::default()
      },
    )
    .await
    .unwrap()
  };
  let my_checkbox_field_id = {
    c.add_database_field(
      &workspace_id,
      &todo_db.id,
      &AFInsertDatabaseField {
        name: "MyCheckboxColumn".to_string(),
        field_type: FieldType::Checkbox.into(),
        ..Default::default()
      },
    )
    .await
    .unwrap()
  };
  {
    let my_description = "my task 123";
    let my_status = "To Do";
    let new_row_id = c
      .add_database_item(
        &workspace_id,
        &todo_db.id,
        HashMap::from([
          (String::from("Description"), json!(my_description)),
          (String::from("Status"), json!(my_status)),
          (String::from("Multiselect"), json!(["social", "news"])),
          (my_num_field_id, json!(123)),
          (my_datetime_field_id, json!(1733210221)),
          (my_url_field_id, json!("https://appflowy.io")),
          (my_checkbox_field_id, json!(true)),
        ]),
        None,
      )
      .await
      .unwrap();

    let row_details = c
      .list_database_row_details(&workspace_id, &todo_db.id, &[&new_row_id])
      .await
      .unwrap();
    assert_eq!(row_details.len(), 1);
    let new_row_detail = &row_details[0];
    assert_eq!(new_row_detail.cells["Description"]["data"], my_description);
    assert_eq!(new_row_detail.cells["Status"]["data"], my_status);
    assert_eq!(new_row_detail.cells["Multiselect"]["data"][0], "social");
    assert_eq!(new_row_detail.cells["Multiselect"]["data"][1], "news");
    assert_eq!(new_row_detail.cells["MyNumberColumn"]["data"], "123");
    assert_eq!(
      new_row_detail.cells["MyDateTimeColumn"]["data"],
      json!({
        "end": serde_json::Value::Null,
        "pretty_end_date": serde_json::Value::Null,
        "pretty_end_datetime": serde_json::Value::Null,
        "pretty_end_time": serde_json::Value::Null,
        "pretty_start_date": "2024-12-03",
        "pretty_start_datetime": "2024-12-03 07:17:01 UTC",
        "pretty_start_time": "07:17:01",
        "start": "2024-12-03T07:17:01+00:00",
        "timezone": "UTC",
      }),
    );
    assert_eq!(
      new_row_detail.cells["MyUrlField"]["data"],
      "https://appflowy.io"
    );
    assert_eq!(new_row_detail.cells["MyCheckboxColumn"]["data"], true);
  }
}

#[tokio::test]
async fn database_fields_unsupported_field_type() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let databases = c.list_databases(&workspace_id).await.unwrap();
  assert_eq!(databases.len(), 1);
  let todo_db = &databases[0];

  let my_rel_field_id = {
    c.add_database_field(
      &workspace_id,
      &todo_db.id,
      &AFInsertDatabaseField {
        name: "MyRelationCol".to_string(),
        field_type: FieldType::Relation.into(),
        ..Default::default()
      },
    )
    .await
    .unwrap()
  };
  {
    let my_description = "my task 123";
    let my_status = "To Do";
    let new_row_id = c
      .add_database_item(
        &workspace_id,
        &todo_db.id,
        HashMap::from([
          (String::from("Description"), json!(my_description)),
          (String::from("Status"), json!(my_status)),
          (String::from("Multiselect"), json!(["social", "news"])),
          (my_rel_field_id, json!("relation_data")),
        ]),
        None,
      )
      .await
      .unwrap();

    let row_details = c
      .list_database_row_details(&workspace_id, &todo_db.id, &[&new_row_id])
      .await
      .unwrap();
    assert_eq!(row_details.len(), 1);
    let new_row_detail = &row_details[0];
    assert!(!new_row_detail.cells.contains_key("MyRelationCol"));
  }
}
