use crate::pg_sql::util::{generate_random_bytes, setup_db, test_create_user};
use collab_entity::CollabType;
use database::collab::insert_into_af_collab;
use database_entity::dto::CollabParams;
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn insert_collab_sql_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);

  let user = test_create_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let data_sizes = vec![1024, 10240, 102400, 1024000]; // Example sizes: 1KB, 10KB, 100KB, 1MB
  for &data_size in &data_sizes {
    let encoded_collab_v1 = generate_random_bytes(data_size);
    let object_id = uuid::Uuid::new_v4().to_string();
    let start_time = std::time::Instant::now(); // Start timing
    let mut txn = pool.begin().await.unwrap();
    let params = CollabParams {
      object_id,
      collab_type: CollabType::Empty,
      encoded_collab_v1,
    };
    insert_into_af_collab(&mut txn, &user.uid, &user.workspace_id, &params)
      .await
      .unwrap();
    txn.commit().await.unwrap();
    let duration = start_time.elapsed(); // End timing
    println!(
      "Data size: {} bytes, Insert time: {:?}",
      data_size, duration
    );
  }
}
