use app_error::ErrorCode;
use appflowy_collaborate::collab::cache::mem_cache::CollabMemCache;
use appflowy_collaborate::CollabMetrics;
use client_api_test::*;
use collab::core::transaction::DocTransactionExtension;
use collab::entity::EncodedCollab;
use collab::preclude::{Doc, Transact};
use collab_entity::CollabType;
use database::collab::CollabMetadata;
use database_entity::dto::{
  CreateCollabParams, DeleteCollabParams, QueryCollab, QueryCollabParams, QueryCollabResult,
};
use sqlx::types::Uuid;
use std::collections::HashMap;
use workspace_template::document::getting_started::GettingStartedTemplate;
use workspace_template::WorkspaceTemplateBuilder;

use crate::collab::util::{redis_connection_manager, test_encode_collab_v1};

#[tokio::test]
async fn success_insert_collab_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let object_id = Uuid::new_v4();
  let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world");
  c.create_collab(CreateCollabParams {
    object_id,
    collab_type: CollabType::Unknown,
    workspace_id,
    encoded_collab_v1: encode_collab.encode_to_bytes().unwrap(),
  })
  .await
  .unwrap();

  let doc_state = c
    .get_collab(QueryCollabParams::new(
      object_id,
      CollabType::Document,
      workspace_id,
    ))
    .await
    .unwrap()
    .encode_collab
    .doc_state;

  assert_eq!(doc_state, encode_collab.doc_state);
}

#[tokio::test]
async fn success_batch_get_collab_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = workspace_id_from_client(&c).await;
  let queries = vec![
    QueryCollab {
      object_id: Uuid::new_v4(),
      collab_type: CollabType::Unknown,
    },
    QueryCollab {
      object_id: Uuid::new_v4(),
      collab_type: CollabType::Unknown,
    },
    QueryCollab {
      object_id: Uuid::new_v4(),
      collab_type: CollabType::Unknown,
    },
  ];

  let mut expected_results = HashMap::new();
  for query in queries.iter() {
    let object_id = query.object_id;
    let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world")
      .encode_to_bytes()
      .unwrap();
    let collab_type = query.collab_type;

    expected_results.insert(
      object_id,
      QueryCollabResult::Success {
        encode_collab_v1: encode_collab.clone(),
      },
    );

    c.create_collab(CreateCollabParams {
      object_id,
      encoded_collab_v1: encode_collab.clone(),
      collab_type,
      workspace_id,
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
      object_id: Uuid::new_v4(),
      collab_type: CollabType::Unknown,
    },
    QueryCollab {
      object_id: Uuid::new_v4(),
      collab_type: CollabType::Unknown,
    },
    QueryCollab {
      object_id: Uuid::new_v4(),
      collab_type: CollabType::Unknown,
    },
  ];

  let mut expected_results = HashMap::new();
  for (index, query) in queries.iter().enumerate() {
    let object_id = query.object_id;
    let collab_type = query.collab_type;
    let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world")
      .encode_to_bytes()
      .unwrap();

    if index == 1 {
      expected_results.insert(
        object_id,
        QueryCollabResult::Failed {
          error: "Record not found".to_string(),
        },
      );
    } else {
      expected_results.insert(
        object_id,
        QueryCollabResult::Success {
          encode_collab_v1: encode_collab.clone(),
        },
      );
      c.create_collab(CreateCollabParams {
        object_id,
        encoded_collab_v1: encode_collab.clone(),
        collab_type,
        workspace_id,
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
  let object_id = Uuid::new_v4();
  let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world")
    .encode_to_bytes()
    .unwrap();

  c.create_collab(CreateCollabParams {
    object_id,
    encoded_collab_v1: encode_collab,
    collab_type: CollabType::Unknown,
    workspace_id,
  })
  .await
  .unwrap();

  c.delete_collab(DeleteCollabParams {
    object_id,
    workspace_id,
  })
  .await
  .unwrap();

  let error = c
    .get_collab(QueryCollabParams::new(
      object_id,
      CollabType::Document,
      workspace_id,
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
      object_id: Uuid::new_v4(),
      encoded_collab_v1: vec![],
      collab_type: CollabType::Document,
      workspace_id,
    })
    .await
    .unwrap_err();

  assert_eq!(error.code, ErrorCode::NoRequiredData);
}

#[tokio::test]
async fn fail_insert_collab_with_invalid_workspace_id_test() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let workspace_id = Uuid::new_v4();
  let object_id = Uuid::new_v4();
  let encode_collab = test_encode_collab_v1(&object_id, "title", "hello world")
    .encode_to_bytes()
    .unwrap();
  let error = c
    .create_collab(CreateCollabParams {
      object_id,
      encoded_collab_v1: encode_collab,
      collab_type: CollabType::Unknown,
      workspace_id,
    })
    .await
    .unwrap_err();

  assert_eq!(error.code, ErrorCode::RecordNotFound);
}

#[tokio::test]
async fn collab_mem_cache_read_write_test() {
  let conn = redis_connection_manager().await;
  let mem_cache = CollabMemCache::new(conn, CollabMetrics::default().into());
  let encode_collab = EncodedCollab::new_v1(vec![1, 2, 3], vec![4, 5, 6]);

  let object_id = Uuid::new_v4();
  let timestamp = chrono::Utc::now().timestamp();
  mem_cache
    .insert_encode_collab_data(
      &object_id,
      &encode_collab.encode_to_bytes().unwrap(),
      timestamp,
      None,
    )
    .await
    .unwrap();

  let (_, encode_collab_from_cache) = mem_cache.get_encode_collab(&object_id).await.unwrap();
  assert_eq!(encode_collab_from_cache.state_vector, vec![1, 2, 3]);
  assert_eq!(encode_collab_from_cache.doc_state, vec![4, 5, 6]);
}

#[tokio::test]
async fn collab_mem_cache_insert_override_test() {
  let conn = redis_connection_manager().await;
  let mem_cache = CollabMemCache::new(conn, CollabMetrics::default().into());
  let object_id = Uuid::new_v4();
  let encode_collab = EncodedCollab::new_v1(vec![1, 2, 3], vec![4, 5, 6]);
  let mut timestamp = chrono::Utc::now().timestamp();
  mem_cache
    .insert_encode_collab_data(
      &object_id,
      &encode_collab.encode_to_bytes().unwrap(),
      timestamp,
      None,
    )
    .await
    .unwrap();

  // the following insert should not override the previous one because the timestamp is older
  // than the previous one
  timestamp -= 100;
  mem_cache
    .insert_encode_collab_data(
      &object_id,
      &EncodedCollab::new_v1(vec![6, 7, 8], vec![9, 10, 11])
        .encode_to_bytes()
        .unwrap(),
      timestamp,
      None,
    )
    .await
    .unwrap();

  // check that the previous insert is still in the cache
  let (_, encode_collab_from_cache) = mem_cache.get_encode_collab(&object_id).await.unwrap();
  assert_eq!(encode_collab_from_cache.doc_state, encode_collab.doc_state);
  assert_eq!(encode_collab_from_cache.state_vector, vec![1, 2, 3]);
  assert_eq!(encode_collab_from_cache.doc_state, vec![4, 5, 6]);

  // the following insert should override the previous one because the timestamp is newer
  timestamp += 500;
  mem_cache
    .insert_encode_collab_data(
      &object_id,
      &EncodedCollab::new_v1(vec![12, 13, 14], vec![15, 16, 17])
        .encode_to_bytes()
        .unwrap(),
      timestamp,
      None,
    )
    .await
    .unwrap();

  // check that the previous insert is overridden
  let (_, encode_collab_from_cache) = mem_cache.get_encode_collab(&object_id).await.unwrap();
  assert_eq!(encode_collab_from_cache.doc_state, vec![15, 16, 17]);
  assert_eq!(encode_collab_from_cache.state_vector, vec![12, 13, 14]);
}

#[tokio::test]
async fn collab_meta_redis_cache_test() {
  let conn = redis_connection_manager().await;
  let mem_cache = CollabMemCache::new(conn, CollabMetrics::default().into());
  mem_cache
    .get_collab_meta(&Uuid::new_v4())
    .await
    .unwrap_err();

  let object_id = Uuid::new_v4();
  let workspace_id = Uuid::new_v4();
  let meta = CollabMetadata {
    object_id,
    workspace_id,
  };
  mem_cache.insert_collab_meta(meta.clone()).await.unwrap();
  let meta_from_cache = mem_cache.get_collab_meta(&object_id).await.unwrap();
  assert_eq!(meta.workspace_id, meta_from_cache.workspace_id);
  assert_eq!(meta.object_id, meta_from_cache.object_id);
}

#[tokio::test]
async fn insert_empty_data_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4();

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
      workspace_id,
      object_id,
      encoded_collab_v1: vec![],
      collab_type,
    };
    let error = test_client
      .api_client
      .create_collab(params)
      .await
      .unwrap_err();
    assert_eq!(error.code, ErrorCode::NoRequiredData);
  }
}

