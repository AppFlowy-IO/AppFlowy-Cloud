use std::collections::HashMap;

use collab::preclude::uuid_v4;
use collab_database::database::DatabaseData;
use collab_database::entity::CreateDatabaseParams;
use collab_document::document_data::generate_id;
use collab_entity::CollabType;
use serde_json::json;

use crate::document::getting_started::*;
use crate::TemplateData;
use crate::TemplateObjectId;
use crate::{hierarchy_builder::WorkspaceViewBuilder, WorkspaceTemplate};

#[cfg(test)]
mod tests {
  use super::*;
  use crate::document::util::{create_database_from_params, create_document_from_json};
  use collab_database::database::gen_database_view_id;

  #[tokio::test]
  async fn create_document_from_desktop_guide_json_test() {
    let json_str = include_str!("../../assets/desktop_guide.json");
    test_document_json(json_str).await;
  }

  #[tokio::test]
  async fn create_document_from_mobile_guide_json_test() {
    let json_str = include_str!("../../assets/mobile_guide.json");
    test_document_json(json_str).await;
  }

  #[tokio::test]
  async fn create_document_from_getting_started_json_test() {
    let json_str = include_str!("../../assets/getting_started.json");
    test_document_json(json_str).await;
  }

  #[tokio::test]
  async fn create_database_from_todos_json_test() {
    let json_str = include_str!("../../assets/to-dos.json");
    let template_data = test_database_json(json_str).await;
    // one database and 5 rows
    assert_eq!(template_data.len(), 6);
  }

  async fn test_document_json(json_str: &str) {
    let object_id = uuid_v4().to_string();
    let result = create_document_from_json(object_id.clone(), json_str).await;
    let template_data = result.unwrap();

    match template_data.template_id {
      TemplateObjectId::Document(oid) => {
        assert_eq!(oid, object_id);
      },
      _ => {
        panic!("Template data is not a document");
      },
    }
    assert_eq!(template_data.collab_type, CollabType::Document);
    assert!(!template_data.encoded_collab.doc_state.is_empty());
  }

  async fn test_database_json(json_str: &str) -> Vec<TemplateData> {
    let object_id = gen_database_view_id().to_string();
    let database_data = serde_json::from_str::<DatabaseData>(json_str).unwrap();

    let database_view_id = database_data.views[0].id.clone();
    let create_database_params =
      CreateDatabaseParams::from_database_data(database_data, &database_view_id, &object_id);
    let result = create_database_from_params(object_id.clone(), create_database_params).await;
    let template_data = result.unwrap();

    for (i, data) in template_data.iter().enumerate() {
      if i == 0 {
        // The first item is the database
        assert_eq!(data.collab_type, CollabType::Database);
      } else {
        // The rest are database rows
        assert_eq!(data.collab_type, CollabType::DatabaseRow);
      }

      assert!(!data.encoded_collab.doc_state.is_empty());
    }

    template_data
  }

  #[tokio::test]
  async fn create_workspace_view_with_getting_started_template_test() {
    let template = GettingStartedTemplate;
    let mut workspace_view_builder = WorkspaceViewBuilder::new(generate_id(), 1);

    let result = template
      .create_workspace_view(1, &mut workspace_view_builder)
      .await
      .unwrap();

    // 2 spaces + 4 documents + 1 database + 5 database rows
    assert_eq!(result.len(), 12);

    let views = workspace_view_builder.build();

    // check the number of spaces
    assert_eq!(views.len(), 2);

    let general_space = &views[0];
    let shared_space = &views[1];

    // General
    assert_eq!(general_space.parent_view.name, "General");
    // generate space contains 1 document and 1 database at the first level
    assert_eq!(general_space.child_views.len(), 2);
    // the first document contains 2 children
    assert_eq!(general_space.child_views[0].child_views.len(), 3);
    // the first database contains 0 children
    assert_eq!(general_space.child_views[1].child_views.len(), 0);

    // Shared
    assert_eq!(shared_space.parent_view.name, "Shared");
    // shared space is empty by default
    assert!(shared_space.child_views.is_empty());
  }

  #[test]
  fn replace_json_placeholders_test() {
    let mut json_value = json!({
        "id": "<desktop_guide_view_id>",
        "children": ["<referenced_view_id_1>", "<referenced_view_id_2>"],
        "attributes": {
            "key": "<value>"
        }
    });

    let mut replacements = HashMap::new();
    replacements.insert("desktop_guide_view_id".to_string(), "1".to_string());
    replacements.insert("referenced_view_id_1".to_string(), "2".to_string());
    replacements.insert("referenced_view_id_2".to_string(), "3".to_string());
    replacements.insert("value".to_string(), "appflowy".to_string());

    replace_json_placeholders(&mut json_value, &replacements);

    let expected = json!({
        "id": "1",
        "children": ["2", "3"],
        "attributes": {
            "key": "appflowy"
        }
    });

    assert_eq!(json_value, expected);
  }
}
