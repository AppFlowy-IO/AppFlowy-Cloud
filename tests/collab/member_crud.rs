use crate::collab::util::test_encode_collab_v1;
use client_api_test::{generate_unique_registered_user_client, workspace_id_from_client};

use collab_entity::CollabType;
use database_entity::dto::{
  AFAccessLevel, CollabMemberIdentify, CreateCollabParams, InsertCollabMemberParams,
  QueryCollabMembers, UpdateCollabMemberParams,
};
use uuid::Uuid;

#[tokio::test]
async fn collab_owner_permission_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let object_id = Uuid::new_v4().to_string();
  let uid = c.get_profile().await.unwrap().uid;
  let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world")
    .encode_to_bytes()
    .unwrap();

  c.create_collab(CreateCollabParams {
    object_id: object_id.clone(),
    encoded_collab_v1: encode_collab,
    collab_type: CollabType::Unknown,
    workspace_id: workspace_id.clone(),
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

  assert_eq!(member.permission.access_level, AFAccessLevel::FullAccess);
}

#[tokio::test]
async fn update_collab_member_permission_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let object_id = Uuid::new_v4().to_string();
  let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world")
    .encode_to_bytes()
    .unwrap();
  let uid = c.get_profile().await.unwrap().uid;

  c.create_collab(CreateCollabParams {
    object_id: object_id.clone(),
    encoded_collab_v1: encode_collab.clone(),
    collab_type: CollabType::Unknown,
    workspace_id: workspace_id.clone(),
  })
  .await
  .unwrap();

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

#[tokio::test]
async fn add_collab_member_test() {
  let (c_1, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c_1).await;
  let object_id = Uuid::new_v4().to_string();
  let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world");
  c_1
    .create_collab(CreateCollabParams {
      object_id: object_id.clone(),
      encoded_collab_v1: encode_collab.encode_to_bytes().unwrap(),
      collab_type: CollabType::Unknown,
      workspace_id: workspace_id.clone(),
    })
    .await
    .unwrap();

  // create new client
  let (c_2, _user) = generate_unique_registered_user_client().await;
  let uid_2 = c_2.get_profile().await.unwrap().uid;

  // add new member
  c_1
    .add_collab_member(InsertCollabMemberParams {
      uid: uid_2,
      workspace_id: workspace_id.clone(),
      object_id: object_id.clone(),
      access_level: AFAccessLevel::ReadAndComment,
    })
    .await
    .unwrap();

  // check the member is added and its permission is correct
  let member = c_1
    .get_collab_member(CollabMemberIdentify {
      uid: uid_2,
      object_id,
      workspace_id,
    })
    .await
    .unwrap();
  assert_eq!(
    member.permission.access_level,
    AFAccessLevel::ReadAndComment
  );
}

#[tokio::test]
async fn add_collab_member_then_remove_test() {
  let (c_1, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c_1).await;
  let object_id = Uuid::new_v4().to_string();
  let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world");
  c_1
    .create_collab(CreateCollabParams {
      object_id: object_id.clone(),
      encoded_collab_v1: encode_collab.encode_to_bytes().unwrap(),
      collab_type: CollabType::Unknown,
      workspace_id: workspace_id.clone(),
    })
    .await
    .unwrap();

  // Create new client
  let (c_2, _user) = generate_unique_registered_user_client().await;
  let uid_2 = c_2.get_profile().await.unwrap().uid;

  // Add new member
  c_1
    .add_collab_member(InsertCollabMemberParams {
      uid: uid_2,
      workspace_id: workspace_id.clone(),
      object_id: object_id.clone(),
      access_level: AFAccessLevel::ReadAndComment,
    })
    .await
    .unwrap();
  let members = c_1
    .get_collab_members(QueryCollabMembers {
      workspace_id: workspace_id.clone(),
      object_id: object_id.clone(),
    })
    .await
    .unwrap()
    .0;
  assert_eq!(members.len(), 2);

  // Delete the member
  c_1
    .remove_collab_member(CollabMemberIdentify {
      uid: uid_2,
      object_id: object_id.clone(),
      workspace_id: workspace_id.clone(),
    })
    .await
    .unwrap();
  let members = c_1
    .get_collab_members(QueryCollabMembers {
      workspace_id,
      object_id,
    })
    .await
    .unwrap()
    .0;
  assert_eq!(members.len(), 1);
}