#[tokio::test]
async fn insert_invalid_data_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4();

  let doc = Doc::new();
  let encoded_collab_v1 = doc
    .transact()
    .get_encoded_collab_v1()
    .encode_to_bytes()
    .unwrap();
  for collab_type in [
    CollabType::Folder,
    CollabType::Document,
    CollabType::UserAwareness,
    CollabType::WorkspaceDatabase,
    CollabType::Database,
    CollabType::DatabaseRow,
  ] {
    let params = CreateCollabParams {
      workspace_id,
      object_id,
      encoded_collab_v1: encoded_collab_v1.clone(),
      collab_type,
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

#[tokio::test]
async fn insert_folder_data_success_test() {
  let test_client = TestClient::new_user().await;
  let workspace_id = test_client.workspace_id().await;
  let object_id = Uuid::new_v4();
  let uid = test_client.uid().await;

  let templates = WorkspaceTemplateBuilder::new(uid, &workspace_id)
    .with_templates(vec![GettingStartedTemplate])
    .build()
    .await
    .unwrap();

  // 2 spaces, 4 documents, 2 databases, 5 rows
  assert_eq!(templates.len(), 13);

  for template in templates.into_iter() {
    let data = template.encoded_collab.encode_to_bytes().unwrap();
    let params = CreateCollabParams {
      workspace_id,
      object_id,
      encoded_collab_v1: data,
      collab_type: template.collab_type,
    };
    test_client.api_client.create_collab(params).await.unwrap();
  }
}
