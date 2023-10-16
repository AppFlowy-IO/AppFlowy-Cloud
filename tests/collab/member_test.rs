use crate::collab::workspace_id_from_client;
use crate::user::utils::generate_unique_registered_user_client;
use collab_entity::CollabType;
use database_entity::{
  AFAccessLevel, CollabMemberIdentify, InsertCollabParams, UpdateCollabMemberParams,
};
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn collab_owner_permission_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let raw_data = "hello world".to_string().as_bytes().to_vec();
  let workspace_id = workspace_id_from_client(&c).await;
  let object_id = Uuid::new_v4().to_string();
  let uid = c.get_profile().await.unwrap().uid.unwrap();

  c.create_collab(InsertCollabParams::new(
    &object_id,
    CollabType::Document,
    raw_data.clone(),
    workspace_id.clone(),
  ))
  .await
  .unwrap();

  let member = c
    .get_collab_member(CollabMemberIdentify {
      uid,
      object_id,
      workspace_id,
    })
    .await
    .unwrap();

  assert_eq!(member.permission.access_level, AFAccessLevel::FullAccess);
}

#[tokio::test]
async fn update_collab_owner_permission_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let raw_data = "hello world".to_string().as_bytes().to_vec();
  let workspace_id = workspace_id_from_client(&c).await;
  let object_id = Uuid::new_v4().to_string();
  let uid = c.get_profile().await.unwrap().uid.unwrap();

  c.create_collab(InsertCollabParams::new(
    &object_id,
    CollabType::Document,
    raw_data.clone(),
    workspace_id.clone(),
  ))
  .await
  .unwrap();

  tokio::time::sleep(Duration::from_secs(1)).await;

  c.update_collab_member(UpdateCollabMemberParams {
    uid,
    workspace_id: workspace_id.clone(),
    object_id: object_id.clone(),
    access_level: AFAccessLevel::ReadOnly,
  })
  .await
  .unwrap();

  let member = c
    .get_collab_member(CollabMemberIdentify {
      uid,
      object_id,
      workspace_id,
    })
    .await
    .unwrap();

  assert_eq!(member.permission.access_level, AFAccessLevel::ReadOnly);
}
