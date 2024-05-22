use crate::sql_test::util::{setup_db, test_create_user};
use collab_entity::CollabType;
use database::history::ops::{
  get_latest_snapshot, get_latest_snapshot_state, get_snapshot_meta_list, insert_history,
};
use sqlx::PgPool;
use tonic_proto::history::{SnapshotMetaPb, SnapshotStatePb};
use uuid::Uuid;

#[sqlx::test(migrations = false)]
async fn insert_snapshot_test(pool: PgPool) {
  setup_db(&pool).await.unwrap();

  let user_uuid = uuid::Uuid::new_v4();
  let name = user_uuid.to_string();
  let email = format!("{}@appflowy.io", name);
  let user = test_create_user(&pool, user_uuid, &email, &name)
    .await
    .unwrap();

  let workspace_id = Uuid::parse_str(&user.workspace_id).unwrap();
  let timestamp = chrono::Utc::now().timestamp();
  let object_id = uuid::Uuid::new_v4().to_string();
  let collab_type = CollabType::Document;

  let snapshots = vec![
    SnapshotMetaPb {
      oid: object_id.clone(),
      snapshot: vec![1, 2, 3],
      snapshot_version: 1,
      created_at: timestamp,
    },
    SnapshotMetaPb {
      oid: object_id.clone(),
      snapshot: vec![3, 4, 5],
      snapshot_version: 1,
      created_at: timestamp + 100,
    },
  ];

  let snapshot_state = SnapshotStatePb {
    oid: object_id.clone(),
    doc_state: vec![10, 11, 12],
    doc_state_version: 1,
    deps_snapshot_id: None,
  };

  insert_history(
    &workspace_id,
    &snapshot_state.oid,
    snapshot_state.doc_state,
    snapshot_state.doc_state_version,
    snapshot_state.deps_snapshot_id,
    collab_type.clone(),
    timestamp + 200,
    snapshots,
    pool.clone(),
  )
  .await
  .unwrap();

  let snapshot_list = get_snapshot_meta_list(&object_id, &collab_type, &pool)
    .await
    .unwrap();
  assert_eq!(snapshot_list.len(), 2);
  assert_eq!(snapshot_list[0].snapshot, vec![3, 4, 5]);
  assert_eq!(snapshot_list[1].snapshot, vec![1, 2, 3]);

  let snapshot_meta = get_latest_snapshot_state(&object_id, timestamp, &collab_type, &pool)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(snapshot_meta.doc_state, vec![10, 11, 12]);

  // Get the latest snapshot
  let snapshot = get_latest_snapshot(&object_id, &collab_type, &pool)
    .await
    .unwrap()
    .unwrap();
  assert_eq!(snapshot.history_state.unwrap().doc_state, vec![10, 11, 12]);
  assert_eq!(snapshot.snapshot_meta.unwrap().snapshot, vec![3, 4, 5]);
}
