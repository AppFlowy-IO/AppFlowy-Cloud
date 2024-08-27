use std::collections::HashSet;

use app_error::ErrorCode;
use client_api::entity::{
  AccountLink, CreateTemplateCategoryParams, CreateTemplateParams, PublishCollabItem,
  PublishCollabMetadata, TemplateCategoryType, UpdateTemplateCategoryParams, UpdateTemplateParams,
};
use client_api_test::*;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab_document::document::Document;
use collab_entity::CollabType;
use database_entity::dto::{QueryCollab, QueryCollabParams};
use sqlx::Database;
use uuid::Uuid;

// |-- General (space)
//     |-- Getting started (document)
//          |-- Desktop guide (document)
//          |-- Mobile guide (document)
//     |-- To-Dos (board)
// |-- Shared (space)
//     |-- ... (empty)
#[tokio::test]
async fn get_user_default_workspace_test() {
  let email = generate_unique_email();
  let password = "Hello!123#";
  let c = localhost_client();
  c.sign_up(&email, password).await.unwrap();
  let mut test_client = TestClient::new_user().await;
  let folder = test_client.get_user_folder().await;

  let workspace_id = test_client.workspace_id().await;
  let views = folder.get_views_belong_to(&workspace_id);

  // 2 spaces
  assert_eq!(views.len(), 2);

  // the first view is the general space
  assert_eq!(views[0].name, "General");

  // and it contains 1 document and 1 board
  let general_space_views = folder.get_views_belong_to(&views[0].id);
  assert_eq!(general_space_views.len(), 2);

  // the first view is the getting started document, and contains 2 sub views
  {
    let getting_started_view = general_space_views[0];
    assert_eq!(getting_started_view.name, "Getting started");
    assert_eq!(getting_started_view.layout, ViewLayout::Document);
    assert_eq!(getting_started_view.icon, "⭐️");

    let getting_started_document =
      get_document_collab_from_remote(&mut test_client, workspace_id, &getting_started_view.id)
        .await;
    let document_data = getting_started_document.get_document_data().unwrap();
    print!(
      "getting_started blocks len {:?}",
      document_data.blocks.len()
    );

    let getting_started_sub_views = folder.get_views_belong_to(&getting_started_view.id);
    assert_eq!(getting_started_sub_views.len(), 2);

    let desktop_guide_view = getting_started_sub_views[0];
    assert_eq!(desktop_guide_view.name, "Desktop guide");
    assert_eq!(desktop_guide_view.layout, ViewLayout::Document);
    assert_eq!(desktop_guide_view.icon, "📎");
    let desktop_guide_document =
      get_document_collab_from_remote(&mut test_client, workspace_id, &desktop_guide_view.id).await;
    let desktop_guide_document_data = desktop_guide_document.get_document_data().unwrap();
    print!(
      "desktop_guide blocks len {:?}",
      desktop_guide_document_data.blocks.len()
    );

    let mobile_guide_view = getting_started_sub_views[1];
    assert_eq!(mobile_guide_view.name, "Mobile guide");
    assert_eq!(mobile_guide_view.layout, ViewLayout::Document);
    assert_eq!(mobile_guide_view.icon, "");
    let mobile_guide_document =
      get_document_collab_from_remote(&mut test_client, workspace_id, &mobile_guide_view.id).await;
    let mobile_guide_document_data = mobile_guide_document.get_document_data().unwrap();
    print!(
      "mobile_guide blocks len {:?}",
      mobile_guide_document_data.blocks.len()
    );
  }

  // the second view is the to-dos board, and contains 0 sub views
  {
    let to_dos_view = general_space_views[1];
    assert_eq!(to_dos_view.name, "To-Dos");
    assert_eq!(to_dos_view.layout, ViewLayout::Board);
    assert_eq!(to_dos_view.icon, "✅");

    let to_dos_sub_views = folder.get_views_belong_to(&to_dos_view.id);
    assert_eq!(to_dos_sub_views.len(), 0);
  }

  // shared space is empty
  assert_eq!(views[1].name, "Shared");
  let shared_space_views = folder.get_views_belong_to(&views[1].id);
  assert_eq!(shared_space_views.len(), 0);
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
  Document::open_with_options(
    CollabOrigin::Empty,
    DataSource::DocStateV1(resp.encode_collab.doc_state.to_vec()),
    document_id,
    vec![],
  )
  .unwrap()
}
