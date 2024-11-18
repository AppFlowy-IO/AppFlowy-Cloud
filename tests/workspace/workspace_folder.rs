use std::time::Duration;

use client_api::entity::{CreateCollabParams, QueryCollabParams};
use client_api_test::generate_unique_registered_user_client;
use collab::core::origin::CollabClient;
use collab_folder::{CollabOrigin, Folder};
use tokio::time::sleep;

#[tokio::test]
async fn get_workpace_folder() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id.to_string();

  let folder_view = c
    .get_workspace_folder(&workspace_id, None, None)
    .await
    .unwrap();
  assert_eq!(folder_view.name, "Workspace");
  assert_eq!(folder_view.children[0].name, "General");
  assert_eq!(folder_view.children[0].children.len(), 0);
  let folder_view = c
    .get_workspace_folder(&workspace_id, Some(2), None)
    .await
    .unwrap();
  assert_eq!(folder_view.name, "Workspace");
  assert_eq!(folder_view.children[0].name, "General");
  assert_eq!(folder_view.children[0].children.len(), 2);
  let folder_view = c
    .get_workspace_folder(
      &workspace_id,
      Some(1),
      Some(folder_view.children[0].view_id.clone()),
    )
    .await
    .unwrap();
  assert_eq!(folder_view.children.len(), 2);
}

#[tokio::test]
async fn get_section_items() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let user_workspace_info = c.get_user_workspace_info().await.unwrap();
  let workspaces = c.get_workspaces().await.unwrap();
  assert_eq!(workspaces.len(), 1);
  let workspace_id = workspaces[0].workspace_id.to_string();
  let folder_collab = c
    .get_collab(QueryCollabParams::new(
      workspace_id.clone(),
      collab_entity::CollabType::Folder,
      workspace_id.clone(),
    ))
    .await
    .unwrap()
    .encode_collab;
  let uid = user_workspace_info.user_profile.uid;
  let mut folder = Folder::from_collab_doc_state(
    uid,
    CollabOrigin::Client(CollabClient::new(uid, c.device_id.clone())),
    folder_collab.into(),
    &workspace_id,
    vec![],
  )
  .unwrap();
  let views = folder.get_views_belong_to(&workspace_id);
  let new_favorite_id = views[0].children[0].id.clone();
  let to_be_deleted_favorite_id = views[0].children[1].id.clone();
  folder.add_favorite_view_ids(vec![
    new_favorite_id.clone(),
    to_be_deleted_favorite_id.clone(),
  ]);
  folder.add_trash_view_ids(vec![to_be_deleted_favorite_id.clone()]);
  let recent_id = folder.get_views_belong_to(&new_favorite_id)[0].id.clone();
  folder.add_recent_view_ids(vec![recent_id.clone()]);
  let collab_type = collab_entity::CollabType::Folder;
  c.update_collab(CreateCollabParams {
    workspace_id: workspace_id.clone(),
    collab_type: collab_type.clone(),
    object_id: workspace_id.clone(),
    encoded_collab_v1: folder
      .encode_collab_v1(|collab| collab_type.validate_require_data(collab))
      .unwrap()
      .encode_to_bytes()
      .unwrap(),
  })
  .await
  .unwrap();
  // Collab update is performed asynchronously via a queue
  sleep(Duration::from_secs(1)).await;
  let favorite_section_items = c.get_workspace_favorite(&workspace_id).await.unwrap();
  assert_eq!(favorite_section_items.views.len(), 1);
  assert_eq!(
    favorite_section_items.views[0].view.view_id,
    new_favorite_id
  );
  let trash_section_items = c.get_workspace_trash(&workspace_id).await.unwrap();
  assert_eq!(trash_section_items.views.len(), 1);
  assert_eq!(
    trash_section_items.views[0].view.view_id,
    to_be_deleted_favorite_id
  );
  let recent_section_items = c.get_workspace_recent(&workspace_id).await.unwrap();
  assert_eq!(recent_section_items.views.len(), 1);
  assert_eq!(recent_section_items.views[0].view.view_id, recent_id);
}
