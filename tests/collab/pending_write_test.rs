use crate::collab::util::redis_connection_manager;
use crate::sql_test::util::{setup_db, test_create_user};
use appflowy_cloud::biz::collab::cache::CollabCache;
use appflowy_cloud::biz::collab::queue::StorageQueue;
use appflowy_cloud::biz::collab::WritePriority;
use client_api_test_util::setup_log;
use collab::core::collab_plugin::EncodedCollab;
use collab_entity::CollabType;
use database_entity::dto::{CollabParams, QueryCollab};
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;

#[sqlx::test(migrations = false)]
async fn pending_queue_write_test(pool: PgPool) {
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
  let storage_queue = StorageQueue::new(collab_cache.clone(), conn);
  storage_queue.clear().await.unwrap();
  sleep(Duration::from_secs(3)).await;

  let mut queries = Vec::new();
  for i in 0..30 {
    let encode_collab = EncodedCollab::new_v1(vec![1, 2, 3], vec![4, 5, 6]);
    let params = CollabParams {
      object_id: uuid::Uuid::new_v4().to_string(),
      collab_type: CollabType::Unknown,
      encoded_collab_v1: encode_collab.encode_to_bytes().unwrap(),
    };

    if i % 2 == 0 {
      // Simulate a failure scenario by using a non-existent user ID. This is designed to test the
      // robustness of the write operation. The objective is to ensure that valid records still get
      // written to disk despite the presence of some invalid entries.
      storage_queue
        .push(&user.workspace_id, &1, &params, WritePriority::Low)
        .await
        .unwrap();
    } else {
      storage_queue
        .push(&user.workspace_id, &user.uid, &params, WritePriority::Low)
        .await
        .unwrap();
      queries.push((params, encode_collab));
    }
  }

  // Allow some time for processing
  sleep(Duration::from_secs(20)).await;

  // Check that all items are processed correctly
  for (params, original_encode_collab) in queries {
    let query = QueryCollab {
      object_id: params.object_id.clone(),
      collab_type: params.collab_type,
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
