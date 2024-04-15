use crate::collab::util::{redis_connection_manager, test_encode_collab_v1};
use app_error::ErrorCode;
use appflowy_cloud::biz::collab::mem_cache::CollabMemCache;
use client_api_test_util::*;
use collab::core::collab_plugin::EncodedCollab;
use collab_entity::CollabType;
use database_entity::dto::{
  CreateCollabParams, DeleteCollabParams, QueryCollab, QueryCollabParams, QueryCollabResult,
};
use sqlx::types::Uuid;
use std::collections::HashMap;

#[tokio::test]
async fn success_insert_collab_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let object_id = Uuid::new_v4().to_string();
  let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world");
  c.create_collab(CreateCollabParams {
    object_id: object_id.clone(),
    collab_type: CollabType::Unknown,
    workspace_id: workspace_id.clone(),
    encoded_collab_v1: encode_collab.encode_to_bytes().unwrap(),
  })
  .await
  .unwrap();

  let doc_state = c
    .get_collab(QueryCollabParams::new(
      &object_id,
      CollabType::Document,
      &workspace_id,
    ))
    .await
    .unwrap()
    .doc_state;

  assert_eq!(doc_state, encode_collab.doc_state);
}

#[tokio::test]
async fn success_batch_get_collab_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let queries = vec![
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Unknown,
    },
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Unknown,
    },
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Unknown,
    },
  ];

  let mut expected_results = HashMap::new();
  for query in queries.iter() {
    let object_id = query.object_id.clone();
    let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world")
      .encode_to_bytes()
      .unwrap();
    let collab_type = query.collab_type.clone();

    expected_results.insert(
      object_id.clone(),
      QueryCollabResult::Success {
        encode_collab_v1: encode_collab.clone(),
      },
    );

    c.create_collab(CreateCollabParams {
      object_id: object_id.clone(),
      encoded_collab_v1: encode_collab.clone(),
      collab_type: collab_type.clone(),
      workspace_id: workspace_id.clone(),
    })
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
      collab_type: CollabType::Unknown,
    },
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Unknown,
    },
    QueryCollab {
      object_id: Uuid::new_v4().to_string(),
      collab_type: CollabType::Unknown,
    },
  ];

  let mut expected_results = HashMap::new();
  for (index, query) in queries.iter().enumerate() {
    let object_id = query.object_id.clone();
    let collab_type = query.collab_type.clone();
    let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world")
      .encode_to_bytes()
      .unwrap();

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
          encode_collab_v1: encode_collab.clone(),
        },
      );
      c.create_collab(CreateCollabParams {
        object_id: object_id.clone(),
        encoded_collab_v1: encode_collab.clone(),
        collab_type: collab_type.clone(),
        workspace_id: workspace_id.clone(),
      })
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
  let workspace_id = workspace_id_from_client(&c).await;
  let object_id = Uuid::new_v4().to_string();
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
    .create_collab(CreateCollabParams {
      object_id: Uuid::new_v4().to_string(),
      encoded_collab_v1: vec![],
      collab_type: CollabType::Unknown,
      workspace_id: workspace_id.clone(),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[tokio::test]
async fn fail_insert_collab_with_invalid_workspace_id_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = Uuid::new_v4().to_string();
  let object_id = Uuid::new_v4().to_string();
  let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world")
    .encode_to_bytes()
    .unwrap();
  let error = c
    .create_collab(CreateCollabParams {
      object_id,
      encoded_collab_v1: encode_collab,
      collab_type: CollabType::Unknown,
      workspace_id: workspace_id.clone(),
    })
    .await
    .unwrap_err();

  assert_eq!(error.code, ErrorCode::NotEnoughPermissions);
}

#[tokio::test]
async fn collab_mem_cache_read_write_test() {
  let conn = redis_connection_manager().await;

  let mem_cache = CollabMemCache::new(conn);
  let encode_collab = EncodedCollab::new_v1(vec![1, 2, 3], vec![4, 5, 6]);

  let object_id = uuid::Uuid::new_v4().to_string();
  let timestamp = chrono::Utc::now().timestamp();
  mem_cache
    .insert_encode_collab_data(
      object_id.clone(),
      encode_collab.encode_to_bytes().unwrap(),
      timestamp,
    )
    .await;

  let encode_collab_from_cache = mem_cache.get_encode_collab(&object_id).await.unwrap();
  assert_eq!(encode_collab_from_cache.state_vector, vec![1, 2, 3]);
  assert_eq!(encode_collab_from_cache.doc_state, vec![4, 5, 6]);
}

#[tokio::test]
async fn collab_mem_cache_insert_override_test() {
  let conn = redis_connection_manager().await;

  let mem_cache = CollabMemCache::new(conn);
  let object_id = uuid::Uuid::new_v4().to_string();
  let encode_collab = EncodedCollab::new_v1(vec![1, 2, 3], vec![4, 5, 6]);
  let mut timestamp = chrono::Utc::now().timestamp();
  mem_cache
    .insert_encode_collab_data(
      object_id.clone(),
      encode_collab.encode_to_bytes().unwrap(),
      timestamp,
    )
    .await;

  // the following insert should not override the previous one because the timestamp is older
  // than the previous one
  timestamp -= 100;
  mem_cache
    .insert_encode_collab_data(
      object_id.clone(),
      EncodedCollab::new_v1(vec![6, 7, 8], vec![9, 10, 11])
        .encode_to_bytes()
        .unwrap(),
      timestamp,
    )
    .await;

  // check that the previous insert is still in the cache
  let encode_collab_from_cache = mem_cache.get_encode_collab(&object_id).await.unwrap();
  assert_eq!(encode_collab_from_cache.doc_state, encode_collab.doc_state);
  assert_eq!(encode_collab_from_cache.state_vector, vec![1, 2, 3]);
  assert_eq!(encode_collab_from_cache.doc_state, vec![4, 5, 6]);

  // the following insert should override the previous one because the timestamp is newer
  timestamp += 500;
  mem_cache
    .insert_encode_collab_data(
      object_id.clone(),
      EncodedCollab::new_v1(vec![12, 13, 14], vec![15, 16, 17])
        .encode_to_bytes()
        .unwrap(),
      timestamp,
    )
    .await;

  // check that the previous insert is overridden
  let encode_collab_from_cache = mem_cache.get_encode_collab(&object_id).await.unwrap();
  assert_eq!(encode_collab_from_cache.doc_state, vec![15, 16, 17]);
  assert_eq!(encode_collab_from_cache.state_vector, vec![12, 13, 14]);
}
