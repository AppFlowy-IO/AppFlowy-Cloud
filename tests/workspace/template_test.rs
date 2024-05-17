use client_api_test::*;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab_document::document::Document;
use collab_entity::CollabType;
use database_entity::dto::{QueryCollab, QueryCollabParams};

#[tokio::test]
async fn get_user_default_workspace_test() {
  let email = generate_unique_email();
  let password = "Hello!123#";
  let c = localhost_client();
  c.sign_up(&email, password).await.unwrap();
  let mut test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let folder = test_client.get_user_folder().await;

  let views = folder.get_views_belong_to(&test_client.workspace_id().await);
  assert_eq!(views.len(), 1);
  assert_eq!(views[0].name, "Getting started");

  let document_id = views[0].id.clone();
  let document =
    get_document_collab_from_remote(&mut test_client, workspace_id, &document_id).await;
  let document_data = document.get_document_data().unwrap();
  assert_eq!(document_data.blocks.len(), 25);
}

async fn get_document_collab_from_remote(
  test_client: &mut TestClient,
  workspace_id: String,
  document_id: &str,
) -> Document {
  let params = QueryCollabParams {
    workspace_id,
    inner: QueryCollab {
      object_id: document_id.to_string(),
      collab_type: CollabType::Document,
    },
  };
  let resp = test_client.get_collab(params).await.unwrap();
  Document::from_doc_state(
    CollabOrigin::Empty,
    DataSource::DocStateV1(resp.encode_collab.doc_state.to_vec()),
    document_id,
    vec![],
  )
  .unwrap()
}
