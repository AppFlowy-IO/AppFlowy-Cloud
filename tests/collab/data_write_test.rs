use app_error::ErrorCode;
use client_api_test_util::TestClient;
use collab::core::transaction::DocTransactionExtension;
use collab::preclude::Doc;

use collab_entity::CollabType;
use database_entity::dto::CreateCollabParams;
use workspace_template::document::get_started::GetStartedDocumentTemplate;
use workspace_template::WorkspaceTemplateBuilder;

#[tokio::test]
async fn insert_empty_data_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = uuid::Uuid::new_v4().to_string();

  // test all collab type
  for collab_type in [
    CollabType::Folder,
    CollabType::Document,
    CollabType::UserAwareness,
    CollabType::WorkspaceDatabase,
    CollabType::Database,
    CollabType::DatabaseRow,
  ] {
    let params = CreateCollabParams {
      workspace_id: workspace_id.clone(),
      object_id: object_id.clone(),
      encoded_collab_v1: vec![],
      collab_type,
    };
    let error = test_client
      .api_client
      .create_collab(params)
      .await
      .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
  }
}

#[tokio::test]
async fn insert_invalid_data_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = uuid::Uuid::new_v4().to_string();

  let doc = Doc::new();
  let encoded_collab_v1 = doc.get_encoded_collab_v1().encode_to_bytes().unwrap();
  for collab_type in [
    CollabType::Folder,
    CollabType::Document,
    CollabType::UserAwareness,
    CollabType::WorkspaceDatabase,
    CollabType::Database,
    CollabType::DatabaseRow,
  ] {
    let params = CreateCollabParams {
      workspace_id: workspace_id.clone(),
      object_id: object_id.clone(),
      encoded_collab_v1: encoded_collab_v1.clone(),
      collab_type: collab_type.clone(),
    };
    let error = test_client
      .api_client
      .create_collab(params)
      .await
      .unwrap_err();
    assert_eq!(
      error.code,
      ErrorCode::NoRequiredData,
      "collab_type: {:?}",
      collab_type
    );
  }
}

// #[tokio::test]
// async fn load_document_test() {
//   let test_client = TestClient::new_user().await;
//   let workspace_id = test_client.workspace_id().await;
//   let object_id = uuid::Uuid::new_v4().to_string();
//
//   for file in [include_str!("../test_asset/document/empty_lines.json")] {
//     let document_data: DocumentData = serde_json::from_str(file).unwrap();
//     let template = DocumentTemplate::from_data(document_data)
//       .create(object_id.clone())
//       .await
//       .unwrap();
//
//     let data = template.object_data.encode_to_bytes().unwrap();
//     let params = CreateCollabParams {
//       workspace_id: workspace_id.clone(),
//       object_id: object_id.clone(),
//       encoded_collab_v1: data,
//       collab_type: template.object_type,
//     };
//     test_client.api_client.create_collab(params).await.unwrap();
//   }
// }

#[tokio::test]
async fn insert_folder_data_success_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = uuid::Uuid::new_v4().to_string();
  let uid = test_client.uid().await;

  let templates = WorkspaceTemplateBuilder::new(uid, &workspace_id)
    .with_templates(vec![GetStartedDocumentTemplate])
    .build()
    .await
    .unwrap();

  assert_eq!(templates.len(), 2);
  for (index, template) in templates.into_iter().enumerate() {
    if index == 0 {
      assert_eq!(template.object_type, CollabType::Document);
    }
    if index == 1 {
      assert_eq!(template.object_type, CollabType::Folder);
    }

    let data = template.object_data.encode_to_bytes().unwrap();
    let params = CreateCollabParams {
      workspace_id: workspace_id.clone(),
      object_id: object_id.clone(),
      encoded_collab_v1: data,
      collab_type: template.object_type,
    };
    test_client.api_client.create_collab(params).await.unwrap();
  }
}
