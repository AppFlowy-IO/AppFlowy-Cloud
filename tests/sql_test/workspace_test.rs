use crate::sql_test::util::{create_test_user, generate_random_bytes, setup_db};

use collab_entity::CollabType;
use database::collab::{
  insert_into_af_collab, insert_into_af_collab_bulk_for_user, select_blob_from_af_collab,
  select_collab_meta_from_af_collab,
};
use database_entity::dto::CollabParams;
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn insert_collab_sql_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = create_test_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let mut object_ids = vec![];

  let data_sizes = vec![
    5120,    // 5 KB
    10240,   // 10 KB
    102400,  // 100 KB
    512000,  // 500 KB
    5120000, // 5 MB
  ];
  let start_time = std::time::Instant::now();
  for &data_size in &data_sizes {
    let encoded_collab_v1 = generate_random_bytes(data_size);
    let object_id = uuid::Uuid::new_v4();
    object_ids.push(object_id);
    let mut txn = pool.begin().await.unwrap();
    let params = CollabParams {
      object_id,
      collab_type: CollabType::Unknown,
      encoded_collab_v1: encoded_collab_v1.into(),
    };
    insert_into_af_collab(&mut txn, &user.uid, &user.workspace_id, &params)
      .await
      .unwrap();
    txn.commit().await.unwrap();
  }
  let duration = start_time.elapsed();
  println!("Insert time: {:?}", duration);

  for object_id in object_ids {
    let meta = select_collab_meta_from_af_collab(&pool, &object_id, &CollabType::Unknown)
      .await
      .unwrap()
      .unwrap();

    assert_eq!(meta.oid, object_id.to_string());
    assert_eq!(meta.workspace_id, user.workspace_id);
    assert!(meta.created_at.is_some());
    assert!(meta.deleted_at.is_none());
  }
}
#[sqlx::test(migrations = false)]
async fn insert_bulk_collab_sql_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = create_test_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let mut object_ids = vec![];
  let data_sizes = vec![
    5120,    // 5 KB
    10240,   // 10 KB
    102400,  // 100 KB
    512000,  // 500 KB
    5120000, // 5 MB
  ];
  let mut collab_params_list = vec![];
  let mut original_data_list = vec![]; // Store original data for validation

  // Prepare bulk insert data
  for &data_size in &data_sizes {
    let encoded_collab_v1 = generate_random_bytes(data_size);
    let object_id = uuid::Uuid::new_v4();
    object_ids.push(object_id);

    let params = CollabParams {
      object_id,
      collab_type: CollabType::Unknown,
      encoded_collab_v1: encoded_collab_v1.clone().into(), // Store the original data for validation
    };

    collab_params_list.push(params);
    original_data_list.push(encoded_collab_v1); // Keep track of original data
  }

  // Perform bulk insert
  let start_time = std::time::Instant::now(); // Start timing
  let mut txn = pool.begin().await.unwrap();
  insert_into_af_collab_bulk_for_user(&mut txn, &user.uid, user.workspace_id, &collab_params_list)
    .await
    .unwrap();
  txn.commit().await.unwrap();
  let duration = start_time.elapsed();
  println!("Bulk insert time: {:?}", duration);

  // Validate inserted data
  for (i, object_id) in object_ids.iter().enumerate() {
    let (_, inserted_data) = select_blob_from_af_collab(&pool, &CollabType::Unknown, object_id)
      .await
      .unwrap();

    // Ensure the inserted data matches the original data
    let original_data = &original_data_list[i];
    assert_eq!(
      inserted_data, *original_data,
      "Data mismatch for object_id: {}",
      object_id
    );
    println!(
      "Validated data size: {} bytes for object_id: {}",
      original_data.len(),
      object_id
    );
  }
}

#[sqlx::test(migrations = false)]
async fn test_bulk_insert_empty_collab_list(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let user = create_test_user(&pool, user_uuid, "test@appflowy.io", "test_user")
    .await
    .unwrap();

  let collab_params_list: Vec<CollabParams> = vec![]; // Empty list
  let mut txn = pool.begin().await.unwrap();
  let result = insert_into_af_collab_bulk_for_user(
    &mut txn,
    &user.uid,
    user.workspace_id,
    &collab_params_list,
  )
  .await;
  assert!(result.is_ok());
  txn.commit().await.unwrap();
}

