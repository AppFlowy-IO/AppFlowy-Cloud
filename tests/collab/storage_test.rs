use crate::{
  collab::workspace_id_from_client, user::utils::generate_unique_registered_user_client,
};
use collab::core::collab_plugin::EncodedCollabV1;
use std::collections::HashMap;

use app_error::ErrorCode;
use collab_entity::CollabType;
use database_entity::dto::{
  CollabParams, CreateCollabParams, DeleteCollabParams, QueryCollab, QueryCollabParams,
  QueryCollabResult,
};
use sqlx::types::Uuid;

#[tokio::test]
async fn success_insert_collab_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let raw_data = "hello world".to_string().as_bytes().to_vec();
  let encoded_collab_v1 = EncodedCollabV1::new(vec![], raw_data.clone())
    .encode_to_bytes()
    .unwrap();
  let workspace_id = workspace_id_from_client(&c).await;
  let object_id = Uuid::new_v4().to_string();
  c.create_collab(CreateCollabParams::new(
    workspace_id.clone(),
    CollabParams::new(&object_id, CollabType::Document, encoded_collab_v1),
  ))
  .await
  .unwrap();

  let bytes = c
    .get_collab(QueryCollabParams::new(
      &object_id,
      CollabType::Document,
      &workspace_id,
    ))
    .await
    .unwrap()
    .doc_state;

  assert_eq!(bytes, raw_data);
}

#[tokio::test]
async fn success_batch_get_collab_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let queries = vec![
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Document,
    },
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Folder,
    },
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Database,
    },
  ];

  let mut expected_results = HashMap::new();
  for (index, query) in queries.iter().enumerate() {
    let object_id = query.object_id.clone();
    let collab_type = query.collab_type.clone();
    let raw_data = format!("hello world {}", index).as_bytes().to_vec();

    expected_results.insert(
      object_id.clone(),
      QueryCollabResult::Success {
        encode_collab_v1: raw_data.clone(),
      },
    );

    c.create_collab(CreateCollabParams::new(
      workspace_id.clone(),
      CollabParams::new(&object_id, collab_type, raw_data.clone()),
    ))
    .await
    .unwrap();
  }

  let results = c.batch_get_collab(&workspace_id, queries).await.unwrap().0;
  for (object_id, result) in expected_results.iter() {
    assert_eq!(result, results.get(object_id).unwrap());
  }
}

#[tokio::test]
async fn success_part_batch_get_collab_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let queries = vec![
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Document,
    },
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Folder,
    },
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Database,
    },
  ];

  let mut expected_results = HashMap::new();
  for (index, query) in queries.iter().enumerate() {
    let object_id = query.object_id.clone();
    let collab_type = query.collab_type.clone();
    let raw_data = format!("hello world {}", index).as_bytes().to_vec();

    if index == 1 {
      expected_results.insert(
        object_id.clone(),
        QueryCollabResult::Failed {
          error: "Record not found".to_string(),
        },
      );
    } else {
      expected_results.insert(
        object_id.clone(),
        QueryCollabResult::Success {
          encode_collab_v1: raw_data.clone(),
        },
      );
      c.create_collab(CreateCollabParams::new(
        workspace_id.clone(),
        CollabParams::new(&object_id, collab_type, raw_data.clone()),
      ))
      .await
      .unwrap();
    }
  }

  let results = c.batch_get_collab(&workspace_id, queries).await.unwrap().0;
  for (object_id, result) in expected_results.iter() {
    assert_eq!(result, results.get(object_id).unwrap());
  }
}

#[tokio::test]
async fn success_delete_collab_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let raw_data = "hello world".to_string().as_bytes().to_vec();
  let workspace_id = workspace_id_from_client(&c).await;
  let object_id = Uuid::new_v4().to_string();
  c.create_collab(CreateCollabParams::new(
    workspace_id.clone(),
    CollabParams::new(&object_id, CollabType::Document, raw_data.clone()),
  ))
  .await
  .unwrap();

  c.delete_collab(DeleteCollabParams {
    object_id: object_id.clone(),
    workspace_id: workspace_id.clone(),
  })
  .await
  .unwrap();

  let error = c
    .get_collab(QueryCollabParams::new(
      &object_id,
      CollabType::Document,
      &workspace_id,
    ))
    .await
    .unwrap_err();

  assert_eq!(error.code, ErrorCode::RecordNotFound);
}

#[tokio::test]
async fn fail_insert_collab_with_empty_payload_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let error = c
    .create_collab(CreateCollabParams::new(
      workspace_id,
      CollabParams::new(Uuid::new_v4(), CollabType::Document, vec![]),
    ))
    .await
    .unwrap_err();

  assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[tokio::test]
async fn fail_insert_collab_with_invalid_workspace_id_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = Uuid::new_v4().to_string();
  let raw_data = "hello world".to_string().as_bytes().to_vec();
  let error = c
    .create_collab(CreateCollabParams::new(
      workspace_id,
      CollabParams::new(Uuid::new_v4(), CollabType::Document, raw_data.clone()),
    ))
    .await
    .unwrap_err();

  assert_eq!(error.code, ErrorCode::NotEnoughPermissions);
}
