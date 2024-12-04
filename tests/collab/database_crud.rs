use client_api_test::{generate_unique_registered_user_client, workspace_id_from_client};
use collab_database::entity::FieldType;
use serde_json::json;
use shared_entity::dto::workspace_dto::AFInsertDatabaseField;

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
        &serde_json::json!({
            "Description": my_description,
            "Status": my_status,
            "Multiselect": ["social", "news"],
            my_num_field_id: 123,
            my_datetime_field_id: 1733210221,
            my_url_field_id: "https://appflowy.io",
            my_checkbox_field_id: true,
        }),
      )
      .await
      .unwrap();

    let row_details = c
      .list_database_row_details(&workspace_id, &todo_db.id, &[&new_row_id])
      .await
      .unwrap();
    assert_eq!(row_details.len(), 1);
    let new_row_detail = &row_details[0];
    assert_eq!(
      new_row_detail.cells["Description"]["data"],
      json!(my_description).to_string()
    );
    assert_eq!(new_row_detail.cells["Status"]["data"][0]["name"], my_status);
    assert_eq!(
      new_row_detail.cells["Multiselect"]["data"][0]["name"],
      "social"
    );
    assert_eq!(
      new_row_detail.cells["Multiselect"]["data"][1]["name"],
      "news"
    );
    assert_eq!(new_row_detail.cells["MyNumberColumn"]["data"], "123");
    assert_eq!(
      new_row_detail.cells["MyDateTimeColumn"]["data"]["timestamp"],
      1733210221
    );
    assert_eq!(
      new_row_detail.cells["MyUrlField"]["data"],
      json!({"data": json!("https://appflowy.io").to_string()}).to_string()
    );
    assert_eq!(new_row_detail.cells["MyCheckboxColumn"]["data"], "true");
  }
}
