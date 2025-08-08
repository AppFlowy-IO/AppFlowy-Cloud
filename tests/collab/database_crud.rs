use std::collections::HashMap;

use client_api_test::{generate_unique_registered_user_client, workspace_id_from_client};
use collab_database::entity::FieldType;
use serde_json::json;
use shared_entity::dto::workspace_dto::AFInsertDatabaseField;

#[tokio::test]
async fn database_row_upsert_with_doc() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let databases = c.list_databases(&workspace_id).await.unwrap();
  assert_eq!(databases.len(), 1);

  let todo_db = &databases[0];

  // Upsert row
  let row_id = c
    .upsert_database_item(
      &workspace_id,
      &todo_db.id,
      "my_pre_hash_123".to_string(),
      HashMap::from([]),
      Some("This is a document of a database row".to_string()),
    )
    .await
    .unwrap();

  {
    // Get row and check data
    let row_detail = &c
      .list_database_row_details(&workspace_id, &todo_db.id, &[&row_id], true)
      .await
      .unwrap()[0];
    assert!(row_detail.has_doc);
    assert_eq!(
      row_detail.doc,
      Some(String::from("This is a document of a database row"))
    );
    let row_uuid = uuid::Uuid::parse_str(&row_id).unwrap();
    let row_collab_doc_exists = &c
      .check_if_row_document_collab_exists(&workspace_id, &row_uuid)
      .await
      .unwrap();
    assert!(row_collab_doc_exists)
  }
  // Upsert row with another doc
  let _ = c
    .upsert_database_item(
      &workspace_id,
      &todo_db.id,
      "my_pre_hash_123".to_string(),
      HashMap::from([]),
      Some("This is a another document".to_string()),
    )
    .await
    .unwrap();
  {
    // Get row and check that doc has been modified
    let row_detail = &c
      .list_database_row_details(&workspace_id, &todo_db.id, &[&row_id], true)
      .await
      .unwrap()[0];
    assert_eq!(
      row_detail.doc,
      Some(String::from("This is a another document"))
    );
  }
}

#[tokio::test]
async fn database_row_upsert() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let databases = c.list_databases(&workspace_id).await.unwrap();
  assert_eq!(databases.len(), 1);

  let todo_db = &databases[0];

  // predefined string to be used to identify the row
  let pre_hash = String::from("my_id_123");

  // Upsert row
  let row_id = c
    .upsert_database_item(
      &workspace_id,
      &todo_db.id,
      pre_hash.clone(),
      HashMap::from([
        (String::from("Description"), json!("description_1")),
        (String::from("Status"), json!("To Do")),
        (String::from("Multiselect"), json!(["social", "news"])),
      ]),
      Some("".to_string()),
    )
    .await
    .unwrap();

  {
    // Get row and check data
    let row_detail = &c
      .list_database_row_details(&workspace_id, &todo_db.id, &[&row_id], false)
      .await
      .unwrap()[0];
    assert_eq!(row_detail.cells["Description"], "description_1");
    assert_eq!(row_detail.cells["Status"], "To Do");
    assert_eq!(row_detail.cells["Multiselect"][0], "social");
    assert_eq!(row_detail.cells["Multiselect"][1], "news");
    assert!(!row_detail.has_doc);
  }

  {
    // Upsert row again with different data, using same pre_hash
    // row_id return should be the same as previous
    let row_id_2 = c
      .upsert_database_item(
        &workspace_id,
        &todo_db.id,
        pre_hash,
        HashMap::from([
          (String::from("Description"), json!("description_2")),
          (String::from("Status"), json!("Doing")),
          (String::from("Multiselect"), json!(["fast", "self-host"])),
        ]),
        Some("This is a document of a database row".to_string()),
      )
      .await
      .unwrap();
    assert_eq!(row_id, row_id_2);
  }
  {
    // Get row and check data, it should be modified
    let row_detail = &c
      .list_database_row_details(&workspace_id, &todo_db.id, &[&row_id], true)
      .await
      .unwrap()[0];
    assert_eq!(row_detail.cells["Description"], "description_2");
    assert_eq!(row_detail.cells["Status"], "Doing");
    assert_eq!(row_detail.cells["Multiselect"][0], "fast");
    assert_eq!(row_detail.cells["Multiselect"][1], "self-host");
    assert!(row_detail.has_doc);
    assert_eq!(
      row_detail.doc,
      Some("This is a document of a database row".to_string())
    );
  }
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
      .list_database_row_details(&workspace_id, &todo_db.id, &[&new_row_id], false)
      .await
      .unwrap();
    assert_eq!(row_details.len(), 1);
    let new_row_detail = &row_details[0];
    println!(
      "Available keys in cells: {:?}",
      new_row_detail.cells.keys().collect::<Vec<_>>()
    );
    println!("Number of cells: {}", new_row_detail.cells.len());
    for (key, value) in &new_row_detail.cells {
      println!("Cell '{}': {:?}", key, value);
    }

    assert_eq!(new_row_detail.cells["Description"], my_description);
    assert_eq!(new_row_detail.cells["Status"], my_status);
    assert_eq!(new_row_detail.cells["Multiselect"][0], "social");
    assert_eq!(new_row_detail.cells["Multiselect"][1], "news");
    assert_eq!(new_row_detail.cells["MyNumberColumn"], "123");
    assert_eq!(
      new_row_detail.cells["MyDateTimeColumn"],
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
    assert_eq!(new_row_detail.cells["MyUrlField"], "https://appflowy.io");
    assert_eq!(new_row_detail.cells["MyCheckboxColumn"], true);
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
      .list_database_row_details(&workspace_id, &todo_db.id, &[&new_row_id], false)
      .await
      .unwrap();
    assert_eq!(row_details.len(), 1);
    let new_row_detail = &row_details[0];
    assert!(!new_row_detail.cells.contains_key("MyRelationCol"));
  }
}

#[tokio::test]
async fn database_insert_row_with_doc() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let databases = c.list_databases(&workspace_id).await.unwrap();
  assert_eq!(databases.len(), 1);
  let todo_db = &databases[0];

  let row_doc = "This is a document of a database row";
  let new_row_id = c
    .add_database_item(
      &workspace_id,
      &todo_db.id,
      HashMap::from([]),
      row_doc.to_string().into(),
    )
    .await
    .unwrap();

  let row_details = c
    .list_database_row_details(&workspace_id, &todo_db.id, &[&new_row_id], true)
    .await
    .unwrap();
  let row_detail = &row_details[0];
  assert!(row_detail.has_doc);
  assert_eq!(
    row_detail.doc,
    Some("This is a document of a database row".to_string())
  );
}
