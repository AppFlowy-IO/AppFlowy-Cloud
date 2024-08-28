use client_api_test::*;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab_document::{blocks::json_str_to_hashmap, document::Document};
use collab_entity::CollabType;
use collab_folder::{IconType, ViewIcon, ViewLayout};
use database_entity::dto::{QueryCollab, QueryCollabParams};

/// Get the document collab from the remote server
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
  let general_space = views[0].clone();
  assert_eq!(general_space.name, "General");
  assert!(general_space.icon.is_none());
  assert!(general_space.extra.is_some());
  let extra = general_space.extra.as_ref().unwrap();
  let general_space_extra = json_str_to_hashmap(extra).unwrap();
  assert_eq!(
    general_space_extra.get("is_space"),
    Some(&serde_json::json!(true))
  );

  // it contains 1 document and 1 board
  let general_space_views = folder.get_views_belong_to(&general_space.id);
  assert_eq!(general_space_views.len(), 2);
  {
    // the first view is the getting started document, and contains 2 sub views
    let getting_started_view = general_space_views[0].clone();
    assert_eq!(getting_started_view.name, "Getting started");
    assert_eq!(getting_started_view.layout, ViewLayout::Document);
    assert_eq!(
      getting_started_view.icon,
      Some(ViewIcon {
        ty: IconType::Emoji,
        value: "⭐️".to_string()
      })
    );

    let getting_started_document = get_document_collab_from_remote(
      &mut test_client,
      workspace_id.clone(),
      &getting_started_view.id,
    )
    .await;
    let document_data = getting_started_document.get_document_data().unwrap();
    assert_eq!(document_data.blocks.len(), 15);

    let getting_started_sub_views = folder.get_views_belong_to(&getting_started_view.id);
    assert_eq!(getting_started_sub_views.len(), 2);

    let desktop_guide_view = getting_started_sub_views[0].clone();
    assert_eq!(desktop_guide_view.name, "Desktop guide");
    assert_eq!(desktop_guide_view.layout, ViewLayout::Document);
    assert_eq!(
      desktop_guide_view.icon,
      Some(ViewIcon {
        ty: IconType::Emoji,
        value: "📎".to_string()
      })
    );
    let desktop_guide_document = get_document_collab_from_remote(
      &mut test_client,
      workspace_id.clone(),
      &desktop_guide_view.id,
    )
    .await;
    let desktop_guide_document_data = desktop_guide_document.get_document_data().unwrap();
    assert_eq!(desktop_guide_document_data.blocks.len(), 39);

    let mobile_guide_view = getting_started_sub_views[1].clone();
    assert_eq!(mobile_guide_view.name, "Mobile guide");
    assert_eq!(mobile_guide_view.layout, ViewLayout::Document);
    assert_eq!(mobile_guide_view.icon, None);
    let mobile_guide_document = get_document_collab_from_remote(
      &mut test_client,
      workspace_id.clone(),
      &mobile_guide_view.id,
    )
    .await;
    let mobile_guide_document_data = mobile_guide_document.get_document_data().unwrap();
    assert_eq!(mobile_guide_document_data.blocks.len(), 34);
  }

  // the second view is the to-dos board, and contains 0 sub views
  {
    let to_dos_view = general_space_views[1].clone();
    assert_eq!(to_dos_view.name, "To-Dos");
    assert_eq!(to_dos_view.layout, ViewLayout::Board);
    assert_eq!(
      to_dos_view.icon,
      Some(ViewIcon {
        ty: IconType::Emoji,
        value: "✅".to_string()
      })
    );

    let to_dos_sub_views = folder.get_views_belong_to(&to_dos_view.id);
    assert_eq!(to_dos_sub_views.len(), 0);
  }

  // shared space is empty
  let shared_space = views[1].clone();
  assert_eq!(shared_space.name, "Shared");
  assert!(shared_space.icon.is_none());
  assert!(shared_space.extra.is_some());
  let extra = shared_space.extra.as_ref().unwrap();
  let shared_space_extra = json_str_to_hashmap(extra).unwrap();
  assert_eq!(
    shared_space_extra.get("is_space"),
    Some(&serde_json::json!(true))
  );
  let shared_space_views = folder.get_views_belong_to(&shared_space.id);
  assert_eq!(shared_space_views.len(), 0);
}
