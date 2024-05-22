use collab::core::transaction::DocTransactionExtension;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use collab::entity::EncodedCollab;
use collab::preclude::Doc;
use collab_entity::CollabType;
use sqlx::types::Uuid;
use sqlx::PgPool;
use tokio::sync::Mutex;
use tokio::time::sleep;

use app_error::ErrorCode;
use appflowy_collaborate::collab::cache::CollabCache;
use appflowy_collaborate::collab::mem_cache::CollabMemCache;
use appflowy_collaborate::collab::queue::StorageQueue;
use appflowy_collaborate::collab::WritePriority;
use client_api_test::*;
use database::collab::CollabMetadata;
use database_entity::dto::{
  CollabParams, CreateCollabParams, DeleteCollabParams, QueryCollab, QueryCollabParams,
  QueryCollabResult,
};
use workspace_template::document::get_started::GetStartedDocumentTemplate;
use workspace_template::WorkspaceTemplateBuilder;

use crate::collab::util::{generate_random_bytes, redis_connection_manager, test_encode_collab_v1};
use crate::sql_test::util::{setup_db, test_create_user};

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

  assert_eq!(error.code, ErrorCode::NoRequiredData);
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
      &object_id,
      &encode_collab.encode_to_bytes().unwrap(),
      timestamp,
    )
    .await
    .unwrap();

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
      &object_id,
      &encode_collab.encode_to_bytes().unwrap(),
      timestamp,
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
    )
    .await
    .unwrap();

  // check that the previous insert is still in the cache
  let encode_collab_from_cache = mem_cache.get_encode_collab(&object_id).await.unwrap();
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
    )
    .await
    .unwrap();

  // check that the previous insert is overridden
  let encode_collab_from_cache = mem_cache.get_encode_collab(&object_id).await.unwrap();
  assert_eq!(encode_collab_from_cache.doc_state, vec![15, 16, 17]);
  assert_eq!(encode_collab_from_cache.state_vector, vec![12, 13, 14]);
}

#[tokio::test]
async fn collab_meta_redis_cache_test() {
  let conn = redis_connection_manager().await;
  let mem_cache = CollabMemCache::new(conn);
  mem_cache.get_collab_meta("1").await.unwrap_err();

  let object_id = uuid::Uuid::new_v4().to_string();
  let meta = CollabMetadata {
    object_id: object_id.clone(),
    workspace_id: "w1".to_string(),
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
    assert_eq!(error.code, ErrorCode::NoRequiredData);
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

#[sqlx::test(migrations = false)]
async fn simulate_small_data_set_write(pool: PgPool) {
  // prepare test prerequisites
  setup_db(&pool).await.unwrap();
  setup_log();
  let conn = redis_connection_manager().await;
  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = test_create_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let collab_cache = CollabCache::new(conn.clone(), pool);
  let queue_name = uuid::Uuid::new_v4().to_string();
  let storage_queue = StorageQueue::new(collab_cache.clone(), conn, &queue_name);

  let queries = Arc::new(Mutex::new(Vec::new()));
  for i in 0..10 {
    // sleep random seconds less than 2 seconds. because the runtime is single-threaded,
    // we need sleep a little time to let the runtime switch to other tasks.
    sleep(Duration::from_millis(i % 2)).await;

    let cloned_storage_queue = storage_queue.clone();
    let cloned_queries = queries.clone();
    let cloned_user = user.clone();
    let encode_collab = EncodedCollab::new_v1(
      generate_random_bytes(1024),
      generate_random_bytes(1024 * 1024),
    );
    let params = CollabParams {
      object_id: format!("object_id_{}", i),
      collab_type: CollabType::Unknown,
      encoded_collab_v1: encode_collab.encode_to_bytes().unwrap(),
    };
    cloned_storage_queue
      .push(
        &cloned_user.workspace_id,
        &cloned_user.uid,
        &params,
        WritePriority::Low,
      )
      .await
      .unwrap();
    cloned_queries.lock().await.push((params, encode_collab));
  }

  // Allow some time for processing
  sleep(Duration::from_secs(30)).await;

  // Check that all items are processed correctly
  for (params, original_encode_collab) in queries.lock().await.iter() {
    let query = QueryCollab {
      object_id: params.object_id.clone(),
      collab_type: params.collab_type.clone(),
    };
    let encode_collab_from_disk = collab_cache
      .get_encode_collab_from_disk(&user.uid, query)
      .await
      .unwrap();

    assert_eq!(
      encode_collab_from_disk.doc_state.len(),
      original_encode_collab.doc_state.len(),
      "doc_state length mismatch"
    );
    assert_eq!(
      encode_collab_from_disk.doc_state,
      original_encode_collab.doc_state
    );

    assert_eq!(
      encode_collab_from_disk.state_vector.len(),
      original_encode_collab.state_vector.len(),
      "state_vector length mismatch"
    );
    assert_eq!(
      encode_collab_from_disk.state_vector,
      original_encode_collab.state_vector
    );
  }
}

#[sqlx::test(migrations = false)]
async fn simulate_large_data_set_write(pool: PgPool) {
  // prepare test prerequisites
  setup_db(&pool).await.unwrap();
  setup_log();

  let conn = redis_connection_manager().await;
  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = test_create_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let collab_cache = CollabCache::new(conn.clone(), pool);
  let queue_name = uuid::Uuid::new_v4().to_string();
  let storage_queue = StorageQueue::new(collab_cache.clone(), conn, &queue_name);

  let origin_encode_collab = EncodedCollab::new_v1(
    generate_random_bytes(10 * 1024),
    generate_random_bytes(2 * 1024 * 1024),
  );
  let params = CollabParams {
    object_id: uuid::Uuid::new_v4().to_string(),
    collab_type: CollabType::Unknown,
    encoded_collab_v1: origin_encode_collab.encode_to_bytes().unwrap(),
  };
  storage_queue
    .push(&user.workspace_id, &user.uid, &params, WritePriority::Low)
    .await
    .unwrap();

  // Allow some time for processing
  sleep(Duration::from_secs(30)).await;

  let query = QueryCollab {
    object_id: params.object_id.clone(),
    collab_type: params.collab_type.clone(),
  };
  let encode_collab_from_disk = collab_cache
    .get_encode_collab_from_disk(&user.uid, query)
    .await
    .unwrap();
  assert_eq!(
    encode_collab_from_disk.doc_state.len(),
    origin_encode_collab.doc_state.len(),
    "doc_state length mismatch"
  );
  assert_eq!(
    encode_collab_from_disk.doc_state,
    origin_encode_collab.doc_state
  );

  assert_eq!(
    encode_collab_from_disk.state_vector.len(),
    origin_encode_collab.state_vector.len(),
    "state_vector length mismatch"
  );
  assert_eq!(
    encode_collab_from_disk.state_vector,
    origin_encode_collab.state_vector
  );
}
