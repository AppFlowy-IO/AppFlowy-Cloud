use crate::collab::util::{generate_random_bytes, redis_connection_manager};
use crate::sql_test::util::{setup_db, test_create_user};
use appflowy_cloud::biz::collab::cache::CollabCache;
use appflowy_cloud::biz::collab::queue::StorageQueue;
use appflowy_cloud::biz::collab::WritePriority;
use client_api_test_util::setup_log;
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use database_entity::dto::{CollabParams, QueryCollab};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;

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
      encode_collab_from_disk.doc_state,
      original_encode_collab.doc_state
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

  let queries = Arc::new(Mutex::new(Vec::new()));
  for i in 0..3 {
    // sleep random seconds less than 2 seconds. because the runtime is single-threaded,
    // we need sleep a little time to let the runtime switch to other tasks.
    sleep(Duration::from_millis(i % 2)).await;

    let encode_collab = EncodedCollab::new_v1(
      generate_random_bytes(10 * 1024),
      generate_random_bytes(2 * 1024 * 1024),
    );
    let params = CollabParams {
      object_id: format!("object_id_{}", i),
      collab_type: CollabType::Unknown,
      encoded_collab_v1: encode_collab.encode_to_bytes().unwrap(),
    };
    storage_queue
      .push(&user.workspace_id, &user.uid, &params, WritePriority::Low)
      .await
      .unwrap();
    queries.lock().await.push((params, encode_collab));
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
      encode_collab_from_disk.doc_state,
      original_encode_collab.doc_state
    );
    assert_eq!(
      encode_collab_from_disk.state_vector,
      original_encode_collab.state_vector
    );
  }
}