#[sqlx::test(migrations = false)]
async fn test_bulk_insert_duplicate_oid_partition_key(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let user = create_test_user(&pool, user_uuid, "test@appflowy.io", "test_user")
    .await
    .unwrap();

  let object_id = uuid::Uuid::new_v4();
  let encoded_collab_v1 = generate_random_bytes(1024); // 1KB of random data

  // Two items with the same oid and partition_key
  let collab_params_list = vec![
    CollabParams {
      object_id,
      collab_type: CollabType::Unknown,
      encoded_collab_v1: encoded_collab_v1.clone().into(),
    },
    CollabParams {
      object_id, // Duplicate oid
      collab_type: CollabType::Unknown,
      encoded_collab_v1: generate_random_bytes(2048).into(), // Different data to test update
    },
  ];

  let mut txn = pool.begin().await.unwrap();
  insert_into_af_collab_bulk_for_user(&mut txn, &user.uid, user.workspace_id, &collab_params_list)
    .await
    .unwrap();
  txn.commit().await.unwrap();

  // Validate the data was updated, not duplicated
  let (_, data) = select_blob_from_af_collab(&pool, &CollabType::Unknown, &object_id)
    .await
    .unwrap();
  assert_eq!(data, encoded_collab_v1); // should equal the data that insert first time
}

#[sqlx::test(migrations = false)]
async fn test_batch_insert_comparison(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let user = create_test_user(&pool, user_uuid, "test@appflowy.io", "test_user")
    .await
    .unwrap();

  // Define the different test cases
  let row_sizes = vec![1024, 5 * 1024]; // 1KB and 5KB row sizes
  let total_rows = vec![500, 1000, 2000, 3000, 6000]; // Number of rows
  let chunk_sizes = vec![2000]; // Chunk size for batch inserts

  // Iterate over the different row sizes
  for row_size in row_sizes {
    // Iterate over the different total row counts
    for &total_row_count in &total_rows {
      // Generate data for the total row count
      let collab_params_list: Vec<CollabParams> = (0..total_row_count)
        .map(|_| CollabParams {
          object_id: uuid::Uuid::new_v4(),
          collab_type: CollabType::Unknown,
          encoded_collab_v1: generate_random_bytes(row_size).into(), // Generate random bytes for the given row size
        })
        .collect();

      // Group the results for readability
      println!("\n==============================");
      println!(
        "Row Size: {}KB, Total Rows: {}",
        row_size / 1024,
        total_row_count
      );

      // === Test Case 1: Insert all rows in one batch ===
      let start_time = std::time::Instant::now();
      let mut txn = pool.begin().await.unwrap();
      let result = insert_into_af_collab_bulk_for_user(
        &mut txn,
        &user.uid,
        user.workspace_id,
        &collab_params_list,
      )
      .await;

      assert!(result.is_ok()); // Ensure the insert doesn't fail
      txn.commit().await.unwrap();
      let total_time_single_batch = start_time.elapsed();
      println!(
        "Batch Insert - Time for inserting {} rows of size {}KB in one batch: {:?}",
        total_row_count,
        row_size / 1024,
        total_time_single_batch
      );

      // === Test Case 2: Insert rows in chunks ===
      for &chunk_size in &chunk_sizes {
        let mut total_time_multiple_batches = std::time::Duration::new(0, 0);
        for chunk in collab_params_list.chunks(chunk_size) {
          let start_time = std::time::Instant::now();
          let mut txn = pool.begin().await.unwrap();
          let result =
            insert_into_af_collab_bulk_for_user(&mut txn, &user.uid, user.workspace_id, chunk)
              .await;

          assert!(result.is_ok()); // Ensure the insert doesn't fail
          txn.commit().await.unwrap();
          total_time_multiple_batches += start_time.elapsed();
        }
        println!(
          "Chunked Insert - Time for inserting {} rows of size {}KB in {}-row chunks: {:?}",
          total_row_count,
          row_size / 1024,
          chunk_size,
          total_time_multiple_batches
        );
      }

      println!("==============================\n");
    }
  }
}
